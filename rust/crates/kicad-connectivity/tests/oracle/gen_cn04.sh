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
# - REV-CN: tests/golden/<board>.txt for the rev_* boards of gen_cn04.py (their complete
#   `connectivity` section, replayed by the CN-01 .. CN-03 tests) and tests/golden/order/*.txt
#   (cn-order-oracle --queries, replayed by tests/rev_order_golden.rs).
#
# Inputs are copied to a scratch directory first (the tool writes side files). Uses the
# existing pcb-dump binary (no C++ build) and, when it has been built (build_order_oracle.sh),
# cn-order-oracle.
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
        > "$SCRATCH/$b.all.txt"
    grep -E "$FILTER" "$SCRATCH/$b.all.txt" > "$GOLDEN/cn04/$b.txt"

    # REV-CN boards: also the complete connectivity section, replayed by the CN-01 .. CN-03
    # tests (zone contact paths, the via-net rule, islands, copper graphics, propagation).
    case "$b" in
    rev_*) grep -E '^(file|cn_item|cn_cluster|rn_net|cn_dangling|cn_island)\|' "$SCRATCH/$b.all.txt" \
               > "$GOLDEN/$b.txt" ;;
    esac
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

# REV-CN: the queries without a pcb-dump line, from cn-order-oracle (build_order_oracle.sh; the
# step is skipped when the binary is absent). Only the load phase and only the lines that do not
# depend on the heap (pointer-ordered results are printed in the port's order). The corpus
# boards add pads: IsConnectedOnLayer of pads in zones, islands with a single connection.
ORDER="${ORDER_ORACLE:-/home/user/kicad-oracle/bin/cn-order-oracle}"
ORDER_BOARDS=( cn04_flash cn04_dangling rev_zone_contacts rev_ratsnest_zones rev_shapes rev_order_probe
               teardrop_close_via net_tie_drc custom_pads padstacks_complex soldermask_track_to_pad
               issue23339_zone_layer_rules notched_zones intersectingzones fill_bad issue14334
               issue9870 issue8003 connection_width_rules test_copper_graphics issue23123_minimal
               tracks_arcs_vias )

if [ -x "$ORDER" ]; then
    mkdir -p "$GOLDEN/order"
    for b in "${ORDER_BOARDS[@]}"; do
        ( cd "$SCRATCH" && "$ORDER" --queries "$b.kicad_pcb" 2>/dev/null ) \
            | grep -E '^load\|(q|rn|island)\|' > "$GOLDEN/order/$b.txt"
    done
fi
