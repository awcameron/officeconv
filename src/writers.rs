//! Writing a [`Table`] out as CSV, TSV, JSON or Markdown.

use std::io::{self, Write};

use serde_json::{Map, Number, Value};

use crate::format::OutputFormat;
use crate::table::{Cell, Table};

/// How JSON writes cell values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JsonValues {
    /// Every value is a string, as the cell's text: `"12"`, `"true"`, `""`.
    #[default]
    Text,
    /// Numbers, booleans and empty cells keep their type: `12`, `true`, `null`.
    Typed,
}

/// Writes `table` to `out` in the given format. `json` only matters for JSON output.
pub fn write_table<W: Write>(
    table: &Table,
    format: OutputFormat,
    json: JsonValues,
    out: W,
) -> io::Result<()> {
    match format {
        OutputFormat::Csv => write_delimited(table, b',', out),
        OutputFormat::Tsv => write_delimited(table, b'\t', out),
        OutputFormat::Json => write_json(table, json, out),
        OutputFormat::Markdown => write_markdown(table, MarkdownCells::Text, out),
        OutputFormat::Pdf => unreachable!("InputKind::check_output rejects sheets as PDF"),
    }
}

/// CSV or TSV, depending on `delimiter`. Quoting is handled by the `csv` crate.
fn write_delimited<W: Write>(table: &Table, delimiter: u8, out: W) -> io::Result<()> {
    if table.is_empty() {
        return Ok(());
    }

    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(out);
    writer.write_record(&table.headers)?;
    for row in &table.rows {
        writer.write_record(row.iter().map(Cell::to_string))?;
    }
    writer.flush()
}

/// A JSON array with one object per row, keyed by header.
fn write_json<W: Write>(table: &Table, values: JsonValues, mut out: W) -> io::Result<()> {
    let keys = json_keys(&table.headers);
    let records: Vec<Value> = table
        .rows
        .iter()
        .map(|row| {
            let object: Map<String, Value> = keys
                .iter()
                .cloned()
                .zip(row.iter().map(|cell| json_value(cell, values)))
                .collect();
            Value::Object(object)
        })
        .collect();

    serde_json::to_writer_pretty(&mut out, &records)?;
    writeln!(out)
}

/// One cell as a JSON value.
fn json_value(cell: &Cell, values: JsonValues) -> Value {
    if values == JsonValues::Text {
        return Value::String(cell.to_string());
    }

    match cell {
        Cell::Empty => Value::Null,
        Cell::Text(text) => Value::String(text.clone()),
        Cell::Int(i) => Value::from(*i),
        Cell::Float(x) => float_value(*x),
        Cell::Bool(b) => Value::Bool(*b),
    }
}

/// Excel stores every number as a float, so `12` arrives as `12.0`; write it as `12`.
///
/// Whole numbers from 2^53 up stay floats: past that, not every integer can be stored exactly,
/// and JavaScript (where most JSON ends up) can't tell neighboring ones apart.
fn float_value(x: f64) -> Value {
    const EXACT_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0; // 2^53

    if x.fract() == 0.0 && x.abs() < EXACT_INTEGER_LIMIT {
        Value::from(x as i64)
    } else {
        // Excel can't store NaN or infinity, but JSON can't either, so fall back to null.
        Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// Makes headers usable as JSON keys: blanks get a column name, repeats get a suffix.
fn json_keys(headers: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(headers.len());
    for (i, header) in headers.iter().enumerate() {
        let base = if header.trim().is_empty() {
            format!("column_{}", i + 1)
        } else {
            header.clone()
        };

        let mut key = base.clone();
        let mut n = 2;
        while keys.contains(&key) {
            key = format!("{base}_{n}");
            n += 1;
        }
        keys.push(key);
    }
    keys
}

/// What the cells given to [`write_markdown`] hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownCells {
    /// Plain text, such as a spreadsheet's values: escaped so it shows as written.
    Text,
    /// Markdown already, such as a document's formatted runs: written as it is.
    Markdown,
}

/// A GitHub-flavored Markdown table with padded columns.
pub fn write_markdown<W: Write>(table: &Table, cells: MarkdownCells, mut out: W) -> io::Result<()> {
    if table.is_empty() {
        return Ok(());
    }

    let cell = |text: &str| match cells {
        MarkdownCells::Text => escape_markdown_cell(&escape_markdown_text(text)),
        MarkdownCells::Markdown => escape_markdown_cell(text),
    };
    let headers: Vec<String> = table.headers.iter().map(|h| cell(h)).collect();
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|c| cell(&c.to_string())).collect())
        .collect();

    // Each column is as wide as its widest cell, and at least 3 so `---` fits.
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count().max(3)).collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    write_markdown_row(&mut out, &headers, &widths)?;
    let rule: Vec<String> = widths.iter().map(|&w| "-".repeat(w)).collect();
    write_markdown_row(&mut out, &rule, &widths)?;
    for row in &rows {
        write_markdown_row(&mut out, row, &widths)?;
    }
    Ok(())
}

