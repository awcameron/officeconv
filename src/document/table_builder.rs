//! Collects a table's cells as [`super::builder::BlockBuilder`] walks through its rows, then
//! works out which cells each merged cell covers.
//!
//! The spans come from an untrusted file, so a merged cell only ever covers cells the file
//! marks as merged: no span passes the table or overlaps another cell (ADR 0004).

use std::mem;

use super::{CellRuns, ImagePart, Run, TableCell, append_paragraph, is_blank};

/// How a file writes the cells a merged cell covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergedCells {
    /// Every covered cell is in the file, marked as part of a neighbor, as PowerPoint does with
    /// `hMerge` and `vMerge`.
    Listed,
    /// The columns a `gridSpan` covers are left out, as Word does, so
    /// [`end_cell`](TableBuilder::end_cell) adds them.
    Omitted,
}

/// How a table cell is marked as part of a merged cell that starts elsewhere.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Merged {
    #[default]
    No,
    /// Part of the cell to its left: PowerPoint's `hMerge`.
    Left,
    /// Part of the cell above: PowerPoint's `vMerge`, or Word's `<w:vMerge/>` without
    /// `restart`.
    Up,
    /// Part of a cell above and to the left: both `hMerge` and `vMerge`.
    Both,
}

/// Collects one table's cells. The reader describes the open cell with
/// [`span_columns`](Self::span_columns), [`span_rows`](Self::span_rows) and
/// [`merge`](Self::merge) before it ends; each resets when the cell ends.
#[derive(Debug)]
pub struct TableBuilder {
    merged_cells: MergedCells,
    rows: Vec<Vec<Slot>>,
    row: Vec<Slot>,
    /// The cell being filled in.
    cell: CellRuns<ImagePart>,
    /// How many columns the table's grid declares. A Word cell never spans past them.
    columns: usize,
    /// How many columns the open cell says it spans.
    span: usize,
    /// How many rows the open cell says it spans.
    row_span: usize,
    /// Whether the open cell is marked as part of a neighbor.
    merged: Merged,
}

/// One position in the grid as the file marks it, before [`TableBuilder::finish`] works out
/// which cell covers what.
#[derive(Debug)]
enum Slot {
    Cell {
        runs: CellRuns<ImagePart>,
        cols: usize,
        rows: usize,
    },
    Merged(Merged),
}

impl TableBuilder {
    pub fn new(merged_cells: MergedCells) -> Self {
        TableBuilder {
            merged_cells,
            rows: Vec::new(),
            row: Vec::new(),
            cell: CellRuns::new(),
            columns: 0,
            span: 1,
            row_span: 1,
            merged: Merged::No,
        }
    }

    /// Adds a column to the table's grid (`<w:gridCol>`).
    pub fn add_grid_column(&mut self) {
        self.columns += 1;
    }

    /// The open cell spans `cols` columns (`gridSpan`).
    pub fn span_columns(&mut self, cols: usize) {
        self.span = cols;
    }

    /// The open cell spans `rows` rows (PowerPoint's `rowSpan`). Only a table that lists its
    /// merged cells says; a Word cell spans as many rows as continue it.
    pub fn span_rows(&mut self, rows: usize) {
        self.row_span = rows;
    }

    /// The open cell is part of a merged cell that starts above or to its left.
    pub fn merge(&mut self, merged: Merged) {
        self.merged = merged;
    }

    /// Adds a paragraph to the open cell.
    pub fn append_paragraph(&mut self, runs: Vec<Run<ImagePart>>) {
        append_paragraph(&mut self.cell, runs);
    }

    /// Moves the finished cell into the current row. In a Word table, a cell spanning several
    /// columns is followed by cells marking the columns it covers.
    pub fn end_cell(&mut self) {
        let runs = mem::take(&mut self.cell);
        let span = mem::replace(&mut self.span, 1).max(1);
        let rows = mem::replace(&mut self.row_span, 1).max(1);
        let lists_merged_cells = self.merged_cells == MergedCells::Listed;
        let merged = match mem::take(&mut self.merged) {
            // Word shows the text of a cell that continues a merge, so keep it as a cell of its
            // own rather than lose it.
            Merged::Up if !lists_merged_cells && !is_blank(&runs) => Merged::No,
            merged => merged,
        };
        self.row.push(match merged {
            Merged::No => Slot::Cell {
                runs,
                cols: span,
                rows: if lists_merged_cells { rows } else { usize::MAX },
            },
            merged => Slot::Merged(merged),
        });
        if !lists_merged_cells {
            // Bounded by the grid, which costs the file bytes for every column, so a huge
            // `gridSpan` value can't make a huge row.
            let room = self.columns.saturating_sub(self.row.len());
            let extra = (span - 1).min(room);
            self.row
                .extend((0..extra).map(|_| Slot::Merged(Merged::Left)));
        }
    }

