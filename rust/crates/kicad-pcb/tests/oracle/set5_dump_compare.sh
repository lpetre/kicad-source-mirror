#!/bin/bash
# SET-5 item-level check of the project binding and the two load sequences: for every corpus
# board (qa/data, demos) with a sibling .kicad_pro, the `board` section of the reference
# `pcb-dump board` (QA KI_TEST::LoadBoard path) and `pcb-dump board --loader` (kicad-cli
# BOARD_LOADER path) against `pcb-dump-rs board` / `pcb-dump-rs board --loader`, compared by
# set5_board_compare.py (ds.*, net classes, per-net and effective net classes by net name,
# composite classes, title block, has_project). Boards over 6 MB and the large demos are left
# out (machine limits, PORTING.md 7). The board, its .kicad_pro, .kicad_prl and .kicad_dru are
# copied to a scratch directory first. Nothing is committed; the summary goes to PORT_STATUS.md.
#
# Usage (from <kicad>/rust): crates/kicad-pcb/tests/oracle/set5_dump_compare.sh [OUT_LOG]
set -uo pipefail
rust=$(cd "$(dirname "$0")/../../../.." && pwd)
root=$(cd "$rust/.." && pwd)
dump=${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}
cmp="$rust/crates/kicad-pcb/tests/oracle/set5_board_compare.py"
log=${1:-/dev/stdout}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

(cd "$rust" && cargo build -q -p kicad-pcb --features dump --bin pcb-dump-rs) || exit 1
rsdump="$rust/target/debug/pcb-dump-rs"

: > "$log"
cd "$root"
find qa/data demos -name '*.kicad_pcb' | sort | while read -r f; do
    base="${f%.kicad_pcb}"
    [ -f "$base.kicad_pro" ] || continue
    [ "$(stat -c %s "$f")" -le 6000000 ] || continue
    case "$f" in
        *jetson*|*vme-wren*|*openair-max*|*tiny_tapeout*|*cm5_minima*) continue ;;
    esac

    rm -rf "$scratch/w" && mkdir "$scratch/w"
    name=$(basename "$base")
    for ext in kicad_pcb kicad_pro kicad_prl kicad_dru; do
        [ -f "$base.$ext" ] && cp "$base.$ext" "$scratch/w/$name.$ext"
    done

    for mode in qa loader; do
        flag=""
        [ "$mode" = loader ] && flag="--loader"
        (cd "$scratch/w" && timeout 300 nice -n 10 "$dump" board $flag --sections board \
            "$name.kicad_pcb" > "$scratch/ref.txt" 2> /dev/null)
        # The reference tool may write side files: restore the inputs for the Rust run.
        rm -f "$scratch/w/$name.kicad_prl"
        [ -f "$base.kicad_prl" ] && cp "$base.kicad_prl" "$scratch/w/$name.kicad_prl"
        (cd "$scratch/w" && timeout 300 "$rsdump" board $flag --sections board \
            "$name.kicad_pcb" > "$scratch/rs.txt" 2> /dev/null)
        echo "[$mode] $(python3 -I "$cmp" "$scratch/ref.txt" "$scratch/rs.txt" "$f")" >> "$log"
    done
done
