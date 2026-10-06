//! An Excel workbook: every sheet in every table format, and each sheet's pictures.

#![no_main]

use std::io::{self, Cursor};

use libfuzzer_sys::fuzz_target;
use officeconv::format::OutputFormat;
use officeconv::opc::Archive;
use officeconv::writers::{self, JsonValues};
use officeconv::xlsx::{self, pictures};
use officeconv_fuzz::LIMITS;

fuzz_target!(|data: &[u8]| {
    let Ok(sheets) = xlsx::read_all_sheets_with_limits(Cursor::new(data), LIMITS) else {
        return;
    };
    for sheet in &sheets {
        for format in [
            OutputFormat::Csv,
            OutputFormat::Tsv,
            OutputFormat::Json,
            OutputFormat::Markdown,
        ] {
            for json in [JsonValues::Text, JsonValues::Typed] {
                writers::write_table(&sheet.table, format, json, io::sink()).unwrap();
            }
        }
        if let Some(part) = &sheet.part
            && let Ok(mut archive) = Archive::with_limits(Cursor::new(data), LIMITS)
        {
            let _ = pictures::read_pictures(&mut archive, part);
        }
    }
});
