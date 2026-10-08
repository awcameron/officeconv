//! Reading PPTX presentations.
//!
//! A `.pptx` is a zip archive. `ppt/presentation.xml` lists the slides in order, and each
//! slide (`ppt/slides/slide1.xml`) is a tree of shapes holding DrawingML text:
//!
//! ```xml
//! <p:sp>                                   <!-- shape -->
//!   <p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>   <!-- placeholder kind -->
//!   <p:txBody>
//!     <a:p>                                <!-- paragraph -->
//!       <a:pPr lvl="1"/>                   <!-- indent level -->
//!       <a:r><a:rPr b="1"/><a:t>Hello</a:t></a:r>   <!-- bold run -->
//!     </a:p>
//!   </p:txBody>
//! </p:sp>
//! ```
//!
//! Each slide becomes a `## Slide N: Title` heading followed by its text, with speaker notes
//! under `### Notes` and a horizontal rule between slides.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use quick_xml::events::BytesStart;

use crate::document::builder::{BlockBuilder, Paragraph, image_run};
use crate::document::{Block, ListKind, Merged, Run, RunStyle, append_run, display_size, is_blank};
use crate::error::Result;
use crate::images::{self, Images};
use crate::opc::{self, Limits, Open, Targets, XmlHandler, attr};

const PRESENTATION: &str = "ppt/presentation.xml";

/// What we read from one slide.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Slide {
    pub title: Vec<Run>,
    pub hidden: bool,
    pub body: Vec<Block>,
    pub notes: Vec<Block>,
}

/// Whether to include each slide's speaker notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notes {
    Include,
    Skip,
}

/// Reads a `.pptx` and returns it as blocks, slide by slide.
///
/// `images` says whether pictures are saved, embedded, or left out.
pub fn read_blocks<R: Read + Seek>(
    reader: R,
    notes: Notes,
    images: Images<'_>,
) -> Result<Vec<Block>> {
    read_blocks_with_limits(reader, notes, images, Limits::DEFAULT)
}

/// [`read_blocks`], decompressing at most `limits` instead of [`Limits::DEFAULT`]. The fuzz
/// targets use this to pass smaller limits.
pub fn read_blocks_with_limits<R: Read + Seek>(
    reader: R,
    notes: Notes,
    images: Images<'_>,
    limits: Limits,
) -> Result<Vec<Block>> {
    let mut archive = opc::Archive::with_limits(reader, limits)?;
    let presentation = archive.read_required_part(PRESENTATION)?;
    let relationships = match archive.read_part(&opc::rels_path(PRESENTATION))? {
        Some(xml) => opc::parse_relationships(&xml)?,
        None => HashMap::new(),
    };

    // A slide or notes part is read once, however many entries point at it. PowerPoint never
    // repeats one, and the size limits count bytes read, not work: listing one slide 500,000
    // times would otherwise turn a 1 MB file into 500 MB of output.
    let mut seen_slides = HashSet::new();
    let mut seen_notes = HashSet::new();

    let mut slides = Vec::new();
    for id in slide_ids(&presentation)? {
        let Some(relationship) = relationships.get(&id) else {
            continue;
        };
        let part = opc::resolve_target(PRESENTATION, &relationship.target);
        if !seen_slides.insert(part.clone()) {
            continue;
        }
        let Some(xml) = archive.read_part(&part)? else {
            continue;
        };

        let slide_relationships = match archive.read_part(&opc::rels_path(&part))? {
            Some(rels) => opc::parse_relationships(&rels)?,
            None => HashMap::new(),
        };
        let mut slide = parse_slide(&xml, &Targets::new(&slide_relationships, &part))?;

        // Speaker notes live in their own part, linked from the slide.
        let notes_part = slide_relationships
            .values()
            .find(|r| r.kind == "notesSlide")
            .map(|r| opc::resolve_target(&part, &r.target));
        if notes == Notes::Include
            && let Some(notes_part) = notes_part
            && seen_notes.insert(notes_part.clone())
            && let Some(notes_xml) = archive.read_part(&notes_part)?
        {
            slide.notes = parse_notes(&notes_xml)?;
        }

        slides.push(slide);
    }

    let mut blocks = slides_to_blocks(slides);
    images::link_images(&mut blocks, &mut archive, images)?;
    Ok(blocks)
}

