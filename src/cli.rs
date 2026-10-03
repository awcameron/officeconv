//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;

use crate::format::OutputFormat;
use crate::input::InputKind;

/// Convert Office files (XLSX, DOCX, PPTX) to plain-text formats.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Input file (.xlsx, .docx, or .pptx), or - to read from stdin
    pub input: PathBuf,

    /// Input type, instead of working it out from the extension (or, for stdin, the contents)
    #[arg(long, value_enum, value_name = "TYPE")]
    pub from: Option<InputKind>,

    /// Output format
    #[arg(short, long, value_enum, value_name = "FORMAT")]
    pub to: OutputFormat,

    /// Write to this file instead of stdout (with --all-sheets: a directory)
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Sheet to convert (XLSX only; defaults to the first sheet)
    #[arg(long, value_name = "NAME", conflicts_with = "all_sheets")]
    pub sheet: Option<String>,

    /// Convert every sheet to its own file, named like sales-Q1.csv (XLSX only)
    #[arg(long)]
    pub all_sheets: bool,

    /// Write numbers, booleans and empty cells as JSON values instead of strings (JSON only)
    #[arg(long)]
    pub typed: bool,

    /// Leave out speaker notes (PPTX only)
    #[arg(long)]
    pub no_notes: bool,

    /// Save images into this directory and link them from the Markdown
    #[arg(long, value_name = "DIR")]
    pub images: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markdown_alias() {
        let cli = Cli::try_parse_from(["officeconv", "a.docx", "--to", "markdown"]).unwrap();
        assert_eq!(cli.to, OutputFormat::Markdown);
    }

    #[test]
    fn notes_are_included_unless_turned_off() {
        let cli = Cli::try_parse_from(["officeconv", "a.pptx", "--to", "md"]).unwrap();
        assert!(!cli.no_notes);
        let cli =
            Cli::try_parse_from(["officeconv", "a.pptx", "--to", "md", "--no-notes"]).unwrap();
        assert!(cli.no_notes);
    }

    #[test]
    fn sheet_and_all_sheets_conflict() {
        let result = Cli::try_parse_from([
            "officeconv",
            "a.xlsx",
            "--to",
            "csv",
            "--sheet",
            "S",
            "--all-sheets",
        ]);
        assert!(result.is_err());
    }
}
