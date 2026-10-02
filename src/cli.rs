//! Command-line interface definition.

use std::fmt;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Convert Office files (XLSX, DOCX, PPTX) to plain-text formats.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Input file (.xlsx, .docx, or .pptx)
    pub input: PathBuf,

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

    /// Leave out speaker notes (PPTX only)
    #[arg(long)]
    pub no_notes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Csv,
    Tsv,
    Json,
    #[value(name = "md", alias = "markdown")]
    Markdown,
}

impl OutputFormat {
    /// File extension for this format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Csv => "csv",
            OutputFormat::Tsv => "tsv",
            OutputFormat::Json => "json",
            OutputFormat::Markdown => "md",
        }
    }
}

impl fmt::Display for OutputFormat {
    /// Prints the name the user types on the command line (`csv`, `md`, ...).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self
            .to_possible_value()
            .expect("every variant has a CLI name");
        f.write_str(value.get_name())
    }
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
