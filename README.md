# officeconv

A small command-line tool that converts Office files to plain-text formats:

- **XLSX** spreadsheets to **CSV**, **TSV**, **JSON**, or a **Markdown table**
- **DOCX** documents to **Markdown**

It's written in Rust as a learning project.

## Install

You need a Rust toolchain ([rustup.rs](https://rustup.rs)). From this directory:

```sh
cargo install --path .
```

That builds a release binary and puts it in `~/.cargo/bin`, so `officeconv` works from anywhere.

## Usage

```text
officeconv <INPUT> --to <FORMAT> [-o PATH] [--sheet NAME | --all-sheets]
```

| Option              | Meaning                                                                  |
| ------------------- | ------------------------------------------------------------------------ |
| `-t, --to <FORMAT>` | `csv`, `tsv`, `json`, or `md` (`markdown` also works)                    |
| `-o, --output PATH` | Write to a file instead of stdout. With `--all-sheets`, a directory      |
| `--sheet NAME`      | XLSX only: which sheet to convert. Defaults to the first one             |
| `--all-sheets`      | XLSX only: write each sheet to its own file, such as `sales-Q1.csv`      |

The input type comes from the file extension. DOCX files only convert to `md`.

### Examples

```sh
# First sheet as CSV, printed to the terminal
officeconv sales.xlsx --to csv

# A named sheet as a Markdown table, saved to a file
officeconv sales.xlsx --to md --sheet "Q1 2026" -o q1.md

# Every sheet as JSON, one file per sheet, into ./out
officeconv sales.xlsx --to json --all-sheets -o out
# wrote out/sales-Q1 2026.json
# wrote out/sales-Q2 2026.json

# A Word document as Markdown
officeconv notes.docx --to md -o notes.md

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
- JSON is an array of objects keyed by header, in column order. Every value is a string. Blank
  headers become `column_N`, and repeated headers become `name_2`, `name_3`, and so on.
- In Markdown tables, columns are padded, `|` is escaped, and line breaks inside a cell become `<br>`.

### DOCX

| Word                                 | Markdown                                                   |
| ------------------------------------ | ---------------------------------------------------------- |
| Title, Heading 1–6 (any language)    | `#` to `######`                                            |
| Bold, italic                         | `**bold**`, `*italic*`                                     |
| Bulleted and numbered lists, nested  | `- item`, `1. item`, with nested items indented 4 spaces   |
| Hyperlinks                           | `[text](url)`                                              |
| Tables                               | A Markdown table. The first row is the header              |
| Line breaks                          | A hard break (two spaces, then a newline)                  |

Text that Markdown would read as formatting (`*`, `_`, `[`, or a line starting with `#`, for
example) is escaped.

Not converted yet: images, footnotes, comments, headers and footers, and merged table cells
(they become empty cells). Headings that use custom style names aren't detected. Every numbered
item is written as `1.` because Markdown renumbers lists when it renders them.

## Development

```sh
cargo test                                   # unit and end-to-end tests
cargo clippy --all-targets -- -D warnings    # lints
cargo fmt                                    # formatting
cargo doc --no-deps --open                   # browse the code's documentation
cargo run -- sales.xlsx --to md              # run without installing
```

The tests build their own `.xlsx` and `.docx` fixtures in temporary directories, so the repo
doesn't need to contain any binary test files.

### Layout

```text
src/
  main.rs            entry point: parse arguments, run, print errors
  lib.rs             run(): validate, then hand off to a converter
  cli.rs             command-line options (clap)
  error.rs           ConvertError, with one variant per kind of failure
  input.rs           xlsx or docx detection, and which outputs each supports
  output.rs          stdout, a file, or one file per sheet
  table.rs           Table: the text grid every writer works from
  xlsx.rs            XLSX sheets to Table (calamine)
  writers.rs         Table to CSV, TSV, JSON, or Markdown
  docx/
    mod.rs           streams word/document.xml into Blocks (zip and quick-xml)
    package.rs       link, numbering, and style lookups from the rest of the .docx
    model.rs         Block, Run, ListKind
    markdown.rs      Blocks to Markdown (Word tables reuse the Markdown table writer)
tests/cli.rs         end-to-end tests that run the real binary
```
