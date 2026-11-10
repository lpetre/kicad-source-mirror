#!/bin/bash
# CN-04: regenerates the CONNECTIVITY_DATA goldens with the reference pcb-dump
# (rust/docs/ORACLE.md, rust/docs/PCB_DUMP_FORMAT.md sections 4 and 8).
#
# - tests/golden/cn04/*.kicad_pcb: synthetic boards (gen_cn04.py); their goldens hold the
#   `cn_dangling` and `rn_net` lines of the `connectivity` section and the `flash.<layer>`
#   lines of the `track` / `pad` sections (PCB_VIA::FlashLayer / PAD::FlashLayer, which ask
#   CONNECTIVITY_DATA::IsConnectedOnLayer).
# - tests/golden/cn04_flash/<board>.txt: the `flash.<layer>` lines of the connectivity golden
#   boards of gen_goldens.sh (their `connectivity` lines are in tests/golden/<board>.txt).
#
# Inputs are copied to a scratch directory first (the tool writes side files). Uses the
# existing pcb-dump binary only (no C++ build).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD="$(cd "$HERE/../../../../.." && pwd)"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
GOLDEN="$HERE/../golden"
DATA="$KICAD/qa/data/pcbnew"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

FILTER='^(cn_dangling|rn_net)\||^(track|pad)\|[^|]*\|flash\.'
FLASH_FILTER='^(track|pad)\|[^|]*\|flash\.'

mkdir -p "$GOLDEN/cn04" "$GOLDEN/cn04_flash"
python3 -I "$HERE/gen_cn04.py" "$GOLDEN/cn04"

for f in "$GOLDEN"/cn04/*.kicad_pcb; do
    b="$(basename "$f" .kicad_pcb)"
    cp "$f" "$SCRATCH/"
    ( cd "$SCRATCH" && "$DUMP" board --sections track,pad,connectivity "$b.kicad_pcb" 2>/dev/null ) \
        | grep -E "$FILTER" > "$GOLDEN/cn04/$b.txt"
done

BOARDS=( via_dangling issue18839 teardrop_close_via issue23123_minimal issue23339_zone_layer_rules
         soldermask_track_to_pad custom_pads padstacks_complex via_stacks tracks_arcs_vias fill_bad
         net_tie_drc connection_width_rules issue8003 test_copper_graphics intersectingzones
         issue14334 notched_zones issue9870
         issue10697 issue18142 issue19574 issue2512 teardrop_multi_inside_via
         teardrop_offcenter_two_segment tuning_generators_load_save two_segment_teardrop
         via_stack_track_width zone_thieving_track_bisection bad_triangulation_case )

for b in "${BOARDS[@]}"; do
    cp "$DATA/$b.kicad_pcb" "$SCRATCH/"
    for ext in kicad_pro kicad_dru; do
        [ -f "$DATA/$b.$ext" ] && cp "$DATA/$b.$ext" "$SCRATCH/"
    done
    ( cd "$SCRATCH" && "$DUMP" board --sections track,pad "$b.kicad_pcb" 2>/dev/null ) \
        | { grep -E "$FLASH_FILTER" || true; } > "$GOLDEN/cn04_flash/$b.txt"
done
