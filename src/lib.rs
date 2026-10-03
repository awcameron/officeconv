pub mod cli;
pub mod document;
pub mod docx;
pub mod error;
pub mod format;
pub mod images;
pub mod input;
pub mod opc;
pub mod output;
#[cfg(feature = "pdf")]
pub mod pdf;
#[cfg(feature = "typst-spike")]
pub mod pdf_typst;
pub mod pptx;
pub mod table;
pub mod writers;
pub mod xlsx;

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use cli::Cli;
use error::{ConvertError, Result};
use format::OutputFormat;
use images::{EmbeddedImages, ImageExport, Images};
use input::{InputKind, ReadSeek, Source};
use writers::JsonValues;
use zip::ZipArchive;

/// Validates the request and runs the matching converter.
pub fn run(cli: &Cli) -> Result<()> {
    // Checks that don't depend on the input come first, so a slow pipe on stdin doesn't
    // have to finish before a simple usage mistake is reported.
    if cli.typed && cli.to != OutputFormat::Json {
        return Err(ConvertError::TypedOnlyForJson);
    }
    if cli.to == OutputFormat::Pdf && !cfg!(any(feature = "pdf", feature = "typst-spike")) {
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
    }
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

    let reader = source.reader()?;
    let blocks = if kind == InputKind::Pptx {
        let notes = if cli.no_notes {
            pptx::Notes::Skip
        } else {
            pptx::Notes::Include
        };
        pptx::read_blocks(reader, notes, images)?
    } else {
        docx::read_blocks(reader, images)?
    };

    if cli.to == OutputFormat::Pdf {
        return write_pdf(cli, kind, &blocks, &embedded);
    }

    write_output(cli, document::markdown::render(&blocks).as_bytes())?;
    if let Some(export) = &export {
        eprintln!(
            "saved {} images to {}",
            export.count(),
            export.dir().display()
        );
    }
    Ok(())
}

/// Writes a Word document as A4 pages, or a deck as one landscape page per slide. Image runs
/// hold keys into `images`.
#[cfg(any(feature = "pdf", feature = "typst-spike"))]
fn write_pdf(
    cli: &Cli,
    kind: InputKind,
    blocks: &[document::Block],
    images: &EmbeddedImages,
) -> Result<()> {
    // PROTOTYPE (#29): render through Typst when it's the only backend built, or when
    // OFFICECONV_PDF_ENGINE=typst.
    #[cfg(feature = "typst-spike")]
    if !cfg!(feature = "pdf") || std::env::var("OFFICECONV_PDF_ENGINE").as_deref() == Ok("typst") {
        let pages = match kind {
            InputKind::Pptx => pdf_typst::Pages::Slides,
            _ => pdf_typst::Pages::Document,
        };
        let pdf = pdf_typst::render(blocks, images, pages)?;
        return write_output(cli, &pdf);
    }
    #[cfg(feature = "pdf")]
    {
        write_krilla_pdf(cli, kind, blocks, images)
    }
    #[cfg(not(feature = "pdf"))]
    unreachable!("the Typst prototype handles every PDF without the pdf feature")
}

#[cfg(feature = "pdf")]
fn write_krilla_pdf(
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
    if rendered.skipped_images > 0 {
        eprintln!(
            "warning: left out {} images in formats a PDF can't hold (such as EMF or TIFF)",
            rendered.skipped_images
        );
    }
}

#[cfg(not(any(feature = "pdf", feature = "typst-spike")))]
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

    for sheet in xlsx::read_all_sheets(source.reader()?)? {
        let path = output::sheet_output_path(dir, &stem, &sheet.name, cli.to);
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
    let pictures = xlsx::pictures::export_sheet_pictures(
        &mut images.archive,
        &sheet.name,
        &mut images.export,
    )?;
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
    archive: ZipArchive<Box<dyn ReadSeek + 'a>>,
    export: ImageExport,
}

impl<'a> SheetImages<'a> {
    /// `None` unless `--images` was given.
    fn open(cli: &Cli, source: &'a Source, markdown_dir: &Path) -> Result<Option<Self>> {
        let Some(dir) = &cli.images else {
            return Ok(None);
        };
        Ok(Some(SheetImages {
            archive: opc::open(source.reader()?)?,
            export: ImageExport::new(dir, markdown_dir)?,
        }))
    }

    fn report(&self) {
        let export = &self.export;
        eprintln!(
            "saved {} images to {}",
            export.count(),
            export.dir().display()
        );
    }
}
