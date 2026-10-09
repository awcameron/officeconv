# officeconv

[![CI](https://github.com/awcameron/officeconv/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/awcameron/officeconv/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/awcameron/officeconv)](https://github.com/awcameron/officeconv/releases/latest)
[![License: MIT](https://img.shields.io/github/license/awcameron/officeconv)](LICENSE)

A small command-line tool that converts Office files to plain-text formats and PDF:

|                  | CSV | TSV | JSON | Markdown | PDF |
| ---------------- | :-: | :-: | :--: | :------: | :-: |
| **XLSX**         | ✓   | ✓   | ✓    | ✓        |     |
| **CSV**, **TSV** | ✓   | ✓   | ✓    | ✓        |     |
| **DOCX**         |     |     |      | ✓        | ✓   |
| **PPTX**         |     |     |      | ✓        | ✓   |

It's a single binary, and doesn't need Office or LibreOffice installed. It's meant to be safe to
run on files from people you don't trust: it bounds how much a file can make it decompress,
writes only the files you ask for, and keeps links and text in the output from running code or
turning into new formatting. [SECURITY.md](SECURITY.md) lists what it protects against.

- [Quick start](#quick-start)
- [Limitations](#limitations)
- [Install](#install)
- [Usage](#usage)
- [Documentation](#documentation)
- [Project status](#project-status)

## Quick start

[Install](#install) it, then try it with no input file:

```console
$ printf 'name,city\nAda,London\nGrace,Arlington\n' | officeconv - --from csv --to md
| name  | city      |
| ----- | --------- |
| Ada   | London    |
| Grace | Arlington |
```

On your own files:

```sh
officeconv sales.xlsx --to csv              # first sheet as CSV, to the terminal
officeconv sales.xlsx --to json -o out.json # as JSON, to a file
officeconv notes.docx --to md -o notes.md   # a Word document as Markdown
officeconv talk.pptx --to pdf -o talk.pdf   # a slide deck as PDF
```

[Usage](#usage) has every option, and [docs/usage.md](docs/usage.md#examples) has more examples.

## Limitations

- **Content, not layout.** Fonts, colors, margins, columns and slide designs aren't kept.
- **Formulas aren't calculated.** A cell shows the result Excel last saved.
- **Right-to-left text in PDF**, such as Arabic or Hebrew, comes out in the wrong order.
- **Merged table cells** can't be shown in Markdown, which has none. PDF draws each as one cell.
- **Not converted yet:** comments, and headers and footers in DOCX; charts, SmartArt, and the
  slide master and layout in PPTX; charts, shapes and in-cell pictures as images.

[docs/formats.md](docs/formats.md) has the details for each format.
[ADR 0004](docs/adr/0004-document-model.md) sets out how the missing pieces will be added.

## Install

### Download a binary

Each [release](https://github.com/awcameron/officeconv/releases) has a ready-built binary:

| Platform                       | `target`                             |
| ------------------------------ | ------------------------------------ |
| macOS, Apple silicon           | `aarch64-apple-darwin`               |
| macOS, Intel                   | `x86_64-apple-darwin`                |
| Linux, x86_64                  | `x86_64-unknown-linux-gnu`           |
| Linux, arm64                   | `aarch64-unknown-linux-gnu`          |
| Linux, x86_64, static (musl)   | `x86_64-unknown-linux-musl`          |
| Linux, arm64, static (musl)    | `aarch64-unknown-linux-musl`         |
| Windows, x86_64                | `x86_64-pc-windows-msvc` (a `.zip`)  |
| Windows, arm64                 | `aarch64-pc-windows-msvc` (a `.zip`) |

Pick `musl` for Alpine or a minimal container image.

#### macOS and Linux

Set `target` from the table. This downloads the latest release, checks it against its SHA-256
checksum, and puts `officeconv` in `~/.local/bin`:

```sh
target=aarch64-apple-darwin
url=https://github.com/awcameron/officeconv/releases/latest/download/officeconv-$target
curl -fsSLO "$url.tar.gz" && curl -fsSLO "$url.sha256"
shasum -a 256 -c "officeconv-$target.sha256"
mkdir -p ~/.local/bin && tar xzf "officeconv-$target.tar.gz" -C ~/.local/bin officeconv
```

On Linux without `shasum`, use `sha256sum -c "officeconv-$target.sha256"` instead. If
`~/.local/bin` isn't on your `PATH` (macOS doesn't add it), add
`export PATH="$HOME/.local/bin:$PATH"` to your shell's startup file.

#### Windows

In PowerShell, set `$target` to `x86_64-pc-windows-msvc`, or `aarch64-pc-windows-msvc` for an
ARM laptop. This downloads the latest release, checks it, and unpacks it into
`%LOCALAPPDATA%\Programs\officeconv`:

```powershell
$target = "x86_64-pc-windows-msvc"
$url = "https://github.com/awcameron/officeconv/releases/latest/download/officeconv-$target"
Invoke-WebRequest "$url.zip" -OutFile "officeconv-$target.zip"
Invoke-WebRequest "$url.sha256" -OutFile "officeconv-$target.sha256"
$expected = (Get-Content "officeconv-$target.sha256").Split(" ")[0]
if ((Get-FileHash "officeconv-$target.zip").Hash -ne $expected) { throw "checksum mismatch" }
$dir = "$env:LOCALAPPDATA\Programs\officeconv"
Expand-Archive "officeconv-$target.zip" -DestinationPath $dir -Force
```

Then add that folder to your user `PATH`, and open a new terminal:

```powershell
$path = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$path;$dir", "User")
```

### Build from source

You need Rust 1.92 or later ([rustup.rs](https://rustup.rs)):

```sh
cargo install --locked --git https://github.com/awcameron/officeconv officeconv
```

[docs/install.md](docs/install.md) covers the rest: which Linux build to pick, checking where a
binary came from, why macOS or Windows may warn about it, older versions, building without PDF
output, and uninstalling.

## Usage

```text
officeconv [OPTIONS] --to <FORMAT> <INPUT>
```

`<INPUT>` is the file to convert, or `-` to read from [stdin](docs/usage.md#reading-from-stdin).

| Option                | Meaning                                                                                     |
| --------------------- | ------------------------------------------------------------------------------------------- |
| `-t, --to <FORMAT>`   | `csv`, `tsv`, `json`, `md` (`markdown` also works), or `pdf`                                |
| `--from <TYPE>`       | `xlsx`, `docx`, `pptx`, `csv`, or `tsv`: the input type, instead of detecting it            |
| `-o, --output <PATH>` | Write to a file instead of stdout. With `--all-sheets`, a directory                         |
| `--sheet <NAME>`      | XLSX only: which sheet to convert. Defaults to the first one. Not with `--all-sheets`       |
| `--all-sheets`        | XLSX only: write each sheet to its own file, such as `sales-Q1.csv`                         |
| `--typed`             | JSON only: write numbers, booleans and empty cells as JSON values. Not for CSV or TSV input |
| `--no-notes`          | PPTX only: leave out speaker notes                                                          |
| `--images <DIR>`      | Save images into `DIR` and link them from the Markdown (not for `pdf`)                      |
| `-h, --help`          | Print help                                                                                  |
| `-V, --version`       | Print the version                                                                           |

The input type comes from the file extension; `--from` overrides it. `-` reads from stdin, which
needs `--from` for CSV and TSV. A file stops with an error if it would decompress to more than
1 GiB in total, or 256 MiB in one part, so a small file can't expand to gigabytes. The limits are
fixed.

[docs/usage.md](docs/usage.md) has the details: how the input type is chosen, reading from stdin,
more examples, exit codes, and every size limit.

## Documentation

- [docs/install.md](docs/install.md): other ways to install, and checking a download
- [docs/usage.md](docs/usage.md): stdin, examples, exit codes and size limits
- [docs/formats.md](docs/formats.md): what each input keeps and how each output is written
- [SECURITY.md](SECURITY.md): what officeconv protects against, and how to report a
  vulnerability
- [CONTRIBUTING.md](CONTRIBUTING.md): building, testing, fuzzing and releasing
- [docs/adr/](docs/adr/README.md): design decisions

## Project status

`officeconv` is written in Rust as a learning project, and hasn't reached 1.0. Any release can
change the output or the options; the
[release notes](https://github.com/awcameron/officeconv/releases) list what each one changes.

## License

[MIT](LICENSE). The Noto Sans fonts in `assets/fonts/`, which are built into the binary, are
licensed under the [SIL Open Font License 1.1](assets/fonts/OFL.txt).
