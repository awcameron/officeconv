# 1. How officeconv renders PDF

- **Status:** Accepted
- **Date:** 2026-10-03
- **Issue:** #27, for the feature in #13 (implemented in #21–#26)

This record was written after the feature was built. The choice below was made while
implementing #13 rather than compared up front; this document compares it with the alternatives
so later changes have the context.

## Context

`officeconv` converts DOCX and PPTX into a small document model (`Block`: headings, paragraphs,
list items, tables, rules, with runs of bold, italic, linked or image content) and writes that
model as Markdown. #13 asked for PDF output from the same model, so documents and decks can be
shared with people who don't have Office.

What mattered:

- **One self-contained binary.** Everything so far is pure Rust with no external programs, and CI
  builds and tests on Linux, macOS and Windows.
- **Content, not fidelity.** The Markdown output already drops Word's fonts, colors and layout.
  #13 accepted the same for PDF: it shows the converted content, laid out simply.
- **Binary size.** The release binary was 1.5 MB (stripped, with LTO).
- **Maintenance.** This is a small project; whatever we choose, we maintain.

## Options

### A. Our own layout on krilla (chosen)

[krilla](https://crates.io/crates/krilla) writes PDF (fonts, subsetting, images, links) but does no
layout, so we lay out the `Block` model ourselves (`src/pdf/layout.rs`): line breaking with
`unicode-linebreak`, shaping with rustybuzz, list numbering, table sizing and pagination.

- **For:** pure Rust, one binary, no external install. Works directly from our model, with no
  intermediate format. Layout is separate from painting, so it's unit-tested without reading
  PDFs back.
- **Against:** we own a small typesetting engine: about 850 lines of layout (excluding tests), plus
  about 250 for fonts and 160 for painting. Every new layout need is ours to build.
- **Size (measured):** 5.4 MB release binary, +3.9 MB over the 1.5 MB without PDF: about 2 MB of
  code (krilla, skrifa, rustybuzz, fontdb, image decoders) and 0.92 MB of compressed fonts. Before
  stripping, LTO and font compression (#26) it was 8.2 MB.

### B. Typst as a library

Generate Typst markup from the `Block` model and compile it with the `typst` crate.

- **For:** layout is solved and mature, including font fallback, right-to-left and complex
  scripts, hyphenation, justification, and tables that split across pages. Almost no layout code
  of our own.
- **Against:** a much larger dependency tree, binary and build time (not measured here). A second
  document language between our model and the PDF, with escaping to get right. We'd track Typst's
  library API, which isn't promised to be stable.

### C. Shell out to LibreOffice

Run `soffice --headless --convert-to pdf` on the original file.

- **For:** by far the best fidelity: Office's own fonts, layout, headers, footers, charts and
  SmartArt.
- **Against:** needs a separate LibreOffice install of several hundred MB. It doesn't work from
  stdin the way the rest of `officeconv` does, and it bypasses our model, so PDF output wouldn't
  match the Markdown. It's slow to start, and hard to run in CI on all three systems.

### D. Don't build PDF

Document a pipeline: `officeconv notes.docx --to md | pandoc -o notes.pdf`.

- **For:** no code and no size cost.
- **Against:** users need pandoc and a PDF engine for it (such as LaTeX or Typst). Pictures need
  `--images` and correct relative paths. It isn't the one-step conversion #13 asked for.

## Decision

**Option A: our own layout on krilla.** It's the only option that keeps `officeconv` a single
self-contained binary with PDF built in. #13 asks for the converted content rather than
fidelity, so the layout needs are small and known: headings, paragraphs, lists, tables, images,
links and slides.

Decisions that came with it:

- **Fonts:** Noto Sans is bundled in four styles (OFL), covering Latin, Greek and Cyrillic, so most
  documents look the same everywhere. Other characters (CJK, emoji, symbols such as `→`) come from
  installed fonts, searched only when needed. Bundling a CJK font was rejected: about 20–30 MB more.
- **Missing characters warn instead of failing:** they're drawn as boxes and listed on stderr, so
  one stray emoji doesn't stop a long conversion.
- **Pages:** A4 with 1-inch margins for DOCX, and one 16:9 page per slide for PPTX.
- **Size:** a release profile with strip, LTO and one codegen unit, and fonts compressed at build
  time (#26). A `pdf` Cargo feature, on by default, lets builds leave PDF out and stay at 1.5 MB.

## Consequences

- **Known gaps, and ours to fix:**
  - Right-to-left text (Arabic, Hebrew) is laid out left to right, so it comes out in the wrong
    order.
  - Table rows taller than a page run off the bottom instead of splitting.
  - No hyphenation, justification, footnotes, or headers and footers.
- **Font search is slow:** the first character Noto Sans lacks costs about 1 s on macOS, spent
  listing installed fonts. Even one `→` triggers it.
- **Output depends on the machine:** the same input can give a different PDF on different machines,
  because installed fallback fonts differ.
- **Release builds are slower:** LTO makes them take about twice as long. Debug builds and tests
  are unaffected.

## When to revisit

Reconsider **Typst (B)** if any of these happen:

- Right-to-left or complex-script support becomes a requirement.
- Layout bugs or feature requests (footnotes, page-splitting rows, justification) start taking a
  steady share of the work.
- The size budget stops mattering.

Reconsider **LibreOffice (C)**, as an optional backend, if users need PDFs that look like the
original document rather than its content.
