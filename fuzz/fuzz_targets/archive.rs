//! A zip archive: opening it, checking its sizes, and reading parts and relationships.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use officeconv::opc::{self, Archive};
use officeconv_fuzz::LIMITS;

fuzz_target!(|data: &[u8]| {
    let Ok(mut archive) = Archive::with_limits(Cursor::new(data), LIMITS) else {
        return;
    };
    let _ = archive.check_part_sizes();
    for name in ["[Content_Types].xml", "_rels/.rels", "word/document.xml"] {
        if let Ok(Some(xml)) = archive.read_part(name) {
            let _ = opc::parse_relationships(&xml);
        }
    }
});
