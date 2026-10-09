//! Laying out [`Block`]s on pages: wrapping text into lines, numbering lists, sizing tables
//! and images, and starting a new page when one is full.
//!
//! The result is a list of [`Page`]s holding positioned [`Item`]s, which `pdf::render` then
//! paints. Keeping the two apart means the layout can be tested without reading a PDF back.
//!
//! Coordinates are in points (1/72 inch), measured from the top-left corner of the page.

use std::borrow::Cow;
use std::collections::HashMap;
use std::mem;
use std::ops::Range;

use krilla::Data;
use krilla::image::Image;
use krilla::text::KrillaGlyph;
use unicode_linebreak::{BreakOpportunity, linebreaks};

use super::fonts::{FontId, Fonts};
use crate::document::{
    Align, Block, CellRuns, EMU_PER_POINT, Field, ImageRef, ListKind, Run, RunStyle, TableCell,
};
use crate::images::{EmbeddedImages, ImageFormat};

/// Page size, margins, and text size for one kind of document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin: f32,
    /// Font size of body text; headings are sized relative to it.
    pub body_size: f32,
    /// What a [`Block::Rule`] does.
    pub rule: RuleStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStyle {
    /// A horizontal line across the page.
    Line,
    /// Start a new page. PPTX puts a rule between slides, so each slide gets its own page.
    PageBreak,
}

impl PageSetup {
    /// A4 portrait with 1-inch margins, for Word documents.
    pub const DOCUMENT: PageSetup = PageSetup {
        width: 595.28,
        height: 841.89,
        margin: 72.0,
        body_size: 11.0,
        rule: RuleStyle::Line,
    };

    /// 16:9 landscape, the size of a default PowerPoint slide, with one slide per page.
    pub const SLIDES: PageSetup = PageSetup {
        width: 960.0,
        height: 540.0,
        margin: 48.0,
        body_size: 16.0,
        rule: RuleStyle::PageBreak,
    };

    fn content_width(&self) -> f32 {
        self.width - 2.0 * self.margin
    }

    fn bottom(&self) -> f32 {
        self.height - self.margin
    }
}

/// One laid-out page.
#[derive(Debug, Default, Clone)]
pub struct Page {
    pub items: Vec<Item>,
    pub links: Vec<Link>,
}

/// Something to paint, at its final position on the page.
#[derive(Debug, Clone)]
pub enum Item {
    Text(TextItem),
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        image: Image,
    },
    /// A thin line, for table borders and rules.
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    /// A light grey rectangle, behind a table's header row.
    Shade {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// Glyphs in one font and size, drawn from `x` along the baseline.
#[derive(Debug, Clone)]
pub struct TextItem {
    pub x: f32,
    pub baseline: f32,
    pub font: FontId,
    pub size: f32,
    /// Each glyph's `text_range` is a range of `text`.
    pub glyphs: Vec<KrillaGlyph>,
    pub text: String,
    /// Drawn in the link color.
    pub link: bool,
}

/// A clickable area that opens `url`.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub url: String,
}

impl Item {
    fn moved(mut self, dx: f32, dy: f32) -> Self {
        match &mut self {
            Item::Text(text) => {
                text.x += dx;
                text.baseline += dy;
            }
            Item::Image { x, y, .. } | Item::Shade { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2 } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
        self
    }
}

/// One line of text, or one image, laid out from (0, 0) until it's placed.
#[derive(Debug, Default)]
struct LineBox {
    width: f32,
    height: f32,
    items: Vec<Item>,
    links: Vec<Link>,
}

/// A table cell laid out from (0, 0), padding included, before it's placed. It starts at
/// `row` and `col` and spans `rows` and `cols`.
#[derive(Debug)]
struct TableBox {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    content: LineBox,
}

/// Heading font sizes, as multiples of the body size, for levels 1 to 6.
const HEADING_SCALE: [f32; 6] = [2.0, 1.6, 1.3, 1.15, 1.0, 1.0];

/// Points per pixel, taking images to be 96 dpi as Office does.
const POINTS_PER_PIXEL: f32 = 0.75;

/// The most characters of a header or footer drawn on every page; a picture counts as one. A
/// real one is a line or two, and the cap keeps a long one in a hostile file from making every
/// page cost as much as the whole of it.
pub const MAX_REPEATED_CHARS: usize = 1_000;

/// Lays out blocks page by page.
pub struct Layout<'a> {
    fonts: &'a mut Fonts,
    images: &'a EmbeddedImages,
    setup: PageSetup,
    pages: Vec<Page>,
    /// Where the next block goes on the last page.
    y: f32,
    /// Decoded images by key; `None` for ones left out of the PDF.
    decoded: HashMap<String, Option<Image>>,
    skipped_images: SkippedImages,
}

/// The most pixels an image may have to go in a PDF.
///
/// krilla decodes the whole image, at up to 16 bytes per pixel while it does (a 16-bit RGBA
/// PNG), and a file of a few bytes can say it's any size. 50 megapixels holds a 48 MP phone
/// photo, and keeps the worst case at about 800 MB.
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;

/// How many images were left out of the PDF, and why. Each image is counted once, however
/// often the document shows it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SkippedImages {
    /// In a format a PDF can't hold (such as EMF or TIFF), or not a readable image.
    pub unsupported: usize,
    /// More than [`MAX_IMAGE_PIXELS`].
    pub too_large: usize,
}

/// The start of `blocks`, holding at most `budget` characters, with a picture, note reference
/// or field counting as one. A paragraph is cut short where the budget runs out; a table that
/// doesn't fit whole is left out, with everything after it.
fn cut_to(blocks: &[Block], mut budget: usize) -> Vec<Block> {
    let mut kept = Vec::new();
    for block in blocks {
        if budget == 0 {
            break;
        }
        match block {
            Block::Heading { runs, .. }
            | Block::Paragraph { runs, .. }
            | Block::ListItem { runs, .. } => {
                let cut = cut_runs(runs, &mut budget);
                let mut block = block.clone();
                if let Block::Heading { runs, .. }
                | Block::Paragraph { runs, .. }
                | Block::ListItem { runs, .. } = &mut block
                {
                    *runs = cut;
                }
                kept.push(block);
            }
            Block::Table(rows) => {
                let size: usize = rows
                    .iter()
                    .flatten()
                    .map(|cell| cell.runs().iter().map(run_size).sum::<usize>())
                    .sum();
                if size > budget {
                    break;
                }
                budget -= size;
                kept.push(block.clone());
            }
            Block::Rule => {
                budget -= 1;
                kept.push(Block::Rule);
            }
            // Readers never put these in a header or footer.
            Block::Note { .. } | Block::Header(_) | Block::Footer(_) => {}
        }
    }
    kept
}

/// The runs of `runs` that fit in `budget` characters, the last one cut short if need be, and
/// what's left of the budget.
fn cut_runs(runs: &[Run], budget: &mut usize) -> Vec<Run> {
    let mut kept = Vec::new();
    for run in runs {
        if *budget == 0 {
            break;
        }
        let size = run_size(run);
        if size <= *budget {
            *budget -= size;
            kept.push(run.clone());
            continue;
        }
        let end = run
            .text
            .char_indices()
            .nth(*budget)
            .map_or(run.text.len(), |(i, _)| i);
        kept.push(Run {
            text: run.text[..end].to_string(),
            ..run.clone()
        });
        *budget = 0;
    }
    kept
}

/// How much of [`cut_to`]'s budget a run takes.
fn run_size(run: &Run) -> usize {
    if run.image.is_some() || run.note.is_some() || run.field.is_some() {
        1
    } else {
        run.text.chars().count()
    }
}

/// True if any run in `blocks` is a page number field.
fn has_fields(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Heading { runs, .. }
        | Block::Paragraph { runs, .. }
        | Block::ListItem { runs, .. } => runs.iter().any(|r| r.field.is_some()),
        Block::Table(rows) => rows
            .iter()
            .flatten()
            .any(|cell| cell.runs().iter().any(|r| r.field.is_some())),
        _ => false,
    })
}

