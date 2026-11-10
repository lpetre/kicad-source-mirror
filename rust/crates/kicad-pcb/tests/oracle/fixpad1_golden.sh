#!/bin/sh
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
# Package FIX-PAD-1: regenerates tests/golden/fixpad1/back_margins_primitives.txt, the hashed
# reference pcb-dump (QA load path, so that the project's max_error is applied) of a scratch
# copy of tests/data/fixpad1/back_margins_primitives.kicad_pcb + .kicad_pro. The Rust replay
# is tests/fixpad1_pads.rs.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
DATA="$HERE/../data/fixpad1"
SCRATCH=$(mktemp -d)
cp "$DATA/back_margins_primitives.kicad_pcb" "$DATA/back_margins_primitives.kicad_pro" "$SCRATCH/"
mkdir -p "$HERE/../golden/fixpad1"
cd "$SCRATCH"
HOME="$SCRATCH/home" nice -n 15 /home/user/kicad-oracle/bin/pcb-dump board --hash \
    --sections item,pad,bbox back_margins_primitives.kicad_pcb \
    > "$HERE/../golden/fixpad1/back_margins_primitives.txt"
cd /
rm -rf "$SCRATCH"
