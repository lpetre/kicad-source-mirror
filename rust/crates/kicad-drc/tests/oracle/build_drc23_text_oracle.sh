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
# Builds drc23-text-oracle (drc23_text_oracle.cpp) against libkicommon.so of the reference
# build (compile flags of common/common.cpp from compile_commands.json) and runs it on
# drc23_text_cases.txt, writing tests/golden/drc23/text_oracle.txt. The compile runs under the
# machine-wide C++ build lock (PORTING.md section 7).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
OUT="${OUT:-/home/user/kicad-oracle/bin/drc23-text-oracle}"
mkdir -p "$(dirname "$OUT")"

FLAGS="$(python3 - "$ORACLE_BUILD" <<'PY'
import json, shlex, sys
build = sys.argv[1]
for e in json.load(open(build + "/compile_commands.json")):
    if e["file"].endswith("/common/common.cpp"):
        args = shlex.split(e["command"])[1:]
        keep, skip = [], False
        for a in args:
            if skip: skip = False; continue
            if a in ("-o", "-c", "-include", "-x"): skip = True; continue
            if a.endswith(".cpp") or a.startswith("-Winvalid-pch") or a.startswith("-fvisibility") \
                    or a == "-DKICOMMON_DLL=1":
                continue
            keep.append(a)
        print(" ".join(shlex.quote(a) for a in keep))
        break
PY
)"

eval "CFLAGS=( $FLAGS )"

flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    c++ "${CFLAGS[@]}" "$HERE/drc23_text_oracle.cpp" -o "$OUT" \
    -L"$ORACLE_BUILD/common" -lkicommon -Wl,-rpath,"$ORACLE_BUILD/common" \
    $(wx-config --libs base)

LD_LIBRARY_PATH="$ORACLE_BUILD/common" "$OUT" < "$HERE/drc23_text_cases.txt" \
    > "$HERE/../golden/drc23/text_oracle.txt"
wc -l "$HERE/../golden/drc23/text_oracle.txt"
