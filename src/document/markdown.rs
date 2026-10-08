//! Rendering [`Block`]s as Markdown.

use super::{Block, ListKind, Run, RunStyle, TableCell, append_run};
use crate::markdown::{escape_text, escape_url, write_table};

/// Renders blocks as Markdown, separated by blank lines.
///
/// Neighboring list items get a single newline so they form one list.
pub fn render(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut previous: Option<&Block> = None;

    for block in blocks {
        if let Some(previous) = previous {
            let both_list_items = matches!(previous, Block::ListItem { .. })
                && matches!(block, Block::ListItem { .. });
            out.push_str(if both_list_items { "\n" } else { "\n\n" });
        }
        out.push_str(&render_block(block));
        previous = Some(block);
    }

    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, runs } => {
            // Headings already render bold, and a line break would end the heading early.
            let text = render_inline(&without_bold(runs)).replace('\n', " ");
            format!("{} {}", "#".repeat(usize::from(*level)), text)
        }
        Block::Paragraph(runs) => escape_block_start(&render_runs(runs)),
        Block::ListItem { kind, level, runs } => render_list_item(*kind, *level, runs),
        Block::Table(rows) => render_table(rows),
        Block::Rule => "---".to_string(),
    }
}

/// Renders `- item` or `1. item`, indented four spaces per nesting level.
///
/// Every numbered item is written as `1.`: Markdown renumbers lists when it renders them.
fn render_list_item(kind: ListKind, level: u8, runs: &[Run]) -> String {
    let indent = " ".repeat(4 * usize::from(level));
    let marker = match kind {
        ListKind::Bullet => "-",
        ListKind::Numbered => "1.",
    };

    // Lines after a line break must line up with the item's text to stay in the item.
    let continuation = format!("\n{indent}{}", " ".repeat(marker.len() + 1));
    let text = render_runs(runs).replace('\n', &continuation);
    format!("{indent}{marker} {text}")
}

/// Renders a table with the same Markdown writer the XLSX converter uses. Markdown has no
/// merged cells, so a merged cell's text goes in its first cell and the cells it covers are
/// left empty.
fn render_table(rows: &[Vec<TableCell>]) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| render_inline(cell.runs())).collect())
        .collect();

    let mut buffer = Vec::new();
    write_table(&rows, &mut buffer).expect("writing to a Vec can't fail");
    String::from_utf8(buffer)
        .expect("the table writer only writes the UTF-8 it was given")
        .trim_end()
        .to_string()
}

/// Clears bold on every run, then joins neighbors whose formatting now matches.
fn without_bold(runs: &[Run]) -> Vec<Run> {
    let mut merged = Vec::new();
    for run in runs {
        let style = RunStyle {
            bold: false,
            ..run.style
        };
        append_run(
            &mut merged,
            Run {
                style,
                ..run.clone()
            },
        );
    }
    merged
}

/// Renders runs as Markdown for a paragraph or list item, where a line break is two spaces
/// then a newline.
fn render_runs(runs: &[Run]) -> String {
    render_inline(runs).replace('\n', "  \n")
}

/// Renders runs as Markdown, wrapping each stretch of linked runs in `[...](url)`. Line breaks
/// stay as `"\n"`, for the caller to write as its kind of block can hold them.
fn render_inline(runs: &[Run]) -> String {
    let mut text = String::new();
    for group in runs.chunk_by(|a, b| a.link == b.link) {
        let inner: String = group.iter().map(render_run).collect();
        match &group[0].link {
            Some(url) => {
                let (leading, inner, trailing) = split_edges(&inner);
                text.push_str(&format!(
                    "{leading}[{inner}]({}){trailing}",
                    escape_url(url)
                ));
            }
            None => text.push_str(&inner),
        }
    }

    text.trim().to_string()
}

/// Wraps a run in `**`/`*`, keeping surrounding spaces outside the markers.
///
/// `**bold **` isn't bold in Markdown, but `**bold** ` is.
fn render_run(run: &Run) -> String {
    if let Some(image) = &run.image {
        // Alt text can't span lines in Markdown.
        let alt = escape_text(&run.text.split_whitespace().collect::<Vec<_>>().join(" "));
        return format!("![{alt}]({})", escape_url(&image.source));
    }

    let marker = match (run.style.bold, run.style.italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => "",
    };

    let (leading, inner, trailing) = split_edges(&run.text);
    if marker.is_empty() || inner.is_empty() {
        return escape_text(&run.text);
    }
    format!("{leading}{marker}{}{marker}{trailing}", escape_text(inner))
}

/// Splits `text` into (leading whitespace, the rest, trailing whitespace).
fn split_edges(text: &str) -> (&str, &str, &str) {
    let inner = text.trim();
    let start = text.len() - text.trim_start().len();
    (&text[..start], inner, &text[start + inner.len()..])
}

/// Stops a paragraph that starts like `# x`, `> x`, `- x` or `1. x` turning into a heading,
/// quote or list.
fn escape_block_start(text: &str) -> String {
    let starts_like_marker = text.starts_with('#')
        || text.starts_with('>')
        || text.starts_with("- ")
        || text.starts_with("+ ")
        || ordered_list_prefix(text);
    if !starts_like_marker {
        return text.to_string();
    }

    // Put a backslash before the punctuation that makes it a marker.
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    format!("{}\\{}", &text[..split], &text[split..])
}

