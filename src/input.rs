//! Working out what kind of file we were given.

use std::fmt;
use std::path::Path;

use crate::cli::OutputFormat;
use crate::error::{ConvertError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Xlsx,
    Docx,
    Pptx,
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
            _ => Err(ConvertError::UnsupportedInput(path.to_path_buf())),
        }
    }

    /// Returns an error if this input can't be converted to `to`.
    pub fn check_output(self, to: OutputFormat) -> Result<()> {
        match (self, to) {
            (InputKind::Xlsx, _) => Ok(()),
            (InputKind::Docx | InputKind::Pptx, OutputFormat::Markdown) => Ok(()),
            (InputKind::Docx | InputKind::Pptx, _) => Err(ConvertError::UnsupportedConversion {
                input: self,
                to,
                supported: "md",
            }),
        }
    }
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            InputKind::Xlsx => "xlsx",
            InputKind::Docx => "docx",
            InputKind::Pptx => "pptx",
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
    }

    #[test]
    fn rejects_unknown_and_missing_extensions() {
        assert!(InputKind::from_path(Path::new("report.pdf")).is_err());
        assert!(InputKind::from_path(Path::new("README")).is_err());
    }

    #[test]
    fn documents_only_convert_to_markdown() {
        assert!(InputKind::Docx.check_output(OutputFormat::Markdown).is_ok());
        assert!(InputKind::Docx.check_output(OutputFormat::Csv).is_err());
        assert!(InputKind::Pptx.check_output(OutputFormat::Markdown).is_ok());
        assert!(InputKind::Pptx.check_output(OutputFormat::Json).is_err());
        assert!(InputKind::Xlsx.check_output(OutputFormat::Json).is_ok());
    }
}
