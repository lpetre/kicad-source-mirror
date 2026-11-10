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
# PCBIO-07 differential oracle (board file pads and padstacks), with the reference
# `kicad-cli pcb upgrade --force` of a board copied alone into a fresh scratch directory:
#  1. corpus boards with pads of many format versions (list below) are reduced with
#     pcbio07_strip.py (footprints with their pads, at most 6 footprints) into
#     tests/data/pcbio07/strip_<name>.kicad_pcb;
#  2. those and the hand-made boards tests/data/pcbio07/handmade_*.kicad_pcb are re-saved to
#     tests/golden/pcbio07/<name>.kicad_pcb (O1), and O1 once more to <name>_o2.kicad_pcb when
#     that differs.
# Replayed by tests/pcbio07_pads.rs.
#
# Usage: tests/oracle/pcbio07_golden.sh   (run from rust/crates/kicad-pcb)
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
    qa/data/pcbnew/custom_pads.kicad_pcb
    qa/data/pcbnew/issue8629/issue8629.kicad_pcb
    qa/data/pcbnew/issue8883.kicad_pcb
    qa/data/pcbnew/issue1358.kicad_pcb
    qa/data/pcbnew/issue9081.kicad_pcb
    qa/data/pcbnew/issue12109.kicad_pcb
    qa/data/pcbnew/solder_mask_bridge_test.kicad_pcb
    qa/data/pcbnew/issue10916.kicad_pcb
    qa/data/pcbnew/padstacks.kicad_pcb
    qa/data/pcbnew/padstacks_complex.kicad_pcb
    qa/data/pcbnew/padstacks_normal.kicad_pcb
    qa/data/pcbnew/issue22751/issue22751.kicad_pcb
    qa/data/pcbnew/teardrop_custom_pad_anchor.kicad_pcb
    qa/data/pcbnew/stacked/stacked.kicad_pcb
    qa/data/pcbnew/issue23690_via_in_pad.kicad_pcb
    qa/data/pcbnew/net_chains/stub_length_pad_to_die.kicad_pcb
)

mkdir -p "$here/data/pcbio07" "$here/golden/pcbio07"

for src in "${corpus[@]}"; do
    n=strip_$(basename "$src" .kicad_pcb)
    python3 -I "$here/oracle/pcbio07_strip.py" "$kicad/$src" "$here/data/pcbio07/$n.kicad_pcb" \
        --max-footprints 6
done

for f in "$here"/data/pcbio07/*.kicad_pcb; do
    n=$(basename "$f" .kicad_pcb)
    o1=$here/golden/pcbio07/$n.kicad_pcb
    if ! resave "$f" "$o1"; then
        echo "oracle failed on $n" >&2
        cat "$scratch/b/log" >&2
        continue
    fi
    resave "$o1" "$scratch/o2.kicad_pcb"
    if cmp -s "$o1" "$scratch/o2.kicad_pcb"; then
        rm -f "$here/golden/pcbio07/${n}_o2.kicad_pcb"
    else
        cp "$scratch/o2.kicad_pcb" "$here/golden/pcbio07/${n}_o2.kicad_pcb"
    fi
done
