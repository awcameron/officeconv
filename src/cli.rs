//! Command-line interface definition.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Convert Office files (XLSX, DOCX) to plain-text formats.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Input file (.xlsx or .docx)
    pub input: PathBuf,

    /// Output format
    #[arg(short, long, value_enum)]
    pub to: OutputFormat,

    /// Write to this file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Sheet to convert (XLSX only; defaults to the first sheet)
    #[arg(long, conflicts_with = "all_sheets")]
    pub sheet: Option<String>,

    /// Convert every sheet to its own file (XLSX only)
    #[arg(long)]
    pub all_sheets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Csv,
    Tsv,
    Json,
    #[value(name = "md", alias = "markdown")]
    Markdown,
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
