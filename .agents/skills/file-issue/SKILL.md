---
name: file-issue
description: 'Files officeconv GitHub issues in the repo''s format: a priority or severity line, the problem, the change, acceptance criteria and a label. Use when the user says "create a ticket (or issue) for this", asks for a ticket before starting work, asks to split an issue or file follow-ups, and before issue-to-pr when there''s no issue yet.'
compatibility: Requires an authenticated GitHub CLI (gh).
allowed-tools: 'Read Write Bash(gh issue list *) Bash(gh issue view *) Bash(gh issue create *) Bash(gh issue comment *) Bash(gh issue close *) Bash(git log *) Bash(git grep *) Bash(.agents/skills/file-issue/scripts/check-issue.sh *)'
---

# Filing an issue

An issue here is a brief for one PR. `issue-to-pr` starts from it and treats its acceptance
criteria as the definition of done, so whoever picks it up should be able to start without asking
anything. Filing ends at the issue: the work starts when the user says "start on #N".

## Steps

1. **Gather the evidence.** Read the code, docs, issue or conversation the request comes from.
   Done when every claim the issue will make points at something checkable: a file and line, a
   command and its output, a version, an issue or PR number.
2. **Check for duplicates.** Search open and closed issues with two or three phrasings:

   ```sh
   gh issue list --state all --search "<keywords>" --limit 10
   ```

   - An open issue already covers it: show it to the user and ask whether to add to it instead.
   - A closed issue covered it: say what's different now, and link it ("Revisits #N").

   Done when each search has run and every match is either linked or ruled out.
3. **Decide how many issues.** One issue per change that can merge on its own as one PR. When a
   request mixes priorities, split it by priority too. Follow-ups noticed while working on a PR
   become their own issues, keeping the PR's scope as it was.
4. **Write each body** to a temporary file outside the repo, from the [template](#template),
   replacing every `<!-- -->` comment. Then check it:

   ```sh
   .agents/skills/file-issue/scripts/check-issue.sh <file>
   ```

   Fix every problem it lists and run it again. Done when it prints `ok`.
5. **Create each issue:**

   ```sh
   gh issue create --title "<title>" --label <label> --body-file <file>
   ```

   Pass `--label` once for each label (see [Labels](#labels)).
6. **When splitting an existing issue**, comment on it listing the new issues by priority, one
   line each, then close it as not planned:

   ```sh
   gh issue close <N> --reason "not planned" --comment "<the list>"
   ```

   This is how #53 was split into #131–#136.
7. **Report** each issue's link, title and priority or severity, one line each. Recommend which
   to start first and offer "start on #N". The turn ends here, on `main`, with no branch created.

## Template

```markdown
Priority: **P2**: <!-- one clause on why this priority -->.

<!-- The problem: what's wrong or missing, and the evidence for it. Bullets for several points,
each led by a bold phrase. -->

## Change

- **<!-- What to do -->**, with the files or areas it touches.
- **Update the docs** a user or contributor would read: the README or `docs/`, SECURITY.md,
  CONTRIBUTING.md, or an ADR for a design decision with real alternatives.

## Acceptance criteria

- [ ] <!-- An observable result: a command's output, a test, a file that exists -->
- [ ] <!-- A test covers it, in `tests/cli/` if a user would see it -->
```

- **The first line** gives the priority, or a bug's severity (see [Bugs](#bugs)). Before it,
  name where the issue came from when it isn't a direct request: `Split from #N.`,
  `Found while fixing #N.`, or ``From the adversarial review of v0.2.2 (`b38f411`).`` After it,
  name any issue that has to land first: `Needs the ADR in #131 first.`
- **The title** says what changes, in the imperative, without a type prefix: "Draw merged table
  cells as one cell in PDF".
- **Context sections** go between the problem and `## Change` when the problem needs them, such
  as `## Measured` for numbers or `## Cause` for a traced root cause.
- **`## Change` takes a note** when the change isn't an ordinary one: `## Change (no behavior
  change)` for a refactor, `## Change (a throwaway prototype, not for merging)`.
- **The acceptance criteria** are things someone can check: a command, a test, a file. "Works
  well" isn't one.
- **An open decision** names the options and recommends one, so `issue-to-pr` can go ahead with
  the recommendation.

## Priorities

| Priority | Meaning                                                                      | Example                                     |
| -------- | ---------------------------------------------------------------------------- | ------------------------------------------- |
| **P0**   | Before anything else: a broken or unsafe release, or a safeguard others need | #50, fuzzing the archive and the readers    |
| **P1**   | Next: a decision other planned work depends on, or a hardening gap           | #131, the ADR the format features build on  |
| **P2**   | A clear improvement users would notice                                       | #132, merged cells drawn as one cell in PDF |
| **P3**   | Nothing broken: new content, friction removed, or docs                       | #142, restructuring the README              |

## Bugs

A bug gives its severity instead of a priority: **high**, **medium** or **low**, with a clause
on the harm or what it depends on: `Severity: **medium**, silent data loss on macOS.`

- **The title** describes the symptom: "`--all-sheets` silently overwrites sheets whose names
  differ only in case".
- **`## Reproduce`** comes before `## Change`: the input, or how to make one, the command, and
  its output, in a code block. #62 is a good model.
- **A vulnerability** in a released version, one that a file from someone else can exploit, goes
  to a private security advisory instead, as SECURITY.md asks of everyone: tell the user, who can
  open one from the repo's Security tab. Hardening that isn't exploitable, such as #52, is an
  ordinary issue.

## Labels

| Issue                                                   | Label           |
| ------------------------------------------------------- | --------------- |
| A bug                                                   | `bug`           |
| Docs only, including an ADR                             | `documentation` |
| Anything else: features, CI, tooling, refactors, skills | `enhancement`   |

Add `good first issue` as well to a small change that needs no knowledge of the readers or the
document model, as #58 and #111 had.
