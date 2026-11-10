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
# PCBIO-12 differential oracle (reference images, targets, points, grid items, drill maps,
# tables, barcodes, generated tables): every board tests/data/pcbio12/*.kicad_pcb and the small
# corpus boards listed below are copied alone into a fresh scratch directory and re-saved with
# the reference `kicad-cli pcb upgrade --force` to tests/golden/pcbio12/<name>.kicad_pcb (O1);
# O1 is re-saved once more to <name>_o2.kicad_pcb when that differs. Replayed by
# tests/pcbio12_misc_items.rs, which compares the top-level blocks of these item kinds.
#
# Usage: tests/oracle/pcbio12_golden.sh   (run from rust/crates/kicad-pcb)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
kicad=$(cd "$here/../../.." && pwd)/..
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

inputs=("$here"/data/pcbio12/*.kicad_pcb
        "$kicad"/qa/data/pcbnew/reference_images_load_save.kicad_pcb
        "$kicad"/qa/data/pcbnew/prettifier/group_and_image.kicad_pcb
        "$kicad"/qa/data/pcbnew/issue7567.kicad_pcb)

for f in "${inputs[@]}"; do
    n=$(basename "$f" .kicad_pcb)
    o1=$here/golden/pcbio12/$n.kicad_pcb
    resave "$f" "$o1" || { echo "oracle failed on $f" >&2; continue; }
    resave "$o1" "$scratch/o2.kicad_pcb"
    if cmp -s "$o1" "$scratch/o2.kicad_pcb"; then
        rm -f "$here/golden/pcbio12/${n}_o2.kicad_pcb"
    else
        cp "$scratch/o2.kicad_pcb" "$here/golden/pcbio12/${n}_o2.kicad_pcb"
    fi
done
