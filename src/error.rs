//! Errors the library can return.

use std::path::PathBuf;

use thiserror::Error;

use crate::cli::OutputFormat;
use crate::input::InputKind;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("input file not found: {}", .0.display())]
    InputNotFound(PathBuf),

    #[error("could not open {}: {source}", path.display())]
    OpenInput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error(
        "unsupported input file {} (expected .xlsx, .docx, or .pptx; use --from to set the type)",
        .0.display()
    )]
    UnsupportedInput(PathBuf),

    #[error("no input on stdin: pipe a file in, or give a file path instead of -")]
    StdinIsTerminal,

    #[error("stdin is empty")]
    EmptyStdin,

    #[error("could not read stdin: {0}")]
    ReadStdin(std::io::Error),

    #[error(
        "could not tell what kind of file is on stdin (expected .xlsx, .docx, or .pptx); \
         use --from to set the type"
    )]
    UnrecognizedStdin,

    #[error("the input isn't a .{expected} file ({})", describe_found(*.found))]
    WrongKind {
        expected: InputKind,
        found: Option<InputKind>,
    },

    #[error("cannot convert {input} to {to}; {input} supports: {supported}")]
    UnsupportedConversion {
        input: InputKind,
        to: OutputFormat,
        supported: &'static str,
    },

    #[error("--sheet and --all-sheets only apply to .xlsx input")]
    SheetOptionOnlyForXlsx,

    #[error("--typed only applies to --to json")]
    TypedOnlyForJson,

    #[error("--no-notes only applies to .pptx input")]
    NotesOptionOnlyForPptx,

    #[error("could not read workbook: {0}")]
    Xlsx(#[from] calamine::XlsxError),

    #[error("sheet {name:?} not found; available sheets: {available}")]
    SheetNotFound { name: String, available: String },

    #[error("workbook has no sheets")]
    NoSheets,

    #[error("could not read document: {0}")]
    Docx(#[from] zip::result::ZipError),

    #[error("could not parse document: {0}")]
    DocxXml(#[from] quick_xml::Error),

    #[error("could not create {}: {source}", path.display())]
    CreateOutput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not write output: {0}")]
    Write(#[from] std::io::Error),
}

/// The end of a [`ConvertError::WrongKind`] message.
fn describe_found(found: Option<InputKind>) -> String {
    match found {
        Some(kind) => format!("it looks like a .{kind}"),
        None => "it isn't an Office file".to_string(),
    }
}

/// Shorthand so functions can write `Result<T>` instead of `Result<T, ConvertError>`.
pub type Result<T> = std::result::Result<T, ConvertError>;
