#!/bin/bash
# PCBIO-04 item-level check: the `board` section of the reference `pcb-dump board --no-init`
# (parser only) against `pcb-dump-rs board` for copies of small boards of several format
# versions (pre-V6 legacy setup tokens, V6 / current stackups, tenting forms, drill symbol
# profile), compared with tools/pcb-dump/compare.py. Only the fields the setup section sets
# matter for PCBIO-04: ds.* (except ds.meander.*, no Rust API), plot.*, stackup_or_default.*.
# Nothing is committed; the result is reported in PORT_STATUS.md.
#
# Usage (from <kicad>/rust): crates/kicad-pcb/tests/oracle/pcbio04_dump_compare.sh
set -euo pipefail
rust=$(cd "$(dirname "$0")/../../../.." && pwd)
root=$(cd "$rust/.." && pwd)
dump=${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

boards=(
    "$rust"/crates/kicad-pcb/tests/data/pcbio04/*.kicad_pcb
    "$root/demos/ecc83/ecc83-pp_v2.kicad_pcb"
    "$root/demos/pic_programmer/pic_programmer.kicad_pcb"
    "$root/qa/data/pcbnew/issue18142.kicad_pcb"
    "$root/demos/complex_hierarchy/complex_hierarchy.kicad_pcb"
)

(cd "$rust" && cargo build -q -p kicad-pcb --features dump --bin pcb-dump-rs)
i=0
for f in "${boards[@]}"; do
    [ -f "$f" ] || continue
    i=$((i + 1))
    cp "$f" "$scratch/b$i.kicad_pcb"
    nice -n 10 "$dump" board --no-init --sections board "$scratch/b$i.kicad_pcb" \
        > "$scratch/ref$i.txt" 2> /dev/null
    "$rust/target/debug/pcb-dump-rs" board --sections board "$scratch/b$i.kicad_pcb" \
        > "$scratch/rs$i.txt" 2> /dev/null
    bad=$(python3 -I "$rust/tools/pcb-dump/compare.py" "$scratch/ref$i.txt" "$scratch/rs$i.txt" \
        --show 100000 2>&1 | grep -E '\|board\|board\|(ds\.|plot\.|stackup_or_default\.)' \
        | grep -v 'ds\.meander\.' || true)
    n=$(grep -cE '^board\|board\|(ds\.|plot\.|stackup_or_default\.)' "$scratch/ref$i.txt" || true)
    echo "$(basename "$f"): $n setup-related reference lines, problems: ${bad:-none}"
done
