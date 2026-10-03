//! Word documents and PowerPoint decks to PDF (`--to pdf`).

use std::path::Path;

use predicates::str::contains;

use crate::common::*;

/// Converts `input` to `<its folder>/out.pdf`, returning the PDF's bytes and stderr.
fn convert_to_pdf(input: &Path, extra_args: &[&str]) -> (Vec<u8>, String) {
    let out = input.with_file_name("out.pdf");
    let output = officeconv()
        .arg(input)
        .args(["--to", "pdf", "-o"])
        .arg(&out)
        .args(extra_args)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr: {stderr}");
    (std::fs::read(&out).unwrap(), stderr)
}

/// The text of each page, as a PDF reader extracts it, with whitespace runs collapsed.
fn page_texts(pdf: &[u8]) -> Vec<String> {
    assert!(pdf.starts_with(b"%PDF-"), "not a PDF");
    pdf_extract::extract_text_from_mem_by_pages(pdf)
        .unwrap()
        .iter()
        .map(|page| page.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// A paragraph holding `text`.
fn para(text: &str) -> String {
    format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
}

#[test]
fn converts_docx_to_pdf() {
    let body = [
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Meeting notes</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:r><w:t xml:space="preserve">Ship the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>beta</w:t></w:r><w:r><w:t xml:space="preserve"> on </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>Friday</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p>"#.to_string(),
        para("Café Ωμέγα Привет"),
    ]
    .concat();
    let (_dir, path) = sample_docx(&body);
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    let pages = page_texts(&pdf);
    assert_eq!(pages.len(), 1);
    // The extractor puts a space between differently styled runs, so "Friday" and "." are
    // checked apart.
    for expected in [
        "Meeting notes",
        "Ship the beta on Friday",
        "Café Ωμέγα Привет",
    ] {
        assert!(
            pages[0].contains(expected),
            "{expected:?} not in {:?}",
            pages[0]
        );
    }
    assert_eq!(stderr, "");
}

#[test]
fn wraps_long_paragraphs_onto_more_pages() {
    let sentence = "The quick brown fox jumps over the lazy dog. ".repeat(30);
    let body: String = (1..=12)
        .map(|i| para(&format!("Paragraph {i}. {sentence}")))
        .collect();
    let (_dir, path) = sample_docx(&body);
    let pages = page_texts(&convert_to_pdf(&path, &[]).0);

    assert!(pages.len() >= 3, "{} pages", pages.len());
    let all = pages.join(" ");
    for i in 1..=12 {
        assert!(
            all.contains(&format!("Paragraph {i}.")),
            "paragraph {i} missing"
        );
    }
}

#[test]
fn converts_pptx_to_one_page_per_slide() {
    let (_dir, path) = sample_pptx();
    let pages = page_texts(&convert_to_pdf(&path, &[]).0);

    assert_eq!(pages.len(), 2);
    assert!(pages[0].starts_with("Slide 1: Agenda"), "{:?}", pages[0]);
    assert!(pages[0].contains("Read the book"), "{:?}", pages[0]);
    assert!(pages[0].contains("Notes Keep it short."), "{:?}", pages[0]);
    assert_eq!(pages[1], "Slide 2: Thanks");
}

#[test]
fn leaves_out_notes_from_pdf_with_no_notes() {
    let (_dir, path) = sample_pptx();
    let pages = page_texts(&convert_to_pdf(&path, &["--no-notes"]).0);

    assert_eq!(pages.len(), 2);
    assert!(!pages[0].contains("Keep it short"), "{:?}", pages[0]);
}

#[test]
fn writes_pdf_to_piped_stdout() {
    let (_dir, path) = sample_docx(&para("Piped"));
    let output = officeconv()
        .arg(&path)
        .args(["--to", "pdf"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(page_texts(&output.stdout), ["Piped"]);
}

#[test]
fn rejects_pdf_for_spreadsheets() {
    let (_dir, path) = sample_xlsx();
    officeconv()
        .arg(&path)
        .args(["--to", "pdf", "-o", "out.pdf"])
        .assert()
        .failure()
        .stderr(contains(
            "cannot convert xlsx to pdf; xlsx supports: csv, tsv, json, md",
        ));
}

#[test]
fn rejects_images_flag_with_pdf() {
    let (dir, path) = sample_docx(&para("Hi"));
    officeconv()
        .arg(&path)
        .args(["--to", "pdf", "-o"])
        .arg(dir.path().join("out.pdf"))
        .args(["--images", "img"])
        .assert()
        .failure()
        .stderr(contains("--images doesn't apply to --to pdf"));
}
