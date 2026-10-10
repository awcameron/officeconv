# Usage

The [README](../README.md#usage) lists the options. This page covers how the input type is
chosen, reading from stdin, more examples, errors and exit status, and the size limits.
[Formats](formats.md) says what each input keeps and how each output is written.

- [Choosing the input type](#choosing-the-input-type)
- [Reading from stdin](#reading-from-stdin)
- [Examples](#examples)
- [Errors and exit status](#errors-and-exit-status)
- [Size limits](#size-limits)

## Choosing the input type

The input type comes from the file extension. If the extension isn't `.xlsx`, `.docx`, `.pptx`,
`.csv` or `.tsv` (for example `.zip`, `.xlsm`, or none at all), `officeconv` looks inside the
file for an Office file instead. A CSV or TSV file with another extension, such as `.txt`,
needs `--from csv` or `--from tsv`.

`--from` sets the type and skips detection. This is useful when a file's extension is misleading,
such as a workbook saved as `report.docx`:

```sh
officeconv report.docx --to csv --from xlsx
```

If `--from` doesn't match what's inside, you get an error that says what the file looks like
instead, for example `the input isn't a .docx file (it looks like a .xlsx)`.

## Reading from stdin

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

## Examples

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

## Errors and exit status

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

When a part of an Office file is missing or broken, the message names it:

```text
$ officeconv talk.pptx --to md
Error: could not parse ppt/slides/slide2.xml: ill-formed document: expected `</p:cSld>`, but `</p:sld>` was found
```

## Size limits

Office files are zip archives, and a small file can decompress to gigabytes. To bound how much
it decompresses, `officeconv` stops with an error when:

- one part of the file (such as `word/document.xml` or an image) decompresses to more than
  256 MiB
- everything it reads from one file decompresses to more than 1 GiB in total, images included
- stdin has more than 1 GiB
- a CSV or TSV file has more than 256 MiB, the same as one part of an Office file, or would make
  a table of more than 32 million cells. Short rows count as padded to the widest one, so a small
  file with one very wide row can't turn into a huge table.

The limits are fixed: no option raises them. The sizes are counted as the file is read, not taken
from the zip's headers, which can be faked. For `.xlsx` files, every part is checked before the
workbook is read, so files it doesn't convert, such as embedded media, count toward the total
too.

Apart from the cell count, these limits bound how much is read or decompressed, not how much
memory or output that turns into. A file within them can still use a few times as much memory as
it decompresses to, such as when PDF output decodes its pictures.

These limits are part of what makes `officeconv` safe to run on files from people you don't
trust. See [SECURITY.md](../SECURITY.md) for the full list, what's out of scope, and how to
report a vulnerability.
