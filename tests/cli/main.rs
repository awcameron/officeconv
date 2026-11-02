//! End-to-end tests: run the real `officeconv` binary and check what it writes.
//!
//! This is one test program split into modules by feature. Each file directly in `tests/`
//! would be compiled and linked as its own program, so keeping one program keeps builds fast.

// Tests may panic, helpers outside `#[test]` functions included: clippy.toml only lets the
// test functions themselves.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// As in the library's unit tests: exact float comparisons where layout is deterministic, casts
// of small known values, and fixtures built with `format!` per item.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::format_collect
)]

mod common;
mod delimited;
mod docx;
mod errors;
mod images;
mod input;
#[cfg(feature = "pdf")]
mod pdf;
mod pptx;
mod seeds;
mod xlsx;
