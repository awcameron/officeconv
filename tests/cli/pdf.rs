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
fn writes_footnotes_and_endnotes_at_the_end() {
    let (_dir, path) = sample_docx_with_notes();
    let (pdf, _) = convert_to_pdf(&path, &[]);

    let pages = page_texts(&pdf);
    assert_eq!(pages.len(), 1);
    let text = &pages[0];
    // The extractor puts a space between runs, so it finds one before each reference.
    let order = [
        "Water boils at 100 °C [1] at sea level [2] .",
        "Again [1]",
        "[1] See the table",
        "[2] Standard pressure.",
    ];
    let positions: Vec<usize> = order
        .iter()
        .map(|t| {
            text.find(t)
                .unwrap_or_else(|| panic!("no {t:?} in {text:?}"))
        })
        .collect();
    assert!(positions.is_sorted(), "out of order: {text:?}");
    assert!(!text.contains("Never referred to"), "{text:?}");
}

#[test]
fn repeats_the_header_and_footer_on_every_page() {
    let (_dir, path) = sample_docx_with_header_and_footer(120);
    let (pdf, _) = convert_to_pdf(&path, &[]);

    let pages = page_texts(&pdf);
    assert!(pages.len() >= 2, "{pages:?}");
    for page in &pages {
        assert!(page.starts_with("Annual report"), "{page:?}");
        // The page number Word saved is left out.
        assert!(page.ends_with(" Page"), "{page:?}");
        assert!(!page.contains("Page 7"), "{page:?}");
    }
    // The logo is drawn on every page.
    assert_eq!(image_sizes(&pdf).len(), pages.len());
}

/// Where each line of text starts across the page, in the order they're drawn.
fn text_starts(pdf: &[u8]) -> Vec<f32> {
    let document = pdf_extract::Document::load_mem(pdf).unwrap();
    let mut starts = Vec::new();
    for page in document.get_pages().into_values() {
        let content = document.get_page_content(page).unwrap();
        for op in pdf_extract::content::Content::decode(&content)
            .unwrap()
            .operations
        {
            if op.operator == "Tm" {
                starts.push(op.operands[4].as_float().unwrap());
            }
        }
    }
    starts
}

/// Checks that three lines start at the left margin, around the middle, and near the right
/// margin of a page `width` wide.
fn assert_left_center_right(starts: &[f32], margin: f32, width: f32) {
    let [left, center, right] = starts else {
        panic!("expected three lines: {starts:?}");
    };
    assert!((left - margin).abs() < 0.01, "{starts:?}");
    assert!(
        width * 0.35 < *center && *center < width * 0.5,
        "{starts:?}"
    );
    assert!(*right > width * 0.8, "{starts:?}");
}

