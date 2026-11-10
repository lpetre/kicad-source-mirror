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
# A second part runs synthetic cases: the PCBIO-04 boards (pre-V6 legacy setup, V6, current) with
# four nets appended, each without a project (QA binds none, the CLI an empty one) and with
# tests/data/set5/design.kicad_pro (design rules, classes, patterns, label assignment); the
# pre-V6 board checks that legacy setup tokens disable board.design_settings.
#
# Usage (from <kicad>/rust): [SET5_SYNTHETIC_ONLY=1] crates/kicad-pcb/tests/oracle/set5_dump_compare.sh [OUT_LOG]
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
# SET5_SYNTHETIC_ONLY=1 skips the corpus part.
[ -n "${SET5_SYNTHETIC_ONLY:-}" ] || find qa/data demos -name '*.kicad_pcb' | sort | while read -r f; do
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

run_case() { # name dir
    for mode in qa loader; do
        flag=""
        [ "$mode" = loader ] && flag="--loader"
        (cd "$2" && timeout 300 nice -n 10 "$dump" board $flag --sections board \
            "$1.kicad_pcb" > "$scratch/ref.txt" 2> /dev/null)
        rm -f "$2/$1.kicad_prl"
        (cd "$2" && timeout 300 "$rsdump" board $flag --sections board \
            "$1.kicad_pcb" > "$scratch/rs.txt" 2> /dev/null)
        echo "[$mode] $(python3 -I "$cmp" "$scratch/ref.txt" "$scratch/rs.txt" "synthetic/$1")" >> "$log"
    done
}

for f in "$rust"/crates/kicad-pcb/tests/data/pcbio04/{legacy_v5,stackup_v6,current_misc}.kicad_pcb; do
    name=$(basename "${f%.kicad_pcb}")
    for pro in none design; do
        rm -rf "$scratch/w" && mkdir "$scratch/w"
        python3 -I - "$f" "$scratch/w/$name-$pro.kicad_pcb" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8").read().rstrip()
assert text.endswith(")")
nets = '  (net 1 "+5V")\n  (net 2 "/SIG")\n  (net 3 "GND")\n  (net 4 "Net-(R1-Pad1)")\n'
open(sys.argv[2], "w", encoding="utf-8").write(text[:-1].rstrip() + "\n" + nets + ")\n")
PY
        [ "$pro" = design ] && cp "$rust/crates/kicad-pcb/tests/data/set5/design.kicad_pro" \
            "$scratch/w/$name-$pro.kicad_pro"
        run_case "$name-$pro" "$scratch/w"
    done
done
