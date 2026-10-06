---
name: release
description: Cuts an officeconv release through a version-bump PR, then checks what the Release workflow published. Use when the user says "cut the X.Y.Z release", asks what's next for a release, or says a release PR merged.
---

# Cutting a release

Since #54, merging a PR that changes `version` in `Cargo.toml` is the release. The Release
workflow (`.github/workflows/release.yml`) runs on that push to `main`. If `vX.Y.Z` isn't
tagged yet, it:

1. runs the tests;
2. creates the GitHub Release and its tag on the merge commit;
3. uploads five archives, each with a `.sha256`.

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
git switch main && git pull --quiet
git switch -c chore/release-X.Y.Z
# Edit `version = "..."` under [package] in Cargo.toml, then:
cargo update -w --offline
git diff --stat            # exactly Cargo.toml and Cargo.lock, one line each
cargo test --locked
```

- **Commit subject:** `chore: release X.Y.Z`.
- **Commit body:** the user-visible changes with their PR numbers. Call out breaking ones.
- **PR:** push, then open it with the same title.
- **PR body:**
  - the version bump;
  - the changes since the last tag, grouped as breaking changes, fixes, then other changes, with
    PR and issue numbers;
  - a line saying that merging it publishes `vX.Y.Z`, with no tag to push.
- Wait for CI with `gh pr checks <PR> --watch --interval 20`, then report the link.

## 3. After the merge

1. Confirm the merge **on its own**, with `gh pr view <PR> --json state --jq .state`. Go on only
   if it prints `MERGED`, and never chain it with the cleanup: printing `OPEN` doesn't stop a
   chain.
2. Sync `main`, then check that it has the new version:

   ```sh
   git switch main && git pull --quiet
   git branch -D chore/release-X.Y.Z
   git fetch --quiet --prune
   grep -m1 '^version' Cargo.toml
   ```

3. Find the Release run for the merge commit and watch it:

   ```sh
   gh run list --workflow Release --branch main --limit 1 --json databaseId,headSha,status
   gh run watch <run> --interval 30 --exit-status
   gh run view <run> --json jobs --jq '.jobs[] | "\(.name): \(.conclusion)"'
   ```

4. Check what was published:
   - The tag points at the merge commit: `git fetch --tags --quiet && git rev-parse --short 'vX.Y.Z^{commit}'`.
   - The release has ten files: an archive and a `.sha256` for each of
     - `x86_64-unknown-linux-gnu`;
     - `aarch64-unknown-linux-gnu`;
     - `aarch64-apple-darwin`;
     - `x86_64-apple-darwin`;
     - `x86_64-pc-windows-msvc` (a `.zip`; the rest are `.tar.gz`).

     Check with `gh release view vX.Y.Z --json url,assets --jq '.url, (.assets[] | .name)'`.
5. Report the release link, the tag commit and the files.

## When the run fails

- **`check` fails (tests):** nothing was tagged.
  - If it's a flaky failure, re-run the workflow.
  - If it's a real failure, fix it in a normal PR. That PR doesn't change `Cargo.toml`, so it
    doesn't start a release. Then release the next patch version, which skips the version that
    failed. Tell the user before doing this.
- **`create-release` or an `upload` job fails after the tag exists:** use "Re-run failed jobs"
  (`gh run rerun <run> --failed`). A full re-run finds the tag already there and publishes
  nothing.
- **A run on `main` says the tag already exists:** that's expected when `Cargo.toml` changed but
  the version didn't, such as after a dependency update. Nothing needs doing.
