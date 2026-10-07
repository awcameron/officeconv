#!/bin/sh
# Checks what merging a release PR published: waits for the Release run on the merge commit,
# then checks that every job passed, that vX.Y.Z points at the merge commit, that the release
# has an archive and a .sha256 for each platform, that each archive is attested by the release
# workflow for the merge commit, and that the README's releases/latest/download links reach it.
# Refuses to start until the PR is merged.
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
targets="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin
    x86_64-apple-darwin x86_64-pc-windows-msvc"
missing=
for target in $targets; do
    archive=officeconv-$target.tar.gz
    [ "$target" = x86_64-pc-windows-msvc ] && archive=officeconv-$target.zip
    for name in "$archive" "officeconv-$target.sha256"; do
        echo "$assets" | grep -qxF "$name" || missing="$missing $name"
    done
done
[ -z "$missing" ] || die "the release is missing:$missing"
count=$(echo "$assets" | grep -c .)
[ "$count" -eq 10 ] || die "expected 10 files in the release, found $count"

downloads=$(mktemp -d)
trap 'rm -rf "$downloads"' EXIT
gh release download "$tag" --dir "$downloads"
repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)

# Each archive must have an attestation signed by this repo's release workflow, for the merge
# commit, so `gh attestation verify` in the README works for it.
for target in $targets; do
    archive=officeconv-$target.tar.gz
    [ "$target" = x86_64-pc-windows-msvc ] && archive=officeconv-$target.zip
    gh attestation verify "$downloads/$archive" --repo "$repo" \
        --signer-workflow "$repo/.github/workflows/release.yml" --source-digest "$sha" \
        > /dev/null 2>&1 ||
        die "$archive has no attestation from release.yml for $short; see gh attestation verify"
done
echo "every archive is attested by release.yml at $short"

# The names have no version, so the README's releases/latest/download links must now reach
# this release. GitHub redirects such a link to the release's own link, then to its storage.
for target in $targets; do
    url=https://github.com/$repo/releases/latest/download/officeconv-$target.sha256
    reached=$(curl -sSI "$url" | tr -d '\r' | sed -n 's/^[Ll]ocation: //p')
    case $reached in
        */download/$tag/*) ;;
        *) die "$url redirects to '$reached', not to $tag" ;;
    esac
    latest=$(curl -sSfL "$url") || die "$url doesn't download"
    [ "$latest" = "$(cat "$downloads/officeconv-$target.sha256")" ] ||
        die "$url doesn't hold $tag's checksum"
done
echo "the latest links reach $tag"

echo "all 10 files published: $(gh release view "$tag" --json url --jq .url)"
