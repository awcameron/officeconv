//! Choosing the input: stdin, `--from`, and working out the type from the contents.

use predicates::str::contains;

use crate::common::*;

#[test]
fn reads_xlsx_from_stdin() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .args(["-", "--to", "csv"])
        .write_stdin(std::fs::read(&path).unwrap())
        .assert()
        .success()
        .stdout(convert(&path, "csv"));
}

#[test]
fn reads_docx_and_pptx_from_stdin() {
    let (_dir, docx) = sample_docx(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Piped</w:t></w:r></w:p>"#,
    );
    officeconv()
        .args(["-", "--to", "md"])
        .write_stdin(std::fs::read(&docx).unwrap())
        .assert()
        .success()
        .stdout("# Piped\n");

    let (_dir, pptx) = sample_pptx();
    officeconv()
        .args(["-", "--to", "md", "--no-notes"])
        .write_stdin(std::fs::read(&pptx).unwrap())
        .assert()
        .success()
        .stdout(predicates::str::starts_with("## Slide 1: Agenda\n"));
}

#[test]
fn names_all_sheets_files_after_stdin() {
    let (dir, path) = sample_xlsx();
    let out = dir.path().join("out");

    officeconv()
        .args(["-", "--to", "json", "--typed", "--all-sheets", "-o"])
        .arg(&out)
        .write_stdin(std::fs::read(&path).unwrap())
        .assert()
        .success()
        .stderr(contains("stdin-Sheet1.json"));
    assert!(out.join("stdin-Sheet1.json").is_file());
}

#[test]
fn reports_empty_or_unrecognized_stdin() {
    officeconv()
        .args(["-", "--to", "md"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(contains("stdin is empty"));

    officeconv()
        .args(["-", "--to", "md"])
        .write_stdin("just some text")
        .assert()
        .failure()
        .stderr(contains("could not tell what kind of file is on stdin"));
}

#[test]
fn rejects_typed_before_reading_stdin() {
    // Not valid input: if stdin were read first, this would fail with a different error.
    officeconv()
        .args(["-", "--to", "csv", "--typed"])
        .write_stdin("not an office file")
        .assert()
        .failure()
        .stderr(contains("--typed only applies to --to json"));
}

#[test]
fn detects_the_type_of_a_file_with_an_unknown_extension() {
    let (dir, path) = sample_xlsx();
    let renamed = dir.path().join("export.zip");
    std::fs::rename(&path, &renamed).unwrap();

    for extra in [&[][..], &["--from", "xlsx"][..]] {
        officeconv()
            .arg(&renamed)
            .args(["--to", "csv"])
            .args(extra)
            .assert()
            .success()
            .stdout(predicates::str::starts_with("Region,Units\n"));
    }
}

#[test]
fn from_overrides_a_misleading_extension() {
    // A workbook saved with a .docx name: the extension is trusted unless --from says otherwise.
    let (dir, path) = sample_xlsx();
    let misnamed = dir.path().join("report.docx");
    std::fs::rename(&path, &misnamed).unwrap();

    officeconv()
        .arg(&misnamed)
        .args(["--to", "md"])
        .assert()
        .failure()
        .stderr(contains("could not read document"));
    officeconv()
        .arg(&misnamed)
        .args(["--to", "csv", "--from", "xlsx"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with("Region,Units\n"));
}

#[test]
fn rejects_a_from_that_does_not_match() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .args(["-", "--to", "md", "--from", "docx"])
        .write_stdin(std::fs::read(&path).unwrap())
        .assert()
        .failure()
        .stderr(contains(
            "the input isn't a .docx file (it looks like a .xlsx)",
        ));
}
