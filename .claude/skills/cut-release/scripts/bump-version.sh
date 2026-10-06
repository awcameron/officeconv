#!/bin/sh
# Starts a release: creates chore/release-X.Y.Z from an up-to-date main, sets the version in
# Cargo.toml and Cargo.lock, checks that nothing else changed, and runs the tests. It commits
# nothing: the commit message and PR body say what's in the release, so they're written by hand.
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

# The `version` line in Cargo.toml's [package] section, not a dependency's or the workspace's.
package_version() {
    awk '/^\[/ { in_package = ($0 == "[package]") }
         in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' Cargo.toml
}

git switch --quiet main
git pull --quiet --ff-only
current=$(package_version)
[ -n "$current" ] || die "couldn't find the version in Cargo.toml"

newest=$(printf '%s\n%s\n' "$current" "$new" | sort -V | tail -n 1)
[ "$new" != "$current" ] && [ "$newest" = "$new" ] ||
    die "$new isn't newer than the current version, $current"

branch=chore/release-$new
if git ls-remote --exit-code --tags origin "refs/tags/v$new" > /dev/null; then
    die "v$new is already tagged"
fi
if git rev-parse --verify --quiet "refs/heads/$branch" > /dev/null ||
    git ls-remote --exit-code --heads origin "$branch" > /dev/null; then
    die "$branch already exists"
fi

git switch --quiet -c "$branch"
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
cargo test --quiet --locked > /dev/null

echo "bumped $current -> $new on $branch; nothing is committed yet"
