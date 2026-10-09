#!/bin/sh
# Runs the checks a change has to pass before it's committed, in order, and stops at the first
# one that fails. The first six are the steps of CI's check job (.github/workflows/ci.yml), so
# change both together. The last two cover what CI doesn't build: the fuzz crate and the
# private-item docs. If cargo-deny is installed, it also runs CI's deny job.
#
# Usage, from anywhere in the repo: scripts/check.sh
set -eu

cd "$(git rev-parse --show-toplevel)"

step() {
    name=$1
    shift
    echo "==> $name: $*"
    "$@" || {
        echo "error: $name failed: $*" >&2
        exit 1
    }
}

step format cargo fmt --check
step lint cargo clippy --locked --all-targets -- -D warnings
step docs env RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
step test cargo test --locked
step "lint without PDF" cargo clippy --locked --all-targets --no-default-features -- -D warnings
step "test without PDF" cargo test --locked --no-default-features
step "lint fuzz" cargo clippy --locked -p officeconv-fuzz --all-targets -- -D warnings
step "private docs" env RUSTDOCFLAGS="-D warnings" \
    cargo doc --locked --no-deps --document-private-items

if cargo deny --version > /dev/null 2>&1; then
    step dependencies cargo deny --all-features check
else
    echo "==> dependencies: skipped, cargo-deny isn't installed (CI runs it)"
fi

echo "all checks passed"
