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
# Regenerates tests/golden/cachegen_oracle.txt (work package DRC-05) with
# `drc-rtree-oracle --cachegen` (build_rtree_oracle.sh). Each board is copied to a scratch
# directory together with its sibling .kicad_pro / .kicad_dru (the loader may write side files).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
ORACLE="${ORACLE:-/home/user/kicad-oracle/bin/drc-rtree-oracle}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Synthetic boards (gen_rtree_boards.py: tracks, arcs, vias, no project; the cachegen/ ones are
# rtree_small with rule files for the physical / clearance branches), then small QA boards
# with pads, zones, graphics, projects and rule files (the Rust replay reports those as pending
# until the parser and the model load them).
BOARDS=( "$HERE/../data/rtree/rtree_small.kicad_pcb"
         "$HERE/../data/rtree/rtree_ties.kicad_pcb"
         "$HERE/../data/rtree/rtree_4layer.kicad_pcb"
         "$HERE/../data/cachegen/phys_cond.kicad_pcb"
         "$HERE/../data/cachegen/phys_uncond.kicad_pcb"
         "$HERE/../data/cachegen/huge.kicad_pcb" )

for b in issue4139 fill_bad intersectingzones issue7267 issue18203 net_tie_hole_clearance \
         via_stacks issue14334 connection_width_rules issue24649 issue5750; do
    BOARDS+=( "$KICAD_SRC/qa/data/pcbnew/$b.kicad_pcb" )
done

ARGS=()

for b in "${BOARDS[@]}"; do
    base="${b%.kicad_pcb}"
    cp "$b" "$TMP/"

    for ext in kicad_pro kicad_dru; do
        if [ -f "$base.$ext" ]; then cp "$base.$ext" "$TMP/"; fi
    done

    ARGS+=( "$TMP/$(basename "$b")" )
done

"$ORACLE" --cachegen "${ARGS[@]}" 2>/dev/null > "$HERE/../golden/cachegen_oracle.txt"
wc -lc "$HERE/../golden/cachegen_oracle.txt"
