//! Helpers for Office Open XML packages (`.docx`, `.pptx`, `.xlsx`).
//!
//! All three are zip archives of XML "parts". Parts point at each other through
//! relationship files: `word/_rels/document.xml.rels` lists the links, images and
//! so on that `word/document.xml` refers to by ID (`rId5`).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::Result;

/// Opens the package at `path`.
pub fn open(path: &Path) -> Result<ZipArchive<File>> {
    let file = File::open(path).map_err(ZipError::from)?;
    Ok(ZipArchive::new(file)?)
}

/// Reads one part from the archive, or `None` if the package doesn't have it.
pub fn read_part<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut xml = String::new();
    entry.read_to_string(&mut xml).map_err(ZipError::from)?;
    Ok(Some(xml))
}

/// Reads a part that must exist.
pub fn read_required_part<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String> {
    read_part(archive, name)?.ok_or_else(|| ZipError::FileNotFound.into())
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
/// Other relationships (images, a link that jumps to another slide) aren't web links.
pub fn hyperlinks(relationships: &HashMap<String, Relationship>) -> HashMap<String, String> {
    relationships
        .iter()
        .filter(|(_, r)| r.kind == "hyperlink")
        .map(|(id, r)| (id.clone(), r.target.clone()))
        .collect()
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

/// Reads an attribute by its local name (`w:val` -> `"val"`).
pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| a.value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
