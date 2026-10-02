//! Reading DOCX files.
//!
//! A `.docx` is a zip archive. The body text lives in `word/document.xml`:
//!
//! ```xml
//! <w:p>                                  <!-- paragraph -->
//!   <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
//!   <w:r>                                <!-- run: text with one formatting -->
//!     <w:rPr><w:b/></w:rPr>              <!-- bold -->
//!     <w:t>Hello</w:t>                   <!-- the text itself -->
//!   </w:r>
//! </w:p>
//! ```
//!
//! We stream through that XML one event at a time and build [`Block`]s.

pub mod markdown;
pub mod model;
pub mod package;

use std::fs::File;
use std::io::{Read, Seek};
use std::mem;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::Result;
use model::{Block, Cell, Run, RunStyle};
use package::Package;

/// Reads the `.docx` at `path` and returns its body as blocks.
pub fn read_blocks(path: &Path) -> Result<Vec<Block>> {
    let file = File::open(path).map_err(ZipError::from)?;
    let mut archive = ZipArchive::new(file)?;

    let document = read_part(&mut archive, "word/document.xml")?.ok_or(ZipError::FileNotFound)?;

    let mut package = Package::default();
    if let Some(xml) = read_part(&mut archive, "word/_rels/document.xml.rels")? {
        package.links = package::parse_relationships(&xml)?;
    }
    if let Some(xml) = read_part(&mut archive, "word/numbering.xml")? {
        package.numbering = package::parse_numbering(&xml)?;
    }
    if let Some(xml) = read_part(&mut archive, "word/styles.xml")? {
        package.styles = package::parse_styles(&xml)?;
    }

    parse_document(&document, &package)
}

/// Reads one file from the archive, or `None` if the document doesn't have it.
fn read_part<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Option<String>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut xml = String::new();
    entry.read_to_string(&mut xml).map_err(ZipError::from)?;
    Ok(Some(xml))
}

/// Parses the contents of `word/document.xml`, looking up IDs in `package`.
pub fn parse_document(xml: &str, package: &Package) -> Result<Vec<Block>> {
    let mut reader = Reader::from_str(xml);
    let mut parser = Parser::new(package);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(e) => parser.start(&e, false),
            Event::Empty(e) => parser.start(&e, true),
            Event::End(e) => parser.end(e.local_name().as_ref()),
            Event::Text(t) => parser.text(&t),
            Event::GeneralRef(r) => {
                // `&amp;` and `&#233;` arrive as their own events.
                let resolved = match r.resolve_char_ref()? {
                    Some(c) => c.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&r)
                        .unwrap_or_default()
                        .to_string(),
                };
                parser.text(&resolved);
            }
            _ => {}
        }
    }

    Ok(parser.blocks)
}

/// The state we track while walking the XML.
///
/// `'p` is the lifetime of the borrowed [`Package`]: a `Parser` can't outlive it.
struct Parser<'p> {
    package: &'p Package,
    blocks: Vec<Block>,
    /// Paragraphs we're inside. Usually 0 or 1, but text boxes nest paragraphs.
    paragraphs: Vec<ParagraphBuilder>,
    /// Tables we're inside. More than one means a table inside a table cell.
    tables: Vec<TableBuilder>,
    /// Formatting of the run we're inside.
    style: RunStyle,
    /// Target of the hyperlink we're inside, if any.
    link: Option<String>,
    in_run: bool,
    in_text: bool,
    /// While above 0, we're inside an element whose contents we ignore.
    skip_depth: usize,
}

#[derive(Default)]
struct ParagraphBuilder {
    style_id: Option<String>,
    num_id: Option<String>,
    list_level: u8,
    runs: Vec<Run>,
}

#[derive(Default)]
struct TableBuilder {
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Cell,
}

impl<'p> Parser<'p> {
    fn new(package: &'p Package) -> Self {
        Parser {
            package,
            blocks: Vec::new(),
            paragraphs: Vec::new(),
            tables: Vec::new(),
            style: RunStyle::default(),
            link: None,
            in_run: false,
            in_text: false,
            skip_depth: 0,
        }
    }

