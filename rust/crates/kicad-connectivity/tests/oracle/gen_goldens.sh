#!/bin/bash
# Regenerates the connectivity goldens (tests/golden/*.txt) with the reference pcb-dump
# (rust/docs/ORACLE.md, section "connectivity" of rust/docs/PCB_DUMP_FORMAT.md).
# The boards are copied to a scratch directory first (the tool writes side files).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD="$(cd "$HERE/../../../../.." && pwd)"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
GOLDEN="$HERE/../golden"
DATA="$KICAD/qa/data/pcbnew"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

PLAIN=( via_dangling issue18839 teardrop_close_via issue23123_minimal issue23339_zone_layer_rules
        soldermask_track_to_pad custom_pads padstacks_complex via_stacks tracks_arcs_vias fill_bad
        net_tie_drc connection_width_rules issue8003 test_copper_graphics intersectingzones
        issue14334 notched_zones issue9870 )
HASHED=( issue16182 )

for b in "${PLAIN[@]}" "${HASHED[@]}"; do
    cp "$DATA/$b.kicad_pcb" "$SCRATCH/"
    for ext in kicad_pro kicad_dru; do
        [ -f "$DATA/$b.$ext" ] && cp "$DATA/$b.$ext" "$SCRATCH/"
    done
done

cd "$SCRATCH"
for b in "${PLAIN[@]}"; do
    "$DUMP" board --sections connectivity "$b.kicad_pcb" 2>/dev/null > "$GOLDEN/$b.txt"
done
for b in "${HASHED[@]}"; do
    "$DUMP" board --hash --sections connectivity "$b.kicad_pcb" 2>/dev/null > "$GOLDEN/$b.hash.txt"
done
