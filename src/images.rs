//! What happens to the images in a `.docx`, `.pptx` or `.xlsx`: saved next to the Markdown
//! that links to them, kept in memory for a PDF, or left out.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::document::{Block, ImagePart, resolve_images};
use crate::error::{ConvertError, Result};
use crate::opc::Archive;
use crate::output::{UniqueNames, ensure_dir, safe_file_name};

/// Writes images into one folder, each at most once, and remembers the link to each.
#[derive(Debug)]
pub struct ImageExport {
    dir: PathBuf,
    /// Put in front of each file name to make the link, e.g. `"notes_images/"`.
    link_prefix: String,
    /// Image part -> the link already written for it.
    links: HashMap<String, String>,
    /// File names already used in `dir`.
    names: UniqueNames,
    /// Parts left out because they aren't images in a format we save.
    skipped: HashSet<String>,
}

impl ImageExport {
    /// Prepares to save images into `dir` (created if needed), linked from Markdown files
    /// in `markdown_dir`.
    pub fn new(dir: &Path, markdown_dir: &Path) -> Result<Self> {
        ensure_dir(dir)?;
        Ok(ImageExport {
            dir: dir.to_path_buf(),
            link_prefix: link_prefix(dir, markdown_dir),
            links: HashMap::new(),
            names: UniqueNames::default(),
            skipped: HashSet::new(),
        })
    }

    /// How many images have been written.
    pub fn count(&self) -> usize {
        self.links.len()
    }

    /// How many parts were left out because they aren't images in a format we save.
    pub fn skipped(&self) -> usize {
        self.skipped.len()
    }

    /// Prints how many images were saved, and how many parts were left out, to stderr.
    pub fn report(&self) {
        eprintln!("saved {} images to {}", self.count(), self.dir.display());
        if self.skipped() > 0 {
            eprintln!(
                "warning: left out {} files that aren't PNG, JPEG, GIF, WebP, BMP, TIFF, EMF or \
                 WMF images",
                self.skipped()
            );
        }
    }

    /// Saves the image stored at `part` and returns its Markdown link.
    ///
    /// Only images in a format [`ImageFormat`] recognizes are saved, named with that format's
    /// extension: the document chooses both the bytes and the name, so otherwise it could put
    /// any file, such as an `.html` page with a script, into the folder.
    ///
    /// Returns `None` if the package doesn't contain that part, or it isn't such an image.
    pub fn export<R: Read + Seek>(
        &mut self,
        archive: &mut Archive<R>,
        part: &str,
    ) -> Result<Option<String>> {
        if let Some(link) = self.links.get(part) {
            return Ok(Some(link.clone()));
        }
        if self.skipped.contains(part) {
            return Ok(None);
        }

        let Some(bytes) = archive.read_bytes(part)? else {
            return Ok(None);
        };
        let Some(format) = ImageFormat::detect(&bytes) else {
            self.skipped.insert(part.to_string());
            return Ok(None);
        };

        // Only the last segment of the part name is used, so a name like
        // `../../etc/x` can't write outside the folder.
        let original = part.rsplit('/').next().unwrap_or(part);
        let name = self
            .names
            .claim(&format.file_name(&safe_file_name(original)));
        let path = self.dir.join(&name);
        fs::write(&path, bytes).map_err(|source| ConvertError::CreateOutput { path, source })?;

        let link = format!("{}{name}", self.link_prefix);
        self.links.insert(part.to_string(), link.clone());
        Ok(Some(link))
    }
}

/// An image format, recognized by a file's first bytes rather than its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Tiff,
    /// Windows' vector formats, common in Office documents.
    Emf,
    Wmf,
}

impl ImageFormat {
    /// Recognizes an image by its signature, or `None` for anything else.
    ///
    /// SVG isn't recognized on purpose: it can contain script.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        let starts = |signature: &[u8]| bytes.starts_with(signature);
        let at = |offset: usize, signature: &[u8]| {
            bytes.get(offset..offset + signature.len()) == Some(signature)
        };
        if starts(b"\x89PNG\r\n\x1a\n") {
            Some(ImageFormat::Png)
        } else if starts(b"\xFF\xD8\xFF") {
            Some(ImageFormat::Jpeg)
        } else if starts(b"GIF87a") || starts(b"GIF89a") {
            Some(ImageFormat::Gif)
        } else if starts(b"RIFF") && at(8, b"WEBP") {
            Some(ImageFormat::Webp)
        } else if starts(b"BM") {
            Some(ImageFormat::Bmp)
        } else if starts(b"II*\0") || starts(b"MM\0*") {
            Some(ImageFormat::Tiff)
        } else if starts(&[1, 0, 0, 0]) && at(40, b" EMF") {
            Some(ImageFormat::Emf)
        } else if starts(&[0xD7, 0xCD, 0xC6, 0x9A])
            || starts(&[1, 0, 9, 0])
            || starts(&[2, 0, 9, 0])
        {
            // A "placeable" WMF, or a plain one in memory or on disk.
            Some(ImageFormat::Wmf)
        } else {
            None
        }
    }

    /// The extensions this format's files use. The first is used when a name has none of them.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            ImageFormat::Png => &["png"],
            ImageFormat::Jpeg => &["jpg", "jpeg", "jpe"],
            ImageFormat::Gif => &["gif"],
            ImageFormat::Webp => &["webp"],
            ImageFormat::Bmp => &["bmp", "dib"],
            ImageFormat::Tiff => &["tiff", "tif"],
            ImageFormat::Emf => &["emf"],
            ImageFormat::Wmf => &["wmf"],
        }
    }

    /// `name`, with this format's extension if it doesn't already have one of them:
    /// `logo.html` holding a PNG becomes `logo.png`, while `photo.jpeg` stays as it is.
    fn file_name(self, name: &str) -> String {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
            _ => (name, None),
        };
        let extensions = self.extensions();
        match ext {
            Some(ext) if extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)) => name.to_string(),
            _ => format!("{stem}.{}", extensions[0]),
        }
    }
}

