//! Word documents to Markdown.

use crate::common::*;

#[test]
fn converts_docx_to_markdown() {
    let (_dir, path) = sample_docx(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Meeting notes</w:t></w:r></w:p>
           <w:p><w:r><w:t xml:space="preserve">Ship the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>beta</w:t></w:r><w:r><w:t xml:space="preserve"> on </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>Friday</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p>"#,
    );

    assert_eq!(
        convert(&path, "md"),
        "# Meeting notes\n\nShip the **beta** on *Friday*.\n"
    );
}

/// A document with one hyperlink, `click me`, whose target is `target` as written in the XML.
fn docx_linking_to(target: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    sample_docx_with_parts(
        r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>click me</w:t></w:r></w:hyperlink></w:p>"#,
        &[part(
            "word/_rels/document.xml.rels",
            rels(&[("rId1", "hyperlink", target)]),
        )],
    )
}

#[test]
fn link_targets_cannot_break_out_of_the_link() {
    // A newline ends the link and a tab separates HTML attributes, so without escaping the
    // `<img>` becomes live HTML on its own line.
    let (_dir, path) = docx_linking_to(
        "https://example.com/&#10;&#10;&lt;img&#9;src=x&#9;onerror=&quot;alert&amp;#40;1&amp;#41;&quot;&gt;",
    );

    assert_eq!(
        convert(&path, "md"),
        "[click me](https://example.com/%0A%0A%3Cimg%09src=x%09onerror=%22alert&amp;#40;1&amp;#41;%22%3E)\n"
    );
}

#[test]
fn entities_in_link_targets_cannot_form_a_scheme() {
    // Renderers decode `&#58;` to `:` and `&#9;` to a tab (which browsers strip from URLs), so
    // these would become `javascript:` links if the `&` weren't escaped.
    for (target, expected) in [
        (
            "javascript&amp;#58;alert&amp;#40;document.domain&amp;#41;",
            "[click me](javascript&amp;#58;alert&amp;#40;document.domain&amp;#41;)\n",
        ),
        (
            "java&amp;#9;script:alert&amp;#40;1&amp;#41;",
            "[click me](java&amp;#9;script:alert&amp;#40;1&amp;#41;)\n",
        ),
    ] {
        let (_dir, path) = docx_linking_to(target);
        assert_eq!(convert(&path, "md"), expected, "target {target}");
    }
}

#[test]
fn drops_links_with_unsafe_schemes_but_keeps_their_text() {
    for target in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "java&#9;script:alert(1)",
        "data:text/html,hi",
        "vbscript:msgbox",
    ] {
        let (_dir, path) = docx_linking_to(target);
        assert_eq!(convert(&path, "md"), "click me\n", "target {target}");
    }

    for (target, expected) in [
        ("https://example.com", "https://example.com"),
        ("HTTP://example.com", "HTTP://example.com"),
        ("mailto:me@example.com", "mailto:me@example.com"),
        ("other.docx", "other.docx"),
        ("../shared/report.pdf#page=2", "../shared/report.pdf#page=2"),
    ] {
        let (_dir, path) = docx_linking_to(target);
        assert_eq!(
            convert(&path, "md"),
            format!("[click me]({expected})\n"),
            "target {target}"
        );
    }
}

#[test]
fn markdown_shows_html_and_entities_in_text_literally() {
    // Python-Markdown doesn't treat `\\<` as an escape, so `<` has to be written as `&lt;`.
    let (_dir, path) = sample_docx(
        "<w:p><w:r><w:t>&lt;img src=x onerror=alert(1)&gt; costs &amp;pound;5, Q&amp;A</w:t></w:r></w:p>",
    );

    assert_eq!(
        convert(&path, "md"),
        "&lt;img src=x onerror=alert(1)> costs &amp;pound;5, Q&A\n"
    );
}

#[test]
fn converts_docx_lists_links_and_tables() {
    let numbering = format!(
        r#"<w:numbering xmlns:w="{WORD_NS}">
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
    </w:numbering>"#
    );
    let item = |text: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    };
    let cell = |text: &str| format!("<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>");
    let body = format!(
        r#"<w:p><w:r><w:t xml:space="preserve">Learn </w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t>Rust</w:t></w:r></w:hyperlink></w:p>{}{}<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>"#,
        item("Read the book"),
        item("Build a CLI"),
        cell("Crate"),
        cell("Use"),
        cell("zip"),
        cell("unpack .docx"),
    );
    let (_dir, path) = sample_docx_with_parts(
        &body,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[("rId9", "hyperlink", "https://www.rust-lang.org")]),
            ),
            part("word/numbering.xml", numbering),
        ],
    );

    assert_eq!(
        convert(&path, "md"),
        "\
Learn [Rust](https://www.rust-lang.org)

1. Read the book
1. Build a CLI

| Crate | Use          |
| ----- | ------------ |
| zip   | unpack .docx |
"
    );
}

#[test]
fn keeps_every_column_of_a_table_with_merged_cells() {
    let cell = |span: u32, text: &str| {
        format!(
            r#"<w:tc><w:tcPr><w:gridSpan w:val="{span}"/></w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#
        )
    };
    let (_dir, path) = sample_docx(&format!(
        "<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr>{}</w:tr><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        cell(2, "Sales 2026"),
        cell(1, "Q1"),
        cell(1, "Q2"),
        cell(1, "120"),
        cell(1, "140"),
    ));

    assert_eq!(
        convert(&path, "md"),
        "\
| Sales 2026 |     |
| ---------- | --- |
| Q1         | Q2  |
| 120        | 140 |
"
    );
}
