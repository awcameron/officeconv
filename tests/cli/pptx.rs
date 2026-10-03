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
