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
