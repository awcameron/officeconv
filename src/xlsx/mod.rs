//! Reading XLSX workbooks into [`Table`]s.

pub mod pictures;

use std::io::{Read, Seek};
use std::path::Path;

use calamine::{Data, ExcelDateTime, Reader, Xlsx, open_workbook};

use crate::error::{ConvertError, Result};
use crate::table::{Cell, Table};

/// One worksheet, converted to text.
#[derive(Debug)]
pub struct Sheet {
    pub name: String,
    pub table: Table,
}

/// Reads one sheet from the workbook at `path`.
///
/// With `sheet: None`, reads the first sheet in the workbook.
pub fn read_sheet(path: &Path, sheet: Option<&str>) -> Result<Sheet> {
    let mut workbook: Xlsx<_> = open_workbook(path)?;
    let names = workbook.sheet_names();

    let name = match sheet {
        Some(wanted) => names
            .iter()
            .find(|n| n.as_str() == wanted)
            .ok_or_else(|| ConvertError::SheetNotFound {
                name: wanted.to_string(),
                available: names.join(", "),
            })?
            .clone(),
        None => names.first().ok_or(ConvertError::NoSheets)?.clone(),
    };

    load_sheet(&mut workbook, name)
}

/// Reads every sheet in the workbook at `path`, in workbook order.
pub fn read_all_sheets(path: &Path) -> Result<Vec<Sheet>> {
    let mut workbook: Xlsx<_> = open_workbook(path)?;
    let names = workbook.sheet_names();
    if names.is_empty() {
        return Err(ConvertError::NoSheets);
    }

    names
        .into_iter()
        .map(|name| load_sheet(&mut workbook, name))
        .collect()
}

/// Loads the cells of the sheet called `name` from an already-open workbook.
fn load_sheet<R: Read + Seek>(workbook: &mut Xlsx<R>, name: String) -> Result<Sheet> {
    let range = workbook.worksheet_range(&name)?;
    let rows = range
        .rows()
        .map(|row| row.iter().map(cell_value).collect())
        .collect();

    Ok(Sheet {
        name,
        table: Table::from_cells(rows),
    })
}

/// Turns one spreadsheet cell into a [`Cell`], keeping numbers and booleans typed.
fn cell_value(cell: &Data) -> Cell {
    match cell {
        Data::Empty => Cell::Empty,
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
        Data::Int(i) => Cell::Int(*i),
        Data::Float(f) => Cell::Float(*f),
        Data::Bool(b) => Cell::Bool(*b),
        Data::DateTime(dt) => Cell::Text(format_datetime(dt)),
        Data::Error(e) => Cell::Text(e.to_string()),
    }
}

/// The text a cell is written as (in CSV, Markdown and untyped JSON).
#[cfg(test)]
fn cell_to_string(cell: &Data) -> String {
    cell_value(cell).to_string()
}

