//! A Word document, converted to Markdown and PDF.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use officeconv::fuzzing::{Archive, EmbeddedImages, Images, PageSetup, read_docx, resolve_images};
use officeconv_fuzz::{LIMITS, render};

fuzz_target!(|data: &[u8]| {
    let Ok(mut archive) = Archive::with_limits(Cursor::new(data), LIMITS) else {
        return;
    };
    let Ok(blocks) = read_docx(&mut archive) else {
        return;
    };
    let mut images = EmbeddedImages::default();
    let Ok(blocks) = resolve_images(blocks, &mut archive, Images::Embed(&mut images)) else {
        return;
    };
    render(&blocks, &images, PageSetup::DOCUMENT);
});
