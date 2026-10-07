//! CSV and TSV input: each output format, quoting, stdin, encoding, ragged rows, and the
//! options that don't apply.

use std::path::PathBuf;

use predicates::str::contains;
use tempfile::TempDir;

use crate::common::*;

/// Writes `contents` to a file called `name` in a fresh temp dir.
fn write_file(name: &str, contents: impl AsRef<[u8]>) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

/// A CSV file, `contacts.csv`, with quoted fields holding a comma, quotes and a line break, and
/// Windows line endings.
fn sample_csv() -> (TempDir, PathBuf) {
    write_file(
        "contacts.csv",
        "Name,Note\r\nAda,\"Hello, world\"\r\n\"Alan \"\"AT\"\" Turing\",\"two\nlines\"\r\n",
    )
}

#[test]
fn converts_csv_to_every_table_format() {
    let (_dir, path) = sample_csv();
    assert_eq!(
        convert(&path, "md"),
        r#"| Name             | Note         |
| ---------------- | ------------ |
| Ada              | Hello, world |
| Alan "AT" Turing | two<br>lines |
"#
    );
    assert_eq!(
        convert(&path, "json"),
        r#"[
  {
    "Name": "Ada",
    "Note": "Hello, world"
  },
  {
    "Name": "Alan \"AT\" Turing",
    "Note": "two\nlines"
  }
]
"#
    );
    assert_eq!(
        convert(&path, "csv"),
        "Name,Note\nAda,\"Hello, world\"\n\"Alan \"\"AT\"\" Turing\",\"two\nlines\"\n"
    );
    assert_eq!(
        convert(&path, "tsv"),
        "Name\tNote\nAda\tHello, world\n\"Alan \"\"AT\"\" Turing\"\t\"two\nlines\"\n"
    );
}

#[test]
fn reads_tsv() {
    let (_dir, path) = write_file("export.tsv", "City\tPopulation\nOslo\t\"709,037\"\n");
    assert_eq!(
        convert(&path, "json"),
        "[\n  {\n    \"City\": \"Oslo\",\n    \"Population\": \"709,037\"\n  }\n]\n"
    );
}

#[test]
fn reads_csv_from_stdin_with_from() {
    officeconv()
        .args(["-", "--to", "md", "--from", "csv"])
        .write_stdin("a,b\n1,2\n")
        .assert()
        .success()
        .stdout("| a   | b   |\n| --- | --- |\n| 1   | 2   |\n");

    officeconv()
        .args(["-", "--to", "csv", "--from", "tsv"])
        .write_stdin("a\tb\n1,5\t2\n")
        .assert()
        .success()
        .stdout("a,b\n\"1,5\",2\n");
}

#[test]
fn csv_on_stdin_without_from_says_to_use_it() {
    officeconv()
        .args(["-", "--to", "md"])
        .write_stdin("a,b\n1,2\n")
        .assert()
        .code(65)
        .stderr(contains("such as --from csv for CSV"));
}

#[test]
fn from_csv_reads_a_file_with_another_extension() {
    let (_dir, path) = write_file("export.txt", "a,b\n1,2\n");
    officeconv()
        .arg(&path)
        .args(["--to", "json", "--from", "csv"])
        .assert()
        .success()
        .stdout("[\n  {\n    \"a\": \"1\",\n    \"b\": \"2\"\n  }\n]\n");
}

#[test]
fn from_csv_refuses_an_office_file() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "csv", "--from", "csv"])
        .assert()
        .code(65)
        .stderr(contains(
            "the input isn't a .csv file (it looks like a .xlsx)",
        ));
}

#[test]
fn skips_a_byte_order_mark() {
    let (_dir, path) = write_file("excel.csv", "\u{FEFF}Name,City\nAda,London\n");
    assert_eq!(convert(&path, "csv"), "Name,City\nAda,London\n");
}

#[test]
fn reports_invalid_utf8_and_bad_quoting_with_the_line() {
    let (_dir, path) = write_file("latin1.csv", b"Name\nAda\nJos\xe9\n");
    officeconv()
        .arg(&path)
        .args(["--to", "md"])
        .assert()
        .code(65)
        .stderr("Error: could not read the csv input: line 3: invalid UTF-8\n");

    let (_dir, path) = write_file("open.csv", "a,b\n1,\"never closed\n2,3\n");
    officeconv()
        .arg(&path)
        .args(["--to", "md"])
        .assert()
        .code(65)
        .stderr("Error: could not read the csv input: line 2: a quoted field is never closed\n");

    let (_dir, path) = write_file("glued.tsv", "a\tb\n\"x\"y\t2\n");
    officeconv()
        .arg(&path)
        .args(["--to", "md"])
        .assert()
        .code(65)
        .stderr("Error: could not read the tsv input: line 2: text after a closing quote\n");
}

#[test]
fn keeps_values_past_the_header() {
    let (_dir, path) = write_file("ragged.csv", "a,b\n1,2,3\n4\n");
    assert_eq!(
        convert(&path, "json"),
        r#"[
  {
    "a": "1",
    "b": "2",
    "column_3": "3"
  },
  {
    "a": "4",
    "b": "",
    "column_3": ""
  }
]
"#
    );
    assert_eq!(convert(&path, "csv"), "a,b,\n1,2,3\n4,,\n");
}

#[test]
fn rejects_typed_for_csv() {
    let (_dir, path) = sample_csv();
    officeconv()
        .arg(&path)
        .args(["--to", "json", "--typed"])
        .assert()
        .code(64)
        .stderr(contains("--typed doesn't apply to .csv or .tsv input"));
}

#[test]
fn rejects_options_that_do_not_apply_to_csv() {
    let (dir, path) = sample_csv();
    // Without the `pdf` feature, --to pdf stops before the input is looked at.
    let pdf_message = if cfg!(feature = "pdf") {
        "cannot convert csv to pdf; csv supports: csv, tsv, json, md"
    } else {
        "doesn't include PDF output"
    };
    let cases: [(&[&str], &str); 5] = [
        (&["--to", "pdf", "-o", "out.pdf"], pdf_message),
        (
            &["--to", "csv", "--sheet", "Sheet1"],
            "--sheet and --all-sheets only apply to .xlsx input",
        ),
        (
            &["--to", "csv", "--all-sheets"],
            "--sheet and --all-sheets only apply to .xlsx input",
        ),
        (
            &["--to", "md", "--no-notes"],
            "--no-notes only applies to .pptx",
        ),
        (
            &["--to", "md", "--images", "img"],
            "--images doesn't apply to .csv or .tsv input: it has no images",
        ),
    ];
    for (args, message) in cases {
        officeconv()
            .current_dir(dir.path())
            .arg(&path)
            .args(args)
            .assert()
            .code(64)
            .stderr(contains(message));
    }
    // Nothing was written for any of them.
    assert!(!dir.path().join("out.pdf").exists());
    assert!(!dir.path().join("img").exists());
}
