//! Where converted output goes: stdout, a file, or one file per sheet.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

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
///
/// Different sheet names can make the same file name (`a|b` and `a_b`), so `names` adds a
/// number to a clash: `out/sales-a_b-2.csv`.
pub fn sheet_output_path(
    dir: &Path,
    stem: &str,
    sheet: &str,
    format: OutputFormat,
    names: &mut UniqueNames,
) -> PathBuf {
    let file_name = format!("{}-{}.{}", stem, safe_file_name(sheet), format.extension());
    dir.join(names.claim(&file_name))
}

/// Hands out file names not yet used in one folder.
#[derive(Debug, Default)]
pub struct UniqueNames {
    /// Names taken so far, as [`same_file_key`] gives them.
    taken: HashSet<String>,
}

impl UniqueNames {
    /// `name`, or `name-2`, `name-3`, ... (before the extension) if it's taken.
    pub fn claim(&mut self, name: &str) -> String {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (name, String::new()),
        };

        let mut candidate = name.to_string();
        let mut n = 2;
        while !self.taken.insert(same_file_key(&candidate)) {
            candidate = format!("{stem}-{n}{ext}");
            n += 1;
        }
        candidate
    }
}

/// A form of `name` that's the same for every name a file system could treat as the same file.
///
/// macOS and Windows ignore case, and macOS also treats different Unicode spellings of the same
/// text as one name: `é` as one code point, or as `e` plus a combining accent. So the name is
/// lowercased, then normalized to NFC (one code point wherever possible).
fn same_file_key(name: &str) -> String {
    name.to_lowercase().nfc().collect()
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
        let mut names = UniqueNames::default();
        let mut path = |sheet| {
            sheet_output_path(
                Path::new("out"),
                "sales",
                sheet,
                OutputFormat::Markdown,
                &mut names,
            )
        };
        assert_eq!(path("Q1 2026"), Path::new("out/sales-Q1 2026.md"));
        assert_eq!(path("a|b"), Path::new("out/sales-a_b.md"));
        assert_eq!(path("a_b"), Path::new("out/sales-a_b-2.md"));
    }

    #[test]
    fn numbers_names_that_differ_only_in_unicode_normalization() {
        // Both display as "Café": one code point for é, or e plus a combining accent. macOS
        // treats them as the same file name.
        let mut names = UniqueNames::default();
        assert_eq!(names.claim("Caf\u{e9}.csv"), "Caf\u{e9}.csv");
        assert_eq!(names.claim("Cafe\u{301}.csv"), "Cafe\u{301}-2.csv");
        assert_eq!(names.claim("CAFE\u{301}.csv"), "CAFE\u{301}-3.csv");
    }

    #[test]
    fn numbers_clashing_names_ignoring_case() {
        let mut names = UniqueNames::default();
        assert_eq!(names.claim("image.png"), "image.png");
        assert_eq!(names.claim("Image.PNG"), "Image-2.PNG");
        assert_eq!(names.claim("image.png"), "image-3.png");
        assert_eq!(names.claim("README"), "README");
        assert_eq!(names.claim("readme"), "readme-2");
        assert_eq!(names.claim(".hidden"), ".hidden");
        assert_eq!(names.claim(".hidden"), ".hidden-2");
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
