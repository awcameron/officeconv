//! A plain grid of text: the shared model every writer works from.

/// A table where every cell is already text.
///
/// The first row of a sheet becomes `headers`; the rest become `rows`.
/// Every row has the same number of cells as `headers`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Builds a table from rows of cells, treating the first row as headers.
    ///
    /// Short rows are padded with empty strings so the table is rectangular.
    pub fn from_rows(rows: Vec<Vec<String>>) -> Self {
        let mut rows = rows.into_iter();
        let Some(headers) = rows.next() else {
            return Table::default();
        };

        let width = headers.len();
        let rows = rows
            .map(|mut row| {
                row.resize(width, String::new());
                row
            })
            .collect();

        Table { headers, rows }
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
        assert_eq!(table.rows, vec![strings(&["1", "2"])]);
    }

    #[test]
    fn pads_short_rows() {
        let table = Table::from_rows(vec![strings(&["a", "b", "c"]), strings(&["1"])]);
        assert_eq!(table.rows, vec![strings(&["1", "", ""])]);
    }

    #[test]
    fn no_rows_is_empty() {
        assert!(Table::from_rows(vec![]).is_empty());
    }
}
