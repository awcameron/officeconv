//! Writes a workbook's sheets: one sheet to stdout or the `-o` file, or with `--all-sheets`
//! each sheet to its own file in a folder. With `--images`, it saves each sheet's pictures
//! too, and Markdown lists them after the sheet's table.

use std::io::{Read, Seek, Write};
use std::path::Path;

use crate::document;
use crate::error::Result;
use crate::images::{self, ImageExport, Images};
use crate::input::Source;
use crate::opc::Archive;
use crate::output::{self, UniqueNames};
use crate::plan::Sheets;
use crate::writers::{self, TableFormat};
use crate::xlsx::{self, Pictures, Sheet, Workbook};

/// Converts the sheets `sheets` picks from the workbook in `source`, saving pictures into the
/// `images` folder if given. One sheet goes to stdout or the `output` file; all sheets go to
/// files named after the input in the `output` folder, or the current one.
///
/// Says what it did on `log`: each file written for all sheets, then how many images were
/// saved and anything it left out.
pub fn convert(
    source: &Source,
    sheets: &Sheets,
    format: TableFormat,
    images: Option<&Path>,
    output: Option<&Path>,
    log: &mut impl Write,
) -> Result<()> {
    let pictures = if images.is_some() {
        Pictures::Include
    } else {
        Pictures::Skip
    };
    let export = match sheets {
        Sheets::One(name) => {
            let workbook = xlsx::read_sheet(source.reader()?, name.as_deref(), pictures)?;
            let mut export = images::export_to(images, output::markdown_dir(output))?;
            write_one(workbook, format, export.as_mut(), output)?;
            export
        }
        Sheets::All => {
            let dir = output.unwrap_or(Path::new("."));
            output::ensure_dir(dir)?;
            let mut export = images::export_to(images, dir)?;
            let workbook = xlsx::read_all_sheets(source.reader()?, pictures)?;
            write_all(workbook, format, export.as_mut(), dir, &source.stem(), log)?;
            export
        }
    };
    if let Some(export) = &export {
        export.report(log)?;
    }
    Ok(())
}

/// Writes the sheets read to stdout or the `output` file, one after another.
fn write_one<R: Read + Seek>(
    workbook: Workbook<R>,
    format: TableFormat,
    mut export: Option<&mut ImageExport>,
    output: Option<&Path>,
) -> Result<()> {
    let Workbook {
        sheets,
        mut archive,
    } = workbook;
    let mut out = output::open_output(output)?;
    for sheet in sheets {
        write_sheet(
            sheet,
            format,
            archive.as_mut().zip(export.as_deref_mut()),
            &mut out,
        )?;
    }
    // BufWriter flushes on drop but ignores errors there, so flush explicitly.
    out.flush()?;
    Ok(())
}

/// Writes each sheet to `<dir>/<stem>-<sheet>.<ext>`, numbering names that clash.
fn write_all<R: Read + Seek>(
    workbook: Workbook<R>,
    format: TableFormat,
    mut export: Option<&mut ImageExport>,
    dir: &Path,
    stem: &str,
    log: &mut impl Write,
) -> Result<()> {
    let Workbook {
        sheets,
        mut archive,
    } = workbook;
    let mut names = UniqueNames::default();
    for sheet in sheets {
        let path = output::sheet_output_path(dir, stem, &sheet.name, format, &mut names);
        let mut out = output::open_output(Some(&path))?;
        write_sheet(
            sheet,
            format,
            archive.as_mut().zip(export.as_deref_mut()),
            &mut out,
        )?;
        out.flush()?;
        writeln!(log, "wrote {}", path.display())?;
    }
    Ok(())
}

