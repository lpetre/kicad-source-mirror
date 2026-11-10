#!/bin/bash
# Builds the FAB-02 Excellon oracle (excellon_oracle.cpp) against the reference KiCad build
# (docs/ORACLE.md) and regenerates tests/golden/excellon_oracle.txt (about 1 minute).
# Same structure as build_oracle.sh (FAB-01); the QA harness objects are shared with it when
# they exist.
#
# Same approach as rust/tools/pcb-dump/build.sh: compile flags from compile_commands.json (the
# entry of pcbnew_kiface_objects/board_loader.cpp), link line of _pcbnew.kiface with -shared,
# -o and pcbnew.cpp.o replaced by our objects. Nothing in the C++ tree or the reference build
# is modified. The corpus boards are copied to a scratch directory before loading.
#
#   ORACLE_BUILD  reference build (default /home/user/kicad-oracle/build)
#   OBJ_DIR       object dir      (default /home/user/kicad-oracle/build-fab-drill)
#   JOBS          parallel compiles (default 2); compiles run under nice -n 15
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
OBJ_DIR="${OBJ_DIR:-/home/user/kicad-oracle/build-fab-excellon}"
OUT="$OBJ_DIR/excellon-oracle"
FAB01_OBJ="/home/user/kicad-oracle/build-fab-drill"
JOBS="${JOBS:-2}"
PCB_DUMP="$KICAD_SRC/rust/tools/pcb-dump"
mkdir -p "$OBJ_DIR"

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
          -I"$KICAD_SRC/pcbnew" -I"$KICAD_SRC/thirdparty/turtle" $(pkg-config --cflags gtk+-3.0)
          -Wno-shadow -Wno-unused-variable -Wno-sign-compare
          "-DQA_DATA_ROOT=\"$QA/data\"" "-DQA_PCBNEW_DATA_LOCATION=\"$QA/data/pcbnew\"" )

# Our driver plus the QA harness sources (compiled unchanged; boost_test_impl.cpp of pcb-dump
# provides the header-only Boost.Test implementation the harness needs).
SRCS=( "$HERE/excellon_oracle.cpp"
       "$QA/mocks/kicad/common_mocks.cpp"
       "$QA/qa_utils/mock_utils.cpp"
       "$QA/qa_utils/wx_utils/unit_test_utils.cpp"
       "$QA/qa_utils/wx_utils/wx_assert.cpp"
       "$PCB_DUMP/boost_test_impl.cpp" )

OBJS=()
pids=()
for s in "${SRCS[@]}"; do
    o="$OBJ_DIR/$(basename "$s").o"
    OBJS+=( "$o" )
    shared="$FAB01_OBJ/$(basename "$s").o"
    if [ ! -f "$o" ] && [ "$s" != "$HERE/excellon_oracle.cpp" ] && [ -f "$shared" ] && [ "$shared" -nt "$s" ]; then
        cp "$shared" "$o"
    fi
    if [ ! -f "$o" ] || [ "$s" -nt "$o" ]; then
        echo "CXX $s"
        nice -n 15 c++ "${CFLAGS[@]}" -I"$PCB_DUMP" -c "$s" -o "$o" &
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
cmd = cmd.split("&&")[1]
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

# The corpus: the boards of tests/golden/drill/generate.sh, copied to a scratch directory.
SCRATCH="$OBJ_DIR/boards"
rm -rf "$SCRATCH"
mkdir -p "$SCRATCH"
BOARDS=( $(sed -n '/^BOARDS=(/,/^)/p' "$HERE/../golden/drill/generate.sh" | grep kicad_pcb) )
ARGS=()
for b in "${BOARDS[@]}"; do
    stem="$(basename "$b" .kicad_pcb)"
    mkdir -p "$SCRATCH/$stem"
    cp "$KICAD_SRC/$b" "$SCRATCH/$stem/"
    pro="$KICAD_SRC/${b%.kicad_pcb}.kicad_pro"
    [ -f "$pro" ] && cp "$pro" "$SCRATCH/$stem/"
    ARGS+=( "$SCRATCH/$stem/$stem.kicad_pcb" )
done

WORK="$OBJ_DIR/work"
rm -rf "$WORK"
mkdir -p "$WORK"
HOME="$OBJ_DIR/home" "$OUT" "$WORK" "${ARGS[@]}" > "$HERE/../golden/excellon_oracle.txt"
rm -rf "$WORK"
wc -lc "$HERE/../golden/excellon_oracle.txt"
