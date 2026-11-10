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
# Writes tests/golden/drc29/oracle.txt: drc29-oracle (build_drc29_oracle.sh) on a copy of every
# board of tests/golden/drc29/corpus.txt (the shared DRC corpus), one process per board,
# sequentially. Per board: `board|<path relative to <kicad>>`, then the oracle's `v|` lines,
# `n|` or `error|` lines.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
ORACLE="${ORACLE:-/home/user/kicad-oracle/bin/drc29-oracle}"
OUT="$HERE/../golden/drc29/oracle.txt"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/drc29.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
export HOME="$SCRATCH/home"
mkdir -p "$HOME"

: > "$OUT.tmp"

while IFS= read -r rel; do
    src="$KICAD_SRC/$rel"
    dir="$SCRATCH/b"
    rm -rf "$dir"
    mkdir -p "$dir"
    base="${src%.kicad_pcb}"

    for ext in kicad_pcb kicad_pro kicad_dru kicad_prl; do
        [ -f "$base.$ext" ] && cp "$base.$ext" "$dir/"
    done

    echo "board|$rel" >> "$OUT.tmp"
    timeout 600 "$ORACLE" "$dir/$(basename "$src")" 2>/dev/null \
        | grep -E '^(v|n|error)\|' >> "$OUT.tmp" || echo "error|run" >> "$OUT.tmp"
done < <(cut -f1 "$HERE/../golden/drc29/corpus.txt" | sort -u)

mv "$OUT.tmp" "$OUT"
echo "$(wc -l < "$OUT") lines -> $OUT"