/// Writes a sheet's table, then saves its pictures through `images` if asked to.
///
/// Markdown lists the pictures after the table. CSV, TSV and JSON have no way to point at an
/// image, so for those the files are saved and the data is left as it is.
fn write_sheet<R: Read + Seek>(
    sheet: Sheet,
    format: TableFormat,
    images: Option<(&mut Archive<R>, &mut ImageExport)>,
    out: &mut impl Write,
) -> Result<()> {
    writers::write_table(&sheet.table, format, &mut *out)?;

    let Some((archive, export)) = images else {
        return Ok(());
    };
    let pictures = images::resolve(sheet.pictures, archive, Images::Save(export))?;
    if format == TableFormat::Markdown && !pictures.is_empty() {
        if !sheet.table.is_empty() {
            writeln!(out)?;
        }
        out.write_all(document::markdown::render(&pictures).as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writers::JsonValues;
    use crate::xlsx::tests::PNG;
    use rust_xlsxwriter::{Image, Workbook};
    use std::fs;
    use tempfile::TempDir;

    /// A workbook on stdin with a sheet per name, each holding its name in A1, and a picture on
    /// the sheets in `with_picture`.
    fn source(names: &[&str], with_picture: &[&str]) -> Source {
        let mut workbook = Workbook::new();
        for &name in names {
            let sheet = workbook.add_worksheet().set_name(name).unwrap();
            sheet.write(0, 0, name).unwrap();
            if with_picture.contains(&name) {
                let logo = Image::new_from_buffer(PNG).unwrap().set_alt_text("Logo");
                sheet.insert_image(2, 2, &logo).unwrap();
            }
        }
        Source::Stdin(workbook.save_to_buffer().unwrap())
    }

    /// Converts `source`, returning what it said on its log.
    fn convert_to(
        source: &Source,
        sheets: Sheets,
        format: TableFormat,
        images: Option<&Path>,
        output: &Path,
    ) -> String {
        let mut log = Vec::new();
        convert(source, &sheets, format, images, Some(output), &mut log).unwrap();
        String::from_utf8(log).unwrap()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn all_sheets_go_to_files_named_after_the_input_and_sheet() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("out");
        let source = source(&["Sales", "Q1 Notes"], &[]);
        let log = convert_to(&source, Sheets::All, TableFormat::Csv, None, &out);

        assert_eq!(files(&out), ["stdin-Q1 Notes.csv", "stdin-Sales.csv"]);
        assert_eq!(
            fs::read_to_string(out.join("stdin-Sales.csv")).unwrap(),
            "Sales\n"
        );
        assert_eq!(
            log,
            format!(
                "wrote {}\nwrote {}\n",
                out.join("stdin-Sales.csv").display(),
                out.join("stdin-Q1 Notes.csv").display()
            )
        );

        let json = convert_to(
            &source,
            Sheets::All,
            TableFormat::Json(JsonValues::Text),
            None,
            &out,
        );
        assert!(json.contains("stdin-Sales.json"));
    }

    #[test]
    fn sheet_names_that_make_the_same_file_name_are_numbered() {
        let dir = TempDir::new().unwrap();
        // `|` can't be in a file name, so both sheets would be `stdin-a_b.md`.
        let source = source(&["a|b", "a_b"], &[]);
        convert_to(
            &source,
            Sheets::All,
            TableFormat::Markdown,
            None,
            dir.path(),
        );

        assert_eq!(files(dir.path()), ["stdin-a_b-2.md", "stdin-a_b.md"]);
        // The first sheet keeps the name; Markdown escapes each sheet's text in A1.
        let first = fs::read_to_string(dir.path().join("stdin-a_b.md")).unwrap();
        let second = fs::read_to_string(dir.path().join("stdin-a_b-2.md")).unwrap();
        assert!(first.contains(r"a\|b"), "{first}");
        assert!(second.contains(r"a\_b"), "{second}");
    }

    #[test]
    fn markdown_lists_a_sheets_pictures_after_its_table() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("images");
        let output = dir.path().join("sales.md");
        let source = source(&["Plain", "Sales"], &["Sales"]);
        let log = convert_to(
            &source,
            Sheets::One(Some("Sales".into())),
            TableFormat::Markdown,
            Some(&images),
            &output,
        );

        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "| Sales |\n| ----- |\n\n![Logo](images/image1.png)\n"
        );
        assert_eq!(files(&images), ["image1.png"]);
        assert_eq!(log, format!("saved 1 images to {}\n", images.display()));
    }

    #[test]
    fn csv_saves_a_sheets_pictures_without_pointing_at_them() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("images");
        let source = source(&["Sales"], &["Sales"]);
        convert_to(
            &source,
            Sheets::All,
            TableFormat::Csv,
            Some(&images),
            dir.path(),
        );

        assert_eq!(files(&images), ["image1.png"]);
        assert_eq!(
            fs::read_to_string(dir.path().join("stdin-Sales.csv")).unwrap(),
            "Sales\n"
        );
    }
}
