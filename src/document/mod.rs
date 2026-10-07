//! A small document model shared by the DOCX and PPTX readers: just enough structure
//! to write Markdown or a simple PDF.

pub mod markdown;

use std::mem;

use crate::error::Result;

/// One top-level piece of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading, level 1 (largest) to 6.
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    /// A bulleted or numbered list item; `level` 0 is the outermost list.
    ListItem {
        kind: ListKind,
        level: u8,
        runs: Vec<Run>,
    },
    /// Rows of cells. The first row is treated as the header row.
    Table(Vec<Vec<CellRuns>>),
    /// A horizontal rule, such as the break between two slides.
    Rule,
}

/// The formatted text of one document table cell. Paragraphs inside the cell are separated by
/// `"\n"`. Not to be confused with [`crate::table::Cell`], a typed spreadsheet value.
pub type CellRuns = Vec<Run>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// A stretch of text that shares the same formatting (and link, if any).
///
/// A run can instead be an image: then `image` holds where it is, and `text` is its alt text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
    /// While reading: the image's part inside the package (`word/media/image1.png`).
    /// After [`resolve_images`]: the link written into the Markdown, or for a PDF, the key of
    /// its bytes in [`EmbeddedImages`](crate::images::EmbeddedImages).
    pub image: Option<String>,
}

/// Character formatting we carry over to Markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
}

impl Run {
    pub fn new(text: impl Into<String>, style: RunStyle) -> Self {
        Run {
            text: text.into(),
            style,
            link: None,
            image: None,
        }
    }

    /// An image run: `source` is where the image is, `alt` describes it.
    pub fn image(source: impl Into<String>, alt: impl Into<String>) -> Self {
        Run {
            text: alt.into(),
            image: Some(source.into()),
            ..Run::default()
        }
    }

    /// The same run, pointing at `url`. The readers set links field by field; tests use this.
    #[cfg(test)]
    pub fn linked(self, url: impl Into<String>) -> Self {
        Run {
            link: Some(url.into()),
            ..self
        }
    }

    /// True if `other` has the same formatting and link, so the two can be merged.
    /// Images never merge: each one is its own run.
    pub fn same_format(&self, other: &Run) -> bool {
        self.image.is_none()
            && other.image.is_none()
            && self.style == other.style
            && self.link == other.link
    }
}

/// Adds `run` to `runs`, merging it into the last run when the formatting matches.
///
/// Office apps often split one word across several runs (spell-check, edits), so merging
/// keeps the Markdown clean: `**Hello**`, not `**Hel****lo**`.
pub fn append_run(runs: &mut Vec<Run>, run: Run) {
    match runs.last_mut() {
        Some(last) if last.same_format(&run) => last.text.push_str(&run.text),
        _ => runs.push(run),
    }
}

/// Adds a paragraph's runs to a table cell, on a new line if the cell already has text.
pub fn append_paragraph(cell: &mut CellRuns, runs: Vec<Run>) {
    if is_blank(&runs) {
        return;
    }
    if !cell.is_empty() {
        append_run(cell, Run::new("\n", RunStyle::default()));
    }
    for run in runs {
        append_run(cell, run);
    }
}

/// True if the runs contain nothing but whitespace (an image counts as content).
pub fn is_blank(runs: &[Run]) -> bool {
    runs.iter()
        .all(|r| r.image.is_none() && r.text.trim().is_empty())
}

/// Replaces each image's package part with the link `export` returns for it.
///
/// `export` returns `None` to leave an image out. Blocks left with nothing in them are removed.
pub fn resolve_images(
    blocks: &mut Vec<Block>,
    mut export: impl FnMut(&str) -> Result<Option<String>>,
) -> Result<()> {
    for block in blocks.iter_mut() {
        match block {
            Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
                resolve_runs(runs, &mut export)?;
            }
            Block::Table(rows) => {
                for cell in rows.iter_mut().flatten() {
                    resolve_runs(cell, &mut export)?;
                }
            }
            Block::Rule => {}
        }
    }

    blocks.retain(|block| match block {
        Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
            !is_blank(runs)
        }
        Block::Table(_) | Block::Rule => true,
    });
    Ok(())
}

fn resolve_runs(
    runs: &mut Vec<Run>,
    export: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<()> {
    let mut resolved = Vec::with_capacity(runs.len());
    for mut run in runs.drain(..) {
        if let Some(part) = &run.image {
            match export(part)? {
                Some(link) => run.image = Some(link),
                None => continue,
            }
        }
        append_run(&mut resolved, run);
    }
    *runs = resolved;
    Ok(())
}

/// Collects a table's cells as a reader walks through its rows.
#[derive(Debug, Default)]
pub struct TableBuilder {
    pub rows: Vec<Vec<CellRuns>>,
    row: Vec<CellRuns>,
    /// The cell being filled in; add paragraphs to it with [`append_paragraph`].
    pub cell: CellRuns,
    /// How many columns the table's grid declares (`<w:gridCol>` or `<a:gridCol>`). A merged
    /// cell never spans past them.
    pub columns: usize,
    /// How many grid columns the cell being filled in covers, from Word's `<w:gridSpan>`.
    /// Reset after each cell.
    pub span: usize,
    /// Whether the cell being filled in is covered by a merged neighbor, as PowerPoint marks
    /// with `hMerge` or `vMerge`. PowerPoint hides its text, so it's read as empty. Reset
    /// after each cell.
    pub covered: bool,
}

impl TableBuilder {
    /// Moves the finished cell into the current row. A cell merged across columns fills the
    /// first of them, and the rest stay empty so later cells keep their columns.
    pub fn end_cell(&mut self) {
        let cell = mem::take(&mut self.cell);
        if mem::take(&mut self.covered) {
            self.row.push(CellRuns::new());
        } else {
            self.row.push(cell);
        }
        // Bounded by the grid, which costs the file bytes for every column, so a huge
        // `gridSpan` value can't make a huge row.
        let room = self.columns.saturating_sub(self.row.len());
        let extra = mem::take(&mut self.span).saturating_sub(1).min(room);
        self.row.resize(self.row.len() + extra, CellRuns::new());
    }

    /// Moves the finished row into the table.
    pub fn end_row(&mut self) {
        let row = mem::take(&mut self.row);
        self.rows.push(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_content_and_never_merge() {
        let image = Run::image("word/media/a.png", "");
        assert!(!is_blank(std::slice::from_ref(&image)));
        assert!(!image.same_format(&Run::image("word/media/a.png", "")));
    }

    #[test]
    fn resolve_images_links_exports_and_drops_the_rest() {
        let mut blocks = vec![
            Block::Paragraph(vec![
                Run::new("Logo: ", RunStyle::default()),
                Run::image("word/media/logo.png", "Logo"),
            ]),
            Block::Paragraph(vec![Run::image("word/media/missing.emf", "")]),
            Block::Table(vec![vec![vec![Run::image("word/media/logo.png", "again")]]]),
        ];

        resolve_images(&mut blocks, |part| {
            Ok(part.ends_with(".png").then(|| "img/logo.png".to_string()))
        })
        .unwrap();

        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![
                    Run::new("Logo: ", RunStyle::default()),
                    Run::image("img/logo.png", "Logo"),
                ]),
                Block::Table(vec![vec![vec![Run::image("img/logo.png", "again")]]]),
            ]
        );
    }
}
