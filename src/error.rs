//! Errors the library can return.

use std::path::PathBuf;

use thiserror::Error;

use crate::format::OutputFormat;
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
        "unsupported input file {} (expected .xlsx, .docx, .pptx, .csv, or .tsv; use --from to set \
         the type)",
        .0.display()
    )]
    UnsupportedInput(PathBuf),

    #[error("no input on stdin: pipe a file in, or give a file path instead of -")]
    StdinIsTerminal,

    #[error("stdin is empty")]
    EmptyStdin,

    #[error("could not read stdin: {0}")]
    ReadStdin(std::io::Error),

    #[error("could not read the input: {0}")]
    ReadInput(std::io::Error),

    #[error(
        "could not tell what kind of file is on stdin (expected .xlsx, .docx, or .pptx); \
         use --from to set the type, such as --from csv for CSV"
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

    #[error("{option} doesn't apply to .csv or .tsv input: {reason}")]
    OptionNotForDelimited {
        option: &'static str,
        reason: &'static str,
    },

    #[error("--images doesn't apply to --to pdf: the PDF holds its images itself")]
    ImagesWithPdf,

    #[error("not writing a PDF to the terminal; use -o FILE, or pipe the output")]
    PdfToTerminal,

    #[error("this build of officeconv doesn't include PDF output (the `pdf` feature)")]
    PdfNotBuilt,

    #[error("could not read workbook: {0}")]
    Xlsx(#[from] calamine::XlsxError),

    #[error("sheet {name:?} not found; available sheets: {available}")]
    SheetNotFound { name: String, available: String },

    #[error("workbook has no sheets")]
    NoSheets,

    #[error("could not read document: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("could not read document: {part} is missing")]
    MissingPart { part: String },

    #[error("could not read document: {part} isn't UTF-8 text")]
    PartNotUtf8 { part: String },

    #[error("could not parse document: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error(
        "{part} in the input decompresses to more than {}, the most officeconv reads from one part",
        megabytes(*.limit)
    )]
    PartTooLarge { part: String, limit: u64 },

    #[error(
        "the input decompresses to more than {}, the most officeconv reads from one file",
        megabytes(*.limit)
    )]
    InputTooLarge { limit: u64 },

    #[error("stdin has more than {}, the most officeconv reads", megabytes(*.limit))]
    StdinTooLarge { limit: u64 },

    #[error("could not read the {kind} input: line {line}: {problem}")]
    Delimited {
        kind: InputKind,
        line: u64,
        problem: &'static str,
    },

    #[error(
        "the input has more than {}, the most officeconv reads from a CSV or TSV file",
        megabytes(*.limit)
    )]
    DelimitedTooLarge { limit: u64 },

    #[error(
        "the table has more than {limit} cells, counting short rows as padded to the widest; \
         that's the most officeconv reads from a CSV or TSV file"
    )]
    DelimitedTooManyCells { limit: u64 },

    #[cfg(feature = "pdf")]
    #[error("could not write PDF: {0}")]
    Pdf(String),

    #[error("could not create {}: {source}", path.display())]
    CreateOutput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not write output: {0}")]
    Write(#[from] std::io::Error),
}

/// Exit codes from BSD's `sysexits.h`, which scripts can check to tell kinds of failure apart.
/// clap exits with 2 for arguments it can't parse, before any of these apply.
pub mod exit_code {
    /// The options don't make sense together, or for this input.
    pub const USAGE: u8 = 64;
    /// The input isn't a file officeconv can read.
    pub const DATA: u8 = 65;
    /// The input doesn't exist or can't be opened.
    pub const NO_INPUT: u8 = 66;
    /// Reading stdin or writing the output failed.
    pub const IO: u8 = 74;
}

impl ConvertError {
    /// The exit status for this error, one of the [`exit_code`] constants.
    ///
    /// The match is exhaustive, so a new variant has to choose its code.
    pub fn exit_code(&self) -> u8 {
        use ConvertError::*;
        match self {
            // Fixed by changing the command, not the file. An unknown sheet is here too: the
            // file is fine, the name given to --sheet isn't.
            UnsupportedConversion { .. }
            | SheetOptionOnlyForXlsx
            | TypedOnlyForJson
            | NotesOptionOnlyForPptx
            | OptionNotForDelimited { .. }
            | ImagesWithPdf
            | PdfToTerminal
            | PdfNotBuilt
            | SheetNotFound { .. } => exit_code::USAGE,

            // The input is there but isn't something officeconv can convert.
            UnsupportedInput(_)
            | UnrecognizedStdin
            | WrongKind { .. }
            | Xlsx(_)
            | NoSheets
            | Zip(_)
            | MissingPart { .. }
            | PartNotUtf8 { .. }
            | Xml(_)
            | PartTooLarge { .. }
            | InputTooLarge { .. }
            | StdinTooLarge { .. }
            | Delimited { .. }
            | DelimitedTooLarge { .. }
            | DelimitedTooManyCells { .. } => exit_code::DATA,
            // krilla rejects a PDF because of what the document holds, so it's bad input too.
            #[cfg(feature = "pdf")]
            Pdf(_) => exit_code::DATA,

            InputNotFound(_) | OpenInput { .. } | StdinIsTerminal | EmptyStdin => {
                exit_code::NO_INPUT
            }

            ReadStdin(_) | ReadInput(_) | CreateOutput { .. } | Write(_) => exit_code::IO,
        }
    }
}

/// The end of a [`ConvertError::WrongKind`] message.
fn describe_found(found: Option<InputKind>) -> String {
    match found {
        Some(kind) => format!("it looks like a .{kind}"),
        None => "it isn't an Office file".to_string(),
    }
}

/// A size limit as it appears in a message, such as `256 MB`.
fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes >> 20)
}

/// Shorthand so functions can write `Result<T>` instead of `Result<T, ConvertError>`.
pub type Result<T> = std::result::Result<T, ConvertError>;
