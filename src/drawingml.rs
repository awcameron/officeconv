//! Pictures in DrawingML, the drawing markup Word, PowerPoint and Excel share.
//!
//! A picture is laid out the same way in all three, apart from the elements around it:
//!
//! ```text
//! <wp:docPr descr="alt text">          (Word only: the drawing holding the picture)
//!   <a:hlinkClick r:id="rId3"/>        clicking the picture opens a link
//! <wp:extent cx="..." cy="..."/>       (Word only: the drawing's size)
//! <pic:pic> / <p:pic> / <xdr:pic>
//!   <..:cNvPr descr="alt text">        the picture's own alt text, and maybe a link
//!   <a:blip r:embed="rId2"/>           the image part
//!   <a:xfrm><a:ext cx="..." cy="..."/> its size
//! ```
//!
//! Each reader decides where a picture starts and when it's finished, because that differs:
//! PowerPoint and Excel give the size after the image, and one Word drawing can hold a group
//! of pictures. [`Picture`] decides what each element says about the picture.

use quick_xml::events::BytesStart;

use crate::document::builder::image_run;
use crate::document::{ImagePart, Run, display_size};
use crate::opc::{Open, Targets, attr};

/// A picture being read.
///
/// Word gives alt text, a size and a link on the drawing (`wp:docPr`, `wp:extent`) and again
/// inside the picture, and the drawing's win. Otherwise the last element to say wins.
#[derive(Debug, Default)]
pub struct Picture {
    alt: Ranked<String>,
    part: Option<String>,
    size: Ranked<(u32, u32)>,
    link: Ranked<String>,
}

/// Where a value came from: the picture itself, or a Word drawing around it, which wins.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    #[default]
    Unset,
    Picture,
    Drawing,
}

/// A value, and the source that set it.
#[derive(Debug)]
struct Ranked<T> {
    value: Option<T>,
    source: Source,
}

impl<T> Default for Ranked<T> {
    fn default() -> Self {
        Ranked {
            value: None,
            source: Source::Unset,
        }
    }
}

impl<T> Ranked<T> {
    /// Keeps `value` unless a source that wins over `source` has already set one.
    fn offer(&mut self, source: Source, value: Option<T>) {
        if source >= self.source {
            *self = Ranked { value, source };
        }
    }

    fn take(&mut self) -> Option<T> {
        std::mem::take(self).value
    }
}

impl Picture {
    /// Notes what `e`, an element inside the picture, says about it. Images and links are
    /// looked up in `targets`, the relationships of the part being read.
    pub fn read(&mut self, e: &BytesStart, open: &Open, targets: &Targets) {
        let in_drawing = open.inside("inline") || open.inside("anchor");
        match e.local_name().as_ref() {
            name @ ("docPr" | "cNvPr") => {
                let source = if name == "docPr" {
                    Source::Drawing
                } else {
                    Source::Picture
                };
                let alt = attr(e, "descr").or_else(|| attr(e, "title"));
                self.alt.offer(source, alt);
            }
            "blip" => {
                self.part = attr(e, "embed").and_then(|id| targets.images.get(&id).cloned());
            }
            "extent" if in_drawing => {
                let size = display_size(attr(e, "cx"), attr(e, "cy"));
                self.size.offer(Source::Drawing, size);
            }
            // Extensions (`a:extLst`) also have `a:ext` elements, which aren't sizes.
            "ext" if open.inside("xfrm") => {
                let size = display_size(attr(e, "cx"), attr(e, "cy"));
                self.size.offer(Source::Picture, size);
            }
            // A text run's link is in its `a:rPr`. A Word drawing sits in a run, so its link
            // is the one in `wp:docPr`.
            "hlinkClick" => {
                let source = if open.inside("docPr") {
                    Source::Drawing
                } else if open.inside("r") || open.inside("fld") {
                    return;
                } else {
                    Source::Picture
                };
                let link = attr(e, "id").and_then(|id| targets.links.get(&id).cloned());
                self.link.offer(source, link);
            }
            _ => {}
        }
    }