/// `blocks` with each page number field written out for page `page` of `count`.
fn fill_fields(blocks: &[Block], page: usize, count: usize) -> Vec<Block> {
    let fill = |runs: &mut Vec<Run>| {
        for run in runs.iter_mut() {
            if let Some(field) = run.field.take() {
                run.text = match field {
                    Field::PageNumber => page,
                    Field::PageCount => count,
                }
                .to_string();
            }
        }
    };
    let mut blocks = blocks.to_vec();
    for block in &mut blocks {
        match block {
            Block::Heading { runs, .. }
            | Block::Paragraph { runs, .. }
            | Block::ListItem { runs, .. } => fill(runs),
            Block::Table(rows) => {
                for cell in rows.iter_mut().flatten() {
                    if let TableCell::Content { runs, .. } = cell {
                        fill(runs);
                    }
                }
            }
            _ => {}
        }
    }
    blocks
}

/// Which margin [`Layout::repeat`] draws in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
}

/// Why [`decode_image`] left an image out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftOut {
    Unsupported,
    TooLarge,
}

impl<'a> Layout<'a> {
    pub fn new(fonts: &'a mut Fonts, images: &'a EmbeddedImages, setup: PageSetup) -> Self {
        Layout {
            fonts,
            images,
            setup,
            pages: vec![Page::default()],
            y: setup.margin,
            decoded: HashMap::new(),
            skipped_images: SkippedImages::default(),
        }
    }

    /// Lays out every block, returning the pages and how many images were left out.
    pub fn run(mut self, blocks: &[Block]) -> (Vec<Page>, SkippedImages) {
        self.place_blocks(blocks);
        for block in blocks {
            match block {
                Block::Header(header) => self.repeat(header, Edge::Top),
                Block::Footer(footer) => self.repeat(footer, Edge::Bottom),
                _ => {}
            }
        }
        (self.pages, self.skipped_images)
    }

    /// Draws `blocks` in the margin at `edge` of every page, as Word draws a header or footer:
    /// from halfway into the margin, and no further than a short gap from the text.
    ///
    /// The blocks are cut to [`MAX_REPEATED_CHARS`] first. Without page number fields they're
    /// laid out once and copied onto every page; with them, laid out again for each page, with
    /// its numbers filled in.
    fn repeat(&mut self, blocks: &[Block], edge: Edge) {
        let margin = self.setup.margin;
        let room = margin / 2.0 - self.setup.body_size / 2.0;
        if room <= 0.0 {
            return;
        }
        let blocks = cut_to(blocks, MAX_REPEATED_CHARS);
        let count = self.pages.len();
        let mut same_on_every_page = None;
        for index in 0..count {
            let (laid_out, height) = if has_fields(&blocks) {
                self.lay_out_apart(&fill_fields(&blocks, index + 1, count), room)
            } else {
                if same_on_every_page.is_none() {
                    same_on_every_page = Some(self.lay_out_apart(&blocks, room));
                }
                same_on_every_page.clone().expect("laid out above")
            };
            self.place_in_margin(index, laid_out, height, edge);
        }
    }

    /// Adds a header or footer laid out by [`lay_out_apart`](Self::lay_out_apart) to page
    /// `index`, in the margin at `edge`.
    fn place_in_margin(&mut self, index: usize, laid_out: Page, height: f32, edge: Edge) {
        let margin = self.setup.margin;
        let top = match edge {
            Edge::Top => margin / 2.0,
            Edge::Bottom => self.setup.height - margin / 2.0 - height,
        };
        let page = &mut self.pages[index];
        // Drawn in reading order, so a PDF reader's text starts with the header and ends with
        // the footer.
        let items = laid_out
            .items
            .into_iter()
            .map(|item| item.moved(margin, top));
        let at = match edge {
            Edge::Top => 0,
            Edge::Bottom => page.items.len(),
        };
        page.items.splice(at..at, items);
        page.links
            .extend(laid_out.links.into_iter().map(|link| Link {
                x: link.x + margin,
                y: link.y + top,
                ..link
            }));
    }

    /// Lays out `blocks` apart from the pages, in a box as wide as the text and `height` tall.
    /// Returns what fits, at positions inside the box, and the height it takes up. What doesn't
    /// fit is left out, so a header can't cover the page.
    fn lay_out_apart(&mut self, blocks: &[Block], height: f32) -> (Page, f32) {
        let setup = PageSetup {
            width: self.setup.content_width(),
            height,
            margin: 0.0,
            ..self.setup
        };
        let setup = mem::replace(&mut self.setup, setup);
        let pages = mem::replace(&mut self.pages, vec![Page::default()]);
        let y = mem::replace(&mut self.y, 0.0);

        self.place_blocks(blocks);
        // Only the first page is kept. If the blocks ran onto another, the first is full.
        let used = if self.pages.len() > 1 { height } else { self.y };
        let first = mem::take(&mut self.pages[0]);

        self.setup = setup;
        self.pages = pages;
        self.y = y;
        (first, used.min(height))
    }

    /// Lays out blocks one under another, each after a gap that depends on what came before it.
    fn place_blocks(&mut self, blocks: &[Block]) {
        let body = self.setup.body_size;
        // How many numbered items came before at each list level, for "1.", "2.", ...
        let mut numbers: Vec<u32> = Vec::new();
        let mut previous: Option<&Block> = None;

        for (i, block) in blocks.iter().enumerate() {
            // Headers and footers go in the margins, once every page is laid out.
            if block.is_page_furniture() {
                continue;
            }
            if !matches!(block, Block::ListItem { .. }) {
                numbers.clear();
            }
            let gap = match (previous, block) {
                (_, Block::Heading { level, .. }) => body * heading_scale(*level) * 0.8,
                (Some(Block::ListItem { .. }), Block::ListItem { .. }) => body * 0.25,
                _ => body * 0.7,
            };
            if previous.is_some() {
                self.add_gap(gap);
            }

            match block {
                Block::Heading { level, runs, align } => {
                    let size = body * heading_scale(*level);
                    let runs = all_bold(runs);
                    let lines = self.layout_runs(&runs, size, self.setup.content_width());
                    // Keep a heading on the same page as the first lines after it.
                    let height: f32 = lines.iter().map(|l| l.height).sum();
                    let follows = blocks.get(i + 1).is_some_and(|b| !matches!(b, Block::Rule));
                    let keep = if follows {
                        2.0 * self.line_height(body)
                    } else {
                        0.0
                    };
                    self.ensure_space(height + keep);
                    self.place_aligned(lines, *align);
                }
                Block::Paragraph { runs, align } => {
                    let lines = self.layout_runs(runs, body, self.setup.content_width());
                    self.place_aligned(lines, *align);
                }
                Block::ListItem { kind, level, runs } => {
                    let marker = list_marker(&mut numbers, *kind, *level);
                    self.place_list_item(&marker, *level, runs);
                }
                Block::Table(rows) => self.place_table(rows),
                Block::Rule => match self.setup.rule {
                    RuleStyle::PageBreak => self.new_page(),
                    RuleStyle::Line => self.draw_line(),
                },
                Block::Note { number, blocks } => {
                    // A line sets the notes apart from the document, as Word does above them.
                    if !matches!(previous, Some(Block::Note { .. })) {
                        self.draw_line();
                        self.add_gap(gap);
                    }
                    self.place_blocks(&with_note_marker(*number, blocks));
                }
                Block::Header(_) | Block::Footer(_) => {}
            }
            previous = Some(block);
        }
    }

    /// A horizontal line across the page.
    fn draw_line(&mut self) {
        self.ensure_space(1.0);
        let (x1, x2) = (self.setup.margin, self.setup.width - self.setup.margin);
        let y = self.y;
        self.page().items.push(Item::Line {
            x1,
            y1: y,
            x2,
            y2: y,
        });
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("there is always a page")
    }

    fn at_page_top(&self) -> bool {
        self.y <= self.setup.margin
    }

