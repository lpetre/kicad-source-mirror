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
# Package LIVE-1: regenerates tests/golden/live1/<board>.golden from the scripts
# tests/golden/live1/<board>.script (written by `LIVE1_WRITE_SCRIPTS=1 cargo test -p
# kicad-session --test live1_oracle live1_write_scripts`) with live1_oracle.cpp, built against
# the pcb-dump objects of the reference build (one compile, one link, under the machine-wide
# C++ build lock, as editmut_golden.sh of kicad-pcb).
#   KEEP=<dir>   keep the driver binary there      NOBUILD=1   reuse $KEEP/live1_oracle
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT
GOLDEN=$CRATE/tests/golden/live1
mkdir -p "$SCRATCH/home" "$SCRATCH/in"
BIN=${KEEP:-$SCRATCH}/live1_oracle

if [ -z "${NOBUILD:-}" ]; then
    eval "CFLAGS=( $(cat "$OBJ_DIR/cflags.txt") )"
    QA="$KICAD_SRC/qa"
    CFLAGS+=( -I"$DUMP_SRC" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include"
              -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools" $(pkg-config --cflags gtk+-3.0) -w )
    LOCK=/home/user/kicad-oracle/cxx-build.lock
    flock "$LOCK" nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/live1_oracle.cpp" -o "$SCRATCH/live1_oracle.o"
    sed -e "s#$OBJ_DIR/rust_tools_pcb-dump_main.cpp.o#$SCRATCH/live1_oracle.o#" \
        -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $BIN#" \
        "$OBJ_DIR/link.sh" > "$SCRATCH/link.sh"
    flock "$LOCK" nice -n 15 bash "$SCRATCH/link.sh"
fi

grep -v '^#' "$HERE/live1_boards.txt" | grep -v '^$' | head -10 | while read -r rel; do
    stem=$(basename "$rel" .kicad_pcb)
    cp "$KICAD_SRC/$rel" "$SCRATCH/in/$stem.kicad_pcb"
    HOME=$SCRATCH/home "$BIN" "$SCRATCH/in/$stem.kicad_pcb" "$GOLDEN/$stem.script" \
        > "$GOLDEN/$stem.golden" 2> "$SCRATCH/$stem.err" || { echo "$stem failed"; tail -5 "$SCRATCH/$stem.err"; }
    echo "$stem: $(wc -l < "$GOLDEN/$stem.golden") section hashes"
done
