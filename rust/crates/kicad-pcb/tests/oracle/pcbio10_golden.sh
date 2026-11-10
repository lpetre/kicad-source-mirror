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
# PCBIO-10 differential oracle (board file dimensions).
#  1. Dimensions-only extracts of every corpus board (qa/data/pcbnew, demos; under 10 MB) with
#     top-level (dimension ..) lists, from the ORIGINAL files (so every format version the
#     corpus holds is read), are written to tests/data/pcbio10/extract_<stem>.kicad_pcb
#     (header lists + top-level dimensions, render caches removed; pcbio10_blocks.py extract).
#  2. Every tests/data/pcbio10/*.kicad_pcb (the hand-made boards dims_v5 / dims_v6 / dims_v7 /
#     dims_current covering the legacy pre-20200610 form, the 6.0 order with gr_text before the
#     style, bare locked, every type and format / style token, and the extracts) is copied
#     alone into a fresh scratch directory and re-saved with the reference
#     `kicad-cli pcb upgrade --force`; the result, with the glyph polygons of regenerated render
#     caches removed (pcbio10_blocks.py strip), is tests/golden/pcbio10/<name>.kicad_pcb.
# Replayed by tests/pcbio10_dimensions.rs (golden_dimension_blocks: Rust parse + format of the
# input, every top-level dimension byte for byte equal to the golden's).
#
# Usage: tests/oracle/pcbio10_golden.sh   (from anywhere; one oracle process at a time)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../../../.." && pwd)
cli=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
py="python3 -I $here/oracle/pcbio10_blocks.py"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export KICAD_ORACLE_HOME=$scratch/home

rm -f "$here"/data/pcbio10/extract_*.kicad_pcb
while IFS= read -r b; do
    [ "$(stat -c %s "$b")" -lt 10000000 ] || continue
    [ "$($py count "$b")" -gt 0 ] || continue
    stem=$(basename "$b" .kicad_pcb)
    $py extract "$b" > "$here/data/pcbio10/extract_$stem.kicad_pcb"
done < <(grep -rl --include='*.kicad_pcb' '(dimension' "$root/qa/data/pcbnew" "$root/demos" | sort)

mkdir -p "$here/golden/pcbio10"
rm -f "$here"/golden/pcbio10/*.kicad_pcb
for f in "$here"/data/pcbio10/*.kicad_pcb; do
    n=$(basename "$f" .kicad_pcb)
    d=$scratch/b
    rm -rf "$d"; mkdir -p "$d"
    cp "$f" "$d/board.kicad_pcb"
    timeout 300 nice -n 10 "$cli" pcb upgrade --force "$d/board.kicad_pcb" > "$d/log" 2>&1 || true
    if grep -q "Successfully saved" "$d/log"; then
        $py strip "$d/board.kicad_pcb" > "$here/golden/pcbio10/$n.kicad_pcb"
    else
        echo "oracle failed on $n:"; cat "$d/log"; exit 1
    fi
done
