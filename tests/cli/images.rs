//! `--images`: saving pictures from DOCX, PPTX and XLSX and linking them.

use predicates::str::contains;
use std::path::PathBuf;
use tempfile::TempDir;

use crate::common::*;

/// More valid 2x2 PNGs, alongside `RED_PNG`. `rust_xlsxwriter` reads each image's header, so
/// fake bytes won't do, and it stores identical images once, so each picture needs its own.
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
fn saves_docx_images_next_to_the_markdown() {
    let rels = rels(&[("rId4", "image", "media/image1.png")]);
    let picture = r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="Picture 1" descr="Team photo"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="pic"><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let (dir, path) = sample_docx_with_parts(
        &format!(r#"<w:p><w:r><w:t xml:space="preserve">Us: </w:t></w:r>{picture}</w:p>"#),
        &[
            part("word/_rels/document.xml.rels", rels),
            part("word/media/image1.png", "fake png bytes"),
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
    let rels = rels(&[("rId4", "image", "media/image1.png")]);
    let (_dir, path) = sample_docx_with_parts(
        r#"<w:p><w:r><w:drawing><wp:docPr id="1" name="p" descr="Photo"/><a:blip r:embed="rId4"/></w:drawing></w:r></w:p><w:p><w:r><w:t>Text</w:t></w:r></w:p>"#,
        &[
            part("word/_rels/document.xml.rels", rels),
            part("word/media/image1.png", "fake png bytes"),
        ],
    );
    assert_eq!(convert(&path, "md"), "Text\n");
}

#[test]
fn saves_pptx_pictures() {
    let picture = r#"<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3" descr="Revenue chart"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic>"#;
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", slide(picture)),
        part(
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[("rId3", "image", "../media/image1.png")]),
        ),
        part("ppt/media/image1.png", "fake png bytes"),
    ]);
    let (dir, path) = sample_package("talk.pptx", &parts);

    officeconv()
        .current_dir(dir.path())
        .arg(&path)
        .args(["--to", "md", "--images", "media"])
        .assert()
        .success()
        .stdout("## Slide 1\n\n![Revenue chart](media/image1.png)\n");
    assert!(dir.path().join("media/image1.png").is_file());
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

    // The chart at D2 is listed before the logo at D5. (Reading order itself is unit-tested in
    // `xlsx::pictures`: rust_xlsxwriter already stores pictures in position order.)
    assert_eq!(
        std::fs::read_to_string(&markdown).unwrap(),
        "\
| Region | Units |
| ------ | ----- |
| North  | 12    |

![Sales chart](img/image1.png)

![Company logo](img/image2.png)
"
    );
    let saved = std::fs::read_dir(dir.path().join("img")).unwrap().count();
    assert_eq!(saved, 2);
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
