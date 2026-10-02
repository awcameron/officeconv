//! End-to-end tests: run the real binary and check its output.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

fn officeconv() -> Command {
    Command::cargo_bin("officeconv").unwrap()
}

/// Writes a small one-sheet workbook into a fresh temp dir.
fn sample_xlsx() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sales.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_row(0, 0, ["Region", "Units"]).unwrap();
    sheet.write_row(1, 0, ["North, East", "12"]).unwrap();
    sheet.write(2, 0, "South").unwrap();
    sheet.write(2, 1, 7.5).unwrap();
    workbook.save(&path).unwrap();
    (dir, path)
}

/// Writes a minimal `.docx` whose body is `body_xml` into a fresh temp dir.
fn sample_docx(body_xml: &str) -> (TempDir, PathBuf) {
    sample_docx_with_parts(body_xml, &[])
}

/// Like [`sample_docx`], plus extra parts such as `word/numbering.xml`.
fn sample_docx_with_parts(body_xml: &str, parts: &[(&str, &str)]) -> (TempDir, PathBuf) {
    use std::io::Write;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("notes.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();

    zip.start_file("word/document.xml", options).unwrap();
    write!(
        zip,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body_xml}</w:body></w:document>"#
    )
    .unwrap();
    for (name, contents) in parts {
        zip.start_file(*name, options).unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    (dir, path)
}

/// Writes a `.zip`-based Office file named `name` containing `parts` into a fresh temp dir.
fn sample_package(name: &str, parts: &[(&str, &str)]) -> (TempDir, PathBuf) {
    use std::io::Write;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (part, contents) in parts {
        zip.start_file(*part, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    (dir, path)
}

/// Runs the converter on `path` and returns its stdout.
fn convert(path: &std::path::Path, to: &str) -> String {
    let output = officeconv().arg(path).args(["--to", to]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Creates an empty file with the given name inside a fresh temp dir.
fn touch(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, b"").unwrap();
    (dir, path)
}

#[test]
fn rejects_unknown_extension() {
    let (_dir, path) = touch("report.pdf");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .failure()
        .stderr(contains("unsupported input file"));
}

#[test]
fn rejects_docx_to_csv() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .failure()
        .stderr(contains("cannot convert docx to csv"));
}

#[test]
fn rejects_sheet_option_on_docx() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--sheet", "Sales"])
        .assert()
        .failure()
        .stderr(contains("only apply to .xlsx"));
}

#[test]
fn reports_missing_input() {
    officeconv()
        .args(["does-not-exist.xlsx", "--to", "csv"])
        .assert()
        .failure()
        .stderr(contains("input file not found"));
}

#[test]
fn reports_corrupt_workbook() {
    // An empty file isn't a valid zip, so calamine can't open it.
    let (_dir, path) = touch("broken.xlsx");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .failure()
        .stderr(contains("could not read workbook"));
}

#[test]
fn converts_xlsx_to_every_format() {
    let (_dir, path) = sample_xlsx();

    assert_eq!(
        convert(&path, "csv"),
        "Region,Units\n\"North, East\",12\nSouth,7.5\n"
    );
    assert_eq!(
        convert(&path, "tsv"),
        "Region\tUnits\nNorth, East\t12\nSouth\t7.5\n"
    );
    assert_eq!(
        convert(&path, "md"),
        "\
| Region      | Units |
| ----------- | ----- |
| North, East | 12    |
| South       | 7.5   |
"
    );

    let json: serde_json::Value = serde_json::from_str(&convert(&path, "json")).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            { "Region": "North, East", "Units": "12" },
            { "Region": "South", "Units": "7.5" },
        ])
    );
}

