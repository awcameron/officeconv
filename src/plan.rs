//! Which inputs, outputs and options go together.
//!
//! [`check_options`] and [`plan`] turn the parsed command line into a [`Plan`], or into the
//! usage error that says why it can't be carried out. `run()` in `lib.rs` carries the plan out
//! and decides nothing itself, so every rule is here, tested without running the binary.

use std::path::PathBuf;

use clap::ValueEnum;

use crate::cli::Cli;
use crate::error::{ConvertError, Result};
use crate::format::OutputFormat;
use crate::input::InputKind;
use crate::pptx::Notes;
use crate::writers::{JsonValues, TableFormat};

/// What to convert, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A CSV or TSV file, as one table.
    Delimited {
        kind: InputKind,
        format: TableFormat,
    },
    /// A workbook's sheets, as tables. With `images`, each sheet's pictures are saved there.
    Workbook {
        sheets: Sheets,
        format: TableFormat,
        images: Option<PathBuf>,
    },
    /// A Word document or a PowerPoint deck.
    Document {
        reader: DocumentReader,
        output: DocumentOutput,
    },
}

/// Which sheets of a workbook to convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sheets {
    /// The sheet with this name, or the first one: to stdout or the `-o` file.
    One(Option<String>),
    /// Every sheet, each to its own file.
    All,
}

/// Which reader a document needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentReader {
    Docx,
    Pptx(Notes),
}

/// What a document becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentOutput {
    /// Markdown. With `images`, the pictures are saved there and linked.
    Markdown { images: Option<PathBuf> },
    /// A PDF holding its own pictures, on pages shaped for the input.
    Pdf(Pages),
}

/// The pages a PDF is laid out on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pages {
    /// A4, for a Word document.
    Document,
    /// One landscape page per slide.
    Slides,
}

/// What `kind` converted to `to` would produce, or `None` if it can't be. The one table of
/// which inputs go to which outputs: [`plan`] and the error messages both read it.
fn target(kind: InputKind, to: OutputFormat, json: JsonValues) -> Option<Target> {
    use InputKind::*;
    Some(match (kind, to) {
        (Xlsx | Csv | Tsv, OutputFormat::Csv) => Target::Table(TableFormat::Csv),
        (Xlsx | Csv | Tsv, OutputFormat::Tsv) => Target::Table(TableFormat::Tsv),
        (Xlsx | Csv | Tsv, OutputFormat::Json) => Target::Table(TableFormat::Json(json)),
        (Xlsx | Csv | Tsv, OutputFormat::Markdown) => Target::Table(TableFormat::Markdown),
        (Docx | Pptx, OutputFormat::Markdown) => Target::Markdown,
        (Docx | Pptx, OutputFormat::Pdf) => Target::Pdf,
        _ => return None,
    })
}

enum Target {
    Table(TableFormat),
    Markdown,
    Pdf,
}

