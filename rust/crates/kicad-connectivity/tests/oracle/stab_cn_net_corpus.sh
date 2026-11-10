#!/bin/bash
# STAB-CN step 4: the net of every connected item after the load path (BOARD_LOADER::Load,
# BuildConnectivity, net propagation) - reference `pcb-dump board --loader --hash --sections
# item` against kicad-session's `pcb-dump-session board --loader` (same flags), over every
# corpus board (qa/data/pcbnew, demos) under 5 MB. Only the `net` / `netname` / `type` lines
# are kept (piped, nothing large is stored). One reference process at a time; the large demo
# boards (openair-max, tiny_tapeout, cm5_minima) run last, alone, unless SKIP_LARGE=1.
#
# Build first: cargo build -p kicad-session --features dump --bin pcb-dump-session
# Output: rust/target/stab_cn/net_results.txt (one line per board, see stab_cn_net_compare.py)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$HERE/../../../../.." && pwd)"
out=$root/rust/target/stab_cn
rs=${PCB_DUMP_SESSION:-$out/pcb-dump-session}
ref=${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}
export KICAD_ORACLE_HOME=$out/home
mkdir -p "$KICAD_ORACLE_HOME" "$out"
res=$out/net_results.txt
: > "$res"
FILTER='^item\|[^|]*\|(type|net|netname)='
LARGE='openair-max|tiny_tapeout|cm5_minima'

run_board() {
    f=$1
    stem=$(basename "$f" .kicad_pcb); d=$(dirname "$f")
    rm -rf "$out/w"; mkdir -p "$out/w"; cp "$f" "$out/w/"
    for e in kicad_pro kicad_prl kicad_dru pro; do [ -f "$d/$stem.$e" ] && cp "$d/$stem.$e" "$out/w/"; done
    rm -rf "$out/w2"; cp -r "$out/w" "$out/w2"
    ( cd "$out/w" && timeout 600 "$rs" board --loader --hash --sections item "$stem.kicad_pcb" 2>/dev/null ) \
        | grep -E "$FILTER|^error\|" > "$out/rs.txt"
    ( cd "$out/w2" && timeout 900 nice -n 10 "$ref" board --loader --hash --sections item "$stem.kicad_pcb" 2>/dev/null ) \
        | grep -E "$FILTER|^error\|" > "$out/ref.txt"
    echo "${f#$root/} $(python3 -I "$HERE/stab_cn_net_compare.py" "$out/ref.txt" "$out/rs.txt")" >> "$res"
    rm -f "$out/rs.txt" "$out/ref.txt"
}

find "$root/qa/data/pcbnew" "$root/demos" -name '*.kicad_pcb' -size -5000k | sort | grep -Ev "$LARGE" \
    | while read -r f; do run_board "$f"; done
if [ "${SKIP_LARGE:-0}" != 1 ]; then
    find "$root/demos" -name '*.kicad_pcb' -size -5000k | sort | grep -E "$LARGE" \
        | while read -r f; do run_board "$f"; done
fi
rm -rf "$out/w" "$out/w2"
echo DONE >> "$res"
