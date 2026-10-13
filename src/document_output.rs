//! Converts a Word document or a deck: reads its blocks, deals with their images as the output
//! needs, and writes Markdown or a PDF to stdout or the `-o` file.

use std::io::Write;
use std::path::Path;

use crate::document::{self, Block};
#[cfg(not(feature = "pdf"))]
use crate::error::ConvertError;
use crate::error::Result;
use crate::images::{self, EmbeddedImages, Images};
use crate::input::Source;
use crate::opc::Archive;
use crate::output;
use crate::plan::{DocumentOutput, DocumentReader, Pages};
use crate::{docx, pptx};

/// Converts the document in `source` with `reader`, to Markdown (saving its images if asked
/// to) or to PDF, written to stdout or the `output` file.
///
/// Says on `log` how many images it saved, and anything it had to leave out.
pub fn convert(
    source: &Source,
    reader: DocumentReader,
    to: DocumentOutput,
    output: Option<&Path>,
    log: &mut impl Write,
) -> Result<()> {
    // The images folder is created before the input is read, as for a workbook.
    let mut export = match &to {
        DocumentOutput::Markdown { images } => {
            images::export_to(images.as_deref(), output::markdown_dir(output))?
        }
        DocumentOutput::Pdf(_) => None,
    };

    let mut archive = Archive::open(source.reader()?)?;
    let mut blocks = match reader {
        DocumentReader::Pptx(notes) => pptx::read_blocks(&mut archive, notes)?,
        DocumentReader::Docx => docx::read_blocks(&mut archive)?,
    };

    match to {
        DocumentOutput::Markdown { .. } => {
            // Markdown leaves headers and footers out, so `--images` mustn't save their pictures.
            blocks.retain(|block| !block.is_page_furniture());
            let blocks = images::resolve(
                blocks,
                &mut archive,
                match export.as_mut() {
                    Some(export) => Images::Save(export),
                    None => Images::Skip,
                },
            )?;
            write_output(output, document::markdown::render(&blocks).as_bytes())?;
            if let Some(export) = &export {
                export.report(log)?;
            }
            Ok(())
        }
        DocumentOutput::Pdf(pages) => {
            let mut embedded = EmbeddedImages::default();
            let blocks = images::resolve(blocks, &mut archive, Images::Embed(&mut embedded))?;
            write_pdf(pages, &blocks, &embedded, output, log)
        }
    }
}

/// Writes a Word document as A4 pages, or a deck as one landscape page per slide. Image runs
/// hold keys into `images`.
#[cfg(feature = "pdf")]
fn write_pdf(
    pages: Pages,
    blocks: &[Block],
    images: &EmbeddedImages,
    output: Option<&Path>,
    log: &mut impl Write,
) -> Result<()> {
    use crate::pdf::{self, layout::PageSetup};

    let setup = match pages {
        Pages::Slides => PageSetup::SLIDES,
        Pages::Document => PageSetup::DOCUMENT,
    };
    let rendered = pdf::render(blocks, images, setup)?;
    write_output(output, &rendered.pdf)?;
    rendered.report(log)?;
    Ok(())
}

#[cfg(not(feature = "pdf"))]
fn write_pdf(
    _pages: Pages,
    _blocks: &[Block],
    _images: &EmbeddedImages,
    _output: Option<&Path>,
    _log: &mut impl Write,
) -> Result<()> {
    Err(ConvertError::PdfNotBuilt)
}

