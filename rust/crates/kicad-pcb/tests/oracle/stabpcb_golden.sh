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
# Package STAB-PCB: regenerates tests/golden/stabpcb/.
#  * flip_scaled_custom_pads.txt: stabpcb_oracle.cpp (built against the pcb-dump objects of
#    the reference build, under the machine-wide C++ build lock) on a scratch copy of
#    tests/data/stabpcb/scaled_custom_pads.kicad_pcb: every footprint flipped, the pad
#    primitives' board / library coordinates and the pcb-dump pad and shape sections.
#  * scaled_custom_pads.upgraded.kicad_pcb: reference `kicad-cli pcb upgrade --force` of the
#    same board (raw `(zone_connect N)` values outside the enum are written back).
# The Rust replay is tests/stab_pcb.rs.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
DUMP_SRC=$(cd "$CRATE/../../tools/pcb-dump" && pwd)
KICAD_SRC=$(cd "$CRATE/../../.." && pwd)
OBJ_DIR=/home/user/kicad-oracle/build-pcb-dump
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT
GOLDEN=$CRATE/tests/golden/stabpcb
mkdir -p "$GOLDEN" "$SCRATCH/home"

eval "CFLAGS=( $(cat "$OBJ_DIR/cflags.txt") )"
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$DUMP_SRC" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include" -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools"
          $(pkg-config --cflags gtk+-3.0) -w )

LOCK=/home/user/kicad-oracle/cxx-build.lock
flock "$LOCK" nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/stabpcb_oracle.cpp" -o "$SCRATCH/stabpcb_oracle.o"

sed -e "s#$OBJ_DIR/rust_tools_pcb-dump_main.cpp.o#$SCRATCH/stabpcb_oracle.o#" \
    -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $SCRATCH/stabpcb_oracle#" \
    "$OBJ_DIR/link.sh" > "$SCRATCH/link.sh"
flock "$LOCK" nice -n 15 bash "$SCRATCH/link.sh"

cp "$CRATE/tests/data/stabpcb/scaled_custom_pads.kicad_pcb" "$SCRATCH/"
(cd "$SCRATCH" && KICAD_CONFIG_HOME="$SCRATCH/home" HOME="$SCRATCH/home" \
    "$SCRATCH/stabpcb_oracle" scaled_custom_pads.kicad_pcb) > "$GOLDEN/flip_scaled_custom_pads.txt"
wc -l "$GOLDEN/flip_scaled_custom_pads.txt"

mkdir -p "$SCRATCH/up"
cp "$CRATE/tests/data/stabpcb/scaled_custom_pads.kicad_pcb" "$SCRATCH/up/"
HOME="$SCRATCH/home" /home/user/kicad-oracle/bin/kicad-cli pcb upgrade --force \
    "$SCRATCH/up/scaled_custom_pads.kicad_pcb" > /dev/null 2>&1 || true
cp "$SCRATCH/up/scaled_custom_pads.kicad_pcb" "$GOLDEN/scaled_custom_pads.upgraded.kicad_pcb"
