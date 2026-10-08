//! Builds [`Block`]s as a reader walks a document's XML.
//!
//! The DOCX and PPTX readers decide what each element means; the builder keeps what's open
//! (paragraphs, tables, the current run's formatting and link) and where finished content
//! goes: a paragraph inside a table becomes part of the open cell, and a finished table
//! becomes a block.

use super::{Block, Run, RunStyle, TableBuilder, append_paragraph, append_run, is_blank};

/// Collects blocks for a reader. `P` is what the reader records about each paragraph, such as
/// its style or list level; the builder only carries it.
pub struct BlockBuilder<P> {
    blocks: Vec<Block>,
    /// Paragraphs we're inside. Usually 0 or 1, but Word's text boxes nest paragraphs.
    paragraphs: Vec<Paragraph<P>>,
    /// Tables we're inside. More than one means a table inside a table cell.
    tables: Vec<TableBuilder>,
    nested_tables: NestedTables,
    /// Formatting of the run we're inside. Readers reset it where their format starts a run.
    pub style: RunStyle,
    /// Target of the link the text we're reading is in, if any.
    pub link: Option<String>,
}

/// A finished paragraph that isn't in a table, for the reader to turn into a block.
pub struct Paragraph<P> {
    pub props: P,
    pub runs: Vec<Run>,
}

/// What happens to a table inside a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedTables {
    /// Markdown tables can't nest, so the inner table's text goes into the outer cell.
    IntoOuterCell,
    /// The inner table becomes a block of its own, before the outer one.
    OwnBlock,
}

impl<P: Default> BlockBuilder<P> {
    pub fn new(nested_tables: NestedTables) -> Self {
        BlockBuilder {
            blocks: Vec::new(),
            paragraphs: Vec::new(),
            tables: Vec::new(),
            nested_tables,
            style: RunStyle::default(),
            link: None,
        }
    }

    pub fn start_paragraph(&mut self) {
        self.paragraphs.push(Paragraph {
            props: P::default(),
            runs: Vec::new(),
        });
    }

    /// What the reader has recorded about the innermost open paragraph.
    pub fn paragraph(&mut self) -> Option<&mut P> {
        self.paragraphs.last_mut().map(|p| &mut p.props)
    }

    /// Adds text to the innermost paragraph with the current style and link, extending the
    /// last run if they match. Text outside a paragraph is dropped.
    pub fn text(&mut self, text: &str) {
        let Some(paragraph) = self.paragraphs.last_mut() else {
            return;
        };
        let mut run = Run::new(text, self.style);
        run.link = self.link.clone();
        append_run(&mut paragraph.runs, run);
    }

    /// Adds an image to the innermost paragraph, linked to the current link.
    pub fn image(&mut self, part: String, alt: String, size: Option<(u32, u32)>) {
        let run = image_run(part, alt, size, self.link.clone());
        if let Some(paragraph) = self.paragraphs.last_mut() {
            append_run(&mut paragraph.runs, run);
        }
    }

    /// Closes the innermost paragraph. Returns it unless it's blank or went into the open
    /// table cell.
    pub fn end_paragraph(&mut self) -> Option<Paragraph<P>> {
        let paragraph = self.paragraphs.pop()?;
        if is_blank(&paragraph.runs) {
            return None;
        }
        if let Some(table) = self.tables.last_mut() {
            append_paragraph(&mut table.cell, paragraph.runs);
            return None;
        }
        Some(paragraph)
    }

    /// Drops the innermost open paragraph and everything in it.
    pub fn discard_paragraph(&mut self) {
        self.paragraphs.pop();
    }

    pub fn start_table(&mut self) {
        self.tables.push(TableBuilder::default());
    }

    /// The innermost open table, for the reader to record its grid and merged cells.
    pub fn table(&mut self) -> Option<&mut TableBuilder> {
        self.tables.last_mut()
    }

    pub fn end_cell(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.end_cell();
        }
    }

    pub fn end_row(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.end_row();
        }
    }

    /// Closes the innermost table, adding it as a block unless it has no rows.
    pub fn end_table(&mut self) {
        let Some(table) = self.tables.pop() else {
            return;
        };
        if table.rows.is_empty() {
            return;
        }
        if self.nested_tables == NestedTables::IntoOuterCell
            && let Some(outer) = self.tables.last_mut()
        {
            for cell in table.rows.into_iter().flatten() {
                append_paragraph(&mut outer.cell, cell);
            }
            return;
        }
        self.blocks.push(Block::Table(table.rows));
    }

    pub fn push(&mut self, block: Block) {
        self.blocks.push(block);
    }

    pub fn into_blocks(self) -> Vec<Block> {
        self.blocks
    }
}