fn write_markdown_row<W: Write>(out: &mut W, cells: &[String], widths: &[usize]) -> io::Result<()> {
    let padded: Vec<String> = cells
        .iter()
        .zip(widths)
        .map(|(cell, &width)| format!("{cell:<width$}"))
        .collect();
    writeln!(out, "| {} |", padded.join(" | "))
}

/// Escapes `text` so Markdown shows it as written, rather than as formatting or HTML.
///
/// `\\`, `*`, `_`, `` ` ``, `[` and `]` get a backslash. `<` becomes `&lt;` rather than `\\<`,
/// because Python-Markdown doesn't treat `\\<` as an escape. `&` becomes `&amp;` where it starts
/// something a renderer would decode, such as `&copy;` or `&#58;`. A lone `&`, as in `Q&A`, stays.
pub fn escape_markdown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            '\\' | '*' | '_' | '`' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '&' if starts_entity(&text[i + 1..]) => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// True if `rest`, the text after an `&`, would make it an entity: a name, or `#` and a number,
/// then `;`.
fn starts_entity(rest: &str) -> bool {
    let body = rest.strip_prefix('#').unwrap_or(rest);
    let len = body.chars().take_while(char::is_ascii_alphanumeric).count();
    len > 0 && body[len..].starts_with(';')
}

