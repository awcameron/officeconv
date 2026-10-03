//! Laying out [`Block`]s on pages: wrapping text into lines and starting a new page when one
//! is full.
//!
//! The result is a list of [`Page`]s holding positioned [`Item`]s, which `pdf::render` then
//! paints. Keeping the two apart means the layout can be tested without reading a PDF back.
//!
//! Coordinates are in points (1/72 inch), measured from the top-left corner of the page.

use std::ops::Range;

use krilla::text::KrillaGlyph;
use unicode_linebreak::{BreakOpportunity, linebreaks};

use super::fonts::{FontId, Fonts};
use crate::document::{Block, Run, RunStyle};

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
}

/// Something to paint, at its final position on the page.
#[derive(Debug, Clone)]
pub enum Item {
    Text(TextItem),
    /// A thin line, for rules.
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
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
}

impl Item {
    fn moved(mut self, dx: f32, dy: f32) -> Self {
        match &mut self {
            Item::Text(text) => {
                text.x += dx;
                text.baseline += dy;
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

/// One line of text, laid out from (0, 0) until it's placed.
#[derive(Debug, Default)]
struct LineBox {
    height: f32,
    items: Vec<Item>,
}

/// Heading font sizes, as multiples of the body size, for levels 1 to 6.
const HEADING_SCALE: [f32; 6] = [2.0, 1.6, 1.3, 1.15, 1.0, 1.0];

/// Lays out blocks page by page.
pub struct Layout<'a> {
    fonts: &'a Fonts,
    setup: PageSetup,
    pages: Vec<Page>,
    /// Where the next block goes on the last page.
    y: f32,
}

impl<'a> Layout<'a> {
    pub fn new(fonts: &'a Fonts, setup: PageSetup) -> Self {
        Layout {
            fonts,
            setup,
            pages: vec![Page::default()],
            y: setup.margin,
        }
    }

    /// Lays out every block and returns the pages.
    ///
    /// Lists and tables aren't laid out as such yet: each list item and each table cell
    /// becomes a paragraph.
    pub fn run(mut self, blocks: &[Block]) -> Vec<Page> {
        let body = self.setup.body_size;
        let width = self.setup.content_width();
        let mut previous: Option<&Block> = None;

        for (i, block) in blocks.iter().enumerate() {
            let gap = match block {
                Block::Heading { level, .. } => body * heading_scale(*level) * 0.8,
                _ => body * 0.7,
            };
            if previous.is_some() {
                self.add_gap(gap);
            }

            match block {
                Block::Heading { level, runs } => {
                    let size = body * heading_scale(*level);
                    let runs = all_bold(runs);
                    let lines = self.layout_runs(&runs, size, width);
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
                Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
                    let lines = self.layout_runs(runs, body, width);
                    self.place_lines(lines, self.setup.margin);
                }
                Block::Table(rows) => {
                    for cell in rows.iter().flatten() {
                        let lines = self.layout_runs(cell, body, width);
                        self.place_lines(lines, self.setup.margin);
                    }
                }
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
        self.pages
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

    /// Adds a line's items to the current page, with its top-left corner at (x, y), and moves
    /// down past it.
    fn emit(&mut self, line: LineBox, x: f32, y: f32) {
        let page = self.page();
        page.items
            .extend(line.items.into_iter().map(|item| item.moved(x, y)));
        self.y = y + line.height;
    }

    /// Wraps runs into lines no wider than `width`, breaking where Unicode allows (after
    /// spaces and hyphens, between CJK characters) and always at `"\n"`. A word wider than a
    /// whole line is broken between characters.
    fn layout_runs(&mut self, runs: &[Run], size: f32, width: f32) -> Vec<LineBox> {
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
    /// `advance_before[i]` is the total advance of glyphs `0..i`, in ems.
    advance_before: Vec<f32>,
}

impl Paragraph {
    fn shape(fonts: &Fonts, runs: &[Run]) -> Self {
        let mut text = String::new();
        let mut glyphs = Vec::new();
        for (r, run) in runs.iter().enumerate() {
            let range = text.len()..text.len() + run.text.len();
            text.push_str(&run.text);
            let font = fonts.font_for(run.style);
            glyphs.extend(
                fonts
                    .shape(font, &text, range)
                    .into_iter()
                    .map(|g| (g, font, r)),
            );
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
            let (_, font, _) = group[0];
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

            line.items.push(Item::Text(TextItem {
                x,
                baseline,
                font,
                size,
                glyphs: rebased,
                text: self.text[start..end].to_string(),
            }));
            x += advance;
        }
        line
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Vec<Run> {
        vec![Run::new(text, RunStyle::default())]
    }

    fn lay_out(blocks: &[Block], setup: PageSetup) -> Vec<Page> {
        let fonts = Fonts::new();
        Layout::new(&fonts, setup).run(blocks)
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
}
