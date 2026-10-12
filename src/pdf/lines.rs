//! Breaking text into lines: a paragraph's runs are joined, shaped, and wrapped at the width
//! they're given. It needs only the fonts, so it's tested without laying out a page.
//!
//! Right-to-left text, such as Arabic or Hebrew, follows the Unicode Bidirectional Algorithm:
//! lines are broken in text order, then each line's runs are reordered for drawing. A
//! paragraph reads right to left when its first letter does.

use std::borrow::Cow;
use std::ops::Range;

use krilla::text::KrillaGlyph;
use unicode_bidi::{BidiInfo, Level};
use unicode_linebreak::{BreakOpportunity, linebreaks};

use super::fonts::{FontId, Fonts};
use super::layout::{Item, Link, TextItem, note_marker};
use crate::document::{Run, RunKind};

/// One line of text, or one image, laid out from (0, 0) until it's placed.
#[derive(Debug, Default)]
pub struct LineBox {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
    pub links: Vec<Link>,
}

/// Wraps text runs into lines no wider than `width`, breaking where Unicode allows (after
/// spaces and hyphens, between CJK characters) and always at `"\n"`. A word wider than a
/// whole line is broken between characters.
pub fn break_lines(fonts: &mut Fonts, runs: &[Run], size: f32, width: f32) -> Vec<LineBox> {
    let paragraph = Paragraph::shape(fonts, runs);
    if paragraph.text.trim().is_empty() {
        return Vec::new();
    }
    let (baseline, line_height) = fonts.line_metrics();
    let line_height = line_height * size;
    let fits = |range: Range<usize>| paragraph.width(range) * size <= width;

    let mut lines: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    // End of the current line's text, without trailing spaces; `start` while empty.
    let mut end = 0;
    let mut word_start = 0;
    // Whether the text since the last "\n" is blank, so its break makes an empty line.
    let mut blank = true;
    for (word_end, opportunity) in linebreaks(&paragraph.text) {
        let word_content = word_start + paragraph.text[word_start..word_end].trim_end().len();
        if word_content > word_start {
            blank = false;
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
            // A word split between characters may have used up the line already.
            if end > start || blank {
                lines.push(start..end.max(start));
            }
            start = word_end;
            end = word_end;
            blank = true;
        }
        word_start = word_end;
    }
    // The text always ends with a mandatory break, so `start` is at the end now.

    lines
        .into_iter()
        .map(|range| paragraph.line(range, size, baseline * size, line_height))
        .collect()
}

/// The width of the widest word in text `runs`, which can't be narrowed without breaking it.
pub fn longest_word(fonts: &mut Fonts, runs: &[Run], size: f32) -> f32 {
    Paragraph::shape(fonts, runs).longest_word() * size
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
    /// The embedding level of each byte of `text`, from the Unicode Bidirectional Algorithm:
    /// odd where it reads right to left.
    levels: Vec<Level>,
}

