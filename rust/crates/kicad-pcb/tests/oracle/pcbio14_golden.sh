#!/bin/bash
# PCBIO-14 goldens (D1 of docs/analysis/pcb-io.md 9.3): `kicad-cli fp upgrade --force -o OUT IN`
# of the reference build, one .kicad_mod per scratch library.
#
# * tests/golden/pcbio14/<name>.kicad_mod: the oracle output of a few small files (byte goldens);
# * tests/golden/pcbio14_d1_corpus.txt: for every .kicad_mod of qa/data and demos, the FNV-1a 64
#   hash of the oracle output after masking the UUIDs that do not occur in the input (fresh
#   random UUIDs of items without one), or "FAIL <exit code>" when the oracle wrote nothing.
#
# Replayed by tests/pcbio14_fp_lib.rs. Runs at most two reference processes at a time (one here).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
KICAD=$(cd "$CRATE/../../.." && pwd)  # <kicad> (crates/kicad-pcb -> rust -> <kicad>)
REF=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
export KICAD_ORACLE_HOME=$WORK/home

upgrade() { # $1 = input file, $2 = output file
    rm -rf "$WORK/in.pretty" "$WORK/out"
    mkdir -p "$WORK/in.pretty"
    cp "$1" "$WORK/in.pretty/"
    "$REF" fp upgrade --force -o "$WORK/out" "$WORK/in.pretty" >/dev/null 2>&1 || true
    local b
    b=$(basename "$1")
    if [ -f "$WORK/out/$b" ]; then cp "$WORK/out/$b" "$2"; else rm -f "$2"; fi
}

for f in qa/data/pcbnew/plugins/kicad_sexpr/fp.pretty/R_0201_0603Metric.kicad_mod \
         qa/data/pcbnew/plugins/kicad_sexpr/Issue25447_FpHasZoneWithNetName/zone_with_netname.kicad_mod \
         qa/data/pcbnew/plugins/altium/pcblib/Tracks.pretty/KEEP_OUT_LAYER.kicad_mod; do
    upgrade "$KICAD/$f" "$CRATE/tests/golden/pcbio14/$(basename "$f")"
done

OUT=$CRATE/tests/golden/pcbio14_d1_corpus.txt
echo "# kicad-cli fp upgrade --force, masked FNV-1a 64 per corpus file (pcbio14_golden.sh)" > "$OUT"
(cd "$KICAD" && find qa/data demos -name '*.kicad_mod' | LC_ALL=C sort) | while read -r f; do
    upgrade "$KICAD/$f" "$WORK/o.kicad_mod"
    if [ -f "$WORK/o.kicad_mod" ]; then
        h=$(python3 -I "$HERE/pcbio14_mask_hash.py" "$KICAD/$f" "$WORK/o.kicad_mod")
        echo "$h $f" >> "$OUT"
    else
        echo "FAIL $f" >> "$OUT"
    fi
done