/// Pipes would end the cell and newlines would end the row, so escape both.
fn escape_markdown_cell(cell: &str) -> String {
    cell.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        Table::from_rows(
            rows.iter()
                .map(|row| row.iter().map(|c| c.to_string()).collect())
                .collect(),
        )
    }

    fn render(table: &Table, format: OutputFormat) -> String {
        render_with(table, format, JsonValues::Text)
    }

    fn render_with(table: &Table, format: OutputFormat, json: JsonValues) -> String {
        let mut buffer = Vec::new();
        write_table(table, format, json, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    /// A header row, then one row of each kind of cell.
    fn typed_table() -> Table {
        Table::from_cells(vec![
            ["text", "int", "float", "whole", "bool", "empty"]
                .map(|h| Cell::Text(h.into()))
                .to_vec(),
            vec![
                Cell::Text("00123".into()),
                Cell::Int(7),
                Cell::Float(7.5),
                Cell::Float(12.0),
                Cell::Bool(true),
                Cell::Empty,
            ],
        ])
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        let t = table(&[&["name", "note"], &["Ada", "says \"hi\", twice"]]);
        assert_eq!(
            render(&t, OutputFormat::Csv),
            "name,note\nAda,\"says \"\"hi\"\", twice\"\n"
        );
    }

    #[test]
    fn tsv_uses_tabs() {
        let t = table(&[&["a", "b"], &["1", "2"]]);
        assert_eq!(render(&t, OutputFormat::Tsv), "a\tb\n1\t2\n");
    }

    #[test]
    fn json_keeps_header_order() {
        let t = table(&[&["z", "a"], &["1", "2"]]);
        assert_eq!(
            render(&t, OutputFormat::Json),
            "[\n  {\n    \"z\": \"1\",\n    \"a\": \"2\"\n  }\n]\n"
        );
    }

    #[test]
    fn typed_json_keeps_cell_types() {
        let json: Value = serde_json::from_str(&render_with(
            &typed_table(),
            OutputFormat::Json,
            JsonValues::Typed,
        ))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!([{
                "text": "00123", "int": 7, "float": 7.5, "whole": 12, "bool": true, "empty": null
            }])
        );
    }

    #[test]
    fn untyped_json_writes_cell_text() {
        let json: Value =
            serde_json::from_str(&render(&typed_table(), OutputFormat::Json)).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{
                "text": "00123", "int": "7", "float": "7.5", "whole": "12", "bool": "true", "empty": ""
            }])
        );
    }

    #[test]
    fn huge_whole_numbers_stay_floats() {
        assert_eq!(
            float_value(9_007_199_254_740_991.0),
            Value::from(9_007_199_254_740_991_i64)
        );
        assert!(float_value(9_007_199_254_740_992.0).is_f64());
        assert_eq!(float_value(-3.0), Value::from(-3));
        assert_eq!(float_value(f64::NAN), Value::Null);
    }

    #[test]
    fn other_formats_ignore_typed() {
        for format in [OutputFormat::Csv, OutputFormat::Tsv, OutputFormat::Markdown] {
            assert_eq!(
                render_with(&typed_table(), format, JsonValues::Typed),
                render(&typed_table(), format)
            );
        }
    }

    #[test]
    fn json_keys_fill_blanks_and_dedupe() {
        let headers = ["id", "", "id", "id"].map(String::from);
        assert_eq!(json_keys(&headers), ["id", "column_2", "id_2", "id_3"]);
    }

    #[test]
    fn markdown_pads_and_escapes() {
        let t = table(&[&["Region", "Units"], &["North|East", "12"], &["S", "7"]]);
        assert_eq!(
            render(&t, OutputFormat::Markdown),
            "\
| Region      | Units |
| ----------- | ----- |
| North\\|East | 12    |
| S           | 7     |
"
        );
    }

    #[test]
    fn markdown_escapes_text_cells_but_not_markdown_ones() {
        let t = table(&[&["<b>", "*"], &["&copy;", "a_b"]]);
        let render = |cells| {
            let mut buffer = Vec::new();
            write_markdown(&t, cells, &mut buffer).unwrap();
            String::from_utf8(buffer).unwrap()
        };
        assert_eq!(
            render(MarkdownCells::Text),
            "\
| &lt;b>     | \\*   |
| ---------- | ---- |
| &amp;copy; | a\\_b |
"
        );
        assert_eq!(
            render(MarkdownCells::Markdown),
            "\
| <b>    | *   |
| ------ | --- |
| &copy; | a_b |
"
        );
    }

    #[test]
    fn escapes_text_so_markdown_shows_it_literally() {
        assert_eq!(escape_markdown_text(r"\ * _ ` [ ]"), r"\\ \* \_ \` \[ \]");
        assert_eq!(escape_markdown_text("<img src=x>"), "&lt;img src=x>");
        assert_eq!(
            escape_markdown_text("&copy; &#58; &#x3A; Q&A R & D &; &#;"),
            "&amp;copy; &amp;#58; &amp;#x3A; Q&A R & D &; &#;"
        );
    }

    #[test]
    fn empty_table_writes_nothing_or_empty_array() {
        let t = Table::default();
        assert_eq!(render(&t, OutputFormat::Csv), "");
        assert_eq!(render(&t, OutputFormat::Markdown), "");
        assert_eq!(render(&t, OutputFormat::Json), "[]\n");
    }
}
