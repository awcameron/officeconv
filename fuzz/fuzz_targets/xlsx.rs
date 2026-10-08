//! An Excel workbook: every sheet in every table format, and each sheet's pictures.

#![no_main]

use std::io::{self, Cursor};

use libfuzzer_sys::fuzz_target;
use officeconv::fuzzing::{JsonValues, OutputFormat, Pictures, read_xlsx, write_table};
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
    // Pictures are read from one archive shared by every sheet, as the CLI does with
    // `--images`, so the limits count what all the sheets read together. A workbook whose
    // pictures can't be read is read again without them, so its tables still reach the writers.
    let read = |pictures| read_xlsx(Cursor::new(data), pictures, LIMITS);
    let Ok(workbook) = read(Pictures::Include).or_else(|_| read(Pictures::Skip)) else {
        return;
    };
    for sheet in &workbook.sheets {
        for (format, json) in OUTPUTS {
            // Writing to a sink can't fail, so an error here is a bug in the writers.
            write_table(&sheet.table, format, json, io::sink()).unwrap();
        }
    }
});
