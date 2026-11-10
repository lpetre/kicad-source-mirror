#!/bin/bash
# PCBIO-02 differential oracle: re-save the hand-made boards in tests/data/pcbio02/ with the
# reference `kicad-cli pcb upgrade --force` (it rewrites the board in place, so each input is
# copied alone into a fresh scratch directory: no .kicad_pro, no .kicad_prl) and store the
# result as tests/golden/pcbio02/<name>.kicad_pcb (O1). Each result is re-saved once more (O2);
# where O2 differs from O1 (legacy inputs can legitimately change once more, e.g. the layer
# names of a version 4 file) it is stored as <name>_o2.kicad_pcb. The Rust test checks
# parse + format of the input against O1 and of O1 against O2 (or O1).
# Replayed by the Rust test `io::writer::tests::oracle_resave`.
#
# Usage: tests/oracle/pcbio02_resave_golden.sh   (run from rust/crates/kicad-pcb)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
cli=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export KICAD_ORACLE_HOME=$scratch/home
outdir=$here/golden/pcbio02
mkdir -p "$outdir"

resave() {   # resave <in> <out>
    local d=$scratch/$RANDOM$RANDOM
    mkdir -p "$d"
    cp "$1" "$d/board.kicad_pcb"
    "$cli" pcb upgrade --force "$d/board.kicad_pcb" > /dev/null 2>&1
    cp "$d/board.kicad_pcb" "$2"
    rm -rf "$d"
}

rm -f "$outdir"/*.kicad_pcb
for f in "$here"/data/pcbio02/*.kicad_pcb; do
    name=$(basename "$f")
    resave "$f" "$outdir/$name"
    resave "$outdir/$name" "$scratch/o2.kicad_pcb"
    o2=$outdir/${name%.kicad_pcb}_o2.kicad_pcb
    rm -f "$o2"
    cmp -s "$outdir/$name" "$scratch/o2.kicad_pcb" || cp "$scratch/o2.kicad_pcb" "$o2"
    echo "$name"
done
