//! The `officeconv` command-line tool.
//!
//! This crate is a binary, not a library to depend on. The library half holds the code so the
//! fuzz targets in `fuzz/` can link against it; it has no public API, and any of it may change
//! in any release. See `docs/adr/0003-binary-only.md`.

mod cli;
mod delimited;
mod document;
mod docx;
mod error;
mod format;
mod images;
mod input;
mod markdown;
mod opc;
mod output;
#[cfg(feature = "pdf")]
mod pdf;
mod plan;
mod pptx;
mod table;
mod writers;
mod xlsx;

/// The readers and writers the fuzz targets in `fuzz/` call directly, re-exported for them.
/// Only built with the `fuzzing` feature, and as unstable as the rest of the crate.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::delimited::{
        Limits as DelimitedLimits, read_table_with_limits as read_delimited,
    };
    pub use crate::document::Block;
    pub use crate::document::markdown::render as render_markdown;
    pub use crate::docx::read_blocks as read_docx;
    pub use crate::images::{EmbeddedImages, Images, resolve as resolve_images};
    pub use crate::input::InputKind;
    pub use crate::opc::{Archive, Limits, parse_relationships};
    #[cfg(feature = "pdf")]
    pub use crate::pdf::{layout::PageSetup, render as render_pdf};
    pub use crate::pptx::{Notes, read_blocks as read_pptx};
    pub use crate::writers::{JsonValues, TableFormat, write_table};
    pub use crate::xlsx::{Pictures, read_all_sheets_with_limits as read_xlsx};
}