    fn new_page(&mut self) {
        self.pages.push(Page::default());
        self.y = self.setup.margin;
    }

    /// Starts a new page unless `height` still fits on this one, or this page is empty.
    fn ensure_space(&mut self, height: f32) {
        if self.y + height > self.setup.bottom() && !self.at_page_top() {
            self.new_page();
        }
    }

    /// Space between blocks, dropped at the top of a page.
    fn add_gap(&mut self, gap: f32) {
        if !self.at_page_top() {
            self.y = (self.y + gap).min(self.setup.bottom());
        }
    }

    fn line_height(&self, size: f32) -> f32 {
        self.fonts.line_metrics().1 * size
    }

    /// Places lines one under another at `x`, moving to a new page when one doesn't fit.
    fn place_lines(&mut self, lines: Vec<LineBox>, x: f32) {
        for line in lines {
            self.ensure_space(line.height);
            self.emit(line, x, self.y);
        }
    }

    /// Places lines across the text's width as `align` says. Justified text stays left-aligned:
    /// stretching the spaces between words isn't built (ADR 0001).
    fn place_aligned(&mut self, lines: Vec<LineBox>, align: Align) {
        for line in lines {
            let room = (self.setup.content_width() - line.width).max(0.0);
            let indent = match align {
                Align::Left | Align::Justify => 0.0,
                Align::Center => room / 2.0,
                Align::Right => room,
            };
            self.ensure_space(line.height);
            self.emit(line, self.setup.margin + indent, self.y);
        }
    }

    /// Adds a line's items and links to the current page, with its top-left corner at (x, y),
    /// and moves down past it.
    fn emit(&mut self, line: LineBox, x: f32, y: f32) {
        let page = self.page();
        page.items
            .extend(line.items.into_iter().map(|item| item.moved(x, y)));
        page.links.extend(line.links.into_iter().map(|link| Link {
            x: link.x + x,
            y: link.y + y,
            ..link
        }));
        self.y = y + line.height;
    }

    /// A list item: the marker hangs to the left of the text, indented by level.
    fn place_list_item(&mut self, marker: &str, level: u8, runs: &[Run]) {
        let body = self.setup.body_size;
        let indent = self.setup.margin + f32::from(level) * body * 1.5;
        let hang = body * 1.5;
        let width = (self.setup.content_width() - (indent - self.setup.margin) - hang).max(hang);

        let mut lines = self.layout_runs(runs, body, width);
        if lines.is_empty() {
            lines.push(LineBox {
                height: self.line_height(body),
                ..LineBox::default()
            });
        }
        let marker_runs = [Run::new(marker, RunStyle::default())];
        let marker_line = self.layout_runs(&marker_runs, body, f32::INFINITY);

        let first = lines.remove(0);
        self.ensure_space(first.height);
        let y = self.y;
        for marker in marker_line {
            self.emit(marker, indent, y);
        }
        self.emit(first, indent + hang, y);
        self.place_lines(lines, indent + hang);
    }

    /// A table with a border around every cell, drawing a merged cell once across its columns
    /// and rows. Columns get their natural width when the table fits, and otherwise share the
    /// page so that words aren't broken unless they have to be (the way browsers size tables).
    /// The header row is bold, shaded, and repeated on each page the table continues onto,
    /// unless a cell spans from it into the rows below.
    fn place_table(&mut self, rows: &[Vec<TableCell>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let body = self.setup.body_size;
        let padding = body * 0.4;

        // Every cell, with a short row's missing cells as empty ones. The readers keep spans
        // inside the table; clamping them again keeps a bad one from panicking here.
        let mut cells = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for c in 0..columns {
                let (runs, cols, tall) = match row.get(c) {
                    Some(TableCell::Content { runs, cols, rows }) => {
                        (cell_runs(runs, r == 0), *cols, *rows)
                    }
                    Some(TableCell::Covered) => continue,
                    None => (Vec::new(), 1, 1),
                };
                cells.push((
                    r,
                    c,
                    cols.clamp(1, columns - c),
                    tall.clamp(1, rows.len() - r),
                    runs,
                ));
            }
        }

        // Each column's narrowest width (its longest word) and natural width (its widest
        // line, unwrapped). One-column cells go first; then a merged cell that needs more room
        // than its columns have widens them evenly.
        let mut narrowest = vec![2.0 * padding; columns];
        let mut natural = vec![2.0 * padding; columns];
        let mut needs = Vec::with_capacity(cells.len());
        for (_, c, cols, _, runs) in &cells {
            let word = self.longest_word(runs, body);
            let lines = self.layout_runs(runs, body, f32::INFINITY);
            let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
            needs.push((*c..c + cols, word + 2.0 * padding, widest + 2.0 * padding));
        }
        needs.sort_by_key(|(span, ..)| span.len());
        for (span, word, widest) in needs {
            widen(&mut narrowest[span.clone()], word);
            widen(&mut natural[span], widest);
        }
        for (natural, narrowest) in natural.iter_mut().zip(&narrowest) {
            *natural = natural.max(*narrowest);
        }
        let widths = column_widths(&narrowest, &natural, self.setup.content_width());

        let mut boxes: Vec<TableBox> = cells
            .into_iter()
            .map(|(row, col, cols, rows, runs)| {
                let width = widths[col..col + cols].iter().sum();
                let content = self.layout_cell(&runs, width, padding);
                TableBox {
                    row,
                    col,
                    cols,
                    rows,
                    content,
                }
            })
            .collect();

        // Rows a merged cell joins go on one page. When they can't fit on any page, they're
        // drawn as separate rows instead, with the cell's text in the first, as Markdown has it.
        let min_height = self.line_height(body) + 2.0 * padding;
        let mut heights = row_heights(&boxes, rows.len(), min_height);
        // Leave room for the header, which may repeat above them.
        let room = self.setup.bottom() - self.setup.margin - heights[0];
        let mut too_tall = vec![false; rows.len()];
        for group in row_groups(&boxes, rows.len()) {
            if group.len() > 1 && heights[group.clone()].iter().sum::<f32>() > room {
                too_tall[group].fill(true);
            }
        }
        if too_tall.contains(&true) {
            let mut split = Vec::new();
            for cell in &mut boxes {
                if cell.rows > 1 && too_tall[cell.row] {
                    split.extend((cell.row + 1..cell.row + cell.rows).map(|row| TableBox {
                        row,
                        rows: 1,
                        content: LineBox::default(),
                        ..*cell
                    }));
                    cell.rows = 1;
                }
            }
            boxes.extend(split);
            boxes.sort_by_key(|cell| (cell.row, cell.col));
            heights = row_heights(&boxes, rows.len(), min_height);
        }

        let header_end = boxes.partition_point(|cell| cell.row == 0);
        let repeat_header = boxes[..header_end].iter().all(|cell| cell.rows == 1);
        let mut start = 0;
        for group in row_groups(&boxes, rows.len()) {
            let end = start + boxes[start..].partition_point(|cell| cell.row < group.end);
            self.ensure_space(heights[group.clone()].iter().sum());
            // The table needs a top border wherever it starts on a page.
            let mut top = group.start == 0 || self.at_page_top();
            if group.start > 0 && repeat_header && self.at_page_top() {
                let header = &boxes[..header_end];
                self.place_rows(header, 0..1, &heights, &widths, true, false);
                top = false;
            }
            self.place_rows(&boxes[start..end], group, &heights, &widths, top, true);
            start = end;
        }
    }

    /// Lays out a cell's runs inside its padding. The box is as tall as they are, padding
    /// included.
    fn layout_cell(&mut self, runs: &[Run], width: f32, padding: f32) -> LineBox {
        let mut cell = LineBox::default();
        let mut y = padding;
        for line in self.layout_runs(runs, self.setup.body_size, width - 2.0 * padding) {
            cell.items
                .extend(line.items.into_iter().map(|item| item.moved(padding, y)));
            cell.links.extend(line.links.into_iter().map(|link| Link {
                x: link.x + padding,
                y: link.y + y,
                ..link
            }));
            y += line.height;
        }
        cell.height = y + padding;
        cell
    }

