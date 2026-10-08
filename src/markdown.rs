//! The Markdown both Markdown writers share: escaping text and link targets, and writing tables.
//!
//! [`crate::writers`] writes a spreadsheet's values with it, and [`crate::document::markdown`]
//! a document's formatted runs.

use std::io::{self, Write};

/// Writes a GitHub-flavored Markdown table with padded columns. The first row is the header.
///
/// Each cell is Markdown already, as its writer decided; a `"\n"` in one is a line break, which
/// a table cell can only hold as `<br>`. The table is as wide as its longest row, and shorter
/// rows are padded with empty cells. A table with no columns writes nothing.
pub fn write_table<W: Write>(rows: &[Vec<String>], mut out: W) -> io::Result<()> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Ok(());
    }

    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let mut cells: Vec<String> = row.iter().map(|cell| escape_cell(cell)).collect();
            cells.resize(columns, String::new());
            cells
        })
        .collect();

    // Each column is as wide as its widest cell, and at least 3 so `---` fits.
    let mut widths = vec![3; columns];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    write_row(&mut out, &rows[0], &widths)?;
    let rule: Vec<String> = widths.iter().map(|&w| "-".repeat(w)).collect();
    write_row(&mut out, &rule, &widths)?;
    for row in &rows[1..] {
        write_row(&mut out, row, &widths)?;
    }
    Ok(())
}

fn write_row<W: Write>(out: &mut W, cells: &[String], widths: &[usize]) -> io::Result<()> {
    let padded: Vec<String> = cells
        .iter()
        .zip(widths)
        .map(|(cell, &width)| format!("{cell:<width$}"))
        .collect();
    writeln!(out, "| {} |", padded.join(" | "))
}

/// Pipes would end the cell and newlines would end the row, so escape both.
fn escape_cell(cell: &str) -> String {
    cell.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

/// Escapes `text` so Markdown shows it as written, rather than as formatting or HTML.
///
/// `\\`, `*`, `_`, `` ` ``, `[` and `]` get a backslash. `<` becomes `&lt;` rather than `\\<`,
/// because Python-Markdown doesn't treat `\\<` as an escape. `&` becomes `&amp;` where it starts
/// something a renderer would decode, such as `&copy;` or `&#58;`. A lone `&`, as in `Q&A`, stays.
pub fn escape_text(text: &str) -> String {
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

/// Percent-encodes what would end a Markdown link target early or change its meaning:
/// whitespace (including line breaks), control characters, `<`, `>`, `"`, `(`, `)` and `\`.
/// `&` becomes `&amp;`.
///
/// Without this, a newline in a link from the document would end the link, and whatever
/// followed, such as raw HTML, would become part of the Markdown. Renderers also decode
/// `&#58;` in a link target to `:`, so `javascript&#58;` would become a `javascript:` link; with
/// `&amp;`, the browser sees the literal text `&#58;` instead.
pub fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        if c == '&' {
            out.push_str("&amp;");
        } else if c.is_whitespace()
            || c.is_control()
            || matches!(c, '<' | '>' | '"' | '(' | ')' | '\\')
        {
            let mut bytes = [0; 4];
            for byte in c.encode_utf8(&mut bytes).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> String {
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|row| row.iter().map(|c| c.to_string()).collect())
            .collect();
        let mut buffer = Vec::new();
        write_table(&rows, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn pads_columns_and_escapes_pipes() {
        assert_eq!(
            table(&[&["Region", "Units"], &["North|East", "12"], &["S", "7"]]),
            "\
| Region      | Units |
| ----------- | ----- |
| North\\|East | 12    |
| S           | 7     |
"
        );
    }

    #[test]
    fn writes_cells_as_the_markdown_they_are() {
        assert_eq!(
            table(&[&["<b>", "*"], &["&copy;", "a_b"]]),
            "\
| <b>    | *   |
| ------ | --- |
| &copy; | a_b |
"
        );
    }

    #[test]
    fn writes_line_breaks_as_br() {
        assert_eq!(
            table(&[&["Team"], &["Bobby\nDon\r\nRuth"]]),
            "\
| Team                 |
| -------------------- |
| Bobby<br>Don<br>Ruth |
"
        );
    }

    #[test]
    fn pads_short_rows_and_widens_the_header() {
        assert_eq!(
            table(&[&["a"], &["1", "2"], &[]]),
            "\
| a   |     |
| --- | --- |
| 1   | 2   |
|     |     |
"
        );
    }

    #[test]
    fn a_table_without_columns_writes_nothing() {
        assert_eq!(table(&[]), "");
        assert_eq!(table(&[&[], &[]]), "");
    }

    #[test]
    fn escapes_text_so_markdown_shows_it_literally() {
        assert_eq!(escape_text(r"\ * _ ` [ ]"), r"\\ \* \_ \` \[ \]");
        assert_eq!(escape_text("<img src=x>"), "&lt;img src=x>");
        assert_eq!(
            escape_text("&copy; &#58; &#x3A; Q&A R & D &; &#;"),
            "&amp;copy; &amp;#58; &amp;#x3A; Q&A R & D &; &#;"
        );
    }

    #[test]
    fn escapes_ampersands_in_link_targets() {
        // Renderers decode `&#58;` in a link target to `:`, so an unescaped `&` could turn
        // `javascript&#58;` into `javascript:`. `&amp;` decodes back to a plain `&`.
        assert_eq!(
            escape_url("javascript&#58;alert"),
            "javascript&amp;#58;alert"
        );
        assert_eq!(
            escape_url("https://x.com/?a=1&b=2"),
            "https://x.com/?a=1&amp;b=2"
        );
    }

    #[test]
    fn escapes_link_targets_that_would_end_the_link() {
        assert_eq!(
            escape_url("https://x.com/a b\n<i>\u{2028}(\"\\)"),
            "https://x.com/a%20b%0A%3Ci%3E%E2%80%A8%28%22%5C%29"
        );
    }
}
