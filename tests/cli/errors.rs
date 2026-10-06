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
        .stderr(contains("could not read document"));
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