/// The relationship IDs of the slides, in presentation order.
fn slide_ids(presentation: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    opc::visit_elements(presentation, |e| {
        if e.local_name().as_ref() != "sldId" {
            return;
        }
        // `<p:sldId id="256" r:id="rId7"/>` has two attributes with the local name `id`:
        // the unprefixed one is the slide's own number, `r:id` is the relationship we want.
        let relationship_id = e
            .attributes()
            .flatten()
            .find(|a| a.key.local_name().as_ref() == "id" && a.key.prefix().is_some())
            .map(|a| a.value.into_owned());
        if let Some(id) = relationship_id {
            ids.push(id);
        }
    })?;
    Ok(ids)
}

/// Lays out slides as Markdown blocks.
fn slides_to_blocks(slides: Vec<Slide>) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (i, slide) in slides.into_iter().enumerate() {
        if i > 0 {
            blocks.push(Block::Rule);
        }

        let mut heading = vec![Run::new(format!("Slide {}", i + 1), RunStyle::default())];
        if slide.hidden {
            append_run(&mut heading, Run::new(" (hidden)", RunStyle::default()));
        }
        if !is_blank(&slide.title) {
            append_run(&mut heading, Run::new(": ", RunStyle::default()));
            for run in slide.title {
                append_run(&mut heading, run);
            }
        }
        blocks.push(Block::Heading {
            level: 2,
            runs: heading,
        });

        blocks.extend(slide.body);
        if !slide.notes.is_empty() {
            blocks.push(Block::Heading {
                level: 3,
                runs: vec![Run::new("Notes", RunStyle::default())],
            });
            blocks.extend(slide.notes);
        }
    }
    blocks
}

/// Parses one slide part, using `targets` to resolve link and image IDs.
pub fn parse_slide(xml: &str, targets: &Targets) -> Result<Slide> {
    let mut parser = SlideParser::new(targets, false);
    opc::walk(xml, &mut parser)?;
    Ok(Slide {
        title: parser.title,
        hidden: parser.hidden,
        body: parser.builder.into_blocks(),
        notes: Vec::new(),
    })
}

/// Parses a notes part. Only the notes text box counts, not the slide image or slide number.
pub fn parse_notes(xml: &str) -> Result<Vec<Block>> {
    let targets = Targets::default();
    let mut parser = SlideParser::new(&targets, true);
    opc::walk(xml, &mut parser)?;
    Ok(parser.builder.into_blocks())
}

/// How a paragraph asked to be bulleted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Bullet {
    /// Nothing set on the paragraph: it inherits from the slide layout.
    #[default]
    Inherit,
    None,
    Symbol,
    Numbered,
}

#[derive(Default)]
struct Shape {
    /// The placeholder type (`title`, `body`, `subTitle`, ...) if this shape is a placeholder.
    placeholder: Option<String>,
    paragraphs: Vec<Paragraph<ParagraphProps>>,
}

impl Shape {
    fn is_title(&self) -> bool {
        matches!(self.placeholder.as_deref(), Some("title" | "ctrTitle"))
    }

    /// Content placeholders show bullets unless a paragraph turns them off.
    fn bullets_by_default(&self) -> bool {
        matches!(self.placeholder.as_deref(), Some("body" | "obj"))
    }
}

#[derive(Default)]
struct Picture {
    alt: Option<String>,
    part: Option<String>,
    link: Option<String>,
    /// From the picture's own `a:xfrm`. In a group that was resized after the picture was
    /// added, this is the size before the group was resized.
    size: Option<(u32, u32)>,
}

/// What a paragraph's properties (`a:pPr`) say about it.
#[derive(Default)]
struct ParagraphProps {
    level: u8,
    bullet: Bullet,
}

