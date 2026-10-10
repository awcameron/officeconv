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

use std::io::{Read, Seek};

use quick_xml::events::BytesStart;

use crate::document::{Block, ImagePart, Run};
use crate::drawingml;
use crate::error::Result;
use crate::opc::{self, Archive, Open, Targets, XmlHandler};

/// One picture on a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// The image run, naming the image's part in the package (`xl/media/image1.png`).
    pub image: Run<ImagePart>,
    /// The cell holding the picture's top-left corner, counting from 0.
    /// Pictures placed by position instead of by cell sort last.
    pub row: u32,
    pub col: u32,
}

/// The pictures on the worksheet stored in `sheet_part`, one paragraph per picture, in reading
/// order (top to bottom, then left to right). Each names its part in the package;
/// [`images::resolve`](crate::images::resolve) deals with them.
pub fn sheet_pictures<R: Read + Seek>(
    archive: &mut Archive<R>,
    sheet_part: &str,
) -> Result<Vec<Block<ImagePart>>> {
    Ok(read_pictures(archive, sheet_part)?
        .into_iter()
        .map(|picture| Block::paragraph(vec![picture.image]))
        .collect())
}

/// Finds the pictures on the worksheet stored in `sheet_part` (see
/// [`sheet_parts`](super::sheet_parts)), sorted into reading order.
pub fn read_pictures<R: Read + Seek>(
    archive: &mut Archive<R>,
    sheet_part: &str,
) -> Result<Vec<Picture>> {
    let mut pictures = Vec::new();
    // Several relationships can point at one drawing, which is read once.
    for drawing in archive.relationships(sheet_part)?.parts("drawing") {
        let targets = archive.relationships(&drawing)?.targets();
        if let Some(found) = archive.parse_part(&drawing, |xml| parse_drawing(xml, &targets))? {
            pictures.extend(found);
        }
    }

    // A stable sort keeps pictures in the same cell in the order they were drawn.
    pictures.sort_by_key(|p| (p.row, p.col));
    Ok(pictures)
}

