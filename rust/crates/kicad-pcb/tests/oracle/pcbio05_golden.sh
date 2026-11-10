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
# PCBIO-05 differential oracle (board text: gr_text, gr_text_box, effects, render cache).
#  1. Texts-only extracts of the corpus boards with outline fonts and render caches are written
#     to tests/data/pcbio05/extract_<stem>.kicad_pcb (header lists + top-level gr_text /
#     gr_text_box, loaded render caches removed; pcbio05_blocks.py extract).
#  2. Every tests/data/pcbio05/*.kicad_pcb (hand-made boards and the extracts) is copied alone
#     into a fresh scratch directory and re-saved with the reference
#     `kicad-cli pcb upgrade --force`; the result, with the glyph polygons of the regenerated
#     render caches removed (pcbio05_blocks.py strip), is tests/golden/pcbio05/<name>.kicad_pcb.
# Replayed by tests/pcbio05_text.rs (golden_text_blocks: Rust parse + format of the input, every
# top-level gr_text / gr_text_box byte for byte equal to the golden's).
#
# Usage: tests/oracle/pcbio05_golden.sh   (from anywhere)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../../../.." && pwd)
cli=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
py="python3 -I $here/oracle/pcbio05_blocks.py"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export KICAD_ORACLE_HOME=$scratch/home

for b in demos/royalblue54L_feather/RoyalBlue54L-Feather.kicad_pcb \
         demos/tiny_tapeout/tinytapeout-demo.kicad_pcb \
         demos/jetson-agx-thor-baseboard/jetson-agx-thor-baseboard.kicad_pcb \
         qa/data/pcbnew/issue14549.kicad_pcb \
         qa/data/pcbnew/issue22475/issue22475.kicad_pcb; do
    $py extract "$root/$b" > "$here/data/pcbio05/extract_$(basename "$b" .kicad_pcb).kicad_pcb"
done

mkdir -p "$here/golden/pcbio05"
for f in "$here"/data/pcbio05/*.kicad_pcb; do
    n=$(basename "$f" .kicad_pcb)
    d=$scratch/b
    rm -rf "$d"; mkdir -p "$d"
    cp "$f" "$d/board.kicad_pcb"
    timeout 300 nice -n 10 "$cli" pcb upgrade --force "$d/board.kicad_pcb" > "$d/log" 2>&1 || true
    if grep -q "Successfully saved" "$d/log"; then
        $py strip "$d/board.kicad_pcb" > "$here/golden/pcbio05/$n.kicad_pcb"
    else
        echo "oracle failed on $n:"; cat "$d/log"; exit 1
    fi
done
