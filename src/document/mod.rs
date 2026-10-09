//! A small document model shared by the DOCX and PPTX readers: just enough structure
//! to write Markdown or a simple PDF.
//!
//! The model is generic over its images. A reader builds blocks of [`ImagePart`]s, naming each
//! image's part inside the package; [`resolve_images`] turns them into [`ImageRef`]s, the
//! links or keys a writer shows. The writers take only the second kind, so a block whose
//! images haven't been resolved can't reach one.

pub mod builder;
pub mod markdown;

use std::mem;

use crate::error::Result;

/// One top-level piece of a document, with images of type `I`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<I = ImageRef> {
    /// A heading, level 1 (largest) to 6.
    Heading {
        level: u8,
        runs: Vec<Run<I>>,
    },
    Paragraph(Vec<Run<I>>),
    /// A bulleted or numbered list item; `level` 0 is the outermost list.
    ListItem {
        kind: ListKind,
        level: u8,
        runs: Vec<Run<I>>,
    },
    /// Rows of cells. The first row is treated as the header row. Every span stays inside the
    /// table, and every covered cell belongs to exactly one cell above or to its left.
    Table(Vec<Vec<TableCell<I>>>),
    /// A horizontal rule, such as the break between two slides.
    Rule,
    /// A footnote or endnote, numbered as the runs that refer to it are. Notes come after the
    /// blocks that refer to them, and never hold notes of their own.
    Note {
        number: usize,
        blocks: Vec<Block<I>>,
    },
    /// Blocks repeated at the top of every page, which come before the document's own. They
    /// never hold headers, footers or notes.
    Header(Vec<Block<I>>),
    /// Blocks repeated at the bottom of every page, as for [`Block::Header`].
    Footer(Vec<Block<I>>),
}

impl<I> Block<I> {
    /// True for a header or footer, which only a paged output can show.
    pub fn is_page_furniture(&self) -> bool {
        matches!(self, Block::Header(_) | Block::Footer(_))
    }
}

/// The formatted text of one document table cell. Paragraphs inside the cell are separated by
/// `"\n"`. Not to be confused with [`crate::table::Cell`], a typed spreadsheet value.
pub type CellRuns<I = ImageRef> = Vec<Run<I>>;

/// One position in a document table's grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableCell<I = ImageRef> {
    /// A cell starting here, `cols` columns wide and `rows` rows tall: 1 and 1 unless merged.
    Content {
        runs: CellRuns<I>,
        cols: usize,
        rows: usize,
    },
    /// Part of a merged cell that starts above or to the left.
    Covered,
}

impl<I> TableCell<I> {
    /// A cell that isn't merged with any other.
    pub fn new(runs: CellRuns<I>) -> Self {
        TableCell::Content {
            runs,
            cols: 1,
            rows: 1,
        }
    }

    /// The cell's text, or nothing if it's covered.
    pub fn runs(&self) -> &[Run<I>] {
        match self {
            TableCell::Content { runs, .. } => runs,
            TableCell::Covered => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// A stretch of text that shares the same formatting (and link, if any).
///
/// A run can instead be an image: then `image` says where it is, and `text` is its alt text.
/// Or it can refer to a note: then `note` is the [`Block::Note`]'s number, and `text` is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<I = ImageRef> {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
    pub image: Option<I>,
    pub note: Option<usize>,
}

impl<I> Default for Run<I> {
    fn default() -> Self {
        Run::new("", RunStyle::default())
    }
}

/// An image as a reader finds it: the part inside the package that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePart {
    /// The image's part inside the package (`word/media/image1.png`).
    pub part: String,
    /// The size the document shows it at, width then height, in EMU (914,400 to the inch).
    /// `None` if the document doesn't give one, or gives one [`display_size`] rejects.
    pub size: Option<(u32, u32)>,
}

/// An image as a writer shows it, once [`resolve_images`] has dealt with its part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// The link written into the Markdown, or for a PDF, the key of its bytes in
    /// [`EmbeddedImages`](crate::images::EmbeddedImages).
    pub source: String,
    /// The [`ImagePart`]'s size. Only PDF uses it: Markdown image syntax has no size.
    pub size: Option<(u32, u32)>,
}

