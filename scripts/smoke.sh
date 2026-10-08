#!/bin/sh
# Runs a built officeconv binary once for each kind of input, to check that a release binary
# starts and works on its platform: it reports the version in Cargo.toml, reads CSV, XLSX, DOCX
# and PPTX, writes PDF (so the pdf feature and the built-in fonts are there), and exits with the
# usage code on a usage mistake. The Release workflow runs it on every target before
# publishing anything. Stops at the first check that fails.
#
# Usage, from anywhere in the repo: scripts/smoke.sh BINARY
#
# For example, after `cargo build --release`: scripts/smoke.sh target/release/officeconv
set -eu

[ $# -eq 1 ] || {
    echo "usage: smoke.sh BINARY" >&2
    exit 2
}
bin=$1
[ -x "$bin" ] || {
    echo "error: $bin isn't an executable file" >&2
    exit 2
}

here=$(cd "$(dirname "$0")" && pwd)
version=$(awk '/^\[/ { in_package = ($0 == "[package]") }
    in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' "$here/../Cargo.toml")

# Windows runners may have only `python`.
python=$(command -v python3 || command -v python) || {
    echo "error: smoke.sh needs Python 3 to write its inputs" >&2
    exit 2
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$python" "$here/smoke-inputs.py" "$work"

fail() {
    echo "error: $*" >&2
    exit 1
}

# check NAME TEXT COMMAND...: runs COMMAND and fails unless it succeeds and prints TEXT.
check() {
    name=$1 text=$2
    shift 2
    echo "==> $name"
    out=$("$@") || fail "$name: exited with $?"
    case $out in
        *"$text"*) ;;
        *) fail "$name: expected '$text' in the output, got: $out" ;;
    esac
}

check version "$version" "$bin" --version
check help "Usage" "$bin" --help
check "csv to json" '"North"' "$bin" "$work/sales.csv" --to json
check "xlsx to csv" "North,12" "$bin" "$work/sales.xlsx" --to csv
check "docx to markdown" "Smoke **test**" "$bin" "$work/notes.docx" --to markdown

echo "==> pptx to pdf"
"$bin" "$work/talk.pptx" --to pdf -o "$work/talk.pdf" || fail "pptx to pdf: exited with $?"
[ "$(head -c 5 "$work/talk.pdf")" = "%PDF-" ] || fail "pptx to pdf: talk.pdf isn't a PDF"

echo "==> usage error"
status=0
"$bin" "$work/sales.csv" --to csv --typed 2> /dev/null || status=$?
[ "$status" -eq 64 ] || fail "usage error: expected exit code 64, got $status"

echo "smoke test passed: $bin"
