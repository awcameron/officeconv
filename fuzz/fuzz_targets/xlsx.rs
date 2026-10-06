//! An Excel workbook: every sheet in every table format, and each sheet's pictures.

#![no_main]

use std::io::{self, Cursor};

use libfuzzer_sys::fuzz_target;
use officeconv::fuzzing::{
    Archive, JsonValues, OutputFormat, read_pictures, read_xlsx, write_table,
};
use officeconv_fuzz::LIMITS;

/// Each way a sheet can be written. Only JSON looks at [`JsonValues`].
const OUTPUTS: [(OutputFormat, JsonValues); 5] = [
    (OutputFormat::Csv, JsonValues::Text),
    (OutputFormat::Tsv, JsonValues::Text),
    (OutputFormat::Json, JsonValues::Text),
    (OutputFormat::Json, JsonValues::Typed),
    (OutputFormat::Markdown, JsonValues::Text),
];

fuzz_target!(|data: &[u8]| {
    let Ok(sheets) = read_xlsx(Cursor::new(data), LIMITS) else {
        return;
    };
    for sheet in &sheets {
        for (format, json) in OUTPUTS {
            // Writing to a sink can't fail, so an error here is a bug in the writers.
            write_table(&sheet.table, format, json, io::sink()).unwrap();
        }
    }

    // Pictures are read from one archive shared by every sheet, as the CLI does with
    // `--images`, so the limits count what all the sheets read together.
    let Ok(mut archive) = Archive::with_limits(Cursor::new(data), LIMITS) else {
        return;
    };
    for part in sheets.iter().filter_map(|sheet| sheet.part.as_deref()) {
        let _ = read_pictures(&mut archive, part);
    }
});
