---
name: cut-release
description: 'Cuts an officeconv release through a version-bump PR, then checks what the Release workflow published. Use when the user says "cut the X.Y.Z release", asks what''s next for a release, or says a release PR merged.'
compatibility: Requires git, an authenticated GitHub CLI (gh) and a Rust toolchain.
allowed-tools: 'Read Bash(git status) Bash(git log *) Bash(git describe *) Bash(git diff *) Bash(git add Cargo.toml Cargo.lock) Bash(git commit *) Bash(git push -u origin chore/release-*) Bash(gh pr create *) Bash(gh pr checks *) Bash(.agents/skills/cut-release/scripts/bump-version.sh *) Bash(.agents/skills/cut-release/scripts/verify-release.sh *) Bash(.agents/skills/issue-to-pr/scripts/after-merge.sh *)'
---

# Cutting a release

Since #54, merging a PR that changes `version` in `Cargo.toml` is the release. The Release
workflow (`.github/workflows/release.yml`) runs on that push to `main`. If `vX.Y.Z` isn't
tagged yet, it:

1. runs the tests;
2. builds eight archives, each with a `.sha256`. Their names have no version
   (`officeconv-<target>.tar.gz`), so the README's `releases/latest/download/` links get the
   newest release;
3. smoke-tests each archive's binary on a runner of its own platform (`scripts/smoke.sh`);
4. once all eight pass, creates the GitHub Release and its tag on the merge commit, with every
   archive;
5. attests each archive, so `gh attestation verify` can show it was built here.

**Never push, move or delete a `v*` tag by hand.** A pushed tag starts nothing, and a ruleset
blocks moving or deleting one, so a wrong tag can't be fixed without the repo owner.

## 1. Choose the version

List what's changed since the last release:

```sh
last=$(git describe --tags --abbrev=0 origin/main)
git log --oneline "$last"..origin/main
```

officeconv is below 1.0, so:

- **Minor (0.X.0):** anything that breaks a user or a script, such as:
  - removed or renamed options;
  - changed exit codes;
  - output formatted differently on purpose;
  - a change to the library's public surface.
- **Patch (0.x.Y):** fixes and additions that don't break anything.
- **Nothing to release:** the log holds only tests, CI, docs or internal changes. Say so, and
  don't release unless the user still wants to.

If the user named a version, use it, but point out if the log suggests otherwise.

## 2. Open the release PR

```sh
.agents/skills/cut-release/scripts/bump-version.sh X.Y.Z
```

The script:
- refuses if there are uncommitted changes (untracked files are fine), if X.Y.Z isn't newer
  than the current version, or if `vX.Y.Z` or the branch already exists;
- creates `chore/release-X.Y.Z` from `origin/main`, so unpushed local commits stay out;
- sets the version in `Cargo.toml` and `Cargo.lock`, and fails unless exactly one line in each
  changed;
- runs the tests, printing their output if they fail.

It commits nothing. If it fails after creating the branch, it leaves the bump uncommitted there
and prints the command that undoes it. Report the failure; don't undo it or retry unasked.
Then:

- **Commit** `Cargo.toml` and `Cargo.lock`:
  - subject: `chore(release): release X.Y.Z`;
  - body: the user-visible changes with their PR numbers. Call out breaking ones.
- **PR:** push, then open it with the same title and `--label release`, which leaves it out of
  the release notes GitHub writes (see `.github/release.yml`).
- **PR body:**
  - the version bump;
  - the changes since the last tag, grouped as breaking changes, fixes, then other changes, with
    PR and issue numbers;
  - a line saying that merging it publishes `vX.Y.Z`, with no tag to push.
- Wait for CI with `gh pr checks <PR> --watch --interval 20`, then report the link.

## 3. After the merge

1. Check what was published:

   ```sh
   .agents/skills/cut-release/scripts/verify-release.sh <PR>
   ```

   It refuses until the PR is merged: if the user spoke a moment before the merge landed, say
   so and try again shortly. Then it:
   - waits for the Release run on the merge commit and checks that every job passed;
   - checks that `vX.Y.Z` points at the merge commit;
   - checks that the release has an archive and a `.sha256` for each of the eight platforms, and
     nothing else;
   - checks that each archive has an attestation from `release.yml` for the merge commit;
   - checks that each `releases/latest/download/` link reaches this release.

   It exits with an error that names what failed. For a failed run, see below.
2. Clean up with the `issue-to-pr` skill's script, which syncs `main` and deletes the branch:

   ```sh
   .agents/skills/issue-to-pr/scripts/after-merge.sh <PR>
   ```

3. Report the release link, the tag commit and the files.

## When the run fails

- **`check` fails (tests):** nothing was tagged.
  - If it's a flaky failure, re-run the workflow.
  - If it's a real failure, fix it in a normal PR. That PR doesn't change `Cargo.toml`, so it
    doesn't start a release. Then release the next patch version, which skips the version that
    failed. Tell the user before doing this.
- **A `build` or `smoke-test` job fails:** nothing was tagged or published.
  - If it's a flaky failure, such as a runner problem, use "Re-run failed jobs"
    (`gh run rerun <run> --failed`). It re-runs the jobs after it too, so `publish` follows.
  - If the binary is really broken, fix it in a normal PR, then release the next patch
    version, as for a failed `check`. Tell the user before doing this.
- **`publish` fails after the release exists,** such as in the attest step: use "Re-run failed
  jobs". The re-run replaces the release's files and attests them again. A full re-run finds the
  tag already there and publishes nothing.
- **A run on `main` says the tag already exists:** that's expected when `Cargo.toml` changed but
  the version didn't, such as after a dependency update. Nothing needs doing.
