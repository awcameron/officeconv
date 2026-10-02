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
    let (_dir, path) = touch("slides.pptx");
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
