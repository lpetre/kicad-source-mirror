#!/bin/bash
# PROP-1: regenerates the goldens of tests/props_oracle.rs with the reference dump tool
# (rust/tools/pcb-dump, `props-table` command and opt-in `props` section; see
# docs/PCB_DUMP_FORMAT.md). The boards are copied to a scratch directory first (pcb-dump, like
# kicad-cli, may write side files next to its input). Values are hashed to keep the goldens small.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD="$(cd "$HERE/../../../../.." && pwd)"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
GOLDEN="$HERE/../golden"
S="$(mktemp -d)"
trap 'rm -rf "$S"' EXIT

"$DUMP" props-table --hash > "$GOLDEN/props_table.txt" 2>/dev/null

: > "$GOLDEN/props_values.txt"
while read -r rel; do
    [ -z "$rel" ] && continue
    src="$KICAD/qa/data/pcbnew/$rel"
    b="$(basename "$rel" .kicad_pcb)"
    mkdir -p "$S/$b"
    cp "$src" "$S/$b/"
    for ext in kicad_pro kicad_dru; do
        [ -f "${src%.kicad_pcb}.$ext" ] && cp "${src%.kicad_pcb}.$ext" "$S/$b/"
    done
    "$DUMP" board --hash --sections props "$S/$b/$b.kicad_pcb" >> "$GOLDEN/props_values.txt" 2>/dev/null
done < "$HERE/props_boards.txt"
