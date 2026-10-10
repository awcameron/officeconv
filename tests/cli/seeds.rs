//! The fuzz targets' seeds: small files to start fuzzing from, built with the same helpers as
//! the tests, so the file a reader feature's test builds can be a seed too.
//!
//! `fuzz/make-seeds.sh` runs [`write_fuzz_seeds`] to write them into `fuzz/corpus/<target>/`.
//! [`every_seed_converts`] runs with the other tests, so a seed that stops converting fails
//! there. A reader change that reads a new part or element adds a seed that has it.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::common::*;

/// Where [`write_fuzz_seeds`] writes: a folder holding one folder per fuzz target.
const SEEDS_DIR: &str = "OFFICECONV_SEEDS_DIR";

/// A seed for the fuzz target `target`, called `name`. The archive target also gets every
/// package seed.
struct Seed {
    target: &'static str,
    name: &'static str,
    file: (TempDir, PathBuf),
}

fn seeds() -> Vec<Seed> {
    let seed = |target, name, file| Seed { target, name, file };
    vec![
        seed("docx", "seed.docx", basic_docx()),
        seed("docx", "seed-notes.docx", sample_docx_with_notes()),
        seed(
            "docx",
            "seed-header-footer.docx",
            sample_docx_with_header_and_footer(3),
        ),
        seed("docx", "seed-layout.docx", layout_docx()),
        seed("pptx", "seed.pptx", basic_pptx()),
        seed("pptx", "seed-layout.pptx", layout_pptx()),
        seed("xlsx", "seed.xlsx", basic_xlsx()),
        seed("delimited", "seed.csv", basic_csv()),
    ]
}

#[test]
#[ignore = "writes the fuzz seeds; fuzz/make-seeds.sh runs it"]
fn write_fuzz_seeds() {
    let dir = PathBuf::from(
        std::env::var_os(SEEDS_DIR).unwrap_or_else(|| panic!("set {SEEDS_DIR} to a folder")),
    );
    let copy = |target: &str, name: &str, from: &Path| {
        let folder = dir.join(target);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::copy(from, folder.join(name)).unwrap();
    };
    for seed in seeds() {
        copy(seed.target, seed.name, &seed.file.1);
        if seed.target != "delimited" {
            copy("archive", seed.name, &seed.file.1);
        }
    }
}

#[test]
fn every_seed_converts() {
    let out = TempDir::new().unwrap();
    for seed in seeds() {
        let formats: &[&str] = match seed.target {
            "docx" | "pptx" if cfg!(feature = "pdf") => &["md", "pdf"],
            "docx" | "pptx" => &["md"],
            "xlsx" => &["md", "csv"],
            _ => &["json"],
        };
        for to in formats {
            officeconv()
                .arg(&seed.file.1)
                .args(["--to", to, "-o"])
                .arg(out.path().join(format!("{}.{to}", seed.name)))
                .assert()
                .success();
        }
    }
}

/// A Word document with a heading, bold and italic runs, a nested numbered list, a link, a
/// table with non-ASCII text, and a picture.
fn basic_docx() -> (TempDir, PathBuf) {
    let text = |t: &str| format!("<w:r><w:t>{t}</w:t></w:r>");
    let item = |level: u8, t: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="1"/></w:numPr></w:pPr>{}</w:p>"#,
            text(t)
        )
    };
    let cell = |t: &str| format!("<w:tc><w:p>{}</w:p></w:tc>", text(t));
    let body = format!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>{}</w:p><w:p><w:r><w:t xml:space="preserve">Ship the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>beta</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> soon</w:t></w:r></w:p>{}{}<w:p><w:hyperlink r:id="rId1">{}</w:hyperlink></w:p><w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl><w:p><w:r><w:drawing><wp:docPr xmlns:wp="wp" id="1" name="p" descr="Logo"/><a:blip xmlns:a="a" r:embed="rId2"/></w:drawing></w:r></w:p>"#,
        text("Notes"),
        item(0, "First"),
        item(1, "Nested"),
        text("Rust"),
        cell("Crate"),
        cell("Use"),
        cell("zip"),
        cell("Café Ωμέγα"),
    );
    sample_docx_with_parts(
        &body,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[
                    ("rId1", "hyperlink", "https://www.rust-lang.org"),
                    ("rId2", "image", "media/image1.png"),
                ]),
            ),
            part(
                "word/numbering.xml",
                format!(
                    r#"<w:numbering xmlns:w="{WORD_NS}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
                ),
            ),
            part(
                "word/styles.xml",
                format!(
                    r#"<w:styles xmlns:w="{WORD_NS}"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#
                ),
            ),
            part("word/media/image1.png", RED_PNG),
        ],
    )
}

