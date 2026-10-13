# 7. Readers decide what kind of block a paragraph is

- **Status:** Accepted
- **Date:** 2026-10-13
- **Issue:** #244, recording a choice left open in #236 and made in #242.

## Context

The DOCX and PPTX readers build blocks through `document::builder::BlockBuilder`
([ARCHITECTURE.md](../../ARCHITECTURE.md#documents)). The builder keeps what's open, the
paragraph, the table and its cells, the current run's formatting and link, and puts a finished
paragraph inside a table into the open cell. A finished paragraph that isn't in a table goes
back to the reader: `BlockBuilder::end_paragraph` returns it, and the reader decides what it
becomes.

- **DOCX** decides straight away, in `Parser::classify` (`src/docx/mod.rs`). Whether the
  paragraph is a heading, a list item or a plain paragraph depends on its style and numbering,
  looked up in the package's styles and numbering parts.
- **PPTX** keeps the paragraph in the open shape and decides in `SlideParser::finish_shape`
  (`src/pptx.rs`), from the shape's placeholder type:
  - a title placeholder's paragraphs don't become blocks: they're joined into the slide's
    title, which goes in the slide's heading;
  - in speaker notes, only the body placeholder's paragraphs are kept;
  - otherwise a body or content placeholder bullets its paragraphs unless one turns bullets off.

Both end in `Block::list_item_or_paragraph`, so the last step is shared. Still, two readers each
routing their own paragraphs looks like duplication, and the architecture review of `418ff69`
flagged it: the builder's own module doc said it owned "where finished content goes". #236
asked whether it should, and #242 kept routing in the readers.

## Options

### A. The reader routes the paragraph (chosen)

`end_paragraph` returns a paragraph that isn't in a table, and the reader turns it into a block,
or into something else, and pushes it.

- **For:** each reader keeps what only it knows. DOCX needs the package's styles and numbering;
  PPTX needs the shape, and sometimes makes no block at all. The builder stays free of anything
  specific to Word or PowerPoint.
- **Against:** two readers each route paragraphs, though both end in one shared function.

### B. The builder routes it through a rule the reader supplies

The reader gives the builder a function from a finished paragraph to a block, and
`end_paragraph` pushes what it returns.

- **For:** one place pushes finished paragraphs. DOCX's rule could borrow the package.
- **Against:** a paragraph doesn't map to one block in PPTX. A title's paragraphs are joined
  across the shape into a title that isn't a block, and a notes shape that isn't the body
  placeholder drops its paragraphs, so the rule would have to return "no block" and keep slide
  state, which the reader still owns. The rule would also run as each paragraph ends, so it
  would depend on the placeholder (`p:nvSpPr`) coming before the text (`p:txBody`). PowerPoint
  writes them in that order, but deciding at the shape's end doesn't need it.

### C. The builder holds shape-level state for PPTX

The builder learns about shapes: it collects a shape's paragraphs, and the reader tells it the
placeholder type when the shape ends.

- **For:** PPTX's stash moves into the builder.
- **Against:** puts a PowerPoint concept, and the slide title, into the builder DOCX shares. DOCX
  has nothing like a shape around its paragraphs, so the extra interface would serve one reader.

## Decision

**Option A.** The builder routes what doesn't depend on the format: paragraphs into table cells,
nested tables into their outer cell, finished tables into blocks. The reader decides what a
paragraph outside a table becomes, and `Block::list_item_or_paragraph` makes the final choice
between a list item and a paragraph for both readers.

## Consequences

- **A new document reader** gets paragraphs back from `end_paragraph` and decides what each one
  is, using `Block::list_item_or_paragraph` for list items.
- **A routing rule that every reader shares**, such as a new kind of block made the same way
  from any format, goes into the builder or the model, not into each reader.
- **Two `match`es on paragraphs stay**, one per reader. That's by design, not duplication to
  remove.

## When to revisit

- If PPTX stops needing the whole shape: for example, if slide titles became ordinary heading
  blocks, and notes kept every placeholder's text.
- If a third document reader is added and routes its paragraphs the same way as one of these,
  consider moving that shared way into the builder.
