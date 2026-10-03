//! Helpers shared by the test modules: running the binary, and building sample Office files.
//!
//! Every fixture is built in a fresh temp dir, so tests don't share files and can run in
//! parallel. The returned `TempDir` deletes the folder when it's dropped, so keep it alive
//! (`let (_dir, path) = ...`) for as long as the test uses the file.

use std::io::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// The relationship types namespace; relationship kinds are appended to it (`.../image`).
pub const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The WordprocessingML namespace, bound to the `w:` prefix in Word parts.
pub const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Namespace declarations for PowerPoint slide and presentation parts.
fn pptx_ns() -> String {
    format!(
        r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="{REL}""#
    )
}

/// The `officeconv` binary, ready for arguments.
pub fn officeconv() -> Command {
    Command::cargo_bin("officeconv").unwrap()
}

/// Runs the converter on `path` and returns its stdout, failing the test if it fails.
pub fn convert(path: &Path, to: &str) -> String {
    let output = officeconv().arg(path).args(["--to", to]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Creates an empty file with the given name inside a fresh temp dir.
pub fn touch(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, b"").unwrap();
    (dir, path)
}

/// Writes a small one-sheet workbook, `sales.xlsx`, into a fresh temp dir.
///
/// "12" is written as text and 7.5 as a number, which the typed-JSON tests rely on.
pub fn sample_xlsx() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sales.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_row(0, 0, ["Region", "Units"]).unwrap();
    sheet.write_row(1, 0, ["North, East", "12"]).unwrap();
    sheet.write(2, 0, "South").unwrap();
    sheet.write(2, 1, 7.5).unwrap();
    workbook.save(&path).unwrap();
    (dir, path)
}

/// One file inside a package: its part name and contents.
pub fn part(name: &str, contents: impl Into<Vec<u8>>) -> (String, Vec<u8>) {
    (name.to_string(), contents.into())
}

/// Writes a zip-based Office file called `name`, containing `parts`, into a fresh temp dir.
pub fn sample_package(name: &str, parts: &[(String, Vec<u8>)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, contents) in parts {
        zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();
    (dir, path)
}

/// A `.rels` part from `(id, kind, target)` entries, such as `("rId4", "image", "media/a.png")`.
///
/// Targets that are web addresses are marked as external, as Office does.
pub fn rels(entries: &[(&str, &str, &str)]) -> String {
    let relationships: String = entries
        .iter()
        .map(|(id, kind, target)| {
            let mode = if target.starts_with("http") {
                r#" TargetMode="External""#
            } else {
                ""
            };
            format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"{mode}/>"#)
        })
        .collect();
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
    )
}

/// `word/document.xml` with `body` inside `<w:body>`.
pub fn docx_document(body: &str) -> String {
    format!(
        r#"<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL}"><w:body>{body}</w:body></w:document>"#
    )
}

/// Writes a minimal `.docx`, `notes.docx`, whose body is `body_xml`.
pub fn sample_docx(body_xml: &str) -> (TempDir, PathBuf) {
    sample_docx_with_parts(body_xml, &[])
}

/// Like [`sample_docx`], plus extra parts such as `word/numbering.xml`.
pub fn sample_docx_with_parts(body_xml: &str, parts: &[(String, Vec<u8>)]) -> (TempDir, PathBuf) {
    let mut all = vec![part("word/document.xml", docx_document(body_xml))];
    all.extend_from_slice(parts);
    sample_package("notes.docx", &all)
}

/// A slide (or notes) part holding `shapes`.
pub fn slide(shapes: &str) -> String {
    let ns = pptx_ns();
    format!(r#"<p:sld {ns}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#)
}

/// `ppt/presentation.xml` and its `.rels`, listing slide parts (relative to `ppt/`) in
/// presentation order.
pub fn presentation(slides_in_order: &[&str]) -> [(String, Vec<u8>); 2] {
    let ids: String = (0..slides_in_order.len())
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rIdSlide{i}"/>"#, 256 + i))
        .collect();
    let entries: Vec<(String, &str)> = slides_in_order
        .iter()
        .enumerate()
        .map(|(i, target)| (format!("rIdSlide{i}"), *target))
        .collect();
    let entries: Vec<(&str, &str, &str)> = entries
        .iter()
        .map(|(id, target)| (id.as_str(), "slide", *target))
        .collect();

    [
        part(
            "ppt/presentation.xml",
            format!(
                r#"<p:presentation {}><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#,
                pptx_ns()
            ),
        ),
        part("ppt/_rels/presentation.xml.rels", rels(&entries)),
    ]
}

/// A placeholder shape: `ph_type` is `title`, `body`, and so on; `paragraphs` is DrawingML.
fn placeholder(ph_type: &str, paragraphs: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="{ph_type}"/><p:cNvSpPr/><p:nvPr><p:ph type="{ph_type}"/></p:nvPr></p:nvSpPr><p:txBody>{paragraphs}</p:txBody></p:sp>"#
    )
}

/// A two-slide deck, `talk.pptx`: "Agenda" with a link and speaker notes, then "Thanks".
///
/// The presentation lists slide2.xml first, so it must come out first.
pub fn sample_pptx() -> (TempDir, PathBuf) {
    let title =
        |text: &str| placeholder("title", &format!("<a:p><a:r><a:t>{text}</a:t></a:r></a:p>"));
    let agenda = slide(&format!(
        "{}{}",
        title("Agenda"),
        placeholder(
            "body",
            r#"<a:p><a:r><a:t xml:space="preserve">Read </a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="rId5"/></a:rPr><a:t>the book</a:t></a:r></a:p>"#
        )
    ));
    let agenda_rels = rels(&[
        ("rId5", "hyperlink", "https://doc.rust-lang.org/book/"),
        ("rId6", "notesSlide", "../notesSlides/notesSlide1.xml"),
    ]);
    let notes = slide(&placeholder(
        "body",
        "<a:p><a:r><a:t>Keep it short.</a:t></a:r></a:p>",
    ));

    let mut parts = presentation(&["slides/slide2.xml", "slides/slide1.xml"]).to_vec();
    parts.extend([
        part("ppt/slides/slide1.xml", slide(&title("Thanks"))),
        part("ppt/slides/slide2.xml", agenda),
        part("ppt/slides/_rels/slide2.xml.rels", agenda_rels),
        part("ppt/notesSlides/notesSlide1.xml", notes),
    ]);
    sample_package("talk.pptx", &parts)
}
