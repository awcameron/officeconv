//! PROTOTYPE (#29): rendering [`Block`]s as a PDF through Typst instead of our own layout.
//!
//! Not for merging. It exists to measure Typst as a library against `crate::pdf`: binary size,
//! build time, speed, output, and how much code it takes.
//!
//! The blocks become Typst markup, which is compiled in an in-memory [`World`] and exported with
//! `typst-pdf`. All document text goes into Typst string literals (`#"..."`), so a document can't
//! inject Typst code.

use std::collections::HashMap;
use std::fmt::Write as _;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

use crate::document::{Block, Cell, ListKind, Run};
use crate::error::{ConvertError, Result};
use crate::images::EmbeddedImages;

/// Noto Sans, the same built-in font the krilla backend uses.
const NOTO_SANS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Italic.ttf"),
    include_bytes!("../assets/fonts/NotoSans-BoldItalic.ttf"),
];

/// Page setup, matching `pdf::layout::PageSetup`.
#[derive(Debug, Clone, Copy)]
pub enum Pages {
    Document,
    Slides,
}

/// Renders `blocks` as a PDF through Typst. Image runs hold keys into `images`.
pub fn render(blocks: &[Block], images: &EmbeddedImages, pages: Pages) -> Result<Vec<u8>> {
    let mut files = HashMap::new();
    let markup = to_markup(blocks, pages, images, &mut files);
    let world = DocWorld::new(markup, files);

    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| ConvertError::Pdf(describe(&errors)))?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|errors| ConvertError::Pdf(describe(&errors)))
}

fn describe(errors: &[typst::diag::SourceDiagnostic]) -> String {
    errors
        .iter()
        .map(|e| e.message.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The Typst source for `blocks`. Images are added to `files` under `/img/N`.
fn to_markup(
    blocks: &[Block],
    pages: Pages,
    images: &EmbeddedImages,
    files: &mut HashMap<FileId, Bytes>,
) -> String {
    let mut out = String::new();
    match pages {
        Pages::Document => {
            out.push_str("#set page(paper: \"a4\", margin: 1in)\n#set text(font: \"Noto Sans\", size: 11pt)\n")
        }
        Pages::Slides => out.push_str(
            "#set page(width: 960pt, height: 540pt, margin: 48pt)\n#set text(font: \"Noto Sans\", size: 16pt)\n",
        ),
    }
    // Headings bold and sized like the krilla backend; no numbering.
    out.push_str("#set par(spacing: 0.7em)\n#show heading: set block(above: 1em, below: 0.6em)\n");
    out.push_str("#set table(stroke: 0.5pt + gray, inset: 0.4em)\n#show table.cell.where(y: 0): set text(weight: \"bold\")\n#show table.cell.where(y: 0): set table.cell(fill: luma(238))\n\n");

    let mut ctx = Context { images, files };
    for block in blocks {
        match block {
            Block::Heading { level, runs } => {
                let _ = writeln!(out, "#heading(level: {level})[{}]\n", ctx.runs(runs));
            }
            Block::Paragraph(runs) => {
                let _ = writeln!(out, "{}\n", ctx.runs(runs));
            }
            Block::ListItem { kind, level, runs } => {
                let marker = match kind {
                    ListKind::Bullet => "-",
                    ListKind::Numbered => "+",
                };
                let indent = "  ".repeat(usize::from(*level));
                let _ = writeln!(out, "{indent}{marker} {}", ctx.runs(runs));
            }
            Block::Table(rows) => out.push_str(&ctx.table(rows)),
            Block::Rule => match pages {
                Pages::Slides => out.push_str("\n#pagebreak(weak: true)\n"),
                Pages::Document => out.push_str("\n#line(length: 100%, stroke: 0.5pt + gray)\n"),
            },
        }
    }
    out
}

struct Context<'a> {
    images: &'a EmbeddedImages,
    files: &'a mut HashMap<FileId, Bytes>,
}

impl Context<'_> {
    fn runs(&mut self, runs: &[Run]) -> String {
        runs.iter().map(|run| self.run(run)).collect()
    }

    fn run(&mut self, run: &Run) -> String {
        let mut content = match &run.image {
            Some(key) => match self.image(key) {
                Some(path) => format!("#block(image({}, width: auto))", string(&path)),
                None => return String::new(),
            },
            None => {
                // Line breaks become `#linebreak()`; everything else is a string literal.
                let parts: Vec<String> = run
                    .text
                    .split('\n')
                    .map(|t| format!("#{}", string(t)))
                    .collect();
                parts.join("#linebreak()")
            }
        };
        if run.style.bold {
            content = format!("#strong[{content}]");
        }
        if run.style.italic {
            content = format!("#emph[{content}]");
        }
        if let Some(url) = &run.link {
            content = format!("#link({})[{content}]", string(url));
        }
        content
    }

    /// Adds an image's bytes as a virtual file and returns its path. Typst decides whether it
    /// can read the format.
    fn image(&mut self, key: &str) -> Option<String> {
        let bytes = self.images.get(key)?;
        let path = format!("/img/{}", self.files.len());
        self.files
            .insert(file_id(&path), Bytes::new(bytes.to_vec()));
        Some(path)
    }

    fn table(&mut self, rows: &[Vec<Cell>]) -> String {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        let cell = |ctx: &mut Self, cell: Option<&Cell>| {
            format!("[{}]", cell.map(|c| ctx.runs(c)).unwrap_or_default())
        };
        let mut out = format!("#table(columns: {columns},\n");
        for (r, row) in rows.iter().enumerate() {
            let cells: Vec<String> = (0..columns).map(|c| cell(self, row.get(c))).collect();
            if r == 0 {
                let _ = writeln!(out, "  table.header({}),", cells.join(", "));
            } else {
                let _ = writeln!(out, "  {},", cells.join(", "));
            }
        }
        out.push_str(")\n\n");
        out
    }
}

/// A Typst string literal holding `text`.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn file_id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("our paths are valid"),
    )
    .intern()
}

/// Everything Typst may read: the generated source, the images, and fonts. Nothing on disk
/// except installed fonts.
struct DocWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl DocWorld {
    fn new(markup: String, files: HashMap<FileId, Bytes>) -> Self {
        let mut fonts = FontStore::new();
        for data in NOTO_SANS {
            for font in Font::iter(Bytes::new(data)) {
                let info = font.info().clone();
                fonts.push((font, info));
            }
        }
        // Installed fonts fill in what Noto Sans lacks, as in the krilla backend. Typst scans
        // them up front, on every run.
        // macOS's LastResort draws a box for every character; skip it, as the krilla backend does.
        fonts.extend(
            typst_kit::fonts::system()
                .filter(|(_, info)| !info.family.to_lowercase().contains("lastresort")),
        );

        DocWorld {
            library: LazyHash::new(Library::default()),
            fonts,
            main: Source::new(file_id("/main.typ"), markup),
            files,
        }
    }
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}
