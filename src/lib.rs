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
    pub use crate::format::OutputFormat;
    pub use crate::images::{EmbeddedImages, Images, resolve as resolve_images};
    pub use crate::input::InputKind;
    pub use crate::opc::{Archive, Limits, parse_relationships};
    #[cfg(feature = "pdf")]
    pub use crate::pdf::{layout::PageSetup, render as render_pdf};
    pub use crate::pptx::{Notes, read_blocks as read_pptx};
    pub use crate::writers::{JsonValues, write_table};
    pub use crate::xlsx::pictures::read_pictures;
    pub use crate::xlsx::read_all_sheets_with_limits as read_xlsx;
}

use std::io::{self, ErrorKind, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use error::{ConvertError, Result};
use format::OutputFormat;
use images::{EmbeddedImages, ImageExport, Images};
use input::{InputKind, ReadSeek, Source};
use writers::JsonValues;

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

/// Validates the request and runs the matching converter.
fn run(cli: &Cli) -> Result<()> {
    // Checks that don't depend on the input come first, so a slow pipe on stdin doesn't
    // have to finish before a simple usage mistake is reported.
    if cli.typed && cli.to != OutputFormat::Json {
        return Err(ConvertError::TypedOnlyForJson);
    }
    if cli.to == OutputFormat::Pdf && !cfg!(feature = "pdf") {
        return Err(ConvertError::PdfNotBuilt);
    }

    let source = Source::from_arg(&cli.input)?;
    let kind = source.kind(cli.from)?;
    kind.check_output(cli.to)?;

    if kind != InputKind::Xlsx && (cli.sheet.is_some() || cli.all_sheets) {
        return Err(ConvertError::SheetOptionOnlyForXlsx);
    }
    if kind != InputKind::Pptx && cli.no_notes {
        return Err(ConvertError::NotesOptionOnlyForPptx);
    }
    if matches!(kind, InputKind::Csv | InputKind::Tsv) {
        // Reading types from text would guess wrong too often: `00123` would lose its zeros.
        if cli.typed {
            return Err(ConvertError::OptionNotForDelimited {
                option: "--typed",
                reason: "it holds only text, with no numbers or booleans to keep",
            });
        }
        if cli.images.is_some() {
            return Err(ConvertError::OptionNotForDelimited {
                option: "--images",
                reason: "it has no images",
            });
        }
    }
    if cli.to == OutputFormat::Pdf {
        if cli.images.is_some() {
            return Err(ConvertError::ImagesWithPdf);
        }
        if cli.output.is_none() && io::stdout().is_terminal() {
            return Err(ConvertError::PdfToTerminal);
        }
    }

    match kind {
        InputKind::Xlsx if cli.all_sheets => convert_all_sheets(cli, &source),
        InputKind::Xlsx => convert_one_sheet(cli, &source),
        InputKind::Docx | InputKind::Pptx => convert_document(cli, &source, kind),
        InputKind::Csv | InputKind::Tsv => convert_delimited(cli, &source, kind),
    }
}

/// Converts a CSV or TSV file, as one table, to stdout or the `-o` file.
fn convert_delimited(cli: &Cli, source: &Source, kind: InputKind) -> Result<()> {
    let table = delimited::read_table(source.reader()?, kind)?;
    let mut out = output::open_output(cli.output.as_deref())?;
    writers::write_table(&table, cli.to, JsonValues::Text, &mut out)?;
    out.flush()?;
    Ok(())
}

/// Converts a Word or PowerPoint file to Markdown (saving its images if asked to) or to PDF.
fn convert_document(cli: &Cli, source: &Source, kind: InputKind) -> Result<()> {
    let mut export = match &cli.images {
        Some(dir) => Some(ImageExport::new(
            dir,
            output::markdown_dir(cli.output.as_deref()),
        )?),
        None => None,
    };
    let mut embedded = EmbeddedImages::default();
    let images = match (cli.to, export.as_mut()) {
        (OutputFormat::Pdf, _) => Images::Embed(&mut embedded),
        (_, Some(export)) => Images::Save(export),
        (_, None) => Images::Skip,
    };

    let mut archive = opc::Archive::open(source.reader()?)?;
    let blocks = if kind == InputKind::Pptx {
        let notes = if cli.no_notes {
            pptx::Notes::Skip
        } else {
            pptx::Notes::Include
        };
        pptx::read_blocks(&mut archive, notes)?
    } else {
        docx::read_blocks(&mut archive)?
    };
    let blocks = images::resolve(blocks, &mut archive, images)?;

    if cli.to == OutputFormat::Pdf {
        return write_pdf(cli, kind, &blocks, &embedded);
    }

    write_output(cli, document::markdown::render(&blocks).as_bytes())?;
    if let Some(export) = &export {
        export.report();
    }
    Ok(())
}

/// Writes a Word document as A4 pages, or a deck as one landscape page per slide. Image runs
/// hold keys into `images`.
#[cfg(feature = "pdf")]
fn write_pdf(
    cli: &Cli,
    kind: InputKind,
    blocks: &[document::Block],
    images: &EmbeddedImages,
) -> Result<()> {
    let setup = match kind {
        InputKind::Pptx => pdf::layout::PageSetup::SLIDES,
        _ => pdf::layout::PageSetup::DOCUMENT,
    };
    let rendered = pdf::render(blocks, images, setup)?;
    write_output(cli, &rendered.pdf)?;
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
    _cli: &Cli,
    _kind: InputKind,
    _blocks: &[document::Block],
    _images: &EmbeddedImages,
) -> Result<()> {
    Err(ConvertError::PdfNotBuilt)
}

/// Writes the converted document to stdout or the `-o` file.
fn write_output(cli: &Cli, bytes: &[u8]) -> Result<()> {
    let mut out = output::open_output(cli.output.as_deref())?;
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

/// Converts one sheet to stdout or the `-o` file.
fn convert_one_sheet(cli: &Cli, source: &Source) -> Result<()> {
    let sheet = xlsx::read_sheet(source.reader()?, cli.sheet.as_deref())?;
    let mut images = SheetImages::open(cli, source, output::markdown_dir(cli.output.as_deref()))?;

    let mut out = output::open_output(cli.output.as_deref())?;
    write_sheet(&sheet, cli, images.as_mut(), &mut out)?;
    // BufWriter flushes on drop but ignores errors there, so flush explicitly.
    out.flush()?;

    if let Some(images) = &images {
        images.report();
    }
    Ok(())
}

/// Writes every sheet to its own file, in the `-o` directory or the current one.
fn convert_all_sheets(cli: &Cli, source: &Source) -> Result<()> {
    let dir = cli.output.as_deref().unwrap_or(Path::new("."));
    output::ensure_dir(dir)?;
    let mut images = SheetImages::open(cli, source, dir)?;
    let stem = source.stem();
    let mut names = output::UniqueNames::default();

    for sheet in xlsx::read_all_sheets(source.reader()?)? {
        let path = output::sheet_output_path(dir, &stem, &sheet.name, cli.to, &mut names);
        let mut out = output::open_output(Some(&path))?;
        write_sheet(&sheet, cli, images.as_mut(), &mut out)?;
        out.flush()?;
        eprintln!("wrote {}", path.display());
    }

    if let Some(images) = &images {
        images.report();
    }
    Ok(())
}

/// Writes a sheet's table, then saves its pictures if asked to.
///
/// Markdown lists the pictures after the table. CSV, TSV and JSON have no way to point at an
/// image, so for those the files are saved and the data is left as it is.
fn write_sheet(
    sheet: &xlsx::Sheet,
    cli: &Cli,
    images: Option<&mut SheetImages>,
    out: &mut impl Write,
) -> Result<()> {
    let format = cli.to;
    let json = if cli.typed {
        JsonValues::Typed
    } else {
        JsonValues::Text
    };
    writers::write_table(&sheet.table, format, json, &mut *out)?;

    let Some(images) = images else {
        return Ok(());
    };
    let pictures = match &sheet.part {
        Some(part) => {
            let pictures = xlsx::pictures::sheet_pictures(&mut images.archive, part)?;
            images::resolve(
                pictures,
                &mut images.archive,
                Images::Save(&mut images.export),
            )?
        }
        None => Vec::new(),
    };
    if format == OutputFormat::Markdown && !pictures.is_empty() {
        if !sheet.table.is_empty() {
            writeln!(out)?;
        }
        out.write_all(document::markdown::render(&pictures).as_bytes())?;
    }
    Ok(())
}

/// A second reader on the workbook (to find pictures in) and where to save them.
///
/// `'a` is the lifetime of the [`Source`]: when the input is stdin, the reader borrows its bytes.
struct SheetImages<'a> {
    archive: opc::Archive<Box<dyn ReadSeek + 'a>>,
    export: ImageExport,
}

impl<'a> SheetImages<'a> {
    /// `None` unless `--images` was given.
    fn open(cli: &Cli, source: &'a Source, markdown_dir: &Path) -> Result<Option<Self>> {
        let Some(dir) = &cli.images else {
            return Ok(None);
        };
        Ok(Some(SheetImages {
            archive: opc::Archive::open(source.reader()?)?,
            export: ImageExport::new(dir, markdown_dir)?,
        }))
    }

    fn report(&self) {
        self.export.report();
    }
}
