# 4. The document model is as rich as the most capable output

- **Status:** Accepted
- **Date:** 2026-10-07
- **Issue:** #131, split from #53. The features it shapes are #132–#136.

## Context

The DOCX and PPTX readers produce `Block`s (`src/document/mod.rs`), and two writers consume them:
Markdown (`document::markdown`) and PDF (`pdf::layout`). [ADR 0001](0001-pdf-rendering.md) built
PDF on the model Markdown already had, and the module still describes itself as "just enough
structure to write Markdown or a simple PDF". So PDF can show only what Markdown can:

- **Merged cells.** `Block::Table` is `Vec<Vec<CellRuns>>`, with no spans. A merged cell's text
  goes in its first cell and the cells it covers are left empty (#105 keeps every column), so
  PDF draws a grid where the document has one wide or tall cell.
- **Image sizes.** An image `Run` has no size. PDF draws it at its pixel size, at 0.75 points per
  pixel, scaled down to fit, so a picture shrunk to a thumbnail in Word can fill the page width.
- **Footnotes, headers and footers, and alignment** have nowhere to go, although PDF could show
  all of them and Markdown some of them.

Each of these features changes the shared model and touches both readers and both writers. In
total, `Block::Table`, `CellRuns` and image runs are used in about 50 places across six files.
Deciding the principle once, before #132–#136, keeps each feature from re-arguing it.

What matters:

- **Markdown output stays as it is**, unless an issue sets out to change it.
- **Untrusted input** ([ADR 0002](0002-untrusted-input.md)): anything new a reader takes from a
  file has to be bounded, and work mustn't grow with how often a file refers to the same thing.
- **Content, not fidelity** (ADR 0001): officeconv converts what a document says, not how it
  looks. Fonts, colors and exact positions stay out.

## Options

### A. The model follows the weakest output

Keep the model to what Markdown can show, as now. PDF stays a laid-out version of the Markdown.

- **For:** no model changes. One mental model for both outputs.
- **Against:** the features in #132–#136 can't be built, or only by giving Markdown syntax it
  doesn't have. PDF can't do better than Markdown even where the document says exactly what to
  draw.

### B. The model follows the most capable output (chosen)

The model holds everything at least one writer can show. Each writer drops, or approximates,
what it can't.

- **For:** each feature is built once, in the reader and the writers that can use it. Writers
  stay independent: adding a field changes nothing for a writer that ignores it.
- **Against:** every writer has to decide what to do with every field, even if that's to drop
  it. The model grows, and with it the code that builds and matches on it.

### C. The model follows Markdown, with PDF hints beside it

Keep `Block` as it is, and let readers return a second structure of PDF-only hints, such as
spans and sizes, keyed by block and cell.

- **For:** Markdown's code doesn't change at all.
- **Against:** two structures to keep in step, through `resolve_images` dropping blocks and
  every other transformation. It's option B with the fields in the wrong place.

## Decision

**Option B.** The model is as rich as the most capable output. Each writer drops what it can't
show.

Rules that come with it:

- **What belongs in the model:** structure and size that a writer can show and that change how
  the content reads, such as spans, display sizes, alignment and notes. Styling stays out, as
  ADR 0001 decided: no fonts, colors, margins or exact positions.
- **New fields default to today's behavior.** A span of one, no size, or left alignment means
  "as before". A reader that doesn't fill a field in yet, and a writer that ignores it, both
  behave exactly as they did.
- **Markdown output doesn't change** unless the issue says it should. A change to the model
  checks this with `tools/compare/compare.sh`.
- **Readers bound every new value when they build the model**, so writers can trust it. Spans
  stay inside the table, sizes inside a sane range, and something referred to many times, such
  as a note, is read once. Clamping happens in one place where possible, such as
  `TableBuilder` for spans.
- **Each writer says what it drops**, in the README section for that output. *(2026-10-07:
  these sections moved to [docs/formats.md](../formats.md) in #142.)*

The sketches below show how the first two features fit. They're designs, not code: #132 and
#133 implement them and can change details.

### Sketch: merged cells (#132)

A table stays a full grid, so every row keeps every column, as #105 guarantees. A cell either
has content, and says how far it spans, or is covered by such a cell:

```rust
Block::Table(Vec<Vec<TableCell>>)

pub enum TableCell {
    /// A cell starting here, `cols` columns wide and `rows` rows tall: 1 and 1 unless merged.
    Content { runs: CellRuns, cols: u32, rows: u32 },
    /// Part of a merged cell that starts above or to the left.
    Covered,
}
```

- **Readers:**
  - PPTX marks the cells a merge covers with `hMerge` or `vMerge`, which the reader already
    reads to leave them empty; they become `Covered`. It would also read `gridSpan` and
    `rowSpan` on the cell the merge starts from, for its `cols` and `rows`.
  - DOCX reads `gridSpan` and would read `vMerge`. Word marks a vertical merge as a `restart`
    cell followed by `continue` cells below it, so the row count is only known when the table
    ends. `TableBuilder` counts each column's run of `continue` cells then. That's one pass over
    the table, however many cells are merged.
- **Bounds:** `TableBuilder` clamps `cols` to the columns left in the row (as it does now) and
  `rows` to the rows left in the table. A cell that would overlap another merged cell is cut
  short. So a huge `rowSpan`, or a long run of `continue` cells, can't make a table bigger than
  its rows and columns.
- **Markdown:** `Content` writes its runs, and `Covered` an empty cell. That's exactly today's
  output.
- **PDF:**
  - Column widths come from one-column cells first. A spanning cell that needs more room widens
    its columns evenly.
  - A row-spanning cell's height is shared by its rows, with any extra added to the last.
  - Rows joined by a span move to a new page together.
  - The header row repeats on later pages only if no cell spans from it into the body.

### Sketch: image sizes (#133)

An image run gets a struct of its own, with an optional size, instead of `Option<String>`:

```rust
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
    pub image: Option<ImageRef>,
}

pub struct ImageRef {
    /// The part inside the package, then, after `resolve_images`, the Markdown link or PDF key.
    pub source: String,
    /// How big the document shows it, in EMU (914,400 to the inch), if it says.
    pub size: Option<(u32, u32)>,
}
```

EMU is what both formats store, and keeping integers lets `Run` stay `Eq`.

- **Readers:**
  - DOCX reads `cx` and `cy` from `wp:extent` in the drawing's `wp:inline` or `wp:anchor`.
  - PPTX reads them from `a:ext` in the picture's `p:spPr/a:xfrm`.
  - `xlsx::pictures` can leave `size` as `None`: only Markdown uses its pictures.
- **Bounds:** a size is kept only if both sides are above zero and at most 100 inches
  (91,440,000 EMU). Otherwise it's `None`. The size only scales an image that has already passed
  the 50-megapixel check, so it can't make decoding cost more.
- **Markdown:** ignores the size; image syntax has none. `--images` saves the same files.
- **PDF:** draws the image at its size, in points (12,700 EMU each), when it has one, and at its
  pixel size otherwise. Then, as now, it's scaled down to fit the column and the page. A size
  with a different shape from the pixels stretches the image, as Word would.

## Consequences

- **Every writer handles every field.** A new field means deciding what each writer does with
  it, and saying so in the README. *(2026-10-07: now in [docs/formats.md](../formats.md), since
  #142.)*
- **The model grows, feature by feature.** Each of #132–#136 adds to it, and the readers and
  writers that use the new field change with it.
- **Readers own the bounds.** A writer can assume spans fit the table and sizes are sane; a
  reader that adds a field adds its limit too.
- **#134–#136 follow the same rules:**
  - Footnotes become a note reference in the runs, plus the notes' blocks.
  - Headers and footers become blocks that PDF repeats and Markdown leaves out.
  - Alignment becomes a field on paragraphs and headings that Markdown ignores.

## When to revisit

- If another output is added, such as HTML or DOCX, it may be able to show more than PDF. The
  principle still holds, and the model grows to match.
- If most of the model ends up used only by PDF, consider making PDF's needs a separate,
  lowered form of the model, so the Markdown writer doesn't have to match on fields it never
  uses.
- If users ask for the original look (fonts, colors, positions), that's ADR 0001's decision to
  revisit, not this one.
