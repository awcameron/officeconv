//! Rendering [`Block`]s as a PDF.
//!
//! The PDF shows the converted content, laid out simply: it doesn't reproduce Word's or
//! PowerPoint's own fonts, colors, margins or slide designs. Text is set in Noto Sans, which is
//! built in (see [`fonts`]).

pub mod fonts;
pub mod layout;

use krilla::Document;
use krilla::color::luma;
use krilla::geom::{PathBuilder, Point};
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;

use crate::document::Block;
use crate::error::{ConvertError, Result};
use fonts::Fonts;
use layout::{Item, Layout, Page, PageSetup};

/// A finished PDF.
#[derive(Debug)]
pub struct Rendered {
    pub pdf: Vec<u8>,
}

/// Lays out `blocks` on pages shaped by `setup` and writes them as a PDF.
pub fn render(blocks: &[Block], setup: PageSetup) -> Result<Rendered> {
    let fonts = Fonts::new();
    let pages = Layout::new(&fonts, setup).run(blocks);

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

    Ok(Rendered { pdf })
}

fn paint_page(document: &mut Document, settings: PageSettings, page: Page, fonts: &Fonts) {
    let mut pdf_page = document.start_page_with(settings);
    let mut surface = pdf_page.surface();
    for item in page.items {
        paint_item(&mut surface, item, fonts);
    }
    surface.finish();
    pdf_page.finish();
}

fn paint_item(surface: &mut Surface<'_>, item: Item, fonts: &Fonts) {
    match item {
        Item::Text(text) => {
            surface.set_fill(Some(Fill::default()));
            surface.draw_glyphs(
                Point::from_xy(text.x, text.baseline),
                &text.glyphs,
                fonts.pdf_font(text.font),
                &text.text,
                text.size,
                false,
            );
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
    }
}