/// A Word document with centered, right-aligned and justified paragraphs, a table with cells
/// merged across (`gridSpan`) and down (`vMerge`), and a sized picture whose drawing links to
/// a web page.
fn layout_docx() -> (TempDir, PathBuf) {
    let para = |jc: &str, t: &str| {
        format!(r#"<w:p><w:pPr><w:jc w:val="{jc}"/></w:pPr><w:r><w:t>{t}</w:t></w:r></w:p>"#)
    };
    let cell = |props: &str, t: &str| {
        format!("<w:tc><w:tcPr>{props}</w:tcPr><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>")
    };
    let table = format!(
        "<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid><w:tr>{}{}</w:tr><w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>",
        cell(r#"<w:gridSpan w:val="2"/>"#, "A"),
        cell("", "B"),
        cell(r#"<w:vMerge w:val="restart"/>"#, "C"),
        cell("", "D"),
        cell("", "E"),
        "<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>",
        cell("", "F"),
        cell("", "G"),
    );
    let picture = r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Picture 1" descr="Logo"><a:hlinkClick xmlns:a="a" r:id="rId9"/></wp:docPr><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="pic"><pic:nvPicPr><pic:cNvPr id="0" name="logo.png"/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
    let body = format!(
        "{}{}{}{table}{picture}",
        para("center", "Centered"),
        para("right", "Right"),
        para("both", "Justified"),
    );
    sample_docx_with_parts(
        &body,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[
                    ("rId4", "image", "media/image1.png"),
                    ("rId9", "hyperlink", "https://example.com/"),
                ]),
            ),
            part("word/media/image1.png", RED_PNG),
        ],
    )
}

/// A two-slide deck: a title, a bulleted body with a link, a picture and speaker notes, then
/// a second slide.
fn basic_pptx() -> (TempDir, PathBuf) {
    let title = |t: &str| placeholder("title", &format!("<a:p><a:r><a:t>{t}</a:t></a:r></a:p>"));
    let first = slide(&format!(
        r#"{}{}<p:pic><p:nvPicPr><p:cNvPr id="3" name="Picture" descr="Logo"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic>"#,
        title("Agenda"),
        placeholder(
            "body",
            r#"<a:p><a:r><a:t>Read the book</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>Rust</a:t></a:r></a:p>"#
        )
    ));
    let mut parts = presentation(&["slides/slide1.xml", "slides/slide2.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", first),
        part(
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[
                ("rId1", "notesSlide", "../notesSlides/notesSlide1.xml"),
                ("rId2", "hyperlink", "https://www.rust-lang.org"),
                ("rId3", "image", "../media/image1.png"),
            ]),
        ),
        part("ppt/slides/slide2.xml", slide(&title("Thanks"))),
        part(
            "ppt/notesSlides/notesSlide1.xml",
            slide(&placeholder(
                "body",
                "<a:p><a:r><a:t>Keep it short.</a:t></a:r></a:p>",
            )),
        ),
        part("ppt/media/image1.png", RED_PNG),
    ]);
    sample_package("talk.pptx", &parts)
}

