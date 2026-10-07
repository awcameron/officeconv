# Contributing

Bug reports, ideas and pull requests are welcome. Report a vulnerability privately instead, as
[SECURITY.md](SECURITY.md) describes.

[AGENTS.md](AGENTS.md) holds the rules a change has to follow, such as staying safe on untrusted
input, and the conventions for branches, commits and PRs. It's written for coding agents, but
the rules are the same for people.

- [Development](#development)
- [Fuzzing](#fuzzing)
- [Comparing two builds](#comparing-two-builds)
- [Layout](#layout)
- [Releasing](#releasing)

## Development

```sh
scripts/check.sh                                      # every check CI runs, and a few more
cargo fmt                                             # fix formatting
cargo doc --no-deps --document-private-items --open   # browse the code's documentation
cargo run -- sales.xlsx --to md                       # run without installing
```

Run `scripts/check.sh` before committing: a change that passes it passes CI's checks. It runs
`cargo deny` too if [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) is installed.

`officeconv` is a binary: its library has no public API, so internals can change in any release.
See [ADR 0003](docs/adr/0003-binary-only.md).

The tests build their own `.xlsx`, `.docx`, and `.pptx` fixtures in temporary directories, so the repo
doesn't need to contain any binary test files.

Design decisions are recorded in [`docs/adr/`](docs/adr/), starting with
[how PDF output is rendered](docs/adr/0001-pdf-rendering.md).

## Fuzzing

[`fuzz/`](fuzz/) feeds random bytes through the zip reader and the DOCX, PPTX, XLSX and CSV
readers, then renders whatever they read as Markdown, PDF, CSV, TSV and JSON. Any panic,
hang or out-of-memory error is a bug. The readers run with small size limits (1 MB per part,
4 MB in total, and 65,536 cells for CSV), so a size bug fails fast instead of using gigabytes. Fuzzing needs a nightly
toolchain and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz), and works on Linux and
macOS:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
./make-seeds.sh                                  # a small .docx, .pptx, .xlsx and .csv to start from
cargo +nightly fuzz run docx -- -max_len=65536   # or archive, delimited, pptx, xlsx; Ctrl-C to stop
```

Run `cargo fuzz` from `fuzz/`: [`fuzz/.cargo/config.toml`](fuzz/.cargo/config.toml) turns off
the release profile's LTO and symbol stripping, which make fuzz builds slow and crash reports
unreadable, and Cargo only reads it there. `-max_total_time=3600` stops a run after an hour. An input that crashes is saved under
`fuzz/artifacts/<target>/`, and `cargo +nightly fuzz run <target> <file>` replays it.

## Comparing two builds

Before merging a change to a reader, check that its output changes only where you meant it to:

```sh
tools/compare/compare.sh            # the working tree against main
tools/compare/compare.sh v0.3.0     # or against any commit, branch or tag
```

It builds both versions (with LTO off, so each build takes about a minute), generates 2,000
`.docx`, `.pptx` and `.xlsx` files whose XML nests elements at random, converts each with both
builds in parallel, and lists every file whose Markdown, saved images, messages or exit status
differ. For each one, `diff -r` on the two folders it prints shows what changed. It needs
Python 3, and takes a few minutes. `COUNT`, `SEED`, `CORPUS` and `JOBS` change what it runs; see
the top of the script.

## Layout

Every top-level file and directory in the repository. [AGENTS.md](AGENTS.md#what-this-is) names
the main modules in `src/`, and each module's doc comment says what it does, so
`cargo doc --document-private-items --open` is a map of the code.

```text
src/                 the code: main() and run() in lib.rs, the readers, and the outputs
tests/cli/           end-to-end tests that run the real binary
fuzz/                fuzz targets for the zip reader and the four readers (cargo-fuzz)
tools/compare/       compares the output of two builds on generated files
scripts/check.sh     the checks to run before committing
build.rs             compresses the fonts in assets/fonts/ before they're built into the binary
assets/fonts/        Noto Sans, built into the binary for PDF output, and its license
docs/adr/            design decisions, one record each
.github/             CI and release workflows, Dependabot, and how release notes are grouped
.agents/skills/      agent skills for this repo's workflows: issue to PR, and releasing
.claude/skills       a symlink to .agents/skills, so Claude Code finds them too
AGENTS.md            the rules and checks for coding agents
CLAUDE.md            imports AGENTS.md for Claude Code
Cargo.toml           the package, its dependencies, features and minimum Rust version
Cargo.lock           the exact dependency versions builds and CI use
deny.toml            which licenses, advisories and sources cargo deny allows
README.md            how to install and use officeconv
CONTRIBUTING.md      this file
SECURITY.md          what officeconv protects against, and how to report a vulnerability
LICENSE              MIT
.gitignore           build output and editor files
```

## Releasing

Bump `version` in `Cargo.toml` (and `Cargo.lock`, with `cargo update -w`) in a PR, and merge
it. That's all: there's no tag to push.

On the merge, the [release workflow](.github/workflows/release.yml) runs the tests, tags the
commit `vX.Y.Z`, and publishes a GitHub Release with a binary for Linux (x86_64, arm64), macOS
(Apple Silicon, Intel) and Windows, each with a SHA-256 checksum. The archives' names have no
version, so the `releases/latest/download/` links in the README's
[Install](README.md#download-a-binary) section always get the newest release. A merge that
changes `Cargo.toml` but not the version finds the tag already there and publishes nothing.
Existing tags are never moved, and a ruleset blocks moving or deleting them by hand.

The release notes are GitHub's generated list of PRs, grouped by label as
[`.github/release.yml`](.github/release.yml) sets out: features, fixes, documentation, then other
changes. The version-bump PR, labeled `release`, is left out.

Nothing is tagged until the tests pass. If a build fails after that, use "Re-run failed jobs"
in the Actions tab: re-running every job would find the tag already there and stop. One
platform failing doesn't cancel the others.

Releases build with exactly the Rust version in `rust-version` in `Cargo.toml`, not whatever
`stable` is that day, so the same commit always builds the same way. CI's `msrv` job runs the
tests with that version too, so a dependency that needs a newer Rust fails there. To move to a
newer Rust, change `rust-version` in a PR; the README's "Rust 1.92 or later" changes with it.
