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
# Review package INT-2b (docs/design/review-2.md): regenerates tests/golden/int2b_review.txt
# from the reference pcb-dump (rust/docs/ORACLE.md, parser-only load, values as FNV-1a
# hashes) on scratch copies of corpus boards. Kept per board: the fields the review's fixes
# changed, so that the Rust replay (tests/int2b_review.rs, feature `dump`) reproduces each
# difference against the reference:
#   * R2-03 pad local solder mask / paste margins of both sides (PAD::SetLocalSolderMaskMargin,
#     SetLocalSolderPasteMargin, SetLocalSolderPasteMarginRatio) and what derives from them;
#   * R2-04 cells of a table inside a footprint (bakeTextBoxLib -> OnFootprintTransformed).
# One reference process at a time; no C++ compile.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
SCRATCH=$(mktemp -d)
OUT="$HERE/../golden/int2b_review.txt"
: > "$OUT"

run() {
    local board=$1 sections=$2 keep=$3   # $4: at most this many lines (keeps the golden small)
    cp "$KICAD/$board" "$SCRATCH/$(basename "$board")"
    echo "file|$board|sections=$sections" >> "$OUT"
    KICAD_ORACLE_HOME="$SCRATCH/home" nice -n 15 /home/user/kicad-oracle/bin/pcb-dump board \
        --no-init --hash --sections "$sections" "$SCRATCH/$(basename "$board")" 2> /dev/null \
        | grep -E "$keep" | head -n "$4" >> "$OUT"
    rm -f "$SCRATCH/$(basename "$board")"
}

PAD='^pad\|[^|]*\|(padstack\.(front|back)_outer\.solder_(mask|paste)_margin(_ratio)?|solder_mask_expansion\.|solder_paste_margin\.|shape\.[FB]\.(Mask|Paste))'
run qa/data/pcbnew/issue14650/KiCAD_Fiducial.kicad_pcb pad "$PAD" 100
run qa/data/pcbnew/ipc2581/round_unequal_drill.kicad_pcb pad "$PAD" 100
run qa/data/pcbnew/issue24474/issue24474.kicad_pcb pad "$PAD" 100
run qa/data/pcbnew/embedded_table_rotated_legacy_v10.kicad_pcb bbox,misc,text \
    '^(bbox\|[^|]*\|bbox=|misc\|[^|]*\|table\.|text\|[^|]*\|(draw_pos|text_box|textbox\.corners)=)' 200
rm -rf "$SCRATCH"
wc -l "$OUT"