    /// Adds table rows `rows`, holding `cells`, at the current position: the header's shading,
    /// each cell's content with its left and bottom borders, then the right border, and the
    /// top border if `top`. Links are left out of a repeated header, whose first copy has them.
    fn place_rows(
        &mut self,
        cells: &[TableBox],
        rows: Range<usize>,
        heights: &[f32],
        widths: &[f32],
        top_border: bool,
        links: bool,
    ) {
        let left = self.setup.margin;
        let top = self.y;
        let right = left + widths.iter().sum::<f32>();
        let bottom = top + heights[rows.clone()].iter().sum::<f32>();
        let page = self.pages.last_mut().expect("there is always a page");

        if rows.start == 0 {
            page.items.push(Item::Shade {
                x: left,
                y: top,
                width: right - left,
                height: heights[0],
            });
        }
        for cell in cells {
            let x = left + widths[..cell.col].iter().sum::<f32>();
            let y = top + heights[rows.start..cell.row].iter().sum::<f32>();
            let width: f32 = widths[cell.col..cell.col + cell.cols].iter().sum();
            let height: f32 = heights[cell.row..cell.row + cell.rows].iter().sum();
            page.items
                .extend(cell.content.items.iter().map(|i| i.clone().moved(x, y)));
            if links {
                page.links
                    .extend(cell.content.links.iter().map(|link| Link {
                        x: link.x + x,
                        y: link.y + y,
                        ..link.clone()
                    }));
            }
            page.items.push(Item::Line {
                x1: x,
                y1: y,
                x2: x,
                y2: y + height,
            });
            page.items.push(Item::Line {
                x1: x,
                y1: y + height,
                x2: x + width,
                y2: y + height,
            });
        }
        page.items.push(Item::Line {
            x1: right,
            y1: top,
            x2: right,
            y2: bottom,
        });
        if top_border {
            page.items.push(Item::Line {
                x1: left,
                y1: top,
                x2: right,
                y2: top,
            });
        }
        self.y = bottom;
    }

    /// Lays out runs as lines no wider than `width`. Each image gets a line of its own.
    fn layout_runs(&mut self, runs: &[Run], size: f32, width: f32) -> Vec<LineBox> {
        let mut lines = Vec::new();
        let mut after_image = false;
        for group in runs.chunk_by(|a, b| a.image.is_none() && b.image.is_none()) {
            match &group[0].image {
                Some(image) => {
                    if let Some(line) = self.layout_image(image, group[0].link.as_deref(), width) {
                        lines.push(line);
                    }
                    after_image = true;
                }
                None if after_image => {
                    // Text after an image starts a new line, so drop the space before it.
                    let mut group = group.to_vec();
                    group[0].text = group[0].text.trim_start().to_string();
                    lines.extend(self.layout_text(&group, size, width));
                }
                None => lines.extend(self.layout_text(group, size, width)),
            }
        }
        lines
    }

    /// The width of the widest word in `runs`, which can't be narrowed without breaking it.
    fn longest_word(&mut self, runs: &[Run], size: f32) -> f32 {
        runs.chunk_by(|a, b| a.image.is_none() && b.image.is_none())
            .filter(|group| group[0].image.is_none())
            .map(|group| Paragraph::shape(self.fonts, group).longest_word() * size)
            .fold(0.0, f32::max)
    }

    /// An image at the size the document shows it, or at its pixel size if it doesn't say,
    /// then scaled down, if needed, to fit `width` and the page's height.
    fn layout_image(
        &mut self,
        image_ref: &ImageRef,
        link: Option<&str>,
        width: f32,
    ) -> Option<LineBox> {
        let image = self.decode(&image_ref.source)?;
        let (pixels_wide, pixels_high) = image.size();
        let natural_width = pixels_wide as f32 * POINTS_PER_PIXEL;
        let natural_height = pixels_high as f32 * POINTS_PER_PIXEL;
        // Fit within the document's size, keeping the image's shape. A cropped picture's size
        // is the size of the part shown, and the whole image is drawn, so stretching it to
        // that size would distort it.
        let (box_width, box_height) = match image_ref.size {
            Some((cx, cy)) => (
                cx as f32 / EMU_PER_POINT as f32,
                cy as f32 / EMU_PER_POINT as f32,
            ),
            None => (natural_width, natural_height),
        };
        let max_height = self.setup.bottom() - self.setup.margin;
        let scale = (box_width / natural_width)
            .min(box_height / natural_height)
            .min(width / natural_width)
            .min(max_height / natural_height);
        let (w, h) = (natural_width * scale, natural_height * scale);
        if !(w > 0.0 && h > 0.0) {
            return None;
        }

        let mut line = LineBox {
            width: w,
            height: h,
            items: vec![Item::Image {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
                image,
            }],
            links: Vec::new(),
        };
        if let Some(url) = link {
            line.links.push(Link {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
                url: url.to_string(),
            });
        }
        Some(line)
    }

    /// The decoded image for `key`, or `None` if it's missing or left out (see [`decode_image`]).
    /// Each image is decoded once, and counted once when left out.
    fn decode(&mut self, key: &str) -> Option<Image> {
        if let Some(image) = self.decoded.get(key) {
            return image.clone();
        }
        let image = match self.images.get(key).map(decode_image) {
            Some(Ok(image)) => Some(image),
            Some(Err(LeftOut::TooLarge)) => {
                self.skipped_images.too_large += 1;
                None
            }
            None | Some(Err(LeftOut::Unsupported)) => {
                self.skipped_images.unsupported += 1;
                None
            }
        };
        self.decoded.insert(key.to_string(), image.clone());
        image
    }

    /// Wraps text runs into lines no wider than `width`, breaking where Unicode allows (after
    /// spaces and hyphens, between CJK characters) and always at `"\n"`. A word wider than a
    /// whole line is broken between characters.
    fn layout_text(&mut self, runs: &[Run], size: f32, width: f32) -> Vec<LineBox> {
        let paragraph = Paragraph::shape(self.fonts, runs);
        if paragraph.text.trim().is_empty() {
            return Vec::new();
        }
        let (baseline, line_height) = self.fonts.line_metrics();
        let line_height = line_height * size;
        let fits = |range: Range<usize>| paragraph.width(range) * size <= width;

        let mut lines: Vec<Range<usize>> = Vec::new();
        let mut start = 0;
        // End of the current line's text, without trailing spaces; `start` while empty.
        let mut end = 0;
        let mut word_start = 0;
        for (word_end, opportunity) in linebreaks(&paragraph.text) {
            let word_content = word_start + paragraph.text[word_start..word_end].trim_end().len();
            if word_content > word_start {
                if end > start && !fits(start..word_content) {
                    lines.push(start..end);
                    start = word_start;
                }
                // A word too long for a line on its own is split between characters.
                while !fits(start..word_content) {
                    let split = paragraph.longest_fit(start..word_content, width / size);
                    lines.push(start..split);
                    start = split;
                }
                end = word_content;
            }
            if opportunity == BreakOpportunity::Mandatory {
                lines.push(start..end.max(start));
                start = word_end;
                end = word_end;
            }
            word_start = word_end;
        }
        // The text always ends with a mandatory break, so `start` is at the end now.

        lines
            .into_iter()
            .map(|range| paragraph.line(range, size, baseline * size, line_height))
            .collect()
    }
}

/// Runs joined into one string and shaped, ready to be broken into lines.
struct Paragraph {
    text: String,
    /// Each glyph with the run it came from, in text order.
    glyphs: Vec<(KrillaGlyph, FontId, usize)>,
    /// The link of each run.
    links: Vec<Option<String>>,
    /// `advance_before[i]` is the total advance of glyphs `0..i`, in ems.
    advance_before: Vec<f32>,
}

