//! Errors the library can return.

use std::path::PathBuf;

use thiserror::Error;

use crate::cli::OutputFormat;
use crate::input::InputKind;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("input file not found: {}", .0.display())]
    InputNotFound(PathBuf),

    #[error("unsupported input file {} (expected .xlsx or .docx)", .0.display())]
    UnsupportedInput(PathBuf),

    #[error("cannot convert {input} to {to}; {input} supports: {supported}")]
    UnsupportedConversion {
        input: InputKind,
        to: OutputFormat,
        supported: &'static str,
    },

    #[error("--sheet and --all-sheets only apply to .xlsx input")]
    SheetOptionOnDocx,

    #[error("could not read workbook: {0}")]
    Xlsx(#[from] calamine::XlsxError),

    #[error("sheet {name:?} not found; available sheets: {available}")]
    SheetNotFound { name: String, available: String },

    #[error("workbook has no sheets")]
    NoSheets,

    #[error("could not create {}: {source}", path.display())]
    CreateOutput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not write output: {0}")]
    Write(#[from] std::io::Error),
}

/// Shorthand so functions can write `Result<T>` instead of `Result<T, ConvertError>`.
pub type Result<T> = std::result::Result<T, ConvertError>;
