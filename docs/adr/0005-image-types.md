# 5. Images a reader finds and images a writer shows are different types

- **Status:** Accepted
- **Date:** 2026-10-08
- **Issue:** #150

## Context

A DOCX or PPTX reader finds each picture as a part inside the package, such as
`word/media/image1.png`. What becomes of it depends on the output: `--images` saves it and links
it from the Markdown, PDF embeds its bytes, and plain Markdown leaves it out. Until #150, each
reader took that choice as an `images::Images` argument and resolved its images itself, and
`ImageRef.source` held the part before that step and a link or embedded key after it, with
nothing in its type to say which.

#150 moves resolving into one step after reading, so the readers no longer need the choice. That
leaves blocks in two states: as a reader returns them, and as a writer takes them. Passing the
first kind to a writer would write package paths as links, or look up keys that don't exist.

## Options

### A. A wrapper around what the readers return

The readers return a type that only the resolving step can open, holding ordinary `Block`s.

- **For:** the model doesn't change. Writers can't be given unresolved blocks.
- **Against:** inside the readers and the resolving step, `source` still means two things. The
  guarantee holds at the readers' edge only, so any new code that builds blocks outside a reader
  has to know to wrap them.

### B. Two variants of the image

`ImageRef` becomes an enum: a part, or a link.

- **For:** the field says what it holds.
- **Against:** every writer has to handle a part it should never see, at run time. The compiler
  can't reject the mistake.

### C. The model is generic over its image (chosen)

`Block`, `Run` and `TableCell` take the image type as a parameter. Readers build them with
`ImagePart`, the part and its display size; `document::resolve_images` turns them into
`ImageRef`, the link or key, which is the default parameter.

- **For:** each image says what it holds, and the compiler rejects a block of `ImagePart`s
  passed to a writer. Writers don't change: `Block` still means `Block<ImageRef>`.
- **Against:** readers and the builder write `Block<ImagePart>`, and the model's helpers, such as
  `append_run`, are generic.

## Decision

**Option C.** Readers return `Block<ImagePart>`. `images::resolve` reads each image from the
same `opc::Archive` the reader used, so the archive's size limits still count them (ADR 0002),
and returns `Block`s for the writers. The caller opens the archive and lends it to the reader.

## Consequences

- **A new reader** returns `Block<ImagePart>` and never decides what happens to images.
- **A new output** decides that in `images::resolve`, or in a new `images::Images` variant.
- **Something new in the model that holds an image** takes the image type as a parameter too,
  so the compiler keeps checking it.

## When to revisit

- If the model gains more state that changes between reading and writing, consider one marker
  type for "as read" and "ready to write" instead of a parameter per concern.