/// Image bytes kept in memory, keyed by their part inside the package, for embedding in a PDF.
#[derive(Debug, Default)]
pub struct EmbeddedImages {
    bytes: HashMap<String, Vec<u8>>,
}

impl EmbeddedImages {
    /// Reads the image stored at `part` (once) and returns the key to look it up by.
    ///
    /// Returns `None` if the package doesn't contain that part.
    pub fn add<R: Read + Seek>(
        &mut self,
        archive: &mut Archive<R>,
        part: &str,
    ) -> Result<Option<String>> {
        if !self.bytes.contains_key(part) {
            let Some(bytes) = archive.read_bytes(part)? else {
                return Ok(None);
            };
            self.bytes.insert(part.to_string(), bytes);
        }
        Ok(Some(part.to_string()))
    }

    /// The bytes of the image that [`EmbeddedImages::add`] returned `key` for.
    #[cfg(any(feature = "pdf", test))]
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.bytes.get(key).map(Vec::as_slice)
    }
}

/// What to do with the images a document refers to.
#[derive(Debug)]
pub enum Images<'a> {
    /// Leave them out.
    Skip,
    /// Save them into a folder and link them from the Markdown.
    Save(&'a mut ImageExport),
    /// Keep their bytes in memory; each image run then holds its key in [`EmbeddedImages`].
    Embed(&'a mut EmbeddedImages),
}

/// Handles the images in blocks a reader returned as `images` says, reading them from the
/// `archive` the reader read, so its size limits count them too. Returns the blocks with each
/// image's link or key, and without the ones it leaves out.
pub fn resolve<R: Read + Seek>(
    blocks: Vec<Block<ImagePart>>,
    archive: &mut Archive<R>,
    images: Images<'_>,
) -> Result<Vec<Block>> {
    match images {
        Images::Skip => resolve_images(blocks, |_| Ok(None)),
        Images::Save(export) => resolve_images(blocks, |part| export.export(archive, part)),
        Images::Embed(embedded) => resolve_images(blocks, |part| embedded.add(archive, part)),
    }
}

/// The path from `markdown_dir` to `images_dir`, with `/` separators and a trailing `/`
/// (or empty when they're the same folder).
///
/// If `images_dir` isn't inside `markdown_dir`, the link is its absolute path.
fn link_prefix(images_dir: &Path, markdown_dir: &Path) -> String {
    let relative = match (fs::canonicalize(images_dir), fs::canonicalize(markdown_dir)) {
        (Ok(dir), Ok(base)) => match dir.strip_prefix(&base) {
            Ok(inside) => inside.to_path_buf(),
            Err(_) => dir,
        },
        _ => images_dir.to_path_buf(),
    };

    let text = relative.to_string_lossy().replace('\\', "/");
    // Windows' canonical paths start with `//?/`, which browsers don't understand.
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    if text.is_empty() || text == "." {
        String::new()
    } else {
        format!("{}/", text.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    /// A zip holding the given parts.
    fn archive(parts: &[(&str, &[u8])]) -> Archive<std::io::Cursor<Vec<u8>>> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        Archive::open(writer.finish().unwrap()).unwrap()
    }

    /// A PNG signature followed by `rest`: enough for [`ImageFormat::detect`].
    fn png(rest: &[u8]) -> Vec<u8> {
        [b"\x89PNG\r\n\x1a\n".as_slice(), rest].concat()
    }

    #[test]
    fn writes_each_image_once_with_unique_names() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("img");
        let (one, two) = (png(b"one"), png(b"two"));
        let mut zip = archive(&[
            ("word/media/image1.png", &one),
            ("word/embeddings/Image1.PNG", &two),
        ]);
        let mut export = ImageExport::new(&images, dir.path()).unwrap();

        let first = export.export(&mut zip, "word/media/image1.png").unwrap();
        let again = export.export(&mut zip, "word/media/image1.png").unwrap();
        let clash = export
            .export(&mut zip, "word/embeddings/Image1.PNG")
            .unwrap();
        let missing = export.export(&mut zip, "word/media/nope.png").unwrap();

        assert_eq!(first.as_deref(), Some("img/image1.png"));
        assert_eq!(again, first);
        assert_eq!(clash.as_deref(), Some("img/Image1-2.PNG"));
        assert_eq!(missing, None);
        assert_eq!(export.count(), 2);
        assert_eq!(fs::read(images.join("image1.png")).unwrap(), one);
        assert_eq!(fs::read(images.join("Image1-2.PNG")).unwrap(), two);
    }

    #[test]
    fn saves_only_images_and_names_them_by_their_format() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("img");
        let mut emf = vec![1, 0, 0, 0];
        emf.resize(40, 0);
        emf.extend_from_slice(b" EMF");
        let (logo, jpeg) = (png(b"logo"), b"\xFF\xD8\xFFjpeg".to_vec());
        let mut zip = archive(&[
            ("word/media/index.html", b"<script>alert(1)</script>"),
            ("word/media/logo.html", &logo),
            ("word/media/photo.jpeg", &jpeg),
            ("word/media/chart.emf", &emf),
            ("word/media/clip.wmf", &[0xD7, 0xCD, 0xC6, 0x9A, 0]),
            ("word/media/scan.tif", b"II*\0scan"),
            ("word/media/icon", &logo),
        ]);
        let mut export = ImageExport::new(&images, dir.path()).unwrap();
        let mut link = |part: &str| export.export(&mut zip, part).unwrap();

        assert_eq!(link("word/media/index.html"), None);
        assert_eq!(link("word/media/index.html"), None);
        assert_eq!(
            link("word/media/logo.html").as_deref(),
            Some("img/logo.png")
        );
        assert_eq!(
            link("word/media/photo.jpeg").as_deref(),
            Some("img/photo.jpeg")
        );
        assert_eq!(
            link("word/media/chart.emf").as_deref(),
            Some("img/chart.emf")
        );
        assert_eq!(link("word/media/clip.wmf").as_deref(), Some("img/clip.wmf"));
        assert_eq!(link("word/media/scan.tif").as_deref(), Some("img/scan.tif"));
        assert_eq!(link("word/media/icon").as_deref(), Some("img/icon.png"));
        assert_eq!((export.count(), export.skipped()), (6, 1));
        assert!(!images.join("index.html").exists());
    }

    #[test]
    fn detects_formats_by_their_signature_not_their_name() {
        for (bytes, format) in [
            (b"\x89PNG\r\n\x1a\nx".as_slice(), Some(ImageFormat::Png)),
            (b"\xFF\xD8\xFF\xE0", Some(ImageFormat::Jpeg)),
            (b"GIF89a", Some(ImageFormat::Gif)),
            (b"RIFF\0\0\0\0WEBPVP8 ", Some(ImageFormat::Webp)),
            (b"BM\0\0", Some(ImageFormat::Bmp)),
            (b"MM\0*", Some(ImageFormat::Tiff)),
            (b"\x01\0\x09\0", Some(ImageFormat::Wmf)),
            (b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", None),
            (b"<html>", None),
            (b"RIFF\0\0\0\0WAVE", None),
            (b"", None),
        ] {
            assert_eq!(ImageFormat::detect(bytes), format, "{bytes:?}");
        }
    }

    #[test]
    fn embeds_each_image_once_by_its_part() {
        let mut zip = archive(&[("word/media/image1.png", b"one")]);
        let mut embedded = EmbeddedImages::default();

        let key = embedded.add(&mut zip, "word/media/image1.png").unwrap();
        let again = embedded.add(&mut zip, "word/media/image1.png").unwrap();
        let missing = embedded.add(&mut zip, "word/media/nope.png").unwrap();

        assert_eq!(key.as_deref(), Some("word/media/image1.png"));
        assert_eq!(again, key);
        assert_eq!(missing, None);
        assert_eq!(embedded.get("word/media/image1.png"), Some(&b"one"[..]));
        assert_eq!(embedded.get("word/media/nope.png"), None);
    }

    #[test]
    fn never_writes_outside_the_folder() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("img");
        let evil = png(b"x");
        let mut zip = archive(&[("../../evil.png", &evil)]);
        let mut export = ImageExport::new(&images, dir.path()).unwrap();

        export.export(&mut zip, "../../evil.png").unwrap();

        assert!(images.join("evil.png").is_file());
        assert!(!dir.path().join("evil.png").exists());
    }

    #[test]
    fn links_relative_to_the_markdown_folder() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("out");
        let images = out.join("media");
        fs::create_dir_all(&images).unwrap();

        assert_eq!(link_prefix(&images, &out), "media/");
        assert_eq!(link_prefix(&out, &out), "");

        // Not inside the Markdown file's folder: use the absolute path.
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let prefix = link_prefix(&elsewhere, &out);
        assert!(prefix.ends_with("/elsewhere/"), "{prefix}");
        assert!(!prefix.starts_with("//?/"), "{prefix}");
    }
}
