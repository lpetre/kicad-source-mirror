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
#
# Integration step 2 (INT-2a kernel changes): builds int2a_oracle.cpp against the
# pcb-dump objects of the reference build (rust/docs/ORACLE.md; the machine-wide C++ build
# lock, low priority) and regenerates tests/golden/int2a_mutators.txt. The Rust replay is
# `cargo test -p kicad-pcb --features dump --lib int2a_matches_oracle`.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

eval "CFLAGS=( $(cat "$OBJ_DIR/cflags.txt") )"
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$DUMP_SRC" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include" -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools"
          $(pkg-config --cflags gtk+-3.0) -w )

LOCK=/home/user/kicad-oracle/cxx-build.lock
flock "$LOCK" nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/int2a_oracle.cpp" -o "$SCRATCH/int2a_oracle.o"

sed -e "s#$OBJ_DIR/rust_tools_pcb-dump_main.cpp.o#$SCRATCH/int2a_oracle.o#" \
    -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $SCRATCH/int2a_oracle#" \
    "$OBJ_DIR/link.sh" > "$SCRATCH/link.sh"
flock "$LOCK" nice -n 15 bash "$SCRATCH/link.sh"

KICAD_CONFIG_HOME="$SCRATCH/home" HOME="$SCRATCH/home" \
    "$SCRATCH/int2a_oracle" > "$CRATE/tests/golden/int2a_mutators.txt"
wc -l "$CRATE/tests/golden/int2a_mutators.txt"
