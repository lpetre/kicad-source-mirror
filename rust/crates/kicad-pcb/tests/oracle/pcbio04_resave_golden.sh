#!/bin/bash
# PCBIO-04 differential oracle: re-save every hand-made board of tests/data/pcbio04/ with the
# reference `kicad-cli pcb upgrade --force` (each board copied alone into a fresh scratch
# directory: no project files) and store the result in tests/golden/pcbio04/ under the same
# name; boards the reference build rejects get a NAME.error file with its message instead.
# Replayed by the Rust test `io::parser::setup::tests::oracle_resave_setup`.
#
# Usage: tests/oracle/pcbio04_resave_golden.sh
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
cli=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export KICAD_ORACLE_HOME=$scratch/home
out=$here/golden/pcbio04
mkdir -p "$out"

for f in "$here"/data/pcbio04/*.kicad_pcb; do
    name=$(basename "$f")
    d=$scratch/b
    rm -rf "$d"; mkdir -p "$d"
    cp "$f" "$d/board.kicad_pcb"
    rm -f "$out/$name" "$out/$name.error"
    if "$cli" pcb upgrade --force "$d/board.kicad_pcb" > "$d/log" 2>&1 \
        && grep -q "Successfully saved" "$d/log"; then
        cp "$d/board.kicad_pcb" "$out/$name"
    else
        grep -v "api.v1.schema.json" "$d/log" > "$out/$name.error" || true
    fi
done
ls -l "$out"
