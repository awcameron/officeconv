#!/bin/sh
# Checks an issue body against this repo's format before it's filed: a priority line first,
# then "## Change", then "## Acceptance criteria" with at least one checkbox, and no template
# comments left in. Prints every problem it finds, not just the first.
#
# Usage, from anywhere in the repo: check-issue.sh BODY_FILE
set -eu

[ $# -eq 1 ] || { echo "usage: check-issue.sh BODY_FILE" >&2; exit 2; }
body=$1
[ -s "$body" ] || { echo "error: $body is missing or empty" >&2; exit 2; }

problems=0
problem() {
    echo "problem: $*"
    problems=$((problems + 1))
}

first=$(grep -m1 -v '^[[:space:]]*$' "$body")
echo "$first" | grep -Eq '^(Split from #[0-9]+\. )?Priority: \*\*P[0-3]\*\*' ||
    problem "the first line must start with 'Priority: **P0**' (to P3), or with 'Split from #N. Priority: ...'; it is: $first"

change=$(grep -n -m1 '^## Change$' "$body" | cut -d: -f1)
criteria=$(grep -n -m1 '^## Acceptance criteria$' "$body" | cut -d: -f1)
[ -n "$change" ] || problem "no '## Change' heading"
[ -n "$criteria" ] || problem "no '## Acceptance criteria' heading"
if [ -n "$change" ] && [ -n "$criteria" ]; then
    [ "$change" -lt "$criteria" ] || problem "'## Change' must come before '## Acceptance criteria'"
    before=$(head -n "$((change - 1))" "$body" | grep -c '[^[:space:]]' || true)
    [ "$before" -ge 2 ] ||
        problem "there's no problem statement between the priority line and '## Change'"
fi
if [ -n "$criteria" ]; then
    tail -n "+$criteria" "$body" | grep -Eq '^- \[ \] [^[:space:]]' ||
        problem "'## Acceptance criteria' has no unchecked '- [ ] ' items"
fi

grep -q '<!--' "$body" && problem "a template comment (<!-- ... -->) is still in the body"

if [ "$problems" -gt 0 ]; then
    echo "$problems problem(s) in $body"
    exit 1
fi
echo "ok: $body"
