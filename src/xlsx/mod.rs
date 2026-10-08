//! Reading XLSX workbooks into [`Table`]s.

pub mod pictures;

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use zip::result::ZipError;

use calamine::{Data, ExcelDateTime, Reader, Xlsx};

use crate::document::{Block, ImagePart};
use crate::error::{ConvertError, Result};
use crate::opc::{self, Archive, Limits, attr};
use crate::table::{Cell, Table};

const WORKBOOK: &str = "xl/workbook.xml";

/// One worksheet, converted to text.
#[derive(Debug)]
pub struct Sheet {
    pub name: String,
    pub table: Table,
    /// The pictures on the sheet, one paragraph each, in reading order. Empty unless
    /// [`Pictures::Include`] asked for them.
    pub pictures: Vec<Block<ImagePart>>,
}

/// Whether to read the pictures on each sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pictures {
    Include,
    Skip,
}

/// The sheets read from a workbook, and the package their pictures are in.
#[derive(Debug)]
pub struct Workbook<R> {
    pub sheets: Vec<Sheet>,
    /// The workbook's package, open when [`Pictures::Include`] asked for pictures, to resolve
    /// them with [`images::resolve`](crate::images::resolve). Its size limits already count
    /// the drawings read to find them, and go on to count the images.
    pub archive: Option<Archive<R>>,
}

/// Reads one sheet from a workbook.
///
/// With `sheet: None`, reads the first sheet in the workbook.
pub fn read_sheet<R: Read + Seek>(
    reader: R,
    sheet: Option<&str>,
    pictures: Pictures,
) -> Result<Workbook<R>> {
    read_sheet_with_limits(reader, sheet, pictures, Limits::DEFAULT)
}

/// [`read_sheet`], refusing a workbook that decompresses to more than `limits` instead of
/// [`Limits::DEFAULT`].
pub fn read_sheet_with_limits<R: Read + Seek>(
    mut reader: R,
    sheet: Option<&str>,
    pictures: Pictures,
    limits: Limits,
) -> Result<Workbook<R>> {
    check_sizes(&mut reader, limits)?;
    let mut parts = read_sheet_parts(&mut reader, limits)?;
    let loaded = {
        let mut workbook = Xlsx::new(&mut reader)?;
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

        let part = parts.remove(&name);
        vec![(load_sheet(&mut workbook, name)?, part)]
    };
    with_pictures(reader, loaded, pictures, limits)
}

/// Reads every sheet in a workbook, in workbook order.
///
/// A worksheet is read once, under the first name that points at it. Excel never gives one
/// worksheet two names, and the size limits count bytes read, not work: listing one worksheet
/// thousands of times would otherwise write it to thousands of files.
pub fn read_all_sheets<R: Read + Seek>(reader: R, pictures: Pictures) -> Result<Workbook<R>> {
    read_all_sheets_with_limits(reader, pictures, Limits::DEFAULT)
}

/// [`read_all_sheets`], refusing a workbook that decompresses to more than `limits` instead of
/// [`Limits::DEFAULT`]. The fuzz targets use this to pass smaller limits.
pub fn read_all_sheets_with_limits<R: Read + Seek>(
    mut reader: R,
    pictures: Pictures,
    limits: Limits,
) -> Result<Workbook<R>> {
    check_sizes(&mut reader, limits)?;
    let mut parts = read_sheet_parts(&mut reader, limits)?;
    let loaded = {
        let mut workbook = Xlsx::new(&mut reader)?;
        let names = workbook.sheet_names();
        if names.is_empty() {
            return Err(ConvertError::NoSheets);
        }

        let mut read = HashSet::new();
        names
            .into_iter()
            .filter_map(|name| {
                let part = parts.remove(&name);
                if let Some(part) = &part
                    && !read.insert(part.clone())
                {
                    return None;
                }
                Some(load_sheet(&mut workbook, name).map(|sheet| (sheet, part)))
            })
            .collect::<Result<Vec<_>>>()?
    };
    with_pictures(reader, loaded, pictures, limits)
}

/// Finishes reading `loaded` sheets, each with its worksheet part if the workbook says. With
/// [`Pictures::Include`], opens the package once more to find every sheet's pictures, and
/// keeps it open for resolving them.
fn with_pictures<R: Read + Seek>(
    reader: R,
    loaded: Vec<(Sheet, Option<String>)>,
    pictures: Pictures,
    limits: Limits,
) -> Result<Workbook<R>> {
    if pictures == Pictures::Skip {
        let sheets = loaded.into_iter().map(|(sheet, _)| sheet).collect();
        return Ok(Workbook {
            sheets,
            archive: None,
        });
    }

    let mut archive = Archive::with_limits(reader, limits)?;
    let mut sheets = Vec::with_capacity(loaded.len());
    for (mut sheet, part) in loaded {
        if let Some(part) = part {
            sheet.pictures = pictures::sheet_pictures(&mut archive, &part)?;
        }
        sheets.push(sheet);
    }
    Ok(Workbook {
        sheets,
        archive: Some(archive),
    })
}

