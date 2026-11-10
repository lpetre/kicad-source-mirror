#!/bin/bash
# STAB-CN: connectivity goldens of corpus boards with footprints (stab_cn_boards.txt), written
# with the existing reference tools (no C++ build):
#
# - tests/golden/stab_cn/<name>.txt: the `connectivity` section of `pcb-dump board` (QA load
#   path), replayed by the CN-01 .. CN-04 tests like tests/golden/<board>.txt;
# - tests/golden/stab_cn/<name>.flash.txt: the `flash.<layer>` lines of the `track` / `pad`
#   sections (PCB_VIA::FlashLayer / PAD::FlashLayer through IsConnectedOnLayer);
# - tests/golden/order/<name>.txt: cn-order-oracle --queries, load phase, heap-independent
#   lines only (as gen_cn04.sh).
#
# Inputs (board plus .kicad_pro / .kicad_dru) are copied to a scratch directory first.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD="$(cd "$HERE/../../../../.." && pwd)"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
ORDER="${ORDER_ORACLE:-/home/user/kicad-oracle/bin/cn-order-oracle}"
GOLDEN="$HERE/../golden"
DATA="$KICAD/qa/data"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

mkdir -p "$GOLDEN/stab_cn" "$GOLDEN/order"
FLASH_FILTER='^(track|pad)\|[^|]*\|flash\.'

grep -v '^#' "$HERE/stab_cn_boards.txt" | while read -r rel; do
    [ -n "$rel" ] || continue
    b="$(basename "$rel")"
    d="$SCRATCH/$b"
    mkdir -p "$d"
    cp "$DATA/$rel.kicad_pcb" "$d/"
    for ext in kicad_pro kicad_dru; do
        [ -f "$DATA/$rel.$ext" ] && cp "$DATA/$rel.$ext" "$d/"
    done
    ( cd "$d" && "$DUMP" board --sections connectivity "$b.kicad_pcb" 2>/dev/null ) \
        > "$GOLDEN/stab_cn/$b.txt"
    ( cd "$d" && "$DUMP" board --sections track,pad "$b.kicad_pcb" 2>/dev/null ) \
        | { grep -E "$FLASH_FILTER" || true; } > "$GOLDEN/stab_cn/$b.flash.txt"
    if [ -x "$ORDER" ]; then
        ( cd "$d" && "$ORDER" --queries "$b.kicad_pcb" 2>/dev/null ) \
            | { grep -E '^load\|(q|rn|island)\|' || true; } > "$GOLDEN/order/$b.txt"
    fi
done
