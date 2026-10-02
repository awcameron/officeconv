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

use cli::Cli;
use error::{ConvertError, Result};
use images::ImageExport;
use input::InputKind;

/// Validates the request and runs the matching converter.
pub fn run(cli: &Cli) -> Result<()> {
    let kind = InputKind::from_path(&cli.input)?;
    kind.check_output(cli.to)?;

    if kind != InputKind::Xlsx && (cli.sheet.is_some() || cli.all_sheets) {
        return Err(ConvertError::SheetOptionOnlyForXlsx);
    }
    if kind != InputKind::Pptx && cli.no_notes {
        return Err(ConvertError::NotesOptionOnlyForPptx);
    }
    if kind == InputKind::Xlsx && cli.images.is_some() {
        return Err(ConvertError::ImagesOptionOnlyForDocuments);
    }
    if !cli.input.is_file() {
        return Err(ConvertError::InputNotFound(cli.input.clone()));
    }

    match kind {
        InputKind::Xlsx if cli.all_sheets => convert_all_sheets(cli),
        InputKind::Xlsx => convert_one_sheet(cli),
        InputKind::Docx | InputKind::Pptx => convert_document(cli, kind),
    }
}

/// Converts a Word or PowerPoint file to Markdown, saving its images if asked to.
fn convert_document(cli: &Cli, kind: InputKind) -> Result<()> {
    let mut images = match &cli.images {
        Some(dir) => Some(ImageExport::new(dir, cli.output.as_deref())?),
        None => None,
    };

    let blocks = if kind == InputKind::Pptx {
        let notes = if cli.no_notes {
            pptx::Notes::Skip
        } else {
            pptx::Notes::Include
        };
        pptx::read_blocks(&cli.input, notes, images.as_mut())?
    } else {
        docx::read_blocks(&cli.input, images.as_mut())?
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
fn convert_one_sheet(cli: &Cli) -> Result<()> {
    let sheet = xlsx::read_sheet(&cli.input, cli.sheet.as_deref())?;
    let mut out = output::open_output(cli.output.as_deref())?;
    writers::write_table(&sheet.table, cli.to, &mut out)?;
    // BufWriter flushes on drop but ignores errors there, so flush explicitly.
    out.flush()?;
    Ok(())
}

/// Writes every sheet to its own file, in the `-o` directory or the current one.
fn convert_all_sheets(cli: &Cli) -> Result<()> {
    let dir = cli.output.as_deref().unwrap_or(Path::new("."));
    output::ensure_dir(dir)?;

    for sheet in xlsx::read_all_sheets(&cli.input)? {
        let path = output::sheet_output_path(dir, &cli.input, &sheet.name, cli.to);
        let mut out = output::open_output(Some(&path))?;
        writers::write_table(&sheet.table, cli.to, &mut out)?;
        out.flush()?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}
