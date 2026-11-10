#!/bin/bash
# Regenerates tests/golden/upgrade/<board>.kicad_pcb with the reference kicad-cli
# (docs/ORACLE.md): `pcb upgrade --force` on a copy of each board (the board is re-saved in
# place) and records the exit code and stdout in tests/golden/upgrade/<board>.out.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
O=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
OUT=$HERE/../golden/upgrade
BOARDS=( qa/data/pcbnew/issue10697.kicad_pcb )
mkdir -p "$OUT"
W=$(mktemp -d)
export KICAD_ORACLE_HOME=$W/home
for b in "${BOARDS[@]}"; do
    name=$(basename "$b")
    cp "$KICAD/$b" "$W/$name"
    (cd "$W" && "$O" pcb upgrade --force "$name" > out.txt 2>/dev/null; echo "rc=$?" >> out.txt)
    cp "$W/$name" "$OUT/$name"
    cp "$W/out.txt" "$OUT/${name%.kicad_pcb}.out"
done
rm -rf "$W"
echo "wrote $OUT"