use std::io::{self, ErrorKind, IsTerminal, Read, Seek, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use error::{ConvertError, Result};
use images::{EmbeddedImages, ImageExport, Images};
use input::{InputKind, Source};
use plan::{DocumentOutput, DocumentReader, Pages, Plan, Sheets};
use writers::TableFormat;

/// The `officeconv` command: parses the arguments, converts, and reports any error.
///
/// The binary is all `main.rs` holds, so this is the library's only public item. It's hidden
/// from the docs because it isn't an API to build on: see the crate docs.
#[doc(hidden)]
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader went away (e.g. `officeconv big.xlsx --to csv | head`); that's not a failure.
        Err(ConvertError::Write(err)) if err.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            // Each error message already includes its cause, so print just that one line.
            eprintln!("Error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

/// Plans the conversion the options ask for, then carries it out.
fn run(cli: &Cli) -> Result<()> {
    plan::check_options(cli)?;
    let source = Source::from_arg(&cli.input)?;
    let kind = source.kind(cli.from)?;
    let output = cli.output.as_deref();
    match plan::plan(cli, kind, io::stdout().is_terminal())? {
        Plan::Delimited { kind, format } => convert_delimited(&source, kind, format, output),
        Plan::Workbook {
            sheets: Sheets::All,
            format,
            images,
        } => convert_all_sheets(&source, format, images.as_deref(), output),
        Plan::Workbook {
            sheets: Sheets::One(name),
            format,
            images,
        } => convert_one_sheet(&source, name.as_deref(), format, images.as_deref(), output),
        Plan::Document { reader, output: to } => convert_document(&source, reader, to, output),
    }
}

/// Converts a CSV or TSV file, as one table, to stdout or the `output` file.
fn convert_delimited(
    source: &Source,
    kind: InputKind,
    format: TableFormat,
    output: Option<&Path>,
) -> Result<()> {
    let table = delimited::read_table(source.reader()?, kind)?;
    let mut out = output::open_output(output)?;
    writers::write_table(&table, format, &mut out)?;
    out.flush()?;
    Ok(())
}

/// Converts a Word or PowerPoint file to Markdown (saving its images if asked to) or to PDF.
fn convert_document(
    source: &Source,
    reader: DocumentReader,
    to: DocumentOutput,
    output: Option<&Path>,
) -> Result<()> {
    let mut export = match &to {
        DocumentOutput::Markdown { images } => {
            image_export(images.as_deref(), output::markdown_dir(output))?
        }
        DocumentOutput::Pdf(_) => None,
    };
    let mut embedded = EmbeddedImages::default();
    let images = match (&to, export.as_mut()) {
        (DocumentOutput::Pdf(_), _) => Images::Embed(&mut embedded),
        (_, Some(export)) => Images::Save(export),
        (_, None) => Images::Skip,
    };

    let mut archive = opc::Archive::open(source.reader()?)?;
    let mut blocks = match reader {
        DocumentReader::Pptx(notes) => pptx::read_blocks(&mut archive, notes)?,
        DocumentReader::Docx => docx::read_blocks(&mut archive)?,
    };
    if let DocumentOutput::Markdown { .. } = to {
        // Markdown leaves headers and footers out, so `--images` mustn't save their pictures.
        blocks.retain(|block| !block.is_page_furniture());
    }
    let blocks = images::resolve(blocks, &mut archive, images)?;

    if let DocumentOutput::Pdf(pages) = to {
        return write_pdf(pages, &blocks, &embedded, output);
    }

    write_output(output, document::markdown::render(&blocks).as_bytes())?;
    if let Some(export) = &export {
        export.report();
    }
    Ok(())
}

/// Writes a Word document as A4 pages, or a deck as one landscape page per slide. Image runs
/// hold keys into `images`.
#[cfg(feature = "pdf")]
fn write_pdf(
    pages: Pages,
    blocks: &[document::Block],
    images: &EmbeddedImages,
    output: Option<&Path>,
) -> Result<()> {
    let setup = match pages {
        Pages::Slides => pdf::layout::PageSetup::SLIDES,
        Pages::Document => pdf::layout::PageSetup::DOCUMENT,
    };
    let rendered = pdf::render(blocks, images, setup)?;
    write_output(output, &rendered.pdf)?;
    report_pdf_gaps(&rendered);
    Ok(())
}

/// Warns about what couldn't go into the PDF: characters no font has, and images in formats
/// a PDF can't hold.
#[cfg(feature = "pdf")]
fn report_pdf_gaps(rendered: &pdf::Rendered) {
    const SHOWN: usize = 10;
    let missing = &rendered.missing_chars;
    if !missing.is_empty() {
        let mut listed: Vec<String> = missing
            .iter()
            .take(SHOWN)
            .map(|c| format!("{c} (U+{:04X})", u32::from(*c)))
            .collect();
        if missing.len() > SHOWN {
            listed.push(format!("and {} more", missing.len() - SHOWN));
        }
        eprintln!(
            "warning: no installed font has these characters, so they show as boxes: {}",
            listed.join(", ")
        );
    }
    let skipped = rendered.skipped_images;
    if skipped.unsupported > 0 {
        eprintln!(
            "warning: left out {} images in formats a PDF can't hold (such as EMF or TIFF)",
            skipped.unsupported
        );
    }
    if skipped.too_large > 0 {
        eprintln!(
            "warning: left out {} images larger than {} megapixels",
            skipped.too_large,
            pdf::layout::MAX_IMAGE_PIXELS / 1_000_000
        );
    }
}

#[cfg(not(feature = "pdf"))]
fn write_pdf(
    _pages: Pages,
    _blocks: &[document::Block],
    _images: &EmbeddedImages,
    _output: Option<&Path>,
) -> Result<()> {
    Err(ConvertError::PdfNotBuilt)
}

/// Writes the converted document to stdout or the `output` file.
fn write_output(output: Option<&Path>, bytes: &[u8]) -> Result<()> {
    let mut out = output::open_output(output)?;
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

/// Converts one sheet, by name or the first, to stdout or the `output` file.
fn convert_one_sheet(
    source: &Source,
    name: Option<&str>,
    format: TableFormat,
    images: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let workbook = xlsx::read_sheet(source.reader()?, name, sheet_pictures(images))?;
    let mut export = image_export(images, output::markdown_dir(output))?;
    let xlsx::Workbook {
        sheets,
        mut archive,
    } = workbook;

    let mut out = output::open_output(output)?;
    for sheet in sheets {
        write_sheet(
            sheet,
            format,
            archive.as_mut().zip(export.as_mut()),
            &mut out,
        )?;
    }
    // BufWriter flushes on drop but ignores errors there, so flush explicitly.
    out.flush()?;

    if let Some(export) = &export {
        export.report();
    }
    Ok(())
}

/// Writes every sheet to its own file, in the `output` directory or the current one.
fn convert_all_sheets(
    source: &Source,
    format: TableFormat,
    images: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let dir = output.unwrap_or(Path::new("."));
    output::ensure_dir(dir)?;
    let mut export = image_export(images, dir)?;
    let stem = source.stem();
    let mut names = output::UniqueNames::default();

    let xlsx::Workbook {
        sheets,
        mut archive,
    } = xlsx::read_all_sheets(source.reader()?, sheet_pictures(images))?;
    for sheet in sheets {
        let path = output::sheet_output_path(dir, &stem, &sheet.name, format, &mut names);
        let mut out = output::open_output(Some(&path))?;
        write_sheet(
            sheet,
            format,
            archive.as_mut().zip(export.as_mut()),
            &mut out,
        )?;
        out.flush()?;
        eprintln!("wrote {}", path.display());
    }

    if let Some(export) = &export {
        export.report();
    }
    Ok(())
}

/// Pictures are read from a workbook only to save them, with `--images`.
fn sheet_pictures(images: Option<&Path>) -> xlsx::Pictures {
    if images.is_some() {
        xlsx::Pictures::Include
    } else {
        xlsx::Pictures::Skip
    }
}

/// Where `--images` saves images, linked from Markdown in `markdown_dir`, or `None` without it.
fn image_export(images: Option<&Path>, markdown_dir: &Path) -> Result<Option<ImageExport>> {
    images
        .map(|dir| ImageExport::new(dir, markdown_dir))
        .transpose()
}

/// Writes a sheet's table, then saves its pictures through `images` if asked to.
///
/// Markdown lists the pictures after the table. CSV, TSV and JSON have no way to point at an
/// image, so for those the files are saved and the data is left as it is.
fn write_sheet<R: Read + Seek>(
    sheet: xlsx::Sheet,
    format: TableFormat,
    images: Option<(&mut opc::Archive<R>, &mut ImageExport)>,
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
