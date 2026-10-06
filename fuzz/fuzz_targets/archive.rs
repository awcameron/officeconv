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
    // Every workbook goes through this before calamine reads it. Errors, such as a part over
    // the limit, are expected here and in every target: only a panic, hang or blowup is a bug.
    let _ = archive.check_part_sizes();

    // Parts are looked up by name, so these are names the seeds contain. Each is parsed as
    // relationships whatever it holds: the fuzzer changes a part's contents far more often
    // than its name, so this gives the relationships parser all kinds of XML.
    for name in ["[Content_Types].xml", "_rels/.rels", "word/document.xml"] {
        if let Ok(Some(xml)) = archive.read_part(name) {
            let _ = opc::parse_relationships(&xml);
        }
    }
});
