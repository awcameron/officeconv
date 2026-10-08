---
name: file-issue
description: 'Files officeconv GitHub issues in the repo''s format: a priority line, the problem, the change, acceptance criteria and a label. Use when the user says "create a ticket (or issue) for this", asks for a ticket before starting work, asks to split an issue or file follow-ups, and before issue-to-pr when there''s no issue yet.'
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

   Leave out `--label` for a change with no label (see [Labels](#labels)).
6. **When splitting an existing issue**, comment on it listing the new issues by priority, one
   line each, then close it as not planned:

   ```sh
   gh issue close <N> --reason "not planned" --comment "<the list>"
   ```

   This is how #53 was split into #131–#136.
7. **Report** each issue's link, title and priority, one line each. Recommend which to start
   first and offer "start on #N". The turn ends here, on `main`, with no branch created.

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

- **The first line** names the priority. An issue split from another starts
  `Split from #N. Priority: **P1**.`
- **The title** says what changes, in the imperative, without a type prefix: "Draw merged table
  cells as one cell in PDF".
- **The acceptance criteria** are things someone can check: a command, a test, a file. "Works
  well" isn't one.
- **An open decision** names the options and recommends one, so `issue-to-pr` can go ahead with
  the recommendation.

## Priorities

| Priority | Meaning                                                                     | Example                                     |
| -------- | --------------------------------------------------------------------------- | ------------------------------------------- |
| **P1**   | Broken or unsafe behavior, or a decision that other planned work depends on | #131, the ADR the format features build on  |
| **P2**   | A clear improvement users would notice                                      | #132, merged cells drawn as one cell in PDF |
| **P3**   | Nothing broken: new content, friction removed, or docs                      | #142, restructuring the README              |

A security vulnerability isn't filed as an issue at all: point the user to the private report in
SECURITY.md.

## Labels

The label matches the commit type the change will use, the same mapping `issue-to-pr` uses for
PRs, so the release notes group them:

| Change                       | Commit type                       | Label           |
| ---------------------------- | --------------------------------- | --------------- |
| New behavior, or a new skill | `feat`                            | `enhancement`   |
| A bug fix                    | `fix`                             | `bug`           |
| Docs only                    | `docs`                            | `documentation` |
| CI, tooling, refactoring     | `ci`, `chore`, `refactor`, `test` | none            |
