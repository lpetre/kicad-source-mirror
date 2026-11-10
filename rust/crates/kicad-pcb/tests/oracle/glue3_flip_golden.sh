#!/bin/bash
# This program source code file is part of KiCad, a free EDA CAD application.
#
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation, either version 3 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program.  If not, see <http://www.gnu.org/licenses/>.
#
# Package GLUE-3: regenerates tests/golden/glue3/flip_tables.txt, the saved board after
# Flip / Mirror of every top-level item of pcbio12/generated_tables.kicad_pcb (a drill chart
# and a stack-up table: PCB_GENERATED_TABLE::Flip). Uses the FIX-EDIT-1 mutator driver
# (editmut_oracle.cpp); BIN=<path> reuses a built binary, else it is built once under the
# machine-wide C++ lock (as editmut_golden.sh does). Replay: tests/glue3.rs.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

if [ -z "${BIN:-}" ]; then
    DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
    KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
    OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
    BIN=$SCRATCH/editmut_oracle
    eval "CFLAGS=( $(cat "$OBJ_DIR/cflags.txt") )"
    QA="$KICAD_SRC/qa"
    CFLAGS+=( -I"$DUMP_SRC" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include" -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools"
              $(pkg-config --cflags gtk+-3.0) -w )
    LOCK=/home/user/kicad-oracle/cxx-build.lock
    flock "$LOCK" nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/editmut_oracle.cpp" -o "$SCRATCH/editmut_oracle.o"
    sed -e "s#$OBJ_DIR/rust_tools_pcb-dump_main.cpp.o#$SCRATCH/editmut_oracle.o#" \
        -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $BIN#" \
        "$OBJ_DIR/link.sh" > "$SCRATCH/link.sh"
    flock "$LOCK" nice -n 15 bash "$SCRATCH/link.sh"
fi

mkdir -p "$SCRATCH/in" "$SCRATCH/out" "$SCRATCH/home"
cp "$CRATE/tests/data/pcbio12/generated_tables.kicad_pcb" "$SCRATCH/in/"
(cd "$SCRATCH/in" && KICAD_CONFIG_HOME="$SCRATCH/home" HOME="$SCRATCH/home" \
    "$BIN" "$SCRATCH/out" generated_tables.kicad_pcb > /dev/null)

python3 -I - "$SCRATCH/in/generated_tables.kicad_pcb" "$SCRATCH/out" > "$CRATE/tests/golden/glue3/flip_tables.txt" <<'PY'
import re, sys
src = open(sys.argv[1], encoding="utf-8").read().lower()
known = set(re.findall(r'"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"', src))
def fnv(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h
uuid = re.compile(r'"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"')
for edit in ["none", "flip_lr", "flip_tb", "mirror_lr", "mirror_tb"]:
    text = open("%s/%s__generated_tables.kicad_pcb" % (sys.argv[2], edit), encoding="utf-8").read()
    text = uuid.sub(lambda m: m.group(0) if m.group(1) in known else '"generated"', text)
    data = text.encode("utf-8")
    print("%s=%s:%d" % (edit, fnv(data), len(data)))
PY
cat "$CRATE/tests/golden/glue3/flip_tables.txt"
