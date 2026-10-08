# Architecture decision records

Each record explains one design decision: the context, the options compared, what was chosen and
why, and when to reconsider it.

| ADR | Decision | Status | Date |
| --- | --- | --- | --- |
| [0001](0001-pdf-rendering.md) | How officeconv renders PDF | Accepted | 2026-10-03 |
| [0002](0002-untrusted-input.md) | officeconv supports untrusted input, within limits | Accepted | 2026-10-05 |
| [0003](0003-binary-only.md) | officeconv is a binary, with no public library API | Accepted | 2026-10-06 |
| [0004](0004-document-model.md) | The document model is as rich as the most capable output | Accepted | 2026-10-07 |

## Adding one

Copy the structure of an existing record into `NNNN-short-title.md`, using the next number, and
add a row here. When a decision changes, write a new record and set the old one's status to
"Superseded by NNNN", here and in the record, rather than editing the old decision.
