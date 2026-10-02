pub mod cli;
pub mod error;
pub mod input;

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

    // The real converters arrive in milestones 3 (xlsx) and 6 (docx).
    eprintln!(
        "would convert {} ({kind}) to {}",
        cli.input.display(),
        cli.to
    );
    Ok(())
}
