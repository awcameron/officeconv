//! End-to-end tests: run the real `officeconv` binary and check what it writes.
//!
//! This is one test program split into modules by feature. Each file directly in `tests/`
//! would be compiled and linked as its own program, so keeping one program keeps builds fast.

mod common;
mod docx;
mod errors;
mod images;
mod input;
mod pdf;
mod pptx;
mod xlsx;
