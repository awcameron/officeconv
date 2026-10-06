//! A PowerPoint deck, with its notes, converted to Markdown and PDF.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use officeconv::images::{EmbeddedImages, Images};
use officeconv::pdf::layout::PageSetup;
use officeconv::pptx::{self, Notes};
use officeconv_fuzz::{LIMITS, render};

fuzz_target!(|data: &[u8]| {
    let mut images = EmbeddedImages::default();
    let Ok(blocks) = pptx::read_blocks_with_limits(
        Cursor::new(data),
        Notes::Include,
        Images::Embed(&mut images),
        LIMITS,
    ) else {
        return;
    };
    render(&blocks, &images, PageSetup::SLIDES);
});
