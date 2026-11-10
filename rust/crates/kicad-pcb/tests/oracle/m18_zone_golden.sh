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
# Package M18 (ZONE model): builds tests/oracle/m18_zone_oracle.cpp against the reference build,
# reusing the pcb-dump objects (rust/tools/pcb-dump/build.sh must have run: compile flags in
# $OBJ_DIR/cflags.txt, format helpers, QA harness objects, link line of _pcbnew.kiface), runs it
# on a scratch copy of tests/oracle/m18_zones.kicad_pcb and regenerates
# tests/golden/m18_zones.txt. The Rust replay is zone::oracle_tests.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
ORACLE_BUILD=/home/user/kicad-oracle/build
OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

eval "CFLAGS=( $(cat "$OBJ_DIR/cflags.txt") )"
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$DUMP_SRC" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include" -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools"
          $(pkg-config --cflags gtk+-3.0) -w )

# Every C++ compile / link runs under the machine-wide lock (PORTING.md section 7).
LOCK=/home/user/kicad-oracle/cxx-build.lock
flock "$LOCK" nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/m18_zone_oracle.cpp" -o "$SCRATCH/m18_zone_oracle.o"

# The pcb-dump link line with our object in place of main.cpp.o and our output.
sed -e "s#$OBJ_DIR/rust_tools_pcb-dump_main.cpp.o#$SCRATCH/m18_zone_oracle.o#" \
    -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $SCRATCH/m18_zone_oracle#" \
    "$OBJ_DIR/link.sh" > "$SCRATCH/link.sh"
flock "$LOCK" nice -n 15 bash "$SCRATCH/link.sh"

cp "$HERE/m18_zones.kicad_pcb" "$SCRATCH/m18_zones.kicad_pcb"
KICAD_CONFIG_HOME="$SCRATCH/home" HOME="$SCRATCH/home" \
    "$SCRATCH/m18_zone_oracle" "$SCRATCH/m18_zones.kicad_pcb" > "$CRATE/tests/golden/m18_zones.txt"
wc -l "$CRATE/tests/golden/m18_zones.txt"