#[test]
fn writes_to_output_file() {
    let (dir, path) = sample_xlsx();
    let out = dir.path().join("sales.csv");

    officeconv()
        .arg(&path)
        .args(["--to", "csv", "-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout("");

    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "Region,Units\n\"North, East\",12\nSouth,7.5\n"
    );
}

#[test]
fn reports_unwritable_output() {
    let (dir, path) = sample_xlsx();
    let out = dir.path().join("no-such-dir").join("sales.csv");

    officeconv()
        .arg(&path)
        .args(["--to", "csv", "-o"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(contains("could not create"));
}

#[test]
fn writes_each_sheet_to_its_own_file() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("report.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Q1")
        .unwrap()
        .write(0, 0, "a")
        .unwrap();
    workbook
        .add_worksheet()
        .set_name("Q2 (draft)")
        .unwrap()
        .write(0, 0, "b")
        .unwrap();
    workbook.save(&input).unwrap();
    let out_dir = dir.path().join("out");

    officeconv()
        .arg(&input)
        .args(["--to", "csv", "--all-sheets", "-o"])
        .arg(&out_dir)
        .assert()
        .success()
        .stderr(contains("report-Q1.csv").and(contains("report-Q2 (draft).csv")));

    assert_eq!(
        std::fs::read_to_string(out_dir.join("report-Q1.csv")).unwrap(),
        "a\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("report-Q2 (draft).csv")).unwrap(),
        "b\n"
    );
}

#[test]
fn all_sheets_defaults_to_current_directory() {
    let (dir, path) = sample_xlsx();

    officeconv()
        .current_dir(dir.path())
        .arg(&path)
        .args(["--to", "md", "--all-sheets"])
        .assert()
        .success();

    assert!(dir.path().join("sales-Sheet1.md").is_file());
}

#[test]
fn reports_unknown_sheet() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "csv", "--sheet", "Nope"])
        .assert()
        .failure()
        .stderr(contains(
            "sheet \"Nope\" not found; available sheets: Sheet1",
        ));
}

#[test]
fn converts_docx_to_markdown() {
    let (_dir, path) = sample_docx(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Meeting notes</w:t></w:r></w:p>
           <w:p><w:r><w:t xml:space="preserve">Ship the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>beta</w:t></w:r><w:r><w:t xml:space="preserve"> on </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>Friday</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p>"#,
    );

    assert_eq!(
        convert(&path, "md"),
        "# Meeting notes\n\nShip the **beta** on *Friday*.\n"
    );
}

#[test]
fn reports_docx_without_document_xml() {
    // A valid zip that isn't a Word document.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fake.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();

    officeconv()
        .arg(&path)
        .args(["--to", "md"])
        .assert()
        .failure()
        .stderr(contains("could not read document"));
}

#[test]
fn converts_docx_lists_links_and_tables() {
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://www.rust-lang.org" TargetMode="External"/>
    </Relationships>"#;
    let numbering = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
    </w:numbering>"#;
    let item = |text: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    };
    let cell = |text: &str| format!("<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>");
    let body = format!(
        r#"<w:p><w:r><w:t xml:space="preserve">Learn </w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t>Rust</w:t></w:r></w:hyperlink></w:p>{}{}<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>"#,
        item("Read the book"),
        item("Build a CLI"),
        cell("Crate"),
        cell("Use"),
        cell("zip"),
        cell("unpack .docx"),
    );
    let (_dir, path) = sample_docx_with_parts(
        &body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/numbering.xml", numbering),
        ],
    );

    assert_eq!(
        convert(&path, "md"),
        "\
Learn [Rust](https://www.rust-lang.org)

1. Read the book
1. Build a CLI

| Crate | Use          |
| ----- | ------------ |
| zip   | unpack .docx |
"
    );
}

