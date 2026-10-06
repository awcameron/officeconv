#!/bin/sh
# Checks what merging a release PR published: waits for the Release run on the merge commit,
# then checks that every job passed, that vX.Y.Z points at the merge commit, and that the
# release has an archive and a .sha256 for each platform. Refuses to start until the PR is
# merged.
#
# Usage, from anywhere in the repo: verify-release.sh PR_NUMBER
set -eu

die() {
    echo "error: $*" >&2
    exit 1
}

[ $# -eq 1 ] || die "usage: verify-release.sh PR_NUMBER"
pr=$1

state=$(gh pr view "$pr" --json state --jq .state)
[ "$state" = MERGED ] || die "PR #$pr is $state, not merged; if it was just merged, try again in a moment"
sha=$(gh pr view "$pr" --json mergeCommit --jq .mergeCommit.oid)

cd "$(git rev-parse --show-toplevel)"
git fetch --quiet --tags origin
version=$(git show "$sha:Cargo.toml" |
    awk '/^\[/ { in_package = ($0 == "[package]") }
         in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }')
tag=v$version
short=$(git rev-parse --short "$sha")
echo "PR #$pr merged as $short, version $version"

# The run can take a few seconds to appear after the merge.
run=
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    run=$(gh run list --workflow Release --commit "$sha" --json databaseId --jq '.[0].databaseId // empty')
    [ -n "$run" ] && break
    sleep 10
done
[ -n "$run" ] || die "no Release run for $short after 2 minutes; did the PR change Cargo.toml?"

echo "waiting for Release run $run"
if ! gh run watch "$run" --interval 30 --exit-status > /dev/null 2>&1; then
    gh run view "$run" --json jobs --jq '.jobs[] | "  \(.name): \(.conclusion)"' >&2
    die "Release run $run failed; see the cut-release skill's \"When the run fails\""
fi
gh run view "$run" --json jobs --jq '.jobs[] | "  \(.name): \(.conclusion)"'

git fetch --quiet --tags origin
tagged=$(git rev-parse --verify --quiet "$tag^{commit}") || die "$tag doesn't exist"
[ "$tagged" = "$sha" ] || die "$tag points at $(git rev-parse --short "$tagged"), not $short"
echo "$tag points at $short"

assets=$(gh release view "$tag" --json assets --jq '.assets[].name')
missing=
for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin \
    x86_64-apple-darwin x86_64-pc-windows-msvc; do
    archive=officeconv-$tag-$target.tar.gz
    [ "$target" = x86_64-pc-windows-msvc ] && archive=officeconv-$tag-$target.zip
    for name in "$archive" "officeconv-$tag-$target.sha256"; do
        echo "$assets" | grep -qxF "$name" || missing="$missing $name"
    done
done
[ -z "$missing" ] || die "the release is missing:$missing"
count=$(echo "$assets" | grep -c .)
[ "$count" -eq 10 ] || die "expected 10 files in the release, found $count"

echo "all 10 files published: $(gh release view "$tag" --json url --jq .url)"