/// An image run for the image in package part `part`, shown at `size` if the document gives
/// one.
pub fn image_run(part: String, alt: String, size: Option<(u32, u32)>, link: Option<String>) -> Run {
    let mut run = Run::image(part, alt);
    if let Some(image) = run.image.as_mut() {
        image.size = size;
    }
    run.link = link;
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOLD: RunStyle = RunStyle {
        bold: true,
        italic: false,
    };

    /// Plain runs holding `text`: a paragraph's, or a table cell's.
    fn cell(text: &str) -> Vec<Run> {
        vec![Run::new(text, RunStyle::default())]
    }

    fn paragraph(builder: &mut BlockBuilder<()>, text: &str) -> Option<Vec<Run>> {
        builder.start_paragraph();
        builder.text(text);
        builder.end_paragraph().map(|p| p.runs)
    }

    /// A one-row table with a cell per entry, each holding that text.
    fn table(builder: &mut BlockBuilder<()>, cells: &[&str]) {
        builder.start_table();
        for text in cells {
            paragraph(builder, text);
            builder.end_cell();
        }
        builder.end_row();
        builder.end_table();
    }

    #[test]
    fn merges_text_with_the_same_style_and_link() {
        let mut builder = BlockBuilder::<()>::new(NestedTables::OwnBlock);
        builder.start_paragraph();
        builder.text("Hel");
        builder.text("lo ");
        builder.style = BOLD;
        builder.text("world");
        builder.style = RunStyle::default();
        builder.link = Some("https://example.com".into());
        builder.text("!");

        let runs = builder.end_paragraph().unwrap().runs;
        assert_eq!(
            runs,
            [
                Run::new("Hello ", RunStyle::default()),
                Run::new("world", BOLD),
                Run::new("!", RunStyle::default()).linked("https://example.com"),
            ]
        );
    }

    #[test]
    fn keeps_props_and_drops_blank_paragraphs_and_stray_text() {
        let mut builder = BlockBuilder::<u8>::new(NestedTables::OwnBlock);
        builder.text("outside any paragraph");
        builder.start_paragraph();
        *builder.paragraph().unwrap() = 2;
        builder.text("Item");
        let item = builder.end_paragraph().unwrap();
        assert_eq!((item.props, item.runs), (2, cell("Item")));

        builder.start_paragraph();
        builder.text("  ");
        assert!(builder.end_paragraph().is_none());
        assert!(builder.end_paragraph().is_none());
        assert!(builder.into_blocks().is_empty());
    }

    #[test]
    fn a_discarded_paragraph_leaves_the_one_around_it() {
        let mut builder = BlockBuilder::<()>::new(NestedTables::OwnBlock);
        builder.start_paragraph();
        builder.text("Outer");
        builder.start_paragraph();
        builder.text("Inner");
        builder.discard_paragraph();
        builder.text(" again");
        assert_eq!(builder.end_paragraph().unwrap().runs, cell("Outer again"));
    }

    #[test]
    fn images_take_the_current_link_and_count_as_content() {
        let mut builder = BlockBuilder::<()>::new(NestedTables::OwnBlock);
        builder.start_paragraph();
        builder.link = Some("https://example.com".into());
        builder.image(
            "word/media/a.png".into(),
            "Logo".into(),
            Some((914_400, 457_200)),
        );

        let runs = builder.end_paragraph().unwrap().runs;
        assert_eq!(
            runs,
            [image_run(
                "word/media/a.png".into(),
                "Logo".into(),
                Some((914_400, 457_200)),
                Some("https://example.com".into()),
            )]
        );
    }

    #[test]
    fn paragraphs_in_a_table_fill_its_cells() {
        let mut builder = BlockBuilder::<()>::new(NestedTables::OwnBlock);
        builder.start_table();
        assert!(paragraph(&mut builder, "Bobby").is_none());
        assert!(paragraph(&mut builder, "Don").is_none());
        builder.end_cell();
        builder.end_row();
        builder.end_table();
        assert_eq!(paragraph(&mut builder, "After"), Some(cell("After")));

        assert_eq!(
            builder.into_blocks(),
            [Block::Table(vec![vec![cell("Bobby\nDon")]])]
        );
    }

    #[test]
    fn a_table_without_rows_adds_nothing() {
        let mut builder = BlockBuilder::<()>::new(NestedTables::OwnBlock);
        builder.start_table();
        builder.end_table();
        builder.end_table();
        assert!(builder.into_blocks().is_empty());
    }

    #[test]
    fn a_nested_table_goes_into_the_outer_cell_or_its_own_block() {
        let nest = |nested_tables| {
            let mut builder = BlockBuilder::<()>::new(nested_tables);
            builder.start_table();
            paragraph(&mut builder, "One");
            table(&mut builder, &["x", "y"]);
            builder.end_cell();
            builder.end_row();
            builder.end_table();
            builder.into_blocks()
        };

        assert_eq!(
            nest(NestedTables::IntoOuterCell),
            [Block::Table(vec![vec![cell("One\nx\ny")]])]
        );
        assert_eq!(
            nest(NestedTables::OwnBlock),
            [
                Block::Table(vec![vec![cell("x"), cell("y")]]),
                Block::Table(vec![vec![cell("One")]]),
            ]
        );
    }
}
