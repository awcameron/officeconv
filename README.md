# officeconv

A small command-line tool that converts Office files to plain-text formats:

- **XLSX** spreadsheets to **CSV**, **TSV**, **JSON**, or a **Markdown table**
- **DOCX** documents to **Markdown**
- **PPTX** presentations to **Markdown**

It's written in Rust as a learning project.

## Install

You need a Rust toolchain ([rustup.rs](https://rustup.rs)). From this directory:

```sh
cargo install --path .
```

That builds a release binary and puts it in `~/.cargo/bin`, so `officeconv` works from anywhere.

## Usage

```text
officeconv <INPUT | -> --to <FORMAT> [--from TYPE] [-o PATH] [--sheet NAME | --all-sheets] [--typed] [--no-notes] [--images DIR]
```

| Option              | Meaning                                                                  |
| ------------------- | ------------------------------------------------------------------------ |
| `-t, --to <FORMAT>` | `csv`, `tsv`, `json`, or `md` (`markdown` also works)                    |
| `--from <TYPE>`     | `xlsx`, `docx`, or `pptx`: the input type, instead of detecting it       |
| `-o, --output PATH` | Write to a file instead of stdout. With `--all-sheets`, a directory      |
| `--sheet NAME`      | XLSX only: which sheet to convert. Defaults to the first one             |
| `--all-sheets`      | XLSX only: write each sheet to its own file, such as `sales-Q1.csv`      |
| `--typed`           | JSON only: write numbers, booleans and empty cells as JSON values        |
| `--no-notes`        | PPTX only: leave out speaker notes                                       |
| `--images DIR`      | Save images into `DIR` and link them from the Markdown                   |

The input type comes from the file extension. If the extension isn't one of these three (for
example `.zip`, `.xlsm`, or none at all), `officeconv` looks inside the file instead. DOCX and PPTX
files only convert to `md`.

### Reading from stdin

Use `-` as the input to read from stdin:

```sh
cat report.xlsx | officeconv - --to csv
curl -s https://example.com/deck.pptx | officeconv - --to md
```

- Stdin has no file name, so its type is worked out from its contents. All three formats are zip
  files, so `officeconv` looks for `xl/workbook.xml`, `word/document.xml`, or
  `ppt/presentation.xml` inside.
- Every option works with stdin. With `--all-sheets`, files are named `stdin-<sheet>.<ext>`.
- Stdin is read into memory first, because zip files need random access. Files given by path are
  read from disk.
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

# Keep the pictures: save them in ./notes_images and link them from notes.md
officeconv notes.docx --to md -o notes.md --images notes_images

# A sheet's pictures, listed under its Markdown table
officeconv sales.xlsx --to md -o sales.md --images sales_images

# Stdout works with other tools
officeconv sales.xlsx --to csv | head -5
```

Errors go to stderr and exit with status 1:

```text
$ officeconv notes.docx --to csv
Error: cannot convert docx to csv; docx supports: md
```

## What gets converted

### XLSX

- The first row of the sheet becomes the header row. Short rows are padded with empty cells.
- Whole numbers have no trailing `.0`. Dates become ISO 8601 (`2026-10-01`, or `2026-10-01T09:30:00`),
  times become `HH:MM:SS`, and durations become `H:MM:SS`.
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
- In Markdown tables, columns are padded, `|` is escaped, and line breaks inside a cell become `<br>`.
- With `--images DIR`, pictures placed on the sheet are saved. Markdown lists them after the table,
  top to bottom and then left to right. CSV, TSV and JSON can't refer to images, so their data is
  unchanged and the files are only saved. See [Images](#images).

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
example) is escaped.

Not converted yet: footnotes, comments, headers and footers, and merged table cells
(they become empty cells). Headings that use custom style names aren't detected. Every numbered
item is written as `1.` because Markdown renumbers lists when it renders them.

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
- Bold, italic, links, line breaks, and tables convert the same way as in DOCX.
- Speaker notes go under `### Notes`, unless you pass `--no-notes`. Hidden slides are marked
  `(hidden)`.

Pictures become their own paragraph where they sit on the slide, but only with `--images DIR`.

Not converted yet: charts, SmartArt, and text or pictures inherited from the slide master or
layout.

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
- Images linked from the web or another file, rather than stored in the document, are skipped.
- With `--all-sheets`, all sheets share `DIR`, and each sheet's Markdown links only its own
  pictures.
- Without `--images`, no files are written and images don't appear in the output.
- Not saved: charts, shapes, and Excel's in-cell pictures ("Place in Cell" or `IMAGE()`), which
  are stored differently.

## Development

```sh
cargo test                                   # unit and end-to-end tests
cargo clippy --all-targets -- -D warnings    # lints
cargo fmt                                    # formatting
cargo doc --no-deps --open                   # browse the code's documentation
cargo run -- sales.xlsx --to md              # run without installing
```

The tests build their own `.xlsx`, `.docx`, and `.pptx` fixtures in temporary directories, so the repo
doesn't need to contain any binary test files.

Design decisions are recorded in [`docs/adr/`](docs/adr/), starting with
[how PDF output is rendered](docs/adr/0001-pdf-rendering.md).

### Layout

```text
src/
  main.rs            entry point: parse arguments, run, print errors
  lib.rs             run(): validate, then hand off to a converter
  cli.rs             command-line options (clap)
  format.rs          OutputFormat: csv, tsv, json, md
  error.rs           ConvertError, with one variant per kind of failure
  images.rs          saves pictures from a .docx, .pptx or .xlsx and works out their links
  input.rs           the input (a file or stdin), its type, and which outputs each supports
  opc.rs             zip parts, relationships, and the XmlHandler event loop
  output.rs          stdout, a file, or one file per sheet
  table.rs           Table and Cell: the grid every writer works from
  xlsx/
    mod.rs           XLSX sheets to Table (calamine)
    pictures.rs      finds the pictures on a sheet through its drawing part
  writers.rs         Table to CSV, TSV, JSON, or Markdown
  document/
    mod.rs           Block, Run, ListKind: the model DOCX and PPTX both produce
    markdown.rs      Blocks to Markdown (tables reuse the Markdown table writer)
  docx/
    mod.rs           streams word/document.xml into Blocks (quick-xml)
    package.rs       numbering and style lookups from the rest of the .docx
  pptx.rs            reads slides in presentation order, with their notes, into Blocks
tests/cli.rs         end-to-end tests that run the real binary
```

## License

[MIT](LICENSE)