impl ImagePart {
    /// The image stored at `part`, with no display size until a reader sets one.
    pub fn new(part: impl Into<String>) -> Self {
        ImagePart {
            part: part.into(),
            size: None,
        }
    }
}

impl ImageRef {
    /// An image found at `source`, with no display size.
    #[cfg(test)]
    pub fn new(source: impl Into<String>) -> Self {
        ImageRef {
            source: source.into(),
            size: None,
        }
    }
}

/// EMU per point: Office measures sizes in English Metric Units.
#[cfg(feature = "pdf")]
pub const EMU_PER_POINT: u32 = 12_700;

/// The largest display size kept, per side: 100 inches. Anything larger, like a size of zero,
/// is a broken or hostile file rather than a real picture.
const MAX_DISPLAY_EMU: u64 = 100 * 914_400;

/// A picture's display size from the `cx` and `cy` attributes DOCX and PPTX store it in, or
/// `None` unless both are whole numbers above zero and at most 100 inches.
pub fn display_size(cx: Option<String>, cy: Option<String>) -> Option<(u32, u32)> {
    let side = |v: Option<String>| {
        let emu = v?.parse::<u64>().ok()?;
        (1..=MAX_DISPLAY_EMU).contains(&emu).then_some(emu as u32)
    };
    Some((side(cx)?, side(cy)?))
}

/// Character formatting we carry over to Markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
}

impl<I> Run<I> {
    pub fn new(text: impl Into<String>, style: RunStyle) -> Self {
        Run {
            text: text.into(),
            style,
            link: None,
            image: None,
            note: None,
        }
    }

    /// A reference to the note numbered `number`.
    pub fn note(number: usize) -> Self {
        Run {
            note: Some(number),
            ..Run::default()
        }
    }

