//! Laying out [`Block`]s on pages: wrapping text into lines, numbering lists, sizing tables
//! and images, and starting a new page when one is full.
//!
//! The result is a list of [`Page`]s holding positioned [`Item`]s, which `pdf::render` then
//! paints. Keeping the two apart means the layout can be tested without reading a PDF back.
//!
//! Coordinates are in points (1/72 inch), measured from the top-left corner of the page.

use std::collections::HashMap;
use std::ops::Range;

use krilla::Data;
use krilla::image::Image;
use krilla::text::KrillaGlyph;
use unicode_linebreak::{BreakOpportunity, linebreaks};

use super::fonts::{FontId, Fonts};
use crate::document::{Block, CellRuns, ListKind, Run, RunStyle};
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
#[derive(Debug, Default)]
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

/// Heading font sizes, as multiples of the body size, for levels 1 to 6.
const HEADING_SCALE: [f32; 6] = [2.0, 1.6, 1.3, 1.15, 1.0, 1.0];

/// Points per pixel, taking images to be 96 dpi as Office does.
const POINTS_PER_PIXEL: f32 = 0.75;

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
        let body = self.setup.body_size;
        // How many numbered items came before at each list level, for "1.", "2.", ...
        let mut numbers: Vec<u32> = Vec::new();
        let mut previous: Option<&Block> = None;

        for (i, block) in blocks.iter().enumerate() {
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
                Block::Heading { level, runs } => {
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
                    self.place_lines(lines, self.setup.margin);
                }
                Block::Paragraph(runs) => {
                    let lines = self.layout_runs(runs, body, self.setup.content_width());
                    self.place_lines(lines, self.setup.margin);
                }
                Block::ListItem { kind, level, runs } => {
                    let marker = list_marker(&mut numbers, *kind, *level);
                    self.place_list_item(&marker, *level, runs);
                }
                Block::Table(rows) => self.place_table(rows),
                Block::Rule => match self.setup.rule {
                    RuleStyle::PageBreak => self.new_page(),
                    RuleStyle::Line => {
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
                },
            }
            previous = Some(block);
        }
        (self.pages, self.skipped_images)
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

    /// A table with a border around every cell. Columns get their natural width when the table
    /// fits, and otherwise share the page so that words aren't broken unless they have to be
    /// (the way browsers size tables). The header row is bold, shaded, and repeated on each
    /// page the table continues onto.
    fn place_table(&mut self, rows: &[Vec<CellRuns>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let body = self.setup.body_size;
        let padding = body * 0.4;

        // Each column's narrowest width (its longest word) and natural width (its widest
        // line, unwrapped).
        let mut narrowest = vec![2.0 * padding; columns];
        let mut natural = vec![2.0 * padding; columns];
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let runs = cell_runs(cell, r == 0);
                let word = self.longest_word(&runs, body);
                narrowest[c] = narrowest[c].max(word + 2.0 * padding);
                let lines = self.layout_runs(&runs, body, f32::INFINITY);
                let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
                natural[c] = natural[c].max(widest + 2.0 * padding);
            }
        }
        let widths = column_widths(&narrowest, &natural, self.setup.content_width());

        let laid_out: Vec<(f32, Vec<LineBox>)> = rows
            .iter()
            .enumerate()
            .map(|(r, row)| self.layout_row(row, r == 0, &widths, padding))
            .collect();

        let mut header: Option<(f32, Vec<Vec<Item>>)> = None;
        for (r, (height, cells)) in laid_out.into_iter().enumerate() {
            self.ensure_space(height);
            if r > 0
                && self.at_page_top()
                && let Some((header_height, items)) = &header
            {
                self.place_row(*header_height, items.clone(), Vec::new(), &widths, true);
            }
            let (items, links): (Vec<_>, Vec<_>) =
                cells.into_iter().map(|c| (c.items, c.links)).unzip();
            if r == 0 {
                header = Some((height, items.clone()));
            }
            self.place_row(height, items, links, &widths, r == 0);
        }
    }

    /// Lays out one row's cells; returns the row's height and each cell's content, positioned
    /// inside its cell.
    fn layout_row(
        &mut self,
        row: &[CellRuns],
        header: bool,
        widths: &[f32],
        padding: f32,
    ) -> (f32, Vec<LineBox>) {
        let min_height = self.line_height(self.setup.body_size) + 2.0 * padding;
        let mut height = min_height;
        let mut cells = Vec::with_capacity(widths.len());
        for (c, width) in widths.iter().enumerate() {
            let lines = match row.get(c) {
                Some(cell) => self.layout_cell(cell, header, width - 2.0 * padding),
                None => Vec::new(),
            };
            let mut cell = LineBox::default();
            let mut y = padding;
            for line in lines {
                cell.items
                    .extend(line.items.into_iter().map(|item| item.moved(padding, y)));
                cell.links.extend(line.links.into_iter().map(|link| Link {
                    x: link.x + padding,
                    y: link.y + y,
                    ..link
                }));
                y += line.height;
            }
            height = height.max(y + padding);
            cells.push(cell);
        }
        (height, cells)
    }

    /// Adds a row at the current position: its shading (for the header), borders, and cells.
    fn place_row(
        &mut self,
        height: f32,
        cells: Vec<Vec<Item>>,
        links: Vec<Vec<Link>>,
        widths: &[f32],
        header: bool,
    ) {
        let left = self.setup.margin;
        let top = self.y;
        let right = left + widths.iter().sum::<f32>();
        let bottom = top + height;
        let page = self.pages.last_mut().expect("there is always a page");

        if header {
            page.items.push(Item::Shade {
                x: left,
                y: top,
                width: right - left,
                height,
            });
        }
        let mut x = left;
        for (c, items) in cells.into_iter().enumerate() {
            page.items
                .extend(items.into_iter().map(|i| i.moved(x, top)));
            if let Some(links) = links.get(c) {
                page.links.extend(links.iter().map(|link| Link {
                    x: link.x + x,
                    y: link.y + top,
                    ..link.clone()
                }));
            }
            x += widths[c];
        }

        for y in [top, bottom] {
            page.items.push(Item::Line {
                x1: left,
                y1: y,
                x2: right,
                y2: y,
            });
        }
        let mut x = left;
        for edge in std::iter::once(0.0).chain(widths.iter().copied()) {
            x += edge;
            page.items.push(Item::Line {
                x1: x,
                y1: top,
                x2: x,
                y2: bottom,
            });
        }
        self.y = bottom;
    }

    fn layout_cell(&mut self, cell: &CellRuns, header: bool, width: f32) -> Vec<LineBox> {
        let runs = cell_runs(cell, header);
        self.layout_runs(&runs, self.setup.body_size, width)
    }

    /// Lays out runs as lines no wider than `width`. Each image gets a line of its own.
    fn layout_runs(&mut self, runs: &[Run], size: f32, width: f32) -> Vec<LineBox> {
        let mut lines = Vec::new();
        let mut after_image = false;
        for group in runs.chunk_by(|a, b| a.image.is_none() && b.image.is_none()) {
            match &group[0].image {
                Some(key) => {
                    if let Some(line) = self.layout_image(key, group[0].link.as_deref(), width) {
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

    /// An image scaled down, if needed, to fit `width` and the page's height.
    fn layout_image(&mut self, key: &str, link: Option<&str>, width: f32) -> Option<LineBox> {
        let image = self.decode(key)?;
        let (pixels_wide, pixels_high) = image.size();
        let natural_width = pixels_wide as f32 * POINTS_PER_PIXEL;
        let natural_height = pixels_high as f32 * POINTS_PER_PIXEL;
        let max_height = self.setup.bottom() - self.setup.margin;
        let scale = 1.0_f32
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
            let offset = text.len();
            text.push_str(&run.text);

            // Split the run where the font changes, and shape each piece.
            let mut pieces: Vec<(FontId, Range<usize>)> = Vec::new();
            for (i, c) in run.text.char_indices() {
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

/// A cell's runs, made bold in the header row.
fn cell_runs(cell: &CellRuns, header: bool) -> Vec<Run> {
    if header { all_bold(cell) } else { cell.clone() }
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
        let pages = lay_out(&[Block::Paragraph(text(&long))], setup);

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
        let pages = lay_out(&[Block::Paragraph(text(&word))], setup);

        let lines = page_lines(&pages[0]);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), word);
        let right = setup.width - setup.margin;
        assert!(right_edges(&pages[0]).iter().all(|&x| x <= right + 0.01));
    }

    #[test]
    fn keeps_line_breaks() {
        let pages = lay_out(&[Block::Paragraph(text("one\ntwo"))], PageSetup::DOCUMENT);
        assert_eq!(page_lines(&pages[0]), ["one", "two"]);
    }

    #[test]
    fn rules_start_a_new_page_for_slides_and_draw_a_line_in_documents() {
        let blocks = [
            Block::Paragraph(text("Slide one")),
            Block::Rule,
            Block::Paragraph(text("Slide two")),
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
            .map(|i| Block::Paragraph(text(&format!("Paragraph {i}"))))
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
            Block::Paragraph(text("between")),
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
        let cell = |t: &str| text(t);
        let mut rows = vec![vec![cell("Name"), cell("Value")]];
        rows.extend((0..80).map(|i| vec![cell(&format!("row {i}")), cell("x")]));
        let pages = lay_out(&[Block::Table(rows)], PageSetup::DOCUMENT);

        assert!(pages.len() > 1);
        for page in &pages {
            assert_eq!(page_lines(page)[0], "NameValue");
            assert!(page.items.iter().any(|i| matches!(i, Item::Shade { .. })));
        }
    }

    #[test]
    fn links_cover_their_text() {
        let runs = vec![
            Run::new("See ", RunStyle::default()),
            Run::new("the docs", RunStyle::default()).linked("https://example.com"),
        ];
        let pages = lay_out(&[Block::Paragraph(runs)], PageSetup::DOCUMENT);

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
