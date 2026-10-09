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
//!
//! Footnotes and endnotes live in parts of their own, which the body refers to by ID
//! (`<w:footnoteReference w:id="2"/>`). They're read first, then numbered as the body refers to
//! them and added after it. So do headers and footers: the first section's default header and
//! footer come before the body, for a paged output to repeat.

pub mod package;

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::mem;

use quick_xml::events::BytesStart;

use crate::document::builder::{BlockBuilder, Paragraph};
use crate::document::{Align, Block, ImagePart, Merged, RunStyle, display_size};
use crate::error::Result;
use crate::opc::{self, Archive, Open, Targets, XmlHandler, attr};
use package::{NoteId, NoteKind, Package};

const DOCUMENT: &str = "word/document.xml";

/// Reads the body of the `.docx` in `archive` as blocks. Each picture names its part in the
/// package; [`images::resolve`](crate::images::resolve) deals with them.
pub fn read_blocks<R: Read + Seek>(archive: &mut Archive<R>) -> Result<Vec<Block<ImagePart>>> {
    let document = archive.read_required_part(DOCUMENT)?;

    let relationships = archive.relationships(DOCUMENT)?;
    let mut package = Package {
        targets: Targets::new(&relationships, DOCUMENT),
        ..Package::default()
    };
    if let Some(xml) = archive.read_part("word/numbering.xml")? {
        package.numbering = package::parse_numbering(&xml)?;
    }
    if let Some(xml) = archive.read_part("word/styles.xml")? {
        package.styles = package::parse_styles(&xml)?;
    }

    let mut notes = HashMap::new();
    for kind in [NoteKind::Footnote, NoteKind::Endnote] {
        let Some(part) = relationships
            .values()
            .find(|r| r.kind == kind.relationship() && !r.external)
            .map(|r| opc::resolve_target(DOCUMENT, &r.target))
        else {
            continue;
        };
        let Some(xml) = archive.read_part(&part)? else {
            continue;
        };
        // A note's links and pictures are listed in its part's own relationships.
        let targets = Targets::new(&archive.relationships(&part)?, &part);
        for (id, blocks) in parse_notes(&xml, &package, &targets)? {
            notes.insert((kind, id), blocks);
        }
    }
    package.notes = notes;

    // Read once each, however many sections refer to them.
    let mut blocks = Vec::new();
    let (header, footer) = first_section_parts(&document)?;
    for (id, kind) in [(header, "header"), (footer, "footer")] {
        let Some(part) = id
            .and_then(|id| relationships.get(&id))
            .filter(|r| r.kind == kind && !r.external)
            .map(|r| opc::resolve_target(DOCUMENT, &r.target))
        else {
            continue;
        };
        let Some(xml) = archive.read_part(&part)? else {
            continue;
        };
        let targets = Targets::new(&archive.relationships(&part)?, &part);
        let content = parse_page_furniture(&xml, &package, &targets)?;
        if !content.is_empty() {
            blocks.push(match kind {
                "header" => Block::Header(content),
                _ => Block::Footer(content),
            });
        }
    }

    blocks.extend(parse_document(&document, &package)?);
    Ok(blocks)
}

/// The relationship IDs of the first section's default header and footer, from the first
/// `w:sectPr` in `word/document.xml`. Each section's properties come at its end, so the first
/// ones found are the first section's.
fn first_section_parts(xml: &str) -> Result<(Option<String>, Option<String>)> {
    let (mut sections, mut header, mut footer) = (0, None, None);
    opc::visit_elements(xml, |e| {
        let name = e.local_name();
        let reference = match name.as_ref() {
            "sectPr" => {
                sections += 1;
                return;
            }
            "headerReference" => &mut header,
            "footerReference" => &mut footer,
            _ => return,
        };
        // A section can also have a header for its first page, and one for even pages.
        let default = attr(e, "type").is_none_or(|kind| kind == "default");
        if sections == 1 && default && reference.is_none() {
            *reference = attr(e, "id");
        }
    })?;
    Ok((header, footer))
}

