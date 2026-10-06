#!/bin/sh
# Starts a release: creates chore/release-X.Y.Z from origin/main, sets the version in Cargo.toml
# and Cargo.lock, checks that nothing else changed, and runs the tests. It commits nothing: the
# commit message and PR body say what's in the release, so they're written by hand.
#
# Usage, from anywhere in the repo: bump-version.sh X.Y.Z
set -eu

die() {
    echo "error: $*" >&2
    exit 1
}

[ $# -eq 1 ] || die "usage: bump-version.sh X.Y.Z"
new=$1
echo "$new" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || die "$new isn't a version like 1.2.3"

cd "$(git rev-parse --show-toplevel)"
# Untracked files are fine: the repo root can hold local files that aren't part of the project.
[ -z "$(git status --porcelain --untracked-files=no)" ] ||
    die "there are uncommitted changes; commit or stash them first"

# The `version` line in a Cargo.toml's [package] section, not a dependency's or the workspace's.
package_version() {
    awk '/^\[/ { in_package = ($0 == "[package]") }
         in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }'
}

# Succeeds if origin has the ref, fails if it doesn't, and stops the script on any other error,
# such as no network, rather than taking it to mean the ref isn't there.
origin_has() {
    status=0
    git ls-remote --exit-code origin "$1" > /dev/null || status=$?
    case $status in
        0) return 0 ;;
        2) return 1 ;;
        *) die "couldn't ask origin whether $1 exists (git ls-remote exited with $status)" ;;
    esac
}

# Every check runs against origin/main before anything changes, so a refusal leaves the
# checkout where it was.
git fetch --quiet origin main
current=$(git show origin/main:Cargo.toml | package_version)
[ -n "$current" ] || die "couldn't find the version in Cargo.toml"

newest=$(printf '%s\n%s\n' "$current" "$new" | sort -V | tail -n 1)
[ "$new" != "$current" ] && [ "$newest" = "$new" ] ||
    die "$new isn't newer than the current version, $current"

branch=chore/release-$new
if origin_has "refs/tags/v$new"; then
    die "v$new is already tagged"
fi
if git rev-parse --verify --quiet "refs/heads/$branch" > /dev/null ||
    origin_has "refs/heads/$branch"; then
    die "$branch already exists"
fi

# From here on a failure leaves the new branch checked out with the bump uncommitted, so say
# how to undo it.
on_exit() {
    status=$?
    rm -f Cargo.toml.new
    [ "$status" -eq 0 ] && return
    echo "the bump is left uncommitted on $branch; to undo it:" >&2
    echo "  git checkout -- Cargo.toml Cargo.lock && git switch - && git branch -D $branch" >&2
}

# From origin/main, not local main: local main may have commits that aren't pushed, and they
# don't belong in a release.
git switch --quiet --no-track -c "$branch" origin/main
trap on_exit EXIT

awk -v new="$new" '/^\[/ { in_package = ($0 == "[package]") }
     in_package && !done && /^version = "/ { $0 = "version = \"" new "\""; done = 1 }
     { print }' Cargo.toml > Cargo.toml.new
mv Cargo.toml.new Cargo.toml
cargo update --quiet --workspace --offline

# Exactly one line each in Cargo.toml and Cargo.lock should change: the version.
changed=$(git diff --numstat | awk '{ print $3 ":" $1 "/" $2 }' | sort | tr '\n' ' ')
[ "$changed" = "Cargo.lock:1/1 Cargo.toml:1/1 " ] ||
    die "expected one changed line in each of Cargo.toml and Cargo.lock, got: $changed"

echo "running the tests"
# cargo test prints which test failed, and why, on stdout: show it if they fail.
if ! output=$(cargo test --quiet --locked 2>&1); then
    printf '%s\n' "$output" >&2
    die "the tests failed"
fi

echo "bumped $current -> $new on $branch; nothing is committed yet"
