//! The output formats `officeconv` can write.
//!
//! This lives in its own module so that both the command line (`cli`) and input handling
//! (`input`) can use it without depending on each other.

use std::fmt;

use clap::ValueEnum;

/// What to convert to (`--to`).
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
