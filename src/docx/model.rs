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
}

/// A stretch of text that shares the same formatting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
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
        }
    }
}
