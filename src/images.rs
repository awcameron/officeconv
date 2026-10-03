//! What happens to the images in a `.docx`, `.pptx` or `.xlsx`: saved next to the Markdown
//! that links to them, kept in memory for a PDF, or left out.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use zip::ZipArchive;
use zip::result::ZipError;

use crate::document::{Block, resolve_images};
use crate::error::{ConvertError, Result};
use crate::output::{ensure_dir, safe_file_name};

/// Writes images into one folder, each at most once, and remembers the link to each.
#[derive(Debug)]
pub struct ImageExport {
    dir: PathBuf,
    /// Put in front of each file name to make the link, e.g. `"notes_images/"`.
    link_prefix: String,
    /// Image part -> the link already written for it.
    links: HashMap<String, String>,
    /// File names taken so far, lowercased because macOS and Windows ignore case.
    taken: HashSet<String>,
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
            taken: HashSet::new(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// How many images have been written.
    pub fn count(&self) -> usize {
        self.links.len()
    }

    /// Saves the image stored at `part` and returns its Markdown link.
    ///
    /// Returns `None` if the package doesn't contain that part.
    pub fn export<R: Read + Seek>(
        &mut self,
        archive: &mut ZipArchive<R>,
        part: &str,
    ) -> Result<Option<String>> {
        if let Some(link) = self.links.get(part) {
            return Ok(Some(link.clone()));
        }

        let Some(bytes) = read_part_bytes(archive, part)? else {
            return Ok(None);
        };

        // Only the last segment of the part name is used, so a name like
        // `../../etc/x` can't write outside the folder.
        let original = part.rsplit('/').next().unwrap_or(part);
        let name = self.unique_name(&safe_file_name(original));
        let path = self.dir.join(&name);
        fs::write(&path, bytes).map_err(|source| ConvertError::CreateOutput { path, source })?;

        let link = format!("{}{name}", self.link_prefix);
        self.links.insert(part.to_string(), link.clone());
        Ok(Some(link))
    }

    /// `image.png`, or `image-2.png` if that's taken, and so on.
    fn unique_name(&mut self, name: &str) -> String {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (name, String::new()),
        };

        let mut candidate = name.to_string();
        let mut n = 2;
        while !self.taken.insert(candidate.to_lowercase()) {
            candidate = format!("{stem}-{n}{ext}");
            n += 1;
        }
        candidate
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
        archive: &mut ZipArchive<R>,
        part: &str,
    ) -> Result<Option<String>> {
        if !self.bytes.contains_key(part) {
            let Some(bytes) = read_part_bytes(archive, part)? else {
                return Ok(None);
            };
            self.bytes.insert(part.to_string(), bytes);
        }
        Ok(Some(part.to_string()))
    }

    /// The bytes of the image that [`EmbeddedImages::add`] returned `key` for.
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

/// Handles the images the blocks refer to as `images` says, removing the ones it leaves out.
pub fn link_images<R: Read + Seek>(
    blocks: &mut Vec<Block>,
    archive: &mut ZipArchive<R>,
    images: Images<'_>,
) -> Result<()> {
    match images {
        Images::Skip => resolve_images(blocks, |_| Ok(None)),
        Images::Save(export) => resolve_images(blocks, |part| export.export(archive, part)),
        Images::Embed(embedded) => resolve_images(blocks, |part| embedded.add(archive, part)),
    }
}

/// The bytes of `part`, or `None` if the package doesn't contain it.
fn read_part_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    part: &str,
) -> Result<Option<Vec<u8>>> {
    let mut entry = match archive.by_name(part) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(ZipError::from)?;
    Ok(Some(bytes))
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
    fn archive(parts: &[(&str, &[u8])]) -> ZipArchive<std::io::Cursor<Vec<u8>>> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        ZipArchive::new(writer.finish().unwrap()).unwrap()
    }

    #[test]
    fn writes_each_image_once_with_unique_names() {
        let dir = TempDir::new().unwrap();
        let images = dir.path().join("img");
        let mut zip = archive(&[
            ("word/media/image1.png", b"one"),
            ("word/embeddings/Image1.PNG", b"two"),
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
        assert_eq!(fs::read(images.join("image1.png")).unwrap(), b"one");
        assert_eq!(fs::read(images.join("Image1-2.PNG")).unwrap(), b"two");
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
        let mut zip = archive(&[("../../evil.png", b"x")]);
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