    /// The image run for the picture, or `None` if it isn't an image stored in the package.
    ///
    /// Takes the image with its alt text and size, so another image read into the same
    /// picture, as in a Word group, doesn't repeat them. The link stays, for the whole group.
    pub fn take_run(&mut self) -> Option<Run<ImagePart>> {
        let alt = self.alt.take().unwrap_or_default();
        let size = self.size.take();
        let part = self.part.take()?;
        Some(image_run(part, alt, size, self.link.value.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::opc::{self, XmlHandler};

    /// Reads every element inside a `pic` into a picture, as PowerPoint and Excel do.
    struct Reader<'a> {
        targets: &'a Targets,
        picture: Option<Picture>,
        runs: Vec<Run<ImagePart>>,
    }

    impl XmlHandler for Reader<'_> {
        fn start(&mut self, e: &BytesStart, _: bool, open: &Open) {
            if e.local_name().as_ref() == "pic" {
                self.picture = Some(Picture::default());
            } else if let Some(picture) = self.picture.as_mut() {
                picture.read(e, open, self.targets);
            }
        }

        fn end(&mut self, name: &str) {
            if name == "pic"
                && let Some(run) = self.picture.take().and_then(|mut p| p.take_run())
            {
                self.runs.push(run);
            }
        }

        fn text(&mut self, _: &str, _: &Open) {}
    }

    fn read(xml: &str) -> Vec<Run<ImagePart>> {
        let targets = Targets {
            links: HashMap::from([("rId3".into(), "https://example.com".into())]),
            images: HashMap::from([("rId2".into(), "ppt/media/image1.png".into())]),
        };
        let mut reader = Reader {
            targets: &targets,
            picture: None,
            runs: Vec::new(),
        };
        opc::walk(xml, &mut reader).unwrap();
        reader.runs
    }

    fn pic(properties: &str, embed: &str, size: &str) -> String {
        format!(
            r#"<p:pic xmlns:p="p" xmlns:a="a" xmlns:r="r"><p:nvPicPr>{properties}</p:nvPicPr><p:blipFill><a:blip r:embed="{embed}"/></p:blipFill><p:spPr>{size}</p:spPr></p:pic>"#
        )
    }

    fn image(alt: &str, size: Option<(u32, u32)>) -> Run<ImagePart> {
        image_run("ppt/media/image1.png".into(), alt.into(), size, None)
    }

    #[test]
    fn reads_alt_text_from_the_description_then_the_title() {
        let runs = read(&format!(
            "{}{}{}",
            pic(
                r#"<p:cNvPr id="1" name="a" descr="Chart" title="T"/>"#,
                "rId2",
                ""
            ),
            pic(r#"<p:cNvPr id="2" name="b" title="Logo"/>"#, "rId2", ""),
            pic(r#"<p:cNvPr id="3" name="c"/>"#, "rId2", ""),
        ));
        assert_eq!(
            runs,
            [image("Chart", None), image("Logo", None), image("", None)]
        );
    }

    #[test]
    fn reads_the_size_from_the_transform_not_an_extension() {
        let size = r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="952500" cy="476250"/></a:xfrm><a:extLst><a:ext uri="{X}" cx="1" cy="1"/></a:extLst>"#;
        assert_eq!(
            read(&pic(r#"<p:cNvPr id="1" name="a"/>"#, "rId2", size)),
            [image("", Some((952_500, 476_250)))]
        );
    }

    #[test]
    fn reads_the_pictures_link_but_not_a_missing_one() {
        let linked = r#"<p:cNvPr id="1" name="a"><a:hlinkClick r:id="rId3"/></p:cNvPr>"#;
        let unknown = r#"<p:cNvPr id="1" name="a"><a:hlinkClick r:id="rId9"/></p:cNvPr>"#;
        assert_eq!(
            read(&format!(
                "{}{}",
                pic(linked, "rId2", ""),
                pic(unknown, "rId2", "")
            )),
            [
                image("", None).linked("https://example.com"),
                image("", None)
            ]
        );
    }

    #[test]
    fn leaves_out_an_image_not_stored_in_the_package() {
        assert!(
            read(&pic(
                r#"<p:cNvPr id="1" name="a" descr="Gone"/>"#,
                "rId9",
                ""
            ))
            .is_empty()
        );
    }

    #[test]
    fn word_drawing_properties_win_over_the_pictures_own() {
        // A Word drawing gives alt text and size before the picture repeats them.
        let xml = r#"<wp:inline xmlns:wp="wp" xmlns:a="a" xmlns:pic="pic" xmlns:r="r"><wp:extent cx="952500" cy="952500"/><wp:docPr id="1" name="d" descr="Drawing"><a:hlinkClick r:id="rId3"/></wp:docPr><a:graphic><a:graphicData><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="p" descr="Picture"/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId2"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx="9525" cy="9525"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline>"#;
        let targets = Targets {
            links: HashMap::from([("rId3".into(), "https://example.com".into())]),
            images: HashMap::from([("rId2".into(), "word/media/image1.png".into())]),
        };

        struct Drawing<'a>(&'a Targets, Picture);
        impl XmlHandler for Drawing<'_> {
            fn start(&mut self, e: &BytesStart, _: bool, open: &Open) {
                self.1.read(e, open, self.0);
            }
            fn end(&mut self, _: &str) {}
            fn text(&mut self, _: &str, _: &Open) {}
        }
        let mut drawing = Drawing(&targets, Picture::default());
        opc::walk(xml, &mut drawing).unwrap();

        assert_eq!(
            drawing.1.take_run(),
            Some(
                image_run(
                    "word/media/image1.png".into(),
                    "Drawing".into(),
                    Some((952_500, 952_500)),
                    None
                )
                .linked("https://example.com")
            )
        );
    }
}
