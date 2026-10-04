//! Helpers for Office Open XML packages (`.docx`, `.pptx`, `.xlsx`).
//!
//! All three are zip archives of XML "parts". Parts point at each other through
//! relationship files: `word/_rels/document.xml.rels` lists the links, images and
//! so on that `word/document.xml` refers to by ID (`rId5`).

use std::collections::HashMap;
use std::io::{self, Read, Seek, Write};

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::{ConvertError, Result};

const MB: u64 = 1 << 20;

/// How much a package may decompress to.
///
/// A small file can decompress to gigabytes (a "zip bomb"), and the sizes in a zip's headers can
/// be faked, so these are checked against the bytes actually read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most one part may decompress to.
    pub part: u64,
    /// The most all the parts read from one package may add up to.
    pub total: u64,
}

impl Limits {
    /// Far above any real document, while keeping memory use bounded.
    pub const DEFAULT: Limits = Limits {
        part: 256 * MB,
        total: 1024 * MB,
    };
}

/// An open package, which keeps count of how much has been read from it.
#[derive(Debug)]
pub struct Archive<R> {
    zip: ZipArchive<R>,
    limits: Limits,
    /// Bytes read so far, after decompressing.
    read: u64,
}

impl<R: Read + Seek> Archive<R> {
    /// Opens a package from anything readable and seekable, such as a file or bytes in memory.
    pub fn open(reader: R) -> Result<Self> {
        Archive::with_limits(reader, Limits::DEFAULT)
    }

    pub fn with_limits(reader: R, limits: Limits) -> Result<Self> {
        Ok(Archive {
            zip: ZipArchive::new(reader)?,
            limits,
            read: 0,
        })
    }

    /// Reads one XML part, or `None` if the package doesn't have it.
    pub fn read_part(&mut self, name: &str) -> Result<Option<String>> {
        let Some(bytes) = self.read_bytes(name)? else {
            return Ok(None);
        };
        let xml = String::from_utf8(bytes)
            .map_err(|err| ZipError::Io(io::Error::new(io::ErrorKind::InvalidData, err)))?;
        Ok(Some(xml))
    }

    /// Reads an XML part that must exist.
    pub fn read_required_part(&mut self, name: &str) -> Result<String> {
        self.read_part(name)?
            .ok_or_else(|| ZipError::FileNotFound.into())
    }

    /// Reads any part, such as an image, or `None` if the package doesn't have it.
    pub fn read_bytes(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let entry = match self.zip.by_name(name) {
            Ok(entry) => entry,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mut bytes = Vec::new();
        read_limited(entry, name, self.limits, &mut self.read, &mut bytes)?;
        Ok(Some(bytes))
    }

    /// Decompresses every XML part without keeping it, to check it's within the limits.
    ///
    /// This is for packages read by a library that can't be limited itself (calamine).
    pub fn check_xml_parts(&mut self) -> Result<()> {
        for i in 0..self.zip.len() {
            let entry = self.zip.by_index(i)?;
            let name = entry.name().to_string();
            if name.ends_with(".xml") || name.ends_with(".rels") {
                read_limited(entry, &name, self.limits, &mut self.read, &mut io::sink())?;
            }
        }
        Ok(())
    }
}

/// Copies the part `name` from `entry` to `out`, adding its size to `read`.
///
/// Stops one byte past what the limits allow, so a part that's too big is never read in full.
fn read_limited(
    entry: impl Read,
    name: &str,
    limits: Limits,
    read: &mut u64,
    out: &mut impl Write,
) -> Result<()> {
    let allowed = limits.part.min(limits.total.saturating_sub(*read));
    let copied = io::copy(&mut entry.take(allowed + 1), out).map_err(ZipError::from)?;
    if copied > allowed {
        return Err(if allowed == limits.part {
            ConvertError::PartTooLarge {
                part: name.to_string(),
                limit: limits.part,
            }
        } else {
            ConvertError::InputTooLarge {
                limit: limits.total,
            }
        });
    }
    *read += copied;
    Ok(())
}

/// The relationships file for a part: `ppt/slides/slide1.xml` -> `ppt/slides/_rels/slide1.xml.rels`.
pub fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolves a relationship target against the folder of the part that refers to it.
///
/// Targets are relative (`slides/slide1.xml`, `../media/a.png`) or start at the package root
/// (`/ppt/slides/slide1.xml`).
pub fn resolve_target(part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }

    let mut segments: Vec<&str> = match part.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    segments.join("/")
}

/// One entry in a `.rels` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The last segment of the relationship type URI: `hyperlink`, `slide`, `notesSlide`, ...
    pub kind: String,
    pub target: String,
    /// True when the target is outside the package, such as a web address.
    pub external: bool,
}

