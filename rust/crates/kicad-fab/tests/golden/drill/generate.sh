#!/bin/bash
# Regenerates the drill goldens of kicad-fab with the reference kicad-cli (docs/ORACLE.md).
#
# For every board of BOARDS and every option set of SETS the board (and its .kicad_pro, if
# any) is copied to a scratch directory, `kicad-cli pcb export drill <set> -o <out> <board>`
# is run there and the produced files are stored as
#   tests/golden/drill/<board stem>/<set name>/<file>
# together with `<set name>/files.txt` (the "Created file" lines of stdout, in order, paths
# reduced to the file name). Header lines carrying dates / versions are kept verbatim; the
# comparison (tests/drill_golden.rs) normalises them.
#
# Option sets: the four of qa/tests/cli/test_pcb.py test_pcb_export_drill, plus
# --excellon-separate-th.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../../.." && pwd)"
CLI="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
SCRATCH="${SCRATCH:-$KICAD_SRC/rust/target/fab-drill-golden}"
export KICAD_ORACLE_HOME="$SCRATCH/home"

BOARDS=(
    qa/data/cli/basic_test/basic_test.kicad_pcb
    qa/data/pcbnew/issue25021/backdrill.kicad_pcb
    qa/data/pcbnew/drc_via_stack/FillFromBoardHonoursTheBoardDefault.kicad_pcb
    qa/data/pcbnew/drc_via_stack/ALoneMicroviaMayBeUnfilled.kicad_pcb
    qa/data/pcbnew/odbpp/npth_pressfit.kicad_pcb
    qa/data/pcbnew/odbpp/npth_pressfit_slot.kicad_pcb
    qa/data/pcbnew/odbpp/via_in_pad.kicad_pcb
    qa/data/pcbnew/padstacks.kicad_pcb
    qa/data/pcbnew/padstacks_complex.kicad_pcb
    qa/data/pcbnew/custom_pads.kicad_pcb
)

SETS=(
    "excellon_default|--format excellon"
    "excellon_inches|--format excellon -u in"
    "excellon_mirror|--format excellon --excellon-mirror-y"
    "excellon_separate_th|--format excellon --excellon-separate-th"
    "gerber_default|--format gerber"
)

rm -rf "$SCRATCH"
mkdir -p "$SCRATCH/home"

for board in "${BOARDS[@]}"; do
    src="$KICAD_SRC/$board"
    stem="$(basename "$board" .kicad_pcb)"
    for set in "${SETS[@]}"; do
        name="${set%%|*}"
        args="${set#*|}"
        work="$SCRATCH/$stem/$name"
        mkdir -p "$work/in" "$work/out"
        cp "$src" "$work/in/"
        pro="${src%.kicad_pcb}.kicad_pro"
        [ -f "$pro" ] && cp "$pro" "$work/in/"
        # shellcheck disable=SC2086
        "$CLI" pcb export drill $args -o "$work/out/" "$work/in/$stem.kicad_pcb" \
            > "$work/stdout.txt" 2> "$work/stderr.txt"
        dest="$HERE/$stem/$name"
        rm -rf "$dest"
        mkdir -p "$dest"
        grep -o "Created file '[^']*'" "$work/stdout.txt" | sed -e "s|Created file '.*/|Created file '|" \
            > "$dest/files.txt" || true
        cp "$work/out/"* "$dest/" 2>/dev/null || true
    done
done
echo "goldens written to $HERE"
