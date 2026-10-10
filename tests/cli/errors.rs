//! Bad input and option combinations: each fails with a clear message and its exit code.
//!
//! The codes are written as numbers, not taken from the `exit_code` constants in
//! `src/error.rs`, because scripts rely on the numbers: changing a constant should fail these
//! tests.

use predicates::str::contains;
use tempfile::TempDir;

use crate::common::*;

#[test]
fn leaves_argument_errors_to_clap() {
    // clap's own exit code for arguments it can't parse, distinct from the codes for usage
    // mistakes it can't see, such as --typed without --to json.
    officeconv()
        .args(["sales.xlsx", "--to", "xml"])
        .assert()
        .code(2)
        .stderr(contains("invalid value 'xml'"));
}

#[test]
fn rejects_unknown_extension() {
    let (_dir, path) = touch("report.pdf");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .code(65)
        .stderr(contains("unsupported input file"));
}

#[test]
fn rejects_docx_to_csv() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .code(64)
        .stderr(contains("cannot convert docx to csv"));
}

#[test]
fn rejects_sheet_option_on_docx() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--sheet", "Sales"])
        .assert()
        .code(64)
        .stderr(contains("only apply to .xlsx"));
}

#[test]
fn reports_missing_input() {
    officeconv()
        .args(["does-not-exist.xlsx", "--to", "csv"])
        .assert()
        .code(66)
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
        .code(65)
        .stderr(contains("could not read workbook"));
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
        .code(74)
        .stderr(contains("could not create"));
}

#[test]
fn reports_unknown_sheet() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "csv", "--sheet", "Nope"])
        .assert()
        .code(64)
        .stderr(contains(
            "sheet \"Nope\" not found; available sheets: Sheet1",
        ));
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
        .code(65)
        .stderr(contains(
            "could not read document: word/document.xml is missing",
        ));
}

#[test]
fn names_the_part_that_is_not_well_formed_xml() {
    let mut parts = presentation(&["slides/slide1.xml", "slides/slide2.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", slide("")),
        part("ppt/slides/slide2.xml", "<p:sld><p:cSld></p:sld>"),
    ]);
    let (_dir, path) = sample_package("talk.pptx", &parts);

    officeconv()
        .arg(&path)
        .args(["--to", "md"])
        .assert()
        .code(65)
        .stderr(contains("could not parse ppt/slides/slide2.xml: "));
}

#[test]
fn rejects_pptx_to_csv_and_sheet_options() {
    let (_dir, path) = touch("talk.pptx");
    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .code(64)
        .stderr(contains(
            "cannot convert pptx to csv; pptx supports: md, pdf",
        ));
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--all-sheets"])
        .assert()
        .code(64)
        .stderr(contains("only apply to .xlsx"));
}

#[test]
fn rejects_no_notes_on_other_inputs() {
    let (_dir, path) = touch("notes.docx");
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--no-notes"])
        .assert()
        .code(64)
        .stderr(contains("--no-notes only applies to .pptx"));
}

#[test]
fn rejects_typed_without_json() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "csv", "--typed"])
        .assert()
        .code(64)
        .stderr(contains("--typed only applies to --to json"));
}

#[test]
fn reports_a_directory_as_not_found_with_or_without_from() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("book.xlsx");
    std::fs::create_dir(&folder).unwrap();

    for extra in [&[][..], &["--from", "xlsx"][..]] {
        officeconv()
            .arg(&folder)
            .args(["--to", "csv"])
            .args(extra)
            .assert()
            .code(66)
            .stderr(contains("input file not found"));
    }
}

#[cfg(unix)]
#[test]
fn reports_a_file_that_cannot_be_opened() {
    use std::os::unix::fs::PermissionsExt;

    // Imported here, not at the top: this test isn't compiled on Windows, where a file-level
    // import would be unused and fail `clippy -D warnings`.
    use predicates::prelude::*;

    let (_dir, path) = sample_xlsx();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Running as root ignores file permissions, so there's nothing to test. Rust has no
    // runtime "skip", so say so: a silent pass would look like a real one.
    if std::fs::File::open(&path).is_ok() {
        eprintln!("skipped: the file is still readable (running as root?)");
        return;
    }

    officeconv()
        .arg(&path)
        .args(["--to", "csv"])
        .assert()
        .code(66)
        .stderr(contains("could not open").and(contains("ermission denied")));
}

#[test]
#[cfg(not(feature = "pdf"))]
fn says_when_pdf_output_is_not_built_in() {
    let (_dir, path) = sample_docx("<w:p/>");
    officeconv()
        .arg(&path)
        .args(["--to", "pdf", "-o", "out.pdf"])
        .assert()
        .code(64)
        .stderr(contains("doesn't include PDF output"));
}

/// Runs officeconv in `dir` with `args`, and checks that it refuses to write over its input:
/// exit 64, naming the `-o` path, with `input` left as it was.
fn refuses_to_overwrite(dir: &std::path::Path, input: &std::path::Path, args: &[&str]) {
    let before = std::fs::read(input).unwrap();
    officeconv()
        .current_dir(dir)
        .args(args)
        .assert()
        .code(64)
        .stderr(contains(
            "is the input file, so converting would replace it",
        ));
    assert_eq!(std::fs::read(input).unwrap(), before, "the input changed");
}

#[test]
fn refuses_to_write_over_its_input() {
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("y.csv");
    std::fs::write(&csv, "a,b\n1,2\n").unwrap();
    refuses_to_overwrite(dir.path(), &csv, &["y.csv", "--to", "md", "-o", "y.csv"]);
    // However the two are spelled.
    refuses_to_overwrite(dir.path(), &csv, &["y.csv", "--to", "csv", "-o", "./y.csv"]);
    let absolute = csv.to_str().unwrap();
    refuses_to_overwrite(dir.path(), &csv, &[absolute, "--to", "json", "-o", "y.csv"]);

    let (dir, docx) = sample_docx("<w:p><w:r><w:t>Hello</w:t></w:r></w:p>");
    let name = docx.file_name().unwrap().to_str().unwrap();
    refuses_to_overwrite(dir.path(), &docx, &[name, "--to", "md", "-o", name]);
}

#[test]
fn refuses_to_write_over_a_workbook_whose_pictures_it_saves() {
    // Writing the Markdown would cut the workbook short before its pictures were read.
    let (dir, book) = crate::images::xlsx_with_pictures();
    refuses_to_overwrite(
        dir.path(),
        &book,
        &[
            "book.xlsx",
            "--to",
            "md",
            "--images",
            "img",
            "-o",
            "book.xlsx",
        ],
    );
    assert!(!dir.path().join("img").exists(), "it saved pictures");
}

#[test]
#[cfg(unix)]
fn refuses_to_write_over_its_input_through_a_link() {
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("y.csv");
    std::fs::write(&csv, "a,b\n1,2\n").unwrap();
    std::os::unix::fs::symlink(&csv, dir.path().join("symlink.md")).unwrap();
    std::fs::hard_link(&csv, dir.path().join("hardlink.md")).unwrap();
    for link in ["symlink.md", "hardlink.md"] {
        refuses_to_overwrite(dir.path(), &csv, &["y.csv", "--to", "md", "-o", link]);
    }
}
