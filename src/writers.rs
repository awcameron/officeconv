//! Writing a [`Table`] out as CSV, TSV, JSON or Markdown.

use std::io::{self, Write};

use serde_json::{Map, Value};

use crate::cli::OutputFormat;
use crate::table::Table;

/// Writes `table` to `out` in the given format.
pub fn write_table<W: Write>(table: &Table, format: OutputFormat, out: W) -> io::Result<()> {
    match format {
        OutputFormat::Csv => write_delimited(table, b',', out),
        OutputFormat::Tsv => write_delimited(table, b'\t', out),
        OutputFormat::Json => write_json(table, out),
        OutputFormat::Markdown => write_markdown(table, out),
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
        writer.write_record(row)?;
    }
    writer.flush()
}

/// A JSON array with one object per row, keyed by header.
fn write_json<W: Write>(table: &Table, mut out: W) -> io::Result<()> {
    let keys = json_keys(&table.headers);
    let records: Vec<Value> = table
        .rows
        .iter()
        .map(|row| {
            let object: Map<String, Value> = keys
                .iter()
                .cloned()
                .zip(row.iter().map(|cell| Value::String(cell.clone())))
                .collect();
            Value::Object(object)
        })
        .collect();

    serde_json::to_writer_pretty(&mut out, &records)?;
    writeln!(out)
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

/// A GitHub-flavored Markdown table with padded columns.
pub fn write_markdown<W: Write>(table: &Table, mut out: W) -> io::Result<()> {
    if table.is_empty() {
        return Ok(());
    }

    let headers: Vec<String> = table
        .headers
        .iter()
        .map(|h| escape_markdown_cell(h))
        .collect();
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|c| escape_markdown_cell(c)).collect())
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
        let mut buffer = Vec::new();
        write_table(table, format, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
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
    fn empty_table_writes_nothing_or_empty_array() {
        let t = Table::default();
        assert_eq!(render(&t, OutputFormat::Csv), "");
        assert_eq!(render(&t, OutputFormat::Markdown), "");
        assert_eq!(render(&t, OutputFormat::Json), "[]\n");
    }
}
