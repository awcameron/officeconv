pub mod cli;
pub mod error;
pub mod input;
pub mod output;
pub mod table;
pub mod writers;
pub mod xlsx;

use std::io::Write;
use std::path::Path;

use cli::Cli;
use error::{ConvertError, Result};
use input::InputKind;

/// Validates the request and runs the matching converter.
pub fn run(cli: &Cli) -> Result<()> {
    let kind = InputKind::from_path(&cli.input)?;
    kind.check_output(cli.to)?;

    if kind == InputKind::Docx && (cli.sheet.is_some() || cli.all_sheets) {
        return Err(ConvertError::SheetOptionOnDocx);
    }
    if !cli.input.is_file() {
        return Err(ConvertError::InputNotFound(cli.input.clone()));
    }

    match kind {
        InputKind::Xlsx if cli.all_sheets => convert_all_sheets(cli),
        InputKind::Xlsx => convert_one_sheet(cli),
        InputKind::Docx => {
            eprintln!("docx conversion arrives in milestone 6");
            Ok(())
        }
    }
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
