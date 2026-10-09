//! A CSV or TSV file: read both ways, and written in every table format.

#![no_main]

use std::io;

use libfuzzer_sys::fuzz_target;
use officeconv::fuzzing::{
    DelimitedLimits, InputKind, JsonValues, TableFormat, read_delimited, write_table,
};

/// Small enough that a size bug fails fast, as with the other targets' limits. A 64 KB input
/// is under `bytes`, but one wide row and many short ones can still reach `cells`.
const LIMITS: DelimitedLimits = DelimitedLimits {
    bytes: 1 << 20,
    cells: 1 << 16,
};

/// Each way a table can be written. CSV and TSV input don't allow `--typed`.
const OUTPUTS: [TableFormat; 4] = [
    TableFormat::Csv,
    TableFormat::Tsv,
    TableFormat::Json(JsonValues::Text),
    TableFormat::Markdown,
];

fuzz_target!(|data: &[u8]| {
    for kind in [InputKind::Csv, InputKind::Tsv] {
        let Ok(table) = read_delimited(data, kind, LIMITS) else {
            continue;
        };
        for format in OUTPUTS {
            // Writing to a sink can't fail, so an error here is a bug in the writers.
            write_table(&table, format, io::sink()).unwrap();
        }
    }
});
