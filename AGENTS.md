# AGENTS.md

Notes for coding agents working on officeconv. The [README](README.md) is the full guide to
using and developing it; this file holds what an agent needs to know before changing anything.

## What this is

A Rust command-line tool that converts Office files: `.xlsx` to CSV, TSV, JSON or Markdown, and
`.docx` and `.pptx` to Markdown or PDF. It's a binary only: the library half exists so the fuzz
targets can link against it, and nothing in it is a public API
([ADR 0003](docs/adr/0003-binary-only.md)). Edition 2024, on stable Rust.

The [Layout](README.md#layout) section of the README says what each file does. In short:

- `src/lib.rs`: `main()` and `run()`, which check the options and pick a converter.
- `src/opc.rs`: the zip reader and its size limits, plus `walk()`, the XML event loop every
  reader uses.
- `src/docx/`, `src/pptx.rs`, `src/xlsx/`: the readers.
- `src/document/`, `src/writers.rs`, `src/pdf/`: the outputs.
- `tests/cli/`: end-to-end tests that run the binary.
- `fuzz/`: cargo-fuzz targets.
- `tools/compare/`: compares two builds' output.

## Checks

Run all of these before committing. The first six are what CI runs; the last two cover what CI
doesn't build.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
cargo test --locked
cargo clippy --locked --all-targets --no-default-features -- -D warnings
cargo test --locked --no-default-features
cargo clippy --locked -p officeconv-fuzz --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items
```

`--no-default-features` builds without the `pdf` feature, so code used only by PDF output needs
`#[cfg(feature = "pdf")]`.

CI also runs `cargo deny` ([`deny.toml`](deny.toml)) on every dependency, so a new crate needs
an allowed license and no open advisories.

## Rules

- **Untrusted input.** officeconv promises to be safe on files from anyone, within the limits in
  [SECURITY.md](SECURITY.md) ([ADR 0002](docs/adr/0002-untrusted-input.md)). The promise covers
  memory, files it writes, links and escaping. Any change that reads a new part of a file,
  writes a new kind of file, or adds an output has to stay within it. Update SECURITY.md if the
  change adds a promise.
  - Read parts through `opc::Archive`, which enforces the size limits.
  - Never make work grow with how many times a file refers to the same thing.
- **Reader changes.** Any change to `src/opc.rs` or a reader can change output. Run
  `tools/compare/compare.sh`, which compares the working tree with `main` on generated files,
  and explain every difference it lists. The readers decide from the stack of open elements
  (`opc::Open`), not from per-reader flags.
- **Errors.** Each `ConvertError` variant picks its exit code in `ConvertError::exit_code`. The
  match is exhaustive, so a new variant has to choose one.
- **Tests.** Tests build their own fixtures in temporary directories; don't commit binary test
  files. A behavior change needs a test, in `tests/cli/` if a user would see it.
- **Releases.** CI tags and publishes a release when a PR changing `version` in `Cargo.toml`
  merges ([Releasing](README.md#releasing)). Never push, move or delete a `v*` tag.
- **Git.**
  - Commit only files you changed. The repo root can hold untracked local files that aren't
    part of the project, so never `git add -A` or `git add .`.
  - Push, open PRs and merge only when asked.

## Conventions

- **Branches:** `<type>/<issue>-<slug>`, such as `fix/56-element-stack`.
- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/):
  - one of `feat:`, `fix:`, `refactor:`, `test:`, `ci:`, `docs:` or `chore:`;
  - the subject says what changes, in the imperative;
  - the body explains why and how it was checked;
  - `Closes #N` at the end.
- **PRs:** say what changed and why, call out any behavior change, and list how it was checked.
- **Code:** match the surrounding code's naming, idioms and comment density. Comments explain
  why, not what.
- **Docs:** update the README for anything a user sees. Record a design decision with real
  alternatives as an ADR in [`docs/adr/`](docs/adr/README.md).
- **Fuzzing** needs nightly and cargo-fuzz, and runs from `fuzz/`; see
  [Fuzzing](README.md#fuzzing).

Claude Code also has skills for this repo's two main workflows in `.claude/skills/`: taking an
issue to a merged PR, and cutting a release.
