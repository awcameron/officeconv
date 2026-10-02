//! Rendering [`Block`]s as Markdown.

use super::model::{Block, Run, RunStyle};

/// Renders blocks as Markdown, separated by blank lines.
pub fn render(blocks: &[Block]) -> String {
    let mut out = blocks
        .iter()
        .map(render_block)
        .collect::<Vec<_>>()
        .join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, runs } => {
            // Headings already render bold, and a line break would end the heading early.
            let text = render_runs(&without_bold(runs)).replace("  \n", " ");
            format!("{} {}", "#".repeat(usize::from(*level)), text)
        }
        Block::Paragraph(runs) => escape_block_start(&render_runs(runs)),
    }
}

/// Clears bold on every run, then joins neighbors whose formatting now matches.
fn without_bold(runs: &[Run]) -> Vec<Run> {
    let mut merged: Vec<Run> = Vec::new();
    for run in runs {
        let style = RunStyle {
            bold: false,
            ..run.style
        };
        match merged.last_mut() {
            Some(last) if last.style == style => last.text.push_str(&run.text),
            _ => merged.push(Run::new(run.text.clone(), style)),
        }
    }
    merged
}

fn render_runs(runs: &[Run]) -> String {
    let text: String = runs.iter().map(render_run).collect();
    // A line break inside a paragraph is two spaces then a newline in Markdown.
    text.trim().replace('\n', "  \n")
}

/// Wraps a run in `**`/`*`, keeping surrounding spaces outside the markers.
///
/// `**bold **` isn't bold in Markdown, but `**bold** ` is.
fn render_run(run: &Run) -> String {
    let marker = match (run.style.bold, run.style.italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => "",
    };

    let inner = run.text.trim();
    if marker.is_empty() || inner.is_empty() {
        return escape_inline(&run.text);
    }

    let leading = &run.text[..run.text.len() - run.text.trim_start().len()];
    let trailing = &run.text[run.text.trim_end().len()..];
    format!(
        "{leading}{marker}{}{marker}{trailing}",
        escape_inline(inner)
    )
}

/// Escapes characters that Markdown would treat as formatting.
fn escape_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<') {
            out.push('\\');
        }
        out.push(c);
    }
    out
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
    fn renders_line_breaks() {
        let runs = [run("one\ntwo", false, false)];
        assert_eq!(render_runs(&runs), "one  \ntwo");
    }

    #[test]
    fn escapes_markdown_syntax() {
        let runs = [run("2*3 = snake_case [x] -> y", false, false)];
        assert_eq!(render_runs(&runs), r"2\*3 = snake\_case \[x\] -> y");
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
