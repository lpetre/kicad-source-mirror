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
# Records the reference `kicad-cli pcb drc` reports of tests/golden/reports (DRC-11, compared by
# the #[ignore]d test report_oracle.rs::reference_cli_reports once the engine exists). Every
# board is copied with its .kicad_pro / .kicad_dru to a scratch directory first (ORACLE.md).
# tests/golden/reports/cases.txt is the exact list of command lines.
#
#   gen_cli_reports.sh [KICAD_CLI]     (default /home/user/kicad-oracle/bin/kicad-cli)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE="$(cd "$HERE/../.." && pwd)"
KICAD_SRC="$(cd "$CRATE/../../.." && pwd)"
CLI="${1:-/home/user/kicad-oracle/bin/kicad-cli}"
OUT="$CRATE/tests/golden/reports"
WORK="$(mktemp -d)"
export KICAD_ORACLE_HOME="$WORK/home"
mkdir -p "$KICAD_ORACLE_HOME"

trap 'rm -rf "$WORK"' EXIT

# The cases (board, options) are listed in tests/golden/reports/cases.txt.
mapfile -t CASES < <(grep -v '^#' "$OUT/cases.txt" | grep -v '^$')

for c in "${CASES[@]}"; do
    read -r board opts <<< "$c"
    mkdir -p "$WORK/$board"

    for ext in kicad_pcb kicad_pro kicad_dru; do
        src="$KICAD_SRC/qa/data/pcbnew/$board.$ext"
        if [ -f "$src" ]; then cp "$src" "$WORK/$board/"; fi
    done

    for fmt in report json; do
        ext=$([ "$fmt" = report ] && echo rpt || echo json)
        # shellcheck disable=SC2086
        ( cd "$WORK" && "$CLI" pcb drc --format "$fmt" $opts -o "$OUT/$board.$ext" "$WORK/$board/$board.kicad_pcb" ) \
            2>&1 | grep -v "schema file" || true
        # The creation date differs per run.
        sed -i -E 's/^\*\* Created on .* \*\*$/** Created on <DATE> **/; s/^    "date": ".*",$/    "date": "<DATE>",/' "$OUT/$board.$ext"
    done
done

du -cb "$OUT"/*.rpt "$OUT"/*.json | tail -1