struct SlideParser<'a> {
    targets: &'a Targets,
    /// Reading speaker notes: keep only the notes body, and don't bullet it.
    notes: bool,
    title: Vec<Run>,
    hidden: bool,
    shape: Option<Shape>,
    /// The picture (`p:pic`) being read, if any.
    picture: Option<Picture>,
    builder: BlockBuilder<ParagraphProps>,
}

impl<'a> SlideParser<'a> {
    fn new(targets: &'a Targets, notes: bool) -> Self {
        SlideParser {
            targets,
            notes,
            picture: None,
            title: Vec::new(),
            hidden: false,
            shape: None,
            builder: BlockBuilder::new(),
        }
    }

    fn finish_paragraph(&mut self) {
        if let Some(paragraph) = self.builder.end_paragraph()
            && let Some(shape) = self.shape.as_mut()
        {
            shape.paragraphs.push(paragraph);
        }
    }

    /// A picture becomes its own paragraph, where it sits among the slide's shapes.
    fn finish_picture(&mut self) {
        let Some(picture) = self.picture.take() else {
            return;
        };
        let Some(part) = picture.part else {
            return;
        };

        let alt = picture.alt.unwrap_or_default();
        let run = image_run(part, alt, picture.size, picture.link);
        self.builder.push(Block::Paragraph(vec![run]));
    }

    fn finish_shape(&mut self) {
        let Some(shape) = self.shape.take() else {
            return;
        };

        if self.notes {
            if shape.placeholder.as_deref() == Some("body") {
                for paragraph in shape.paragraphs {
                    self.builder.push(Block::Paragraph(paragraph.runs));
                }
            }
            return;
        }

        if shape.is_title() {
            for paragraph in shape.paragraphs {
                if !self.title.is_empty() {
                    append_run(&mut self.title, Run::new(" ", RunStyle::default()));
                }
                for run in paragraph.runs {
                    append_run(&mut self.title, run);
                }
            }
            return;
        }

        let bullets_by_default = shape.bullets_by_default();
        for paragraph in shape.paragraphs {
            let kind = match paragraph.props.bullet {
                Bullet::None => None,
                Bullet::Symbol => Some(ListKind::Bullet),
                Bullet::Numbered => Some(ListKind::Numbered),
                Bullet::Inherit => bullets_by_default.then_some(ListKind::Bullet),
            };
            self.builder.push(match kind {
                Some(kind) => Block::ListItem {
                    kind,
                    level: paragraph.props.level,
                    runs: paragraph.runs,
                },
                None => Block::Paragraph(paragraph.runs),
            });
        }
    }
}

