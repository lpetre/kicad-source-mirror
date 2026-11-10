#!/bin/bash
# FIX-CN-1: regenerates tests/golden/fix_cn1_unconnected.txt, the `unconnected_items` of
# `kicad-cli pcb drc --format json` (the reference build, rust/docs/ORACLE.md) for the boards
# whose ratsnest ends tie in heap order (review-connectivity.md F-1) and one control board.
# Replayed by tests/fix_cn1_ties.rs with the tie-aware comparison rule.
#
# One line per violation, in report order:  <board>|<run>|<uuid of item 0>|<uuid of item 1>
# rn_coincident is run three times: its chain ties differ from run to run in the reference.
# Inputs are copied to a scratch directory first (kicad-cli writes side files).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CLI="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
GOLDEN="$HERE/../golden"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
export KICAD_ORACLE_HOME="$SCRATCH/home"
mkdir -p "$KICAD_ORACLE_HOME"
OUT="$GOLDEN/fix_cn1_unconnected.txt"
: > "$OUT.tmp"

run() { # board source, runs
    local src="$1" runs="$2" name
    name="$(basename "$src" .kicad_pcb)"
    cp "$src" "$SCRATCH/$name.kicad_pcb"
    for i in $(seq 1 "$runs"); do
        "$CLI" pcb drc --format json -o "$SCRATCH/$name.json" "$SCRATCH/$name.kicad_pcb" \
            > /dev/null 2>&1
        python3 -I - "$SCRATCH/$name.json" "$name" "$i" >> "$OUT.tmp" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
for v in d["unconnected_items"]:
    print("|".join([sys.argv[2], sys.argv[3]] + [i["uuid"] for i in v["items"]]))
PY
    done
}

run "$GOLDEN/cn04/rev_order_probe.kicad_pcb" 1
run "$GOLDEN/cn04/rev_ratsnest_zones.kicad_pcb" 1
run "$GOLDEN/rn_random/rn_coincident.kicad_pcb" 3
mv "$OUT.tmp" "$OUT"
