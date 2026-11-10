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
# Regenerates tests/golden/report_oracle.txt (DRC-11) with drc-report-oracle (built by
# build_report_oracle.sh): every scenario of tests/data/reports/*.scn, in name order. The
# scenarios only read boards from qa/data (parser only, nothing is written next to them).
#
#   gen_report_goldens.sh [ORACLE]     (default /home/user/kicad-oracle/bin/drc-report-oracle)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE="$(cd "$HERE/../.." && pwd)"
KICAD_SRC="$(cd "$CRATE/../../.." && pwd)"
ORACLE="${1:-/home/user/kicad-oracle/bin/drc-report-oracle}"

cd "$CRATE/tests/data/reports"
"$ORACLE" --root "$KICAD_SRC" $(ls *.scn | sort) 2>/dev/null > "$CRATE/tests/golden/report_oracle.txt"
wc -l "$CRATE/tests/golden/report_oracle.txt"