/// A two-slide deck: "Agenda" with a link and speaker notes, then "Thanks".
///
/// The presentation lists slide2.xml first, so it must come out first.
fn sample_pptx() -> (TempDir, PathBuf) {
    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    let title = |text: &str| {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    };
    let body = |paragraphs: &str| {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody>{paragraphs}</p:txBody></p:sp>"#
        )
    };
    let slide = |shapes: String| {
        format!(r#"<p:sld {NS}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#)
    };

    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#
    );
    let presentation_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{REL}/slide" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="{REL}/slide" Target="slides/slide2.xml"/></Relationships>"#
    );
    let first = slide(format!(
        "{}{}",
        title("Agenda"),
        body(
            r#"<a:p><a:r><a:t xml:space="preserve">Read </a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="rId5"/></a:rPr><a:t>the book</a:t></a:r></a:p>"#
        )
    ));
    let first_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="{REL}/hyperlink" Target="https://doc.rust-lang.org/book/" TargetMode="External"/><Relationship Id="rId6" Type="{REL}/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#
    );
    let notes = slide(body("<a:p><a:r><a:t>Keep it short.</a:t></a:r></a:p>"));
    let second = slide(title("Thanks"));

    sample_package(
        "talk.pptx",
        &[
            ("ppt/presentation.xml", &presentation),
            ("ppt/_rels/presentation.xml.rels", &presentation_rels),
            ("ppt/slides/slide1.xml", &second),
            ("ppt/slides/slide2.xml", &first),
            ("ppt/slides/_rels/slide2.xml.rels", &first_rels),
            ("ppt/notesSlides/notesSlide1.xml", &notes),
        ],
    )
}

#[test]
fn converts_pptx_to_markdown() {
    let (_dir, path) = sample_pptx();

    assert_eq!(
        convert(&path, "md"),
        "\
## Slide 1: Agenda

- Read [the book](https://doc.rust-lang.org/book/)

### Notes

Keep it short.

---

## Slide 2: Thanks
"
    );
}

#[test]
fn rejects_pptx_to_csv_and_sheet_options() {
    let (_dir, path) = touch("talk.pptx");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .failure()
        .stderr(contains("cannot convert pptx to csv; pptx supports: md"));
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--all-sheets"])
        .assert()
        .failure()
        .stderr(contains("only apply to .xlsx"));
}

#[test]
fn leaves_out_notes_with_no_notes() {
    let (_dir, path) = sample_pptx();
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--no-notes"])
        .assert()
        .success()
        .stdout(
            "\
## Slide 1: Agenda

- Read [the book](https://doc.rust-lang.org/book/)

---

## Slide 2: Thanks
",
        );
}

#[test]
fn rejects_no_notes_on_other_inputs() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--no-notes"])
        .assert()
        .failure()
        .stderr(contains("--no-notes only applies to .pptx"));
}

#[test]
fn saves_docx_images_next_to_the_markdown() {
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
    </Relationships>"#;
    let picture = r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="Picture 1" descr="Team photo"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="pic"><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let (dir, path) = sample_docx_with_parts(
        &format!(r#"<w:p><w:r><w:t xml:space="preserve">Us: </w:t></w:r>{picture}</w:p>"#),
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/media/image1.png", "fake png bytes"),
        ],
    );
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    officeconv()
        .arg(&path)
        .args(["--to", "md", "-o"])
        .arg(out.join("notes.md"))
        .arg("--images")
        .arg(out.join("notes_images"))
        .assert()
        .success()
        .stderr(contains("saved 1 images"));

    assert_eq!(
        std::fs::read_to_string(out.join("notes.md")).unwrap(),
        "Us: ![Team photo](notes_images/image1.png)\n"
    );
    assert_eq!(
        std::fs::read(out.join("notes_images/image1.png")).unwrap(),
        b"fake png bytes"
    );
}

#[test]
fn leaves_images_out_without_the_flag() {
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
    </Relationships>"#;
    let (_dir, path) = sample_docx_with_parts(
        r#"<w:p><w:r><w:drawing><wp:docPr id="1" name="p" descr="Photo"/><a:blip r:embed="rId4"/></w:drawing></w:r></w:p><w:p><w:r><w:t>Text</w:t></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/media/image1.png", "fake png bytes"),
        ],
    );
    assert_eq!(convert(&path, "md"), "Text\n");
}

#[test]
fn saves_pptx_pictures() {
    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#
    );
    let presentation_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{REL}/slide" Target="slides/slide1.xml"/></Relationships>"#
    );
    let slide = format!(
        r#"<p:sld {NS}><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3" descr="Revenue chart"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#
    );
    let slide_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId3" Type="{REL}/image" Target="../media/image1.png"/></Relationships>"#
    );
    let (dir, path) = sample_package(
        "talk.pptx",
        &[
            ("ppt/presentation.xml", &presentation),
            ("ppt/_rels/presentation.xml.rels", &presentation_rels),
            ("ppt/slides/slide1.xml", &slide),
            ("ppt/slides/_rels/slide1.xml.rels", &slide_rels),
            ("ppt/media/image1.png", "fake png bytes"),
        ],
    );

    officeconv()
        .current_dir(dir.path())
        .arg(&path)
        .args(["--to", "md", "--images", "media"])
        .assert()
        .success()
        .stdout("## Slide 1\n\n![Revenue chart](media/image1.png)\n");
    assert!(dir.path().join("media/image1.png").is_file());
}

