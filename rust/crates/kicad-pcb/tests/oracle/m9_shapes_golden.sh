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
# Package M9 (PCB_SHAPE): regenerates tests/golden/m9_shapes.txt from the reference pcb-dump
# (rust/docs/ORACLE.md) on a scratch copy of tests/oracle/m9_shapes.kicad_pcb: board-level
# copper shapes with solder mask / mask margin / net, and the same eleven footprint shapes
# (segment, rectangles plain and rounded, circle, arc, polygon, bezier, ellipses with a
# skewed and a cardinal rotation, ellipse arcs) in six footprints: rotated, flipped,
# non-uniformly scaled and rotated, uniformly scaled and rotated, scaled only, flipped and
# non-uniformly scaled. Kept: the footprint transforms, every `bbox` / `shape` value of the
# shapes and their M9-relevant `item` values (layers, position, copper, side), in clear
# except values longer than 100 characters (polygons, bezier points, effective shapes
# `shape.<L>`, `TransformShapeToPolygon` combos `tpoly.*`), which are stored as FNV-1a
# hashes. The Rust replay is `pcb_shape::tests::m9_shapes_match_oracle` (feature `dump`).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
SCRATCH=$(mktemp -d)
cp "$HERE/m9_shapes.kicad_pcb" "$SCRATCH/m9_shapes.kicad_pcb"
for MODE in plain hash; do
    FLAG=""
    [ "$MODE" = hash ] && FLAG="--hash"
    KICAD_ORACLE_HOME="$SCRATCH/home" /home/user/kicad-oracle/bin/pcb-dump board $FLAG \
        --sections item,bbox,shape,footprint "$SCRATCH/m9_shapes.kicad_pcb" > "$SCRATCH/$MODE.txt"
done
python3 -I - "$SCRATCH/plain.txt" "$SCRATCH/hash.txt" > "$HERE/../golden/m9_shapes.txt" <<'PY'
import re, sys
plain = open(sys.argv[1]).read().splitlines()
hashed = open(sys.argv[2]).read().splitlines()
assert len(plain) == len(hashed)
keep = re.compile(r'^(footprint\|[^|]*\|(flipped|transform\.(translate|rotate|scale))='
                  r'|item\|c9[^|]*\|(layer|layers|layerset|position|on_copper|side_specific)='
                  r'|(bbox|shape)\|c9)')
for p, h in zip(plain, hashed):
    if keep.match(p):
        print(h if len(p.split('=', 1)[1]) > 100 else p)
PY
rm -rf "$SCRATCH"
