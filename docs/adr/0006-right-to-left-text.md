# 6. Right-to-left text in PDF is reordered on our own layout

- **Status:** Accepted
- **Date:** 2026-10-12
- **Issue:** #229. The line-breaking module it builds on came first, in #191.

## Context

[ADR 0001](0001-pdf-rendering.md) lays out PDF pages ourselves on krilla, and left right-to-left
text as a known gap: every line was shaped and drawn left to right, so Arabic and Hebrew came out
with their letters in reverse order. Unlike the other gaps, this one gives wrong output rather
than missing output, and a reader may not notice it.

ADR 0001's "When to revisit" named two ways to close it. One is to add bidi reordering and
right-to-left shaping to our own layout, which hadn't been tried. The other is to switch to
Typst, which the #29 prototype measured at 6.4 times the binary size and 5 times the clean
release build time.

## Options

### A. The Unicode Bidirectional Algorithm on our own layout

Resolve each paragraph's embedding levels with `unicode-bidi`, shape right-to-left runs right to
left with harfrust (which already supports it), break lines in text order as now, and reorder
each line's runs for drawing (rule L2).

- **For:** small. `unicode-bidi` is one crate with no dependencies of its own, already vetted by
  `cargo deny` as a dependency of `pdf-extract`. The release binary grows by 49 KB (0.9%, from
  5,672,672 to 5,721,864 bytes on macOS x86_64). Line breaking, links and everything else keep
  working on text positions, so the change is confined to `pdf::lines` and `Fonts::shape`.
- **Against:** it covers ordering and shaping, not the rest of right-to-left layout. A
  paragraph's direction comes from its first letter (rules P2 and P3), not from Word's
  `<w:bidi/>` or PowerPoint's `rtl="1"`, and alignment stays as the document gives it.

### B. Switch to Typst

- **For:** right-to-left layout, including paragraph direction and alignment, is solved and
  mature.
- **Against:** the costs ADR 0001 measured: a 6.4 times bigger binary and 5 times slower clean
  release builds, and generating Typst markup from the document model.

### C. Leave the gap

- **For:** no work.
- **Against:** Arabic and Hebrew PDFs stay wrong, silently.

## Decision

**A.** It fixes the order of right-to-left text for under 1% of the binary, without changing how
the rest of the layout works. Typst stays the fallback for when right-to-left layout needs more
than ordering.

## Consequences

- Each line is reordered by rule L2 after it's broken. Rule L1 needs no work, because a line
  never ends in spaces. The work is linear in the length of the line, since embedding levels go
  no higher than 125.
- `Fonts::shape` returns glyphs in text order in both directions, so line breaking slices them by
  text position as before; a right-to-left run's glyphs are reversed when the line is drawn.
- A link's area is laid out with its glyphs, so it moves with reordered text.
- Left-to-right documents lay out exactly as before.
- **Still missing:** paragraph direction and alignment from the document. A right-to-left
  paragraph that starts with a Latin word reads left to right, and a right-to-left paragraph
  Word aligns to the right by its direction stays left-aligned unless it also sets
  right alignment.

## When to revisit

- Users need paragraph direction or alignment from the document: read `<w:bidi/>` and `rtl="1"`
  first, as a reader change, before anything larger.
- Right-to-left layout needs more than ordering, such as mirrored lists and tables, or
  justification: reconsider Typst (ADR 0001's option B).
