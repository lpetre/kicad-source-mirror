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
# PCBIO-06 differential oracle (board file footprints), with the reference
# `kicad-cli pcb upgrade --force` of a board copied alone into a fresh scratch directory:
#  1. corpus boards of many format versions (list below) are reduced with pcbio06_strip.py
#     (footprints without pads / groups / constraints, at most one drawing and one zone each,
#     no top-level items besides the settings: the footprint child sort of package M20 is not
#     needed) into tests/data/pcbio06/strip_<name>.kicad_pcb;
#  2. those and the hand-made boards tests/data/pcbio06/handmade_*.kicad_pcb are re-saved to
#     tests/golden/pcbio06/<name>.kicad_pcb (O1), and O1 once more to <name>_o2.kicad_pcb when
#     that differs.
# Replayed by tests/pcbio06_footprints.rs.
#
# Usage: tests/oracle/pcbio06_golden.sh   (run from rust/crates/kicad-pcb)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
kicad=$(cd "$here/../../../.." && pwd)
cli=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export KICAD_ORACLE_HOME=$scratch/home

resave() {   # resave <in> <out>
    local d=$scratch/b
    rm -rf "$d"; mkdir -p "$d"
    cp "$1" "$d/board.kicad_pcb"
    timeout 300 nice -n 10 "$cli" pcb upgrade --force "$d/board.kicad_pcb" > "$d/log" 2>&1 \
        && grep -q "Successfully saved" "$d/log" && cp "$d/board.kicad_pcb" "$2"
}

corpus=(
    qa/data/pcbnew/issue4257.kicad_pcb
    qa/data/pcbnew/issue4139.kicad_pcb
    qa/data/pcbnew/issue5978.kicad_pcb
    qa/data/pcbnew/custom_fields.kicad_pcb
    qa/data/pcbnew/issue8629/issue8629.kicad_pcb
    qa/data/pcbnew/issue12109.kicad_pcb
    qa/data/pcbnew/issue9870.kicad_pcb
    qa/data/pcbnew/issue14412.kicad_pcb
    qa/data/pcbnew/issue18.kicad_pcb
    qa/data/pcbnew/footprints_load_save.kicad_pcb
    qa/data/pcbnew/component_classes.kicad_pcb
    qa/data/pcbnew/issue17429.kicad_pcb
    qa/data/pcbnew/stacked/stacked.kicad_pcb
    qa/data/pcbnew/issue22102.kicad_pcb
    qa/data/pcbnew/issue24799/issue24799.kicad_pcb
    qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb
    qa/data/pcbnew/api_kitchen_sink.kicad_pcb
    qa/data/pcbnew/net_tie_hole_clearance.kicad_pcb
    demos/microwave/microwave.kicad_pcb
    demos/ecc83/ecc83-pp_v2.kicad_pcb
    demos/pic_programmer/pic_programmer.kicad_pcb
)

mkdir -p "$here/data/pcbio06" "$here/golden/pcbio06"

for src in "${corpus[@]}"; do
    n=strip_$(basename "$src" .kicad_pcb)
    python3 -I "$here/oracle/pcbio06_strip.py" "$kicad/$src" "$here/data/pcbio06/$n.kicad_pcb" \
        --max-footprints 12
done

for f in "$here"/data/pcbio06/*.kicad_pcb; do
    n=$(basename "$f" .kicad_pcb)
    o1=$here/golden/pcbio06/$n.kicad_pcb
    resave "$f" "$o1"
    resave "$o1" "$scratch/o2.kicad_pcb"
    if cmp -s "$o1" "$scratch/o2.kicad_pcb"; then
        rm -f "$here/golden/pcbio06/${n}_o2.kicad_pcb"
    else
        cp "$scratch/o2.kicad_pcb" "$here/golden/pcbio06/${n}_o2.kicad_pcb"
    fi
done
