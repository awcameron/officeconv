pub mod cli;
pub mod document;
pub mod docx;
pub mod error;
pub mod images;
pub mod input;
pub mod opc;
pub mod output;
pub mod pptx;
pub mod table;
pub mod writers;
pub mod xlsx;

use std::io::Write;
use std::path::Path;

use cli::{Cli, OutputFormat};
use error::{ConvertError, Result};
use images::ImageExport;
use input::{InputKind, ReadSeek, Source};
use writers::JsonValues;
use zip::ZipArchive;

/// Validates the request and runs the matching converter.
pub fn run(cli: &Cli) -> Result<()> {
    let source = Source::from_arg(&cli.input)?;
    let kind = source.kind(cli.from)?;
    kind.check_output(cli.to)?;

    if kind != InputKind::Xlsx && (cli.sheet.is_some() || cli.all_sheets) {
        return Err(ConvertError::SheetOptionOnlyForXlsx);
    }
    if cli.typed && cli.to != OutputFormat::Json {
        return Err(ConvertError::TypedOnlyForJson);
    }
    if kind != InputKind::Pptx && cli.no_notes {
        return Err(ConvertError::NotesOptionOnlyForPptx);
    }
    if let Source::File(path) = &source
        && !path.is_file()
    {
        return Err(ConvertError::InputNotFound(path.clone()));
    }

    match kind {
        InputKind::Xlsx if cli.all_sheets => convert_all_sheets(cli, &source),
        InputKind::Xlsx => convert_one_sheet(cli, &source),
        InputKind::Docx | InputKind::Pptx => convert_document(cli, &source, kind),
    }
}

/// Converts a Word or PowerPoint file to Markdown, saving its images if asked to.
fn convert_document(cli: &Cli, source: &Source, kind: InputKind) -> Result<()> {
    let mut images = match &cli.images {
        Some(dir) => Some(ImageExport::new(
            dir,
            output::markdown_dir(cli.output.as_deref()),
        )?),
        None => None,
    };

    let reader = source.reader()?;
    let blocks = if kind == InputKind::Pptx {
        let notes = if cli.no_notes {
            pptx::Notes::Skip
        } else {
            pptx::Notes::Include
        };
        pptx::read_blocks(reader, notes, images.as_mut())?
    } else {
        docx::read_blocks(reader, images.as_mut())?
    };
    write_document(cli, &blocks)?;

    if let Some(images) = &images {
        eprintln!(
            "saved {} images to {}",
            images.count(),
            images.dir().display()
        );
    }
    Ok(())
}

/// Writes a Word or PowerPoint document as Markdown to stdout or the `-o` file.
fn write_document(cli: &Cli, blocks: &[document::Block]) -> Result<()> {
    let mut out = output::open_output(cli.output.as_deref())?;
    out.write_all(document::markdown::render(blocks).as_bytes())?;
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