/// Valid 2x2 PNGs in red, blue and green. `rust_xlsxwriter` reads each image's header, so fake
/// bytes won't do, and it stores identical images once, so each picture needs its own.
const RED_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 253, 212, 154, 115, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 252, 207, 0, 2, 76, 96,
    146, 1, 0, 13, 29, 1, 3, 130, 201, 113, 255, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const BLUE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 253, 212, 154, 115, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 100, 96, 248, 207, 192,
    192, 192, 196, 0, 6, 0, 11, 31, 1, 3, 20, 227, 11, 0, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
    130,
];
const GREEN_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 253, 212, 154, 115, 0, 0, 0, 19, 73, 68, 65, 84, 120, 156, 99, 100, 104, 96, 96, 96, 96,
    96, 2, 17, 12, 12, 0, 6, 42, 0, 132, 216, 239, 242, 82, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
    96, 130,
];

/// A workbook with pictures: "Sales" has a logo at D5 (inserted first) and a chart at D2;
/// "R&D" has one picture at B2.
fn xlsx_with_pictures() -> (TempDir, PathBuf) {
    use rust_xlsxwriter::{Image, Workbook};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("book.xlsx");
    let picture = |png: &[u8], alt: &str| Image::new_from_buffer(png).unwrap().set_alt_text(alt);

    let mut workbook = Workbook::new();
    let sales = workbook.add_worksheet().set_name("Sales").unwrap();
    sales.write_row(0, 0, ["Region", "Units"]).unwrap();
    sales.write_row(1, 0, ["North", "12"]).unwrap();
    sales
        .insert_image(4, 3, &picture(RED_PNG, "Company logo"))
        .unwrap();
    sales
        .insert_image(1, 3, &picture(BLUE_PNG, "Sales chart"))
        .unwrap();
    let research = workbook.add_worksheet().set_name("R&D").unwrap();
    research.write(0, 0, "Note").unwrap();
    research
        .insert_image(1, 1, &picture(GREEN_PNG, "Prototype"))
        .unwrap();
    workbook.save(&path).unwrap();
    (dir, path)
}

#[test]
fn lists_xlsx_pictures_after_the_markdown_table() {
    let (dir, path) = xlsx_with_pictures();
    let markdown = dir.path().join("book.md");

    officeconv()
        .arg(&path)
        .args(["--to", "md", "-o"])
        .arg(&markdown)
        .arg("--images")
        .arg(dir.path().join("img"))
        .assert()
        .success()
        .stderr(contains("saved 2 images"));

    let text = std::fs::read_to_string(&markdown).unwrap();
    // The chart at D2 comes before the logo at D5, whatever order they were inserted in.
    assert!(
        has_table_then_chart_then_logo(&text),
        "unexpected Markdown:\n{text}"
    );
    let saved = std::fs::read_dir(dir.path().join("img")).unwrap().count();
    assert_eq!(saved, 2);
}

/// Checks the Markdown has the table, then the chart, then the logo, each linking into `img/`.
fn has_table_then_chart_then_logo(text: &str) -> bool {
    let table = "| Region | Units |\n| ------ | ----- |\n| North  | 12    |\n\n";
    let Some(rest) = text.strip_prefix(table) else {
        return false;
    };
    let lines: Vec<&str> = rest.lines().filter(|l| !l.is_empty()).collect();
    lines.len() == 2
        && lines[0].starts_with("![Sales chart](img/")
        && lines[1].starts_with("![Company logo](img/")
}

