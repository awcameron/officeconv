//! Fonts for PDF output: Noto Sans is built in, in the four styles a run can have.
//!
//! Noto Sans covers Latin, Greek and Cyrillic. Characters it doesn't have are drawn as boxes.

use std::ops::Range;

use krilla::text::{Font, GlyphId, KrillaGlyph};
use rustybuzz::{Direction, UnicodeBuffer};

use crate::document::RunStyle;

/// Noto Sans in the four styles a run can have, indexed by [`style_index`].
const NOTO_SANS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../../assets/fonts/NotoSans-Italic.ttf"),
    include_bytes!("../../assets/fonts/NotoSans-BoldItalic.ttf"),
];

/// Which loaded font to draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(usize);

/// One font, ready for both shaping (rustybuzz) and embedding (krilla).
struct LoadedFont {
    face: rustybuzz::Face<'static>,
    pdf: Font,
}

impl LoadedFont {
    fn new(data: &'static [u8], index: u32) -> Option<Self> {
        Some(LoadedFont {
            face: rustybuzz::Face::from_slice(data, index)?,
            pdf: Font::new(data.into(), index)?,
        })
    }
}

/// The fonts a PDF draws with.
pub struct Fonts {
    loaded: Vec<LoadedFont>,
}

impl Fonts {
    pub fn new() -> Self {
        let loaded = NOTO_SANS
            .iter()
            .map(|data| LoadedFont::new(data, 0).expect("the built-in fonts are valid"))
            .collect();
        Fonts { loaded }
    }

    /// The font to draw text in `style` with.
    pub fn font_for(&self, style: RunStyle) -> FontId {
        FontId(style_index(style))
    }

    /// The font to embed in the PDF for `id`.
    pub fn pdf_font(&self, id: FontId) -> Font {
        self.loaded[id.0].pdf.clone()
    }

    /// Distance from the top of a line to its baseline, and the line's height, as fractions of
    /// the font size.
    pub fn line_metrics(&self) -> (f32, f32) {
        let face = &self.loaded[0].face;
        let em = face.units_per_em() as f32;
        let ascent = f32::from(face.ascender()) / em;
        let descent = -f32::from(face.descender()) / em;
        let height = 1.4_f32.max(ascent + descent);
        (ascent + (height - ascent - descent) / 2.0, height)
    }

    /// Shapes `text[range]` with one font, left to right.
    ///
    /// Each glyph's `text_range` is a range of `text` (not of the slice), and its advance and
    /// offsets are fractions of the font size, which is what krilla expects.
    pub fn shape(&self, id: FontId, text: &str, range: Range<usize>) -> Vec<KrillaGlyph> {
        let face = &self.loaded[id.0].face;
        let em = face.units_per_em() as f32;

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(&text[range.clone()]);
        buffer.guess_segment_properties();
        // Lines are laid out left to right, and the glyph slicing in `layout` relies on
        // clusters only going forward.
        buffer.set_direction(Direction::LeftToRight);
        let shaped = rustybuzz::shape(face, &[], buffer);

        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        (0..shaped.len())
            .map(|i| {
                let start = infos[i].cluster as usize;
                // A cluster ends where the next different one starts.
                let end = infos[i + 1..]
                    .iter()
                    .map(|info| info.cluster as usize)
                    .find(|&cluster| cluster != start)
                    .unwrap_or(range.len());
                let position = positions[i];
                KrillaGlyph::new(
                    GlyphId::new(infos[i].glyph_id),
                    position.x_advance as f32 / em,
                    position.x_offset as f32 / em,
                    position.y_offset as f32 / em,
                    position.y_advance as f32 / em,
                    range.start + start..range.start + end,
                    None,
                )
            })
            .collect()
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

/// Which of the four Noto Sans styles to use.
fn style_index(style: RunStyle) -> usize {
    usize::from(style.bold) + 2 * usize::from(style.italic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_style() {
        let fonts = Fonts::new();
        let style = |bold, italic| RunStyle { bold, italic };
        assert_eq!(fonts.font_for(style(false, false)), FontId(0));
        assert_eq!(fonts.font_for(style(true, false)), FontId(1));
        assert_eq!(fonts.font_for(style(false, true)), FontId(2));
        assert_eq!(fonts.font_for(style(true, true)), FontId(3));
    }

    #[test]
    fn shaping_maps_glyphs_back_to_the_whole_text() {
        let fonts = Fonts::new();
        let text = "ab café";
        let glyphs = fonts.shape(FontId(0), text, 3..text.len());

        let ranges: Vec<_> = glyphs.iter().map(|g| g.text_range.clone()).collect();
        assert_eq!(ranges.first(), Some(&(3..4)));
        assert_eq!(ranges.last().map(|r| r.end), Some(text.len()));
        assert!(glyphs.iter().all(|g| g.x_advance > 0.0));
    }

    #[test]
    fn line_height_fits_the_font() {
        let (baseline, height) = Fonts::new().line_metrics();
        assert!(baseline > 0.9 && baseline < height, "{baseline} {height}");
        assert!((1.36..1.5).contains(&height), "{height}");
    }
}
