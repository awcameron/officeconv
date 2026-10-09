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
        let xml = String::from_utf8(bytes).map_err(|_| ConvertError::PartNotUtf8 {
            part: name.to_string(),
        })?;
        Ok(Some(xml))
    }

    /// Reads an XML part that must exist.
    pub fn read_required_part(&mut self, name: &str) -> Result<String> {
        self.read_part(name)?
            .ok_or_else(|| ConvertError::MissingPart {
                part: name.to_string(),
            })
    }

    /// The relationships of `part`, from its `.rels` part, or none if it has no `.rels` part.
    ///
    /// The `.rels` part is read each time, and counts against the limits each time, so a reader
    /// that looks up several relationships of one part keeps what this returns.
    pub fn relationships(&mut self, part: &str) -> Result<Relationships> {
        let by_id = match self.read_part(&rels_path(part))? {
            Some(xml) => parse_relationships(&xml)?,
            None => HashMap::new(),
        };
        Ok(Relationships {
            part: part.to_string(),
            by_id,
        })
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

    /// Decompresses every part without keeping it, to check it's within the limits.
    ///
    /// This is for packages read by a library that can't be limited itself (calamine). Every
    /// part is checked, whatever its name: a workbook's relationships can point a sheet at a part
    /// named anything, such as `sheet1.dat`.
    pub fn check_part_sizes(&mut self) -> Result<()> {
        for i in 0..self.zip.len() {
            let entry = self.zip.by_index(i)?;
            let name = entry.name().to_string();
            read_limited(entry, &name, self.limits, &mut self.read, &mut io::sink())?;
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

/// One entry in a `.rels` part. Readers look relationships up through [`Relationships`], which
/// checks their kind and where they point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The last segment of the relationship type URI: `hyperlink`, `slide`, `notesSlide`, ...
    kind: String,
    target: String,
    /// True when the target is outside the package, such as a web address.
    external: bool,
}

/// The relationships of one part, from [`Archive::relationships`].
///
/// A part of the package is only found through a relationship of the kind the reader expects,
/// and never through an external one: a file can point any relationship ID anywhere, such as a
/// slide ID at a web address.
#[derive(Debug, Default)]
pub struct Relationships {
    /// The part they belong to; targets are resolved against its folder.
    part: String,
    by_id: HashMap<String, Relationship>,
}

impl Relationships {
    /// The part that relationship `id` points at, if it's a part of the package of `kind`.
    pub fn part(&self, id: &str, kind: &str) -> Option<String> {
        self.by_id
            .get(id)
            .filter(|r| r.kind == kind && !r.external)
            .map(|r| resolve_target(&self.part, &r.target))
    }

    /// Every part of the package linked by a relationship of `kind`, sorted, and each once
    /// however many relationships point at it.
    pub fn parts(&self, kind: &str) -> Vec<String> {
        let mut parts: Vec<String> = self
            .by_id
            .values()
            .filter(|r| r.kind == kind && !r.external)
            .map(|r| resolve_target(&self.part, &r.target))
            .collect();
        // HashMap order is random; sort so output doesn't change from run to run.
        parts.sort();
        parts.dedup();
        parts
    }

    /// The links and images the part refers to.
    pub fn targets(&self) -> Targets {
        Targets {
            links: hyperlinks(&self.by_id),
            images: image_parts(&self.by_id, &self.part),
        }
    }
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
fn hyperlinks(relationships: &HashMap<String, Relationship>) -> HashMap<String, String> {
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
fn image_parts(
    relationships: &HashMap<String, Relationship>,
    part: &str,
) -> HashMap<String, String> {
    relationships
        .iter()
        .filter(|(_, r)| r.kind == "image" && !r.external)
        .map(|(id, r)| (id.clone(), resolve_target(part, &r.target)))
        .collect()
}

/// What a document or slide part's relationship IDs point at, from [`Relationships::targets`].
#[derive(Debug, Default)]
pub struct Targets {
    /// Relationship ID -> web link, from [`hyperlinks`].
    pub links: HashMap<String, String>,
    /// Relationship ID -> image part (`word/media/image1.png`), from [`image_parts`].
    pub images: HashMap<String, String>,
}

/// The elements open at a point in [`walk`], by local name (`<w:r>` -> `"r"`).
///
/// Readers decide what an element or piece of text means from where it is, such as "text
/// inside `t`" or "`b` inside `r`", instead of keeping a flag for each element they're inside.
#[derive(Debug, Default)]
pub struct Open {
    /// Every open element, outermost first, as an index into `names` and `counts`.
    stack: Vec<usize>,
    /// Each local name seen so far.
    names: Vec<Box<str>>,
    /// How many elements of each name are open, so [`Open::inside`] doesn't have to search the
    /// stack: a hostile file can nest elements a million deep.
    counts: Vec<usize>,
    /// Name -> index into `names` and `counts`.
    ids: HashMap<Box<str>, usize>,
}

impl Open {
    /// True if an element named `name` is open, at any depth.
    pub fn inside(&self, name: &str) -> bool {
        self.ids.get(name).is_some_and(|&id| self.counts[id] > 0)
    }

    /// The innermost open element.
    pub fn current(&self) -> Option<&str> {
        self.stack.last().map(|&id| &*self.names[id])
    }

    fn push(&mut self, name: &str) {
        let id = match self.ids.get(name) {
            Some(&id) => id,
            None => {
                let id = self.names.len();
                self.names.push(name.into());
                self.counts.push(0);
                self.ids.insert(name.into(), id);
                id
            }
        };
        self.counts[id] += 1;
        self.stack.push(id);
    }

    fn pop(&mut self) {
        if let Some(id) = self.stack.pop() {
            self.counts[id] -= 1;
        }
    }
}

/// Something that reacts to XML as it streams past. See [`walk`].
///
/// `start` and `text` get the elements open around them. For `start` that leaves out the
/// element itself: its parent is [`Open::current`].
pub trait XmlHandler {
    /// Elements whose contents are ignored, such as the `Fallback` copy of content stored twice.
    /// The handler sees nothing from the start of one to its end.
    const SKIP: &'static [&'static str] = &[];

    /// An opening tag, or a self-closing one when `is_empty` is true (no matching `end` follows).
    fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open);
    /// A closing tag, by local name (`</w:p>` -> `"p"`).
    fn end(&mut self, name: &str);
    /// Text between tags, with entities such as `&amp;` already resolved.
    fn text(&mut self, text: &str, open: &Open);
}

/// Streams through `xml`, calling `handler` for each tag and piece of text.
pub fn walk<H: XmlHandler>(xml: &str, handler: &mut H) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut open = Open::default();
    // While above 0, we're inside one of `H::SKIP`: this many elements deep.
    let mut skip_depth = 0usize;
    loop {
        match reader.read_event()? {
            Event::Eof => return Ok(()),
            Event::Start(e) => {
                let name = e.local_name();
                let name: &str = name.as_ref();
                if skip_depth > 0 || H::SKIP.contains(&name) {
                    skip_depth += 1;
                } else {
                    handler.start(&e, false, &open);
                    open.push(name);
                }
            }
            Event::Empty(e) => {
                if skip_depth == 0 && !H::SKIP.contains(&e.local_name().as_ref()) {
                    handler.start(&e, true, &open);
                }
            }
            Event::End(e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else {
                    // quick-xml has already checked that the end tag matches the open one.
                    open.pop();
                    handler.end(e.local_name().as_ref());
                }
            }
            Event::Text(t) if skip_depth == 0 => handler.text(&t, &open),
            Event::GeneralRef(r) if skip_depth == 0 => {
                // `&amp;` and `&#233;` arrive as their own events.
                let resolved = match r.resolve_char_ref()? {
                    Some(c) => c.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&r)
                        .unwrap_or_default()
                        .to_string(),
                };
                handler.text(&resolved, &open);
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

/// A package holding `parts`, as `(name, contents)`, built in memory. Every unit test that
/// needs a zip builds it here.
#[cfg(test)]
pub fn test_package(parts: &[(&str, &[u8])]) -> io::Cursor<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_part_that_is_missing_or_not_utf8() {
        let mut archive =
            Archive::open(test_package(&[("word/document.xml", b"\xff\xfe")])).unwrap();

        let err = archive
            .read_required_part("ppt/presentation.xml")
            .unwrap_err();
        assert!(
            matches!(&err, ConvertError::MissingPart { part } if part == "ppt/presentation.xml")
        );
        assert_eq!(
            err.to_string(),
            "could not read document: ppt/presentation.xml is missing"
        );

        let err = archive.read_part("word/document.xml").unwrap_err();
        assert_eq!(
            err.to_string(),
            "could not read document: word/document.xml isn't UTF-8 text"
        );
    }

    #[test]
    fn reads_a_parts_relationships_or_none() {
        let rels = br#"<Relationships><Relationship Id="rId1" Type="http://x/image" Target="media/a.png"/></Relationships>"#;
        let mut archive =
            Archive::open(test_package(&[("word/_rels/document.xml.rels", rels)])).unwrap();

        let relationships = archive.relationships("word/document.xml").unwrap();
        assert_eq!(
            relationships.part("rId1", "image").as_deref(),
            Some("word/media/a.png")
        );
        let none = archive.relationships("word/footnotes.xml").unwrap();
        assert!(none.by_id.is_empty());
    }

    const SMALL: Limits = Limits {
        part: 10,
        total: 25,
    };

    #[test]
    fn reads_parts_within_the_limits() {
        let mut archive = Archive::with_limits(
            test_package(&[("a.xml", b"0123456789"), ("b.png", b"012")]),
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
            Archive::with_limits(test_package(&[("word/document.xml", &[b' '; 11])]), SMALL)
                .unwrap();
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
            test_package(&[("a.xml", &ten), ("b.xml", &ten), ("c.xml", &ten)]),
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
    fn checks_every_part_whatever_its_name() {
        let big = [b' '; 11];
        // calamine finds sheets through the workbook's relationships, so a sheet can be named
        // anything, and media files count toward the total too.
        for name in [
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/sheet1.dat",
            "xl/media/v.mp4",
        ] {
            let mut archive = Archive::with_limits(test_package(&[(name, &big)]), SMALL).unwrap();
            assert!(
                matches!(
                    archive.check_part_sizes(),
                    Err(ConvertError::PartTooLarge { .. })
                ),
                "{name}"
            );
        }

        let mut small =
            Archive::with_limits(test_package(&[("xl/worksheets/sheet1.dat", b"ok")]), SMALL)
                .unwrap();
        small.check_part_sizes().unwrap();
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

    /// The relationships of `ppt/slides/slide1.xml`, from `(id, kind, target)` entries. Web
    /// addresses are external, as Office marks them.
    fn slide_relationships(entries: &[(&str, &str, &str)]) -> Relationships {
        let body: String = entries
            .iter()
            .map(|(id, kind, target)| {
                let mode = if target.starts_with("http") {
                    r#" TargetMode="External""#
                } else {
                    ""
                };
                format!(
                    r#"<Relationship Id="{id}" Type="http://x/{kind}" Target="{target}"{mode}/>"#
                )
            })
            .collect();
        Relationships {
            part: "ppt/slides/slide1.xml".into(),
            by_id: parse_relationships(&format!("<Relationships>{body}</Relationships>")).unwrap(),
        }
    }

    #[test]
    fn finds_a_part_by_id_only_with_the_right_kind_inside_the_package() {
        let relationships = slide_relationships(&[
            ("rId1", "notesSlide", "../notesSlides/notesSlide1.xml"),
            ("rId2", "notesSlide", "https://example.com/notes.xml"),
        ]);
        assert_eq!(
            relationships.part("rId1", "notesSlide").as_deref(),
            Some("ppt/notesSlides/notesSlide1.xml")
        );
        assert_eq!(relationships.part("rId1", "slide"), None);
        assert_eq!(relationships.part("rId2", "notesSlide"), None);
        assert_eq!(relationships.part("rId9", "notesSlide"), None);
    }

    #[test]
    fn lists_parts_of_a_kind_sorted_and_once_each() {
        let relationships = slide_relationships(&[
            ("rId1", "drawing", "../drawings/drawing2.xml"),
            ("rId2", "drawing", "../drawings/drawing1.xml"),
            ("rId3", "drawing", "/ppt/drawings/drawing2.xml"),
            ("rId4", "drawing", "https://example.com/drawing3.xml"),
            ("rId5", "image", "../media/image1.png"),
        ]);
        assert_eq!(
            relationships.parts("drawing"),
            ["ppt/drawings/drawing1.xml", "ppt/drawings/drawing2.xml"]
        );
        assert!(relationships.parts("chart").is_empty());
    }

    #[test]
    fn targets_hold_safe_links_and_package_images() {
        let relationships = slide_relationships(&[
            ("rId1", "hyperlink", "https://example.com"),
            ("rId2", "hyperlink", "javascript:alert(1)"),
            ("rId3", "image", "../media/image1.png"),
            ("rId4", "image", "https://example.com/a.png"),
        ]);
        let targets = relationships.targets();
        assert_eq!(targets.links.len(), 1);
        assert_eq!(targets.links["rId1"], "https://example.com");
        assert_eq!(targets.images.len(), 1);
        assert_eq!(targets.images["rId3"], "ppt/media/image1.png");
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

    /// Records what a handler sees, and where.
    #[derive(Default)]
    struct Recorder {
        events: Vec<String>,
    }

    impl XmlHandler for Recorder {
        const SKIP: &'static [&'static str] = &["Fallback"];

        fn start(&mut self, e: &BytesStart, is_empty: bool, open: &Open) {
            let name = e.local_name();
            let name: &str = name.as_ref();
            let slash = if is_empty { "/" } else { "" };
            self.events.push(format!(
                "<{name}{slash}> in {:?}, inside r: {}",
                open.current(),
                open.inside("r")
            ));
        }

        fn end(&mut self, name: &str) {
            self.events.push(format!("</{name}>"));
        }

        fn text(&mut self, text: &str, open: &Open) {
            self.events
                .push(format!("{text:?} in {:?}", open.current()));
        }
    }

    #[test]
    fn walk_passes_the_open_elements_and_skips_their_contents() {
        let mut recorder = Recorder::default();
        walk(
            r#"<w:p><w:r><w:t>a&amp;b</w:t><w:br/></w:r><mc:Fallback><w:r><w:t>old</w:t></w:r></mc:Fallback></w:p>"#,
            &mut recorder,
        )
        .unwrap();
        assert_eq!(
            recorder.events,
            [
                "<p> in None, inside r: false",
                "<r> in Some(\"p\"), inside r: false",
                "<t> in Some(\"r\"), inside r: true",
                "\"a\" in Some(\"t\")",
                "\"&\" in Some(\"t\")",
                "\"b\" in Some(\"t\")",
                "</t>",
                "<br/> in Some(\"r\"), inside r: true",
                "</r>",
                "</p>",
            ]
        );
    }

    #[test]
    fn inside_stays_fast_however_deep_the_nesting() {
        /// Asks whether it's inside `r` at every element, as the readers do.
        struct Asker(usize);
        impl XmlHandler for Asker {
            fn start(&mut self, _: &BytesStart, _: bool, open: &Open) {
                self.0 += usize::from(open.inside("r"));
            }
            fn end(&mut self, _: &str) {}
            fn text(&mut self, _: &str, _: &Open) {}
        }

        // Searching the stack each time would take about 10^10 steps here.
        let depth = 200_000;
        let xml = format!("<r>{}{}</r>", "<a>".repeat(depth), "</a>".repeat(depth));
        let mut asker = Asker(0);
        walk(&xml, &mut asker).unwrap();
        assert_eq!(asker.0, depth);
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
