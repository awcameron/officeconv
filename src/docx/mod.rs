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

use crate::document::builder::{BlockBuilder, Paragraph};
use crate::document::{Block, ImagePart, Merged, RunStyle, display_size};
use crate::error::Result;
use crate::opc::{self, Archive, Open, XmlHandler, attr};
use package::Package;

const DOCUMENT: &str = "word/document.xml";

/// Reads the body of the `.docx` in `archive` as blocks. Each picture names its part in the
/// package; [`images::resolve`](crate::images::resolve) deals with them.
pub fn read_blocks<R: Read + Seek>(archive: &mut Archive<R>) -> Result<Vec<Block<ImagePart>>> {
    let document = archive.read_required_part(DOCUMENT)?;

    let mut package = Package::default();
    if let Some(xml) = archive.read_part(&opc::rels_path(DOCUMENT))? {
        package.targets = opc::Targets::new(&opc::parse_relationships(&xml)?, DOCUMENT);
    }
    if let Some(xml) = archive.read_part("word/numbering.xml")? {
        package.numbering = package::parse_numbering(&xml)?;
    }
    if let Some(xml) = archive.read_part("word/styles.xml")? {
        package.styles = package::parse_styles(&xml)?;
    }

    parse_document(&document, &package)
}

/// Parses the contents of `word/document.xml`, looking up IDs in `package`.
pub fn parse_document(xml: &str, package: &Package) -> Result<Vec<Block<ImagePart>>> {
    let mut parser = Parser::new(package);
    opc::walk(xml, &mut parser)?;
    Ok(parser.builder.into_blocks())
}

/// The state we track while walking the XML.
///
/// `'p` is the lifetime of the borrowed [`Package`]: a `Parser` can't outlive it.
struct Parser<'p> {
    package: &'p Package,
    builder: BlockBuilder<ParagraphProps>,
    /// Alt text of the picture being read, from its `wp:docPr` description.
    image_alt: Option<String>,
    /// Display size of the picture being read, from its `wp:extent`.
    image_size: Option<(u32, u32)>,
}

/// What a paragraph's properties (`w:pPr`) say about it.
#[derive(Default)]
struct ParagraphProps {
    style_id: Option<String>,
    num_id: Option<String>,
    list_level: u8,
}

impl XmlHandler for Parser<'_> {
    // Word stores some content twice (a modern version and a fallback), and tracked changes
    // keep the old formatting and table grid around. We only want the current version.
    const SKIP: &'static [&'static str] = &[
        "Fallback",
        "pPrChange",
        "rPrChange",
        "tblGridChange",
        "tcPrChange",
    ];

    fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open) {
        let in_run = open.inside("r");
        match e.local_name().as_ref() {
            "p" if !is_empty => self.builder.start_paragraph(),
            "pStyle" => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.style_id = attr(e, "val");
                }
            }
            "numId" => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.num_id = attr(e, "val");
                }
            }
            "ilvl" => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.list_level = attr(e, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
            }
            "hyperlink" if !is_empty => {
                // External links have an `r:id`; links to bookmarks inside the document don't.
                self.builder.link =
                    attr(e, "id").and_then(|id| self.package.targets.links.get(&id).cloned());
            }
            "r" if !is_empty => self.builder.style = RunStyle::default(),
            "b" if in_run => self.builder.style.bold = is_on(e),
            "i" if in_run => self.builder.style.italic = is_on(e),
            // A picture: `wp:extent` carries its size and `wp:docPr` its alt text, then `a:blip`
            // points at the image. Each drawing starts afresh, so a size can't carry over.
            "inline" | "anchor" if in_run => {
                self.image_size = None;
            }
            "extent" if in_run && (open.inside("inline") || open.inside("anchor")) => {
                self.image_size = display_size(attr(e, "cx"), attr(e, "cy"));
            }
            "docPr" if in_run => {
                self.image_alt = attr(e, "descr").or_else(|| attr(e, "title"));
            }
            "blip" if in_run => {
                let alt = self.image_alt.take().unwrap_or_default();
                let size = self.image_size.take();
                self.push_image(attr(e, "embed"), alt, size);
            }
            // Older documents use VML: `<v:imagedata r:id="rId5" o:title="..."/>`.
            "imagedata" if in_run => {
                let alt = attr(e, "title").unwrap_or_default();
                self.push_image(attr(e, "id"), alt, None);
            }
            "tab" if in_run => self.builder.text(" "),
            "br" | "cr" if in_run => {
                // Page and column breaks don't mean anything in Markdown.
                if !matches!(attr(e, "type").as_deref(), Some("page" | "column")) {
                    self.builder.text("\n");
                }
            }
            "tbl" if !is_empty => self.builder.start_table(),
            "gridCol" => {
                if let Some(table) = self.builder.table() {
                    table.columns += 1;
                }
            }
            "gridSpan" if open.inside("tcPr") => {
                if let Some(table) = self.builder.table() {
                    table.span = attr(e, "val").and_then(|v| v.parse().ok()).unwrap_or(1);
                }
            }
            // A cell merged down several rows is a `restart` cell, then a `<w:vMerge/>` in the
            // same column of each row it covers.
            "vMerge" if open.inside("tcPr") => {
                if let Some(table) = self.builder.table()
                    && attr(e, "val").as_deref() != Some("restart")
                {
                    table.merged = Merged::Up;
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            "hyperlink" => self.builder.link = None,
            "p" => {
                if let Some(paragraph) = self.builder.end_paragraph() {
                    let block = self.classify(paragraph);
                    self.builder.push(block);
                }
            }
            "tc" => self.builder.end_cell(),
            "tr" => self.builder.end_row(),
            "tbl" => self.builder.end_table(),
            _ => {}
        }
    }

    fn text(&mut self, text: &str, open: &Open) {
        if open.inside("t") {
            self.builder.text(text);
        }
    }
}

impl<'p> Parser<'p> {
    fn new(package: &'p Package) -> Self {
        Parser {
            package,
            builder: BlockBuilder::new(),
            image_alt: None,
            image_size: None,
        }
    }

    /// Adds the image with relationship ID `id`, if it's one stored in the document.
    fn push_image(&mut self, id: Option<String>, alt: String, size: Option<(u32, u32)>) {
        if let Some(part) = id.and_then(|id| self.package.targets.images.get(&id)) {
            self.builder.image(part.clone(), alt, size);
        }
    }

    /// Decides whether a paragraph is a heading, a list item, or plain text.
    fn classify(&self, paragraph: Paragraph<ParagraphProps>) -> Block<ImagePart> {
        let Paragraph { props, runs } = paragraph;
        let style = props
            .style_id
            .as_ref()
            .and_then(|id| self.package.styles.get(id));

        // Style IDs can be localized ("Kop1" in Dutch Word), but the style name is not.
        let heading = props
            .style_id
            .as_deref()
            .and_then(heading_level)
            .or_else(|| style.and_then(|s| heading_level(&s.name)));
        if let Some(level) = heading {
            return Block::Heading { level, runs };
        }

        // Numbering set on the paragraph wins over numbering from its style.
        let num_id = props
            .num_id
            .as_deref()
            .or_else(|| style.and_then(|s| s.num_id.as_deref()));
        let kind = num_id.and_then(|id| self.package.numbering.kind(id, props.list_level));
        match kind {
            Some(kind) => Block::ListItem {
                kind,
                level: props.list_level,
                runs,
            },
            None => Block::Paragraph(runs),
        }
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