/// True for text like `1. ` or `12) `.
fn ordered_list_prefix(text: &str) -> bool {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && {
        let rest = &text[digits..];
        rest.starts_with(". ") || rest.starts_with(") ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::ImageRef;

    fn run(text: &str, bold: bool, italic: bool) -> Run {
        Run::new(text, RunStyle { bold, italic })
    }

    #[test]
    fn renders_headings_and_paragraphs() {
        let blocks = [
            Block::Heading {
                level: 2,
                runs: vec![run("Intro", false, false)],
            },
            Block::Paragraph(vec![run("Body.", false, false)]),
        ];
        assert_eq!(render(&blocks), "## Intro\n\nBody.\n");
    }

    #[test]
    fn headings_drop_bold_but_keep_italic() {
        let blocks = [Block::Heading {
            level: 1,
            runs: vec![
                run("TDD: ", true, false),
                run("Template", true, false),
                run(" v2", true, true),
            ],
        }];
        assert_eq!(render(&blocks), "# TDD: Template *v2*\n");
    }

    #[test]
    fn keeps_spaces_outside_emphasis() {
        let runs = [
            run("Say ", false, false),
            run("hello ", true, false),
            run("there", false, true),
            run(" and ", false, false),
            run("both", true, true),
        ];
        assert_eq!(render_runs(&runs), "Say **hello** *there* and ***both***");
    }

    #[test]
    fn renders_links_with_spaces_outside() {
        let runs = [
            run("See ", false, false),
            run("the ", false, false).linked("https://example.com/a b"),
            run("docs ", true, false).linked("https://example.com/a b"),
            run("now.", false, false),
        ];
        assert_eq!(
            render_runs(&runs),
            "See [the **docs**](https://example.com/a%20b) now."
        );
    }

    #[test]
    fn renders_nested_lists_tightly() {
        let item = |kind, level, text: &str| Block::ListItem {
            kind,
            level,
            runs: vec![run(text, false, false)],
        };
        let blocks = [
            Block::Paragraph(vec![run("Steps:", false, false)]),
            item(ListKind::Numbered, 0, "Unzip"),
            item(ListKind::Bullet, 1, "word/document.xml"),
            item(ListKind::Numbered, 0, "Parse"),
            Block::Paragraph(vec![run("Done.", false, false)]),
        ];
        assert_eq!(
            render(&blocks),
            "Steps:\n\n1. Unzip\n    - word/document.xml\n1. Parse\n\nDone.\n"
        );
    }

    #[test]
    fn continues_list_items_across_line_breaks() {
        let text = render_list_item(ListKind::Bullet, 1, &[run("one\ntwo", false, false)]);
        assert_eq!(text, "    - one  \n      two");
    }

    #[test]
    fn renders_tables_with_line_breaks_and_pipes() {
        let cell = |text: &str| TableCell::new(vec![run(text, false, false)]);
        let blocks = [Block::Table(vec![
            vec![cell("Team"), cell("Members")],
            vec![cell("A|B"), cell("Bobby\nDon")],
        ])];
        assert_eq!(
            render(&blocks),
            "\
| Team | Members      |
| ---- | ------------ |
| A\\|B | Bobby<br>Don |
"
        );
    }

    #[test]
    fn writes_a_merged_cell_in_its_first_cell() {
        let blocks = [Block::Table(vec![
            vec![
                TableCell::Content {
                    runs: vec![run("Wide and tall", false, false)],
                    cols: 2,
                    rows: 2,
                },
                TableCell::Covered,
            ],
            vec![TableCell::Covered, TableCell::Covered],
        ])];
        assert_eq!(
            render(&blocks),
            "\
| Wide and tall |     |
| ------------- | --- |
|               |     |
"
        );
    }

    #[test]
    fn escapes_link_targets() {
        let blocks = [Block::Paragraph(vec![
            run("a", false, false).linked("https://x.com/?a=1&b=2 (x)"),
        ])];
        assert_eq!(
            render(&blocks),
            "[a](https://x.com/?a=1&amp;b=2%20%28x%29)\n"
        );
    }

    #[test]
    fn renders_images_with_alt_text() {
        let runs = [
            run("See ", false, false),
            Run::image(ImageRef::new("img/chart 1.png"), "Sales [Q3]\nchart"),
        ];
        assert_eq!(
            render_runs(&runs),
            r"See ![Sales \[Q3\] chart](img/chart%201.png)"
        );
    }

    #[test]
    fn renders_linked_images() {
        let runs =
            [Run::image(ImageRef::new("img/logo.png"), "Logo").linked("https://example.com")];
        assert_eq!(
            render_runs(&runs),
            "[![Logo](img/logo.png)](https://example.com)"
        );
    }

    #[test]
    fn renders_line_breaks() {
        let runs = [run("one\ntwo", false, false)];
        assert_eq!(render_runs(&runs), "one  \ntwo");
    }

    #[test]
    fn escapes_markdown_syntax() {
        let runs = [run("2*3 = snake_case [x] -> y", false, false)];
        assert_eq!(render_runs(&runs), r"2\*3 = snake\_case \[x\] -> y");

        let runs = [run("<b> &copy; Q&A", false, false)];
        assert_eq!(render_runs(&runs), "&lt;b> &amp;copy; Q&A");
    }

    #[test]
    fn escapes_text_that_looks_like_a_marker() {
        assert_eq!(escape_block_start("# not a heading"), r"\# not a heading");
        assert_eq!(escape_block_start("- not a list"), r"\- not a list");
        assert_eq!(escape_block_start("> not a quote"), r"\> not a quote");
        assert_eq!(escape_block_start("1. not a list"), r"1\. not a list");
        assert_eq!(escape_block_start("2026 was fine"), "2026 was fine");
    }

    #[test]
    fn empty_document_renders_nothing() {
        assert_eq!(render(&[]), "");
    }
}