/// Parses a header or footer part. Page numbers are left out: the number saved in the file is
/// the page Word last drew it on, not the page it's repeated on. Note references are dropped.
pub fn parse_page_furniture(
    xml: &str,
    package: &Package,
    targets: &Targets,
) -> Result<Vec<Block<ImagePart>>> {
    let mut parser = Parser::new(package, targets, None);
    parser.drop_page_numbers = true;
    opc::walk(xml, &mut parser)?;
    Ok(parser.builder.into_blocks())
}

/// Parses the contents of `word/document.xml`, looking up IDs in `package`. The notes the body
/// refers to follow it, each once, numbered from 1 in the order the body first refers to them.
pub fn parse_document(xml: &str, package: &Package) -> Result<Vec<Block<ImagePart>>> {
    let mut parser = Parser::new(package, &package.targets, Some(References::default()));
    opc::walk(xml, &mut parser)?;
    let references = parser.references.take().unwrap_or_default();
    let mut blocks = parser.builder.into_blocks();
    // Each note is copied once, however many times the body refers to it.
    blocks.extend(
        references
            .order
            .iter()
            .zip(1..)
            .map(|(id, number)| Block::Note {
                number,
                blocks: package.notes[id].clone(),
            }),
    );
    Ok(blocks)
}

/// Parses a footnotes or endnotes part, returning each note's blocks by its ID. The separators
/// Word keeps there, which draw the line above the notes on a page, aren't notes. A reference
/// inside a note is dropped, so notes never hold notes.
pub fn parse_notes(
    xml: &str,
    package: &Package,
    targets: &Targets,
) -> Result<Vec<(String, Vec<Block<ImagePart>>)>> {
    let mut parser = NotesParser {
        parser: Parser::new(package, targets, None),
        id: None,
        notes: Vec::new(),
    };
    opc::walk(xml, &mut parser)?;
    Ok(parser.notes)
}

/// The state we track while walking the XML.
///
/// `'p` is the lifetime of the borrowed [`Package`]: a `Parser` can't outlive it.
struct Parser<'p> {
    package: &'p Package,
    /// What the relationship IDs in the part being read point at.
    targets: &'p Targets,
    builder: BlockBuilder<ParagraphProps>,
    /// The notes the body has referred to, or `None` while reading the notes themselves.
    references: Option<References>,
    /// Whether to leave out the text of page number fields (`PAGE`, `NUMPAGES`).
    drop_page_numbers: bool,
    /// The instruction of the field being read (`PAGE \* Arabic`), between its `begin` and
    /// `separate` marks. Fields span runs, so this can't come from the open elements.
    field_instruction: Option<String>,
    /// Inside the text of a page number field that's being left out.
    in_page_number: bool,
    /// Alt text of the picture being read, from its `wp:docPr` description.
    image_alt: Option<String>,
    /// Display size of the picture being read, from its `wp:extent`.
    image_size: Option<(u32, u32)>,
}

/// The notes the body refers to, in the order it first refers to each.
#[derive(Default)]
struct References {
    order: Vec<NoteId>,
    numbers: HashMap<NoteId, usize>,
}

impl References {
    /// The number of the note `id`: the next one the first time it's referred to, and the same
    /// one after that.
    fn number(&mut self, id: NoteId) -> usize {
        let next = self.order.len() + 1;
        *self.numbers.entry(id).or_insert_with_key(|id| {
            self.order.push(id.clone());
            next
        })
    }
}

/// A [`Parser`] for a footnotes or endnotes part, which collects each note's blocks.
struct NotesParser<'p> {
    parser: Parser<'p>,
    /// The ID of the note being read, or `None` outside a note or in a separator.
    id: Option<String>,
    notes: Vec<(String, Vec<Block<ImagePart>>)>,
}