/// The outputs `kind` converts to, as a message lists them: `md, pdf`.
fn supported_outputs(kind: InputKind) -> String {
    OutputFormat::value_variants()
        .iter()
        .filter(|&&to| target(kind, to, JsonValues::Text).is_some())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The inputs officeconv reads, as a message lists them: `.xlsx, .docx, .pptx, .csv, or .tsv`.
/// With `packages_only`, just the Office files it can recognize from their contents.
pub fn expected_inputs(packages_only: bool) -> String {
    let names: Vec<String> = InputKind::value_variants()
        .iter()
        .filter(|kind| !packages_only || kind.is_package())
        .map(|kind| format!(".{kind}"))
        .collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// Checks the options that don't depend on the input. `run()` calls this before reading it, so
/// a slow pipe on stdin doesn't have to finish before a simple mistake is reported.
pub fn check_options(cli: &Cli) -> Result<()> {
    if cli.typed && cli.to != OutputFormat::Json {
        return Err(ConvertError::TypedOnlyForJson);
    }
    if cli.to == OutputFormat::Pdf && !cfg!(feature = "pdf") {
        return Err(ConvertError::PdfNotBuilt);
    }
    Ok(())
}

/// The plan for converting input of `kind` as `cli` asks, or the first usage error in it.
/// `stdout_is_terminal` says whether output with no `-o` would go to a terminal.
pub fn plan(cli: &Cli, kind: InputKind, stdout_is_terminal: bool) -> Result<Plan> {
    check_options(cli)?;
    let json = if cli.typed {
        JsonValues::Typed
    } else {
        JsonValues::Text
    };
    let Some(target) = target(kind, cli.to, json) else {
        return Err(ConvertError::UnsupportedConversion {
            input: kind,
            to: cli.to,
            supported: supported_outputs(kind),
        });
    };

    if kind != InputKind::Xlsx && (cli.sheet.is_some() || cli.all_sheets) {
        return Err(ConvertError::SheetOptionOnlyForXlsx);
    }
    if kind != InputKind::Pptx && cli.no_notes {
        return Err(ConvertError::NotesOptionOnlyForPptx);
    }
    if matches!(kind, InputKind::Csv | InputKind::Tsv) {
        // Reading types from text would guess wrong too often: `00123` would lose its zeros.
        if cli.typed {
            return Err(ConvertError::OptionNotForDelimited {
                option: "--typed",
                reason: "it holds only text, with no numbers or booleans to keep",
            });
        }
        if cli.images.is_some() {
            return Err(ConvertError::OptionNotForDelimited {
                option: "--images",
                reason: "it has no images",
            });
        }
    }

    let images = cli.images.clone();
    Ok(match target {
        Target::Table(format) if kind == InputKind::Xlsx => Plan::Workbook {
            sheets: if cli.all_sheets {
                Sheets::All
            } else {
                Sheets::One(cli.sheet.clone())
            },
            format,
            images,
        },
        Target::Table(format) => Plan::Delimited { kind, format },
        Target::Markdown => Plan::Document {
            reader: reader(kind, cli),
            output: DocumentOutput::Markdown { images },
        },
        Target::Pdf => {
            if images.is_some() {
                return Err(ConvertError::ImagesWithPdf);
            }
            if cli.output.is_none() && stdout_is_terminal {
                return Err(ConvertError::PdfToTerminal);
            }
            let pages = if kind == InputKind::Pptx {
                Pages::Slides
            } else {
                Pages::Document
            };
            Plan::Document {
                reader: reader(kind, cli),
                output: DocumentOutput::Pdf(pages),
            }
        }
    })
}

/// The reader for a document of `kind`, which [`target`] only allows to be DOCX or PPTX.
fn reader(kind: InputKind, cli: &Cli) -> DocumentReader {
    if kind == InputKind::Pptx {
        DocumentReader::Pptx(if cli.no_notes {
            Notes::Skip
        } else {
            Notes::Include
        })
    } else {
        DocumentReader::Docx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// The options `args` parse to, after the program name and an input named `input.<kind>`.
    fn cli(kind: InputKind, args: &str) -> Cli {
        let input = format!("input.{kind}");
        let all = ["officeconv", input.as_str()]
            .into_iter()
            .chain(args.split_whitespace());
        Cli::try_parse_from(all).unwrap()
    }

    /// The plan for `kind` with `args`, writing to a pipe.
    fn plan_for(kind: InputKind, args: &str) -> Result<Plan> {
        plan(&cli(kind, args), kind, false)
    }

    /// The message of the usage error `kind` with `args` gives.
    fn error_for(kind: InputKind, args: &str) -> String {
        plan_for(kind, args).unwrap_err().to_string()
    }

    #[test]
    fn plans_every_supported_input_and_output() {
        use InputKind::*;
        let tables = [
            ("csv", TableFormat::Csv),
            ("tsv", TableFormat::Tsv),
            ("json", TableFormat::Json(JsonValues::Text)),
            ("md", TableFormat::Markdown),
        ];
        for (to, format) in tables {
            for kind in [Csv, Tsv] {
                assert_eq!(
                    plan_for(kind, &format!("--to {to}")).unwrap(),
                    Plan::Delimited { kind, format }
                );
            }
            assert_eq!(
                plan_for(Xlsx, &format!("--to {to}")).unwrap(),
                Plan::Workbook {
                    sheets: Sheets::One(None),
                    format,
                    images: None
                }
            );
        }
        for (kind, reader, pages) in [
            (Docx, DocumentReader::Docx, Pages::Document),
            (Pptx, DocumentReader::Pptx(Notes::Include), Pages::Slides),
        ] {
            assert_eq!(
                plan_for(kind, "--to md").unwrap(),
                Plan::Document {
                    reader,
                    output: DocumentOutput::Markdown { images: None }
                }
            );
            if cfg!(feature = "pdf") {
                assert_eq!(
                    plan_for(kind, "--to pdf").unwrap(),
                    Plan::Document {
                        reader,
                        output: DocumentOutput::Pdf(pages)
                    }
                );
            }
        }
    }

    #[test]
    fn carries_the_options_into_the_plan() {
        use InputKind::*;
        assert_eq!(
            plan_for(Xlsx, "--to json --typed --sheet Q1 --images img").unwrap(),
            Plan::Workbook {
                sheets: Sheets::One(Some("Q1".into())),
                format: TableFormat::Json(JsonValues::Typed),
                images: Some("img".into())
            }
        );
        assert_eq!(
            plan_for(Xlsx, "--to csv --all-sheets").unwrap(),
            Plan::Workbook {
                sheets: Sheets::All,
                format: TableFormat::Csv,
                images: None
            }
        );
        assert_eq!(
            plan_for(Pptx, "--to md --no-notes --images img").unwrap(),
            Plan::Document {
                reader: DocumentReader::Pptx(Notes::Skip),
                output: DocumentOutput::Markdown {
                    images: Some("img".into())
                }
            }
        );
    }

    #[test]
    fn rejects_outputs_an_input_has_none_of() {
        use InputKind::*;
        // Without PDF built in, `--to pdf` stops sooner, at `PdfNotBuilt`.
        if cfg!(feature = "pdf") {
            assert_eq!(
                error_for(Csv, "--to pdf"),
                "cannot convert csv to pdf; csv supports: csv, tsv, json, md"
            );
            assert_eq!(
                error_for(Xlsx, "--to pdf"),
                "cannot convert xlsx to pdf; xlsx supports: csv, tsv, json, md"
            );
        }
        for kind in [Docx, Pptx] {
            for to in ["csv", "tsv", "json"] {
                assert_eq!(
                    error_for(kind, &format!("--to {to}")),
                    format!("cannot convert {kind} to {to}; {kind} supports: md, pdf")
                );
            }
        }
    }

    #[test]
    fn rejects_options_for_other_inputs_and_outputs() {
        use InputKind::*;
        let cases = [
            (Docx, "--to md --typed", ConvertError::TypedOnlyForJson),
            (
                Docx,
                "--to md --sheet S",
                ConvertError::SheetOptionOnlyForXlsx,
            ),
            (
                Csv,
                "--to csv --all-sheets",
                ConvertError::SheetOptionOnlyForXlsx,
            ),
            (
                Docx,
                "--to md --no-notes",
                ConvertError::NotesOptionOnlyForPptx,
            ),
            (
                Xlsx,
                "--to csv --no-notes",
                ConvertError::NotesOptionOnlyForPptx,
            ),
        ];
        for (kind, args, expected) in cases {
            let err = plan_for(kind, args).unwrap_err();
            assert_eq!(err.to_string(), expected.to_string(), "{kind} {args}");
        }
        assert_eq!(
            error_for(Csv, "--to json --typed"),
            "--typed doesn't apply to .csv or .tsv input: it holds only text, with no numbers or \
             booleans to keep"
        );
        assert_eq!(
            error_for(Tsv, "--to md --images img"),
            "--images doesn't apply to .csv or .tsv input: it has no images"
        );
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn checks_the_input_before_options_that_depend_on_it() {
        // Both mistakes are there; the conversion is reported, since it's the bigger one.
        assert_eq!(
            error_for(InputKind::Csv, "--to pdf --images img"),
            "cannot convert csv to pdf; csv supports: csv, tsv, json, md"
        );
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn keeps_pdf_off_the_terminal_and_images_out_of_it() {
        let docx = InputKind::Docx;
        assert!(matches!(
            plan(&cli(docx, "--to pdf"), docx, true),
            Err(ConvertError::PdfToTerminal)
        ));
        assert!(plan(&cli(docx, "--to pdf -o out.pdf"), docx, true).is_ok());
        assert!(matches!(
            plan_for(docx, "--to pdf --images img"),
            Err(ConvertError::ImagesWithPdf)
        ));
    }

    #[cfg(not(feature = "pdf"))]
    #[test]
    fn says_when_pdf_output_is_not_built_in() {
        assert!(matches!(
            check_options(&cli(InputKind::Docx, "--to pdf")),
            Err(ConvertError::PdfNotBuilt)
        ));
    }

    #[test]
    fn lists_the_inputs_officeconv_reads() {
        assert_eq!(expected_inputs(false), ".xlsx, .docx, .pptx, .csv, or .tsv");
        assert_eq!(expected_inputs(true), ".xlsx, .docx, or .pptx");
    }
}
