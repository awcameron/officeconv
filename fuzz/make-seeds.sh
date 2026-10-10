#!/bin/sh
# Writes the fuzz targets' seeds into corpus/<target>/: small documents, decks, a workbook and a
# CSV file that between them use every part and element the readers handle, so the fuzzer starts
# from inputs that reach all of them. The archive target gets every Office file.
#
# tests/cli/seeds.rs builds them, with the same helpers as the tests, and its
# every_seed_converts test checks that each one converts. A reader change that reads a new part
# or element adds a seed there.
#
# Run from fuzz/: ./make-seeds.sh
set -eu

cd "$(dirname "$0")"
corpus="$(pwd)/corpus"

# From the repository root, where cargo finds the officeconv package and its tests.
cd ..
OFFICECONV_SEEDS_DIR="$corpus" cargo test --locked --quiet --test cli seeds::write_fuzz_seeds \
    -- --ignored --exact > /dev/null

echo "wrote seeds to $corpus"
