//! Reads a CSV or TSV file into a [`Table`].
//!
//! The first row is the header, as it is for a sheet. Fields can be quoted the way the writers
//! in `writers` quote them, so a field may hold the delimiter, a quote (written twice) or a line
//! break. TSV follows the same rules with a tab, as Excel's "Text (Tab delimited)" does.

use std::io::Read;

use crate::error::{ConvertError, Result};
use crate::input::InputKind;
use crate::opc;
use crate::table::Table;

/// How much one CSV or TSV input may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes read: the same as one part of an Office file, since a CSV file is what a
    /// worksheet part holds.
    pub bytes: u64,
    /// The most cells in the table, counting short rows as padded to the widest one. Each cell
    /// takes 32 bytes however short its text, so this bounds memory where `bytes` alone
    /// wouldn't: a small file with one very wide row and many short ones pads out to a huge
    /// table.
    pub cells: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        bytes: opc::Limits::DEFAULT.part,
        cells: 32_000_000,
    };
}

/// The UTF-8 byte-order mark, which Excel writes at the start of "CSV UTF-8" files.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Reads `reader` as CSV (`kind` is [`InputKind::Csv`]) or TSV ([`InputKind::Tsv`]).
pub fn read_table(reader: impl Read, kind: InputKind) -> Result<Table> {
    read_table_with_limits(reader, kind, Limits::DEFAULT)
}

