//! End-to-end tests: run the real binary and check its output.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn officeconv() -> Command {
    Command::cargo_bin("officeconv").unwrap()
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
