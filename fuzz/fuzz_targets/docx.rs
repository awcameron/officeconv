//! A Word document, converted to Markdown and PDF.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use officeconv::fuzzing::{EmbeddedImages, Images, PageSetup, read_docx};
use officeconv_fuzz::{LIMITS, render};

fuzz_target!(|data: &[u8]| {
    let mut images = EmbeddedImages::default();
    let Ok(blocks) = read_docx(Cursor::new(data), Images::Embed(&mut images), LIMITS) else {
        return;
    };
    render(&blocks, &images, PageSetup::DOCUMENT);
});
