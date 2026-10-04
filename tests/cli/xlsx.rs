//! Spreadsheets to CSV, TSV, JSON (plain and `--typed`) and Markdown, one sheet or all.

use predicates::prelude::*;
use predicates::str::contains;
use std::path::PathBuf;
use tempfile::TempDir;

use crate::common::*;

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
fn markdown_shows_cell_text_literally() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("notes.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    let rows = [
        ["<b>Name</b>", "Note"],
        ["Ann", "<img src=x onerror=alert(1)>"],
        ["Bob", "&copy; *not italic* Q&A"],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            sheet.write(r as u32, c as u16, *text).unwrap();
        }
    }
    workbook.save(&input).unwrap();

    // `<` and entities would otherwise be live HTML, and `*` would be formatting.
    assert_eq!(
        convert(&input, "md"),
        "\
| &lt;b>Name&lt;/b> | Note                            |
| ----------------- | ------------------------------- |
| Ann               | &lt;img src=x onerror=alert(1)> |
| Bob               | &amp;copy; \\*not italic\\* Q&A   |
"
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
fn all_sheets_keeps_sheets_whose_file_names_clash() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("report.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    // `|` becomes `_` and a trailing dot is dropped, so each pair makes the same file name.
    for (name, text) in [("a|b", "1"), ("a_b", "2"), ("Notes", "3"), ("Notes.", "4")] {
        workbook
            .add_worksheet()
            .set_name(name)
            .unwrap()
            .write(0, 0, text)
            .unwrap();
    }
    workbook.save(&input).unwrap();
    let out_dir = dir.path().join("out");

    officeconv()
        .arg(&input)
        .args(["--to", "csv", "--all-sheets", "-o"])
        .arg(&out_dir)
        .assert()
        .success()
        .stderr(contains("report-a_b-2.csv").and(contains("report-Notes-2.csv")));

    for (file, text) in [
        ("report-a_b.csv", "1\n"),
        ("report-a_b-2.csv", "2\n"),
        ("report-Notes.csv", "3\n"),
        ("report-Notes-2.csv", "4\n"),
    ] {
        assert_eq!(std::fs::read_to_string(out_dir.join(file)).unwrap(), text);
    }
}

#[test]
fn all_sheets_keeps_sheets_that_differ_only_in_unicode_normalization() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("report.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    // Both display as "Café"; macOS treats the two file names as the same file.
    let (composed, decomposed) = ("Caf\u{e9}", "Cafe\u{301}");
    for (name, text) in [(composed, "1"), (decomposed, "2")] {
        workbook
            .add_worksheet()
            .set_name(name)
            .unwrap()
            .write(0, 0, text)
            .unwrap();
    }
    workbook.save(&input).unwrap();
    let out_dir = dir.path().join("out");

    officeconv()
        .arg(&input)
        .args(["--to", "csv", "--all-sheets", "-o"])
        .arg(&out_dir)
        .assert()
        .success();

    for (file, text) in [
        (format!("report-{composed}.csv"), "1\n"),
        (format!("report-{decomposed}-2.csv"), "2\n"),
    ] {
        assert_eq!(std::fs::read_to_string(out_dir.join(file)).unwrap(), text);
    }
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
