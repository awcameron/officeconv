# officeconv

[![CI](https://github.com/awcameron/officeconv/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/awcameron/officeconv/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/awcameron/officeconv)](https://github.com/awcameron/officeconv/releases/latest)
[![License: MIT](https://img.shields.io/github/license/awcameron/officeconv)](LICENSE)

A small command-line tool that converts Office files to plain-text formats and PDF:

- **XLSX** spreadsheets to **CSV**, **TSV**, **JSON**, or a **Markdown table**
- **CSV** and **TSV** files to the same four
- **DOCX** documents to **Markdown** or **PDF**
- **PPTX** presentations to **Markdown** or **PDF**

It's meant to be safe to run on files from people you don't trust: it bounds how much a file
can make it decompress, writes only the files you ask for, and keeps links and text in the
output from running code or turning into new formatting. [SECURITY.md](SECURITY.md) lists what
it protects against. It's written in Rust as a learning project.

- [Limitations](#limitations)
- [Install](#install)
- [Usage](#usage): [stdin](#reading-from-stdin), [examples](#examples),
  [size limits](#size-limits)
- [What gets converted](#what-gets-converted): [XLSX](#xlsx), [CSV and TSV](#csv-and-tsv),
  [DOCX](#docx), [PPTX](#pptx), [PDF](#pdf), [images](#images)
- [Contributing](#contributing)

## Limitations

- **Content, not layout.** Markdown and PDF output keep the text, tables, lists, links and
  pictures, but not Word's or PowerPoint's fonts, colors, margins, columns or slide designs. See
  [PDF](#pdf).
- **Right-to-left text** in PDF output, such as Arabic or Hebrew, comes out in the wrong order.
  See [PDF](#pdf).
- **Formulas aren't calculated.** A spreadsheet cell shows the result Excel last saved, or
  nothing if there isn't one. See [XLSX](#xlsx).
- **Merged table cells in PDF** are drawn as separate cells, as in Markdown, rather than as one
  cell across the rows or columns it covers.
- **Not converted yet:** footnotes, comments, and headers and footers in DOCX
  ([DOCX](#docx)); charts, SmartArt, and anything from the slide master or layout in PPTX
  ([PPTX](#pptx)); charts, shapes and in-cell pictures as images ([Images](#images)).

[ADR 0004](docs/adr/0004-document-model.md) sets out how these will be added: the converted
document holds everything at least one output can show, and each output leaves out what it
can't.

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

The `gnu` Linux builds need glibc 2.17 or later, which nearly every distribution has. The `musl`
builds are statically linked and need no system libraries, so they also run on Alpine and in
minimal container images such as `scratch` or distroless. For PDF output with characters Noto
Sans lacks, the system still needs a font that has them; on Alpine, install `font-noto-cjk`.
The `musl` builds and the Windows arm64 build start with the first release after v0.3.6.

On macOS or Linux, set `target` from the table. This downloads the latest release, checks it
against its SHA-256 checksum, and puts `officeconv` in `~/.local/bin`:

```sh
target=aarch64-apple-darwin
url=https://github.com/awcameron/officeconv/releases/latest/download/officeconv-$target
curl -fsSLO "$url.tar.gz" && curl -fsSLO "$url.sha256"
shasum -a 256 -c "officeconv-$target.sha256"    # Linux: sha256sum -c "officeconv-$target.sha256"
mkdir -p ~/.local/bin && tar xzf "officeconv-$target.tar.gz" -C ~/.local/bin officeconv
```

If `~/.local/bin` isn't on your `PATH` (macOS doesn't add it), add
`export PATH="$HOME/.local/bin:$PATH"` to your shell's startup file. Then try it, no input file
needed:

```sh
printf 'name,city\nAda,London\n' | officeconv - --from csv --to md
```

The [GitHub CLI](https://cli.github.com) can download the same two files in place of the `curl`
line: `gh release download -R awcameron/officeconv -p "officeconv-$target.*"`. On Windows,
download `officeconv-x86_64-pc-windows-msvc.zip` (or `aarch64-pc-windows-msvc` for an ARM
laptop) from the latest release and put
`officeconv.exe` in a folder on your `PATH`. To get one version instead of the latest, replace
`latest/download` with `download/<tag>`, such as `download/v0.3.5`. Releases before v0.3.5 have
the version in their file names too, such as `officeconv-v0.3.4-<target>.tar.gz`.

The checksum shows the download wasn't damaged, but it sits on the same page as the archive. To
check that the archive was built by this repository's release workflow, from a commit in it, use
the GitHub CLI:

```sh
gh attestation verify "officeconv-$target.tar.gz" -R awcameron/officeconv
```

It prints the workflow and commit that built the file, and fails for anything else. Releases
after v0.3.5 are attested; earlier ones aren't.

The binaries aren't code-signed by Apple or Microsoft. A file downloaded in a browser is marked
as coming from the internet, so macOS refuses to open it and Windows SmartScreen warns about it;
`curl` and `gh` downloads aren't marked. On macOS, `xattr -d com.apple.quarantine officeconv`
removes the mark.

### Build from source

You need Rust 1.92 or later ([rustup.rs](https://rustup.rs)):

```sh
cargo install --locked --git https://github.com/awcameron/officeconv officeconv
```

Or, from a clone of this repository, `cargo install --locked --path .`. Either one puts
`officeconv` in `~/.cargo/bin`. `--locked` builds with the dependency versions in `Cargo.lock`,
the ones CI tests and checks for security advisories. The build takes several minutes, because the
release profile optimizes the whole program at once.

PDF output is included by default. To leave it out for a smaller binary, add
`--no-default-features`; `--to pdf` then says it isn't built in.

### Uninstall

Delete the binary (`rm ~/.local/bin/officeconv`), or run `cargo uninstall officeconv` if you
built it with Cargo.

## Usage

```text
officeconv [OPTIONS] --to <FORMAT> <INPUT>
```

`<INPUT>` is the file to convert, or `-` to read from [stdin](#reading-from-stdin).

| Option              | Meaning                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------- |
| `-t, --to <FORMAT>` | `csv`, `tsv`, `json`, `md` (`markdown` also works), or `pdf`                                |
| `--from <TYPE>`     | `xlsx`, `docx`, `pptx`, `csv`, or `tsv`: the input type, instead of detecting it            |
| `-o, --output PATH` | Write to a file instead of stdout. With `--all-sheets`, a directory                         |
| `--sheet NAME`      | XLSX only: which sheet to convert. Defaults to the first one                                |
| `--all-sheets`      | XLSX only: write each sheet to its own file, such as `sales-Q1.csv`                         |
| `--typed`           | JSON only: write numbers, booleans and empty cells as JSON values. Not for CSV or TSV input |
| `--no-notes`        | PPTX only: leave out speaker notes                                                          |
| `--images DIR`      | Save images into `DIR` and link them from the Markdown (not for `pdf`)                      |
| `-h, --help`        | Print help                                                                                  |
| `-V, --version`     | Print the version                                                                           |

The input type comes from the file extension. If the extension isn't one of these five (for
example `.zip`, `.xlsm`, or none at all), `officeconv` looks inside the file for an Office file
instead. A CSV or TSV file with another extension, such as `.txt`, needs `--from csv` or
`--from tsv`. DOCX and PPTX files convert to `md` or `pdf`; XLSX, CSV and TSV files convert to
everything except `pdf`.

### Reading from stdin

Use `-` as the input to read from stdin:

```sh
cat report.xlsx | officeconv - --to csv
curl -s https://example.com/deck.pptx | officeconv - --to md
```

- Stdin has no file name, so its type is worked out from its contents. The three Office formats
  are zip files, so `officeconv` looks for `xl/workbook.xml`, `word/document.xml`, or
  `ppt/presentation.xml` inside.
- CSV and TSV are plain text, with nothing to recognize them by, so they need `--from`:
  `cat contacts.csv | officeconv - --to md --from csv`.
- Every option works with stdin. With `--all-sheets`, files are named `stdin-<sheet>.<ext>`.
- Stdin is read into memory first, because zip files need random access. Office files given by
  path are read from disk as they're needed; CSV and TSV files are read into memory whole, from
  stdin or not.
- If nothing is piped in, `officeconv -` stops with an error instead of waiting for input.
- To read a file that's actually named `-`, write it as `./-`.

`--from` sets the type and skips detection. This is useful when a file's extension is misleading,
such as a workbook saved as `report.docx`:

```sh
officeconv report.docx --to csv --from xlsx
```

If `--from` doesn't match what's inside, you get an error that says what the file looks like
instead, for example `the input isn't a .docx file (it looks like a .xlsx)`.

### Examples

```sh
# First sheet as CSV, printed to the terminal
officeconv sales.xlsx --to csv

# A named sheet as a Markdown table, saved to a file
officeconv sales.xlsx --to md --sheet "Q1 2026" -o q1.md

# JSON with real numbers, booleans and nulls instead of strings
officeconv sales.xlsx --to json --typed

# Every sheet as JSON, one file per sheet, into ./out
officeconv sales.xlsx --to json --all-sheets -o out
# wrote out/sales-Q1 2026.json
# wrote out/sales-Q2 2026.json

# A Word document as Markdown
officeconv notes.docx --to md -o notes.md

# A slide deck as Markdown, one section per slide
officeconv talk.pptx --to md -o talk.md

# The same deck without speaker notes, for sharing
officeconv talk.pptx --to md --no-notes -o handout.md

# A Word document or a deck as PDF
officeconv notes.docx --to pdf -o notes.pdf
officeconv talk.pptx --to pdf --no-notes -o handout.pdf

# Keep the pictures: save them in ./notes_images and link them from notes.md
officeconv notes.docx --to md -o notes.md --images notes_images

# A sheet's pictures, listed under its Markdown table
officeconv sales.xlsx --to md -o sales.md --images sales_images

# A CSV file as a Markdown table, or as JSON
officeconv contacts.csv --to md
officeconv contacts.csv --to json -o contacts.json

# Stdout works with other tools
officeconv sales.xlsx --to csv | head -5
```

Errors go to stderr:

```text
$ officeconv notes.docx --to csv
Error: cannot convert docx to csv; docx supports: md, pdf
```

The exit status says what kind of error it was, using the codes from BSD's `sysexits.h`:

| Code | Meaning                                      | Examples                                                                                |
| ---- | -------------------------------------------- | --------------------------------------------------------------------------------------- |
| 0    | Success                                      | Also when the reader of a pipe stops early, as with `\| head`                           |
| 2    | Arguments that can't be parsed               | An unknown option, `--to xml`                                                           |
| 64   | Options that don't fit together or the input | `--typed` without `--to json`, `--to csv` for a `.docx`, an unknown `--sheet`           |
| 65   | Input officeconv can't read                  | A corrupt or unsupported file, `--from` that doesn't match, a file over the size limits |
| 66   | No input                                     | A missing or unreadable file, empty stdin                                               |
| 74   | Reading stdin or writing the output failed   | An `-o` path in a folder that doesn't exist, a full disk                                |

### Size limits

Office files are zip archives, and a small file can decompress to gigabytes. To bound how much
it decompresses, `officeconv` stops with an error when:

- one part of the file (such as `word/document.xml` or an image) decompresses to more than 256 MB
- everything it reads from one file decompresses to more than 1 GB in total, images included
- stdin has more than 1 GB
- a CSV or TSV file has more than 256 MB, the same as one part of an Office file, or would make a
  table of more than 32 million cells. Short rows count as padded to the widest one, so a small
  file with one very wide row can't turn into a huge table.

The sizes are counted as the file is read, not taken from the zip's headers, which can be faked.
For `.xlsx` files, every part is checked before the workbook is read, so files it doesn't convert,
such as embedded media, count toward the total too.

Apart from the cell count, these limits bound how much is read or decompressed, not how much
memory or output that turns into. A file within them can still use a few times as much memory as
it decompresses to, such as when PDF output decodes its pictures.

These limits are part of what makes `officeconv` safe to run on files from people you don't
trust. See [SECURITY.md](SECURITY.md) for the full list, what's out of scope, and how to report
a vulnerability.

## What gets converted

### XLSX

- The first row of the sheet becomes the header row. Short rows are padded with empty cells.
- Whole numbers have no trailing `.0`. Dates become ISO 8601 (`2026-10-01`, or
  `2026-10-01T09:30:00`), times become `HH:MM:SS`, and durations become `H:MM:SS`.
- Error cells keep their Excel text, such as `#DIV/0!`.
- CSV and TSV quote fields when needed.
- JSON is an array of objects keyed by header, in column order. Blank headers become `column_N`,
  and repeated headers become `name_2`, `name_3`, and so on. By default every value is a string.
  With `--typed`, each value keeps the type Excel stored:

  | Cell                 | Default           | `--typed`      |
  | -------------------- | ----------------- | -------------- |
  | Number               | `"12"`, `"7.5"`   | `12`, `7.5`    |
  | Boolean              | `"true"`          | `true`         |
  | Empty                | `""`              | `null`         |
  | Text, even `"00123"` | `"00123"`         | `"00123"`      |
  | Date, time, duration | `"2026-10-01"`    | `"2026-10-01"` |
  | Error                | `"#DIV/0!"`       | `"#DIV/0!"`    |

  Excel stores `12` as `12.0`, so whole numbers are written as integers. Numbers of 2^53 or more
  stay floats, since JavaScript can't hold larger integers exactly. A formula is written as the
  result Excel last calculated. If the file has none saved, the cell is empty.
- In Markdown tables, columns are padded, `|` is escaped, and line breaks inside a cell become
  `<br>`. Text that Markdown would read as formatting or HTML is escaped too, so a cell shows
  exactly what it holds.
- With `--all-sheets`, each file is named after its sheet. Characters a file name can't hold, such
  as `|`, become `_`, and trailing dots are dropped. If two sheets end up with the same file name,
  the later one gets a number added, such as `sales-a_b-2.csv`. Names count as the same when they
  differ only in case or in how an accented letter is encoded, since macOS and Windows treat
  those as one file.
- With `--images DIR`, pictures placed on the sheet are saved. Markdown lists them after the table,
  top to bottom and then left to right. CSV, TSV and JSON can't refer to images, so their data is
  unchanged and the files are only saved. See [Images](#images).

### CSV and TSV

- The first row becomes the header row, as it does for a sheet. Every value is text.
- Fields can be quoted with `"`, so a field can hold the delimiter, a line break, or a quote
  written twice (`""`). TSV follows the same rules with a tab, as Excel's "Text (Tab delimited)"
  does. A quote inside an unquoted field, as in `5" screen`, is kept as it is.
- The file must be UTF-8. A leading byte-order mark, which Excel writes when it saves "CSV UTF-8",
  is skipped. Other encodings, such as Latin-1, stop with an error that names the line.
- A quoted field that's never closed, or text right after a closing quote (`"x"y`), stops with an
  error that names the line, instead of quietly running fields together.
- Short rows are padded with empty cells. A row longer than the header widens the table instead of
  losing values: the extra columns get blank headers, which JSON names `column_N`.
- Output is the same as for a sheet: see [XLSX](#xlsx) for how CSV, TSV, JSON and Markdown are
  written. Line endings become `\n`.
- `--typed` isn't allowed: the file holds only text, and guessing types from it would turn
  `00123` into `123`. `--sheet`, `--all-sheets`, `--no-notes` and `--images` don't apply either.
- The whole table is held in memory. See [Size limits](#size-limits).

### DOCX

| Word                                 | Markdown                                                   |
| ------------------------------------ | ---------------------------------------------------------- |
| Title, Heading 1–6 (any language)    | `#` to `######`                                            |
| Bold, italic                         | `**bold**`, `*italic*`                                     |
| Bulleted and numbered lists, nested  | `- item`, `1. item`, with nested items indented 4 spaces   |
| Hyperlinks                           | `[text](url)`                                              |
| Tables                               | A Markdown table. The first row is the header              |
| Line breaks                          | A hard break (two spaces, then a newline)                  |

Images are left out unless you pass `--images DIR`; see [Images](#images).

Text that Markdown would read as formatting (`*`, `_`, `[`, or a line starting with `#`, for
example) is escaped. So is HTML: `<` is written as `&lt;`, and `&` as `&amp;` where it would start
an entity such as `&copy;`, so the text shows exactly as written.

Links are kept when they're `http`, `https` or `mailto`, or relative, such as `other.docx`. Any
other kind, such as `javascript:` or `file:`, could run code or open local files when clicked, so
its text is kept without the link, in Markdown and PDF.

Markdown has no merged cells, so a cell merged across columns keeps its text in the first one and
leaves the others empty. Every row keeps all its columns.

Not converted yet: footnotes, comments, and headers and footers; see
[ADR 0004](docs/adr/0004-document-model.md) for how they'll fit. Headings that use custom style
names aren't detected. Every numbered item is written as `1.` because Markdown renumbers lists
when it renders them.

### PPTX

Slides follow the order of the presentation, which isn't always the order of the files inside it.
Each slide becomes one section:

```markdown
## Slide 2: Highlights

- Revenue up 12%
    - Driven by EMEA
- See [the report](https://example.com/report)

### Notes

Mention the EMEA team.

---

## Slide 3 (hidden)
```

- The slide's title placeholder becomes the heading. A slide without a title is just `Slide N`.
- Content placeholders become bullet lists, keeping their indent levels. Text boxes and subtitles
  become paragraphs. Bullets and numbering set on a paragraph override those defaults.
- Bold, italic, links, line breaks, and tables convert the same way as in DOCX, merged cells
  included.
- Speaker notes go under `### Notes`, unless you pass `--no-notes`. Hidden slides are marked
  `(hidden)`.

Pictures become their own paragraph where they sit on the slide, but only with `--images DIR`.

Not converted yet: charts, SmartArt, and text or pictures inherited from the slide master or
layout; see [ADR 0004](docs/adr/0004-document-model.md) for how new content fits.

### PDF

`--to pdf` lays out the same content the Markdown output has: headings, bold and italic, lists,
links, tables, line breaks, and pictures.

- **It shows the content, not the original layout.** Word's and PowerPoint's own fonts, colors,
  margins, columns and slide designs aren't reproduced.
- DOCX becomes A4 pages with 1-inch margins. PPTX becomes one 16:9 landscape page per slide, with
  speaker notes under the slide unless you pass `--no-notes`. A slide with more text than fits
  continues onto another page.
- Numbered lists are numbered properly (`1.`, `2.`, ...), restarting at each level. Long table rows
  wrap inside their cells, and a table that runs onto another page repeats its header row.
  Links are clickable.
- Pictures are stored inside the PDF, so `--images` isn't used. PNG, JPEG, GIF and WebP pictures
  are kept; other formats, such as EMF or TIFF, are left out with a warning. So are pictures
  larger than 50 megapixels, which could take gigabytes of memory to decode.
- Pictures are drawn at the size the document shows them, scaled down if they don't fit the
  page. A picture with no size, or one over 100 inches, is drawn at its pixel size at 96 dpi.
  Cropping isn't applied: the whole picture is drawn, fitted inside the cropped size without
  being stretched.
- Text is set in [Noto Sans](https://notofonts.github.io), which is built in and covers Latin,
  Greek and Cyrillic. Characters it doesn't have, such as Chinese, Japanese, Korean or emoji, are
  drawn with a font installed on your computer. macOS and Windows always have one; on Linux, install
  a package such as `fonts-noto-cjk`. If no installed font has a character, it shows as a box, and
  `officeconv` lists the characters in a warning. Looking through the installed fonts takes about a
  second the first time a document needs one.
- A PDF is binary, so it's only written to stdout when stdout is piped or redirected
  (`officeconv notes.docx --to pdf > notes.pdf`). In a terminal, use `-o`.
- Right-to-left text, such as Arabic or Hebrew, is laid out left to right, so it comes out in the
  wrong order.

### Images

With `--images DIR`, each picture stored in a DOCX, PPTX or XLSX is saved into `DIR`, which is
created if needed, and linked from the Markdown:

```markdown
Our chart: ![Sales by region](notes_images/image1.png)
```

- The alt text is the description set in Word, PowerPoint or Excel (Alt Text). It's empty if none
  is set.
- Links are relative to the folder of the `-o` file, or to the current directory when writing to
  stdout. If `DIR` is somewhere else, the link is its absolute path.
- An image used more than once is saved once. File names are kept from the document, and a
  clash gets a number added, such as `image1-2.png`.
- Only PNG, JPEG, GIF, WebP, BMP, TIFF, EMF and WMF images are saved, recognized by their contents
  rather than their names. Each gets its format's extension, so a PNG stored as `logo.html` is
  saved as `logo.png`. Anything else, including SVG (which can contain scripts), is left out with
  a warning.
- Images linked from the web or another file, rather than stored in the document, are skipped.
- With `--all-sheets`, all sheets share `DIR`, and each sheet's Markdown links only its own
  pictures.
- Without `--images`, no files are written and images don't appear in the output.
- Not saved: charts, shapes, and Excel's in-cell pictures ("Place in Cell" or `IMAGE()`), which
  are stored differently.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers building and testing officeconv, fuzzing, comparing
two builds, how the repository is laid out, and releasing.

## License

[MIT](LICENSE). The Noto Sans fonts in `assets/fonts/`, which are built into the binary, are
licensed under the [SIL Open Font License 1.1](assets/fonts/OFL.txt).
