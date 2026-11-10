#!/bin/bash
# Builds and runs the M6 (stackup / plot params) differential oracle against the reference
# KiCad build, writing tests/golden/m6_stackup_oracle.txt.
#
# Reuses the compile flags and the QA harness objects of rust/tools/pcb-dump/build.sh (run that
# first) and the link line of _pcbnew.kiface; nothing in the C++ tree or the reference build is
# modified. Objects and the binary go to $OBJ_DIR (default /home/user/kicad-oracle/build-m6).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
DUMP_OBJ="${DUMP_OBJ:-/home/user/kicad-oracle/build-pcb-dump}"
OBJ_DIR="${OBJ_DIR:-/home/user/kicad-oracle/build-m6}"
GOLDEN="$HERE/../golden/m6_stackup_oracle.txt"
mkdir -p "$OBJ_DIR"

eval "CFLAGS=( $(cat "$DUMP_OBJ/cflags.txt") )"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
QA="$KICAD_SRC/qa"
CFLAGS+=( -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include" -I"$KICAD_SRC/thirdparty/turtle"
          $(pkg-config --cflags gtk+-3.0) -Wno-shadow -Wno-unused-variable -Wno-sign-compare )

nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/m6_stackup_oracle.cpp" -o "$OBJ_DIR/m6_stackup_oracle.o"

# The pcb-dump link line with its own objects replaced by ours (the QA harness objects stay).
sed -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $OBJ_DIR/m6_oracle#" \
    -e "s#$DUMP_OBJ/rust_tools_pcb-dump_[a-z_]*\.cpp\.o##g" \
    -e "s#$DUMP_OBJ/qa_qa_utils_mock_utils.cpp.o#$DUMP_OBJ/rust_tools_pcb-dump_boost_test_impl.cpp.o $OBJ_DIR/m6_stackup_oracle.o &#" \
    "$DUMP_OBJ/link.sh" > "$OBJ_DIR/link.sh"
bash "$OBJ_DIR/link.sh"

export HOME="$OBJ_DIR/home"
mkdir -p "$HOME"
"$OBJ_DIR/m6_oracle" 2>/dev/null > "$GOLDEN"
echo "wrote $GOLDEN ($(wc -l < "$GOLDEN") lines)"
