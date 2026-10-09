//! Unit tests for the DOCX reader. A child module of `docx`, so it can use private items.

use super::*;
use crate::document::{Field, ImagePart, ListKind, Run, TableCell};

const BOLD: RunStyle = RunStyle {
    bold: true,
    italic: false,
};
const PLAIN: RunStyle = RunStyle {
    bold: false,
    italic: false,
};

/// Wraps body XML in the `w:document` element Word uses.
fn parse(body: &str) -> Vec<Block<ImagePart>> {
    parse_with(body, &Package::default())
}

fn parse_with(body: &str, package: &Package) -> Vec<Block<ImagePart>> {
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
            Block::heading(2, vec![Run::new("Intro", PLAIN)]),
            Block::paragraph(vec![Run::new("Body text.", PLAIN)]),
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
        [Block::paragraph(vec![
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
        [Block::paragraph(vec![Run::new(
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
    assert_eq!(blocks, [Block::paragraph(vec![Run::new("kept", PLAIN)])]);
}

#[test]
fn ignores_fallback_copies() {
    let blocks = parse(
        r#"<w:p><w:r><mc:AlternateContent>
                 <mc:Choice><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></mc:Choice>
                 <mc:Fallback><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></mc:Fallback>
               </mc:AlternateContent></w:r></w:p>"#,
    );
    assert_eq!(blocks, [Block::paragraph(vec![Run::new("boxed", PLAIN)])]);
}

/// A package with one link (`rId1`), a numbered list (`numId` 1) whose second level
/// is bulleted, a "List Bullet" style, and a Dutch heading style.
fn sample_package() -> Package {
    let mut package = Package::default();
    package
        .targets
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

fn text_block(text: &str) -> Vec<Run<ImagePart>> {
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
            Block::paragraph(text_block("Unlisted")),
        ]
    );
}

#[test]
fn finds_headings_by_style_name() {
    let blocks = parse_with(
        r#"<w:p><w:pPr><w:pStyle w:val="Kop2"/></w:pPr><w:r><w:t>Inleiding</w:t></w:r></w:p>"#,
        &sample_package(),
    );
    assert_eq!(blocks, [Block::heading(2, text_block("Inleiding"))]);
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
        [Block::paragraph(vec![
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
                vec![cell_of("Team"), cell_of("Members")],
                vec![cell_of("One"), cell_of("Bobby\nDon\nx\ny")],
            ]),
            Block::paragraph(text_block("After")),
        ]
    );
}

#[test]
fn ignores_tracked_formatting_changes() {
    let blocks = parse(
        r#"<w:p><w:r><w:rPr><w:b/><w:rPrChange><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>now bold</w:t></w:r></w:p>"#,
    );
    assert_eq!(blocks, [Block::paragraph(vec![Run::new("now bold", BOLD)])]);
}

#[test]
fn reads_pictures_with_alt_text() {
    let mut package = Package::default();
    package
        .targets
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
            Block::paragraph(vec![
                Run::new("Chart: ", PLAIN),
                Run::image(ImagePart::new("word/media/image1.png"), "Sales by region"),
            ]),
            Block::paragraph(vec![Run::image(
                ImagePart::new("word/media/image1.png"),
                "Old style"
            )]),
        ]
    );
}

/// `run`, an image run, with the display size the document gives it.
fn sized(mut run: Run<ImagePart>, cx: u32, cy: u32) -> Run<ImagePart> {
    run.image.as_mut().unwrap().size = Some((cx, cy));
    run
}

#[test]
fn reads_picture_sizes_from_their_extent() {
    let mut package = Package::default();
    package
        .targets
        .images
        .insert("rId7".into(), "word/media/image1.png".into());
    let picture = r#"<a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill>
                       <pic:spPr><a:xfrm><a:ext cx="1" cy="1"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic>"#;
    let blocks = parse_with(
        &format!(
            r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="1828800" cy="914400"/><wp:docPr id="1" name="a"/>{picture}</wp:inline></w:drawing></w:r></w:p>
               <w:p><w:r><w:drawing><wp:anchor><wp:extent cx="0" cy="914400"/><wp:docPr id="2" name="b"/>{picture}</wp:anchor></w:drawing></w:r></w:p>
               <w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="3" name="chart"/></wp:inline></w:drawing></w:r></w:p>
               <w:p><w:r><w:drawing><wp:inline><wp:docPr id="4" name="c"/>{picture}</wp:inline></w:drawing></w:r></w:p>"#
        ),
        &package,
    );
    assert_eq!(
        blocks,
        [
            // 2 by 1 inches. The `a:ext` inside the graphic isn't the displayed size.
            Block::paragraph(vec![sized(
                Run::image(ImagePart::new("word/media/image1.png"), ""),
                1_828_800,
                914_400
            )]),
            // A size of zero is ignored.
            Block::paragraph(vec![Run::image(
                ImagePart::new("word/media/image1.png"),
                ""
            )]),
            // The chart's size doesn't carry over to the next picture, which has none.
            Block::paragraph(vec![Run::image(
                ImagePart::new("word/media/image1.png"),
                ""
            )]),
        ]
    );
}

#[test]
fn reads_a_picture_grouped_with_a_text_box() {
    // The text box's runs sit inside the run that holds the drawing. The picture after them is
    // still in that outer run, so it isn't lost when the first inner run ends.
    let mut package = Package::default();
    package
        .targets
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
            Block::paragraph(vec![Run::new("Caption", PLAIN)]),
            Block::paragraph(vec![Run::image(
                ImagePart::new("word/media/image1.png"),
                "Team photo"
            )]),
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

/// A table cell holding `text`, merged across `cols` columns and `rows` rows.
fn merged(text: &str, cols: usize, rows: usize) -> TableCell<ImagePart> {
    TableCell::Content {
        runs: text_block(text),
        cols,
        rows,
    }
}

fn cell_of(text: &str) -> TableCell<ImagePart> {
    TableCell::new(text_block(text))
}

/// A table cell with properties `props` holding `text`, or an empty paragraph.
fn tc(props: &str, text: &str) -> String {
    format!(r#"<w:tc><w:tcPr>{props}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#)
}

/// A table with `columns` grid columns and these rows of cells.
fn grid_table(columns: usize, rows: &[&[String]]) -> String {
    let rows: String = rows
        .iter()
        .map(|cells| format!("<w:tr>{}</w:tr>", cells.concat()))
        .collect();
    format!(
        "<w:tbl><w:tblGrid>{}</w:tblGrid>{rows}</w:tbl>",
        "<w:gridCol/>".repeat(columns)
    )
}

#[test]
fn a_cell_merged_across_columns_covers_the_rest() {
    let span = |n: u32| format!(r#"<w:gridSpan w:val="{n}"/>"#);
    let body = grid_table(
        3,
        &[
            &[tc(&span(3), "Sales 2026")],
            &[tc("", "Q1"), tc("", "Q2"), tc("", "Q3")],
            &[tc(&span(2), "H1"), tc("", "260")],
        ],
    );

    assert_eq!(
        parse(&body),
        [Block::Table(vec![
            vec![
                merged("Sales 2026", 3, 1),
                TableCell::Covered,
                TableCell::Covered
            ],
            vec![cell_of("Q1"), cell_of("Q2"), cell_of("Q3")],
            vec![merged("H1", 2, 1), TableCell::Covered, cell_of("260")],
        ])]
    );
}

#[test]
fn a_cell_merged_down_rows_covers_the_cells_that_continue_it() {
    let restart = r#"<w:gridSpan w:val="2"/><w:vMerge w:val="restart"/>"#;
    let continued = r#"<w:gridSpan w:val="2"/><w:vMerge/>"#;
    let body = grid_table(
        3,
        &[
            &[tc(restart, "North"), tc("", "Q1")],
            &[tc(continued, ""), tc("", "Q2")],
            &[tc(continued, ""), tc("", "Q3")],
            &[tc("", "South"), tc("", ""), tc("", "Q1")],
        ],
    );

    assert_eq!(
        parse(&body),
        [Block::Table(vec![
            vec![merged("North", 2, 3), TableCell::Covered, cell_of("Q1")],
            vec![TableCell::Covered, TableCell::Covered, cell_of("Q2")],
            vec![TableCell::Covered, TableCell::Covered, cell_of("Q3")],
            vec![cell_of("South"), TableCell::new(Vec::new()), cell_of("Q1")],
        ])]
    );
}

/// Word hides nothing a continuing cell holds, so neither do we: it ends the merge instead.
#[test]
fn a_continuing_cell_with_text_keeps_it() {
    let body = grid_table(
        1,
        &[
            &[tc(r#"<w:vMerge w:val="restart"/>"#, "Top")],
            &[tc("<w:vMerge/>", "")],
            &[tc("<w:vMerge/>", "Kept")],
            &[tc("<w:vMerge/>", "")],
        ],
    );

    assert_eq!(
        parse(&body),
        [Block::Table(vec![
            vec![merged("Top", 1, 2)],
            vec![TableCell::Covered],
            vec![merged("Kept", 1, 2)],
            vec![TableCell::Covered],
        ])]
    );
}

#[test]
fn a_merged_cell_never_spans_past_the_grid() {
    let body = r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:tblGridChange><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid></w:tblGridChange></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val="4000000000"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    assert_eq!(
        parse(body),
        [Block::Table(vec![vec![
            merged("x", 2, 1),
            TableCell::Covered
        ]])]
    );
}

/// A merge that continues past the last row, or a continuing cell with nothing to continue,
/// stays inside the table.
#[test]
fn a_merge_down_ends_with_the_table() {
    let body = grid_table(
        2,
        &[
            &[
                tc("<w:vMerge/>", ""),
                tc(r#"<w:vMerge w:val="restart"/>"#, "x"),
            ],
            &[tc("", "y"), tc("<w:vMerge/>", "")],
        ],
    );
    assert_eq!(
        parse(&body),
        [Block::Table(vec![
            vec![TableCell::new(Vec::new()), merged("x", 1, 2)],
            vec![cell_of("y"), TableCell::Covered],
        ])]
    );
}

/// A footnotes part holding Word's two separators, then `notes`.
fn footnotes(notes: &str) -> String {
    format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
             <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
             <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
             {notes}
           </w:footnotes>"#
    )
}

/// A note with ID `id` holding one paragraph: Word's mark for the note, then `text`.
fn note(id: &str, text: &str) -> String {
    format!(
        r#"<w:footnote w:id="{id}"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> {text}</w:t></w:r></w:p></w:footnote>"#
    )
}

#[test]
fn reads_notes_and_skips_separators() {
    let xml = footnotes(&format!(
        r#"{}<w:footnote w:id="2"><w:p><w:r><w:t>Cites</w:t><w:footnoteReference w:id="1"/></w:r></w:p></w:footnote>"#,
        note("1", "Source.")
    ));
    let notes = parse_notes(&xml, &Package::default(), &Targets::default()).unwrap();
    assert_eq!(
        notes,
        [
            (
                "1".to_string(),
                vec![Block::paragraph(text_block(" Source."))]
            ),
            // A reference inside a note is dropped.
            ("2".to_string(), vec![Block::paragraph(text_block("Cites"))]),
        ]
    );
}

/// A package holding footnotes 1 and 2 and endnote 1, each one paragraph naming itself.
fn package_with_notes() -> Package {
    let paragraph = |text: &str| vec![Block::paragraph(text_block(text))];
    Package {
        notes: HashMap::from([
            ((NoteKind::Footnote, "1".into()), paragraph("footnote 1")),
            ((NoteKind::Footnote, "2".into()), paragraph("footnote 2")),
            ((NoteKind::Endnote, "1".into()), paragraph("endnote 1")),
        ]),
        ..Package::default()
    }
}

#[test]
fn numbers_notes_in_the_order_the_body_first_refers_to_them() {
    let blocks = parse_with(
        r#"<w:p><w:r><w:t>A</w:t><w:footnoteReference w:id="2"/></w:r>
               <w:r><w:t>B</w:t><w:endnoteReference w:id="1"/></w:r>
               <w:r><w:t>C</w:t><w:footnoteReference w:id="1"/></w:r></w:p>"#,
        &package_with_notes(),
    );
    let note = |number, text: &str| Block::Note {
        number,
        blocks: vec![Block::paragraph(text_block(text))],
    };
    assert_eq!(
        blocks,
        [
            Block::paragraph(vec![
                Run::new("A", PLAIN),
                Run::note(1),
                Run::new("B", PLAIN),
                Run::note(2),
                Run::new("C", PLAIN),
                Run::note(3),
            ]),
            note(1, "footnote 2"),
            note(2, "endnote 1"),
            note(3, "footnote 1"),
        ]
    );
}

#[test]
fn adds_a_note_once_however_often_it_is_referred_to() {
    let body = r#"<w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#.repeat(1000);
    let blocks = parse_with(&body, &package_with_notes());

    let notes: Vec<_> = blocks
        .iter()
        .filter(|b| matches!(b, Block::Note { .. }))
        .collect();
    assert_eq!(
        notes,
        [&Block::Note {
            number: 1,
            blocks: vec![Block::paragraph(text_block("footnote 1"))],
        }]
    );
    assert_eq!(blocks.len(), 1001);
    assert!(
        blocks[..1000]
            .iter()
            .all(|b| *b == Block::paragraph(vec![Run::note(1)]))
    );
}

#[test]
fn drops_references_to_notes_the_file_does_not_have() {
    let blocks = parse_with(
        r#"<w:p><w:r><w:t>Text</w:t><w:footnoteReference w:id="9"/><w:endnoteReference w:id="2"/><w:footnoteReference/></w:r></w:p>"#,
        &package_with_notes(),
    );
    assert_eq!(blocks, [Block::paragraph(text_block("Text"))]);
}

/// A section's properties, referring to its headers and footers as `(element, type, id)`.
fn sect_pr(references: &[(&str, &str, &str)]) -> String {
    let references: String = references
        .iter()
        .map(|(element, kind, id)| format!(r#"<w:{element} w:type="{kind}" r:id="{id}"/>"#))
        .collect();
    format!("<w:sectPr>{references}</w:sectPr>")
}

#[test]
fn finds_the_first_sections_default_header_and_footer() {
    let first = sect_pr(&[
        ("headerReference", "first", "rIdFirst"),
        ("headerReference", "default", "rIdHeader"),
        ("footerReference", "even", "rIdEven"),
        ("footerReference", "default", "rIdFooter"),
    ]);
    let second = sect_pr(&[
        ("headerReference", "default", "rIdLater"),
        ("footerReference", "default", "rIdLater"),
    ]);
    let xml = format!(
        r#"<w:document xmlns:w="w" xmlns:r="r"><w:body><w:p><w:pPr>{first}</w:pPr></w:p>{second}</w:body></w:document>"#
    );
    assert_eq!(
        first_section_parts(&xml).unwrap(),
        (Some("rIdHeader".into()), Some("rIdFooter".into()))
    );

    // A first section without a default footer has none, whatever later sections have.
    let xml = format!(
        r#"<w:document xmlns:w="w" xmlns:r="r"><w:body><w:p><w:pPr>{}</w:pPr></w:p>{second}</w:body></w:document>"#,
        sect_pr(&[("headerReference", "default", "rIdHeader")])
    );
    assert_eq!(
        first_section_parts(&xml).unwrap(),
        (Some("rIdHeader".into()), None)
    );
}

/// Header XML holding one paragraph of `content`.
fn header(content: &str) -> String {
    format!(
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p>{content}</w:p></w:hdr>"#
    )
}

#[test]
fn reads_page_numbers_in_headers_and_footers_as_fields() {
    let xml = header(
        r#"<w:r><w:t xml:space="preserve">Report, page </w:t></w:r>
           <w:fldSimple w:instr=" PAGE \* MERGEFORMAT "><w:r><w:t>4</w:t></w:r></w:fldSimple>
           <w:r><w:t xml:space="preserve"> of </w:t></w:r>
           <w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> NUMPAGES </w:instrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>9</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>
           <w:r><w:t xml:space="preserve">, </w:t></w:r>
           <w:fldSimple w:instr="AUTHOR"><w:r><w:t>Ada</w:t></w:r></w:fldSimple>"#,
    );
    let blocks = parse_page_furniture(&xml, &Package::default(), &Targets::default()).unwrap();
    // The numbers Word saved, 4 and 9, are dropped; the author's name isn't a page field.
    assert_eq!(
        blocks,
        [Block::paragraph(vec![
            Run::new("Report, page ", PLAIN),
            Run::field(Field::PageNumber, PLAIN),
            Run::new(" of ", PLAIN),
            Run::field(Field::PageCount, PLAIN),
            Run::new(", Ada", PLAIN),
        ])]
    );
}

#[test]
fn keeps_page_numbers_in_the_body() {
    let blocks = parse(
        r#"<w:p><w:r><w:t xml:space="preserve">See page </w:t></w:r><w:fldSimple w:instr="PAGE"><w:r><w:t>4</w:t></w:r></w:fldSimple></w:p>"#,
    );
    assert_eq!(blocks, [Block::paragraph(text_block("See page 4"))]);
}

#[test]
fn reads_a_header_once_however_many_sections_refer_to_it() {
    let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let section = sect_pr(&[
        ("headerReference", "default", "rId1"),
        ("headerReference", "first", "rId1"),
        ("headerReference", "even", "rId1"),
    ]);
    let body = format!("<w:p><w:pPr>{section}</w:pPr></w:p>").repeat(1000);
    let document = format!(
        r#"<w:document xmlns:w="{w}" xmlns:r="{rel}"><w:body>{body}</w:body></w:document>"#
    );
    let rels = format!(
        r#"<Relationships><Relationship Id="rId1" Type="{rel}/header" Target="header1.xml"/></Relationships>"#
    );
    let padding = "x".repeat(100_000);
    let header = header(&format!("<w:r><w:t>Title</w:t></w:r><!--{padding}-->"));

    // Enough to read every part once, but not the header twice.
    let total = (document.len() + rels.len() + header.len() * 3 / 2) as u64;
    let package = crate::opc::test_package(&[
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
    ]);
    let mut archive =
        Archive::with_limits(package, crate::opc::Limits { part: total, total }).unwrap();

    let blocks = read_blocks(&mut archive).unwrap();
    assert_eq!(
        blocks,
        [Block::Header(vec![Block::paragraph(text_block("Title"))])]
    );
}

#[test]
fn reads_alignment_from_paragraphs_and_their_styles() {
    let mut package = Package::default();
    package.styles = package::parse_styles(
        r#"<w:styles xmlns:w="w">
             <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:pPr><w:jc w:val="center"/></w:pPr></w:style>
             <w:style w:type="paragraph" w:styleId="Signature"><w:name w:val="Signature"/><w:pPr><w:jc w:val="right"/></w:pPr></w:style>
           </w:styles>"#,
    )
    .unwrap();
    let blocks = parse_with(
        r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
           <w:p><w:pPr><w:pStyle w:val="Signature"/></w:pPr><w:r><w:t>Signed</w:t></w:r></w:p>
           <w:p><w:pPr><w:pStyle w:val="Signature"/><w:jc w:val="left"/></w:pPr><w:r><w:t>Own wins</w:t></w:r></w:p>
           <w:p><w:pPr><w:jc w:val="both"/><w:pPrChange><w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange></w:pPr><w:r><w:t>Justified</w:t></w:r></w:p>
           <w:p><w:pPr><w:jc w:val="sideways"/></w:pPr><w:r><w:t>Unknown</w:t></w:r></w:p>"#,
        &package,
    );
    let paragraph = |text: &str, align| Block::Paragraph {
        runs: text_block(text),
        align,
    };
    assert_eq!(
        blocks,
        [
            Block::Heading {
                level: 1,
                runs: text_block("Title"),
                align: Align::Center,
            },
            paragraph("Signed", Align::Right),
            paragraph("Own wins", Align::Left),
            // The tracked change's old alignment is skipped.
            paragraph("Justified", Align::Justify),
            paragraph("Unknown", Align::Left),
        ]
    );
}

#[test]
fn a_tables_alignment_is_not_its_texts() {
    // A table in a text box sits inside a paragraph, which its `w:jc` mustn't align.
    let blocks = parse(
        r#"<w:p><w:r><w:t>Outer</w:t></w:r><w:r><w:pict><v:shape xmlns:v="v"><v:textbox><w:txbxContent>
             <w:tbl><w:tblPr><w:jc w:val="center"/></w:tblPr><w:tr><w:trPr><w:jc w:val="right"/></w:trPr>
               <w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
           </w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
    );
    let outer = blocks
        .iter()
        .find(|b| matches!(b, Block::Paragraph { runs, .. } if runs[0].text == "Outer"))
        .unwrap();
    assert_eq!(outer, &Block::paragraph(text_block("Outer")));
}