/// [`read_table`] with smaller limits than [`Limits::DEFAULT`]. The fuzz target uses this.
pub fn read_table_with_limits(reader: impl Read, kind: InputKind, limits: Limits) -> Result<Table> {
    let delimiter = match kind {
        InputKind::Tsv => b'\t',
        _ => b',',
    };
    let syntax = |line, problem| ConvertError::Delimited {
        kind,
        line,
        problem,
    };

    let mut bytes = Vec::new();
    reader
        .take(limits.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(ConvertError::ReadInput)?;
    if bytes.len() as u64 > limits.bytes {
        return Err(ConvertError::DelimitedTooLarge {
            limit: limits.bytes,
        });
    }
    let bytes = bytes.strip_prefix(BOM).unwrap_or(&bytes);
    let text = std::str::from_utf8(bytes)
        .map_err(|err| syntax(line_at(bytes, err.valid_up_to()), "invalid UTF-8"))?;
    check_quotes(text, delimiter).map_err(|(line, problem)| syntax(line, problem))?;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut width = 0;
    for record in reader.records() {
        // The text is valid UTF-8 and reading from memory can't fail, so this can't happen.
        let record = record.expect("the csv crate accepts any UTF-8 text");
        width = width.max(record.len());
        // Checked as rows arrive, so a file over the limit stops before it's all held.
        let cells = (width as u64).saturating_mul(rows.len() as u64 + 1);
        if cells > limits.cells {
            return Err(ConvertError::DelimitedTooManyCells {
                limit: limits.cells,
            });
        }
        rows.push(record.iter().map(str::to_owned).collect());
    }

    // A row longer than the header widens the table instead of losing its last values. The
    // extra headers are blank, and JSON names them column_N as it does any blank header.
    Ok(Table::from_rows(rows))
}

/// The line, counting from 1, that byte `offset` of `bytes` is on.
fn line_at(bytes: &[u8], offset: usize) -> u64 {
    bytes[..offset].iter().filter(|&&b| b == b'\n').count() as u64 + 1
}

/// Finds the quoting mistakes the csv crate accepts without a word: a quoted field that's never
/// closed swallows the rest of the file, and text after a closing quote is glued onto the field.
/// Returns the line of the mistake and what it is.
///
/// A quote inside an unquoted field, as in `5" screen`, is left alone: it's common, and its
/// meaning is clear.
fn check_quotes(text: &str, delimiter: u8) -> std::result::Result<(), (u64, &'static str)> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        FieldStart,
        Unquoted,
        Quoted,
        /// Just after a quote inside a quoted field: either it closes the field, or a second
        /// quote follows and the pair stands for one quote.
        AfterQuote,
    }
    use State::*;

    let mut state = FieldStart;
    let mut line = 1;
    let mut opened_on = 1;
    for &b in text.as_bytes() {
        state = match (state, b) {
            (Quoted, b'"') => AfterQuote,
            (Quoted, _) => Quoted,
            (AfterQuote, b'"') => Quoted,
            (FieldStart, b'"') => {
                opened_on = line;
                Quoted
            }
            (_, b'\n' | b'\r') => FieldStart,
            (_, b) if b == delimiter => FieldStart,
            (AfterQuote, _) => return Err((line, "text after a closing quote")),
            _ => Unquoted,
        };
        if b == b'\n' {
            line += 1;
        }
    }
    if state == Quoted {
        return Err((opened_on, "a quoted field is never closed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Cell;

    fn read(text: &str) -> Result<Table> {
        read_table(text.as_bytes(), InputKind::Csv)
    }

    fn text(cells: &[&str]) -> Vec<Cell> {
        cells.iter().map(|c| Cell::Text(c.to_string())).collect()
    }

    #[test]
    fn first_row_is_the_header() {
        let table = read("Name,Age\nAda,36\nAlan,41\n").unwrap();
        assert_eq!(table.headers, ["Name", "Age"]);
        assert_eq!(table.rows, [text(&["Ada", "36"]), text(&["Alan", "41"])]);
    }

    #[test]
    fn quoted_fields_hold_delimiters_quotes_and_line_breaks() {
        let table = read("a,b,c\n\"x, y\",\"say \"\"hi\"\"\",\"two\r\nlines\"\r\n").unwrap();
        assert_eq!(table.rows, [text(&["x, y", "say \"hi\"", "two\r\nlines"])]);
    }

    #[test]
    fn tsv_splits_on_tabs() {
        let table = read_table("a\tb\n1,5\t\"x\ty\"\n".as_bytes(), InputKind::Tsv).unwrap();
        assert_eq!(table.headers, ["a", "b"]);
        assert_eq!(table.rows, [text(&["1,5", "x\ty"])]);
    }

    #[test]
    fn skips_a_byte_order_mark() {
        let table = read("\u{FEFF}Name\nAda\n").unwrap();
        assert_eq!(table.headers, ["Name"]);
    }

    #[test]
    fn widens_the_table_for_long_rows_and_pads_short_ones() {
        let table = read("a,b\n1,2,3\n4\n").unwrap();
        assert_eq!(table.headers, ["a", "b", ""]);
        assert_eq!(
            table.rows,
            [
                text(&["1", "2", "3"]),
                vec![Cell::Text("4".into()), Cell::Empty, Cell::Empty]
            ]
        );
    }

    #[test]
    fn an_empty_file_is_an_empty_table() {
        assert!(read("").unwrap().is_empty());
        assert!(read("\u{FEFF}").unwrap().is_empty());
    }

    #[test]
    fn a_quote_inside_an_unquoted_field_is_kept() {
        let table = read("size\n5\" screen\n").unwrap();
        assert_eq!(table.rows, [text(&["5\" screen"])]);
    }

    #[test]
    fn reports_quoting_mistakes_with_their_line() {
        let err = read("a,b\n1,2\n\"open,3\n4,5\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "could not read the csv input: line 3: a quoted field is never closed"
        );
        let err = read("a,b\n\"x\"y,2\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "could not read the csv input: line 2: text after a closing quote"
        );
    }

    #[test]
    fn reports_invalid_utf8_with_its_line() {
        let err = read_table(&b"a\nok\nbad \xFF\n"[..], InputKind::Csv).unwrap_err();
        assert_eq!(
            err.to_string(),
            "could not read the csv input: line 3: invalid UTF-8"
        );
    }

    #[test]
    fn stops_at_the_byte_limit() {
        let limits = Limits {
            bytes: 8,
            cells: 100,
        };
        assert!(read_table_with_limits(&b"a,b\n1,2\n"[..], InputKind::Csv, limits).is_ok());
        let err = read_table_with_limits(&b"a,b\n1,2\n3\n"[..], InputKind::Csv, limits);
        assert!(
            matches!(err, Err(ConvertError::DelimitedTooLarge { limit: 8 })),
            "{err:?}"
        );
    }

    #[test]
    fn counts_short_rows_as_padded_to_the_widest() {
        let limits = Limits {
            bytes: 1000,
            cells: 20,
        };
        // 10 columns by 2 rows fits; one more short row would pad out to 30 cells.
        let wide = "a,b,c,d,e,f,g,h,i,j\n1\n";
        assert!(read_table_with_limits(wide.as_bytes(), InputKind::Csv, limits).is_ok());
        let err = read_table_with_limits(format!("{wide}2\n").as_bytes(), InputKind::Csv, limits);
        assert!(
            matches!(err, Err(ConvertError::DelimitedTooManyCells { limit: 20 })),
            "{err:?}"
        );
    }
}