impl Paragraph {
    fn shape(fonts: &mut Fonts, runs: &[Run]) -> Self {
        let mut text = String::new();
        let mut glyphs = Vec::new();
        for (r, run) in runs.iter().enumerate() {
            let run_text = match run.note {
                Some(number) => Cow::Owned(note_marker(number)),
                None => Cow::Borrowed(run.text.as_str()),
            };
            let offset = text.len();
            text.push_str(&run_text);

            // Split the run where the font changes, and shape each piece.
            let mut pieces: Vec<(FontId, Range<usize>)> = Vec::new();
            for (i, c) in run_text.char_indices() {
                let font = fonts.font_for(c, run.style);
                let at = offset + i;
                match pieces.last_mut() {
                    Some((last, range)) if *last == font => range.end = at + c.len_utf8(),
                    _ => pieces.push((font, at..at + c.len_utf8())),
                }
            }
            for (font, range) in pieces {
                glyphs.extend(
                    fonts
                        .shape(font, &text, range)
                        .into_iter()
                        .map(|g| (g, font, r)),
                );
            }
        }

        let mut advance_before = Vec::with_capacity(glyphs.len() + 1);
        let mut total = 0.0;
        advance_before.push(total);
        for (glyph, _, _) in &glyphs {
            total += glyph.x_advance;
            advance_before.push(total);
        }

        Paragraph {
            text,
            glyphs,
            links: runs.iter().map(|run| run.link.clone()).collect(),
            advance_before,
        }
    }

    /// The glyphs whose text starts inside `range`.
    fn glyph_span(&self, range: Range<usize>) -> Range<usize> {
        let first = self
            .glyphs
            .partition_point(|(g, _, _)| g.text_range.start < range.start);
        let last = self
            .glyphs
            .partition_point(|(g, _, _)| g.text_range.start < range.end);
        first..last
    }

    /// How wide `range` of the text is, in ems.
    fn width(&self, range: Range<usize>) -> f32 {
        let span = self.glyph_span(range);
        self.advance_before[span.end] - self.advance_before[span.start]
    }

    /// The width of the widest word, without trailing spaces, in ems.
    fn longest_word(&self) -> f32 {
        let mut start = 0;
        let mut widest = 0.0_f32;
        for (end, _) in linebreaks(&self.text) {
            let content = start + self.text[start..end].trim_end().len();
            widest = widest.max(self.width(start..content));
            start = end;
        }
        widest
    }

    /// The furthest point in `range` that fits in `width` ems: always past at least one
    /// character, so a line can't be empty.
    fn longest_fit(&self, range: Range<usize>, width: f32) -> usize {
        let span = self.glyph_span(range.clone());
        let start_advance = self.advance_before[span.start];
        let mut fit = None;
        for i in span.clone() {
            let (glyph, _, _) = &self.glyphs[i];
            // Only split where a new character starts, never inside one.
            if i > span.start && glyph.text_range.start != self.glyphs[i - 1].0.text_range.start {
                if self.advance_before[i] - start_advance > width {
                    break;
                }
                fit = Some(glyph.text_range.start);
            }
        }
        fit.unwrap_or_else(|| {
            let first = &self.text[range.start..range.end];
            range.start + first.chars().next().map_or(0, char::len_utf8)
        })
    }

    /// One line of the paragraph, as text items grouped by font and run.
    fn line(&self, range: Range<usize>, size: f32, baseline: f32, height: f32) -> LineBox {
        let span = self.glyph_span(range);
        let mut line = LineBox {
            height,
            ..LineBox::default()
        };
        let mut x = 0.0;
        let glyphs = &self.glyphs[span];
        for group in glyphs.chunk_by(|a, b| a.1 == b.1 && a.2 == b.2) {
            let (_, font, run) = group[0];
            let start = group
                .iter()
                .map(|(g, _, _)| g.text_range.start)
                .min()
                .unwrap_or(0);
            let end = group
                .iter()
                .map(|(g, _, _)| g.text_range.end)
                .max()
                .unwrap_or(start);
            let rebased: Vec<KrillaGlyph> = group
                .iter()
                .map(|(g, _, _)| {
                    let mut g = g.clone();
                    g.text_range = g.text_range.start - start..g.text_range.end - start;
                    g
                })
                .collect();
            let advance: f32 = rebased.iter().map(|g| g.x_advance).sum::<f32>() * size;

            let link = &self.links[run];
            if let Some(url) = link {
                line.links.push(Link {
                    x,
                    y: 0.0,
                    width: advance,
                    height,
                    url: url.clone(),
                });
            }
            line.items.push(Item::Text(TextItem {
                x,
                baseline,
                font,
                size,
                glyphs: rebased,
                text: self.text[start..end].to_string(),
                link: link.is_some(),
            }));
            x += advance;
        }
        line.width = x;
        line
    }
}

/// Column widths that fit `available`: the natural widths if they fit, else each column's
/// narrowest width plus a share of the space left in proportion to how much more it would
/// like, else (when even the narrowest widths don't fit) the narrowest widths scaled down.
fn column_widths(narrowest: &[f32], natural: &[f32], available: f32) -> Vec<f32> {
    let total_natural: f32 = natural.iter().sum();
    let total_narrowest: f32 = narrowest.iter().sum();
    if total_natural <= available {
        return natural.to_vec();
    }
    if total_narrowest >= available {
        let scale = available / total_narrowest;
        return narrowest.iter().map(|w| w * scale).collect();
    }
    let share = (available - total_narrowest) / (total_natural - total_narrowest);
    narrowest
        .iter()
        .zip(natural)
        .map(|(min, max)| min + (max - min) * share)
        .collect()
}

/// Widens `widths` evenly until together they're at least `needed`.
fn widen(widths: &mut [f32], needed: f32) {
    let short = needed - widths.iter().sum::<f32>();
    if short > 0.0 {
        let share = short / widths.len() as f32;
        widths.iter_mut().for_each(|w| *w += share);
    }
}

/// Each table row's height: at least `min_height`, and tall enough for every cell that
/// starts in it. A merged cell taller than its rows adds what it still needs to the last.
fn row_heights(cells: &[TableBox], rows: usize, min_height: f32) -> Vec<f32> {
    let mut heights = vec![min_height; rows];
    let mut order: Vec<&TableBox> = cells.iter().collect();
    order.sort_by_key(|cell| (cell.rows > 1, cell.row + cell.rows));
    for cell in order {
        let span = &mut heights[cell.row..cell.row + cell.rows];
        let short = cell.content.height - span.iter().sum::<f32>();
        if short > 0.0
            && let Some(last) = span.last_mut()
        {
            *last += short;
        }
    }
    heights
}

/// The runs of rows that merged cells join, which have to go on one page together. A row
/// no merged cell joins to another is a run of its own.
fn row_groups(cells: &[TableBox], rows: usize) -> Vec<Range<usize>> {
    let mut reach: Vec<usize> = (1..=rows).collect();
    for cell in cells {
        reach[cell.row] = reach[cell.row].max(cell.row + cell.rows);
    }
    let mut groups = Vec::new();
    let (mut start, mut end) = (0, 0);
    for (row, reach) in reach.into_iter().enumerate() {
        end = end.max(reach);
        if row + 1 == end {
            groups.push(start..end);
            start = end;
        }
    }
    groups
}

/// A cell's runs, made bold in the header row.
fn cell_runs(cell: &CellRuns, header: bool) -> Vec<Run> {
    if header { all_bold(cell) } else { cell.clone() }
}

/// How a note reference reads in the text, and the marker before the note: `[1]`. The layout
/// draws every run on one baseline, so the number isn't raised as Word raises it.
fn note_marker(number: usize) -> String {
    format!("[{number}]")
}

