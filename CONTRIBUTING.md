# Contributing

Bug reports, ideas and pull requests are welcome. Report a vulnerability privately instead, as
[SECURITY.md](SECURITY.md) describes.

[AGENTS.md](AGENTS.md) holds the rules a change has to follow, such as staying safe on untrusted
input, and the conventions for branches, commits and PRs. It's written for coding agents, but
the rules are the same for people. [ARCHITECTURE.md](ARCHITECTURE.md) describes how a conversion
moves through the code.

- [Development](#development)
- [Developing in a container](#developing-in-a-container)
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

The tests build their own `.xlsx`, `.docx`, and `.pptx` fixtures in temporary directories, so
the repo doesn't need to contain any binary test files.

Design decisions are recorded in [`docs/adr/`](docs/adr/), starting with
[how PDF output is rendered](docs/adr/0001-pdf-rendering.md).

## Developing in a container

[`.devcontainer/Dockerfile`](.devcontainer/Dockerfile) is a Linux image with everything the
checks, fuzzing and `tools/compare` need: Rust at exactly the `rust-version` releases build
with, nightly, `cargo-fuzz`, `cargo-deny`, Python 3 and git. Use it if you'd rather not install
those, if Homebrew's Rust gets in the way of rustup's, or to fuzz on Windows. It's only for
development; it doesn't run officeconv for you.

From the repository root, build the image once, then run commands in it:

```sh
docker build -t officeconv-dev .devcontainer
alias dev='docker run --rm -it -v "$PWD:/work" -v officeconv-target:/cache/target -v officeconv-cargo-registry:/usr/local/cargo/registry officeconv-dev'

dev scripts/check.sh                               # every check, cargo deny included
dev sh -c 'cd fuzz && cargo +nightly fuzz run docx'  # fuzzing; Ctrl-C to stop
dev tools/compare/compare.sh                       # compare with main
dev bash                                           # a shell
```

The repository is mounted at `/work`, so edits on either side show up on the other. Builds go to the
`officeconv-target` volume and downloaded crates to `officeconv-cargo-registry`, which keeps them
between runs and away from your own `target/`. On Linux, add `--user "$(id -u):$(id -g)"` after
`docker run`, so files it writes into the repository, such as fuzz inputs, belong to you. Use the
same user every time: the volumes keep whichever user first wrote to them, and another can't write
there. To start them afresh, run `docker volume rm officeconv-target officeconv-cargo-registry`.
`compare.sh` builds into `target/compare/` in the repository, which is slower under Docker Desktop
on macOS than a native build.

VS Code's Dev Containers extension and GitHub Codespaces use the same image through
[`.devcontainer/devcontainer.json`](.devcontainer/devcontainer.json), with the same volumes.

The [Dev container workflow](.github/workflows/dev-container.yml) builds the image and checks
that each of its tools runs whenever the image changes, and also builds the fuzz targets in it
once a week. CI's `msrv` job fails if the image's Rust version differs from `rust-version`, so a
change to one needs the other.

## Fuzzing

[`fuzz/`](fuzz/) feeds random bytes through the zip reader and the DOCX, PPTX, XLSX and CSV
readers, then renders whatever they read as Markdown, PDF, CSV, TSV and JSON. Any panic, hang
or out-of-memory error is a bug. The readers run with small size limits (1 MB per part, 4 MB in
total, and 65,536 cells for CSV), so a size bug fails fast instead of using gigabytes. Fuzzing
needs a nightly toolchain and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz), and works
on Linux and macOS:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
./make-seeds.sh                                  # small files to start from
cargo +nightly fuzz run docx -- -max_len=65536   # or archive, delimited, pptx, xlsx; Ctrl-C to stop
```

`make-seeds.sh` writes seeds that between them use every part and element the readers handle:
footnotes, headers and footers, page number fields, merged cells, sized and linked pictures,
speaker notes and so on. [`tests/cli/seeds.rs`](tests/cli/seeds.rs) builds them with the same
helpers as the tests, and its `every_seed_converts` test checks that each one converts. A
reader change that reads a new part or element adds a seed there.

Run `cargo fuzz` from `fuzz/`: [`fuzz/.cargo/config.toml`](fuzz/.cargo/config.toml) turns off
the release profile's LTO and symbol stripping, which make fuzz builds slow and crash reports
unreadable, and Cargo only reads it there. `-max_total_time=3600` stops a run after an hour. An
input that crashes is saved under `fuzz/artifacts/<target>/`, and
`cargo +nightly fuzz run <target> <file>` replays it.

`cargo +nightly` only works when `cargo` is rustup's. If Homebrew's Rust comes first on your
`PATH` (`which cargo` prints `/opt/homebrew/bin/cargo` or `/usr/local/bin/cargo`), it fails with
"no such command: `+nightly`". `rustup run nightly cargo fuzz` fails too, with "the option `Z`
is only accepted on the nightly compiler", because it still finds Homebrew's `cargo`. Put
nightly's tools first instead, in place of `cargo +nightly`:

```sh
PATH="$(dirname "$(rustup which --toolchain nightly rustc)"):$PATH" cargo fuzz run docx
```

The [Fuzz workflow](.github/workflows/fuzz.yml) fuzzes each target for an hour every Monday,
from the seeds, with the same `-max_len` as above and `-timeout=25`, so an input that takes 25
seconds counts as a hang. "Run workflow" on its Actions page fuzzes for as many seconds as you
ask, and a PR that changes the workflow, `fuzz/` or the seeds fuzzes each target for a minute.
When a target fails, its job uploads the input as an artifact named `fuzz-<target>`. Download
it from the run's page, or with `gh run download <run> -n fuzz-<target>`, and replay it with
`cargo +nightly fuzz run <target> <file>`. A crash it finds is a bug to fix like any other,
with the input as its test.

## Comparing two builds

Before merging a change to a reader, check that its output changes only where you meant it to:

```sh
tools/compare/compare.sh            # the working tree against main
tools/compare/compare.sh v0.3.0     # or against any commit, branch or tag
```

It builds both versions (with LTO off, so each build takes about a minute), generates 2,000
`.docx`, `.pptx` and `.xlsx` files whose XML nests elements at random, converts each with both
builds in parallel, and lists every file whose Markdown, saved images, PDF, messages or exit
status differ. The PDF shows what Markdown leaves out, such as headers, footers, page numbers
and alignment, and its bytes are the same on every run. For each file that differs, `diff -r`
on the two folders it prints shows what changed. It needs Python 3, and takes a few minutes. A
reader change that reads a new part or element adds it to
[`gen-nesting.py`](tools/compare/gen-nesting.py) first, so the generated files have it. `COUNT`, `SEED`, `CORPUS` and `JOBS` change what it runs; see
the top of the script.

## Layout

Every top-level file and directory in the repository. [ARCHITECTURE.md](ARCHITECTURE.md) shows
how the modules in `src/` fit together, and each module's doc comment says what it does, so
`cargo doc --document-private-items --open` is a map of the code.

```text
src/                 the code: main() and run() in lib.rs, the readers, and the outputs
tests/cli/           end-to-end tests that run the real binary
fuzz/                fuzz targets for the zip reader and the four readers (cargo-fuzz)
tools/compare/       compares the output of two builds on generated files
scripts/check.sh     the checks to run before committing
scripts/smoke.sh     runs a built binary once for each kind of input, as releases do
build.rs             compresses the fonts in assets/fonts/ before they're built into the binary
assets/fonts/        Noto Sans, built into the binary for PDF output, and its license
docs/                the user guide beyond the README: installing, usage, and formats
docs/adr/            design decisions, one record each
.devcontainer/       a Linux image to build, test and fuzz in, for Docker, VS Code and Codespaces
.github/             CI, fuzz, release and dev container workflows, Dependabot, and release notes
.agents/skills/      agent skills for this repo's workflows: filing issues, issue to PR, releasing
.claude/skills       a symlink to .agents/skills, so Claude Code finds them too
AGENTS.md            the rules and checks for coding agents
CLAUDE.md            imports AGENTS.md for Claude Code
Cargo.toml           the package, its dependencies, features and minimum Rust version
Cargo.lock           the exact dependency versions builds and CI use
deny.toml            which licenses, advisories and sources cargo deny allows
README.md            what officeconv does, and how to install it and start using it
CONTRIBUTING.md      this file
ARCHITECTURE.md      how a conversion moves through the code, from options to output
SECURITY.md          what officeconv protects against, and how to report a vulnerability
LICENSE              MIT
.gitignore           build output and editor files
```

## Releasing

Bump `version` in `Cargo.toml` (and `Cargo.lock`, with `cargo update -w`) in a PR, and merge
it. That's all: there's no tag to push.

On the merge, the [release workflow](.github/workflows/release.yml) runs the tests, builds a
binary for Linux (x86_64 and arm64, each linked against glibc and statically with musl), macOS
(Apple Silicon, Intel) and Windows (x86_64, arm64), and smoke-tests each one on a runner of its
own platform. Only when every binary has passed does it tag the commit `vX.Y.Z` and publish a
GitHub Release with all of them, each with a SHA-256 checksum. The archives' names have no
version, so the `releases/latest/download/` links in the README's
[Install](README.md#download-a-binary) section always get the newest release. A merge that
changes `Cargo.toml` but not the version finds the tag already there and publishes nothing.
Existing tags are never moved, and a ruleset blocks moving or deleting them by hand.

The release notes are GitHub's generated list of PRs, grouped by label as
[`.github/release.yml`](.github/release.yml) sets out: features, fixes, documentation, then other
changes. The version-bump PR, labeled `release`, is left out.

The smoke test, [`scripts/smoke.sh`](scripts/smoke.sh), runs a built binary once for each kind
of input, including PDF output, and checks its version and its usage exit code. Run it on your
own build with `scripts/smoke.sh target/release/officeconv`. It needs Python 3, which writes
its inputs. PRs that change the release workflow or the smoke test build and smoke-test every
platform without publishing anything.

Nothing is tagged or published until the tests and every smoke test pass. If a build or smoke
test fails, nothing is public yet: use "Re-run failed jobs" in the Actions tab for a flaky
failure. If publishing fails after the release was created, "Re-run failed jobs" replaces its
files; re-running every job would find the tag already there and stop. One platform failing
doesn't cancel the others.

Releases build with exactly the Rust version in `rust-version` in `Cargo.toml`, not whatever
`stable` is that day, so the same commit always builds the same way. CI's `msrv` job runs the
tests with that version too, so a dependency that needs a newer Rust fails there. To move to a
newer Rust, change `rust-version` in a PR; the README's "Rust 1.92 or later" changes with it.