    /// Moves the finished row into the table.
    pub fn end_row(&mut self) {
        let row = mem::take(&mut self.row);
        self.rows.push(row);
    }

    /// The finished table: each cell spans as far as the cells marked as part of it go, up to
    /// the span it declares.
    ///
    /// Merges only ever claim cells the file marks as merged, so no span passes the table or
    /// overlaps another cell, and a merged mark nothing claims becomes an empty cell. It's one
    /// pass: each cell is claimed at most once, and a failed claim stops at the first cell
    /// that isn't marked.
    pub fn finish(self) -> Vec<Vec<TableCell<ImagePart>>> {
        let slots = self.rows;
        let mut claimed: Vec<Vec<bool>> = slots.iter().map(|row| vec![false; row.len()]).collect();
        let mut spans = Vec::new();
        for (r, row) in slots.iter().enumerate() {
            for (c, slot) in row.iter().enumerate() {
                let &Slot::Cell { cols, rows, .. } = slot else {
                    continue;
                };
                let width = 1 + row[c + 1..]
                    .iter()
                    .zip(&claimed[r][c + 1..])
                    .take(cols - 1)
                    .take_while(|&(slot, &claimed)| {
                        matches!(slot, Slot::Merged(Merged::Left)) && !claimed
                    })
                    .count();
                claimed[r][c + 1..c + width].fill(true);

                let mut height = 1;
                while height < rows {
                    let below = r + height;
                    let Some(next) = slots.get(below) else {
                        break;
                    };
                    let covers = next.len() >= c + width
                        && matches!(next[c], Slot::Merged(Merged::Up | Merged::Both))
                        && next[c + 1..c + width]
                            .iter()
                            .all(|slot| matches!(slot, Slot::Merged(_)))
                        && claimed[below][c..c + width].iter().all(|&claimed| !claimed);
                    if !covers {
                        break;
                    }
                    claimed[below][c..c + width].fill(true);
                    height += 1;
                }
                spans.push((width, height));
            }
        }

        let mut spans = spans.into_iter();
        slots
            .into_iter()
            .zip(claimed)
            .map(|(row, claimed)| {
                row.into_iter()
                    .zip(claimed)
                    .map(|(slot, claimed)| match slot {
                        Slot::Cell { runs, .. } => {
                            let (cols, rows) = spans.next().expect("a span for every cell");
                            TableCell::Content { runs, cols, rows }
                        }
                        Slot::Merged(_) if claimed => TableCell::Covered,
                        Slot::Merged(_) => TableCell::new(CellRuns::new()),
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RunStyle;

    use Merged::{Both, Left, Up};

    /// Ends a cell holding `text`, spanning `cols` columns and `rows` rows.
    fn cell(table: &mut TableBuilder, text: &str, cols: usize, rows: usize) {
        table.append_paragraph(vec![Run::new(text, RunStyle::default())]);
        table.span_columns(cols);
        table.span_rows(rows);
        table.end_cell();
    }

    /// Ends a cell marked as part of a merged cell that starts elsewhere.
    fn mark(table: &mut TableBuilder, merged: Merged) {
        table.merge(merged);
        table.end_cell();
    }

    fn content(text: &str, cols: usize, rows: usize) -> TableCell<ImagePart> {
        TableCell::Content {
            runs: vec![Run::new(text, RunStyle::default())],
            cols,
            rows,
        }
    }

    fn empty() -> TableCell<ImagePart> {
        TableCell::new(CellRuns::new())
    }

    /// A Word table with `columns` grid columns.
    fn word(columns: usize) -> TableBuilder {
        let mut table = TableBuilder::new(MergedCells::Omitted);
        for _ in 0..columns {
            table.add_grid_column();
        }
        table
    }

    #[test]
    fn a_merged_cell_covers_the_cells_marked_as_part_of_it() {
        let mut slides = TableBuilder::new(MergedCells::Listed);
        cell(&mut slides, "A", 2, 2);
        mark(&mut slides, Left);
        cell(&mut slides, "B", 1, 1);
        slides.end_row();
        mark(&mut slides, Up);
        mark(&mut slides, Both);
        cell(&mut slides, "C", 1, 1);
        slides.end_row();

        use TableCell::Covered;
        assert_eq!(
            slides.finish(),
            [
                vec![content("A", 2, 2), Covered, content("B", 1, 1)],
                vec![Covered, Covered, content("C", 1, 1)],
            ]
        );

        // Word leaves out the column a `gridSpan` covers, and a cell spans down as far as
        // `<w:vMerge/>` continues it.
        let mut doc = word(3);
        cell(&mut doc, "A", 2, 1);
        cell(&mut doc, "B", 1, 1);
        doc.end_row();
        doc.span_columns(2);
        mark(&mut doc, Up);
        cell(&mut doc, "C", 1, 1);
        doc.end_row();
        doc.span_columns(2);
        mark(&mut doc, Up);
        cell(&mut doc, "D", 1, 1);
        doc.end_row();

        assert_eq!(
            doc.finish(),
            [
                vec![content("A", 2, 3), Covered, content("B", 1, 1)],
                vec![Covered, Covered, content("C", 1, 1)],
                vec![Covered, Covered, content("D", 1, 1)],
            ]
        );
    }

    #[test]
    fn a_span_stops_at_another_cell() {
        // A says it's three columns wide, but B starts in its second column, so A keeps only
        // its own column. A mark no cell reaches is left empty.
        let mut table = TableBuilder::new(MergedCells::Listed);
        cell(&mut table, "A", 3, 2);
        cell(&mut table, "B", 2, 2);
        mark(&mut table, Left);
        table.end_row();
        mark(&mut table, Up);
        mark(&mut table, Up);
        mark(&mut table, Left);
        mark(&mut table, Up);
        table.end_row();

        use TableCell::Covered;
        assert_eq!(
            table.finish(),
            [
                vec![content("A", 1, 2), content("B", 2, 2), Covered],
                vec![Covered, Covered, Covered, empty()],
            ]
        );
    }

    #[test]
    fn a_span_past_the_table_stops_at_its_edge() {
        let mut slides = TableBuilder::new(MergedCells::Listed);
        cell(&mut slides, "A", 5, 5);
        mark(&mut slides, Left);
        slides.end_row();
        mark(&mut slides, Up);
        mark(&mut slides, Both);
        slides.end_row();

        use TableCell::Covered;
        assert_eq!(
            slides.finish(),
            [vec![content("A", 2, 2), Covered], vec![Covered, Covered]]
        );

        // Word adds the columns a cell covers only as far as the grid goes.
        let mut doc = word(2);
        cell(&mut doc, "A", 5, 1);
        doc.end_row();
        assert_eq!(doc.finish(), [vec![content("A", 2, 1), Covered]]);
    }

    #[test]
    fn a_continuation_with_nothing_to_continue_is_an_empty_cell() {
        let mut slides = TableBuilder::new(MergedCells::Listed);
        mark(&mut slides, Up);
        mark(&mut slides, Both);
        slides.end_row();
        mark(&mut slides, Left);
        cell(&mut slides, "A", 1, 1);
        slides.end_row();

        assert_eq!(
            slides.finish(),
            [vec![empty(), empty()], vec![empty(), content("A", 1, 1)]]
        );

        // Word shows the text of a cell that continues a merge, so it stays a cell of its own.
        let mut doc = word(2);
        mark(&mut doc, Up);
        doc.append_paragraph(vec![Run::new("B", RunStyle::default())]);
        mark(&mut doc, Up);
        doc.end_row();
        assert_eq!(doc.finish(), [vec![empty(), content("B", 1, 1)]]);
    }

    #[test]
    fn a_huge_span_stays_inside_the_table() {
        let mut slides = TableBuilder::new(MergedCells::Listed);
        cell(&mut slides, "A", usize::MAX, usize::MAX);
        mark(&mut slides, Left);
        slides.end_row();
        mark(&mut slides, Up);
        mark(&mut slides, Both);
        slides.end_row();
        cell(&mut slides, "B", 0, 0);
        slides.end_row();

        use TableCell::Covered;
        assert_eq!(
            slides.finish(),
            [
                vec![content("A", 2, 2), Covered],
                vec![Covered, Covered],
                vec![content("B", 1, 1)],
            ]
        );

        // A Word row grows only to the grid's columns, however many the cell says it spans.
        let mut doc = word(3);
        cell(&mut doc, "A", usize::MAX, 1);
        doc.end_row();
        doc.span_columns(usize::MAX);
        mark(&mut doc, Up);
        doc.end_row();
        assert_eq!(
            doc.finish(),
            [
                vec![content("A", 3, 2), Covered, Covered],
                vec![Covered, Covered, Covered],
            ]
        );
    }
}