/// Relationship ID -> relationship, from a `.rels` part.
pub fn parse_relationships(xml: &str) -> Result<HashMap<String, Relationship>> {
    let mut relationships = HashMap::new();
    visit_elements(xml, |e| {
        if e.local_name().as_ref() == "Relationship"
            && let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target"))
        {
            let kind = attr(e, "Type")
                .and_then(|t| t.rsplit('/').next().map(str::to_string))
                .unwrap_or_default();
            let external = attr(e, "TargetMode").as_deref() == Some("External");
            relationships.insert(
                id,
                Relationship {
                    kind,
                    target,
                    external,
                },
            );
        }
    })?;
    Ok(relationships)
}

/// Just the external links: relationship ID -> URL.
///
/// Other relationships (images, a link that jumps to another slide) aren't web links. Only
/// `http`, `https`, `mailto` and relative links are kept: the text of any other link, such as
/// `javascript:`, is converted without the link.
pub fn hyperlinks(relationships: &HashMap<String, Relationship>) -> HashMap<String, String> {
    relationships
        .iter()
        .filter(|(_, r)| r.kind == "hyperlink" && is_safe_link(&r.target))
        .map(|(id, r)| (id.clone(), r.target.clone()))
        .collect()
}

/// True for an `http`, `https` or `mailto` link, or a relative one with no scheme, such as
/// `other.docx`.
///
/// Other schemes, such as `javascript:`, `data:` or `file:`, can run code or open local files
/// when someone clicks the link in the Markdown or PDF.
fn is_safe_link(target: &str) -> bool {
    // The scheme is everything before the first `:`, as long as no `/`, `?` or `#` comes first.
    match target.find([':', '/', '?', '#']) {
        Some(i) if target[i..].starts_with(':') => {
            let scheme = &target[..i];
            ["http", "https", "mailto"]
                .iter()
                .any(|safe| scheme.eq_ignore_ascii_case(safe))
        }
        _ => true,
    }
}

/// Images stored in the package: relationship ID -> image part (`ppt/media/image1.png`).
///
/// `part` is the part the relationships belong to; targets are resolved against it.
pub fn image_parts(
    relationships: &HashMap<String, Relationship>,
    part: &str,
) -> HashMap<String, String> {
    relationships
        .iter()
        .filter(|(_, r)| r.kind == "image" && !r.external)
        .map(|(id, r)| (id.clone(), resolve_target(part, &r.target)))
        .collect()
}

/// Something that reacts to XML as it streams past. See [`walk`].
pub trait XmlHandler {
    /// An opening tag, or a self-closing one when `is_empty` is true (no matching `end` follows).
    fn start(&mut self, e: &BytesStart, is_empty: bool);
    /// A closing tag, by local name (`</w:p>` -> `"p"`).
    fn end(&mut self, name: &str);
    /// Text between tags, with entities such as `&amp;` already resolved.
    fn text(&mut self, text: &str);
}

/// Streams through `xml`, calling `handler` for each tag and piece of text.
pub fn walk(xml: &str, handler: &mut impl XmlHandler) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Eof => return Ok(()),
            Event::Start(e) => handler.start(&e, false),
            Event::Empty(e) => handler.start(&e, true),
            Event::End(e) => handler.end(e.local_name().as_ref()),
            Event::Text(t) => handler.text(&t),
            Event::GeneralRef(r) => {
                // `&amp;` and `&#233;` arrive as their own events.
                let resolved = match r.resolve_char_ref()? {
                    Some(c) => c.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&r)
                        .unwrap_or_default()
                        .to_string(),
                };
                handler.text(&resolved);
            }
            _ => {}
        }
    }
}

/// Calls `visit` for every opening or self-closing element in `xml`.
pub fn visit_elements(xml: &str, mut visit: impl FnMut(&BytesStart)) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => visit(&e),
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

