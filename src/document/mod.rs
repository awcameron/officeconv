//! A small document model shared by the DOCX and PPTX readers: just enough structure
//! to write Markdown or a simple PDF.
//!
//! The model is generic over its images. A reader builds blocks of [`ImagePart`]s, naming each
//! image's part inside the package; [`resolve_images`] turns them into [`ImageRef`]s, the
//! links or keys a writer shows. The writers take only the second kind, so a block whose
//! images haven't been resolved can't reach one.

pub mod builder;
pub mod markdown;
pub mod table_builder;

use crate::error::Result;

/// One top-level piece of a document, with images of type `I`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<I = ImageRef> {
    /// A heading, level 1 (largest) to 6.
    Heading {
        level: u8,
        runs: Vec<Run<I>>,
        align: Align,
    },
    Paragraph {
        runs: Vec<Run<I>>,
        align: Align,
    },
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

/// How a paragraph's lines line up. Only PDF shows it: Markdown has no alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// Stretched to both edges. PDF lays it out as [`Align::Left`] for now.
    Justify,
}

impl<I> Block<I> {
    /// A left-aligned paragraph.
    pub fn paragraph(runs: Vec<Run<I>>) -> Self {
        Block::Paragraph {
            runs,
            align: Align::Left,
        }
    }

    /// A list item of `kind` at `level`, or a paragraph aligned `align` if it isn't in a list.
    pub fn list_item_or_paragraph(
        kind: Option<ListKind>,
        level: u8,
        align: Align,
        runs: Vec<Run<I>>,
    ) -> Self {
        match kind {
            Some(kind) => Block::ListItem { kind, level, runs },
            None => Block::Paragraph { runs, align },
        }
    }

    /// A left-aligned heading at `level`.
    pub fn heading(level: u8, runs: Vec<Run<I>>) -> Self {
        Block::Heading {
            level,
            runs,
            align: Align::Left,
        }
    }

    /// True for a header or footer, which only a paged output can show.
    pub fn is_page_furniture(&self) -> bool {
        matches!(self, Block::Header(_) | Block::Footer(_))
    }

    /// The runs of a heading, paragraph or list item, or `None` for any other block.
    pub fn text_runs(&self) -> Option<&Vec<Run<I>>> {
        match self {
            Block::Heading { runs, .. }
            | Block::Paragraph { runs, .. }
            | Block::ListItem { runs, .. } => Some(runs),
            _ => None,
        }
    }

    /// The runs of a heading, paragraph or list item, to change, or `None` for any other block.
    #[cfg(any(feature = "pdf", test))]
    pub fn text_runs_mut(&mut self) -> Option<&mut Vec<Run<I>>> {
        match self {
            Block::Heading { runs, .. }
            | Block::Paragraph { runs, .. }
            | Block::ListItem { runs, .. } => Some(runs),
            _ => None,
        }
    }

    /// Every run in the block, a table's cell by cell. Not the runs of the blocks a note,
    /// header or footer holds: those are blocks of their own.
    #[cfg(any(feature = "pdf", test))]
    pub fn runs(&self) -> impl Iterator<Item = &Run<I>> {
        let (runs, rows) = match self {
            Block::Table(rows) => (None, Some(rows)),
            block => (block.text_runs(), None),
        };
        let cells = rows
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(TableCell::runs);
        runs.into_iter().flatten().chain(cells)
    }

    /// Every run in the block, to change, as for [`runs`](Self::runs).
    #[cfg(any(feature = "pdf", test))]
    pub fn runs_mut(&mut self) -> impl Iterator<Item = &mut Run<I>> {
        let (runs, rows) = match self {
            Block::Table(rows) => (None, Some(rows)),
            block => (block.text_runs_mut(), None),
        };
        let cells = rows
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|cell| match cell {
                TableCell::Content { runs, .. } => runs.as_mut_slice(),
                TableCell::Covered => &mut [],
            });
        runs.into_iter().flatten().chain(cells)
    }

    /// The same block with each list of runs, here and in the blocks it holds, replaced by
    /// what `f` returns for it. This is how a block's images change type.
    pub fn map_runs<J>(
        self,
        f: &mut impl FnMut(Vec<Run<I>>) -> Result<Vec<Run<J>>>,
    ) -> Result<Block<J>> {
        let blocks = |blocks: Vec<Block<I>>, f: &mut _| -> Result<Vec<Block<J>>> {
            blocks.into_iter().map(|block| block.map_runs(f)).collect()
        };
        Ok(match self {
            Block::Heading { level, runs, align } => Block::Heading {
                level,
                runs: f(runs)?,
                align,
            },
            Block::Paragraph { runs, align } => Block::Paragraph {
                runs: f(runs)?,
                align,
            },
            Block::ListItem { kind, level, runs } => Block::ListItem {
                kind,
                level,
                runs: f(runs)?,
            },
            Block::Table(rows) => Block::Table(
                rows.into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|cell| {
                                Ok(match cell {
                                    TableCell::Content { runs, cols, rows } => TableCell::Content {
                                        runs: f(runs)?,
                                        cols,
                                        rows,
                                    },
                                    TableCell::Covered => TableCell::Covered,
                                })
                            })
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ),
            Block::Rule => Block::Rule,
            Block::Note { number, blocks: b } => Block::Note {
                number,
                blocks: blocks(b, f)?,
            },
            Block::Header(b) => Block::Header(blocks(b, f)?),
            Block::Footer(b) => Block::Footer(blocks(b, f)?),
        })
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