impl XmlHandler for SlideParser<'_> {
    // Some content is stored twice (a modern version and a fallback); keep one.
    const SKIP: &'static [&'static str] = &["Fallback"];

    fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open) {
        match e.local_name().as_ref() {
            "sld" => self.hidden = attr(e, "show").as_deref() == Some("0"),
            "sp" if !is_empty => self.shape = Some(Shape::default()),
            "pic" if !is_empty && !self.notes => self.picture = Some(Picture::default()),
            "cNvPr" => {
                if let Some(picture) = self.picture.as_mut() {
                    picture.alt = attr(e, "descr").or_else(|| attr(e, "title"));
                }
            }
            "blip" => {
                if let Some(picture) = self.picture.as_mut() {
                    picture.part =
                        attr(e, "embed").and_then(|id| self.targets.images.get(&id).cloned());
                }
            }
            // The picture's size is `a:ext` in its `a:xfrm`. Extensions (`a:extLst`) also have
            // `a:ext` elements, which aren't sizes.
            "ext" if open.inside("xfrm") => {
                if let Some(picture) = self.picture.as_mut() {
                    picture.size = display_size(attr(e, "cx"), attr(e, "cy"));
                }
            }
            // Clicking a picture can open a link.
            "hlinkClick" if self.picture.is_some() && !in_run(open) => {
                let link = attr(e, "id").and_then(|id| self.targets.links.get(&id).cloned());
                if let Some(picture) = self.picture.as_mut() {
                    picture.link = link;
                }
            }
            "ph" => {
                if let Some(shape) = self.shape.as_mut() {
                    // A placeholder with no type is a content placeholder.
                    shape.placeholder = Some(attr(e, "type").unwrap_or_else(|| "body".into()));
                }
            }
            "p" if !is_empty => self.builder.start_paragraph(),
            "pPr" => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.level = attr(e, "lvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
            }
            "buNone" | "buChar" | "buBlip" | "buAutoNum" => {
                if let Some(paragraph) = self.builder.paragraph() {
                    paragraph.bullet = match e.local_name().as_ref() {
                        "buNone" => Bullet::None,
                        "buAutoNum" => Bullet::Numbered,
                        _ => Bullet::Symbol,
                    };
                }
            }
            // A text field (slide number, date) holds text just like a run.
            "r" | "fld" if !is_empty => {
                self.builder.style = RunStyle::default();
                self.builder.link = None;
            }
            "rPr" if in_run(open) => {
                self.builder.style.bold = is_on(attr(e, "b"));
                self.builder.style.italic = is_on(attr(e, "i"));
            }
            "hlinkClick" if in_run(open) => {
                self.builder.link =
                    attr(e, "id").and_then(|id| self.targets.links.get(&id).cloned());
            }
            "br" => self.builder.text("\n"),
            "tbl" if !is_empty => {
                self.builder.start_table();
                if let Some(table) = self.builder.table() {
                    table.lists_merged_cells = true;
                }
            }
            // A merged cell keeps the cells it covers in the XML, each marked `hMerge` or
            // `vMerge`, so every row already has all its columns; only their text is hidden.
            // The cell the merge starts from says how far it goes.
            "tc" if !is_empty => {
                if let Some(table) = self.builder.table() {
                    let span = |name| attr(e, name).and_then(|v| v.parse().ok()).unwrap_or(1);
                    table.span = span("gridSpan");
                    table.row_span = span("rowSpan");
                    table.merged = match (is_on(attr(e, "hMerge")), is_on(attr(e, "vMerge"))) {
                        (false, false) => Merged::No,
                        (true, false) => Merged::Left,
                        (false, true) => Merged::Up,
                        (true, true) => Merged::Both,
                    };
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            "r" | "fld" => {
                // Line breaks sit between runs, so they mustn't inherit the last run's formatting.
                self.builder.style = RunStyle::default();
                self.builder.link = None;
            }
            "p" => self.finish_paragraph(),
            "sp" => self.finish_shape(),
            "pic" => self.finish_picture(),
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

/// Inside a run, or a text field (slide number, date), which holds text just like one.
fn in_run(open: &Open) -> bool {
    open.inside("r") || open.inside("fld")
}

/// DrawingML writes on/off attributes as `"1"`/`"0"` or `"true"`/`"false"`.
fn is_on(value: Option<String>) -> bool {
    matches!(value.as_deref(), Some("1" | "true"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::TableCell;

    const PLAIN: RunStyle = RunStyle {
        bold: false,
        italic: false,
    };

    /// Wraps shapes in the elements a slide part uses.
    fn slide_xml(shapes: &str) -> String {
        format!(
            r#"<p:sld xmlns:a="a" xmlns:p="p" xmlns:r="r"><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#
        )
    }

    /// A shape, optionally a placeholder of `ph_type` (`Some("")` means a placeholder with no type).
    fn shape(ph_type: Option<&str>, paragraphs: &str) -> String {
        let ph = match ph_type {
            Some("") => r#"<p:ph idx="1"/>"#.to_string(),
            Some(t) => format!(r#"<p:ph type="{t}"/>"#),
            None => String::new(),
        };
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="s"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp>"#
        )
    }

    fn para(text: &str) -> String {
        format!("<a:p><a:r><a:t>{text}</a:t></a:r></a:p>")
    }

    fn parse(shapes: &str) -> Slide {
        parse_slide(&slide_xml(shapes), &Targets::default()).unwrap()
    }

    fn runs(text: &str) -> Vec<Run> {
        vec![Run::new(text, PLAIN)]
    }

    #[test]
    fn reads_title_and_subtitle() {
        let slide = parse(&format!(
            "{}{}",
            shape(Some("ctrTitle"), &para("Quarterly Review")),
            shape(Some("subTitle"), &para("Q3 &amp; beyond")),
        ));
        assert_eq!(slide.title, runs("Quarterly Review"));
        assert_eq!(slide.body, [Block::Paragraph(runs("Q3 & beyond"))]);
    }

    #[test]
    fn content_placeholders_are_bulleted_and_text_boxes_are_not() {
        let slide = parse(&format!(
            "{}{}",
            shape(
                Some(""),
                &format!(
                    "{}{}{}",
                    para("Revenue up"),
                    r#"<a:p><a:pPr lvl="1"/><a:r><a:t>EMEA</a:t></a:r></a:p>"#,
                    r#"<a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>No bullet</a:t></a:r></a:p>"#,
                )
            ),
            shape(
                None,
                &format!(
                    "{}{}",
                    para("Source: finance"),
                    r#"<a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:t>Step</a:t></a:r></a:p>"#,
                )
            ),
        ));
        let item = |kind, level, text| Block::ListItem {
            kind,
            level,
            runs: runs(text),
        };
        assert_eq!(
            slide.body,
            [
                item(ListKind::Bullet, 0, "Revenue up"),
                item(ListKind::Bullet, 1, "EMEA"),
                Block::Paragraph(runs("No bullet")),
                Block::Paragraph(runs("Source: finance")),
                item(ListKind::Numbered, 0, "Step"),
            ]
        );
    }

    #[test]
    fn reads_formatting_links_and_breaks() {
        let targets = Targets {
            links: HashMap::from([("rId2".to_string(), "https://example.com".to_string())]),
            ..Targets::default()
        };
        let xml = slide_xml(&shape(
            None,
            r#"<a:p><a:r><a:rPr b="1"/><a:t>Bold</a:t></a:r><a:r><a:rPr lang="en-US" i="1"/><a:t>Italic</a:t></a:r><a:br/><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>link</a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/></a:rPr><a:t> next</a:t></a:r></a:p>"#,
        ));
        let slide = parse_slide(&xml, &targets).unwrap();

        let bold = RunStyle {
            bold: true,
            italic: false,
        };
        let italic = RunStyle {
            bold: false,
            italic: true,
        };
        assert_eq!(
            slide.body,
            [Block::Paragraph(vec![
                Run::new("Bold", bold),
                Run::new("Italic", italic),
                Run::new("\n", PLAIN),
                Run::new("link", PLAIN).linked("https://example.com"),
                Run::new(" next", PLAIN),
            ])]
        );
    }

    #[test]
    fn reads_tables() {
        let cell = |text: &str| {
            format!(
                "<a:tc><a:txBody><a:bodyPr/>{}</a:txBody><a:tcPr/></a:tc>",
                para(text)
            )
        };
        let table = format!(
            r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tblGrid/><a:tr h="1">{}{}</a:tr><a:tr h="1">{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
            cell("Region"),
            cell("Growth"),
            cell("EMEA"),
            cell("18%"),
        );
        let slide = parse(&table);
        assert_eq!(
            slide.body,
            [Block::Table(vec![
                vec![
                    TableCell::new(runs("Region")),
                    TableCell::new(runs("Growth"))
                ],
                vec![TableCell::new(runs("EMEA")), TableCell::new(runs("18%"))],
            ])]
        );
    }

    #[test]
    fn a_merged_cell_hides_the_cells_it_covers() {
        let table = merged_table(&[
            &[
                (r#" gridSpan="2""#, "Sales 2026"),
                (r#" hMerge="1""#, "hidden"),
            ],
            &[(r#" rowSpan="2""#, "Q1"), ("", "Q2")],
            &[(r#" vMerge="1""#, "hidden"), ("", "140")],
        ]);
        let merged = |text: &str, cols, rows| TableCell::Content {
            runs: runs(text),
            cols,
            rows,
        };
        assert_eq!(
            parse(&table).body,
            [Block::Table(vec![
                vec![merged("Sales 2026", 2, 1), TableCell::Covered],
                vec![merged("Q1", 1, 2), TableCell::new(runs("Q2"))],
                vec![TableCell::Covered, TableCell::new(runs("140"))],
            ])]
        );
    }

    #[test]
    fn a_span_larger_than_the_table_is_clamped_to_it() {
        let table = merged_table(&[
            &[
                (r#" gridSpan="2" rowSpan="4000000000""#, "Both"),
                (r#" hMerge="1""#, ""),
            ],
            &[(r#" hMerge="1" vMerge="1""#, ""), (r#" vMerge="1""#, "")],
        ]);
        assert_eq!(
            parse(&table).body,
            [Block::Table(vec![
                vec![
                    TableCell::Content {
                        runs: runs("Both"),
                        cols: 2,
                        rows: 2,
                    },
                    TableCell::Covered,
                ],
                vec![TableCell::Covered, TableCell::Covered],
            ])]
        );
    }

    /// A merge mark nothing claims, such as one past the span the merged cell declares, is an
    /// empty cell of its own.
    #[test]
    fn a_merge_never_passes_the_span_it_declares() {
        let table = merged_table(&[
            &[("", "One"), (r#" hMerge="1""#, "hidden")],
            &[(r#" vMerge="1""#, "hidden"), ("", "Two")],
        ]);
        let empty = || TableCell::new(Vec::new());
        assert_eq!(
            parse(&table).body,
            [Block::Table(vec![
                vec![TableCell::new(runs("One")), empty()],
                vec![empty(), TableCell::new(runs("Two"))],
            ])]
        );
    }

    /// A slide holding one table: each row is a list of cells' attributes and text.
    fn merged_table(rows: &[&[(&str, &str)]]) -> String {
        let rows: String = rows
            .iter()
            .map(|cells| {
                let cells: String = cells
                    .iter()
                    .map(|(attrs, text)| {
                        format!(
                            "<a:tc{attrs}><a:txBody><a:bodyPr/>{}</a:txBody><a:tcPr/></a:tc>",
                            para(text)
                        )
                    })
                    .collect();
                format!(r#"<a:tr h="1">{cells}</a:tr>"#)
            })
            .collect();
        format!(
            r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="1"/><a:gridCol w="1"/></a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
        )
    }

    /// PowerPoint never nests paragraphs, but a broken file can. The outer paragraph keeps its
    /// text, as in DOCX.
    #[test]
    fn a_nested_paragraph_keeps_the_text_around_it() {
        let slide = parse(&shape(
            None,
            &format!(
                r#"<a:p><a:r><a:t xml:space="preserve">First </a:t></a:r>{}<a:r><a:t>Last</a:t></a:r></a:p>"#,
                para("Inner")
            ),
        ));
        assert_eq!(
            slide.body,
            [
                Block::Paragraph(runs("Inner")),
                Block::Paragraph(runs("First Last")),
            ]
        );
    }

    /// A DrawingML table cell holds only paragraphs, but a broken file can nest a table in one.
    /// Its text goes into the outer cell, as in DOCX.
    #[test]
    fn a_nested_table_goes_into_the_outer_cell() {
        let cell =
            |text: &str| format!("<a:tc><a:txBody>{}</a:txBody><a:tcPr/></a:tc>", para(text));
        let table = format!(
            r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr h="1"><a:tc><a:txBody>{}</a:txBody><a:tbl><a:tr h="1">{}{}</a:tr></a:tbl><a:tcPr/></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
            para("Outer"),
            cell("x"),
            cell("y"),
        );
        assert_eq!(
            parse(&table).body,
            [Block::Table(vec![vec![TableCell::new(runs(
                "Outer\nx\ny"
            ))]])]
        );
    }

    #[test]
    fn reads_pictures_in_slide_order() {
        let targets = Targets {
            links: HashMap::from([("rId4".to_string(), "https://example.com".to_string())]),
            images: HashMap::from([("rId3".to_string(), "ppt/media/image1.png".to_string())]),
        };
        let picture = |descr: &str, extra: &str| {
            format!(
                r#"<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3" descr="{descr}">{extra}</p:cNvPr><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic>"#
            )
        };
        let xml = slide_xml(&format!(
            "{}{}{}",
            shape(None, &para("Before")),
            picture("Team photo", r#"<a:hlinkClick r:id="rId4"/>"#),
            picture("", ""),
        ));

        let slide = parse_slide(&xml, &targets).unwrap();
        assert_eq!(
            slide.body,
            [
                Block::Paragraph(runs("Before")),
                Block::Paragraph(vec![
                    Run::image("ppt/media/image1.png", "Team photo").linked("https://example.com")
                ]),
                Block::Paragraph(vec![Run::image("ppt/media/image1.png", "")]),
            ]
        );
    }

    #[test]
    fn reads_picture_sizes_from_their_xfrm() {
        let targets = Targets {
            images: HashMap::from([("rId3".to_string(), "ppt/media/image1.png".to_string())]),
            ..Targets::default()
        };
        let picture = |sp_pr: &str| {
            format!(
                r#"<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill><p:spPr>{sp_pr}</p:spPr></p:pic>"#
            )
        };
        let xml = slide_xml(&format!(
            "{}{}",
            picture(r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="3657600" cy="1828800"/></a:xfrm>"#),
            picture(concat!(
                r#"<a:xfrm><a:ext cx="914400" cy="914400"/></a:xfrm>"#,
                r#"<a:extLst><a:ext uri="{28A0092B-C50C-407E-A947-70E740481C1C}"/></a:extLst>"#
            )),
        ));

        let slide = parse_slide(&xml, &targets).unwrap();
        let sized = |cx, cy| {
            let mut run = Run::image("ppt/media/image1.png", "");
            run.image.as_mut().unwrap().size = Some((cx, cy));
            Block::Paragraph(vec![run])
        };
        assert_eq!(
            slide.body,
            [
                // 4 by 2 inches.
                sized(3_657_600, 1_828_800),
                // An extension's `a:ext` after the size isn't a size, so it doesn't replace it.
                sized(914_400, 914_400),
            ]
        );
    }

    #[test]
    fn notice_hidden_slides() {
        let xml = r#"<p:sld xmlns:p="p" show="0"><p:cSld><p:spTree/></p:cSld></p:sld>"#;
        assert!(parse_slide(xml, &Targets::default()).unwrap().hidden);
    }

    #[test]
    fn notes_keep_only_the_body_placeholder() {
        let xml = slide_xml(&format!(
            "{}{}{}",
            shape(Some("sldImg"), ""),
            shape(
                Some("body"),
                &format!("{}{}", para("Mention EMEA."), para("Then pause."))
            ),
            shape(Some("sldNum"), &para("2")),
        ));
        assert_eq!(
            parse_notes(&xml).unwrap(),
            [
                Block::Paragraph(runs("Mention EMEA.")),
                Block::Paragraph(runs("Then pause.")),
            ]
        );
    }

    #[test]
    fn lists_slide_ids_in_order() {
        let xml = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="257" r:id="rId9"/><p:sldId id="256" r:id="rId7"/></p:sldIdLst></p:presentation>"#;
        assert_eq!(slide_ids(xml).unwrap(), ["rId9", "rId7"]);
    }

    #[test]
    fn lays_out_slides_with_rules_and_notes() {
        let slides = vec![
            Slide {
                title: runs("Intro"),
                body: vec![Block::Paragraph(runs("Hi"))],
                notes: vec![Block::Paragraph(runs("Smile"))],
                ..Slide::default()
            },
            Slide {
                hidden: true,
                ..Slide::default()
            },
        ];
        let heading = |level, text: &str| Block::Heading {
            level,
            runs: runs(text),
        };
        assert_eq!(
            slides_to_blocks(slides),
            [
                heading(2, "Slide 1: Intro"),
                Block::Paragraph(runs("Hi")),
                heading(3, "Notes"),
                Block::Paragraph(runs("Smile")),
                Block::Rule,
                heading(2, "Slide 2 (hidden)"),
            ]
        );
    }
}
