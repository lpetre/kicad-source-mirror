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
# Builds drc-report-oracle (drc_report_oracle.cpp) against the reference KiCad build, the same
# way rust/tools/pcb-dump/build.sh builds pcb-dump: compile flags of a pcbnew_kiface_objects
# source from compile_commands.json, ninja's _pcbnew.kiface link line with -shared, -o and
# pcbnew.cpp.o replaced by our objects. Nothing in the C++ tree or the reference build changes.
#
#   ORACLE_BUILD  reference build (default /home/user/kicad-oracle/build)
#   OBJ_DIR       object dir      (default /home/user/kicad-oracle/build-drc-report)
#   OUT           binary          (default /home/user/kicad-oracle/bin/drc-report-oracle)
#   JOBS          parallel compiles (default 2); compiles run under nice -n 15
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
PCB_DUMP="$KICAD_SRC/rust/tools/pcb-dump"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
OBJ_DIR="${OBJ_DIR:-/home/user/kicad-oracle/build-drc-report}"
OUT="${OUT:-/home/user/kicad-oracle/bin/drc-report-oracle}"
JOBS="${JOBS:-2}"
mkdir -p "$OBJ_DIR" "$(dirname "$OUT")"

CFLAGS_FILE="$OBJ_DIR/cflags.txt"
python3 - "$ORACLE_BUILD" "$CFLAGS_FILE" <<'PY'
import json, shlex, sys
build, out = sys.argv[1], sys.argv[2]
for e in json.load(open(build + "/compile_commands.json")):
    if "pcbnew_kiface_objects.dir/board_loader.cpp.o" in e["output"]:
        args = shlex.split(e["command"])[1:]
        keep, skip = [], False
        for a in args:
            if skip: skip = False; continue
            if a in ("-o", "-c", "-include", "-x"): skip = True; continue
            if a.endswith(".cpp") or a.startswith("-Winvalid-pch") or a.startswith("-fvisibility"):
                continue
            keep.append(a)
        open(out, "w").write(" ".join(shlex.quote(a) for a in keep))
        break
else:
    sys.exit("board_loader.cpp.o not found in compile_commands.json")
PY
eval "CFLAGS=( $(cat "$CFLAGS_FILE") )"
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include"
          -I"$KICAD_SRC/thirdparty/turtle"
          $(pkg-config --cflags gtk+-3.0)
          -Wno-shadow -Wno-unused-variable -Wno-sign-compare
          "-DQA_DATA_ROOT=\"$QA/data\"" "-DQA_PCBNEW_DATA_LOCATION=\"$QA/data/pcbnew\"" )

# The driver, the header-only Boost.Test implementation of pcb-dump (read, not modified) and the
# reused QA harness sources (compiled unchanged).
SRCS=( "$HERE/drc_report_oracle.cpp"
       "$PCB_DUMP/boost_test_impl.cpp"
       "$QA/mocks/kicad/common_mocks.cpp"
       "$QA/qa_utils/mock_utils.cpp"
       "$QA/qa_utils/stdstream_line_reader.cpp"
       "$QA/qa_utils/wx_utils/unit_test_utils.cpp"
       "$QA/qa_utils/wx_utils/wx_assert.cpp"
       "$QA/pcbnew_utils/board_file_utils.cpp" )

OBJS=()
pids=()
for s in "${SRCS[@]}"; do
    o="$OBJ_DIR/$(echo "${s#$KICAD_SRC/}" | tr '/' '_').o"
    OBJS+=( "$o" )
    if [ ! -f "$o" ] || [ "$s" -nt "$o" ] || [ "$0" -nt "$o" ]; then
        echo "CXX $s"
        nice -n 15 c++ "${CFLAGS[@]}" -c "$s" -o "$o" &
        pids+=( $! )
        if [ "${#pids[@]}" -ge "$JOBS" ]; then wait "${pids[0]}"; pids=( "${pids[@]:1}" ); fi
    fi
done
for p in "${pids[@]}"; do wait "$p"; done

LINK_FILE="$OBJ_DIR/link.txt"
ninja -C "$ORACLE_BUILD" -t commands pcbnew_kiface | tail -1 > "$LINK_FILE"
python3 - "$LINK_FILE" "$ORACLE_BUILD" "$OUT" "${OBJS[@]}" > "$OBJ_DIR/link.sh" <<'PY'
import shlex, sys
link, build, out, objs = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
cmd = open(link).read().strip()
cmd = cmd.split("&&")[1]           # ": && c++ ... && :"
args = shlex.split(cmd)
res, skip = [], False
for a in args:
    if skip: skip = False; continue
    if a == "-o": skip = True; continue
    if a == "-shared" or a.endswith("pcbnew_kiface.dir/pcbnew.cpp.o"): continue
    if not a.startswith("-") and not a.startswith("/") : a = build + "/" + a
    res.append(a)
res = res[:1] + ["-o", out] + objs + res[1:]
print("cd " + shlex.quote(build) + " && nice -n 15 " + " ".join(shlex.quote(a) for a in res))
PY
echo "LINK $OUT"
bash "$OBJ_DIR/link.sh"
echo "built $OUT"
