//! Finding the pictures placed on a worksheet.
//!
//! Pictures aren't stored in the sheet itself. The chain is:
//!
//! ```text
//! xl/workbook.xml            <sheet name="Sales" r:id="rId1"/>
//!   -> xl/_rels/workbook.xml.rels         rId1 -> worksheets/sheet1.xml
//!   -> xl/worksheets/_rels/sheet1.xml.rels   (a "drawing") -> ../drawings/drawing1.xml
//!   -> xl/drawings/drawing1.xml           <xdr:pic> ... <a:blip r:embed="rId2"/>
//!   -> xl/drawings/_rels/drawing1.xml.rels   rId2 -> ../media/image1.png
//! ```
//!
//! Each picture in the drawing sits in an anchor that says which cell its top-left corner is in.

use std::collections::HashMap;
use std::io::{Read, Seek};

use quick_xml::events::BytesStart;
use zip::ZipArchive;

use crate::document::{Block, Run};
use crate::error::Result;
use crate::images::{self, ImageExport};
use crate::opc::{self, XmlHandler, attr};

const WORKBOOK: &str = "xl/workbook.xml";

/// One picture on a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// The image's part in the package (`xl/media/image1.png`).
    pub part: String,
    pub alt: String,
    /// The cell holding the picture's top-left corner, counting from 0.
    /// Pictures placed by position instead of by cell sort last.
    pub row: u32,
    pub col: u32,
}

/// Saves the pictures on `sheet` and returns one Markdown paragraph per picture, in reading
/// order (top to bottom, then left to right).
pub fn export_sheet_pictures<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    sheet: &str,
    export: &mut ImageExport,
) -> Result<Vec<Block>> {
    let mut blocks: Vec<Block> = read_pictures(archive, sheet)?
        .into_iter()
        .map(|picture| Block::Paragraph(vec![Run::image(picture.part, picture.alt)]))
        .collect();
    images::link_images(&mut blocks, archive, Some(export))?;
    Ok(blocks)
}

/// Finds the pictures on the sheet called `sheet`, sorted into reading order.
pub fn read_pictures<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    sheet: &str,
) -> Result<Vec<Picture>> {
    let Some(sheet_part) = sheet_part(archive, sheet)? else {
        return Ok(Vec::new());
    };

    let mut pictures = Vec::new();
    for drawing in related_parts(archive, &sheet_part, "drawing")? {
        let Some(xml) = opc::read_part(archive, &drawing)? else {
            continue;
        };
        let images = match opc::read_part(archive, &opc::rels_path(&drawing))? {
            Some(rels) => opc::image_parts(&opc::parse_relationships(&rels)?, &drawing),
            None => HashMap::new(),
        };
        pictures.extend(parse_drawing(&xml, &images)?);
    }

    // A stable sort keeps pictures in the same cell in the order they were drawn.
    pictures.sort_by_key(|p| (p.row, p.col));
    Ok(pictures)
}

/// The worksheet part for the sheet called `name`, such as `xl/worksheets/sheet1.xml`.
fn sheet_part<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Option<String>> {
    let Some(workbook) = opc::read_part(archive, WORKBOOK)? else {
        return Ok(None);
    };

    let mut relationship_id = None;
    opc::visit_elements(&workbook, |e| {
        if e.local_name().as_ref() == "sheet" && attr(e, "name").as_deref() == Some(name) {
            relationship_id = attr(e, "id");
        }
    })?;
    let Some(id) = relationship_id else {
        return Ok(None);
    };

    let relationships = match opc::read_part(archive, &opc::rels_path(WORKBOOK))? {
        Some(xml) => opc::parse_relationships(&xml)?,
        None => return Ok(None),
    };
    Ok(relationships
        .get(&id)
        .map(|r| opc::resolve_target(WORKBOOK, &r.target)))
}

/// Parts that `part` links to with relationships of the given kind.
fn related_parts<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    part: &str,
    kind: &str,
) -> Result<Vec<String>> {
    let Some(xml) = opc::read_part(archive, &opc::rels_path(part))? else {
        return Ok(Vec::new());
    };
    let mut parts: Vec<String> = opc::parse_relationships(&xml)?
        .into_values()
        .filter(|r| r.kind == kind && !r.external)
        .map(|r| opc::resolve_target(part, &r.target))
        .collect();
    // HashMap order is random; sort so output doesn't change from run to run.
    parts.sort();
    Ok(parts)
}

