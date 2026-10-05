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
fn numbers_lists_and_draws_tables() {
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
    let cell = |text: &str| format!("<w:tc>{}</w:tc>", para(text));
    let body = format!(
        "{}{}<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        item("Read the book"),
        item("Build a CLI"),
        cell("Crate"),
        cell("Use"),
        cell("zip"),
        cell("unpack .docx"),
    );
    let (_dir, path) = sample_docx_with_parts(&body, &[part("word/numbering.xml", numbering)]);
    let pages = page_texts(&convert_to_pdf(&path, &[]).0);

    for expected in [
        "1. Read the book",
        "2. Build a CLI",
        "Crate Use",
        "zip unpack .docx",
    ] {
        assert!(
            pages[0].contains(expected),
            "{expected:?} not in {:?}",
            pages[0]
        );
    }
}

#[test]
fn leaves_out_links_with_unsafe_schemes() {
    let (_dir, path) = sample_docx_with_parts(
        r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>click me</w:t></w:r></w:hyperlink></w:p>"#,
        &[part(
            "word/_rels/document.xml.rels",
            rels(&[("rId1", "hyperlink", "javascript:alert(1)")]),
        )],
    );
    let (pdf, _stderr) = convert_to_pdf(&path, &[]);

    assert_eq!(page_texts(&pdf), ["click me"]);
    assert!(
        !String::from_utf8_lossy(&pdf).contains("javascript"),
        "the PDF links to javascript:"
    );
}

#[test]
fn links_and_embeds_pictures() {
    let picture = r#"<w:p><w:r><w:drawing><wp:docPr id="1" name="p" descr="Logo"/><a:blip r:embed="rId4"/></w:drawing></w:r></w:p>"#;
    let body = format!(
        r#"<w:p><w:r><w:t xml:space="preserve">Learn </w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t>Rust</w:t></w:r></w:hyperlink></w:p>{picture}"#
    );
    let (_dir, path) = sample_docx_with_parts(
        &body,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[
                    ("rId9", "hyperlink", "https://www.rust-lang.org"),
                    ("rId4", "image", "media/image1.png"),
                ]),
            ),
            part("word/media/image1.png", RED_PNG),
        ],
    );
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    assert_eq!(page_texts(&pdf), ["Learn Rust"]);
    let raw = String::from_utf8_lossy(&pdf);
    assert!(
        raw.contains("https://www.rust-lang.org"),
        "no link annotation"
    );
    let document = pdf_extract::Document::load_mem(&pdf).unwrap();
    let has_image = document.objects.values().any(|object| {
        object
            .as_stream()
            .is_ok_and(|stream| is_image(&stream.dict))
    });
    assert!(has_image, "no image in the PDF");
    assert_eq!(stderr, "");
}

/// True for an image XObject's dictionary.
fn is_image(dict: &pdf_extract::Dictionary) -> bool {
    dict.get(b"Subtype")
        .and_then(|subtype| subtype.as_name())
        .is_ok_and(|name| name == b"Image")
}

#[test]
fn draws_cjk_with_an_installed_font() {
    let (_dir, path) = sample_docx(&para("中文 日本語 한국어"));
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    // Noto Sans has no CJK, so this needs a font installed on the computer. macOS and Windows
    // always have one; a bare Linux machine may not, and then the PDF shows boxes instead.
    if stderr.contains("no installed font has these characters") {
        eprintln!("skipped: no CJK font installed ({stderr})");
        return;
    }
    assert_eq!(stderr, "");
    assert_eq!(page_texts(&pdf), ["中文 日本語 한국어"]);
}

#[test]
fn warns_about_images_a_pdf_cannot_hold() {
    let (_dir, path) = sample_docx_with_parts(
        r#"<w:p><w:r><w:t>Chart:</w:t></w:r><w:r><w:drawing><wp:docPr id="1" name="p" descr="Chart"/><a:blip r:embed="rId4"/></w:drawing></w:r></w:p>"#,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[("rId4", "image", "media/image1.emf")]),
            ),
            part("word/media/image1.emf", "not a format a PDF can hold"),
        ],
    );
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    assert_eq!(page_texts(&pdf), ["Chart:"]);
    assert!(stderr.contains("left out 1 images"), "{stderr}");
}

/// [`RED_PNG`], with a header that says it's `width` x `height` pixels.
fn png_declaring(width: u32, height: u32) -> Vec<u8> {
    let mut png = RED_PNG.to_vec();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    png
}

#[test]
fn leaves_out_images_too_large_to_decode() {
    // 100,000 x 100,000 pixels would take tens of GB to decode, from a file of a few bytes.
    let (_dir, path) = sample_docx_with_parts(
        r#"<w:p><w:r><w:t>Map:</w:t></w:r><w:r><w:drawing><wp:docPr id="1" name="p" descr="Map"/><a:blip r:embed="rId4"/></w:drawing></w:r></w:p>"#,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[("rId4", "image", "media/image1.png")]),
            ),
            part("word/media/image1.png", png_declaring(100_000, 100_000)),
        ],
    );
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    assert_eq!(page_texts(&pdf), ["Map:"]);
    assert!(
        stderr.contains("left out 1 images larger than 50 megapixels"),
        "{stderr}"
    );
    assert!(!stderr.contains("formats a PDF can't hold"), "{stderr}");
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
