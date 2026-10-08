#!/bin/sh
# Checks an issue body against this repo's format before it's filed: a priority or severity on
# the first line, a problem statement, "## Reproduce" for a bug, then "## Change", then
# "## Acceptance criteria" with at least one checkbox, and no template comments left in. Prints
# every problem it finds, not just the first.
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
severity=no
if echo "$first" | grep -Eq 'Severity: \*\*(high|medium|low|low to medium)\*\*'; then
    severity=yes
elif ! echo "$first" | grep -Eq 'Priority: \*\*P[0-3]\*\*'; then
    problem "the first line must give 'Priority: **P0**' (to P3), or for a bug 'Severity: **high**' (medium, low); it is: $first"
fi

change=$(grep -n -m1 -E '^## Change( \(.+\))?$' "$body" | cut -d: -f1)
criteria=$(grep -n -m1 '^## Acceptance criteria$' "$body" | cut -d: -f1)
[ -n "$change" ] || problem "no '## Change' heading"
[ -n "$criteria" ] || problem "no '## Acceptance criteria' heading"
if [ -n "$change" ] && [ -n "$criteria" ]; then
    [ "$change" -lt "$criteria" ] || problem "'## Change' must come before '## Acceptance criteria'"
    before=$(head -n "$((change - 1))" "$body" | grep -c '[^[:space:]]' || true)
    [ "$before" -ge 2 ] ||
        problem "there's no problem statement between the priority line and '## Change'"
fi
if [ "$severity" = yes ] && ! grep -q '^## Reproduce$' "$body"; then
    problem "a bug (it has a severity) needs a '## Reproduce' section"
fi
if [ -n "$criteria" ]; then
    tail -n "+$criteria" "$body" | grep -Eq '^- \[[ xX]\] [^[:space:]]' ||
        problem "'## Acceptance criteria' has no '- [ ] ' items"
fi

grep -q '<!--' "$body" && problem "a template comment (<!-- ... -->) is still in the body"

if [ "$problems" -gt 0 ]; then
    echo "$problems problem(s) in $body"
    exit 1
fi
echo "ok: $body"