/// Reads the pictures in a drawing part. `images` maps relationship IDs to image parts.
pub fn parse_drawing(xml: &str, images: &HashMap<String, String>) -> Result<Vec<Picture>> {
    let mut parser = DrawingParser {
        images,
        pictures: Vec::new(),
        anchor: None,
        in_from: false,
        reading: None,
        picture: None,
    };
    opc::walk(xml, &mut parser)?;
    Ok(parser.pictures)
}

/// Where an anchor's top-left corner is.
#[derive(Default)]
struct Anchor {
    row: Option<u32>,
    col: Option<u32>,
}

#[derive(Default)]
struct PictureBuilder {
    alt: Option<String>,
    part: Option<String>,
}

#[derive(Clone, Copy)]
enum Coordinate {
    Row,
    Col,
}

struct DrawingParser<'a> {
    images: &'a HashMap<String, String>,
    pictures: Vec<Picture>,
    anchor: Option<Anchor>,
    /// Inside `<xdr:from>`, the anchor's top-left corner (not `<xdr:to>`, its bottom-right).
    in_from: bool,
    /// Inside `<xdr:row>` or `<xdr:col>`, whose text is the number.
    reading: Option<Coordinate>,
    picture: Option<PictureBuilder>,
}

impl XmlHandler for DrawingParser<'_> {
    fn start(&mut self, e: &BytesStart, is_empty: bool) {
        match e.local_name().as_ref() {
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" if !is_empty => {
                self.anchor = Some(Anchor::default());
            }
            "from" if !is_empty => self.in_from = true,
            "row" if self.in_from && !is_empty => self.reading = Some(Coordinate::Row),
            "col" if self.in_from && !is_empty => self.reading = Some(Coordinate::Col),
            "pic" if !is_empty => self.picture = Some(PictureBuilder::default()),
            "cNvPr" => {
                if let Some(picture) = self.picture.as_mut() {
                    picture.alt = attr(e, "descr").or_else(|| attr(e, "title"));
                }
            }
            "blip" => {
                let part = attr(e, "embed").and_then(|id| self.images.get(&id).cloned());
                if let Some(picture) = self.picture.as_mut() {
                    picture.part = part;
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            "row" | "col" => self.reading = None,
            "from" => self.in_from = false,
            "pic" => {
                let Some(PictureBuilder {
                    alt,
                    part: Some(part),
                }) = self.picture.take()
                else {
                    return;
                };
                let anchor = self.anchor.as_ref();
                self.pictures.push(Picture {
                    part,
                    alt: alt.unwrap_or_default(),
                    row: anchor.and_then(|a| a.row).unwrap_or(u32::MAX),
                    col: anchor.and_then(|a| a.col).unwrap_or(u32::MAX),
                });
            }
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" => self.anchor = None,
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        let (Some(coordinate), Some(anchor)) = (self.reading, self.anchor.as_mut()) else {
            return;
        };
        let value = text.trim().parse().ok();
        match coordinate {
            Coordinate::Row => anchor.row = value,
            Coordinate::Col => anchor.col = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(kind: &str, row: u32, col: u32, picture: &str) -> String {
        format!(
            "<xdr:{kind}><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
             <xdr:to><xdr:col>99</xdr:col><xdr:row>99</xdr:row></xdr:to>{picture}<xdr:clientData/></xdr:{kind}>"
        )
    }

    fn pic(descr: &str, embed: &str) -> String {
        format!(
            r#"<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="Picture 1" descr="{descr}"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="{embed}"/></xdr:blipFill></xdr:pic>"#
        )
    }

    fn drawing(anchors: &str) -> String {
        format!(r#"<xdr:wsDr xmlns:xdr="xdr" xmlns:a="a" xmlns:r="r">{anchors}</xdr:wsDr>"#)
    }

    #[test]
    fn reads_pictures_with_their_top_left_cell() {
        let images = HashMap::from([
            ("rId1".to_string(), "xl/media/image1.jpeg".to_string()),
            ("rId2".to_string(), "xl/media/image2.png".to_string()),
        ]);
        let xml = drawing(&format!(
            "{}{}{}",
            anchor("twoCellAnchor", 4, 3, &pic("Company logo", "rId1")),
            anchor("oneCellAnchor", 1, 3, &pic("Sales chart", "rId2")),
            // A chart or shape, not a picture: skipped.
            anchor("twoCellAnchor", 0, 0, "<xdr:graphicFrame/>"),
        ));

        let pictures = parse_drawing(&xml, &images).unwrap();
        assert_eq!(
            pictures,
            [
                Picture {
                    part: "xl/media/image1.jpeg".into(),
                    alt: "Company logo".into(),
                    row: 4,
                    col: 3
                },
                Picture {
                    part: "xl/media/image2.png".into(),
                    alt: "Sales chart".into(),
                    row: 1,
                    col: 3
                },
            ]
        );
    }

    /// A one-sheet workbook package, built in memory, whose drawing holds `anchors`.
    ///
    /// Spreadsheet libraries tend to write pictures already in position order, which would let
    /// a missing sort go unnoticed, so this writes the drawing XML by hand.
    fn workbook_with_drawing(anchors: &str) -> ZipArchive<std::io::Cursor<Vec<u8>>> {
        use std::io::Write;

        let rels = |entries: &[(&str, &str, &str)]| {
            let body: String = entries
                .iter()
                .map(|(id, kind, target)| {
                    format!(r#"<Relationship Id="{id}" Type="http://x/{kind}" Target="{target}"/>"#)
                })
                .collect();
            format!("<Relationships>{body}</Relationships>")
        };
        let parts = [
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="r"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string(),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                rels(&[("rId1", "worksheet", "worksheets/sheet1.xml")]),
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                rels(&[("rId1", "drawing", "../drawings/drawing1.xml")]),
            ),
            ("xl/drawings/drawing1.xml", drawing(anchors)),
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                rels(&[
                    ("rId1", "image", "../media/logo.png"),
                    ("rId2", "image", "../media/chart.png"),
                    ("rId3", "image", "../media/key.png"),
                ]),
            ),
        ];

        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, contents) in parts {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        ZipArchive::new(writer.finish().unwrap()).unwrap()
    }

    #[test]
    fn sorts_pictures_into_reading_order() {
        // Drawn in the order logo (D5), chart (D2), key (A2).
        let mut archive = workbook_with_drawing(&format!(
            "{}{}{}",
            anchor("twoCellAnchor", 4, 3, &pic("Company logo", "rId1")),
            anchor("twoCellAnchor", 1, 3, &pic("Sales chart", "rId2")),
            anchor("oneCellAnchor", 1, 0, &pic("Key", "rId3")),
        ));

        let pictures = read_pictures(&mut archive, "Sales").unwrap();
        let read: Vec<(&str, &str)> = pictures
            .iter()
            .map(|p| (p.alt.as_str(), p.part.as_str()))
            .collect();
        // Top to bottom, then left to right: A2, D2, D5.
        assert_eq!(
            read,
            [
                ("Key", "xl/media/key.png"),
                ("Sales chart", "xl/media/chart.png"),
                ("Company logo", "xl/media/logo.png"),
            ]
        );
        assert!(
            read_pictures(&mut archive, "No such sheet")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn handles_unprefixed_xml_and_absolute_anchors() {
        // openpyxl writes the drawing namespace as the default, with no `xdr:` prefix.
        let images = HashMap::from([("rId1".to_string(), "xl/media/image1.png".to_string())]);
        let xml = r#"<wsDr xmlns="xdr"><absoluteAnchor><pos x="0" y="0"/><pic><nvPicPr><cNvPr id="1" name="Image 1"/></nvPicPr><blipFill><a:blip xmlns:a="a" xmlns:r="r" r:embed="rId1"/></blipFill></pic></absoluteAnchor></wsDr>"#;

        let pictures = parse_drawing(xml, &images).unwrap();
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].alt, "");
        assert_eq!((pictures[0].row, pictures[0].col), (u32::MAX, u32::MAX));
    }
}