/// Writes the converted document to stdout or the `output` file.
fn write_output(output: Option<&Path>, bytes: &[u8]) -> Result<()> {
    let mut out = output::open_output(output)?;
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opc::test_package;
    use crate::xlsx::tests::PNG;
    use std::fs;
    use tempfile::TempDir;

    const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    /// A picture whose image is the part relationship `rId1` points at.
    const PICTURE: &str = r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="p" descr="Logo"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="pic"><pic:blipFill><a:blip r:embed="rId1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;

    fn rels(entries: &[(&str, &str, &str)]) -> String {
        let relationships: String = entries
            .iter()
            .map(|(id, kind, target)| {
                format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"/>"#)
            })
            .collect();
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
        )
    }

    /// A Word document on stdin whose header holds a picture, with a picture in the body too
    /// if `body_picture`.
    fn document(body_picture: bool) -> Source {
        let picture = if body_picture { PICTURE } else { "" };
        let document = format!(
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL}"><w:body><w:p><w:r><w:t>Body</w:t></w:r>{picture}</w:p><w:sectPr><w:headerReference w:type="default" r:id="rId2"/></w:sectPr></w:body></w:document>"#
        );
        let header = format!(
            r#"<w:hdr xmlns:w="{WORD_NS}" xmlns:r="{REL}"><w:p><w:r><w:t>Header</w:t></w:r>{PICTURE}</w:p></w:hdr>"#
        );
        let package = test_package(&[
            ("word/document.xml", document.as_bytes()),
            (
                "word/_rels/document.xml.rels",
                rels(&[
                    ("rId1", "image", "media/body.png"),
                    ("rId2", "header", "header1.xml"),
                ])
                .as_bytes(),
            ),
            ("word/header1.xml", header.as_bytes()),
            (
                "word/_rels/header1.xml.rels",
                rels(&[("rId1", "image", "media/logo.png")]).as_bytes(),
            ),
            ("word/media/body.png", PNG),
            ("word/media/logo.png", PNG),
        ]);
        Source::Stdin(package.into_inner())
    }

    /// Converts `source` to `output`, returning what it said on its log.
    fn convert_to(source: &Source, to: DocumentOutput, output: &Path) -> Result<String> {
        let mut log = Vec::new();
        convert(source, DocumentReader::Docx, to, Some(output), &mut log)?;
        Ok(String::from_utf8(log).unwrap())
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn markdown_saves_the_pictures_it_shows_and_none_from_headers() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("images");
        let output = dir.path().join("doc.md");
        let to = DocumentOutput::Markdown {
            images: Some(images.clone()),
        };
        let log = convert_to(&document(true), to, &output).unwrap();

        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "Body![Logo](images/body.png)\n"
        );
        assert_eq!(files(&images), ["body.png"]);
        assert_eq!(log, format!("saved 1 images to {}\n", images.display()));
    }

    #[test]
    fn markdown_saves_no_header_pictures_when_the_body_has_none() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("images");
        let output = dir.path().join("doc.md");
        let to = DocumentOutput::Markdown {
            images: Some(images.clone()),
        };
        let log = convert_to(&document(false), to, &output).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "Body\n");
        assert!(files(&images).is_empty());
        assert_eq!(log, format!("saved 0 images to {}\n", images.display()));
    }

    #[test]
    fn markdown_without_images_saves_none_and_says_nothing() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("doc.md");
        let to = DocumentOutput::Markdown { images: None };
        let log = convert_to(&document(true), to, &output).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "Body\n");
        assert_eq!(files(dir.path()), ["doc.md"]);
        assert_eq!(log, "");
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn pdf_embeds_its_pictures_and_saves_no_files() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("doc.pdf");
        let to = DocumentOutput::Pdf(Pages::Document);
        let log = convert_to(&document(true), to, &output).unwrap();

        assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
        assert_eq!(files(dir.path()), ["doc.pdf"]);
        assert_eq!(log, "");
    }

    #[cfg(not(feature = "pdf"))]
    #[test]
    fn pdf_needs_the_pdf_feature() {
        let dir = TempDir::new().unwrap();
        let to = DocumentOutput::Pdf(Pages::Document);
        let err = convert_to(&document(true), to, &dir.path().join("doc.pdf")).unwrap_err();
        assert!(matches!(err, ConvertError::PdfNotBuilt), "{err}");
    }
}