/// Reads the pictures in a drawing part. `targets` are the drawing's relationships.
pub fn parse_drawing(xml: &str, targets: &Targets) -> Result<Vec<Picture>> {
    let mut parser = DrawingParser {
        targets,
        pictures: Vec::new(),
        anchor: None,
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

struct DrawingParser<'a> {
    targets: &'a Targets,
    pictures: Vec<Picture>,
    anchor: Option<Anchor>,
    picture: Option<drawingml::Picture>,
}

impl XmlHandler for DrawingParser<'_> {
    fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open) {
        if let Some(picture) = self.picture.as_mut() {
            picture.read(e, open, self.targets);
        }
        match e.local_name().as_ref() {
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" if !is_empty => {
                self.anchor = Some(Anchor::default());
            }
            "pic" if !is_empty => self.picture = Some(drawingml::Picture::default()),
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            "pic" => {
                let Some(image) = self.picture.take().and_then(|mut p| p.take_run()) else {
                    return;
                };
                let anchor = self.anchor.as_ref();
                self.pictures.push(Picture {
                    image,
                    row: anchor.and_then(|a| a.row).unwrap_or(u32::MAX),
                    col: anchor.and_then(|a| a.col).unwrap_or(u32::MAX),
                });
            }
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" => self.anchor = None,
            _ => {}
        }
    }

    fn text(&mut self, text: &str, open: &Open) {
        // `<xdr:from>` is the anchor's top-left corner (`<xdr:to>` is its bottom-right), and the
        // text of its `<xdr:row>` and `<xdr:col>` is the cell.
        let Some(anchor) = self.anchor.as_mut() else {
            return;
        };
        if !open.inside("from") {
            return;
        }
        let value = text.trim().parse().ok();
        match open.current() {
            Some("row") => anchor.row = value,
            Some("col") => anchor.col = value,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Targets holding only images, by relationship ID.
    fn images(entries: &[(&str, &str)]) -> Targets {
        Targets {
            images: entries
                .iter()
                .map(|(id, part)| (id.to_string(), part.to_string()))
                .collect(),
            ..Targets::default()
        }
    }

    fn image(part: &str, alt: &str) -> Run<ImagePart> {
        Run::image(ImagePart::new(part), alt)
    }

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
        let targets = images(&[
            ("rId1", "xl/media/image1.jpeg"),
            ("rId2", "xl/media/image2.png"),
        ]);
        let xml = drawing(&format!(
            "{}{}{}",
            anchor("twoCellAnchor", 4, 3, &pic("Company logo", "rId1")),
            anchor("oneCellAnchor", 1, 3, &pic("Sales chart", "rId2")),
            // A chart or shape, not a picture: skipped.
            anchor("twoCellAnchor", 0, 0, "<xdr:graphicFrame/>"),
        ));

        let pictures = parse_drawing(&xml, &targets).unwrap();
        assert_eq!(
            pictures,
            [
                Picture {
                    image: image("xl/media/image1.jpeg", "Company logo"),
                    row: 4,
                    col: 3
                },
                Picture {
                    image: image("xl/media/image2.png", "Sales chart"),
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
    fn workbook_with_drawing(anchors: &str) -> Archive<std::io::Cursor<Vec<u8>>> {
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

        let parts: Vec<(&str, &[u8])> = parts
            .iter()
            .map(|(name, contents)| (*name, contents.as_bytes()))
            .collect();
        Archive::open(opc::test_package(&parts)).unwrap()
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

        let pictures = read_pictures(&mut archive, "xl/worksheets/sheet1.xml").unwrap();
        let read: Vec<(&str, &str)> = pictures
            .iter()
            .map(|p| {
                let part = p.image.image.as_ref().map(|i| i.part.as_str());
                (p.image.text.as_str(), part.unwrap_or_default())
            })
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
            read_pictures(&mut archive, "xl/worksheets/missing.xml")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn maps_sheet_names_to_their_parts() {
        let mut archive = workbook_with_drawing("");
        assert_eq!(
            crate::xlsx::sheet_parts(&mut archive).unwrap(),
            HashMap::from([("Sales".to_string(), "xl/worksheets/sheet1.xml".to_string())])
        );
    }

    #[test]
    fn maps_only_sheets_that_are_worksheet_or_chart_parts() {
        let sheets: String = ["Sales", "Chart", "Web", "Styles"]
            .iter()
            .enumerate()
            .map(|(i, name)| format!(r#"<sheet name="{name}" r:id="rId{}"/>"#, i + 1))
            .collect();
        let workbook = format!(r#"<workbook xmlns:r="r"><sheets>{sheets}</sheets></workbook>"#);
        let rels = r#"<Relationships>
            <Relationship Id="rId1" Type="http://x/worksheet" Target="worksheets/sheet1.xml"/>
            <Relationship Id="rId2" Type="http://x/chartsheet" Target="chartsheets/sheet1.xml"/>
            <Relationship Id="rId3" Type="http://x/worksheet" Target="worksheets/sheet2.xml" TargetMode="External"/>
            <Relationship Id="rId4" Type="http://x/styles" Target="styles.xml"/>
        </Relationships>"#;
        let mut archive = Archive::open(opc::test_package(&[
            ("xl/workbook.xml", workbook.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ]))
        .unwrap();

        assert_eq!(
            crate::xlsx::sheet_parts(&mut archive).unwrap(),
            HashMap::from([
                ("Sales".to_string(), "xl/worksheets/sheet1.xml".to_string()),
                ("Chart".to_string(), "xl/chartsheets/sheet1.xml".to_string()),
            ])
        );
    }

    #[test]
    fn handles_unprefixed_xml_and_absolute_anchors() {
        // openpyxl writes the drawing namespace as the default, with no `xdr:` prefix.
        let targets = images(&[("rId1", "xl/media/image1.png")]);
        let xml = r#"<wsDr xmlns="xdr"><absoluteAnchor><pos x="0" y="0"/><pic><nvPicPr><cNvPr id="1" name="Image 1"/></nvPicPr><blipFill><a:blip xmlns:a="a" xmlns:r="r" r:embed="rId1"/></blipFill></pic></absoluteAnchor></wsDr>"#;

        let pictures = parse_drawing(xml, &targets).unwrap();
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0].image, image("xl/media/image1.png", ""));
        assert_eq!((pictures[0].row, pictures[0].col), (u32::MAX, u32::MAX));
    }
}
