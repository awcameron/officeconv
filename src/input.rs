//! Where the input comes from (a file or stdin) and what kind of document it is.

use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, IsTerminal, Read, Seek};
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use zip::ZipArchive;

use crate::error::{ConvertError, Result};
use crate::format::OutputFormat;
use crate::opc::Limits;

/// Anything that can be read and jumped around in, like a file or bytes in memory.
///
/// Zip archives need both, so this is what the readers take. It exists so that a file and
/// stdin's bytes can be handed out as the same type: `Box<dyn ReadSeek>`.
pub trait ReadSeek: Read + Seek {}

/// Every type that is both `Read` and `Seek` is a `ReadSeek`.
impl<T: Read + Seek> ReadSeek for T {}

/// The input to convert.
#[derive(Debug)]
pub enum Source {
    File(PathBuf),
    /// Everything read from stdin. Zip archives need random access, which a pipe can't give,
    /// so it's read into memory first.
    Stdin(Vec<u8>),
}

impl Source {
    /// The input named on the command line: `-` reads all of stdin, anything else is a path.
    ///
    /// A path must be a file that exists. Checking here, before anything tries to work out the
    /// type, means a missing file or a directory always gets the same "not found" error.
    pub fn from_arg(arg: &Path) -> Result<Self> {
        if arg != Path::new("-") {
            if !arg.is_file() {
                return Err(ConvertError::InputNotFound(arg.to_path_buf()));
            }
            return Ok(Source::File(arg.to_path_buf()));
        }

        let stdin = io::stdin().lock();
        // Nothing piped in: fail now instead of silently waiting for keyboard input.
        if stdin.is_terminal() {
            return Err(ConvertError::StdinIsTerminal);
        }
        let bytes = read_at_most(stdin, Limits::DEFAULT.total)?;
        if bytes.is_empty() {
            return Err(ConvertError::EmptyStdin);
        }
        Ok(Source::Stdin(bytes))
    }

    /// Works out the input kind. In order: `from` when given; a file's extension when it's
    /// one we know; otherwise the parts inside the archive (always the case for stdin). CSV
    /// and TSV are plain text with nothing to recognize them by, so on stdin they need `from`.
    ///
    /// A `from` that doesn't match the contents is an error, so a wrong `--from` gets a clear
    /// message instead of a confusing one about a missing part. For CSV and TSV, that means
    /// anything but an Office file.
    pub fn kind(&self, from: Option<InputKind>) -> Result<InputKind> {
        if let Some(expected) = from {
            let found = InputKind::from_contents(self.reader()?);
            let matches = match expected {
                InputKind::Csv | InputKind::Tsv => found.is_none(),
                _ => found == Some(expected),
            };
            if !matches {
                return Err(ConvertError::WrongKind { expected, found });
            }
            return Ok(expected);
        }
        match self {
            // A known extension is trusted without opening the file. An unknown one (`.zip`,
            // `.xlsm`, no extension) falls back to looking inside before giving up.
            Source::File(path) => match InputKind::from_path(path) {
                Ok(kind) => Ok(kind),
                Err(unsupported) => InputKind::from_contents(self.reader()?).ok_or(unsupported),
            },
            Source::Stdin(bytes) => InputKind::from_contents(Cursor::new(bytes.as_slice()))
                .ok_or(ConvertError::UnrecognizedStdin),
        }
    }

    /// A fresh reader positioned at the start. Call it again to read the input a second time.
    pub fn reader(&self) -> Result<Box<dyn ReadSeek + '_>> {
        match self {
            Source::File(path) => {
                let file = File::open(path).map_err(|source| match source.kind() {
                    io::ErrorKind::NotFound => ConvertError::InputNotFound(path.clone()),
                    _ => ConvertError::OpenInput {
                        path: path.clone(),
                        source,
                    },
                })?;
                Ok(Box::new(file))
            }
            Source::Stdin(bytes) => Ok(Box::new(Cursor::new(bytes.as_slice()))),
        }
    }

    /// The name used for per-sheet output files: the file's stem, or `stdin`.
    pub fn stem(&self) -> String {
        match self {
            Source::File(path) => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".to_string()),
            Source::Stdin(_) => "stdin".to_string(),
        }
    }
}

