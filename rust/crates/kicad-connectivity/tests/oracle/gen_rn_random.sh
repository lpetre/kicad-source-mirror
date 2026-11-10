#!/bin/bash
# CN-03: regenerates the synthetic ratsnest boards (tests/golden/rn_random/*.kicad_pcb, written
# by gen_rn_random.py) and their goldens (the `rn_net` lines of the reference pcb-dump
# `connectivity` section, rust/docs/PCB_DUMP_FORMAT.md section 8).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
OUT="$HERE/../golden/rn_random"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

mkdir -p "$OUT"
python3 -I "$HERE/gen_rn_random.py" "$OUT"

for f in "$OUT"/*.kicad_pcb; do
    b="$(basename "$f" .kicad_pcb)"
    cp "$f" "$SCRATCH/"
    ( cd "$SCRATCH" && "$DUMP" board --sections connectivity "$b.kicad_pcb" 2>/dev/null ) \
        | grep '^rn_net|' > "$OUT/$b.txt"
done