impl XmlHandler for NotesParser<'_> {
    const SKIP: &'static [&'static str] = Parser::SKIP;

    fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open) {
        if NoteKind::of_note(e.local_name().as_ref()).is_some() {
            // Separators have a `w:type`; an ordinary note has none, or `normal`.
            let ordinary = attr(e, "type").is_none_or(|kind| kind == "normal");
            self.id = attr(e, "id").filter(|_| ordinary && !is_empty);
            return;
        }
        self.parser.start(e, is_empty, open);
    }

    fn end(&mut self, name: &str) {
        if NoteKind::of_note(name).is_some() {
            let blocks = mem::replace(&mut self.parser.builder, BlockBuilder::new()).into_blocks();
            if let Some(id) = self.id.take() {
                self.notes.push((id, blocks));
            }
            return;
        }
        self.parser.end(name);
    }

    fn text(&mut self, text: &str, open: &Open) {
        self.parser.text(text, open);
    }
}

/// What a paragraph's properties (`w:pPr`) say about it.
#[derive(Default)]
struct ParagraphProps {
    style_id: Option<String>,
    num_id: Option<String>,
    list_level: u8,
    align: Option<Align>,
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
            // A table's `w:jc`, in `w:tblPr` or `w:trPr`, places the table, not its text.
            "jc" if open.inside("pPr") => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.align = attr(e, "val").as_deref().and_then(package::alignment);
                }
            }
            "hyperlink" if !is_empty => {
                // External links have an `r:id`; links to bookmarks inside the document don't.
                self.builder.link =
                    attr(e, "id").and_then(|id| self.targets.links.get(&id).cloned());
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
            name @ ("footnoteReference" | "endnoteReference") if in_run => {
                if let (Some(kind), Some(id), Some(references)) = (
                    NoteKind::of_reference(name),
                    attr(e, "id"),
                    self.references.as_mut(),
                ) {
                    // A reference to a note the file doesn't have is dropped.
                    let id = (kind, id);
                    if self.package.notes.contains_key(&id) {
                        self.builder.note(references.number(id));
                    }
                }
            }
            // A field is either one `w:fldSimple` element, or runs marked `begin`, then the
            // instruction, `separate`, the text Word last showed, and `end`.
            "fldSimple" if !is_empty => {
                self.in_page_number =
                    self.drop_page_numbers && attr(e, "instr").is_some_and(|i| is_page_number(&i));
            }
            "fldChar" if in_run => match attr(e, "fldCharType").as_deref() {
                Some("begin") => self.field_instruction = Some(String::new()),
                Some("separate") => {
                    let instruction = self.field_instruction.take().unwrap_or_default();
                    self.in_page_number = self.drop_page_numbers && is_page_number(&instruction);
                }
                Some("end") => {
                    self.field_instruction = None;
                    self.in_page_number = false;
                }
                _ => {}
            },
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
            "fldSimple" => self.in_page_number = false,
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
        if open.inside("instrText") {
            if let Some(instruction) = &mut self.field_instruction {
                instruction.push_str(text);
            }
        } else if open.inside("t") && !self.in_page_number {
            self.builder.text(text);
        }
    }
}

impl<'p> Parser<'p> {
    fn new(package: &'p Package, targets: &'p Targets, references: Option<References>) -> Self {
        Parser {
            package,
            targets,
            builder: BlockBuilder::new(),
            references,
            drop_page_numbers: false,
            field_instruction: None,
            in_page_number: false,
            image_alt: None,
            image_size: None,
        }
    }

    /// Adds the image with relationship ID `id`, if it's one stored in the document.
    fn push_image(&mut self, id: Option<String>, alt: String, size: Option<(u32, u32)>) {
        if let Some(part) = id.and_then(|id| self.targets.images.get(&id)) {
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
        // Alignment set on the paragraph wins over alignment from its style.
        let align = props
            .align
            .or_else(|| style.and_then(|s| s.align))
            .unwrap_or_default();
        if let Some(level) = heading {
            return Block::Heading { level, runs, align };
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
            None => Block::Paragraph { runs, align },
        }
    }
}

/// True if a field instruction (`PAGE \* MERGEFORMAT`) shows a page number or page count.
fn is_page_number(instruction: &str) -> bool {
    instruction.split_whitespace().next().is_some_and(|name| {
        ["PAGE", "NUMPAGES", "SECTIONPAGES"]
            .iter()
            .any(|field| name.eq_ignore_ascii_case(field))
    })
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
