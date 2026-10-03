//! Fonts for PDF output: Noto Sans is built in, and installed fonts fill in what it lacks.
//!
//! Noto Sans covers Latin, Greek and Cyrillic, so most documents never look further. For
//! anything else, such as Chinese, Japanese or Korean, the fonts installed on the computer
//! are searched the first time a character is missing, and only the ones that are needed
//! are loaded.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use fontdb::{Database, Style, Weight};
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

/// Installed fonts to try first for characters Noto Sans doesn't have, in order: the CJK
/// fonts that come with Linux distributions, macOS and Windows. Any other installed font
/// that has the character is used after these.
const PREFERRED_FALLBACKS: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "Noto Sans CJK TC",
    "Source Han Sans SC",
    "Source Han Sans",
    "PingFang SC",
    "Hiragino Sans",
    "Hiragino Sans GB",
    "Apple SD Gothic Neo",
    "Microsoft YaHei",
    "Yu Gothic",
    "Malgun Gothic",
    "WenQuanYi Micro Hei",
    "Droid Sans Fallback",
];

/// Installed fonts never used as a fallback. macOS's LastResort claims every character but
/// only draws a placeholder box for each, which would hide that the character is missing.
const NEVER_FALLBACKS: &[&str] = &["LastResort", "Last Resort", ".LastResort"];

/// Which loaded font to draw with. The first four are Noto Sans; fallbacks come after.
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

    fn has(&self, c: char) -> bool {
        self.face.glyph_index(c).is_some()
    }
}

/// The fonts a PDF draws with, and the characters none of them could draw.
pub struct Fonts {
    loaded: Vec<LoadedFont>,
    /// Installed fonts, read the first time Noto Sans is missing a character, and the order
    /// to try them in.
    system: Option<(Database, Vec<fontdb::ID>)>,
    /// Character -> the fallback that has it, or `None` if no font does.
    fallbacks: HashMap<char, Option<FontId>>,
    missing: BTreeSet<char>,
}

impl Fonts {
    pub fn new() -> Self {
        let loaded = NOTO_SANS
            .iter()
            .map(|data| LoadedFont::new(data, 0).expect("the built-in fonts are valid"))
            .collect();
        Fonts {
            loaded,
            system: None,
            fallbacks: HashMap::new(),
            missing: BTreeSet::new(),
        }
    }

    /// The font to draw `c` with in `style`: Noto Sans when it has `c`, otherwise an installed
    /// font that does. If none does, Noto Sans draws it as a box and `c` is noted as missing.
    pub fn font_for(&mut self, c: char, style: RunStyle) -> FontId {
        let noto = FontId(style_index(style));
        if c.is_control() || self.loaded[noto.0].has(c) {
            return noto;
        }
        match self.fallback(c) {
            Some(id) => id,
            None => {
                self.missing.insert(c);
                noto
            }
        }
    }

    /// Characters no font could draw, in order.
    pub fn missing(&self) -> &BTreeSet<char> {
        &self.missing
    }

    /// The font to embed in the PDF for `id`.
    pub fn pdf_font(&self, id: FontId) -> Font {
        self.loaded[id.0].pdf.clone()
    }

    /// Distance from the top of a line to its baseline, and the line's height, as fractions of
    /// the font size. Taken from Noto Sans, so lines are evenly spaced whatever the fallback.
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

    /// A loaded or installed font that has `c`, loading it if needed.
    fn fallback(&mut self, c: char) -> Option<FontId> {
        if let Some(&found) = self.fallbacks.get(&c) {
            return found;
        }
        let found = match (NOTO_SANS.len()..self.loaded.len()).find(|&i| self.loaded[i].has(c)) {
            Some(i) => Some(FontId(i)),
            None => self.load_fallback(c),
        };
        self.fallbacks.insert(c, found);
        found
    }

    /// Finds an installed font that has `c` and loads it.
    fn load_fallback(&mut self, c: char) -> Option<FontId> {
        let (system, candidates) = self.system.get_or_insert_with(|| {
            let mut db = Database::new();
            db.load_system_fonts();
            let candidates = fallback_candidates(&db);
            (db, candidates)
        });

        for &id in candidates.iter() {
            let has_c = system
                .with_face_data(id, |data, index| ttf_has(data, index, c))
                .unwrap_or(false);
            if !has_c {
                continue;
            }
            // krilla and rustybuzz both borrow the font's bytes for as long as the PDF is being
            // built, which is the rest of the run. Leaking the (usually one) fallback font is
            // simpler than tying every glyph to its owner, and the OS frees it on exit.
            let Some((data, index)) =
                system.with_face_data(id, |data, index| (data.to_vec(), index))
            else {
                continue;
            };
            let data: &'static [u8] = Box::leak(data.into_boxed_slice());
            if let Some(font) = LoadedFont::new(data, index) {
                self.loaded.push(font);
                return Some(FontId(self.loaded.len() - 1));
            }
        }
        None
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

/// True if the font in `data` has a glyph for `c`.
fn ttf_has(data: &[u8], index: u32, c: char) -> bool {
    rustybuzz::ttf_parser::Face::parse(data, index).is_ok_and(|face| face.glyph_index(c).is_some())
}

/// Installed fonts in the order to try them: the preferred families first, then regular
/// upright faces, then everything else.
fn fallback_candidates(system: &Database) -> Vec<fontdb::ID> {
    let rank = |face: &fontdb::FaceInfo| {
        let preferred = face
            .families
            .iter()
            .filter_map(|(name, _)| PREFERRED_FALLBACKS.iter().position(|p| p == name))
            .min()
            .unwrap_or(PREFERRED_FALLBACKS.len());
        let plain = face.style == Style::Normal && face.weight == Weight::NORMAL;
        (preferred, !plain)
    };
    let mut faces: Vec<_> = system
        .faces()
        .filter(|face| {
            !face
                .families
                .iter()
                .any(|(name, _)| NEVER_FALLBACKS.contains(&name.as_str()))
        })
        .collect();
    faces.sort_by_key(|face| rank(face));
    faces.into_iter().map(|face| face.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: RunStyle = RunStyle {
        bold: false,
        italic: false,
    };

    #[test]
    fn noto_sans_covers_latin_greek_and_cyrillic() {
        let mut fonts = Fonts::new();
        for c in "Café Ωμέγα Привет – • ".chars() {
            assert_eq!(fonts.font_for(c, PLAIN), FontId(0), "{c:?}");
        }
        let bold_italic = RunStyle {
            bold: true,
            italic: true,
        };
        assert_eq!(fonts.font_for('a', bold_italic), FontId(3));
        assert!(fonts.missing().is_empty());
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
