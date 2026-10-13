//! The images that go into a PDF, decoded once each.
//!
//! An image's bytes come from the document, so before decoding one this checks it's in a
//! format a PDF can hold and isn't larger than [`MAX_IMAGE_PIXELS`]. Layout asks for an image
//! by its key, and gets it or nothing.

use std::collections::HashMap;

use krilla::Data;
use krilla::image::Image;

use crate::images::{EmbeddedImages, ImageFormat};

/// The most pixels an image may have to go in a PDF.
///
/// krilla decodes the whole image, at up to 16 bytes per pixel while it does (a 16-bit RGBA
/// PNG), and a file of a few bytes can say it's any size. 50 megapixels holds a 48 MP phone
/// photo, and keeps the worst case at about 800 MB.
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;

/// How many images were left out of the PDF, and why. Each image is counted once, however
/// often the document shows it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SkippedImages {
    /// In a format a PDF can't hold (such as EMF or TIFF), or not a readable image.
    pub unsupported: usize,
    /// More than [`MAX_IMAGE_PIXELS`].
    pub too_large: usize,
}

/// Why [`decode_image`] left an image out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftOut {
    Unsupported,
    TooLarge,
}

/// The images an [`EmbeddedImages`] holds, decoded as layout asks for them.
pub struct DecodedImages<'a> {
    images: &'a EmbeddedImages,
    /// Decoded images by key; `None` for ones left out of the PDF.
    decoded: HashMap<String, Option<Image>>,
    skipped: SkippedImages,
}

impl<'a> DecodedImages<'a> {
    pub fn new(images: &'a EmbeddedImages) -> Self {
        DecodedImages {
            images,
            decoded: HashMap::new(),
            skipped: SkippedImages::default(),
        }
    }

    /// The decoded image for `key`, or `None` if it's missing or left out (see [`decode_image`]).
    /// Each image is decoded once, and counted once when left out.
    pub fn get(&mut self, key: &str) -> Option<Image> {
        if let Some(image) = self.decoded.get(key) {
            return image.clone();
        }
        let image = match self.images.get(key).map(decode_image) {
            Some(Ok(image)) => Some(image),
            Some(Err(LeftOut::TooLarge)) => {
                self.skipped.too_large += 1;
                None
            }
            None | Some(Err(LeftOut::Unsupported)) => {
                self.skipped.unsupported += 1;
                None
            }
        };
        self.decoded.insert(key.to_string(), image.clone());
        image
    }

    /// How many images [`get`](Self::get) has left out, and why.
    pub fn skipped(&self) -> SkippedImages {
        self.skipped
    }
}

/// Decodes an image by its first bytes, unless its format can't go in a PDF or it has more
/// than [`MAX_IMAGE_PIXELS`].
fn decode_image(bytes: &[u8]) -> Result<Image, LeftOut> {
    let decode: fn(Data, bool) -> Result<Image, String> =
        match ImageFormat::detect(bytes).ok_or(LeftOut::Unsupported)? {
            ImageFormat::Png => Image::from_png,
            ImageFormat::Jpeg => Image::from_jpeg,
            ImageFormat::Gif => Image::from_gif,
            ImageFormat::Webp => Image::from_webp,
            ImageFormat::Bmp | ImageFormat::Tiff | ImageFormat::Emf | ImageFormat::Wmf => {
                return Err(LeftOut::Unsupported);
            }
        };

    // krilla decodes the whole image as soon as it's created, so read the size from the header
    // first.
    let size = imagesize::blob_size(bytes).map_err(|_| LeftOut::Unsupported)?;
    if size.width as u64 * size.height as u64 > MAX_IMAGE_PIXELS {
        return Err(LeftOut::TooLarge);
    }
    decode(bytes.to_vec().into(), true).map_err(|_| LeftOut::Unsupported)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::opc::{Archive, test_package};
    use crate::xlsx::tests::PNG;

    #[test]
    fn skips_images_a_pdf_cannot_hold() {
        assert_eq!(
            decode_image(b"not an image").err(),
            Some(LeftOut::Unsupported)
        );
        assert_eq!(
            decode_image(b"\x89PNG but not really").err(),
            Some(LeftOut::Unsupported)
        );
    }

    /// The start of a PNG, up to the end of its header, saying it's `width` x `height` pixels.
    /// The pixel data is missing, so decoding it would fail.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(width.to_be_bytes());
        png.extend(height.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]); // 8-bit RGBA, then a (wrong) checksum
        png
    }

    #[test]
    fn skips_images_with_too_many_pixels_before_decoding_them() {
        // Neither is decoded: a decode would allocate 40 GB, or fail on the missing pixel data.
        assert_eq!(
            decode_image(&png_header(100_000, 100_000)).err(),
            Some(LeftOut::TooLarge)
        );
        assert_eq!(
            decode_image(&png_header(50_001, 1_000)).err(),
            Some(LeftOut::TooLarge)
        );
        // At the limit, it gets as far as decoding, which fails here on the missing data.
        assert_eq!(
            decode_image(&png_header(50_000, 1_000)).err(),
            Some(LeftOut::Unsupported)
        );
    }

    #[test]
    fn decodes_each_image_once_and_counts_each_left_out_once() {
        let huge = png_header(100_000, 100_000);
        let package = test_package(&[
            ("logo.png", PNG),
            ("chart.emf", b"\x01\0\0\0 not really EMF"),
            ("huge.png", &huge),
        ]);
        let mut archive = Archive::open(package).unwrap();
        let mut embedded = EmbeddedImages::default();
        for part in ["logo.png", "chart.emf", "huge.png"] {
            embedded.add(&mut archive, part).unwrap();
        }

        let mut images = DecodedImages::new(&embedded);
        for _ in 0..2 {
            assert!(images.get("logo.png").is_some());
            assert!(images.get("chart.emf").is_none());
            assert!(images.get("huge.png").is_none());
            assert!(images.get("missing.png").is_none());
        }
        assert_eq!(
            images.skipped(),
            SkippedImages {
                unsupported: 2,
                too_large: 1
            }
        );
    }
}