/// Reads an attribute by its local name (`w:val` -> `"val"`), decoding entities like `&amp;`.
pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    let attribute = e
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)?;
    let value = attribute
        .normalized_value(XmlVersion::Implicit1_0)
        .map(|v| v.into_owned())
        // A value with a broken entity is still better kept as written than dropped.
        .unwrap_or_else(|_| attribute.value.into_owned());
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package with the given parts, built in memory.
    fn zip_with(parts: &[(&str, &[u8])]) -> io::Cursor<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap()
    }

    const SMALL: Limits = Limits {
        part: 10,
        total: 25,
    };

    #[test]
    fn reads_parts_within_the_limits() {
        let mut archive = Archive::with_limits(
            zip_with(&[("a.xml", b"0123456789"), ("b.png", b"012")]),
            SMALL,
        )
        .unwrap();
        assert_eq!(archive.read_part("a.xml").unwrap().unwrap(), "0123456789");
        assert_eq!(archive.read_bytes("b.png").unwrap().unwrap(), b"012");
        assert_eq!(archive.read_part("missing.xml").unwrap(), None);
    }

    #[test]
    fn rejects_a_part_over_the_limit() {
        let mut archive =
            Archive::with_limits(zip_with(&[("word/document.xml", &[b' '; 11])]), SMALL).unwrap();
        let err = archive.read_part("word/document.xml").unwrap_err();
        assert!(
            matches!(&err, ConvertError::PartTooLarge { part, limit: 10 } if part == "word/document.xml"),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_parts_that_add_up_to_more_than_the_total() {
        let ten = [b' '; 10];
        let mut archive = Archive::with_limits(
            zip_with(&[("a.xml", &ten), ("b.xml", &ten), ("c.xml", &ten)]),
            SMALL,
        )
        .unwrap();
        archive.read_bytes("a.xml").unwrap();
        archive.read_bytes("b.xml").unwrap();
        let err = archive.read_bytes("c.xml").unwrap_err();
        assert!(
            matches!(err, ConvertError::InputTooLarge { limit: 25 }),
            "{err:?}"
        );
    }

    #[test]
    fn checks_xml_parts_but_not_other_files() {
        let big = [b' '; 11];
        let mut media = Archive::with_limits(zip_with(&[("xl/media/v.mp4", &big)]), SMALL).unwrap();
        media.check_xml_parts().unwrap();

        let mut sheet =
            Archive::with_limits(zip_with(&[("xl/worksheets/sheet1.xml", &big)]), SMALL).unwrap();
        assert!(matches!(
            sheet.check_xml_parts(),
            Err(ConvertError::PartTooLarge { .. })
        ));
    }

    #[test]
    fn reads_relationship_targets() {
        let links = parse_relationships(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                 <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
                 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
               </Relationships>"#,
        )
        .unwrap();
        assert_eq!(
            links["rId4"],
            Relationship {
                kind: "hyperlink".into(),
                target: "https://example.com".into(),
                external: true,
            }
        );
        assert_eq!(links["rId1"].kind, "styles");
        assert_eq!(hyperlinks(&links).len(), 1);
    }

    #[test]
    fn keeps_only_safe_links() {
        for target in [
            "https://a.com",
            "HTTP://a.com",
            "mailto:me@a.com",
            "a.docx",
            "../b.pdf#p=2",
            "/x:y",
        ] {
            assert!(is_safe_link(target), "{target}");
        }
        for target in [
            "javascript:alert(1)",
            "java\tscript:x",
            "data:text/html,x",
            "file:///etc/passwd",
            "C:\\a.docx",
            " https://a.com",
        ] {
            assert!(!is_safe_link(target), "{target}");
        }
    }

    #[test]
    fn finds_embedded_images_only() {
        let relationships = parse_relationships(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                 <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
                 <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.com/a.png" TargetMode="External"/>
               </Relationships>"#,
        )
        .unwrap();
        let images = image_parts(&relationships, "ppt/slides/slide1.xml");
        assert_eq!(images.len(), 1);
        assert_eq!(images["rId2"], "ppt/media/image1.png");
    }

    #[test]
    fn decodes_entities_in_attribute_values() {
        let mut found = Vec::new();
        visit_elements(
            r#"<r><a href="https://example.com/?a=1&amp;b=2" name="R&amp;D &#8212; 2026"/></r>"#,
            |e| {
                if e.local_name().as_ref() == "a" {
                    found.push(attr(e, "href"));
                    found.push(attr(e, "name"));
                    found.push(attr(e, "missing"));
                }
            },
        )
        .unwrap();
        assert_eq!(
            found,
            [
                Some("https://example.com/?a=1&b=2".to_string()),
                Some("R&D — 2026".to_string()),
                None,
            ]
        );
    }

    #[test]
    fn finds_rels_for_a_part() {
        assert_eq!(
            rels_path("ppt/slides/slide1.xml"),
            "ppt/slides/_rels/slide1.xml.rels"
        );
        assert_eq!(
            rels_path("ppt/presentation.xml"),
            "ppt/_rels/presentation.xml.rels"
        );
    }

    #[test]
    fn resolves_relative_and_absolute_targets() {
        assert_eq!(
            resolve_target("ppt/presentation.xml", "slides/slide2.xml"),
            "ppt/slides/slide2.xml"
        );
        assert_eq!(
            resolve_target("ppt/slides/slide1.xml", "../notesSlides/notesSlide1.xml"),
            "ppt/notesSlides/notesSlide1.xml"
        );
        assert_eq!(
            resolve_target("ppt/presentation.xml", "/ppt/slides/slide3.xml"),
            "ppt/slides/slide3.xml"
        );
    }
}