/// Formats an Excel date/time as ISO 8601, dropping the parts that aren't used.
fn format_datetime(dt: &ExcelDateTime) -> String {
    if dt.is_duration() {
        let total_seconds = (dt.as_f64() * 86_400.0).round() as i64;
        let (h, m, s) = (
            total_seconds / 3600,
            total_seconds % 3600 / 60,
            total_seconds % 60,
        );
        return format!("{h}:{m:02}:{s:02}");
    }

    let (year, month, day, hour, min, sec, _milli) = dt.to_ymd_hms_milli();
    let date = format!("{year:04}-{month:02}-{day:02}");
    let time = format!("{hour:02}:{min:02}:{sec:02}");

    let has_date = dt.as_f64() >= 1.0;
    let has_time = dt.as_f64().fract() != 0.0;
    match (has_date, has_time) {
        (true, false) => date,
        (false, _) => time,
        (true, true) => format!("{date}T{time}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{CellErrorType, ExcelDateTimeType};

    fn datetime(serial: f64) -> Data {
        Data::DateTime(ExcelDateTime::new(
            serial,
            ExcelDateTimeType::DateTime,
            false,
        ))
    }

    #[test]
    fn formats_scalar_cells() {
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::String("hi".into())), "hi");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Float(3.0)), "3");
        assert_eq!(cell_to_string(&Data::Float(2.5)), "2.5");
        assert_eq!(cell_to_string(&Data::Bool(true)), "true");
        assert_eq!(cell_to_string(&Data::Error(CellErrorType::Div0)), "#DIV/0!");
    }

    #[test]
    fn keeps_numbers_and_booleans_typed() {
        assert_eq!(cell_value(&Data::Int(42)), Cell::Int(42));
        assert_eq!(cell_value(&Data::Float(2.5)), Cell::Float(2.5));
        assert_eq!(cell_value(&Data::Bool(true)), Cell::Bool(true));
        assert_eq!(cell_value(&Data::Empty), Cell::Empty);
        assert_eq!(
            cell_value(&datetime(45943.0)),
            Cell::Text("2025-10-13".into())
        );
        assert_eq!(
            cell_value(&Data::Error(CellErrorType::Div0)),
            Cell::Text("#DIV/0!".into())
        );
    }

    #[test]
    fn formats_dates_times_and_durations() {
        // 45943 is 2025-10-13 in Excel's 1900 date system.
        assert_eq!(cell_to_string(&datetime(45943.0)), "2025-10-13");
        assert_eq!(cell_to_string(&datetime(45943.5)), "2025-10-13T12:00:00");
        assert_eq!(cell_to_string(&datetime(0.75)), "18:00:00");

        let duration = ExcelDateTime::new(1.5, ExcelDateTimeType::TimeDelta, false);
        assert_eq!(cell_to_string(&Data::DateTime(duration)), "36:00:00");
    }

    /// Writes a small workbook with two sheets into `dir` and returns its path.
    fn sample_workbook(dir: &Path) -> std::path::PathBuf {
        use rust_xlsxwriter::{Format, Workbook};

        let path = dir.join("sample.xlsx");
        let mut workbook = Workbook::new();
        let date_format = Format::new().set_num_format("yyyy-mm-dd");

        let sales = workbook.add_worksheet().set_name("Sales").unwrap();
        sales
            .write_row(0, 0, ["Region", "Units", "Shipped"])
            .unwrap();
        sales.write(1, 0, "North").unwrap();
        sales.write(1, 1, 12).unwrap();
        let shipped = rust_xlsxwriter::ExcelDateTime::from_ymd(2025, 10, 13).unwrap();
        sales
            .write_with_format(1, 2, &shipped, &date_format)
            .unwrap();
        sales.write(2, 0, "South").unwrap();
        sales.write(2, 1, 7.5).unwrap();

        let notes = workbook.add_worksheet().set_name("Notes").unwrap();
        notes.write(0, 0, "Note").unwrap();

        workbook.save(&path).unwrap();
        path
    }

    #[test]
    fn reads_first_sheet_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let sheet = read_sheet(&sample_workbook(dir.path()), None).unwrap();

        assert_eq!(sheet.name, "Sales");
        assert_eq!(sheet.table.headers, ["Region", "Units", "Shipped"]);
        assert_eq!(
            sheet.table.rows,
            [
                vec![
                    Cell::Text("North".into()),
                    Cell::Float(12.0),
                    Cell::Text("2025-10-13".into())
                ],
                vec![Cell::Text("South".into()), Cell::Float(7.5), Cell::Empty],
            ]
        );
    }

    #[test]
    fn reads_named_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let sheet = read_sheet(&sample_workbook(dir.path()), Some("Notes")).unwrap();
        assert_eq!(sheet.table.headers, ["Note"]);
    }

    #[test]
    fn reads_all_sheets_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let sheets = read_all_sheets(&sample_workbook(dir.path())).unwrap();
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Sales", "Notes"]);
        assert_eq!(sheets[1].table.headers, ["Note"]);
    }

    #[test]
    fn unknown_sheet_lists_available_ones() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_sheet(&sample_workbook(dir.path()), Some("Nope")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "sheet \"Nope\" not found; available sheets: Sales, Notes"
        );
    }
}