/// A note's blocks, with its marker at the start of the first: `[1] The note.`
fn with_note_marker(number: usize, blocks: &[Block]) -> Vec<Block> {
    let marker = Run::new(format!("{} ", note_marker(number)), RunStyle::default());
    let mut blocks = blocks.to_vec();
    match blocks.first_mut() {
        Some(
            Block::Heading { runs, .. }
            | Block::Paragraph { runs, .. }
            | Block::ListItem { runs, .. },
        ) => {
            // Word puts a space between its own marker and the note's text.
            if let Some(first) = runs.first_mut() {
                first.text = first.text.trim_start().to_string();
            }
            runs.insert(0, marker);
        }
        _ => blocks.insert(0, Block::paragraph(vec![marker])),
    }
    blocks
}

fn heading_scale(level: u8) -> f32 {
    HEADING_SCALE[usize::from(level.clamp(1, 6)) - 1]
}

/// The same runs, all bold.
fn all_bold(runs: &[Run]) -> Vec<Run> {
    runs.iter()
        .map(|run| Run {
            style: RunStyle {
                bold: true,
                ..run.style
            },
            ..run.clone()
        })
        .collect()
}

/// The marker for a list item: a bullet, or the item's number at its level. `numbers` holds
/// the count so far at each level; going back out to a level forgets the deeper counts.
fn list_marker(numbers: &mut Vec<u32>, kind: ListKind, level: u8) -> String {
    let level = usize::from(level);
    numbers.resize(level + 1, 0);
    match kind {
        ListKind::Bullet => if level % 2 == 0 { "•" } else { "–" }.to_string(),
        ListKind::Numbered => {
            numbers[level] += 1;
            format!("{}.", numbers[level])
        }
    }
}

