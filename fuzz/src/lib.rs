//! What the fuzz targets share.

use officeconv::fuzzing::{Block, EmbeddedImages, Limits, PageSetup, render_markdown, render_pdf};

/// Small enough that a size bug fails fast with an error, instead of using gigabytes or
/// minutes; large enough for any seed in the corpus.
///
/// Deflate shrinks repeated bytes about 1000 to 1, so a 64 KB input (the `-max_len` the README
/// suggests) can decompress to about 64 MB. That's well past these limits, so the fuzzer
/// reaches the code that enforces them.
pub const LIMITS: Limits = Limits {
    part: 1 << 20,
    total: 4 << 20,
};

/// Renders blocks a reader returned as Markdown and as PDF, as `--to md` and `--to pdf` would.
/// The results are thrown away: rendering may fail with an error, but must not panic.
pub fn render(blocks: &[Block], images: &EmbeddedImages, setup: PageSetup) {
    let _ = render_markdown(blocks);
    let _ = render_pdf(blocks, images, setup);
}
