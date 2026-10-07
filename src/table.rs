//! A grid of cells: the shared model every writer works from.

use std::fmt;

/// One cell's value, keeping the type the spreadsheet stored.
///
/// Dates, durations and error cells are already formatted as [`Cell::Text`]:
/// JSON has no types for them, so there's nothing more to keep.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Cell {
    #[default]
    Empty,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl fmt::Display for Cell {
    /// The text written to CSV, TSV, Markdown and untyped JSON.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Empty => Ok(()),
            Cell::Text(text) => f.write_str(text),
            Cell::Int(i) => write!(f, "{i}"),
            // Rust prints whole floats without a fraction: 3.0 -> "3", 2.5 -> "2.5".
            Cell::Float(x) => write!(f, "{x}"),
            Cell::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Cell::Text(text)
    }
}

/// A table of cells.
///
/// The first row of a sheet becomes `headers`, as text; the rest become `rows`.
/// Every row has the same number of cells as `headers`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    /// Builds a table from rows of cells, treating the first row as headers.
    ///
    /// The table is as wide as its longest row, headers included. Shorter rows are padded with
    /// empty cells, and so are the headers, so no row loses cells.
    pub fn from_cells(rows: Vec<Vec<Cell>>) -> Self {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut rows = rows.into_iter();
        let Some(headers) = rows.next() else {
            return Table::default();
        };

        let mut headers: Vec<String> = headers.iter().map(Cell::to_string).collect();
        headers.resize(width, String::new());
        let rows = rows
            .map(|mut row| {
                row.resize(width, Cell::Empty);
                row
            })
            .collect();

        Table { headers, rows }
    }

    /// Like [`Table::from_cells`], for rows that are all text.
    pub fn from_rows(rows: Vec<Vec<String>>) -> Self {
        Table::from_cells(
            rows.into_iter()
                .map(|row| row.into_iter().map(Cell::from).collect())
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn first_row_becomes_headers() {
        let table = Table::from_rows(vec![strings(&["a", "b"]), strings(&["1", "2"])]);
        assert_eq!(table.headers, strings(&["a", "b"]));
        assert_eq!(
            table.rows,
            vec![vec![Cell::Text("1".into()), Cell::Text("2".into())]]
        );
    }

    #[test]
    fn pads_short_rows() {
        let table = Table::from_rows(vec![strings(&["a", "b", "c"]), strings(&["1"])]);
        assert_eq!(
            table.rows,
            vec![vec![Cell::Text("1".into()), Cell::Empty, Cell::Empty]]
        );
    }

    #[test]
    fn widens_to_the_longest_row_instead_of_cutting_it() {
        let table = Table::from_rows(vec![strings(&["title"]), strings(&["1", "2", "3"])]);
        assert_eq!(table.headers, strings(&["title", "", ""]));
        assert_eq!(
            table.rows,
            vec![vec![
                Cell::Text("1".into()),
                Cell::Text("2".into()),
                Cell::Text("3".into())
            ]]
        );
    }

    #[test]
    fn typed_header_cells_become_text() {
        let table = Table::from_cells(vec![vec![Cell::Int(2026), Cell::Bool(true), Cell::Empty]]);
        assert_eq!(table.headers, ["2026", "true", ""]);
    }

    #[test]
    fn cells_print_as_before() {
        let printed: Vec<String> = [
            Cell::Empty,
            Cell::Text("hi".into()),
            Cell::Int(42),
            Cell::Float(3.0),
            Cell::Float(2.5),
            Cell::Bool(false),
        ]
        .iter()
        .map(Cell::to_string)
        .collect();
        assert_eq!(printed, ["", "hi", "42", "3", "2.5", "false"]);
    }

    #[test]
    fn no_rows_is_empty() {
        assert!(Table::from_rows(vec![]).is_empty());
    }
}
