#!/bin/sh
# Converts the same inputs with two builds of officeconv, the working tree and a git ref, and
# lists every input whose result differs: the Markdown, the saved images, the PDF of a document
# or deck, the messages on stderr, or the exit status. Use it to check that a change to a reader
# changes only what it's meant to. PDF shows what Markdown leaves out: headers and footers, page
# numbers, alignment and picture sizes. Its bytes are the same each time, so any difference is
# real.
#
# Usage, from anywhere in the repo:
#
#   tools/compare/compare.sh [BASE_REF]       # BASE_REF defaults to main
#
# The inputs are files from gen-nesting.py, which reach the XML readers every time. Settings:
#
#   COUNT=2000      generated files of each kind (.docx, .pptx, .xlsx)
#   SEED=1          generator seed; the same seed generates the same files
#   CORPUS=dir      also convert dir/docx, dir/pptx and dir/xlsx, such as fuzz/corpus. Most fuzz
#                   inputs are broken zips that never reach the readers, so it's off by default.
#   JOBS=n          conversions to run at once; defaults to the number of CPUs
#
# Everything goes under target/compare. For each input that differs, the two results are kept
# in a folder named in the output, so `diff -r FOLDER/a FOLDER/b` shows what changed.
#
# Exits with 0 when nothing differs, 1 when something does.
set -eu

# One input: `compare.sh --one KIND FILE`, run in parallel by xargs below.
if [ "${1:-}" = --one ]; then
    kind=$2 file=$3
    dir=$(mktemp -d "$WORK/runs/XXXXXX")

    # convert BIN OUT NAME ARGS...: runs BIN with ARGS, keeping its stdout, stderr and exit
    # status as OUT/NAME.stdout, OUT/NAME.stderr and OUT/NAME.status.
    convert() {
        bin=$1 out=$2 name=$3
        shift 3
        status=0
        "$bin" "$@" > "$out/$name.stdout" 2> "$out/$name.stderr.raw" || status=$?
        echo "$status" > "$out/$name.status"
        # stderr names the output folder, which differs between the two sides.
        sed "s|$out|OUT|g" "$out/$name.stderr.raw" > "$out/$name.stderr"
        rm "$out/$name.stderr.raw"
    }

    for side in a b; do
        bin=$WORK/officeconv-base
        [ "$side" = b ] && bin=$WORK/officeconv-new
        out=$dir/$side
        mkdir "$out"
        if [ "$kind" = xlsx ]; then
            convert "$bin" "$out" md "$file" --from xlsx --to md --all-sheets -o "$out/md" \
                --images "$out/md/img"
        else
            convert "$bin" "$out" md "$file" --from "$kind" --to md -o "$out/out.md" \
                --images "$out/img"
            convert "$bin" "$out" pdf "$file" --from "$kind" --to pdf -o "$out/out.pdf"
        fi
    done
    if diff -rq "$dir/a" "$dir/b" > /dev/null; then
        rm -rf "$dir"
    else
        echo "$kind $file -> $dir"
    fi
    exit 0
fi

root=$(git rev-parse --show-toplevel)
base_ref=${1:-main}
count=${COUNT:-2000}
seed=${SEED:-1}
jobs=${JOBS:-$(getconf _NPROCESSORS_ONLN 2> /dev/null || sysctl -n hw.ncpu 2> /dev/null || echo 4)}
WORK=$root/target/compare
export WORK
base_src=$WORK/base-src

git -C "$root" rev-parse --verify --quiet "$base_ref^{commit}" > /dev/null ||
    { echo "error: $base_ref isn't a commit" >&2; exit 2; }

rm -rf "$WORK/runs" "$WORK/generated"
mkdir -p "$WORK/runs"
cleanup() { git -C "$root" worktree remove --force "$base_src" 2> /dev/null || true; }
trap cleanup EXIT
cleanup

# The release profile with LTO off: a build takes about a minute instead of several, and the
# output is the same. These variables work for any ref, even one older than this script.
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
# Each build gets its own target directory. In a shared one, cargo takes the second build for
# up to date, because what it recorded about the first one points at the base checkout's files,
# which haven't changed, and both binaries come out the same.
target=$WORK/target

echo "building $base_ref"
git -C "$root" worktree add --quiet --detach "$base_src" "$base_ref"
cargo build --quiet --release --locked --bin officeconv \
    --manifest-path "$base_src/Cargo.toml" --target-dir "$target/base"
cp "$target/base/release/officeconv" "$WORK/officeconv-base"

echo "building the working tree"
cargo build --quiet --release --locked --bin officeconv \
    --manifest-path "$root/Cargo.toml" --target-dir "$target/new"
cp "$target/new/release/officeconv" "$WORK/officeconv-new"

echo "generating $count files of each kind (seed $seed)"
python3 "$root/tools/compare/gen-nesting.py" "$WORK/generated" "$count" "$seed"
inputs=$WORK/generated
[ -n "${CORPUS:-}" ] && inputs="$inputs $CORPUS"

echo "converting with both builds, $jobs at a time"
differ=$WORK/differ.txt
: > "$differ"
checked=0
for dir in $inputs; do
    for kind in docx pptx xlsx; do
        [ -d "$dir/$kind" ] || continue
        checked=$((checked + $(find "$dir/$kind" -type f | wc -l)))
        find "$dir/$kind" -type f -print0 |
            xargs -0 -n 1 -P "$jobs" "$0" --one "$kind" >> "$differ"
    done
done

n=$(wc -l < "$differ" | tr -d ' ')
echo "checked $checked inputs: $n differ"
if [ "$n" -gt 0 ]; then
    head -n 20 "$differ"
    [ "$n" -gt 20 ] && echo "... the full list is in $differ"
    exit 1
fi
