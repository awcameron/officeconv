//! Where converted output goes: stdout, a file, or one file per sheet.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{ConvertError, Result};
use crate::format::OutputFormat;

/// Opens the file at `path`, or stdout when there's no path. Either way, output is buffered.
pub fn open_output(path: Option<&Path>) -> Result<Box<dyn Write>> {
    match path {
        Some(path) => {
            let file = File::create(path).map_err(|source| ConvertError::CreateOutput {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(Box::new(BufWriter::new(file)))
        }
        None => Ok(Box::new(BufWriter::new(io::stdout().lock()))),
    }
}

/// The folder a Markdown file written to `output` lives in: its parent, or the current
/// directory when writing to stdout.
pub fn markdown_dir(output: Option<&Path>) -> &Path {
    output
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Creates `dir` (and any missing parents) if it doesn't exist yet.
pub fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|source| ConvertError::CreateOutput {
        path: dir.to_path_buf(),
        source,
    })
}

/// Builds `<dir>/<stem>-<sheet>.<ext>`, e.g. `out/sales-Q1.csv`.
pub fn sheet_output_path(dir: &Path, stem: &str, sheet: &str, format: OutputFormat) -> PathBuf {
    let file_name = format!("{}-{}.{}", stem, safe_file_name(sheet), format.extension());
    dir.join(file_name)
}

/// Replaces characters that aren't allowed in file names on common systems.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    // Windows also dislikes names ending in a dot or space.
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "sheet".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sheet_paths() {
        let path = sheet_output_path(Path::new("out"), "sales", "Q1 2026", OutputFormat::Markdown);
        assert_eq!(path, Path::new("out/sales-Q1 2026.md"));
    }

    #[test]
    fn finds_the_markdown_folder() {
        assert_eq!(markdown_dir(None), Path::new("."));
        assert_eq!(markdown_dir(Some(Path::new("notes.md"))), Path::new("."));
        assert_eq!(
            markdown_dir(Some(Path::new("out/notes.md"))),
            Path::new("out")
        );
    }

    #[test]
    fn makes_sheet_names_safe() {
        assert_eq!(safe_file_name("Q1/Q2"), "Q1_Q2");
        assert_eq!(safe_file_name("Notes: draft?"), "Notes_ draft_");
        assert_eq!(safe_file_name("  trailing. "), "trailing");
        assert_eq!(safe_file_name("..."), "sheet");
    }
}
