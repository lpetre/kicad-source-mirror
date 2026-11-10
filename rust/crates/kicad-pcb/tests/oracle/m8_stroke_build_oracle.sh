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
# Package M8: builds tests/oracle/m8_stroke_oracle.cpp against the reference build
# (rust/docs/ORACLE.md: compile flags of common/stroke_params.cpp, libcommon.a, libkicommon.so,
# kimath, core, wx) and regenerates tests/golden/m8_stroke.txt.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
ORACLE_BUILD=/home/user/kicad-oracle/build
OUT_DIR=$(mktemp -d)
FLAGS_FILE="$OUT_DIR/flags"

python3 - "$ORACLE_BUILD" "$FLAGS_FILE" <<'PY'
import json, shlex, sys
build, out = sys.argv[1], sys.argv[2]
for e in json.load(open(build + "/compile_commands.json")):
    if e["output"].endswith("common.dir/stroke_params.cpp.o"):
        args = shlex.split(e["command"])[1:]
        keep, skip = [], False
        for a in args:
            if skip: skip = False; continue
            if a in ("-o", "-c", "-include"): skip = True; continue
            if a.endswith(".cpp") or a.startswith("-Winvalid-pch") or a.startswith("-fvisibility"):
                continue
            keep.append(a)
        open(out, "w").write(" ".join(shlex.quote(a) for a in keep))
        break
else:
    sys.exit("stroke_params.cpp.o not found")
PY
eval "CFLAGS=( $(cat "$FLAGS_FILE") )"
CFLAGS+=( -w )

nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/m8_stroke_oracle.cpp" -o "$OUT_DIR/m8_stroke_oracle.o"
nice -n 15 c++ -o "$OUT_DIR/m8_stroke_oracle" "$OUT_DIR/m8_stroke_oracle.o" \
    "$ORACLE_BUILD/common/libcommon.a" "$ORACLE_BUILD/common/libkicommon.so" \
    "$ORACLE_BUILD/libs/kimath/libkimath.a" "$ORACLE_BUILD/libs/core/libcore.a" \
    "$ORACLE_BUILD/thirdparty/clipper2/libclipper2.a" "$ORACLE_BUILD/thirdparty/delaunator/libdelaunator.a" \
    -Wl,-rpath,"$ORACLE_BUILD/common" $(wx-config --libs core,base) -lz -lpthread

"$OUT_DIR/m8_stroke_oracle" > "$CRATE/tests/golden/m8_stroke.txt"
wc -l "$CRATE/tests/golden/m8_stroke.txt"
rm -rf "$OUT_DIR"
