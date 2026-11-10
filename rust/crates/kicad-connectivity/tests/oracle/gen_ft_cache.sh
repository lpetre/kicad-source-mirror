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
# CN-05: regenerates tests/golden/ft_cache/<board>.txt with ft-cache-oracle
# (ft_cache_oracle.cpp, built by build_ft_oracle.sh) for the boards of ft_cache_boards.txt.
# Each board is copied (with its .kicad_pro / .kicad_dru) to a scratch directory first and
# run alone (one reference-tool process at a time).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD="$(cd "$HERE/../../../../.." && pwd)"
ORACLE="${FT_ORACLE:-/home/user/kicad-oracle/bin/ft-cache-oracle}"
GOLDEN="$HERE/../golden/ft_cache"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

mkdir -p "$GOLDEN"

grep -v '^#' "$HERE/ft_cache_boards.txt" | grep -v '^\s*$' | while read -r rel; do
    b="$(basename "$rel")"
    dir="$KICAD/$(dirname "$rel")"
    rm -rf "${SCRATCH:?}"/*
    cp "$dir/$b.kicad_pcb" "$SCRATCH/"
    for ext in kicad_pro kicad_dru; do
        [ -f "$dir/$b.$ext" ] && cp "$dir/$b.$ext" "$SCRATCH/"
    done
    ( cd "$SCRATCH" && timeout 600 "$ORACLE" "$b.kicad_pcb" 2>/dev/null ) > "$GOLDEN/$b.txt" \
        || echo "error|oracle" >> "$GOLDEN/$b.txt"
    echo "$b: $(grep -c '^ft|' "$GOLDEN/$b.txt" || true) pairs"
done
