# 2. officeconv supports untrusted input, within limits

- **Status:** Accepted
- **Date:** 2026-10-05
- **Issue:** #51, after the protections added in #46, #47 and #70.

## Context

#46 (for #43) kept link targets inside their Markdown link and dropped links with unsafe
schemes, #47 (for #44) limited how much a file may decompress to, and #70 saved only real images
under their own extension. Together they made `officeconv` behave as if it handled untrusted
files, but nothing said it did. Each module made its own choice (`opc::Limits`,
`opc::is_safe_link`, `output::safe_file_name`), and each new feature would have had to make one
too.

Office files are a common way to send someone a harmful file, and a converter is a natural thing
to run on a file before opening it. The repository is also public now, so a security report has
to be measured against something.

## Options

### A. Trusted input only

Say that `officeconv` is for your own files. Keep the limits as defense in depth, but don't treat
a crash or slowdown on a hostile file as a vulnerability.

- **For:** honest while #63 (repeated slides), #52 (image dimensions) and #50 (fuzzing) are open.
  Nothing more to maintain.
- **Against:** throws away the work in #46, #47 and #70, and doesn't match how a converter gets
  used. Future features would have no rule to follow, which is the problem #51 describes.

### B. Untrusted input, within stated limits (chosen)

Promise that a hostile file can't use more than the stated memory, write outside the chosen
folders, write a file that isn't an image, or inject links or markup into the output. List the
known gaps.

- **For:** matches what the code already does. Gives new features one rule: a file from anyone
  must stay within these promises. Gives reporters a clear line.
- **Against:** the known gaps break the promise until they're fixed, and more checks are needed:
  dependency advisories now, fuzzing later.

## Decision

**Option B.** [SECURITY.md](../../SECURITY.md) lists the threats in scope, the code that handles
each, the known gaps and what's out of scope.

Decisions that came with it:

- **`cargo deny` in CI** checks advisories, licenses and sources for every crate (`deny.toml`).
  The build pulls in 130 crates. Dependabot alerts cover them too, but don't fail CI or check
  licenses and sources. Two unmaintained crates, `rustybuzz` and `ttf-parser`, are allowed until
  krilla moves off them (#76).
- **The policies stay in their modules.** #51 suggested moving them into one module. They're
  small, each sits next to the code it protects, and SECURITY.md is the one place that lists
  them all. Revisit if a fourth or fifth policy appears.
- **Faithful data isn't changed for safety.** CSV and JSON keep cell values exactly, including
  ones that look like spreadsheet formulas. What other programs do with the output is out of
  scope.

## Consequences

- #63, #52 and #50 are security work, not just enhancements: until they're done, SECURITY.md
  lists them as gaps.
- A new advisory can fail CI on a PR that didn't touch dependencies. That's intended: fix it, or
  add it to `deny.toml` with the reason.
- A new feature that reads a new part of the file, writes a new kind of file, or adds a new
  output format has to say how it stays within SECURITY.md, and update it if it adds a promise.

## When to revisit

- If the known gaps stay open long enough that the promise misleads people, switch to option A
  until they're fixed.
- If `officeconv` becomes a library others call (#55), the limits need to be part of its API,
  as `Archive::with_limits` already is, rather than fixed defaults.