/// Every sheet's worksheet part, such as `xl/worksheets/sheet1.xml`, by sheet name.
///
/// Reads the workbook and its relationships once. A sheet whose relationship is missing isn't
/// listed.
pub fn sheet_parts<R: Read + Seek>(archive: &mut Archive<R>) -> Result<HashMap<String, String>> {
    let Some(workbook) = archive.read_part(WORKBOOK)? else {
        return Ok(HashMap::new());
    };
    let mut ids = Vec::new();
    opc::visit_elements(&workbook, |e| {
        if e.local_name().as_ref() == "sheet"
            && let (Some(name), Some(id)) = (attr(e, "name"), attr(e, "id"))
        {
            ids.push((name, id));
        }
    })?;

    let relationships = match archive.read_part(&opc::rels_path(WORKBOOK))? {
        Some(xml) => opc::parse_relationships(&xml)?,
        None => return Ok(HashMap::new()),
    };
    Ok(ids
        .into_iter()
        .filter_map(|(name, id)| {
            let target = &relationships.get(&id)?.target;
            Some((name, opc::resolve_target(WORKBOOK, target)))
        })
        .collect())
}

/// [`sheet_parts`] for the package `reader` holds, then rewinds `reader`. A file that isn't a
/// zip archive has none, and is left for calamine to report.
fn read_sheet_parts<R: Read + Seek>(
    reader: &mut R,
    limits: Limits,
) -> Result<HashMap<String, String>> {
    let parts = match Archive::with_limits(&mut *reader, limits) {
        Ok(mut archive) => sheet_parts(&mut archive)?,
        Err(_) => HashMap::new(),
    };
    reader.rewind().map_err(ZipError::from)?;
    Ok(parts)
}

/// Checks every part of the workbook decompresses to within `limits`, then rewinds `reader`.
///
/// calamine has no size limit of its own, so a small workbook could otherwise make it use
/// gigabytes. A file that isn't a zip archive is left for calamine to report.
fn check_sizes<R: Read + Seek>(reader: &mut R, limits: Limits) -> Result<()> {
    if let Ok(mut archive) = Archive::with_limits(&mut *reader, limits) {
        archive.check_part_sizes()?;
    }
    reader.rewind().map_err(ZipError::from)?;
    Ok(())
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
        pictures: Vec::new(),
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
    use std::io::Cursor;

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

    /// A small workbook with two sheets, built in memory.
    ///
    /// The readers take any `Read + Seek`, so a `Cursor` over the bytes works like a file.
    fn sample_workbook() -> Cursor<Vec<u8>> {
        use rust_xlsxwriter::{Format, Workbook};

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

        Cursor::new(workbook.save_to_buffer().unwrap())
    }

    /// Reads the sheet `name`, or the first one, without its pictures.
    fn one_sheet(name: Option<&str>) -> Sheet {
        let mut workbook = read_sheet(sample_workbook(), name, Pictures::Skip).unwrap();
        assert!(workbook.archive.is_none());
        workbook.sheets.remove(0)
    }

    #[test]
    fn reads_first_sheet_by_default() {
        let sheet = one_sheet(None);

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
        let sheet = one_sheet(Some("Notes"));
        assert_eq!(sheet.table.headers, ["Note"]);
    }

    #[test]
    fn reads_all_sheets_in_order() {
        let sheets = read_all_sheets(sample_workbook(), Pictures::Skip)
            .unwrap()
            .sheets;
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Sales", "Notes"]);
        assert_eq!(sheets[1].table.headers, ["Note"]);
    }

    #[test]
    fn unknown_sheet_lists_available_ones() {
        let err = read_sheet(sample_workbook(), Some("Nope"), Pictures::Skip).unwrap_err();
        assert_eq!(
            err.to_string(),
            "sheet \"Nope\" not found; available sheets: Sales, Notes"
        );
    }

    /// A valid 2x2 PNG: `rust_xlsxwriter` reads each image's header.
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2,
        0, 0, 0, 253, 212, 154, 115, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 252, 207, 0, 2, 76,
        96, 146, 1, 0, 13, 29, 1, 3, 130, 201, 113, 255, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ];

    /// Two sheets, the second with a picture.
    fn workbook_with_picture() -> Cursor<Vec<u8>> {
        use rust_xlsxwriter::{Image, Workbook};

        let mut workbook = Workbook::new();
        workbook.add_worksheet().set_name("Plain").unwrap();
        let sales = workbook.add_worksheet().set_name("Sales").unwrap();
        let logo = Image::new_from_buffer(PNG).unwrap().set_alt_text("Logo");
        sales.insert_image(1, 1, &logo).unwrap();
        Cursor::new(workbook.save_to_buffer().unwrap())
    }

    #[test]
    fn reads_each_sheets_pictures_when_asked() {
        let workbook = read_all_sheets(workbook_with_picture(), Pictures::Include).unwrap();
        let logo = Block::Paragraph(vec![crate::document::Run::image(
            ImagePart::new("xl/media/image1.png"),
            "Logo",
        )]);
        let pictures: Vec<_> = workbook.sheets.iter().map(|s| &s.pictures[..]).collect();
        assert_eq!(pictures, [&[][..], &[logo.clone()][..]]);
        assert!(workbook.archive.is_some());

        let mut one =
            read_sheet(workbook_with_picture(), Some("Sales"), Pictures::Include).unwrap();
        assert_eq!(one.sheets.remove(0).pictures, [logo]);
    }

    #[test]
    fn leaves_pictures_unread_unless_asked() {
        let workbook = read_all_sheets(workbook_with_picture(), Pictures::Skip).unwrap();
        assert!(workbook.sheets.iter().all(|s| s.pictures.is_empty()));
        assert!(workbook.archive.is_none());
    }
}
