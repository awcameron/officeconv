//! Compresses the built-in Noto Sans fonts before they're embedded in the binary, which
//! roughly halves their size. `pdf::fonts` decompresses them the first time a PDF is written.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::DeflateEncoder;

const FONTS: [&str; 4] = [
    "NotoSans-Regular.ttf",
    "NotoSans-Bold.ttf",
    "NotoSans-Italic.ttf",
    "NotoSans-BoldItalic.ttf",
];

fn main() {
    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    for font in FONTS {
        let source = Path::new("assets/fonts").join(font);
        println!("cargo:rerun-if-changed={}", source.display());

        let bytes = fs::read(&source).unwrap_or_else(|err| panic!("{}: {err}", source.display()));
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(&bytes)
            .expect("writing to a Vec can't fail");
        let compressed = encoder.finish().expect("writing to a Vec can't fail");
        fs::write(
            Path::new(&out_dir).join(format!("{font}.deflate")),
            compressed,
        )
        .expect("OUT_DIR is writable");
    }
}
