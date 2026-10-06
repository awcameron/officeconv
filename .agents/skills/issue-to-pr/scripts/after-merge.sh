#!/bin/sh
# Cleans up after a PR merges: syncs main, deletes the PR's local branch and prunes remote
# branches, then reports whether the issues the PR closes are closed. Refuses to touch anything
# until the PR is merged, and keeps the branch if it has commits the PR doesn't.
#
# Usage, from anywhere in the repo: after-merge.sh PR_NUMBER
set -eu

die() {
    echo "error: $*" >&2
    exit 1
}

[ $# -eq 1 ] || die "usage: after-merge.sh PR_NUMBER"
pr=$1

state=$(gh pr view "$pr" --json state --jq .state)
[ "$state" = MERGED ] || die "PR #$pr is $state, not merged; if it was just merged, try again in a moment"
branch=$(gh pr view "$pr" --json headRefName --jq .headRefName)
head=$(gh pr view "$pr" --json headRefOid --jq .headRefOid)
[ "$branch" != main ] || die "PR #$pr was opened from main; not deleting it"

cd "$(git rev-parse --show-toplevel)"
# Untracked files are fine: the repo root can hold local files that aren't part of the project.
[ -z "$(git status --porcelain --untracked-files=no)" ] ||
    die "there are uncommitted changes; commit or stash them first"

git switch --quiet main
git pull --quiet --ff-only
echo "main is at $(git log --oneline -1)"

# A squash merge doesn't look merged to git, so the branch has to be deleted with -D, which
# deletes whatever is on it. Only do that when everything on it is in the PR: its tip is the
# PR's last commit, or an earlier one. Checked before pruning, while the PR's commits are still
# reachable from origin/<branch>.
kept=0
if tip=$(git rev-parse --verify --quiet "refs/heads/$branch"); then
    if [ "$tip" = "$head" ] ||
        { git cat-file -e "$head^{commit}" 2> /dev/null &&
            git merge-base --is-ancestor "$tip" "$head"; }; then
        git branch --quiet -D "$branch"
        echo "deleted $branch"
    else
        echo "kept $branch: it has commits that aren't in PR #$pr" >&2
        kept=1
    fi
else
    echo "$branch isn't a local branch; nothing to delete"
fi
git fetch --quiet --prune

issues=$(gh pr view "$pr" --json closingIssuesReferences --jq '.closingIssuesReferences[].number')
if [ -z "$issues" ]; then
    echo "PR #$pr doesn't close any issues"
fi
open=0
for issue in $issues; do
    issue_state=$(gh issue view "$issue" --json state --jq .state)
    echo "#$issue is $issue_state"
    [ "$issue_state" = CLOSED ] || open=1
done
[ "$open" -eq 0 ] || die "an issue the PR closes is still open"
[ "$kept" -eq 0 ] || die "$branch was kept; check what's on it before deleting it"
