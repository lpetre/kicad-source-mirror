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
# Regenerates tests/golden/rtree_oracle.txt with drc-rtree-oracle (build_rtree_oracle.sh).
# The boards are copied to a scratch directory first (the loader may write side files).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
ORACLE="${ORACLE:-/home/user/kicad-oracle/bin/drc-rtree-oracle}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Synthetic boards (gen_rtree_boards.py) first, then small QA boards with pads, zones and
# graphics (the Rust replay reports those as pending until the model shapes them).
BOARDS=( "$HERE/../data/rtree/rtree_small.kicad_pcb"
         "$HERE/../data/rtree/rtree_ties.kicad_pcb"
         "$HERE/../data/rtree/rtree_4layer.kicad_pcb" )

for b in issue7267 issue18203 issue16566 net_tie_hole_clearance via_stacks \
         connection_width_rules issue24649; do
    BOARDS+=( "$KICAD_SRC/qa/data/pcbnew/$b.kicad_pcb" )
done

ARGS=()

for b in "${BOARDS[@]}"; do
    cp "$b" "$TMP/"
    ARGS+=( "$TMP/$(basename "$b")" )
done

"$ORACLE" --global "${ARGS[@]}" 2>/dev/null > "$HERE/../golden/rtree_oracle.txt"
wc -lc "$HERE/../golden/rtree_oracle.txt"
