# AGENTS.md

Notes for coding agents working on officeconv. The [README](README.md) is the guide to using
it, and [CONTRIBUTING.md](CONTRIBUTING.md) to developing it; this file holds what an agent needs
to know before changing anything.

## What this is

A Rust command-line tool that converts Office files: `.xlsx` to CSV, TSV, JSON or Markdown, and
`.docx` and `.pptx` to Markdown or PDF. It's a binary only: the library half exists so the fuzz
targets can link against it, and nothing in it is a public API
([ADR 0003](docs/adr/0003-binary-only.md)). Edition 2024, on stable Rust.

[CONTRIBUTING.md](CONTRIBUTING.md#layout) lists every top-level file and directory. The main
modules:

- `src/lib.rs`: `main()` and `run()`, which check the options and pick a converter.
- `src/opc.rs`: the zip reader and its size limits, plus `walk()`, the XML event loop every
  reader uses.
- `src/docx/`, `src/pptx.rs`, `src/xlsx/`: the readers. DOCX and PPTX build their blocks
  through `document::builder`, which owns open paragraphs and tables. Readers name each image
  by its part in the package; `images::resolve` then saves, embeds or drops them for the
  output ([ADR 0005](docs/adr/0005-image-types.md)).
- `src/document/`, `src/writers.rs`, `src/pdf/`: the outputs.
- `src/markdown.rs`: Markdown escaping and tables, shared by the spreadsheet and document
  Markdown writers.
- `tests/cli/`: end-to-end tests that run the binary.
- `fuzz/`: cargo-fuzz targets.
- `tools/compare/`: compares two builds' output.
- `scripts/check.sh`: the checks to run before committing.
- `scripts/smoke.sh`: the smoke test the Release workflow runs on every binary.

## Checks

Run [`scripts/check.sh`](scripts/check.sh) before committing. It stops at the first check that
fails and says which. Its first six checks are the steps of CI's `check` job; the other two
cover what CI doesn't build: the fuzz crate and the private-item docs.

Two of the checks build without the `pdf` feature (`--no-default-features`), so code used only
by PDF output needs `#[cfg(feature = "pdf")]`.

CI also runs `cargo deny` ([`deny.toml`](deny.toml)) on every dependency, so a new crate needs
an allowed license and no open advisories. The script runs it too when `cargo-deny` is
installed.

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
- **Document model.** `Block` holds everything at least one writer can show, and each writer
  drops what it can't ([ADR 0004](docs/adr/0004-document-model.md)). A new field defaults to
  today's behavior, the reader bounds its value, Markdown output doesn't change unless the issue
  says so, and [docs/formats.md](docs/formats.md) says what each output does with it.
- **Errors.** Each `ConvertError` variant picks its exit code in `ConvertError::exit_code`. The
  match is exhaustive, so a new variant has to choose one.
- **Tests.** Tests build their own fixtures in temporary directories; don't commit binary test
  files. A behavior change needs a test, in `tests/cli/` if a user would see it.
- **Releases.** CI tags and publishes a release when a PR changing `version` in `Cargo.toml`
  merges ([Releasing](CONTRIBUTING.md#releasing)). Never push, move or delete a `v*` tag.
- **Git.**
  - Commit only files you changed. The repo root can hold untracked local files that aren't
    part of the project, so never `git add -A` or `git add .`.
  - Push, open PRs and merge only when asked.

## Conventions

- **Branches:** `<type>/<issue>-<slug>`, such as `fix/56-element-stack`.
- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/):
  - one of `feat:`, `fix:`, `refactor:`, `test:`, `ci:`, `docs:` or `chore:`;
  - optionally a scope naming the area it changes, such as `docs(readme):` or `fix(docx):`.
    Leave it off when a change spans several areas, rather than picking a vague one. Docs in
    `docs/` and CONTRIBUTING.md take the scope of their topic, such as `docs(pdf):`, and a type
    is never a scope. Use only these scopes, and add one here in the PR that first needs it:

    | Scope      | Area                                |
    | ---------- | ----------------------------------- |
    | `docx`     | `src/docx/`                         |
    | `pptx`     | `src/pptx.rs`                       |
    | `xlsx`     | `src/xlsx/`                         |
    | `opc`      | `src/opc.rs`                        |
    | `model`    | `src/document/`                     |
    | `pdf`      | `src/pdf/`                          |
    | `writers`  | `src/writers.rs`                    |
    | `markdown` | `src/markdown.rs`                   |
    | `cli`      | `src/lib.rs`, `tests/cli/`          |
    | `fuzz`     | `fuzz/`                             |
    | `tools`    | `tools/compare/`, `scripts/`        |
    | `readme`   | `README.md`                         |
    | `skills`   | `.agents/skills/`, `AGENTS.md`      |
    | `release`  | version bumps, the release workflow |
    | `deps`     | dependency bumps, `deny.toml`       |

  - the subject says what changes, in the imperative;
  - the body explains why and how it was checked;
  - `Closes #N` at the end.
- **PRs:** say what changed and why, call out any behavior change, and list how it was checked.
- **Code:** match the surrounding code's naming, idioms and comment density. Comments explain
  why, not what.
- **Docs:** update the user docs for anything a user sees: the README for what it does, how to
  install it and its options, and `docs/` for the details. Update CONTRIBUTING.md for anything a
  contributor does. Record a design decision with real alternatives as an ADR in
  [`docs/adr/`](docs/adr/README.md).
- **Fuzzing** needs nightly and cargo-fuzz, and runs from `fuzz/`; see
  [Fuzzing](CONTRIBUTING.md#fuzzing).

Skills for this repo's three main workflows live in `.agents/skills/`: filing an issue, taking
an issue to a merged PR, and cutting a release. Codex, Cursor and other agents that support
[Agent Skills](https://agentskills.io) read them from there; `.claude/skills` is a symlink to
the same directory for Claude Code.
