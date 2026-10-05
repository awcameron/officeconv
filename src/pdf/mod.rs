//! Rendering [`Block`]s as a PDF.
//!
//! The PDF shows the converted content, laid out simply: it doesn't reproduce Word's or
//! PowerPoint's own fonts, colors, margins or slide designs. Text is set in Noto Sans, which is
//! built in, with installed fonts filling in characters it lacks (see [`fonts`]).

pub mod fonts;
pub mod layout;

use std::collections::BTreeSet;

use krilla::Document;
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::{luma, rgb};
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;

use crate::document::Block;
use crate::error::{ConvertError, Result};
use crate::images::EmbeddedImages;
use fonts::Fonts;
use layout::{Item, Layout, Page, PageSetup, SkippedImages};

/// A finished PDF, and what couldn't go into it.
#[derive(Debug)]
pub struct Rendered {
    pub pdf: Vec<u8>,
    /// Characters that no font had, drawn as boxes.
    pub missing_chars: BTreeSet<char>,
    /// Images left out, and why.
    pub skipped_images: SkippedImages,
}

/// Lays out `blocks` on pages shaped by `setup` and writes them as a PDF. Image runs hold keys
/// into `images`.
pub fn render(blocks: &[Block], images: &EmbeddedImages, setup: PageSetup) -> Result<Rendered> {
    let mut fonts = Fonts::new();
    let (pages, skipped_images) = Layout::new(&mut fonts, images, setup).run(blocks);

    let mut document = Document::new();
    document
        .set_metadata(Metadata::new().creator(format!("officeconv {}", env!("CARGO_PKG_VERSION"))));
    let settings = PageSettings::from_wh(setup.width, setup.height)
        .expect("page sizes are positive and finite");
    for page in pages {
        paint_page(&mut document, settings.clone(), page, &fonts);
    }
    let pdf = document
        .finish()
        .map_err(|err| ConvertError::Pdf(err.to_string()))?;

    Ok(Rendered {
        pdf,
        missing_chars: fonts.missing().clone(),
        skipped_images,
    })
}

/// The color of link text.
const LINK_BLUE: (u8, u8, u8) = (0x1a, 0x5f, 0xb4);

fn paint_page(document: &mut Document, settings: PageSettings, page: Page, fonts: &Fonts) {
    let mut pdf_page = document.start_page_with(settings);
    let mut surface = pdf_page.surface();
    for item in page.items {
        paint_item(&mut surface, item, fonts);
    }
    surface.finish();

    for link in page.links {
        let Some(rect) = Rect::from_xywh(link.x, link.y, link.width, link.height) else {
            continue;
        };
        let target = Target::Action(Action::Link(LinkAction::new(link.url)));
        pdf_page.add_annotation(Annotation::new_link(
            LinkAnnotation::new(rect, target),
            None,
        ));
    }
    pdf_page.finish();
}

fn paint_item(surface: &mut Surface<'_>, item: Item, fonts: &Fonts) {
    match item {
        Item::Text(text) => {
            let fill = if text.link {
                let (r, g, b) = LINK_BLUE;
                Fill {
                    paint: rgb::Color::new(r, g, b).into(),
                    ..Fill::default()
                }
            } else {
                Fill::default()
            };
            surface.set_fill(Some(fill));
            surface.draw_glyphs(
                Point::from_xy(text.x, text.baseline),
                &text.glyphs,
                fonts.pdf_font(text.font),
                &text.text,
                text.size,
                false,
            );
        }
        Item::Image {
            x,
            y,
            width,
            height,
            image,
        } => {
            let Some(size) = Size::from_wh(width, height) else {
                return;
            };
            surface.push_transform(&Transform::from_translate(x, y));
            surface.draw_image(image, size);
            surface.pop();
        }
        Item::Line { x1, y1, x2, y2 } => {
            let mut path = PathBuilder::new();
            path.move_to(x1, y1);
            path.line_to(x2, y2);
            let Some(path) = path.finish() else {
                return;
            };
            surface.set_fill(None);
            surface.set_stroke(Some(Stroke {
                paint: luma::Color::new(150).into(),
                width: 0.5,
                ..Stroke::default()
            }));
            surface.draw_path(&path);
            surface.set_stroke(None);
        }
        Item::Shade {
            x,
            y,
            width,
            height,
        } => {
            let Some(rect) = Rect::from_xywh(x, y, width, height) else {
                return;
            };
            let mut path = PathBuilder::new();
            path.push_rect(rect);
            let Some(path) = path.finish() else {
                return;
            };
            surface.set_fill(Some(Fill {
                paint: luma::Color::new(238).into(),
                ..Fill::default()
            }));
            surface.draw_path(&path);
        }
    }
}
