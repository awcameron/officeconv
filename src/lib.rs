pub mod cli;
pub mod error;
pub mod input;
pub mod table;
pub mod writers;
pub mod xlsx;

use std::fs::File;
use std::io::{self, BufWriter, Write};
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
        InputKind::Xlsx => {
            let sheet = xlsx::read_sheet(&cli.input, cli.sheet.as_deref())?;
            let mut out = open_output(cli.output.as_deref())?;
            writers::write_table(&sheet.table, cli.to, &mut out)?;
            // BufWriter flushes on drop but ignores errors there, so flush explicitly.
            out.flush()?;
        }
        InputKind::Docx => eprintln!("docx conversion arrives in milestone 6"),
    }
    Ok(())
}

/// Opens the file at `path`, or stdout when there's no path. Either way, output is buffered.
fn open_output(path: Option<&Path>) -> Result<Box<dyn Write>> {
    match path {
        Some(path) => {
            let file = File::create(path).map_err(|source| ConvertError::CreateOutput {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(Box::new(BufWriter::new(file)))
        }
        None => Ok(Box::new(BufWriter::new(io::stdout().lock()))),
    }
}
