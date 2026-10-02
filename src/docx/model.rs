//! A small document model: just enough of Word's structure to write Markdown.

/// One top-level piece of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading, level 1 (largest) to 6.
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    /// A bulleted or numbered list item; `level` 0 is the outermost list.
    ListItem {
        kind: ListKind,
        level: u8,
        runs: Vec<Run>,
    },
    /// Rows of cells. The first row is treated as the header row.
    Table(Vec<Vec<Cell>>),
}

/// The text of one table cell. Paragraphs inside the cell are separated by `"\n"`.
pub type Cell = Vec<Run>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// A stretch of text that shares the same formatting (and link, if any).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
}

/// Character formatting we carry over to Markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
}

impl Run {
    pub fn new(text: impl Into<String>, style: RunStyle) -> Self {
        Run {
            text: text.into(),
            style,
            link: None,
        }
    }

    /// The same run, pointing at `url`.
    pub fn linked(self, url: impl Into<String>) -> Self {
        Run {
            link: Some(url.into()),
            ..self
        }
    }

    /// True if `other` has the same formatting and link, so the two can be merged.
    pub fn same_format(&self, other: &Run) -> bool {
        self.style == other.style && self.link == other.link
    }
}