/// A stretch of text that shares the same formatting (and link, if any), or one of the other
/// kinds of run in [`RunKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<I = ImageRef> {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
    pub kind: RunKind<I>,
}

/// What a run is. Only text runs merge with their neighbors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunKind<I = ImageRef> {
    /// Text.
    Text,
    /// An image: the run's text is its alt text.
    Image(I),
    /// A reference to the [`Block::Note`] with this number. The run's text is empty.
    Note(usize),
    /// A page number for a paged output to fill in. The run's text is empty.
    Field(Field),
}

/// A number that depends on the page it's shown on. Readers only put these in headers and
/// footers, which only a paged output shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The number of the page.
    PageNumber,
    /// How many pages there are.
    PageCount,
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
    /// The image stored at `part`, with no display size. Readers build image runs with
    /// `builder::image_run`; tests use this.
    #[cfg(test)]
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
            kind: RunKind::Text,
        }
    }

    /// A page number field, with the style of the text around it.
    pub fn field(field: Field, style: RunStyle) -> Self {
        Run {
            kind: RunKind::Field(field),
            ..Run::new("", style)
        }
    }

    /// A reference to the note numbered `number`.
    pub fn note(number: usize) -> Self {
        Run {
            kind: RunKind::Note(number),
            ..Run::default()
        }
    }

    /// An image run: `image` says where the image is, `alt` describes it.
    pub fn image(image: I, alt: impl Into<String>) -> Self {
        Run {
            text: alt.into(),
            kind: RunKind::Image(image),
            ..Run::default()
        }
    }

    /// True for a text run.
    pub fn is_text(&self) -> bool {
        matches!(self.kind, RunKind::Text)
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
    /// Images, note references and fields never merge: each one is its own run.
    pub fn same_format(&self, other: &Run<I>) -> bool {
        self.is_text() && other.is_text() && self.style == other.style && self.link == other.link
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

/// True if the runs contain nothing but whitespace (an image, a note reference or a field counts
/// as content).
pub fn is_blank<I>(runs: &[Run<I>]) -> bool {
    runs.iter().all(|r| r.is_text() && r.text.trim().is_empty())
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
        // The blocks inside are resolved as a list of their own, so that empty ones go too.
        let block = match block {
            Block::Note { number, blocks } => Block::Note {
                number,
                blocks: resolve_blocks(blocks, export)?,
            },
            Block::Header(blocks) => Block::Header(resolve_blocks(blocks, export)?),
            Block::Footer(blocks) => Block::Footer(resolve_blocks(blocks, export)?),
            block => block.map_runs(&mut |runs| resolve_runs(runs, export))?,
        };
        let empty = match &block {
            Block::Header(blocks) | Block::Footer(blocks) => blocks.is_empty(),
            block => block.text_runs().is_some_and(|runs| is_blank(runs)),
        };
        if !empty {
            resolved.push(block);
        }
    }
    Ok(resolved)
}

fn resolve_runs(
    runs: Vec<Run<ImagePart>>,
    export: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<Vec<Run>> {
    let mut resolved = Vec::with_capacity(runs.len());
    for run in runs {
        let kind = match run.kind {
            RunKind::Image(image) => match export(&image.part)? {
                Some(source) => RunKind::Image(ImageRef {
                    source,
                    size: image.size,
                }),
                None => continue,
            },
            RunKind::Text => RunKind::Text,
            RunKind::Note(number) => RunKind::Note(number),
            RunKind::Field(field) => RunKind::Field(field),
        };
        let run = Run {
            text: run.text,
            style: run.style,
            link: run.link,
            kind,
        };
        append_run(&mut resolved, run);
    }
    Ok(resolved)
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

    fn text(text: &str) -> Run<ImagePart> {
        Run::new(text, RunStyle::default())
    }

    fn texts<'a, I: 'a>(runs: impl Iterator<Item = &'a Run<I>>) -> Vec<&'a str> {
        runs.map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn runs_go_through_a_tables_cells_row_by_row() {
        let mut table = Block::Table(vec![
            vec![
                TableCell::Content {
                    runs: vec![text("a1"), Run::note(1)],
                    cols: 2,
                    rows: 2,
                },
                TableCell::Covered,
                TableCell::new(vec![text("c1")]),
            ],
            vec![
                TableCell::Covered,
                TableCell::Covered,
                TableCell::new(vec![]),
            ],
            vec![
                TableCell::new(vec![text("a3")]),
                TableCell::new(vec![text("b3"), text("b3+")]),
                TableCell::Covered,
            ],
        ]);
        assert_eq!(table.text_runs(), None);
        assert_eq!(texts(table.runs()), ["a1", "", "c1", "a3", "b3", "b3+"]);

        for run in table.runs_mut() {
            run.text.make_ascii_uppercase();
        }
        assert_eq!(texts(table.runs()), ["A1", "", "C1", "A3", "B3", "B3+"]);
    }

    #[test]
    fn runs_are_a_paragraphs_own_and_not_a_notes() {
        let mut item = Block::ListItem {
            kind: ListKind::Bullet,
            level: 0,
            runs: vec![text("item")],
        };
        assert_eq!(texts(item.runs()), ["item"]);
        item.text_runs_mut().unwrap().push(text("!"));
        assert_eq!(texts(item.text_runs().unwrap().iter()), ["item", "!"]);

        let note = Block::Note {
            number: 1,
            blocks: vec![Block::paragraph(vec![text("inside")])],
        };
        assert_eq!(note.text_runs(), None);
        assert_eq!(note.runs().count(), 0);
        assert_eq!(Block::<ImagePart>::Rule.runs().count(), 0);
    }

    #[test]
    fn map_runs_reaches_every_list_of_runs_and_keeps_the_rest() {
        let block = Block::Header(vec![
            Block::heading(2, vec![text("h")]),
            Block::Table(vec![vec![
                TableCell::Content {
                    runs: vec![text("a")],
                    cols: 2,
                    rows: 1,
                },
                TableCell::Covered,
            ]]),
        ]);
        let mut seen = 0;
        let mapped = block
            .map_runs(&mut |runs: Vec<Run<ImagePart>>| {
                seen += 1;
                Ok(runs
                    .into_iter()
                    .map(|run| Run::<ImageRef>::new(run.text + "!", run.style))
                    .collect())
            })
            .unwrap();
        assert_eq!(seen, 2);
        assert_eq!(
            mapped,
            Block::Header(vec![
                Block::heading(2, vec![Run::new("h!", RunStyle::default())]),
                Block::Table(vec![vec![
                    TableCell::Content {
                        runs: vec![Run::new("a!", RunStyle::default())],
                        cols: 2,
                        rows: 1,
                    },
                    TableCell::Covered,
                ]]),
            ])
        );
    }

    #[test]
    fn resolve_images_links_exports_and_drops_the_rest() {
        let part = |part: &str, alt: &str| Run::image(ImagePart::new(part), alt);
        let blocks = vec![
            Block::paragraph(vec![
                Run::new("Logo: ", RunStyle::default()),
                part("word/media/logo.png", "Logo"),
            ]),
            Block::paragraph(vec![part("word/media/missing.emf", "")]),
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
                Block::paragraph(vec![Run::new("Logo: ", RunStyle::default()), link("Logo")]),
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
            Block::paragraph(vec![Run::new("See", RunStyle::default()), Run::note(1)]),
            Block::Note {
                number: 1,
                blocks: vec![Block::paragraph(vec![Run::image(
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
                Block::paragraph(vec![Run::new("See", RunStyle::default()), Run::note(1)]),
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
        let blocks = vec![Block::paragraph(vec![Run::image(image, "Logo")])];

        let blocks = resolve_images(blocks, |_| Ok(Some("img/logo.png".to_string()))).unwrap();

        let image = ImageRef {
            source: "img/logo.png".into(),
            size: Some((914_400, 457_200)),
        };
        assert_eq!(blocks, [Block::paragraph(vec![Run::image(image, "Logo")])]);
    }
}
