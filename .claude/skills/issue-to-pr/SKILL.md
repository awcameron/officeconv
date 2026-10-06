---
name: issue-to-pr
description: 'Takes an officeconv GitHub issue to a merged PR in three steps. Use when the user says "start on #N", "push it and open the PR", or "#N merged now" (or "merged"), and for any change made on its own branch in this repo.'
compatibility: Requires git, an authenticated GitHub CLI (gh), a Rust toolchain, and Python 3 for tools/compare.
allowed-tools: 'Read Edit Write Bash(git status) Bash(git status *) Bash(git diff *) Bash(git log *) Bash(git show *) Bash(git describe *) Bash(git switch *) Bash(git pull *) Bash(git add *) Bash(git commit *) Bash(git push -u origin *) Bash(gh issue view *) Bash(gh issue list *) Bash(gh pr create *) Bash(gh pr view *) Bash(gh pr checks *) Bash(gh run view *) Bash(cargo fmt *) Bash(cargo clippy *) Bash(cargo doc *) Bash(cargo test *) Bash(cargo build *) Bash(cargo deny *) Bash(tools/compare/compare.sh) Bash(tools/compare/compare.sh *) Bash(.claude/skills/issue-to-pr/scripts/after-merge.sh *)'
---

# Issue to merged PR

Changes to officeconv go through three steps, each started by the user. Do one step, report,
and wait: don't push, open a PR or clean up until the user asks for that step.

## 1. Start on an issue ("start on #N")

1. Read the issue: `gh issue view N`. Its acceptance criteria are the definition of done. If it
   leaves a decision open and the code doesn't settle it, pick the option the issue recommends
   and say so in the report. Ask only if the choice is the user's to make.
2. Start from an up-to-date `main`, then branch:

   ```sh
   git switch main && git pull --quiet && git switch -c <type>/<N>-<short-slug>
   ```

   `<type>` matches the commit type: `feat`, `fix`, `refactor`, `test`, `ci`, `docs` or `chore`.
3. Make the change. Match the surrounding code: its comment density, naming and idiom.
   - Add or update tests for any behavior change. CLI behavior is tested in `tests/cli/`,
     which runs the real binary. Reader details are tested in unit tests next to the code.
   - Update the docs the change touches:
     - the README for anything a user sees;
     - `SECURITY.md` when the change affects how untrusted input is handled;
     - a new ADR in `docs/adr/` (see its README) for a design decision with real alternatives.
4. If the change touches a reader (`src/opc.rs`, `src/docx/`, `src/pptx.rs`, `src/xlsx/`) or
   anything that shapes output, compare it with `main`:

   ```sh
   tools/compare/compare.sh
   ```

   Every difference must be explained in the report. Either it's the intended change, or it's a
   bug to fix. `diff -r FOLDER/a FOLDER/b` shows one. Don't also run the fuzz corpus unless
   there's a reason: most of its inputs never reach the readers.
5. Run the checks. The first six are what CI runs; the last two catch what CI doesn't build:

   ```sh
   cargo fmt --check
   cargo clippy --locked --all-targets -- -D warnings
   RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
   cargo test --locked
   cargo clippy --locked --all-targets --no-default-features -- -D warnings
   cargo test --locked --no-default-features
   cargo clippy --locked -p officeconv-fuzz --all-targets -- -D warnings
   RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items
   ```

   If `Cargo.toml` or `Cargo.lock` changed, also run `cargo deny check` if it's installed.
6. Commit only the files you changed. The user keeps untracked files in the repo root that
   aren't part of the project, so never `git add -A` or `git add .` there.
   - Subject: a conventional commit, such as `fix: keep a picture grouped with a text box`.
     Write it in the imperative, saying what changes for the user where there's something to
     see.
   - Body: why the change was needed, what changed, and how it was checked. Use plain prose and
     wrap at 72 columns.
   - End the body with `Closes #N`, then any attribution lines Claude Code asks for.
7. Report, then ask whether to push and open the PR:
   - the branch and commit;
   - what changed;
   - any decisions you made;
   - how it was checked, with real numbers;
   - anything left open.

## 2. Push and open the PR ("push it and open the PR")

1. Push: `git push -u origin <branch>`.
2. Open the PR with `gh pr create`:
   - The title is the commit subject.
   - The body has these parts, in this order:
     - `Closes #N.`
     - Why the change was needed.
     - `## Change`: what changed.
     - Anything decided along the way, and any behavior change, stated plainly.
     - The acceptance criteria and how each is met.
     - `## Checks`: what was run.
     - Any footer Claude Code asks for.
3. Wait for CI: `gh pr checks <PR> --watch --interval 20`. A PR that changes
   `.github/workflows/release.yml` also runs that workflow's test build of every platform.
4. Report the PR link and the check results. If a check fails, read its log
   (`gh run view <run> --log-failed`), fix the cause, push again, and say what failed and why.

## 3. After the merge ("#N merged now")

1. Clean up:

   ```sh
   .claude/skills/issue-to-pr/scripts/after-merge.sh <PR>
   ```

   It refuses to touch anything until the PR is merged. If the user spoke a moment before the
   merge landed, say so and try again shortly. Then it:
   - syncs `main`;
   - deletes the PR's local branch, unless it has commits that aren't in the PR. Then it keeps
     the branch and fails: tell the user what's on it (`git log origin/main..<branch>`) and
     leave it;
   - prunes remote branches;
   - says whether each issue the PR closes is closed, and fails if one isn't. Then check that
     the PR body said `Closes #N`, and tell the user.
2. Report: `main` synced to which commit, the branch deleted, the issue closed. Then suggest
   what's next from `gh issue list --state open`. Recommend one item; don't start it unasked.
   - If `main` has user-visible changes since the last tag (`git log $(git describe --tags --abbrev=0)..main`),
     mention that a release could go out. The `cut-release` skill covers it.