impl Paragraph {
    fn shape(fonts: &mut Fonts, runs: &[Run]) -> Self {
        let mut text = String::new();
        let mut run_ranges = Vec::with_capacity(runs.len());
        for run in runs {
            let run_text = match run.kind {
                RunKind::Note(number) => Cow::Owned(note_marker(number)),
                RunKind::Text | RunKind::Image(_) | RunKind::Field(_) => {
                    Cow::Borrowed(run.text.as_str())
                }
            };
            let start = text.len();
            text.push_str(&run_text);
            run_ranges.push(start..text.len());
        }
        // Each "\n" starts a paragraph of its own here, with its own direction.
        let levels = BidiInfo::new(&text, None).levels;

        let mut glyphs = Vec::new();
        for (r, (run, range)) in runs.iter().zip(&run_ranges).enumerate() {
            // Split the run where the font or the direction changes, and shape each piece.
            let mut pieces: Vec<(FontId, bool, Range<usize>)> = Vec::new();
            for (i, c) in text[range.clone()].char_indices() {
                let at = range.start + i;
                let font = fonts.font_for(c, run.style);
                let rtl = levels[at].is_rtl();
                match pieces.last_mut() {
                    Some((last, last_rtl, piece)) if *last == font && *last_rtl == rtl => {
                        piece.end = at + c.len_utf8();
                    }
                    _ => pieces.push((font, rtl, at..at + c.len_utf8())),
                }
            }
            for (font, rtl, piece) in pieces {
                glyphs.extend(
                    fonts
                        .shape(font, &text, piece, rtl)
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
            levels,
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

    /// `range` of the text as runs of one embedding level, in the order they're drawn, each with
    /// whether it reads right to left.
    ///
    /// This is rule L2 of the Unicode Bidirectional Algorithm: from the highest level down to
    /// the lowest odd one, every sequence of runs at that level or higher is reversed. Levels
    /// go no higher than 125, so it's linear in the length of the line. Rule L1 needs no work:
    /// a line never ends in spaces.
    fn visual_runs(&self, range: Range<usize>) -> Vec<(Range<usize>, bool)> {
        let mut runs: Vec<(Range<usize>, Level)> = Vec::new();
        for (i, c) in self.text[range.clone()].char_indices() {
            let at = range.start + i;
            let level = self.levels[at];
            match runs.last_mut() {
                Some((run, last)) if *last == level => run.end = at + c.len_utf8(),
                _ => runs.push((at..at + c.len_utf8(), level)),
            }
        }
        let highest = runs.iter().map(|(_, l)| l.number()).max().unwrap_or(0);
        let lowest_odd = runs.iter().map(|(_, l)| l.number() | 1).min().unwrap_or(1);
        for level in (lowest_odd..=highest).rev() {
            let mut i = 0;
            while i < runs.len() {
                let len = runs[i..]
                    .iter()
                    .take_while(|(_, l)| l.number() >= level)
                    .count();
                runs[i..i + len].reverse();
                i += len.max(1);
            }
        }
        runs.into_iter()
            .map(|(run, level)| (run, level.is_rtl()))
            .collect()
    }

    /// One line of the paragraph, as text items grouped by font and run, in the order they're
    /// drawn.
    fn line(&self, range: Range<usize>, size: f32, baseline: f32, height: f32) -> LineBox {
        let mut line = LineBox {
            height,
            ..LineBox::default()
        };
        let mut x = 0.0;
        for (piece, rtl) in self.visual_runs(range) {
            let span = self.glyph_span(piece);
            // Glyphs are kept in text order, so a right-to-left run's are drawn backwards.
            let mut glyphs: Vec<&(KrillaGlyph, FontId, usize)> = self.glyphs[span].iter().collect();
            if rtl {
                glyphs.reverse();
            }
            for group in glyphs.chunk_by(|a, b| a.1 == b.1 && a.2 == b.2) {
                let &(_, font, run) = group[0];
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
        }
        line.width = x;
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RunStyle;

    const SIZE: f32 = 12.0;

    fn text(text: &str) -> Vec<Run> {
        vec![Run::new(text, RunStyle::default())]
    }

    /// Each line's text.
    fn texts(lines: &[LineBox]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Text(t) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect()
            })
            .collect()
    }

    fn assert_near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    /// How wide `text` is on one line, without trailing spaces.
    fn width_of(fonts: &mut Fonts, text_: &str) -> f32 {
        break_lines(fonts, &text(text_), SIZE, f32::INFINITY)[0].width
    }

    #[test]
    fn wraps_between_words_at_the_width() {
        let mut fonts = Fonts::new();
        let width = width_of(&mut fonts, "aaa bbb");
        let lines = break_lines(&mut fonts, &text("aaa bbb ccc"), SIZE, width);
        assert_eq!(texts(&lines), ["aaa bbb", "ccc"]);
        assert!(lines.iter().all(|line| line.width <= width));
        // A trailing space doesn't count against the width.
        assert_eq!(lines[0].width, width);
    }

    #[test]
    fn breaks_at_every_newline_and_drops_blank_text() {
        let mut fonts = Fonts::new();
        let lines = break_lines(&mut fonts, &text("one\ntwo"), SIZE, f32::INFINITY);
        assert_eq!(texts(&lines), ["one", "two"]);
        assert!(break_lines(&mut fonts, &text("  \n "), SIZE, f32::INFINITY).is_empty());
    }

    #[test]
    fn splits_a_word_wider_than_a_line_between_characters() {
        let mut fonts = Fonts::new();
        let word = "abcdefghijklmnop";
        let width = width_of(&mut fonts, word) / 3.0;
        let lines = break_lines(&mut fonts, &text(word), SIZE, width);

        let texts = texts(&lines);
        assert!(texts.len() >= 3, "{texts:?}");
        assert_eq!(texts.concat(), word);
        assert!(lines.iter().all(|line| line.width <= width));
    }

    #[test]
    fn measures_the_widest_word() {
        let mut fonts = Fonts::new();
        let widest = width_of(&mut fonts, "elephant");
        assert_near(
            longest_word(&mut fonts, &text("an elephant ate"), SIZE),
            widest,
        );
    }

    #[test]
    fn a_link_covers_its_runs_text() {
        let mut fonts = Fonts::new();
        let runs = vec![
            Run::new("see ", RunStyle::default()),
            Run::new("here", RunStyle::default()).linked("https://example.com"),
        ];
        let lines = break_lines(&mut fonts, &runs, SIZE, f32::INFINITY);
        let [link] = &lines[0].links[..] else {
            panic!("one link: {:?}", lines[0].links);
        };
        assert_eq!(link.url, "https://example.com");
        // It starts after "see " and its space, and runs to the end of the line.
        assert!(link.x > width_of(&mut fonts, "see"));
        assert_near(link.width, width_of(&mut fonts, "here"));
        assert_near(link.x + link.width, lines[0].width);
    }

    /// Each glyph's text in the order the line draws it, left to right, checking the glyphs
    /// really do go left to right.
    fn drawn(line: &LineBox) -> String {
        let mut drawn = String::new();
        let mut last_x = f32::NEG_INFINITY;
        for item in &line.items {
            let Item::Text(t) = item else { continue };
            let mut x = t.x;
            for glyph in &t.glyphs {
                assert!(x > last_x, "glyphs go left to right");
                last_x = x;
                x += glyph.x_advance * t.size;
                drawn.push_str(&t.text[glyph.text_range.clone()]);
            }
        }
        drawn
    }

    fn one_line(fonts: &mut Fonts, runs: &[Run]) -> LineBox {
        let mut lines = break_lines(fonts, runs, SIZE, f32::INFINITY);
        assert_eq!(lines.len(), 1);
        lines.remove(0)
    }

    #[test]
    fn draws_right_to_left_words_from_the_right() {
        // Glyphs are checked by the text they draw, so this holds even where no installed font
        // has Hebrew or Arabic and they're drawn as boxes.
        let mut fonts = Fonts::new();
        assert_eq!(drawn(&one_line(&mut fonts, &text("שלום"))), "םולש");
        assert_eq!(drawn(&one_line(&mut fonts, &text("مرحبا"))), "ابحرم");
    }

    #[test]
    fn reverses_a_right_to_left_phrase_in_place_in_a_left_to_right_line() {
        let mut fonts = Fonts::new();
        let line = one_line(&mut fonts, &text("Hello שלום עולם world"));
        assert_eq!(drawn(&line), "Hello םלוע םולש world");
    }

    #[test]
    fn keeps_numbers_left_to_right_inside_right_to_left_text() {
        let mut fonts = Fonts::new();
        let line = one_line(&mut fonts, &text("שלום 123 עולם"));
        assert_eq!(drawn(&line), "םלוע 123 םולש");
    }

    #[test]
    fn gives_each_line_after_a_newline_its_own_direction() {
        let mut fonts = Fonts::new();
        let lines = break_lines(
            &mut fonts,
            &text("שלום world\nHello עולם"),
            SIZE,
            f32::INFINITY,
        );
        let drawn: Vec<String> = lines.iter().map(drawn).collect();
        assert_eq!(drawn, ["world םולש", "Hello םלוע"]);
    }

    #[test]
    fn a_link_in_right_to_left_text_covers_where_it_is_drawn() {
        let mut fonts = Fonts::new();
        let runs = vec![
            Run::new("שלום ", RunStyle::default()),
            Run::new("עולם", RunStyle::default()).linked("https://example.com"),
        ];
        let line = one_line(&mut fonts, &runs);
        assert_eq!(drawn(&line), "םלוע םולש");
        // The linked word comes second, so it's drawn first, at the left.
        let [link] = &line.links[..] else {
            panic!("one link: {:?}", line.links);
        };
        assert_near(link.x, 0.0);
        assert_near(link.width, width_of(&mut fonts, "עולם"));
    }

    #[test]
    fn a_word_split_to_the_last_character_adds_no_empty_line() {
        let mut fonts = Fonts::new();
        let lines = break_lines(&mut fonts, &text("abc"), SIZE, 0.1);
        assert_eq!(texts(&lines), ["a", "b", "c"]);
        // A blank line between two others is still kept.
        let lines = break_lines(&mut fonts, &text("one\n\ntwo"), SIZE, f32::INFINITY);
        assert_eq!(texts(&lines), ["one", "", "two"]);
    }
}
