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
# Package M8 (EDA_SHAPE effective shapes, polygons, hatching, line endings): regenerates
# tests/golden/m8_shapes.txt from the reference pcb-dump (rust/docs/ORACLE.md) on a scratch
# copy of tests/oracle/m8_shapes.kicad_pcb (every SHAPE_T, fills incl. the three hatch kinds,
# rounded rectangles, line endings of every style). The stored members are kept in clear
# (the Rust replay rebuilds the shapes from them), the effective shapes (`shape.<L>`), the
# `TransformShapeToPolygon` combos (`tpoly.*`) and `hatching` as FNV-1a hashes. The Rust
# replay is tests/m8_shapes_oracle.rs (feature `dump`).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
SCRATCH=$(mktemp -d)
cp "$HERE/m8_shapes.kicad_pcb" "$SCRATCH/m8_shapes.kicad_pcb"
KICAD_ORACLE_HOME="$SCRATCH/home" /home/user/kicad-oracle/bin/pcb-dump board \
    --sections shape "$SCRATCH/m8_shapes.kicad_pcb" > "$SCRATCH/plain.txt"
KICAD_ORACLE_HOME="$SCRATCH/home" /home/user/kicad-oracle/bin/pcb-dump board --hash \
    --sections shape "$SCRATCH/m8_shapes.kicad_pcb" > "$SCRATCH/hash.txt"
{
    grep -E '^shape\|[^|]*\|(shape|start|end|width|effective_width|fill|closed|corner_radius|center|mid|radius|poly|bezier_c1|bezier_c2|bezier_points|ellipse_[a-z]*|hatch_line_width|hatch_line_spacing)=' "$SCRATCH/plain.txt"
    grep -E '^shape\|[^|]*\|(shape\.|tpoly\.|hatching=)' "$SCRATCH/hash.txt"
} > "$HERE/../golden/m8_shapes.txt"
rm -rf "$SCRATCH"
