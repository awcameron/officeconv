# 3. officeconv is a binary, with no public library API

- **Status:** Accepted
- **Date:** 2026-10-06
- **Issue:** #55

## Context

Every module and every item in `src/` was `pub`, including the parsers, the PDF layout engine
and `run(&Cli)`, whose argument is a clap type. Nothing outside the crate used them except
`main.rs` and, since #50, the fuzz targets. `Cargo.toml` already has the crates.io metadata, and
once the crate is published, everything `pub` is an API: renaming an internal function would be
a breaking change.

Making everything `pub` also hid unused code, because the compiler can't warn about something
another crate might call.

## Options

### A. Binary only (chosen)

Make every module private. `lib.rs` stays, because the fuzz targets need a library to link
against, but its only public item is a hidden `main()` that `main.rs` calls.

- **For:** any internal change is free. The compiler flags unused code again. Nothing ties a
  caller to clap.
- **Against:** a program that wants to convert files has to run the binary.

### B. A deliberate library API

For example `convert(input, Options) -> Result<Output>`, with `Options` independent of clap,
and everything else private.

- **For:** other Rust programs could convert files in-process.
- **Against:** nobody has asked for it. Designing `Options` and `Output` well, covering images,
  several sheets, PDF warnings and size limits, is real work, and once published it has to stay
  stable.

## Decision

**Option A.** All modules in `lib.rs` are private. The crate docs say there's no public API.

Decisions that came with it:

- **The fuzz targets use a `fuzzing` feature.** It adds a hidden `officeconv::fuzzing` module
  that re-exports the readers, writers and types the targets call. `fuzz/Cargo.toml` turns it
  on. It isn't a public API either: it's off by default and can change in any release.
- **Unused code was removed or limited.** With the modules private, the compiler found two
  unused methods: `ImageExport::dir`, now removed, and `Run::linked`, now built only for tests.
  Without the `pdf` feature it also found `EmbeddedImages::get` and `ConvertError::Pdf`, which
  only PDF output uses, so they're built only with that feature.
- **The integration tests run the binary**, as they already did, so they need nothing public.

## Consequences

- `cargo doc` shows only the crate page.
- `cargo publish` publishes a crate for `cargo install officeconv`, not one to depend on.
- A new fuzz target that needs another internal item adds it to `officeconv::fuzzing`.

## When to revisit

- If someone needs to convert files from Rust without starting a process, design option B.
  Start from what they need, not from the internal modules, and keep `Limits` part of the API
  (see [ADR 0002](0002-untrusted-input.md)).
