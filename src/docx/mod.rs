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

use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::Result;
use model::{Block, Run, RunStyle};

/// Reads the `.docx` at `path` and returns its body as blocks.
pub fn read_blocks(path: &Path) -> Result<Vec<Block>> {
    let file = File::open(path).map_err(ZipError::from)?;
    let mut archive = ZipArchive::new(file)?;

    let mut xml = String::new();
    archive
        .by_name("word/document.xml")?
        .read_to_string(&mut xml)
        .map_err(ZipError::from)?;

    parse_document(&xml)
}

/// Parses the contents of `word/document.xml`.
pub fn parse_document(xml: &str) -> Result<Vec<Block>> {
    let mut reader = Reader::from_str(xml);
    let mut parser = Parser::default();

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
#[derive(Default)]
struct Parser {
    blocks: Vec<Block>,
    /// Paragraphs we're inside. Usually 0 or 1, but text boxes nest paragraphs.
    paragraphs: Vec<ParagraphBuilder>,
    /// Formatting of the run we're inside.
    style: RunStyle,
    in_run: bool,
    in_text: bool,
    /// While above 0, we're inside an element whose contents we ignore.
    skip_depth: usize,
}

#[derive(Default)]
struct ParagraphBuilder {
    style_id: Option<String>,
    runs: Vec<Run>,
}

impl Parser {
    fn start(&mut self, e: &BytesStart, is_empty: bool) {
        if self.skip_depth > 0 {
            if !is_empty {
                self.skip_depth += 1;
            }
            return;
        }

        match e.local_name().as_ref() {
            // Word stores some content twice (a modern version and a fallback); keep one.
            "Fallback" if !is_empty => self.skip_depth = 1,
            "p" if !is_empty => self.paragraphs.push(ParagraphBuilder::default()),
            "pStyle" => {
                if let Some(paragraph) = self.paragraphs.last_mut() {
                    paragraph.style_id = attr(e, "val");
                }
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
            "p" => {
                if let Some(paragraph) = self.paragraphs.pop() {
                    self.finish_paragraph(paragraph);
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
        let style = self.style;
        let Some(paragraph) = self.paragraphs.last_mut() else {
            return;
        };

        match paragraph.runs.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => paragraph.runs.push(Run::new(text, style)),
        }
    }

    fn finish_paragraph(&mut self, paragraph: ParagraphBuilder) {
        if paragraph.runs.iter().all(|r| r.text.trim().is_empty()) {
            return;
        }

        let level = paragraph.style_id.as_deref().and_then(heading_level);
        let block = match level {
            Some(level) => Block::Heading {
                level,
                runs: paragraph.runs,
            },
            None => Block::Paragraph(paragraph.runs),
        };
        self.blocks.push(block);
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

/// Maps Word's built-in style IDs to Markdown heading levels.
fn heading_level(style_id: &str) -> Option<u8> {
    let id = style_id.to_ascii_lowercase().replace(' ', "");
    if id == "title" {
        return Some(1);
    }

    let level: u8 = id.strip_prefix("heading")?.parse().ok()?;
    (1..=6).contains(&level).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        parse_document(&xml).unwrap()
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

    #[test]
    fn maps_heading_styles() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("heading 3"), Some(3));
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("Heading9"), None);
        assert_eq!(heading_level("Normal"), None);
    }
}
