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
