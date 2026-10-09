//! PowerPoint decks to Markdown.

use crate::common::*;

#[test]
fn converts_pptx_to_markdown() {
    let (_dir, path) = sample_pptx();

    assert_eq!(
        convert(&path, "md"),
        "\
## Slide 1: Agenda

- Read [the book](https://doc.rust-lang.org/book/)

### Notes

Keep it short.

---

## Slide 2: Thanks
"
    );
}

#[test]
fn leaves_out_notes_with_no_notes() {
    let (_dir, path) = sample_pptx();
    officeconv()
        .arg(&path)
        .args(["--to", "md", "--no-notes"])
        .assert()
        .success()
        .stdout(
            "\
## Slide 1: Agenda

- Read [the book](https://doc.rust-lang.org/book/)

---

## Slide 2: Thanks
",
        );
}

/// A slide whose title is `text`.
fn titled_slide(text: &str) -> String {
    slide(&placeholder(
        "title",
        &format!("<a:p><a:r><a:t>{text}</a:t></a:r></a:p>"),
    ))
}

#[test]
fn keeps_the_text_around_a_nested_paragraph_or_table() {
    // PowerPoint never nests paragraphs or tables, but a broken file can. Neither loses text:
    // the outer paragraph keeps its own, and a nested table's text goes into its outer cell.
    let paragraphs = r#"<a:p><a:r><a:t xml:space="preserve">First </a:t></a:r><a:p><a:r><a:t>Inner</a:t></a:r></a:p><a:r><a:t>Last</a:t></a:r></a:p>"#;
    let cell = |text: &str| {
        format!("<a:tc><a:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></a:txBody></a:tc>")
    };
    let table = format!(
        "<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Outer</a:t></a:r></a:p></a:txBody><a:tbl><a:tr>{}{}</a:tr></a:tbl></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        cell("x"),
        cell("y"),
    );
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.push(part(
        "ppt/slides/slide1.xml",
        slide(&format!("{}{table}", placeholder("subTitle", paragraphs))),
    ));
    let (_dir, path) = sample_package("talk.pptx", &parts);

    assert_eq!(
        convert(&path, "md"),
        "\
## Slide 1

Inner

First Last

| Outer<br>x<br>y |
| --------------- |
"
    );
}

#[test]
fn converts_a_slide_listed_twice_once() {
    // Two entries point at the same slide through two relationships. PowerPoint never does
    // this, but a small file could list one slide thousands of times.
    let mut parts = presentation(&[
        "slides/slide1.xml",
        "slides/slide2.xml",
        "slides/slide1.xml",
    ])
    .to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", titled_slide("Intro")),
        part("ppt/slides/slide2.xml", titled_slide("Thanks")),
    ]);
    let (_dir, path) = sample_package("talk.pptx", &parts);

    assert_eq!(
        convert(&path, "md"),
        "## Slide 1: Intro\n\n---\n\n## Slide 2: Thanks\n"
    );
}

#[test]
fn converts_a_slide_id_listed_twice_once() {
    // The same entry, twice.
    let [presentation_xml, presentation_rels] = presentation(&["slides/slide1.xml"]);
    let repeated = String::from_utf8(presentation_xml.1).unwrap().replace(
        "<p:sldIdLst>",
        r#"<p:sldIdLst><p:sldId id="300" r:id="rIdSlide0"/>"#,
    );
    let (_dir, path) = sample_package(
        "talk.pptx",
        &[
            part("ppt/presentation.xml", repeated),
            presentation_rels,
            part("ppt/slides/slide1.xml", titled_slide("Intro")),
        ],
    );

    assert_eq!(convert(&path, "md"), "## Slide 1: Intro\n");
}

#[test]
fn keeps_links_in_notes() {
    // The notes part has its own relationships, so its rId1 isn't the slide's rId1.
    let link = |id: &str, text: &str| {
        format!(r#"<a:r><a:rPr><a:hlinkClick r:id="{id}"/></a:rPr><a:t>{text}</a:t></a:r>"#)
    };
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", titled_slide("Intro")),
        part(
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[
                ("rId1", "hyperlink", "https://example.com/slide"),
                ("rId2", "notesSlide", "../notesSlides/notesSlide1.xml"),
            ]),
        ),
        part(
            "ppt/notesSlides/notesSlide1.xml",
            slide(&placeholder(
                "body",
                &format!(
                    "<a:p>{}<a:r><a:t xml:space=\"preserve\"> and </a:t></a:r>{}</a:p>",
                    link("rId1", "the book"),
                    link("rId2", "click me"),
                ),
            )),
        ),
        part(
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            rels(&[
                ("rId1", "hyperlink", "https://doc.rust-lang.org/book/"),
                ("rId2", "hyperlink", "javascript:alert(1)"),
            ]),
        ),
    ]);
    let (_dir, path) = sample_package("talk.pptx", &parts);

    assert_eq!(
        convert(&path, "md"),
        "## Slide 1: Intro\n\n### Notes\n\n[the book](https://doc.rust-lang.org/book/) and click me\n"
    );
}

#[test]
fn reads_notes_shared_by_two_slides_once() {
    // Each slide has its own notes in a real deck. Sharing one would let a small file repeat a
    // large notes part on every slide.
    let notes_rels = rels(&[("rId1", "notesSlide", "../notesSlides/notesSlide1.xml")]);
    let mut parts = presentation(&["slides/slide1.xml", "slides/slide2.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", titled_slide("Intro")),
        part("ppt/slides/slide2.xml", titled_slide("Thanks")),
        part("ppt/slides/_rels/slide1.xml.rels", notes_rels.clone()),
        part("ppt/slides/_rels/slide2.xml.rels", notes_rels),
        part(
            "ppt/notesSlides/notesSlide1.xml",
            slide(&placeholder(
                "body",
                "<a:p><a:r><a:t>Say hello.</a:t></a:r></a:p>",
            )),
        ),
    ]);
    let (_dir, path) = sample_package("talk.pptx", &parts);

    assert_eq!(
        convert(&path, "md"),
        "## Slide 1: Intro\n\n### Notes\n\nSay hello.\n\n---\n\n## Slide 2: Thanks\n"
    );
}

#[test]
fn keeps_every_column_of_a_table_with_merged_cells() {
    let cell = |attrs: &str, text: &str| {
        format!(
            "<a:tc{attrs}><a:txBody><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>"
        )
    };
    let table = format!(
        r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="1"/><a:gridCol w="1"/></a:tblGrid><a:tr h="1">{}{}</a:tr><a:tr h="1">{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
        cell(r#" gridSpan="2""#, "Sales 2026"),
        cell(r#" hMerge="1""#, ""),
        cell("", "Q1"),
        cell("", "Q2"),
    );
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.push(part("ppt/slides/slide1.xml", slide(&table)));
    let (_dir, path) = sample_package("talk.pptx", &parts);

    assert_eq!(
        convert(&path, "md"),
        "\
## Slide 1

| Sales 2026 |     |
| ---------- | --- |
| Q1         | Q2  |
"
    );
}
