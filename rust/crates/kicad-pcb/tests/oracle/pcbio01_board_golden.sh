#!/bin/bash
# PCBIO-01 differential oracle: the board-skeleton fields of `pcb-dump board --no-init
# --sections board` (the C++ parser only) for the boards listed in pcbio01_boards.txt (paths
# relative to the KiCad source root) and the hand-made boards in tests/data/pcbio01/.
# Writes tests/golden/pcbio01_board_section.txt, replayed by the Rust test
# `io::parser::tests::oracle_board_section`.
#
# Usage: tests/oracle/pcbio01_board_golden.sh   (run from rust/crates/kicad-pcb)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../../../.." && pwd)
dump=${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}
scratch=$(mktemp -d)
export KICAD_ORACLE_HOME=$scratch/home
out=$here/golden/pcbio01_board_section.txt
keys='^board\|board\|(file_format_version|generator|copper_layer_count|enabled_layers|layer\..*\.(name|type)|page\..*|title_block\..*|property\..*|net\.-?[0-9]+\.name)='
: > "$out"

emit() {   # emit <label> <file>
    local d=$scratch/$RANDOM$RANDOM
    mkdir -p "$d"
    cp "$2" "$d/"
    echo "file|$1" >> "$out"
    "$dump" board --no-init --sections board "$d/$(basename "$2")" 2>/dev/null \
        | grep -E "$keys|^error\|load" | sed -e 's/^board|board|//' -e 's/^error|load|[^|]*|/error=/' >> "$out" || true
    rm -rf "$d"
}

while read -r f; do emit "$f" "$root/$f"; done < "$here/oracle/pcbio01_boards.txt"
for f in "$here"/data/pcbio01/*.kicad_pcb; do emit "inline:$(basename "$f")" "$f"; done
rm -rf "$scratch"
