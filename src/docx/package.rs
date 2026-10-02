//! The supporting parts of a `.docx` that `document.xml` refers to by ID.
//!
//! - `word/_rels/document.xml.rels`: relationship IDs (`rId5`) to link targets.
//! - `word/numbering.xml`: numbering IDs to bullet or numbered list formats.
//! - `word/styles.xml`: style IDs to style names, and any list numbering a style applies.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::attr;
use super::model::ListKind;
use crate::error::Result;

/// Lookup tables built from a document's supporting parts.
#[derive(Debug, Default)]
pub struct Package {
    /// Relationship ID -> link target.
    pub links: HashMap<String, String>,
    pub numbering: Numbering,
    /// Style ID -> style details.
    pub styles: HashMap<String, Style>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Style {
    pub name: String,
    /// Set when the style itself makes a paragraph a list item (e.g. Word's "List Bullet").
    pub num_id: Option<String>,
}

/// Word stores list formats in two steps: a `num` points at an `abstractNum`,
/// which defines the format of each indent level.
#[derive(Debug, Default)]
pub struct Numbering {
    num_to_abstract: HashMap<String, String>,
    /// Abstract numbering ID -> (indent level -> list kind).
    levels: HashMap<String, HashMap<u8, ListKind>>,
}

impl Numbering {
    /// Returns the list kind for a paragraph's numbering ID and indent level.
    ///
    /// `numId` 0 means "numbering removed", so it isn't a list item at all.
    pub fn kind(&self, num_id: &str, level: u8) -> Option<ListKind> {
        if num_id == "0" {
            return None;
        }

        let kind = self
            .num_to_abstract
            .get(num_id)
            .and_then(|abstract_id| self.levels.get(abstract_id))
            .and_then(|levels| levels.get(&level))
            .copied();
        // A list we can't look up is still a list; bullets are the safe guess.
        Some(kind.unwrap_or(ListKind::Bullet))
    }
}

/// Calls `visit` for every opening or self-closing element in `xml`.
fn visit_elements(xml: &str, mut visit: impl FnMut(&BytesStart)) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => visit(&e),
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

pub fn parse_relationships(xml: &str) -> Result<HashMap<String, String>> {
    let mut links = HashMap::new();
    visit_elements(xml, |e| {
        if e.local_name().as_ref() == "Relationship"
            && let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target"))
        {
            links.insert(id, target);
        }
    })?;
    Ok(links)
}

pub fn parse_numbering(xml: &str) -> Result<Numbering> {
    let mut numbering = Numbering::default();
    let mut abstract_id: Option<String> = None;
    let mut level: u8 = 0;
    let mut num_id: Option<String> = None;

    visit_elements(xml, |e| match e.local_name().as_ref() {
        "abstractNum" => abstract_id = attr(e, "abstractNumId"),
        "lvl" => level = attr(e, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
        "numFmt" => {
            if let (Some(id), Some(format)) = (&abstract_id, attr(e, "val")) {
                let kind = match format.as_str() {
                    "bullet" | "none" => ListKind::Bullet,
                    _ => ListKind::Numbered,
                };
                numbering
                    .levels
                    .entry(id.clone())
                    .or_default()
                    .insert(level, kind);
            }
        }
        "num" => num_id = attr(e, "numId"),
        "abstractNumId" => {
            if let (Some(num), Some(target)) = (&num_id, attr(e, "val")) {
                numbering.num_to_abstract.insert(num.clone(), target);
            }
        }
        _ => {}
    })?;
    Ok(numbering)
}

pub fn parse_styles(xml: &str) -> Result<HashMap<String, Style>> {
    let mut styles: HashMap<String, Style> = HashMap::new();
    let mut current: Option<String> = None;

    visit_elements(xml, |e| match e.local_name().as_ref() {
        "style" => {
            current = attr(e, "styleId");
            if let Some(id) = &current {
                styles.insert(id.clone(), Style::default());
            }
        }
        "name" | "numId" => {
            let Some(style) = current.as_ref().and_then(|id| styles.get_mut(id)) else {
                return;
            };
            let value = attr(e, "val");
            if e.local_name().as_ref() == "name" {
                style.name = value.unwrap_or_default();
            } else {
                style.num_id = value;
            }
        }
        _ => {}
    })?;
    Ok(styles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_relationship_targets() {
        let links = parse_relationships(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                 <Relationship Id="rId4" Type=".../hyperlink" Target="https://example.com" TargetMode="External"/>
                 <Relationship Id="rId1" Type=".../styles" Target="styles.xml"/>
               </Relationships>"#,
        )
        .unwrap();
        assert_eq!(links["rId4"], "https://example.com");
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn resolves_list_kinds_through_abstract_numbering() {
        let numbering = parse_numbering(
            r#"<w:numbering xmlns:w="w">
                 <w:abstractNum w:abstractNumId="10">
                   <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
                   <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl>
                 </w:abstractNum>
                 <w:num w:numId="3"><w:abstractNumId w:val="10"/></w:num>
               </w:numbering>"#,
        )
        .unwrap();

        assert_eq!(numbering.kind("3", 0), Some(ListKind::Numbered));
        assert_eq!(numbering.kind("3", 1), Some(ListKind::Bullet));
        assert_eq!(numbering.kind("99", 0), Some(ListKind::Bullet));
        assert_eq!(numbering.kind("0", 0), None);
    }

    #[test]
    fn reads_style_names_and_numbering() {
        let styles = parse_styles(
            r#"<w:styles xmlns:w="w">
                 <w:style w:type="paragraph" w:styleId="Kop1"><w:name w:val="heading 1"/></w:style>
                 <w:style w:type="paragraph" w:styleId="ListBullet">
                   <w:name w:val="List Bullet"/>
                   <w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr>
                 </w:style>
               </w:styles>"#,
        )
        .unwrap();

        assert_eq!(styles["Kop1"].name, "heading 1");
        assert_eq!(styles["Kop1"].num_id, None);
        assert_eq!(styles["ListBullet"].num_id.as_deref(), Some("7"));
    }
}
