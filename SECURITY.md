# Security

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub's private vulnerability reporting](https://github.com/awcameron/officeconv/security/advisories/new),
not in a public issue. Include the input file, or how to make one, the command you ran, and what
happened.

`officeconv` is a learning project maintained by one person, so fixes are best effort. Only the
latest release gets fixes.

## What officeconv protects against

`officeconv` is meant to be safe to run on files from people you don't trust, within the limits
below. [ADR 0002](docs/adr/0002-untrusted-input.md) explains why.

| Threat | How it's handled | Where |
| --- | --- | --- |
| Using all memory: a small file that decompresses to gigabytes ("zip bomb"), or an endless stdin | Each part may decompress to 256 MB and a file to 1 GB in total, counted from the bytes actually read rather than the zip's headers. Every part of an `.xlsx` is checked before the workbook is read. Stdin stops at 1 GB. | `opc::Limits`, `xlsx::check_sizes`, `input::read_at_most` |
| Using all memory by decoding an image: a few bytes can declare any size | For PDF output, an image's dimensions are read from its header before it's decoded, and images over 50 megapixels are left out with a warning. That keeps one decode under about 800 MB. | `pdf::layout::decode_image`, `MAX_IMAGE_PIXELS` |
| Writing outside the output folder | Saved images use only the last segment of their name inside the package, with unsafe characters replaced, so `../../x` can't leave the folder. The same goes for sheet names with `--all-sheets`. | `ImageExport::export`, `output::safe_file_name` |
| Writing files that aren't images | `--images` saves only files whose bytes are a recognized image format, and names them with that format's extension, so a document can't save an `.html` page with a script. | `ImageFormat::detect` |
| Links that run code or open local files | Only `http`, `https`, `mailto` and relative links are kept. Other links, such as `javascript:`, `data:` or `file:`, become plain text. | `opc::is_safe_link` |
| Text that turns into Markdown or HTML | Text, table cells and link targets are escaped, so the Markdown shows what the document holds instead of new formatting, raw HTML or a different link. | `writers::escape_markdown_text`, `document::markdown::escape_url` |

CI also runs [`cargo deny`](deny.toml) to check every dependency against the RustSec advisory
database.

## Known gaps

These are open, and a fix for each is welcome:

- **Repeated work within the size limits** ([#63](https://github.com/awcameron/officeconv/issues/63)):
  the limits count bytes decompressed, not work done. A 1 MB `.pptx` that lists the same slide
  500,000 times stays within them, but takes minutes and gigabytes of memory.
- **No fuzzing yet** ([#50](https://github.com/awcameron/officeconv/issues/50)): the protections
  above are tested with hand-written cases only.
- **Unmaintained font crates** ([#76](https://github.com/awcameron/officeconv/issues/76)): the PDF
  feature depends on `rustybuzz` and `ttf-parser`, which won't get fixes. Neither has a known
  vulnerability.

## Out of scope

- **What other programs do with the output.** CSV, TSV and JSON keep cell values exactly as they
  are. A cell such as `=HYPERLINK(...)` stays as written, and a spreadsheet that opens the CSV may
  run it as a formula. Likewise, bugs in a Markdown renderer or PDF viewer aren't `officeconv`'s.
- **Memory use below the limits.** A file within them can still make `officeconv` use a few GB of
  memory. If that's too much for where you run it, add your own limit, such as `ulimit -v` or a
  container's memory limit.
- **The paths you choose.** `-o` and `--images` write where you tell them to, replacing files that
  are already there.
- **Fonts installed on the machine.** PDF output reads installed fonts for characters Noto Sans
  doesn't have. Those fonts are trusted.
