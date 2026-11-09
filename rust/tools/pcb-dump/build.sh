#!/bin/bash
# Builds pcb-dump against the reference KiCad build (see README.md).
#
# Compile flags are taken from compile_commands.json of a pcbnew_kiface_objects source; the link
# line is the one ninja uses for _pcbnew.kiface with -shared/-o/pcbnew.cpp.o replaced by our
# objects. Nothing in the C++ tree or the reference build is modified.
#
#   KICAD_SRC     C++ tree        (default: the tree containing this script)
#   ORACLE_BUILD  reference build (default /home/user/kicad-oracle/build)
#   OBJ_DIR       object dir      (default /home/user/kicad-oracle/build-pcb-dump)
#   OUT           binary          (default /home/user/kicad-oracle/bin/pcb-dump)
#   JOBS          parallel compiles (default 2); compiles run under nice -n 15
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="${KICAD_SRC:-$(cd "$HERE/../../.." && pwd)}"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
OBJ_DIR="${OBJ_DIR:-/home/user/kicad-oracle/build-pcb-dump}"
OUT="${OUT:-/home/user/kicad-oracle/bin/pcb-dump}"
JOBS="${JOBS:-2}"
mkdir -p "$OBJ_DIR" "$(dirname "$OUT")"

# ---- compile flags -------------------------------------------------------------------------
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
read -r -a CFLAGS < <(cat "$CFLAGS_FILE")
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$HERE" -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include"
          -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router" -I"$KICAD_SRC/pcbnew/tools"
          -Wno-shadow -Wno-unused-variable -Wno-sign-compare
          "-DQA_DATA_ROOT=\"$QA/data\"" "-DQA_PCBNEW_DATA_LOCATION=\"$QA/data/pcbnew\"" )

# Our sources plus the reused QA harness sources (compiled unchanged).
SRCS=( "$HERE"/*.cpp
       "$QA/mocks/kicad/common_mocks.cpp"
       "$QA/qa_utils/mock_utils.cpp"
       "$QA/qa_utils/stdstream_line_reader.cpp"
       "$QA/qa_utils/wx_utils/unit_test_utils.cpp"
       "$QA/pcbnew_utils/board_file_utils.cpp" )

OBJS=()
pids=()
for s in "${SRCS[@]}"; do
    o="$OBJ_DIR/$(echo "${s#$KICAD_SRC/}" | tr '/' '_').o"
    OBJS+=( "$o" )
    if [ ! -f "$o" ] || [ "$s" -nt "$o" ] || [ "$HERE/pcb_dump.h" -nt "$o" ] || [ "$0" -nt "$o" ]; then
        echo "CXX $s"
        nice -n 15 c++ "${CFLAGS[@]}" -c "$s" -o "$o" &
        pids+=( $! )
        if [ "${#pids[@]}" -ge "$JOBS" ]; then wait "${pids[0]}"; pids=( "${pids[@]:1}" ); fi
    fi
done
for p in "${pids[@]}"; do wait "$p"; done

# ---- link ----------------------------------------------------------------------------------
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