    fn start(&mut self, e: &BytesStart, is_empty: bool) {
        if self.skip_depth > 0 {
            if !is_empty {
                self.skip_depth += 1;
            }
            return;
        }

        match e.local_name().as_ref() {
            // Word stores some content twice (a modern version and a fallback), and tracked
            // changes keep the old formatting around. We only want the current version.
            "Fallback" | "pPrChange" | "rPrChange" if !is_empty => self.skip_depth = 1,
            "p" if !is_empty => self.paragraphs.push(ParagraphBuilder::default()),
            "pStyle" => {
                if let Some(paragraph) = self.paragraphs.last_mut() {
                    paragraph.style_id = attr(e, "val");
                }
            }
            "numId" => {
                if let Some(paragraph) = self.paragraphs.last_mut() {
                    paragraph.num_id = attr(e, "val");
                }
            }
            "ilvl" => {
                if let Some(paragraph) = self.paragraphs.last_mut() {
                    paragraph.list_level = attr(e, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
            }
            "hyperlink" if !is_empty => {
                // External links have an `r:id`; links to bookmarks inside the document don't.
                self.link = attr(e, "id").and_then(|id| self.package.links.get(&id).cloned());
            }
            "r" if !is_empty => {
                self.in_run = true;
                self.style = RunStyle::default();
            }
            "b" if self.in_run => self.style.bold = is_on(e),
            "i" if self.in_run => self.style.italic = is_on(e),
            "t" if !is_empty => self.in_text = true,
            "tab" if self.in_run => self.push_text(" "),
            "br" | "cr" if self.in_run => {
                // Page and column breaks don't mean anything in Markdown.
                if !matches!(attr(e, "type").as_deref(), Some("page" | "column")) {
                    self.push_text("\n");
                }
            }
            "tbl" if !is_empty => self.tables.push(TableBuilder::default()),
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            return;
        }

        match name {
            "t" => self.in_text = false,
            "r" => self.in_run = false,
            "hyperlink" => self.link = None,
            "p" => {
                if let Some(paragraph) = self.paragraphs.pop() {
                    self.finish_paragraph(paragraph);
                }
            }
            "tc" => {
                if let Some(table) = self.tables.last_mut() {
                    let cell = mem::take(&mut table.cell);
                    table.row.push(cell);
                }
            }
            "tr" => {
                if let Some(table) = self.tables.last_mut() {
                    let row = mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            "tbl" => {
                if let Some(table) = self.tables.pop() {
                    self.finish_table(table.rows);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_text && self.skip_depth == 0 {
            self.push_text(text);
        }
    }

    /// Adds text to the current paragraph, extending the last run if the formatting matches.
    ///
    /// Word often splits one word across several runs (spell-check, edits), so merging here
    /// keeps the Markdown clean: `**Hello**`, not `**Hel****lo**`.
    fn push_text(&mut self, text: &str) {
        let Some(paragraph) = self.paragraphs.last_mut() else {
            return;
        };

        let mut run = Run::new(text, self.style);
        run.link = self.link.clone();
        append_run(&mut paragraph.runs, run);
    }

    fn finish_paragraph(&mut self, paragraph: ParagraphBuilder) {
        if paragraph.runs.iter().all(|r| r.text.trim().is_empty()) {
            return;
        }

        // Inside a table, the paragraph becomes part of the current cell.
        if let Some(table) = self.tables.last_mut() {
            append_paragraph(&mut table.cell, paragraph.runs);
            return;
        }

        let block = self.classify(paragraph);
        self.blocks.push(block);
    }

    /// Decides whether a paragraph is a heading, a list item, or plain text.
    fn classify(&self, paragraph: ParagraphBuilder) -> Block {
        let style = paragraph
            .style_id
            .as_ref()
            .and_then(|id| self.package.styles.get(id));

        // Style IDs can be localized ("Kop1" in Dutch Word), but the style name is not.
        let heading = paragraph
            .style_id
            .as_deref()
            .and_then(heading_level)
            .or_else(|| style.and_then(|s| heading_level(&s.name)));
        if let Some(level) = heading {
            return Block::Heading {
                level,
                runs: paragraph.runs,
            };
        }

        // Numbering set on the paragraph wins over numbering from its style.
        let num_id = paragraph
            .num_id
            .as_deref()
            .or_else(|| style.and_then(|s| s.num_id.as_deref()));
        let kind = num_id.and_then(|id| self.package.numbering.kind(id, paragraph.list_level));
        match kind {
            Some(kind) => Block::ListItem {
                kind,
                level: paragraph.list_level,
                runs: paragraph.runs,
            },
            None => Block::Paragraph(paragraph.runs),
        }
    }

    fn finish_table(&mut self, rows: Vec<Vec<Cell>>) {
        if rows.is_empty() {
            return;
        }

        // Markdown tables can't nest, so an inner table's text goes into the outer cell.
        if let Some(outer) = self.tables.last_mut() {
            for cell in rows.into_iter().flatten() {
                append_paragraph(&mut outer.cell, cell);
            }
            return;
        }

        self.blocks.push(Block::Table(rows));
    }
}

/// Adds `run` to `runs`, merging it into the last run when the formatting matches.
fn append_run(runs: &mut Vec<Run>, run: Run) {
    match runs.last_mut() {
        Some(last) if last.same_format(&run) => last.text.push_str(&run.text),
        _ => runs.push(run),
    }
}

/// Adds a paragraph's runs to a table cell, on a new line if the cell already has text.
fn append_paragraph(cell: &mut Cell, runs: Vec<Run>) {
    if runs.iter().all(|r| r.text.trim().is_empty()) {
        return;
    }
    if !cell.is_empty() {
        append_run(cell, Run::new("\n", RunStyle::default()));
    }
    for run in runs {
        append_run(cell, run);
    }
}

/// Reads an attribute by its local name (`w:val` -> `"val"`).
fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| a.value.into_owned())
}

/// `<w:b/>` turns bold on, and so does `<w:b w:val="1"/>`, but `<w:b w:val="0"/>` turns it off.
fn is_on(e: &BytesStart) -> bool {
    !matches!(attr(e, "val").as_deref(), Some("0" | "false" | "off"))
}

/// Maps a style ID (`Heading1`) or style name (`heading 1`) to a Markdown heading level.
fn heading_level(style: &str) -> Option<u8> {
    let id = style.to_ascii_lowercase().replace(' ', "");
    if id == "title" {
        return Some(1);
    }

    let level: u8 = id.strip_prefix("heading")?.parse().ok()?;
    (1..=6).contains(&level).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::ListKind;

    const BOLD: RunStyle = RunStyle {
        bold: true,
        italic: false,
    };
    const PLAIN: RunStyle = RunStyle {
        bold: false,
        italic: false,
    };

    /// Wraps body XML in the `w:document` element Word uses.
    fn parse(body: &str) -> Vec<Block> {
        parse_with(body, &Package::default())
    }

    fn parse_with(body: &str, package: &Package) -> Vec<Block> {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        parse_document(&xml, package).unwrap()
    }

    #[test]
    fn reads_headings_and_paragraphs() {
        let blocks = parse(
            r#"<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Intro</w:t></w:r></w:p>
               <w:p><w:r><w:t>Body text.</w:t></w:r></w:p>"#,
        );
        assert_eq!(
            blocks,
            [
                Block::Heading {
                    level: 2,
                    runs: vec![Run::new("Intro", PLAIN)]
                },
                Block::Paragraph(vec![Run::new("Body text.", PLAIN)]),
            ]
        );
    }

    #[test]
    fn merges_runs_with_the_same_formatting() {
        let blocks = parse(
            r#"<w:p>
                 <w:r><w:t xml:space="preserve">Say </w:t></w:r>
                 <w:r><w:rPr><w:b/></w:rPr><w:t>Hel</w:t></w:r>
                 <w:r><w:rPr><w:b/></w:rPr><w:t>lo</w:t></w:r>
                 <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>!</w:t></w:r>
               </w:p>"#,
        );
        assert_eq!(
            blocks,
            [Block::Paragraph(vec![
                Run::new("Say ", PLAIN),
                Run::new("Hello", BOLD),
                Run::new("!", PLAIN),
            ])]
        );
    }

    #[test]
    fn resolves_entities_and_breaks() {
        let blocks = parse(
            r#"<w:p><w:r><w:t>Fish &amp; chips&#233;</w:t><w:br/><w:t>line two</w:t><w:br w:type="page"/></w:r></w:p>"#,
        );
        assert_eq!(
            blocks,
            [Block::Paragraph(vec![Run::new(
                "Fish & chipsé\nline two",
                PLAIN
            )])]
        );
    }

    #[test]
    fn skips_empty_paragraphs_and_deleted_text() {
        let blocks = parse(
            r#"<w:p/>
               <w:p><w:r><w:t>  </w:t></w:r></w:p>
               <w:p><w:del><w:r><w:delText>gone</w:delText></w:r></w:del><w:r><w:t>kept</w:t></w:r></w:p>"#,
        );
        assert_eq!(blocks, [Block::Paragraph(vec![Run::new("kept", PLAIN)])]);
    }

    #[test]
    fn ignores_fallback_copies() {
        let blocks = parse(
            r#"<w:p><w:r><mc:AlternateContent>
                 <mc:Choice><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></mc:Choice>
                 <mc:Fallback><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></mc:Fallback>
               </mc:AlternateContent></w:r></w:p>"#,
        );
        assert_eq!(blocks, [Block::Paragraph(vec![Run::new("boxed", PLAIN)])]);
    }

    /// A package with one link (`rId1`), a numbered list (`numId` 1) whose second level
    /// is bulleted, a "List Bullet" style, and a Dutch heading style.
    fn sample_package() -> Package {
        let mut package = Package::default();
        package
            .links
            .insert("rId1".into(), "https://example.com".into());
        package.numbering = package::parse_numbering(
            r#"<w:numbering xmlns:w="w">
                 <w:abstractNum w:abstractNumId="0">
                   <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
                   <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl>
                 </w:abstractNum>
                 <w:abstractNum w:abstractNumId="1">
                   <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
                 </w:abstractNum>
                 <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
                 <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
               </w:numbering>"#,
        )
        .unwrap();
        package.styles = package::parse_styles(
            r#"<w:styles xmlns:w="w">
                 <w:style w:styleId="ListBullet"><w:name w:val="List Bullet"/>
                   <w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style>
                 <w:style w:styleId="Kop2"><w:name w:val="heading 2"/></w:style>
               </w:styles>"#,
        )
        .unwrap();
        package
    }

    fn text_block(text: &str) -> Vec<Run> {
        vec![Run::new(text, PLAIN)]
    }

    #[test]
    fn reads_list_items_from_numbering_and_styles() {
        let blocks = parse_with(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
               <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Nested</w:t></w:r></w:p>
               <w:p><w:pPr><w:pStyle w:val="ListBullet"/></w:pPr><w:r><w:t>Styled</w:t></w:r></w:p>
               <w:p><w:pPr><w:pStyle w:val="ListBullet"/><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr><w:r><w:t>Unlisted</w:t></w:r></w:p>"#,
            &sample_package(),
        );
        assert_eq!(
            blocks,
            [
                Block::ListItem {
                    kind: ListKind::Numbered,
                    level: 0,
                    runs: text_block("First")
                },
                Block::ListItem {
                    kind: ListKind::Bullet,
                    level: 1,
                    runs: text_block("Nested")
                },
                Block::ListItem {
                    kind: ListKind::Bullet,
                    level: 0,
                    runs: text_block("Styled")
                },
                Block::Paragraph(text_block("Unlisted")),
            ]
        );
    }

    #[test]
    fn finds_headings_by_style_name() {
        let blocks = parse_with(
            r#"<w:p><w:pPr><w:pStyle w:val="Kop2"/></w:pPr><w:r><w:t>Inleiding</w:t></w:r></w:p>"#,
            &sample_package(),
        );
        assert_eq!(
            blocks,
            [Block::Heading {
                level: 2,
                runs: text_block("Inleiding")
            }]
        );
    }

    #[test]
    fn reads_hyperlinks() {
        let blocks = parse_with(
            r#"<w:p><w:r><w:t xml:space="preserve">Visit </w:t></w:r>
               <w:hyperlink r:id="rId1"><w:r><w:t>our site</w:t></w:r></w:hyperlink>
               <w:hyperlink w:anchor="_Toc1"><w:r><w:t>, section 2</w:t></w:r></w:hyperlink></w:p>"#,
            &sample_package(),
        );
        assert_eq!(
            blocks,
            [Block::Paragraph(vec![
                Run::new("Visit ", PLAIN),
                Run::new("our site", PLAIN).linked("https://example.com"),
                Run::new(", section 2", PLAIN),
            ])]
        );
    }

    #[test]
    fn reads_tables_and_flattens_nested_ones() {
        let cell = |xml: &str| format!("<w:tc><w:tcPr/>{xml}</w:tc>");
        let para = |text: &str| format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>");
        let inner_table = format!(
            "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
            cell(&para("x")),
            cell(&para("y"))
        );
        let body = format!(
            "<w:tbl><w:tblPr/><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>{}",
            cell(&para("Team")),
            cell(&para("Members")),
            cell(&para("One")),
            cell(&format!("{}{}{}", para("Bobby"), para("Don"), inner_table)),
            para("After"),
        );

        assert_eq!(
            parse(&body),
            [
                Block::Table(vec![
                    vec![text_block("Team"), text_block("Members")],
                    vec![text_block("One"), text_block("Bobby\nDon\nx\ny")],
                ]),
                Block::Paragraph(text_block("After")),
            ]
        );
    }

    #[test]
    fn ignores_tracked_formatting_changes() {
        let blocks = parse(
            r#"<w:p><w:r><w:rPr><w:b/><w:rPrChange><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>now bold</w:t></w:r></w:p>"#,
        );
        assert_eq!(blocks, [Block::Paragraph(vec![Run::new("now bold", BOLD)])]);
    }

    #[test]
    fn maps_heading_styles() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("heading 3"), Some(3));
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("Heading9"), None);
        assert_eq!(heading_level("Normal"), None);
    }
}
