//! The `officeconv` command-line tool.
//!
//! This crate is a binary, not a library to depend on. The library half holds the code so the
//! fuzz targets in `fuzz/` can link against it; it has no public API, and any of it may change
//! in any release. See `docs/adr/0003-binary-only.md`.

mod cli;
mod delimited;
mod document;
mod document_output;
mod docx;
mod drawingml;
mod error;
mod format;
mod images;
mod input;
mod markdown;
mod opc;
mod output;
#[cfg(feature = "pdf")]
mod pdf;
mod plan;
mod pptx;
mod table;
mod workbook;
mod writers;
mod xlsx;

/// The readers and writers the fuzz targets in `fuzz/` call directly, re-exported for them.
/// Only built with the `fuzzing` feature, and as unstable as the rest of the crate.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::delimited::{
        Limits as DelimitedLimits, read_table_with_limits as read_delimited,
    };
    pub use crate::document::Block;
    pub use crate::document::markdown::render as render_markdown;
    pub use crate::docx::read_blocks as read_docx;
    pub use crate::images::{EmbeddedImages, Images, resolve as resolve_images};
    pub use crate::input::InputKind;
    pub use crate::opc::{Archive, Limits, parse_relationships};
    #[cfg(feature = "pdf")]
    pub use crate::pdf::{layout::PageSetup, render as render_pdf};
    pub use crate::pptx::{Notes, read_blocks as read_pptx};
    pub use crate::writers::{JsonValues, TableFormat, write_table};
    pub use crate::xlsx::{Pictures, read_all_sheets_with_limits as read_xlsx};
}

use std::io::{self, ErrorKind, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use error::{ConvertError, Result};
use input::{InputKind, Source};
use plan::Plan;
use writers::TableFormat;

/// The `officeconv` command: parses the arguments, converts, and reports any error.
///
/// The binary is all `main.rs` holds, so this is the library's only public item. It's hidden
/// from the docs because it isn't an API to build on: see the crate docs.
#[doc(hidden)]
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader went away (e.g. `officeconv big.xlsx --to csv | head`); that's not a failure.
        Err(ConvertError::Write(err)) if err.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            // Each error message already includes its cause, so print just that one line.
            eprintln!("Error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

/// Plans the conversion the options ask for, then carries it out.
fn run(cli: &Cli) -> Result<()> {
    plan::check_options(cli)?;
    output::check_not_input(&cli.input, cli.output.as_deref())?;
    let source = Source::from_arg(&cli.input)?;
    let kind = source.kind(cli.from)?;
    let output = cli.output.as_deref();
    match plan::plan(cli, kind, io::stdout().is_terminal())? {
        Plan::Delimited { kind, format } => convert_delimited(&source, kind, format, output),
        Plan::Workbook {
            sheets,
            format,
            images,
        } => workbook::convert(
            &source,
            &sheets,
            format,
            images.as_deref(),
            output,
            &mut io::stderr().lock(),
        ),
        Plan::Document { reader, output: to } => {
            document_output::convert(&source, reader, to, output, &mut io::stderr().lock())
        }
    }
}

/// Converts a CSV or TSV file, as one table, to stdout or the `output` file.
fn convert_delimited(
    source: &Source,
    kind: InputKind,
    format: TableFormat,
    output: Option<&Path>,
) -> Result<()> {
    let table = delimited::read_table(source.reader()?, kind)?;
    let mut out = output::open_output(output)?;
    writers::write_table(&table, format, &mut out)?;
    out.flush()?;
    Ok(())
}
