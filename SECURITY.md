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
| Using all memory with a CSV or TSV file: one very wide row and many short ones pad out to a huge table | A CSV or TSV file may hold 256 MB, and its table 32 million cells, counting short rows as padded to the widest. The cell count is checked as rows are read. | `delimited::Limits` |
| Using all memory by decoding an image: a few bytes can declare any size | For PDF output, an image's dimensions are read from its header before it's decoded, and images over 50 megapixels are left out with a warning. That keeps one decode under about 800 MB. | `pdf::layout::decode_image`, `MAX_IMAGE_PIXELS` |
| Repeating work: many entries in a small file pointing at the same large part | A slide, a slide's notes, a worksheet, or a sheet's drawing is read once, however many entries point at it. A `.pptx` that lists one slide 500,000 times converts it once, and `--all-sheets` writes a worksheet listed under 20,000 names to one file. In JSON output, a header repeated across 100,000 columns is numbered as fast as 100,000 different ones. | `pptx::read_blocks`, `xlsx::read_all_sheets`, `xlsx::pictures::related_parts`, `writers::json_keys` |
| Writing outside the output folder | Saved images use only the last segment of their name inside the package, with unsafe characters replaced, so `../../x` can't leave the folder. The same goes for sheet names with `--all-sheets`. | `ImageExport::export`, `output::safe_file_name` |
| Writing files that aren't images | `--images` saves only files whose bytes are a recognized image format, and names them with that format's extension, so a document can't save an `.html` page with a script. | `ImageFormat::detect` |
| Links that run code or open local files | Only `http`, `https`, `mailto` and relative links are kept. Other links, such as `javascript:`, `data:` or `file:`, become plain text. | `opc::is_safe_link` |
| Text that turns into Markdown or HTML | Text, table cells and link targets are escaped, so the Markdown shows what the document holds instead of new formatting, raw HTML or a different link. | `writers::escape_markdown_text`, `document::markdown::escape_url` |

CI also runs [`cargo deny`](deny.toml) to check every dependency against the RustSec advisory
database. The zip reader and the four readers are fuzzed, with what they read rendered in every
output format; see [Fuzzing](CONTRIBUTING.md#fuzzing).

## Verifying a download

Each release archive has a SHA-256 checksum and, for releases after v0.3.5, a build provenance
attestation signed through [Sigstore](https://www.sigstore.dev). The attestation records that the
release workflow (`.github/workflows/release.yml`) built the archive, and from which commit.
Check one with the GitHub CLI:

```sh
gh attestation verify officeconv-<target>.tar.gz -R awcameron/officeconv
```

The checksum only shows a download wasn't damaged: anyone who could replace an archive on the
release page could replace its checksum too. The attestation can't be replaced that way:
`gh attestation verify` only accepts one signed by a workflow run in this repository.

## Known gaps

None are open right now.

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