/// Decodes an image by its first bytes, unless its format can't go in a PDF or it has more
/// than [`MAX_IMAGE_PIXELS`].
fn decode_image(bytes: &[u8]) -> Result<Image, LeftOut> {
    let decode: fn(Data, bool) -> Result<Image, String> =
        match ImageFormat::detect(bytes).ok_or(LeftOut::Unsupported)? {
            ImageFormat::Png => Image::from_png,
            ImageFormat::Jpeg => Image::from_jpeg,
            ImageFormat::Gif => Image::from_gif,
            ImageFormat::Webp => Image::from_webp,
            ImageFormat::Bmp | ImageFormat::Tiff | ImageFormat::Emf | ImageFormat::Wmf => {
                return Err(LeftOut::Unsupported);
            }
        };

    // krilla decodes the whole image as soon as it's created, so read the size from the header
    // first.
    let size = imagesize::blob_size(bytes).map_err(|_| LeftOut::Unsupported)?;
    if size.width as u64 * size.height as u64 > MAX_IMAGE_PIXELS {
        return Err(LeftOut::TooLarge);
    }
    decode(bytes.to_vec().into(), true).map_err(|_| LeftOut::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Vec<Run> {
        vec![Run::new(text, RunStyle::default())]
    }

    fn lay_out(blocks: &[Block], setup: PageSetup) -> Vec<Page> {
        let mut fonts = Fonts::new();
        let images = EmbeddedImages::default();
        Layout::new(&mut fonts, &images, setup).run(blocks).0
    }

    /// Each page's text, one string per line (items sharing a baseline are joined).
    fn page_lines(page: &Page) -> Vec<String> {
        let mut lines: Vec<(f32, String)> = Vec::new();
        for item in &page.items {
            if let Item::Text(t) = item {
                match lines.iter_mut().find(|(y, _)| *y == t.baseline) {
                    Some((_, line)) => line.push_str(&t.text),
                    None => lines.push((t.baseline, t.text.clone())),
                }
            }
        }
        lines.into_iter().map(|(_, line)| line).collect()
    }

    /// The right edge of each text item.
    fn right_edges(page: &Page) -> Vec<f32> {
        page.items
            .iter()
            .filter_map(|item| match item {
                Item::Text(t) => {
                    Some(t.x + t.glyphs.iter().map(|g| g.x_advance).sum::<f32>() * t.size)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn wraps_paragraphs_within_the_margins() {
        let setup = PageSetup::DOCUMENT;
        let long = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let pages = lay_out(&[Block::paragraph(text(&long))], setup);

        let lines = page_lines(&pages[0]);
        assert!(lines.len() > 5, "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| !l.starts_with(' ') && !l.ends_with(' '))
        );
        assert_eq!(lines.join(" "), long.trim_end());
        let right = setup.width - setup.margin;
        assert!(right_edges(&pages[0]).iter().all(|&x| x <= right + 0.01));
    }

    #[test]
    fn breaks_words_too_long_for_a_line() {
        let setup = PageSetup::DOCUMENT;
        let word = "x".repeat(400);
        let pages = lay_out(&[Block::paragraph(text(&word))], setup);

        let lines = page_lines(&pages[0]);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), word);
        let right = setup.width - setup.margin;
        assert!(right_edges(&pages[0]).iter().all(|&x| x <= right + 0.01));
    }

    #[test]
    fn keeps_line_breaks() {
        let pages = lay_out(&[Block::paragraph(text("one\ntwo"))], PageSetup::DOCUMENT);
        assert_eq!(page_lines(&pages[0]), ["one", "two"]);
    }

    #[test]
    fn rules_start_a_new_page_for_slides_and_draw_a_line_in_documents() {
        let blocks = [
            Block::paragraph(text("Slide one")),
            Block::Rule,
            Block::paragraph(text("Slide two")),
        ];

        let slides = lay_out(&blocks, PageSetup::SLIDES);
        assert_eq!(slides.len(), 2);
        assert_eq!(page_lines(&slides[1]), ["Slide two"]);

        let document = lay_out(&blocks, PageSetup::DOCUMENT);
        assert_eq!(document.len(), 1);
        assert!(
            document[0]
                .items
                .iter()
                .any(|i| matches!(i, Item::Line { .. }))
        );
    }

    #[test]
    fn continues_onto_new_pages_when_full() {
        let blocks: Vec<Block> = (0..200)
            .map(|i| Block::paragraph(text(&format!("Paragraph {i}"))))
            .collect();
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);

        assert!(pages.len() > 1);
        let all: Vec<String> = pages.iter().flat_map(page_lines).collect();
        assert_eq!(all.len(), 200);
        assert_eq!(all[199], "Paragraph 199");
    }

    #[test]
    fn numbers_lists_per_level() {
        let item = |kind, level, label: &str| Block::ListItem {
            kind,
            level,
            runs: text(label),
        };
        let blocks = [
            item(ListKind::Numbered, 0, "a"),
            item(ListKind::Numbered, 1, "a.a"),
            item(ListKind::Numbered, 1, "a.b"),
            item(ListKind::Numbered, 0, "b"),
            item(ListKind::Numbered, 1, "b.a"),
            item(ListKind::Bullet, 2, "deep"),
            Block::paragraph(text("between")),
            item(ListKind::Numbered, 0, "new list"),
        ];
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);
        assert_eq!(
            page_lines(&pages[0]),
            [
                "1.a",
                "1.a.a",
                "2.a.b",
                "2.b",
                "1.b.a",
                "•deep",
                "between",
                "1.new list"
            ]
        );
    }

    #[test]
    fn repeats_the_table_header_on_each_page() {
        let cell = |t: &str| TableCell::new(text(t));
        let mut rows = vec![vec![cell("Name"), cell("Value")]];
        rows.extend((0..80).map(|i| vec![cell(&format!("row {i}")), cell("x")]));
        let pages = lay_out(&[Block::Table(rows)], PageSetup::DOCUMENT);

        assert!(pages.len() > 1);
        for page in &pages {
            assert_eq!(page_lines(page)[0], "NameValue");
            assert!(page.items.iter().any(|i| matches!(i, Item::Shade { .. })));
        }
    }

    fn merged(t: &str, cols: usize, rows: usize) -> TableCell {
        TableCell::Content {
            runs: text(t),
            cols,
            rows,
        }
    }

    /// Where the text item reading `t` starts, and its baseline.
    fn text_at(page: &Page, t: &str) -> (f32, f32) {
        page.items
            .iter()
            .find_map(|item| match item {
                Item::Text(item) if item.text == t => Some((item.x, item.baseline)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {t:?} on the page"))
    }

    /// The table borders crossing the point (x, y): (vertical, horizontal).
    fn borders_through(page: &Page, x: f32, y: f32) -> (usize, usize) {
        let (mut vertical, mut horizontal) = (0, 0);
        for item in &page.items {
            if let Item::Line { x1, y1, x2, y2 } = *item {
                if x1 == x2 && (x1 - x).abs() < 0.01 && y1 < y && y < y2 {
                    vertical += 1;
                }
                if y1 == y2 && (y1 - y).abs() < 0.01 && x1 < x && x < x2 {
                    horizontal += 1;
                }
            }
        }
        (vertical, horizontal)
    }

    #[test]
    fn draws_a_merged_cell_once_across_its_span() {
        let rows = vec![
            vec![TableCell::new(text("A")), TableCell::new(text("B"))],
            vec![merged("Tall", 1, 2), TableCell::new(text("x"))],
            vec![TableCell::Covered, TableCell::new(text("y"))],
            vec![merged("Wide", 2, 1), TableCell::Covered],
        ];
        let pages = lay_out(&[Block::Table(rows)], PageSetup::DOCUMENT);
        let page = &pages[0];
        let padding = PageSetup::DOCUMENT.body_size * 0.4;

        let (x, x_baseline) = text_at(page, "x");
        let (_, y_baseline) = text_at(page, "y");
        let (wide_x, wide_baseline) = text_at(page, "Wide");
        let between_columns = x - padding;
        let between_rows = (x_baseline + y_baseline) / 2.0;
        let line = y_baseline - x_baseline;
        // Find the border between "x" and "y" near halfway between their baselines.
        let border = page
            .items
            .iter()
            .find_map(|item| match *item {
                Item::Line { x1, y1, y2, .. }
                    if y1 == y2
                        && (y1 - between_rows).abs() < line / 2.0
                        && x1 >= between_columns - 0.01 =>
                {
                    Some(y1)
                }
                _ => None,
            })
            .expect("a border between x and y");

        // "Tall" has no border across it, though "x" and "y" beside it do.
        assert_eq!(borders_through(page, wide_x + 1.0, border), (0, 0));
        assert_eq!(borders_through(page, x + 1.0, border), (0, 1));
        // "Wide" has no border down its middle, though the rows above it do.
        assert_eq!(
            borders_through(page, between_columns, wide_baseline),
            (0, 0)
        );
        assert_eq!(borders_through(page, between_columns, x_baseline), (1, 0));
    }

    #[test]
    fn keeps_the_rows_of_a_merged_cell_on_one_page() {
        let mut rows = vec![vec![
            TableCell::new(text("Name")),
            TableCell::new(text("Value")),
        ]];
        for i in 0..40 {
            rows.push(vec![
                merged(&format!("g{i}"), 1, 3),
                TableCell::new(text(&format!("a{i}"))),
            ]);
            for part in ["b", "c"] {
                rows.push(vec![
                    TableCell::Covered,
                    TableCell::new(text(&format!("{part}{i}"))),
                ]);
            }
        }
        let pages = lay_out(&[Block::Table(rows)], PageSetup::DOCUMENT);

        assert!(pages.len() > 1);
        for page in &pages {
            let lines = page_lines(page);
            assert_eq!(lines[0], "NameValue");
            for line in lines.iter().filter_map(|l| l.strip_prefix('g')) {
                let i = line.split('a').next().unwrap();
                assert!(lines.iter().any(|l| *l == format!("c{i}")), "{lines:?}");
            }
        }
    }

    #[test]
    fn repeats_no_header_that_a_cell_spans_down_from() {
        let mut rows = vec![
            vec![merged("Name", 1, 2), TableCell::new(text("Value"))],
            vec![TableCell::Covered, TableCell::new(text("Unit"))],
        ];
        rows.extend((0..80).map(|i| {
            vec![
                TableCell::new(text(&format!("row {i}"))),
                TableCell::new(text("x")),
            ]
        }));
        let pages = lay_out(&[Block::Table(rows)], PageSetup::DOCUMENT);

        assert!(pages.len() > 1);
        assert!(page_lines(&pages[1])[0].starts_with("row "));
    }

    /// A merged cell too tall for any page can't move to the next one, so its rows are drawn
    /// apart instead, its text in the first, rather than run off the page.
    #[test]
    fn splits_a_merged_cell_too_tall_for_a_page() {
        let setup = PageSetup::DOCUMENT;
        let mut rows = vec![
            vec![TableCell::new(text("Name")), TableCell::new(text("Value"))],
            vec![merged("Tall", 1, 100), TableCell::new(text("row 0"))],
        ];
        rows.extend((1..100).map(|i| {
            vec![
                TableCell::Covered,
                TableCell::new(text(&format!("row {i}"))),
            ]
        }));
        let pages = lay_out(&[Block::Table(rows)], setup);

        assert!(pages.len() > 1);
        let mut seen = Vec::new();
        for page in &pages {
            for item in &page.items {
                match item {
                    Item::Text(t) => {
                        assert!(t.baseline < setup.bottom());
                        seen.push(t.text.clone());
                    }
                    Item::Line { y2, .. } => assert!(*y2 <= setup.bottom() + 0.01),
                    _ => {}
                }
            }
        }
        assert_eq!(seen.iter().filter(|t| *t == "Tall").count(), 1);
        assert_eq!(seen.iter().filter(|t| t.starts_with("row")).count(), 100);
    }

    #[test]
    fn a_merged_cell_widens_and_heightens_what_it_spans() {
        let mut widths = [10.0, 20.0];
        widen(&mut widths, 50.0);
        assert_eq!(widths, [20.0, 30.0]);
        widen(&mut widths, 40.0);
        assert_eq!(widths, [20.0, 30.0]);

        let cell = |row, rows, height| TableBox {
            row,
            col: 0,
            rows,
            cols: 1,
            content: LineBox {
                height,
                ..LineBox::default()
            },
        };
        let cells = [cell(0, 3, 50.0), cell(1, 1, 20.0), cell(3, 1, 5.0)];
        assert_eq!(row_heights(&cells, 4, 10.0), [10.0, 20.0, 20.0, 10.0]);
        assert_eq!(row_groups(&cells, 4), [0..3, 3..4]);
    }

    #[test]
    fn writes_notes_after_a_line_with_their_numbers() {
        let note = |number, text: &str| Block::Note {
            number,
            blocks: vec![Block::paragraph(vec![Run::new(text, RunStyle::default())])],
        };
        let blocks = [
            Block::paragraph(vec![
                Run::new("Claim", RunStyle::default()),
                Run::note(1),
                Run::new(" and more", RunStyle::default()),
                Run::note(2),
            ]),
            note(1, " Source."),
            note(2, "Another."),
        ];
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);

        assert_eq!(
            page_lines(&pages[0]),
            ["Claim[1] and more[2]", "[1] Source.", "[2] Another."]
        );
        let lines: Vec<f32> = pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Line { y1, .. } => Some(*y1),
                _ => None,
            })
            .collect();
        // One line, between the text and the first note.
        let baseline = |t: &str| {
            pages[0].items.iter().find_map(|item| match item {
                Item::Text(item) if item.text.contains(t) => Some(item.baseline),
                _ => None,
            })
        };
        let (claim, first) = (baseline("Claim").unwrap(), baseline("Source").unwrap());
        assert!(
            matches!(lines.as_slice(), [y] if claim < *y && *y < first),
            "{lines:?}"
        );
    }

    /// The baseline of each text item reading `t` on `page`.
    fn baselines_of(page: &Page, t: &str) -> Vec<f32> {
        page.items
            .iter()
            .filter_map(|item| match item {
                Item::Text(item) if item.text == t => Some(item.baseline),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn repeats_headers_and_footers_in_the_margins_of_every_page() {
        let setup = PageSetup::DOCUMENT;
        let mut blocks = vec![
            Block::Header(vec![Block::paragraph(text("Report"))]),
            Block::Footer(vec![Block::paragraph(text("Confidential"))]),
        ];
        blocks.extend((0..80).map(|i| Block::paragraph(text(&format!("Paragraph {i}")))));
        let pages = lay_out(&blocks, setup);

        assert!(pages.len() > 1);
        for page in &pages {
            let [header] = baselines_of(page, "Report")[..] else {
                panic!("not one header: {:?}", page_lines(page));
            };
            let [footer] = baselines_of(page, "Confidential")[..] else {
                panic!("not one footer: {:?}", page_lines(page));
            };
            assert!(setup.margin / 2.0 < header && header < setup.margin);
            assert!(setup.bottom() < footer && footer < setup.height - setup.margin / 2.0);
        }
        // The body starts where it would without them.
        let alone = lay_out(&blocks[2..], setup);
        assert_eq!(
            text_at(&pages[0], "Paragraph 0"),
            text_at(&alone[0], "Paragraph 0")
        );
    }

    #[test]
    fn fills_in_page_numbers_on_each_page() {
        let footer = vec![
            Run::new("Page ", RunStyle::default()),
            Run::field(Field::PageNumber, RunStyle::default()),
            Run::new(" of ", RunStyle::default()),
            Run::field(Field::PageCount, RunStyle::default()),
        ];
        let mut blocks = vec![Block::Footer(vec![Block::paragraph(footer)])];
        blocks.extend((0..120).map(|i| Block::paragraph(text(&format!("Paragraph {i}")))));
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);

        let count = pages.len();
        assert!(count > 2);
        for (i, page) in pages.iter().enumerate() {
            let lines = page_lines(page);
            assert_eq!(
                lines.last().unwrap(),
                &format!("Page {} of {count}", i + 1),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn repeats_only_the_start_of_a_long_header() {
        // Zero-width spaces take no room, so all of them fit on one line of the margin.
        let long = "\u{200b}".repeat(100_000);
        let blocks = [
            Block::Header(vec![
                Block::paragraph(text(&long)),
                Block::paragraph(text("Never reached")),
            ]),
            Block::paragraph(text("Body")),
        ];
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);

        let glyphs: usize = pages[0]
            .items
            .iter()
            .map(|item| match item {
                Item::Text(t) => t.glyphs.len(),
                _ => 0,
            })
            .sum();
        assert!(
            glyphs <= MAX_REPEATED_CHARS + "Body".len(),
            "{glyphs} glyphs"
        );
        assert!(!page_lines(&pages[0]).iter().any(|l| l.contains("Never")));
    }

    #[test]
    fn cuts_runs_and_tables_to_the_budget() {
        let cell = |t: &str| TableCell::new(text(t));
        let blocks = [
            Block::paragraph(text("abcdef")),
            Block::Table(vec![vec![cell("12345")]]),
            Block::paragraph(text("after")),
        ];
        assert_eq!(cut_to(&blocks, 4), [Block::paragraph(text("abcd"))]);
        // The table doesn't fit whole, so it and what follows are left out.
        assert_eq!(cut_to(&blocks, 10), [Block::paragraph(text("abcdef"))]);
        assert_eq!(cut_to(&blocks, 13).len(), 3);
        // Characters, not bytes.
        assert_eq!(
            cut_to(&[Block::paragraph(text("été"))], 2),
            [Block::paragraph(text("ét"))]
        );
    }

    #[test]
    fn cuts_a_header_to_what_fits_in_the_margin() {
        let lines: Vec<Block> = (0..50)
            .map(|i| Block::paragraph(text(&format!("Line {i}"))))
            .collect();
        let blocks = [Block::Header(lines), Block::paragraph(text("Body"))];
        let pages = lay_out(&blocks, PageSetup::DOCUMENT);

        assert_eq!(pages.len(), 1);
        let shown = page_lines(&pages[0]);
        assert!(shown.contains(&"Line 0".to_string()), "{shown:?}");
        assert!(!shown.contains(&"Line 2".to_string()), "{shown:?}");
        let (_, body) = text_at(&pages[0], "Body");
        for item in &pages[0].items {
            if let Item::Text(t) = item
                && t.text.starts_with("Line")
            {
                assert!(t.baseline < body, "{} overlaps the body", t.text);
            }
        }
    }

    #[test]
    fn aligns_lines_within_the_margins() {
        let setup = PageSetup::DOCUMENT;
        let paragraph = |t: &str, align| Block::Paragraph {
            runs: text(t),
            align,
        };
        let blocks = [
            paragraph("Left", Align::Left),
            paragraph("Centered", Align::Center),
            paragraph("Right", Align::Right),
            paragraph("Justified", Align::Justify),
            Block::Heading {
                level: 1,
                runs: text("Heading"),
                align: Align::Center,
            },
        ];
        let pages = lay_out(&blocks, setup);
        let page = &pages[0];

        let edges = |t: &str| {
            let item = page
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Text(item) if item.text == t => Some(item),
                    _ => None,
                })
                .unwrap();
            let width: f32 = item.glyphs.iter().map(|g| g.x_advance).sum::<f32>() * item.size;
            (item.x, item.x + width)
        };
        let (left, right) = (setup.margin, setup.width - setup.margin);
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;

        assert!(close(edges("Left").0, left));
        assert!(close(edges("Justified").0, left));
        assert!(close(edges("Right").1, right));
        for t in ["Centered", "Heading"] {
            let (start, end) = edges(t);
            assert!(close(start - left, right - end), "{t}: {start}..{end}");
        }
    }

    #[test]
    fn links_cover_their_text() {
        let runs = vec![
            Run::new("See ", RunStyle::default()),
            Run::new("the docs", RunStyle::default()).linked("https://example.com"),
        ];
        let pages = lay_out(&[Block::paragraph(runs)], PageSetup::DOCUMENT);

        let [link] = pages[0].links.as_slice() else {
            panic!("expected one link: {:?}", pages[0].links);
        };
        assert_eq!(link.url, "https://example.com");
        assert!(link.x > PageSetup::DOCUMENT.margin && link.width > 0.0);
    }

    #[test]
    fn sizes_columns_like_a_browser() {
        // Everything fits: natural widths.
        assert_eq!(
            column_widths(&[10.0, 10.0], &[30.0, 50.0], 100.0),
            [30.0, 50.0]
        );
        // Too wide: each column keeps its narrowest width and shares what's left.
        assert_eq!(
            column_widths(&[20.0, 20.0], &[20.0, 220.0], 100.0),
            [20.0, 80.0]
        );
        // Even the narrowest widths don't fit: scale them down.
        assert_eq!(
            column_widths(&[100.0, 100.0], &[200.0, 200.0], 100.0),
            [50.0, 50.0]
        );
    }

    #[test]
    fn skips_images_a_pdf_cannot_hold() {
        assert_eq!(
            decode_image(b"not an image").err(),
            Some(LeftOut::Unsupported)
        );
        assert_eq!(
            decode_image(b"\x89PNG but not really").err(),
            Some(LeftOut::Unsupported)
        );
    }

    /// The start of a PNG, up to the end of its header, saying it's `width` x `height` pixels.
    /// The pixel data is missing, so decoding it would fail.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(width.to_be_bytes());
        png.extend(height.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]); // 8-bit RGBA, then a (wrong) checksum
        png
    }

    #[test]
    fn skips_images_with_too_many_pixels_before_decoding_them() {
        // Neither is decoded: a decode would allocate 40 GB, or fail on the missing pixel data.
        assert_eq!(
            decode_image(&png_header(100_000, 100_000)).err(),
            Some(LeftOut::TooLarge)
        );
        assert_eq!(
            decode_image(&png_header(50_001, 1_000)).err(),
            Some(LeftOut::TooLarge)
        );
        // At the limit, it gets as far as decoding, which fails here on the missing data.
        assert_eq!(
            decode_image(&png_header(50_000, 1_000)).err(),
            Some(LeftOut::Unsupported)
        );
    }
}
