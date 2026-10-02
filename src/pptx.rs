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

use std::collections::HashMap;
use std::path::Path;

use quick_xml::events::BytesStart;

use crate::document::{
    Block, ListKind, Run, RunStyle, TableBuilder, append_paragraph, append_run, is_blank,
};
use crate::error::Result;
use crate::opc::{self, XmlHandler, attr};

const PRESENTATION: &str = "ppt/presentation.xml";

/// What we read from one slide.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Slide {
    pub title: Vec<Run>,
    pub hidden: bool,
    pub body: Vec<Block>,
    pub notes: Vec<Block>,
}

/// Reads the `.pptx` at `path` and returns it as blocks, slide by slide.
pub fn read_blocks(path: &Path) -> Result<Vec<Block>> {
    let mut archive = opc::open(path)?;
    let presentation = opc::read_required_part(&mut archive, PRESENTATION)?;
    let relationships = match opc::read_part(&mut archive, &opc::rels_path(PRESENTATION))? {
        Some(xml) => opc::parse_relationships(&xml)?,
        None => HashMap::new(),
    };

    let mut slides = Vec::new();
    for id in slide_ids(&presentation)? {
        let Some(relationship) = relationships.get(&id) else {
            continue;
        };
        let part = opc::resolve_target(PRESENTATION, &relationship.target);
        let Some(xml) = opc::read_part(&mut archive, &part)? else {
            continue;
        };

        let slide_relationships = match opc::read_part(&mut archive, &opc::rels_path(&part))? {
            Some(rels) => opc::parse_relationships(&rels)?,
            None => HashMap::new(),
        };
        let links = opc::hyperlinks(&slide_relationships);
        let mut slide = parse_slide(&xml, &links)?;

        // Speaker notes live in their own part, linked from the slide.
        let notes_part = slide_relationships
            .values()
            .find(|r| r.kind == "notesSlide")
            .map(|r| opc::resolve_target(&part, &r.target));
        if let Some(notes_part) = notes_part
            && let Some(notes_xml) = opc::read_part(&mut archive, &notes_part)?
        {
            slide.notes = parse_notes(&notes_xml)?;
        }

        slides.push(slide);
    }

    Ok(slides_to_blocks(slides))
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

/// Parses one slide part, using `links` to resolve hyperlink IDs.
pub fn parse_slide(xml: &str, links: &HashMap<String, String>) -> Result<Slide> {
    let mut parser = SlideParser::new(links, false);
    opc::walk(xml, &mut parser)?;
    Ok(Slide {
        title: parser.title,
        hidden: parser.hidden,
        body: parser.blocks,
        notes: Vec::new(),
    })
}

/// Parses a notes part. Only the notes text box counts, not the slide image or slide number.
pub fn parse_notes(xml: &str) -> Result<Vec<Block>> {
    let links = HashMap::new();
    let mut parser = SlideParser::new(&links, true);
    opc::walk(xml, &mut parser)?;
    Ok(parser.blocks)
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
    paragraphs: Vec<Paragraph>,
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
struct Paragraph {
    level: u8,
    bullet: Bullet,
    runs: Vec<Run>,
}

struct SlideParser<'a> {
    links: &'a HashMap<String, String>,
    /// Reading speaker notes: keep only the notes body, and don't bullet it.
    notes: bool,
    title: Vec<Run>,
    hidden: bool,
    blocks: Vec<Block>,
    shape: Option<Shape>,
    paragraph: Option<Paragraph>,
    tables: Vec<TableBuilder>,
    style: RunStyle,
    link: Option<String>,
    in_run: bool,
    in_text: bool,
    skip_depth: usize,
}

impl<'a> SlideParser<'a> {
    fn new(links: &'a HashMap<String, String>, notes: bool) -> Self {
        SlideParser {
            links,
            notes,
            title: Vec::new(),
            hidden: false,
            blocks: Vec::new(),
            shape: None,
            paragraph: None,
            tables: Vec::new(),
            style: RunStyle::default(),
            link: None,
            in_run: false,
            in_text: false,
            skip_depth: 0,
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(paragraph) = self.paragraph.as_mut() {
            let mut run = Run::new(text, self.style);
            run.link = self.link.clone();
            append_run(&mut paragraph.runs, run);
        }
    }

    fn finish_paragraph(&mut self) {
        let Some(paragraph) = self.paragraph.take() else {
            return;
        };
        if is_blank(&paragraph.runs) {
            return;
        }

        if let Some(table) = self.tables.last_mut() {
            append_paragraph(&mut table.cell, paragraph.runs);
        } else if let Some(shape) = self.shape.as_mut() {
            shape.paragraphs.push(paragraph);
        }
    }

    fn finish_shape(&mut self) {
        let Some(shape) = self.shape.take() else {
            return;
        };

        if self.notes {
            if shape.placeholder.as_deref() == Some("body") {
                let paragraphs = shape.paragraphs.into_iter();
                self.blocks
                    .extend(paragraphs.map(|p| Block::Paragraph(p.runs)));
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
            let kind = match paragraph.bullet {
                Bullet::None => None,
                Bullet::Symbol => Some(ListKind::Bullet),
                Bullet::Numbered => Some(ListKind::Numbered),
                Bullet::Inherit => bullets_by_default.then_some(ListKind::Bullet),
            };
            self.blocks.push(match kind {
                Some(kind) => Block::ListItem {
                    kind,
                    level: paragraph.level,
                    runs: paragraph.runs,
                },
                None => Block::Paragraph(paragraph.runs),
            });
        }
    }
}

impl XmlHandler for SlideParser<'_> {
    fn start(&mut self, e: &BytesStart, is_empty: bool) {
        if self.skip_depth > 0 {
            if !is_empty {
                self.skip_depth += 1;
            }
            return;
        }

        match e.local_name().as_ref() {
            // Some content is stored twice (a modern version and a fallback); keep one.
            "Fallback" if !is_empty => self.skip_depth = 1,
            "sld" => self.hidden = attr(e, "show").as_deref() == Some("0"),
            "sp" if !is_empty => self.shape = Some(Shape::default()),
            "ph" => {
                if let Some(shape) = self.shape.as_mut() {
                    // A placeholder with no type is a content placeholder.
                    shape.placeholder = Some(attr(e, "type").unwrap_or_else(|| "body".into()));
                }
            }
            "p" if !is_empty => self.paragraph = Some(Paragraph::default()),
            "pPr" => {
                if let Some(paragraph) = self.paragraph.as_mut() {
                    paragraph.level = attr(e, "lvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
            }
            "buNone" | "buChar" | "buBlip" | "buAutoNum" => {
                if let Some(paragraph) = self.paragraph.as_mut() {
                    paragraph.bullet = match e.local_name().as_ref() {
                        "buNone" => Bullet::None,
                        "buAutoNum" => Bullet::Numbered,
                        _ => Bullet::Symbol,
                    };
                }
            }
            // A text field (slide number, date) holds text just like a run.
            "r" | "fld" if !is_empty => {
                self.in_run = true;
                self.style = RunStyle::default();
                self.link = None;
            }
            "rPr" if self.in_run => {
                self.style.bold = is_on(attr(e, "b"));
                self.style.italic = is_on(attr(e, "i"));
            }
            "hlinkClick" if self.in_run => {
                self.link = attr(e, "id").and_then(|id| self.links.get(&id).cloned());
            }
            "t" if !is_empty => self.in_text = true,
            "br" => self.push_text("\n"),
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
            "r" | "fld" => {
                // Line breaks sit between runs, so they mustn't inherit the last run's formatting.
                self.in_run = false;
                self.style = RunStyle::default();
                self.link = None;
            }
            "p" => self.finish_paragraph(),
            "sp" => self.finish_shape(),
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
                if let Some(table) = self.tables.pop()
                    && !table.rows.is_empty()
                {
                    self.blocks.push(Block::Table(table.rows));
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

/// DrawingML writes on/off attributes as `"1"`/`"0"` or `"true"`/`"false"`.
fn is_on(value: Option<String>) -> bool {
    matches!(value.as_deref(), Some("1" | "true"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        parse_slide(&slide_xml(shapes), &HashMap::new()).unwrap()
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
        let links = HashMap::from([("rId2".to_string(), "https://example.com".to_string())]);
        let xml = slide_xml(&shape(
            None,
            r#"<a:p><a:r><a:rPr b="1"/><a:t>Bold</a:t></a:r><a:r><a:rPr lang="en-US" i="1"/><a:t>Italic</a:t></a:r><a:br/><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>link</a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/></a:rPr><a:t> next</a:t></a:r></a:p>"#,
        ));
        let slide = parse_slide(&xml, &links).unwrap();

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
                vec![runs("Region"), runs("Growth")],
                vec![runs("EMEA"), runs("18%")],
            ])]
        );
    }

    #[test]
    fn notice_hidden_slides() {
        let xml = r#"<p:sld xmlns:p="p" show="0"><p:cSld><p:spTree/></p:cSld></p:sld>"#;
        assert!(parse_slide(xml, &HashMap::new()).unwrap().hidden);
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
