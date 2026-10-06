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

pub mod package;

use std::io::{Read, Seek};

use quick_xml::events::BytesStart;

use crate::document::{
    Block, Cell, Run, RunStyle, TableBuilder, append_paragraph, append_run, is_blank,
};
use crate::error::Result;
use crate::images::{self, Images};
use crate::opc::{self, Limits, XmlHandler, attr};
use package::Package;

const DOCUMENT: &str = "word/document.xml";

/// Reads a `.docx` and returns its body as blocks.
///
/// `images` says whether pictures are saved, embedded, or left out.
pub fn read_blocks<R: Read + Seek>(reader: R, images: Images<'_>) -> Result<Vec<Block>> {
    read_blocks_with_limits(reader, images, Limits::DEFAULT)
}

/// [`read_blocks`], decompressing at most `limits`.
pub fn read_blocks_with_limits<R: Read + Seek>(
    reader: R,
    images: Images<'_>,
    limits: Limits,
) -> Result<Vec<Block>> {
    let mut archive = opc::Archive::with_limits(reader, limits)?;
    let document = archive.read_required_part(DOCUMENT)?;

    let mut package = Package::default();
    if let Some(xml) = archive.read_part(&opc::rels_path(DOCUMENT))? {
        let relationships = opc::parse_relationships(&xml)?;
        package.links = opc::hyperlinks(&relationships);
        package.images = opc::image_parts(&relationships, DOCUMENT);
    }
    if let Some(xml) = archive.read_part("word/numbering.xml")? {
        package.numbering = package::parse_numbering(&xml)?;
    }
    if let Some(xml) = archive.read_part("word/styles.xml")? {
        package.styles = package::parse_styles(&xml)?;
    }

    let mut blocks = parse_document(&document, &package)?;
    images::link_images(&mut blocks, &mut archive, images)?;
    Ok(blocks)
}

/// Parses the contents of `word/document.xml`, looking up IDs in `package`.
pub fn parse_document(xml: &str, package: &Package) -> Result<Vec<Block>> {
    let mut parser = Parser::new(package);
    opc::walk(xml, &mut parser)?;
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
    /// Alt text of the picture being read, from its `wp:docPr` description.
    image_alt: Option<String>,
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

impl XmlHandler for Parser<'_> {
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
            // A picture: `wp:docPr` carries its alt text, then `a:blip` points at the image.
            "docPr" if self.in_run => {
                self.image_alt = attr(e, "descr").or_else(|| attr(e, "title"));
            }
            "blip" if self.in_run => {
                let alt = self.image_alt.take().unwrap_or_default();
                self.push_image(attr(e, "embed"), alt);
            }
            // Older documents use VML: `<v:imagedata r:id="rId5" o:title="..."/>`.
            "imagedata" if self.in_run => {
                let alt = attr(e, "title").unwrap_or_default();
                self.push_image(attr(e, "id"), alt);
            }
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
                    table.end_cell();
                }
            }
            "tr" => {
                if let Some(table) = self.tables.last_mut() {
                    table.end_row();
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
            image_alt: None,
            in_run: false,
            in_text: false,
            skip_depth: 0,
        }
    }

    /// Adds text to the current paragraph, extending the last run if the formatting matches.
    fn push_text(&mut self, text: &str) {
        let Some(paragraph) = self.paragraphs.last_mut() else {
            return;
        };

        let mut run = Run::new(text, self.style);
        run.link = self.link.clone();
        append_run(&mut paragraph.runs, run);
    }

    /// Adds the image with relationship ID `id`, if it's one stored in the document.
    fn push_image(&mut self, id: Option<String>, alt: String) {
        let part = id.and_then(|id| self.package.images.get(&id));
        let (Some(part), Some(paragraph)) = (part, self.paragraphs.last_mut()) else {
            return;
        };

        let mut run = Run::image(part.clone(), alt);
        run.link = self.link.clone();
        append_run(&mut paragraph.runs, run);
    }

    fn finish_paragraph(&mut self, paragraph: ParagraphBuilder) {
        if is_blank(&paragraph.runs) {
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
mod tests;
