//! Unit tests for the DOCX reader. A child module of `docx`, so it can use private items.

use super::*;
use crate::document::ListKind;

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
fn reads_pictures_with_alt_text() {
    let mut package = Package::default();
    package
        .images
        .insert("rId7".into(), "word/media/image1.png".into());
    let blocks = parse_with(
        r#"<w:p><w:r><w:t xml:space="preserve">Chart: </w:t></w:r><w:r><w:drawing><wp:inline>
                 <wp:docPr id="1" name="Picture 1" descr="Sales by region"/>
                 <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
               </wp:inline></w:drawing></w:r></w:p>
               <w:p><w:r><w:pict><v:shape><v:imagedata r:id="rId7" o:title="Old style"/></v:shape></w:pict></w:r></w:p>
               <w:p><w:r><w:drawing><wp:docPr id="2" name="x"/><a:blip r:link="rId99"/></w:drawing></w:r></w:p>"#,
        &package,
    );
    assert_eq!(
        blocks,
        [
            Block::Paragraph(vec![
                Run::new("Chart: ", PLAIN),
                Run::image("word/media/image1.png", "Sales by region"),
            ]),
            Block::Paragraph(vec![Run::image("word/media/image1.png", "Old style")]),
        ]
    );
}

#[test]
fn reads_a_picture_grouped_with_a_text_box() {
    // The text box's runs sit inside the run that holds the drawing. The picture after them is
    // still in that outer run, so it isn't lost when the first inner run ends.
    let mut package = Package::default();
    package
        .images
        .insert("rId7".into(), "word/media/image1.png".into());
    let blocks = parse_with(
        r#"<w:p><w:r><w:drawing><wp:anchor>
                 <wp:docPr id="1" name="Group 1" descr="Team photo"/>
                 <a:graphic><a:graphicData><wpg:wgp>
                   <wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Caption</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp>
                   <pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill></pic:pic>
                 </wpg:wgp></a:graphicData></a:graphic>
               </wp:anchor></w:drawing></w:r></w:p>"#,
        &package,
    );
    assert_eq!(
        blocks,
        [
            Block::Paragraph(vec![Run::new("Caption", PLAIN)]),
            Block::Paragraph(vec![Run::image("word/media/image1.png", "Team photo")]),
        ]
    );
}

#[test]
fn maps_heading_styles() {
    assert_eq!(heading_level("Heading1"), Some(1));
    assert_eq!(heading_level("heading 3"), Some(3));
    assert_eq!(heading_level("Title"), Some(1));
    assert_eq!(heading_level("Heading9"), None);
    assert_eq!(heading_level("Normal"), None);
}

#[test]
fn a_merged_cell_fills_its_first_column_and_keeps_the_rest() {
    let cell = |span: u32, text: &str| {
        format!(
            r#"<w:tc><w:tcPr><w:gridSpan w:val="{span}"/></w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#
        )
    };
    let row = |cells: &[String]| format!("<w:tr>{}</w:tr>", cells.concat());
    let body = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid>{}{}{}</w:tbl>"#,
        row(&[cell(3, "Sales 2026")]),
        row(&[cell(1, "Q1"), cell(1, "Q2"), cell(1, "Q3")]),
        row(&[cell(2, "H1"), cell(1, "260")]),
    );

    let empty = Vec::new;
    assert_eq!(
        parse(&body),
        [Block::Table(vec![
            vec![text_block("Sales 2026"), empty(), empty()],
            vec![text_block("Q1"), text_block("Q2"), text_block("Q3")],
            vec![text_block("H1"), empty(), text_block("260")],
        ])]
    );
}

#[test]
fn a_merged_cell_never_spans_past_the_grid() {
    let body = r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:tblGridChange><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid></w:tblGridChange></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val="4000000000"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    assert_eq!(
        parse(body),
        [Block::Table(vec![vec![text_block("x"), Vec::new()]])]
    );
}