#[test]
fn keeps_csv_data_unchanged_while_saving_pictures() {
    let (dir, path) = xlsx_with_pictures();
    let plain = convert(&path, "csv");

    officeconv()
        .current_dir(dir.path())
        .arg(&path)
        .args(["--to", "csv", "--images", "img"])
        .assert()
        .success()
        .stdout(plain);
    assert_eq!(
        std::fs::read_dir(dir.path().join("img")).unwrap().count(),
        2
    );
}

#[test]
fn saves_pictures_for_the_chosen_sheet_only() {
    let (dir, path) = xlsx_with_pictures();

    officeconv()
        .current_dir(dir.path())
        .arg(&path)
        .args(["--to", "md", "--sheet", "R&D", "--images", "img"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "| Note |\n| ---- |\n\n![Prototype](img/",
        ))
        .stderr(contains("saved 1 images"));
}

#[test]
fn links_pictures_from_each_sheet_file_with_all_sheets() {
    let (dir, path) = xlsx_with_pictures();
    let out = dir.path().join("out");

    officeconv()
        .arg(&path)
        .args(["--to", "md", "--all-sheets", "-o"])
        .arg(&out)
        .arg("--images")
        .arg(out.join("img"))
        .assert()
        .success()
        .stderr(contains("saved 3 images"));

    let sales = std::fs::read_to_string(out.join("book-Sales.md")).unwrap();
    let research = std::fs::read_to_string(out.join("book-R&D.md")).unwrap();
    assert_eq!(sales.matches("](img/").count(), 2, "{sales}");
    assert_eq!(research.matches("](img/").count(), 1, "{research}");
    assert!(research.contains("![Prototype](img/"), "{research}");
}

#[test]
fn xlsx_output_is_unchanged_without_images_flag() {
    let (dir, path) = xlsx_with_pictures();
    assert_eq!(
        convert(&path, "md"),
        "| Region | Units |\n| ------ | ----- |\n| North  | 12    |\n"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// One sheet with a column of each kind of cell.
fn xlsx_with_types() -> (TempDir, PathBuf) {
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orders.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .write_row(0, 0, ["id", "zip", "price", "paid", "shipped", "note"])
        .unwrap();
    sheet.write(1, 0, 1).unwrap();
    sheet.write(1, 1, "00123").unwrap();
    sheet.write(1, 2, 19.99).unwrap();
    sheet.write(1, 3, true).unwrap();
    let date = ExcelDateTime::from_ymd(2026, 10, 1).unwrap();
    let date_format = Format::new().set_num_format("yyyy-mm-dd");
    sheet.write_with_format(1, 4, &date, &date_format).unwrap();
    // Column F ("note") is left empty.
    workbook.save(&path).unwrap();
    (dir, path)
}

#[test]
fn typed_json_keeps_numbers_booleans_and_empty_cells() {
    let (_dir, path) = xlsx_with_types();
    let output = officeconv()
        .arg(&path)
        .args(["--to", "json", "--typed"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{
            "id": 1, "zip": "00123", "price": 19.99, "paid": true,
            "shipped": "2026-10-01", "note": null
        }])
    );
}

#[test]
fn json_stays_text_without_typed() {
    let (_dir, path) = xlsx_with_types();
    let json: serde_json::Value = serde_json::from_str(&convert(&path, "json")).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{
            "id": "1", "zip": "00123", "price": "19.99", "paid": "true",
            "shipped": "2026-10-01", "note": ""
        }])
    );
}

#[test]
fn typed_works_with_all_sheets() {
    let (dir, path) = sample_xlsx();
    let out = dir.path().join("out");

    officeconv()
        .arg(&path)
        .args(["--to", "json", "--typed", "--all-sheets", "-o"])
        .arg(&out)
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("sales-Sheet1.json")).unwrap())
            .unwrap();
    // "12" was written as text, so it stays a string; 7.5 was a number.
    assert_eq!(
        json,
        serde_json::json!([
            { "Region": "North, East", "Units": "12" },
            { "Region": "South", "Units": 7.5 },
        ])
    );
}

#[test]
fn rejects_typed_without_json() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "csv", "--typed"])
        .assert()
        .failure()
        .stderr(contains("--typed only applies to --to json"));
}
