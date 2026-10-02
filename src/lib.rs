pub mod cli;
pub mod error;
pub mod input;
pub mod table;
pub mod xlsx;

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
            // Writers arrive in milestone 4; for now, show what we read.
            eprintln!(
                "read sheet {:?}: {} columns, {} rows",
                sheet.name,
                sheet.table.headers.len(),
                sheet.table.rows.len()
            );
        }
        InputKind::Docx => eprintln!("docx conversion arrives in milestone 6"),
    }
    Ok(())
}