    /// An image run: `image` says where the image is, `alt` describes it.
    pub fn image(image: I, alt: impl Into<String>) -> Self {
        Run {
            text: alt.into(),
            image: Some(image),
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
    /// Images and note references never merge: each one is its own run.
    pub fn same_format(&self, other: &Run<I>) -> bool {
        self.image.is_none()
            && other.image.is_none()
            && self.note.is_none()
            && other.note.is_none()
            && self.style == other.style
            && self.link == other.link
    }
}

/// Adds `run` to `runs`, merging it into the last run when the formatting matches.
///
/// Office apps often split one word across several runs (spell-check, edits), so merging
/// keeps the Markdown clean: `**Hello**`, not `**Hel****lo**`.
pub fn append_run<I>(runs: &mut Vec<Run<I>>, run: Run<I>) {
    match runs.last_mut() {
        Some(last) if last.same_format(&run) => last.text.push_str(&run.text),
        _ => runs.push(run),
    }
}

/// Adds a paragraph's runs to a table cell, on a new line if the cell already has text.
pub fn append_paragraph<I>(cell: &mut CellRuns<I>, runs: Vec<Run<I>>) {
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

/// True if the runs contain nothing but whitespace (an image or a note reference counts as
/// content).
pub fn is_blank<I>(runs: &[Run<I>]) -> bool {
    runs.iter()
        .all(|r| r.image.is_none() && r.note.is_none() && r.text.trim().is_empty())
}

/// Turns each image's package part into the link or key `export` returns for it.
///
/// `export` returns `None` to leave an image out. Blocks left with nothing in them are removed,
/// except notes, which stay so that the runs referring to them still have something to refer to.
pub fn resolve_images(
    blocks: Vec<Block<ImagePart>>,
    mut export: impl FnMut(&str) -> Result<Option<String>>,
) -> Result<Vec<Block>> {
    resolve_blocks(blocks, &mut export)
}

fn resolve_blocks(
    blocks: Vec<Block<ImagePart>>,
    export: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<Vec<Block>> {
    let mut resolved = Vec::with_capacity(blocks.len());
    for block in blocks {
        let block = match block {
            Block::Heading { level, runs } => Block::Heading {
                level,
                runs: resolve_runs(runs, export)?,
            },
            Block::Paragraph(runs) => Block::Paragraph(resolve_runs(runs, export)?),
            Block::ListItem { kind, level, runs } => Block::ListItem {
                kind,
                level,
                runs: resolve_runs(runs, export)?,
            },
            Block::Table(rows) => Block::Table(
                rows.into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|cell| resolve_cell(cell, export))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ),
            Block::Rule => Block::Rule,
            Block::Note { number, blocks } => Block::Note {
                number,
                blocks: resolve_blocks(blocks, export)?,
            },
            Block::Header(blocks) => Block::Header(resolve_blocks(blocks, export)?),
            Block::Footer(blocks) => Block::Footer(resolve_blocks(blocks, export)?),
        };
        let empty = match &block {
            Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
                is_blank(runs)
            }
            Block::Header(blocks) | Block::Footer(blocks) => blocks.is_empty(),
            Block::Table(_) | Block::Rule | Block::Note { .. } => false,
        };
        if !empty {
            resolved.push(block);
        }
    }
    Ok(resolved)
}

fn resolve_cell(
    cell: TableCell<ImagePart>,
    export: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<TableCell> {
    Ok(match cell {
        TableCell::Content { runs, cols, rows } => TableCell::Content {
            runs: resolve_runs(runs, export)?,
            cols,
            rows,
        },
        TableCell::Covered => TableCell::Covered,
    })
}

fn resolve_runs(
    runs: Vec<Run<ImagePart>>,
    export: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<Vec<Run>> {
    let mut resolved = Vec::with_capacity(runs.len());
    for run in runs {
        let image = match run.image {
            Some(image) => match export(&image.part)? {
                Some(source) => Some(ImageRef {
                    source,
                    size: image.size,
                }),
                None => continue,
            },
            None => None,
        };
        let run = Run {
            text: run.text,
            style: run.style,
            link: run.link,
            image,
            note: run.note,
        };
        append_run(&mut resolved, run);
    }
    Ok(resolved)
}

/// Collects a table's cells as [`builder::BlockBuilder`] walks through its rows.
#[derive(Debug, Default)]
pub struct TableBuilder {
    rows: Vec<Vec<Slot>>,
    row: Vec<Slot>,
    /// The cell being filled in; add paragraphs to it with [`append_paragraph`].
    pub cell: CellRuns<ImagePart>,
    /// How many columns the table's grid declares (`<w:gridCol>` or `<a:gridCol>`). A merged
    /// cell never spans past them.
    pub columns: usize,
    /// Whether the file lists every cell a merge covers, as PowerPoint does, marking each with
    /// `hMerge` or `vMerge`. Word instead leaves out the columns a `gridSpan` covers, so
    /// [`end_cell`](Self::end_cell) adds them.
    pub lists_merged_cells: bool,
    /// How many columns the cell being filled in says it spans (`gridSpan`). Reset after each
    /// cell.
    pub span: usize,
    /// How many rows the cell being filled in says it spans (PowerPoint's `rowSpan`). Word
    /// doesn't say, so a Word cell spans as many rows as continue it. Reset after each cell.
    pub row_span: usize,
    /// Whether the cell being filled in is marked as part of a neighbor. Reset after each cell.
    pub merged: Merged,
}

/// How a table cell is marked as part of a merged cell that starts elsewhere.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Merged {
    #[default]
    No,
    /// Part of the cell to its left: PowerPoint's `hMerge`.
    Left,
    /// Part of the cell above: PowerPoint's `vMerge`, or Word's `<w:vMerge/>` without
    /// `restart`.
    Up,
    /// Part of a cell above and to the left: both `hMerge` and `vMerge`.
    Both,
}

/// One position in the grid as the file marks it, before [`TableBuilder::finish`] works out
/// which cell covers what.
#[derive(Debug)]
enum Slot {
    Cell {
        runs: CellRuns<ImagePart>,
        cols: usize,
        rows: usize,
    },
    Merged(Merged),
}

impl TableBuilder {
    /// Moves the finished cell into the current row. In a Word table, a cell spanning several
    /// columns is followed by cells marking the columns it covers.
    pub fn end_cell(&mut self) {
        let runs = mem::take(&mut self.cell);
        let span = mem::take(&mut self.span).max(1);
        let rows = mem::take(&mut self.row_span).max(1);
        let merged = match mem::take(&mut self.merged) {
            // Word shows the text of a cell that continues a merge, so keep it as a cell of its
            // own rather than lose it.
            Merged::Up if !self.lists_merged_cells && !is_blank(&runs) => Merged::No,
            merged => merged,
        };
        self.row.push(match merged {
            Merged::No => Slot::Cell {
                runs,
                cols: span,
                rows: if self.lists_merged_cells {
                    rows
                } else {
                    usize::MAX
                },
            },
            merged => Slot::Merged(merged),
        });
        if !self.lists_merged_cells {
            // Bounded by the grid, which costs the file bytes for every column, so a huge
            // `gridSpan` value can't make a huge row.
            let room = self.columns.saturating_sub(self.row.len());
            let extra = (span - 1).min(room);
            self.row
                .extend((0..extra).map(|_| Slot::Merged(Merged::Left)));
        }
    }

    /// Moves the finished row into the table.
    pub fn end_row(&mut self) {
        let row = mem::take(&mut self.row);
        self.rows.push(row);
    }

    /// The finished table: each cell spans as far as the cells marked as part of it go, up to
    /// the span it declares.
    ///
    /// Merges only ever claim cells the file marks as merged, so no span passes the table or
    /// overlaps another cell, and a merged mark nothing claims becomes an empty cell. It's one
    /// pass: each cell is claimed at most once, and a failed claim stops at the first cell
    /// that isn't marked.
    pub fn finish(self) -> Vec<Vec<TableCell<ImagePart>>> {
        let slots = self.rows;
        let mut claimed: Vec<Vec<bool>> = slots.iter().map(|row| vec![false; row.len()]).collect();
        let mut spans = Vec::new();
        for (r, row) in slots.iter().enumerate() {
            for (c, slot) in row.iter().enumerate() {
                let &Slot::Cell { cols, rows, .. } = slot else {
                    continue;
                };
                let width = 1 + row[c + 1..]
                    .iter()
                    .zip(&claimed[r][c + 1..])
                    .take(cols - 1)
                    .take_while(|&(slot, &claimed)| {
                        matches!(slot, Slot::Merged(Merged::Left)) && !claimed
                    })
                    .count();
                claimed[r][c + 1..c + width].fill(true);

                let mut height = 1;
                while height < rows {
                    let below = r + height;
                    let Some(next) = slots.get(below) else {
                        break;
                    };
                    let covers = next.len() >= c + width
                        && matches!(next[c], Slot::Merged(Merged::Up | Merged::Both))
                        && next[c + 1..c + width]
                            .iter()
                            .all(|slot| matches!(slot, Slot::Merged(_)))
                        && claimed[below][c..c + width].iter().all(|&claimed| !claimed);
                    if !covers {
                        break;
                    }
                    claimed[below][c..c + width].fill(true);
                    height += 1;
                }
                spans.push((width, height));
            }
        }

        let mut spans = spans.into_iter();
        slots
            .into_iter()
            .zip(claimed)
            .map(|(row, claimed)| {
                row.into_iter()
                    .zip(claimed)
                    .map(|(slot, claimed)| match slot {
                        Slot::Cell { runs, .. } => {
                            let (cols, rows) = spans.next().expect("a span for every cell");
                            TableCell::Content { runs, cols, rows }
                        }
                        Slot::Merged(_) if claimed => TableCell::Covered,
                        Slot::Merged(_) => TableCell::new(CellRuns::new()),
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_content_and_never_merge() {
        let image = Run::image(ImagePart::new("word/media/a.png"), "");
        assert!(!is_blank(std::slice::from_ref(&image)));
        assert!(!image.same_format(&Run::image(ImagePart::new("word/media/a.png"), "")));
    }

    #[test]
    fn resolve_images_links_exports_and_drops_the_rest() {
        let part = |part: &str, alt: &str| Run::image(ImagePart::new(part), alt);
        let blocks = vec![
            Block::Paragraph(vec![
                Run::new("Logo: ", RunStyle::default()),
                part("word/media/logo.png", "Logo"),
            ]),
            Block::Paragraph(vec![part("word/media/missing.emf", "")]),
            Block::Table(vec![vec![
                TableCell::new(vec![part("word/media/logo.png", "again")]),
                TableCell::Covered,
            ]]),
        ];

        let blocks = resolve_images(blocks, |part| {
            Ok(part.ends_with(".png").then(|| "img/logo.png".to_string()))
        })
        .unwrap();

        let link = |alt: &str| Run::image(ImageRef::new("img/logo.png"), alt);
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![Run::new("Logo: ", RunStyle::default()), link("Logo")]),
                Block::Table(vec![vec![
                    TableCell::new(vec![link("again")]),
                    TableCell::Covered,
                ]]),
            ]
        );
    }

    #[test]
    fn resolve_images_reaches_into_notes_and_keeps_them() {
        let blocks = vec![
            Block::Paragraph(vec![Run::new("See", RunStyle::default()), Run::note(1)]),
            Block::Note {
                number: 1,
                blocks: vec![Block::Paragraph(vec![Run::image(
                    ImagePart::new("word/media/chart.emf"),
                    "",
                )])],
            },
        ];

        let blocks = resolve_images(blocks, |_| Ok(None)).unwrap();

        // The note's only paragraph is gone with its image, but the note stays for the
        // reference to point at.
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![Run::new("See", RunStyle::default()), Run::note(1)]),
                Block::Note {
                    number: 1,
                    blocks: vec![],
                },
            ]
        );
    }

    #[test]
    fn keeps_display_sizes_only_within_bounds() {
        let size = |cx: &str, cy: &str| display_size(Some(cx.into()), Some(cy.into()));
        assert_eq!(size("914400", "457200"), Some((914_400, 457_200)));
        assert_eq!(size("91440000", "1"), Some((91_440_000, 1)));
        // Zero, negative, past 100 inches, not a whole number, or past u64.
        assert_eq!(size("0", "457200"), None);
        assert_eq!(size("914400", "-1"), None);
        assert_eq!(size("91440001", "457200"), None);
        assert_eq!(size("1e6", "457200"), None);
        assert_eq!(size("99999999999999999999999", "1"), None);
        assert_eq!(display_size(None, Some("914400".into())), None);
    }

    #[test]
    fn resolving_an_image_keeps_its_size() {
        let image = ImagePart {
            part: "word/media/logo.png".into(),
            size: Some((914_400, 457_200)),
        };
        let blocks = vec![Block::Paragraph(vec![Run::image(image, "Logo")])];

        let blocks = resolve_images(blocks, |_| Ok(Some("img/logo.png".to_string()))).unwrap();

        let image = ImageRef {
            source: "img/logo.png".into(),
            size: Some((914_400, 457_200)),
        };
        assert_eq!(blocks, [Block::Paragraph(vec![Run::image(image, "Logo")])]);
    }
}