/// Reads all of stdin, up to `limit` bytes. Stdin has to be held in memory (see [`Source`]),
/// so without a limit a never-ending pipe would use all of it.
fn read_at_most(stdin: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stdin
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(ConvertError::ReadStdin)?;
    if bytes.len() as u64 > limit {
        return Err(ConvertError::StdinTooLarge { limit });
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputKind {
    Xlsx,
    Docx,
    Pptx,
    Csv,
    Tsv,
}

impl InputKind {
    /// Picks the input kind from the file extension, ignoring case.
    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        match ext.as_deref() {
            Some("xlsx") => Ok(InputKind::Xlsx),
            Some("docx") => Ok(InputKind::Docx),
            Some("pptx") => Ok(InputKind::Pptx),
            Some("csv") => Ok(InputKind::Csv),
            Some("tsv") => Ok(InputKind::Tsv),
            _ => Err(ConvertError::UnsupportedInput(path.to_path_buf())),
        }
    }

    /// Recognizes an Office file by the main part inside its zip archive.
    ///
    /// Only the archive's table of contents is read, so this is cheap even for big files.
    /// Returns `None` for anything else, including files that aren't zip archives at all
    /// (such as the old binary `.xls` and `.doc` formats).
    pub fn from_contents<R: Read + Seek>(reader: R) -> Option<Self> {
        let archive = ZipArchive::new(reader).ok()?;
        let has = |name: &str| archive.index_for_name(name).is_some();

        if has("xl/workbook.xml") {
            Some(InputKind::Xlsx)
        } else if has("word/document.xml") {
            Some(InputKind::Docx)
        } else if has("ppt/presentation.xml") {
            Some(InputKind::Pptx)
        } else {
            None
        }
    }

    /// Returns an error if this input can't be converted to `to`.
    pub fn check_output(self, to: OutputFormat) -> Result<()> {
        let supported = match self {
            InputKind::Xlsx | InputKind::Csv | InputKind::Tsv => "csv, tsv, json, md",
            InputKind::Docx | InputKind::Pptx => "md, pdf",
        };
        let ok = match self {
            InputKind::Xlsx | InputKind::Csv | InputKind::Tsv => to != OutputFormat::Pdf,
            InputKind::Docx | InputKind::Pptx => {
                matches!(to, OutputFormat::Markdown | OutputFormat::Pdf)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(ConvertError::UnsupportedConversion {
                input: self,
                to,
                supported,
            })
        }
    }
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            InputKind::Xlsx => "xlsx",
            InputKind::Docx => "docx",
            InputKind::Pptx => "pptx",
            InputKind::Csv => "csv",
            InputKind::Tsv => "tsv",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kind_case_insensitively() {
        assert_eq!(
            InputKind::from_path(Path::new("Report.XLSX")).unwrap(),
            InputKind::Xlsx
        );
        assert_eq!(
            InputKind::from_path(Path::new("notes.docx")).unwrap(),
            InputKind::Docx
        );
        assert_eq!(
            InputKind::from_path(Path::new("deck.Pptx")).unwrap(),
            InputKind::Pptx
        );
        assert_eq!(
            InputKind::from_path(Path::new("contacts.CSV")).unwrap(),
            InputKind::Csv
        );
        assert_eq!(
            InputKind::from_path(Path::new("export.tsv")).unwrap(),
            InputKind::Tsv
        );
    }

    #[test]
    fn rejects_unknown_and_missing_extensions() {
        assert!(InputKind::from_path(Path::new("report.pdf")).is_err());
        assert!(InputKind::from_path(Path::new("README")).is_err());
    }

    /// A zip containing empty parts with the given names.
    fn zip_with(parts: &[&str]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for part in parts {
            writer
                .start_file(*part, zip::write::SimpleFileOptions::default())
                .unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn recognizes_office_files_by_their_contents() {
        let kind = |parts: &[&str]| InputKind::from_contents(Cursor::new(zip_with(parts)));
        assert_eq!(
            kind(&["[Content_Types].xml", "xl/workbook.xml"]),
            Some(InputKind::Xlsx)
        );
        assert_eq!(kind(&["word/document.xml"]), Some(InputKind::Docx));
        assert_eq!(kind(&["ppt/presentation.xml"]), Some(InputKind::Pptx));
        assert_eq!(kind(&["hello.txt"]), None);
        assert_eq!(InputKind::from_contents(Cursor::new(b"not a zip")), None);
    }

    #[test]
    fn unknown_extensions_fall_back_to_the_contents() {
        let dir = tempfile::TempDir::new().unwrap();
        let office = dir.path().join("report.zip");
        std::fs::write(&office, zip_with(&["word/document.xml"])).unwrap();
        let not_office = dir.path().join("notes.zip");
        std::fs::write(&not_office, zip_with(&["notes.txt"])).unwrap();

        let office = Source::from_arg(&office).unwrap();
        assert_eq!(office.kind(None).unwrap(), InputKind::Docx);
        assert_eq!(office.kind(Some(InputKind::Docx)).unwrap(), InputKind::Docx);

        let err = Source::from_arg(&not_office)
            .unwrap()
            .kind(None)
            .unwrap_err();
        assert!(
            err.to_string().starts_with("unsupported input file"),
            "{err}"
        );
    }

    #[test]
    fn known_extensions_are_trusted_without_opening_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("book.xlsx");
        std::fs::write(&path, b"not really a workbook").unwrap();
        assert_eq!(
            Source::from_arg(&path).unwrap().kind(None).unwrap(),
            InputKind::Xlsx
        );
    }

    #[test]
    fn missing_files_and_directories_are_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        for path in [dir.path().join("missing.xlsx"), dir.path().to_path_buf()] {
            let err = Source::from_arg(&path).unwrap_err();
            assert!(err.to_string().starts_with("input file not found"), "{err}");
        }
    }

    #[test]
    fn detects_stdin_and_rejects_a_wrong_from() {
        let stdin = Source::Stdin(zip_with(&["word/document.xml"]));
        assert_eq!(stdin.kind(None).unwrap(), InputKind::Docx);
        assert_eq!(
            stdin.kind(Some(InputKind::Pptx)).unwrap_err().to_string(),
            "the input isn't a .pptx file (it looks like a .docx)"
        );

        let text = Source::Stdin(b"plain text".to_vec());
        assert!(text.kind(None).is_err());
        assert_eq!(
            text.kind(Some(InputKind::Xlsx)).unwrap_err().to_string(),
            "the input isn't a .xlsx file (it isn't an Office file)"
        );
    }

    #[test]
    fn csv_and_tsv_on_stdin_need_from_and_must_not_be_office_files() {
        let text = Source::Stdin(b"a,b\n1,2\n".to_vec());
        assert_eq!(text.kind(Some(InputKind::Csv)).unwrap(), InputKind::Csv);
        assert_eq!(text.kind(Some(InputKind::Tsv)).unwrap(), InputKind::Tsv);

        let workbook = Source::Stdin(zip_with(&["xl/workbook.xml"]));
        assert_eq!(
            workbook.kind(Some(InputKind::Csv)).unwrap_err().to_string(),
            "the input isn't a .csv file (it looks like a .xlsx)"
        );
    }

    #[test]
    fn names_per_sheet_files_after_the_input() {
        assert_eq!(
            Source::File(PathBuf::from("data/sales.xlsx")).stem(),
            "sales"
        );
        assert_eq!(Source::Stdin(Vec::new()).stem(), "stdin");
    }

    #[test]
    fn stops_reading_stdin_at_the_limit() {
        assert_eq!(read_at_most(&b"abc"[..], 3).unwrap(), b"abc");
        let err = read_at_most(&b"abcd"[..], 3).unwrap_err();
        assert!(
            matches!(err, ConvertError::StdinTooLarge { limit: 3 }),
            "{err:?}"
        );
    }

    #[test]
    fn stdin_can_be_read_more_than_once() {
        let source = Source::Stdin(b"abc".to_vec());
        for _ in 0..2 {
            let mut text = String::new();
            source.reader().unwrap().read_to_string(&mut text).unwrap();
            assert_eq!(text, "abc");
        }
    }

    #[test]
    fn documents_convert_to_markdown_or_pdf_and_sheets_to_text() {
        assert!(InputKind::Docx.check_output(OutputFormat::Markdown).is_ok());
        assert!(InputKind::Docx.check_output(OutputFormat::Pdf).is_ok());
        assert!(InputKind::Docx.check_output(OutputFormat::Csv).is_err());
        assert!(InputKind::Pptx.check_output(OutputFormat::Pdf).is_ok());
        assert!(InputKind::Pptx.check_output(OutputFormat::Json).is_err());
        assert!(InputKind::Xlsx.check_output(OutputFormat::Json).is_ok());
        assert!(InputKind::Csv.check_output(OutputFormat::Markdown).is_ok());
        assert!(InputKind::Tsv.check_output(OutputFormat::Csv).is_ok());
        assert_eq!(
            InputKind::Csv
                .check_output(OutputFormat::Pdf)
                .unwrap_err()
                .to_string(),
            "cannot convert csv to pdf; csv supports: csv, tsv, json, md"
        );
        assert_eq!(
            InputKind::Xlsx
                .check_output(OutputFormat::Pdf)
                .unwrap_err()
                .to_string(),
            "cannot convert xlsx to pdf; xlsx supports: csv, tsv, json, md"
        );
    }
}