/// A one-slide deck with a centered title, a table with cells merged across and down, a sized
/// picture that links to a web page, and speaker notes whose link comes from the notes part's
/// own relationships.
fn layout_pptx() -> (TempDir, PathBuf) {
    let cell = |attrs: &str, t: &str| {
        format!(
            "<a:tc{attrs}><a:txBody><a:bodyPr/><a:p><a:r><a:t>{t}</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>"
        )
    };
    let table = format!(
        r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="1"/><a:gridCol w="1"/><a:gridCol w="1"/></a:tblGrid><a:tr h="1">{}{}{}</a:tr><a:tr h="1">{}{}{}</a:tr><a:tr h="1">{}{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
        cell(r#" gridSpan="2""#, "A"),
        cell(r#" hMerge="1""#, ""),
        cell("", "B"),
        cell(r#" rowSpan="2""#, "C"),
        cell("", "D"),
        cell("", "E"),
        cell(r#" vMerge="1""#, ""),
        cell("", "F"),
        cell("", "G"),
    );
    let picture = r#"<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3" descr="Chart"><a:hlinkClick r:id="rId2"/></p:cNvPr><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="914400"/></a:xfrm></p:spPr></p:pic>"#;
    let shapes = format!(
        r#"{}{table}{picture}"#,
        placeholder(
            "title",
            r#"<a:p><a:pPr algn="ctr"/><a:r><a:t>Results</a:t></a:r></a:p>"#
        )
    );
    let notes = slide(&placeholder(
        "body",
        r#"<a:p><a:r><a:t xml:space="preserve">See </a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="rId1"/></a:rPr><a:t>the report</a:t></a:r></a:p>"#,
    ));
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", slide(&shapes)),
        part(
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[
                ("rId1", "notesSlide", "../notesSlides/notesSlide1.xml"),
                ("rId2", "hyperlink", "https://example.com/chart"),
                ("rId3", "image", "../media/image1.png"),
            ]),
        ),
        part("ppt/notesSlides/notesSlide1.xml", notes),
        part(
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            rels(&[("rId1", "hyperlink", "https://example.com/report")]),
        ),
        part("ppt/media/image1.png", RED_PNG),
    ]);
    sample_package("talk.pptx", &parts)
}

/// A workbook with two sheets: shared and inline strings, a number, a boolean, formulas, an
/// error, a gap row, and a picture on the first sheet.
fn basic_xlsx() -> (TempDir, PathBuf) {
    const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml";
    let content_types = format!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="{TYPE}.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="{TYPE}.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="{TYPE}.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="{TYPE}.sharedStrings+xml"/></Types>"#
    );
    let sales = format!(
        r#"<worksheet xmlns="{S}" xmlns:r="{REL}"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>Done</t></is></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>12.5</v></c><c r="C2" t="b"><v>1</v></c></row><row r="4"><c r="A4" t="str"><f>A2</f><v>North</v></c><c r="B4"><f>B2*2</f><v>25</v></c><c r="C4" t="e"><v>#DIV/0!</v></c></row></sheetData><drawing r:id="rId1"/></worksheet>"#
    );
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="{XDR}" xmlns:a="{A}" xmlns:r="{REL}"><xdr:twoCellAnchor><xdr:from><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="Picture 1" descr="Chart"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#
    );
    sample_package(
        "book.xlsx",
        &[
            part("[Content_Types].xml", content_types),
            part(
                "_rels/.rels",
                rels(&[("rId1", "officeDocument", "xl/workbook.xml")]),
            ),
            part(
                "xl/workbook.xml",
                format!(
                    r#"<workbook xmlns="{S}" xmlns:r="{REL}"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/></sheets></workbook>"#
                ),
            ),
            part(
                "xl/_rels/workbook.xml.rels",
                rels(&[
                    ("rId1", "worksheet", "worksheets/sheet1.xml"),
                    ("rId2", "worksheet", "worksheets/sheet2.xml"),
                    ("rId3", "sharedStrings", "sharedStrings.xml"),
                ]),
            ),
            part(
                "xl/sharedStrings.xml",
                format!(
                    r#"<sst xmlns="{S}"><si><t>Region</t></si><si><t>Total</t></si><si><t>North | "east"</t></si></sst>"#
                ),
            ),
            part("xl/worksheets/sheet1.xml", sales),
            part(
                "xl/worksheets/_rels/sheet1.xml.rels",
                rels(&[("rId1", "drawing", "../drawings/drawing1.xml")]),
            ),
            part(
                "xl/worksheets/sheet2.xml",
                format!(
                    r#"<worksheet xmlns="{S}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Café</t></is></c></row></sheetData></worksheet>"#
                ),
            ),
            part("xl/drawings/drawing1.xml", drawing),
            part(
                "xl/drawings/_rels/drawing1.xml.rels",
                rels(&[("rId1", "image", "../media/image1.png")]),
            ),
            part("xl/media/image1.png", RED_PNG),
        ],
    )
}

/// A CSV file with the quoting a reader has to get right: a byte-order mark, CRLF and LF line
/// ends, quoted commas, doubled quotes, a line break inside quotes, a bare quote, a tab and a
/// short row. The delimited target reads it as TSV too.
fn basic_csv() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("seed.csv");
    std::fs::write(
        &path,
        "\u{feff}Name,Note,Size\r\nAda,\"Hello, world\",5\" screen\r\n\"Alan \"\"AT\"\" Turing\",\"two\nlines\"\r\nTab\there,,,extra\n",
    )
    .unwrap();
    (dir, path)
}
