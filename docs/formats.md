# Formats

What `officeconv` reads from each kind of file, and how it writes each output. The
[README](../README.md) covers installing and running it, and [Usage](usage.md) covers the
options in detail.

- [Inputs](#inputs): [XLSX](#xlsx), [CSV and TSV](#csv-and-tsv-files), [DOCX](#docx),
  [PPTX](#pptx)
- [Outputs](#outputs): [CSV and TSV](#csv-and-tsv), [JSON](#json), [Markdown](#markdown),
  [PDF](#pdf), [images](#images)

## Inputs

What `officeconv` reads from each kind of file. [Outputs](#outputs) says how it's written.

### XLSX

- The first row of the sheet becomes the header row. Short rows are padded with empty cells.
- Whole numbers have no trailing `.0`: Excel stores `12` as `12.0`, and it's written as `12`.
  Dates become ISO 8601 (`2026-10-01`, or `2026-10-01T09:30:00`), times become `HH:MM:SS`, and
  durations become `H:MM:SS`.
- Each cell keeps the type Excel stored (number, boolean, text, date, error or empty), which
  [`--typed` JSON](#json) uses.
- Error cells keep their Excel text, such as `#DIV/0!`.
- A formula is written as the result Excel last calculated. If the file has none saved, the cell
  is empty.
- With `--all-sheets`, each file is named after its sheet. Characters a file name can't hold, such
  as `|`, become `_`, and trailing dots are dropped. If two sheets end up with the same file name,
  the later one gets a number added, such as `sales-a_b-2.csv`. Names count as the same when they
  differ only in case or in how an accented letter is encoded, since macOS and Windows treat
  those as one file.
- With `--images DIR`, pictures placed on the sheet are saved. Markdown lists them after the
  table, top to bottom and then left to right. CSV, TSV and JSON can't refer to images, so their
  data is unchanged and the files are only saved. See [Images](#images).

### CSV and TSV files

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
- `--typed` isn't allowed: the file holds only text, and guessing types from it would turn
  `00123` into `123`. `--sheet`, `--all-sheets`, `--no-notes` and `--images` don't apply either.
- The whole table is held in memory. See [Size limits](usage.md#size-limits).

### DOCX

`officeconv` keeps:

- the Title style and Heading 1–6, in any language;
- bold and italic;
- bulleted and numbered lists, nested;
- hyperlinks;
- tables, with the first row as the header;
- line breaks;
- footnotes and endnotes;
- headers and footers, in PDF only;
- paragraph alignment, set on the paragraph or its style, in PDF only;
- pictures, with `--images DIR` or in PDF. See [Images](#images).

[Markdown](#markdown) shows how each is written.

Footnotes and endnotes are numbered together, from 1, in the order the text first refers to
them, and all of them go at the end of the document. A note referred to more than once keeps
its number, and is written once. Notes nothing refers to are left out, and so is a reference to
a note the file doesn't have.

PDF repeats the first section's header and footer on every page. Word can give a section a
different header for its first page or for even pages, and each section its own; those aren't
used. Page numbers and page counts in a header or footer are filled in for each page, so a footer
reads "Page 3 of 9" on the third of nine pages, whatever number Word saved. Markdown leaves
headers and footers out, and `--images` doesn't save their pictures.

A table cell merged across columns or down rows keeps its text in its first cell, so every row
keeps all its columns. Markdown has no merged cells, so it leaves the cells the merge covers
empty. PDF draws the merged cell once, across all its columns and rows. A merge never reaches
past the table, however far the file says it goes, and a continuing cell that holds text of its
own starts a new cell rather than lose it. A table inside a table cell can't be shown either, so
its text goes into that cell, a line for each of its cells.

Not converted yet: comments; see
[ADR 0004](adr/0004-document-model.md) for how they'll fit. Headings that use custom style
names aren't detected.

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
- Bold, italic, links, line breaks, and tables are kept as in [DOCX](#docx), merged cells
  included.
- Paragraph alignment set on the paragraph itself is kept in PDF. Alignment a paragraph takes
  from the slide layout or master, as many titles do, isn't read.
- Speaker notes go under `### Notes`, unless you pass `--no-notes`. Hidden slides are marked
  `(hidden)`.
- Pictures become their own paragraph where they sit on the slide, with `--images DIR` or in
  PDF.

Not converted yet: charts, SmartArt, and text or pictures inherited from the slide master or
layout; see [ADR 0004](adr/0004-document-model.md) for how new content fits.

## Outputs

Every output shows the same content, and each leaves out what it can't.

Links are kept when they're `http`, `https` or `mailto`, or relative, such as `other.docx`. Any
other kind, such as `javascript:` or `file:`, could run code or open local files when clicked, so
its text is kept without the link, in Markdown and PDF.

### CSV and TSV

- The first row is the header row.
- Fields are quoted when needed.
- Line endings are `\n`.

### JSON

JSON is an array of objects keyed by header, in column order. Blank headers become `column_N`,
and repeated headers become `name_2`, `name_3`, and so on. By default every value is a string.
With `--typed` (XLSX only), each value keeps the type Excel stored:

| Cell                 | Default           | `--typed`      |
| -------------------- | ----------------- | -------------- |
| Number               | `"12"`, `"7.5"`   | `12`, `7.5`    |
| Boolean              | `"true"`          | `true`         |
| Empty                | `""`              | `null`         |
| Text, even `"00123"` | `"00123"`         | `"00123"`      |
| Date, time, duration | `"2026-10-01"`    | `"2026-10-01"` |
| Error                | `"#DIV/0!"`       | `"#DIV/0!"`    |

Whole numbers are written as integers. Numbers of 2^53 or more stay floats, since JavaScript
can't hold larger integers exactly.

### Markdown

| Document                         | Markdown                                                 |
| -------------------------------- | -------------------------------------------------------- |
| Title, Heading 1–6               | `#` to `######`                                          |
| Bold, italic                     | `**bold**`, `*italic*`                                   |
| Bulleted and numbered lists      | `- item`, `1. item`, with nested items indented 4 spaces |
| Hyperlinks                       | `[text](url)`                                            |
| Tables, and spreadsheets         | A Markdown table. The first row is the header            |
| Line breaks                      | A hard break (two spaces, then a newline)                |
| Footnotes and endnotes           | `[^1]` in the text, and `[^1]: note` at the end          |
| Pictures, with `--images DIR`    | `![alt text](DIR/image1.png)`; see [Images](#images)     |

- Every numbered item is written as `1.`, because Markdown renumbers lists when it renders them.
- Text that Markdown would read as formatting (`*`, `_`, `[`, or a line starting with `#`, for
  example) is escaped. So is HTML: `<` is written as `&lt;`, and `&` as `&amp;` where it would
  start an entity such as `&copy;`, so the text shows exactly as written.
- In tables, columns are padded, `|` is escaped, and line breaks inside a cell become `<br>`.
- A note of more than one paragraph indents the rest by 4 spaces, so they stay in the note.
- DOCX headers and footers are left out: a Markdown file has no pages to repeat them on.
- Alignment is left out too: Markdown has no way to center or right-align a paragraph.
- [PPTX](#pptx) shows how slides are laid out.

### PDF

`--to pdf` lays out the same content the Markdown output has: headings, bold and italic, lists,
links, tables, line breaks, footnotes and endnotes, and pictures.

- **It shows the content, not the original layout.** Word's and PowerPoint's own fonts, colors,
  margins, columns and slide designs aren't reproduced.
- DOCX becomes A4 pages with 1-inch margins. PPTX becomes one 16:9 landscape page per slide, with
  speaker notes under the slide unless you pass `--no-notes`. A slide with more text than fits
  continues onto another page.
- Numbered lists are numbered properly (`1.`, `2.`, ...), restarting at each level. Long table rows
  wrap inside their cells, and a table that runs onto another page repeats its header row,
  unless a merged cell joins it to the rows below. Links are clickable.
- A merged table cell is drawn once, across its columns and rows, and the rows it joins go onto
  the same page. If they're too tall to fit on any page, they're drawn as separate rows instead,
  with the text in the first, as in Markdown.
- Centered and right-aligned paragraphs and headings are laid out that way. Justified text is
  left-aligned, since stretching the spaces between words isn't built.
- A DOCX header is drawn at the top of every page and a footer at the bottom, in the margins:
  from halfway into the margin, as Word places them, to just short of the text. A header or
  footer taller than that is cut to what fits, rather than pushing the text down as Word does,
  and only its first 1,000 characters are used.
- A footnote or endnote reference is written as its number in brackets, `[1]`, on the line rather
  than raised. The notes go at the end of the document, below a line, each starting with its
  number. They aren't put at the foot of the page that refers to them.
- Pictures are stored inside the PDF, so `--images` isn't used. PNG, JPEG, GIF and WebP pictures
  are kept; other formats, such as EMF or TIFF, are left out with a warning. So are pictures
  larger than 50 megapixels, which could take gigabytes of memory to decode.
- Pictures are drawn at the size the document shows them, scaled down if they don't fit the
  page. A picture with no size, or one over 100 inches, is drawn at its pixel size at 96 dpi.
  Cropping isn't applied: the whole picture is drawn, fitted inside the cropped size without
  being stretched.
- Text is set in [Noto Sans](https://notofonts.github.io), which is built in and covers Latin,
  Greek and Cyrillic. Characters it doesn't have, such as Chinese, Japanese, Korean or emoji, are
  drawn with a font installed on your computer. macOS and Windows always have one; on Linux,
  install a package such as `fonts-noto-cjk` (`font-noto-cjk` on Alpine). If no installed font
  has a character, it shows as a box, and `officeconv` lists the characters in a warning. Looking
  through the installed fonts takes about a second the first time a document needs one.
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
