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
# Package FIX-EDIT-1: regenerates tests/golden/editmut/editmut.txt.
# editmut_oracle.cpp (built against the pcb-dump objects of the reference build, one compile
# and one link under the machine-wide C++ build lock) applies every scripted edit to scratch
# copies of the boards of editmut_boards.txt; editmut_hash.py condenses the dumps and the
# saved boards into per-section hashes. The Rust replay is tests/fix_edit1.rs.
#   KEEP=<dir>   also keep the raw dumps and saved boards there (debugging; never commit)
#   NOBUILD=1    reuse the driver binary of a previous run in $KEEP
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT
GOLDEN=$CRATE/tests/golden/editmut
mkdir -p "$GOLDEN" "$SCRATCH/home" "$SCRATCH/in" "$SCRATCH/out"
[ -n "${KEEP:-}" ] && mkdir -p "$KEEP"
BIN=${KEEP:-$SCRATCH}/editmut_oracle

if [ -z "${NOBUILD:-}" ]; then
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

FILES=()
while read -r rel; do
    [ -z "$rel" ] && continue
    cp "$CRATE/tests/data/$rel" "$SCRATCH/in/"
    FILES+=( "$(basename "$rel")" )
done < "$HERE/editmut_boards.txt"

OUT=$SCRATCH/out
[ -n "${KEEP:-}" ] && OUT=$KEEP && mkdir -p "$KEEP"
(cd "$SCRATCH/in" && KICAD_CONFIG_HOME="$SCRATCH/home" HOME="$SCRATCH/home" \
    "$BIN" "$OUT" "${FILES[@]}") | python3 -I "$HERE/editmut_hash.py" "$OUT" "$SCRATCH/in" > "$GOLDEN/editmut.txt"
wc -l "$GOLDEN/editmut.txt"