#[test]
fn aligns_docx_paragraphs() {
    let aligned = |jc: &str, text: &str| {
        format!(r#"<w:p><w:pPr><w:jc w:val="{jc}"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#)
    };
    let body = [
        aligned("left", "Left"),
        aligned("center", "Centered"),
        aligned("right", "Right"),
    ]
    .concat();
    let (_dir, path) = sample_docx(&body);
    let (pdf, _) = convert_to_pdf(&path, &[]);

    assert_left_center_right(&text_starts(&pdf), 72.0, 595.28);
    // Markdown has no alignment.
    assert_eq!(convert(&path, "md"), "Left\n\nCentered\n\nRight\n");
}

#[test]
fn aligns_pptx_paragraphs() {
    let aligned = |algn: &str, text: &str| {
        format!(r#"<a:p><a:pPr algn="{algn}"/><a:r><a:t>{text}</a:t></a:r></a:p>"#)
    };
    let text_box = format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Text"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:txBody>{}{}{}</p:txBody></p:sp>"#,
        aligned("l", "Left"),
        aligned("ctr", "Centered"),
        aligned("r", "Right"),
    );
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.push(part("ppt/slides/slide1.xml", slide(&text_box)));
    let (_dir, path) = sample_package("talk.pptx", &parts);
    let (pdf, _) = convert_to_pdf(&path, &[]);

    // The first line is the slide's heading, on the left.
    let starts = text_starts(&pdf);
    assert!((starts[0] - 48.0).abs() < 0.01, "{starts:?}");
    assert_left_center_right(&starts[1..], 48.0, 960.0);
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

/// The table borders a PDF draws, added up: for each x a vertical border is drawn at, how
/// long it is in all, and the same for each y of a horizontal border. Only stroked paths
/// count, so the header's shading doesn't.
fn border_lengths(pdf: &[u8]) -> (Vec<f32>, Vec<f32>) {
    let document = pdf_extract::Document::load_mem(pdf).unwrap();
    let (mut vertical, mut horizontal) = (Vec::new(), Vec::new());
    for page in document.get_pages().into_values() {
        let content = document.get_page_content(page).unwrap();
        let mut path: Vec<((f32, f32), (f32, f32))> = Vec::new();
        let mut from = (0.0, 0.0);
        for op in pdf_extract::content::Content::decode(&content)
            .unwrap()
            .operations
        {
            let point = || {
                let number = |i: usize| op.operands[i].as_float().unwrap();
                (number(0), number(1))
            };
            match op.operator.as_str() {
                "m" => from = point(),
                "l" => {
                    let to = point();
                    path.push((from, to));
                    from = to;
                }
                "S" => {
                    for (from, to) in path.drain(..) {
                        if from.0 == to.0 {
                            add_length(&mut vertical, to.0, (to.1 - from.1).abs());
                        } else if from.1 == to.1 {
                            add_length(&mut horizontal, to.1, (to.0 - from.0).abs());
                        }
                    }
                }
                "f" | "f*" | "n" => path.clear(),
                _ => {}
            }
        }
    }
    let lengths = |lines: Vec<(f32, f32)>| lines.into_iter().map(|(_, length)| length).collect();
    (lengths(vertical), lengths(horizontal))
}

/// Adds `length` to the total for position `at`.
fn add_length(totals: &mut Vec<(f32, f32)>, at: f32, length: f32) {
    match totals
        .iter_mut()
        .find(|(other, _)| (other - at).abs() < 0.01)
    {
        Some((_, total)) => *total += length,
        None => totals.push((at, length)),
    }
}

/// Checks a three-by-three table whose first row merges its first two cells, and whose first
/// column merges its last two: every border but one in each direction runs the full width or
/// height of the table.
fn assert_draws_merged_cells(pdf: &[u8]) {
    let (vertical, horizontal) = border_lengths(pdf);
    for (lengths, direction) in [(vertical, "vertical"), (horizontal, "horizontal")] {
        assert_eq!(lengths.len(), 4, "{direction} borders: {lengths:?}");
        let full = lengths.iter().copied().fold(0.0, f32::max);
        let short: Vec<_> = lengths.iter().filter(|&&l| l < full - 0.01).collect();
        assert_eq!(short.len(), 1, "{direction} borders: {lengths:?}");
    }
}

/// The Markdown both merged tables come out as, merged cells' text in their first cell.
const MERGED_TABLE_MARKDOWN: &str = "\
| A   |     | B   |
| --- | --- | --- |
| C   | D   | E   |
|     | F   | G   |
";

#[test]
fn draws_merged_docx_cells_across_their_span() {
    let cell =
        |props: &str, text: &str| format!("<w:tc><w:tcPr>{props}</w:tcPr>{}</w:tc>", para(text));
    let body = format!(
        "<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid><w:tr>{}{}</w:tr><w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>",
        cell(r#"<w:gridSpan w:val="2"/>"#, "A"),
        cell("", "B"),
        cell(r#"<w:vMerge w:val="restart"/>"#, "C"),
        cell("", "D"),
        cell("", "E"),
        "<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>",
        cell("", "F"),
        cell("", "G"),
    );
    let (_dir, path) = sample_docx(&body);
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    assert_draws_merged_cells(&pdf);
    assert_eq!(page_texts(&pdf), ["A B C D E F G"]);
    assert_eq!(stderr, "");
    assert_eq!(convert(&path, "md"), MERGED_TABLE_MARKDOWN);
}

#[test]
fn draws_merged_pptx_cells_across_their_span() {
    let cell = |attrs: &str, text: &str| {
        format!(
            "<a:tc{attrs}><a:txBody><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>"
        )
    };
    let table = format!(
        r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="1"/><a:gridCol w="1"/><a:gridCol w="1"/></a:tblGrid><a:tr h="1">{}{}{}</a:tr><a:tr h="1">{}{}{}</a:tr><a:tr h="1">{}{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
        cell(r#" gridSpan="2""#, "A"),
        cell(r#" hMerge="1""#, ""),
        cell("", "B"),
        cell(r#" rowSpan="2""#, "C"),
        cell("", "D"),
        cell("", "E"),
        cell(r#" vMerge="1""#, ""),
        cell("", "F"),
        cell("", "G"),
    );
    let mut parts = presentation(&["slides/slide1.xml"]).to_vec();
    parts.push(part("ppt/slides/slide1.xml", slide(&table)));
    let (_dir, path) = sample_package("talk.pptx", &parts);
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    assert_draws_merged_cells(&pdf);
    assert_eq!(page_texts(&pdf), ["Slide 1 A B C D E F G"]);
    assert_eq!(stderr, "");
    assert_eq!(
        convert(&path, "md"),
        format!("## Slide 1\n\n{MERGED_TABLE_MARKDOWN}")
    );
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

/// The width and height each image is drawn at, in points, in drawing order: the scale of the
/// last `cm` transform before each `Do`.
fn image_sizes(pdf: &[u8]) -> Vec<(f32, f32)> {
    let document = pdf_extract::Document::load_mem(pdf).unwrap();
    let mut sizes = Vec::new();
    for page in document.get_pages().into_values() {
        let content = document.get_page_content(page).unwrap();
        let mut scale = None;
        for op in pdf_extract::content::Content::decode(&content)
            .unwrap()
            .operations
        {
            match op.operator.as_str() {
                "cm" => {
                    let number = |i: usize| op.operands[i].as_float().unwrap().abs();
                    scale = Some((number(0), number(3)));
                }
                "Do" => sizes.extend(scale),
                _ => {}
            }
        }
    }
    sizes
}

#[test]
fn draws_pictures_at_the_size_the_document_gives_them() {
    let picture = |extent: &str| {
        format!(
            r#"<w:p><w:r><w:drawing><wp:inline>{extent}<wp:docPr id="1" name="p"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        )
    };
    let body = [
        picture(r#"<wp:extent cx="1828800" cy="1828800"/>"#),
        picture(r#"<wp:extent cx="1828800" cy="914400"/>"#),
        picture(""),
        picture(r#"<wp:extent cx="45720000" cy="45720000"/>"#),
    ]
    .concat();
    let (_dir, path) = sample_docx_with_parts(
        &body,
        &[
            part(
                "word/_rels/document.xml.rels",
                rels(&[("rId4", "image", "media/image1.png")]),
            ),
            part("word/media/image1.png", RED_PNG),
        ],
    );
    let (pdf, stderr) = convert_to_pdf(&path, &[]);

    let sizes = image_sizes(&pdf);
    let content_width = 595.28 - 2.0 * 72.0;
    let expected = [
        // 2 by 2 inches.
        (144.0, 144.0),
        // 2 by 1 inches, but the image is square: it fits inside, keeping its shape.
        (72.0, 72.0),
        // No size: 2 pixels at 96 dpi, as before.
        (1.5, 1.5),
        // 50 inches: scaled down to the page.
        (content_width, content_width),
    ];
    assert_eq!(sizes.len(), expected.len(), "{sizes:?}");
    for ((w, h), (want_w, want_h)) in sizes.iter().zip(expected) {
        assert!(
            (w - want_w).abs() < 0.01 && (h - want_h).abs() < 0.01,
            "{sizes:?}"
        );
    }
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
        .code(64)
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
        .code(64)
        .stderr(contains("--images doesn't apply to --to pdf"));
}
