#!/bin/bash
# Package SET-4 differential oracle: builds tests/oracle/bds_json_oracle.cpp against the
# reference build (real SETTINGS_MANAGER / PROJECT_FILE / BOARD_DESIGN_SETTINGS) and runs it on
#   - every .kicad_pro under qa/data and demos (copied to a scratch directory first), hashed:
#     tests/golden/bds_json_corpus.txt
#   - the hand-written scenarios tests/data/bds_json/*.kicad_pro, in full:
#     tests/golden/bds_json_scenarios.txt
# replayed by the Rust tests project::board_design_settings_json::tests::oracle_*.
#
# Reuses the compile flags, QA harness objects and link line of rust/tools/pcb-dump/build.sh
# (run that first). Usage: tests/oracle/bds_json_golden.sh [--full LABEL...]
#   --full LABEL...  print the unhashed lines of the given corpus labels (for diagnosis).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE="$(cd "$HERE/../.." && pwd)"
KICAD_SRC="$(cd "$HERE/../../../../.." && pwd)"
DUMP_OBJ="${DUMP_OBJ:-/home/user/kicad-oracle/build-pcb-dump}"
OBJ_DIR="${OBJ_DIR:-/home/user/kicad-oracle/build-bds-json}"
mkdir -p "$OBJ_DIR"

if [ ! -x "$OBJ_DIR/bds_json_oracle" ] || [ "$HERE/bds_json_oracle.cpp" -nt "$OBJ_DIR/bds_json_oracle" ]; then
    eval "CFLAGS=( $(cat "$DUMP_OBJ/cflags.txt") )"
    QA="$KICAD_SRC/qa"
    CFLAGS+=( -I"$QA/qa_utils/include" -I"$QA/pcbnew_utils/include" -I"$QA/mocks/include"
              -I"$KICAD_SRC/thirdparty/turtle" -I"$KICAD_SRC/pcbnew/router"
              $(pkg-config --cflags gtk+-3.0) -Wno-shadow -Wno-unused-variable -Wno-sign-compare )

    nice -n 15 c++ "${CFLAGS[@]}" -c "$HERE/bds_json_oracle.cpp" -o "$OBJ_DIR/bds_json_oracle.o"

    # The pcb-dump link line with its own objects replaced by ours (the QA harness objects stay).
    sed -e "s#-o /home/user/kicad-oracle/bin/pcb-dump#-o $OBJ_DIR/bds_json_oracle#" \
        -e "s#$DUMP_OBJ/rust_tools_pcb-dump_[a-z_]*\.cpp\.o##g" \
        -e "s#$DUMP_OBJ/qa_qa_utils_mock_utils.cpp.o#$DUMP_OBJ/rust_tools_pcb-dump_boost_test_impl.cpp.o $OBJ_DIR/bds_json_oracle.o &#" \
        "$DUMP_OBJ/link.sh" > "$OBJ_DIR/link.sh"
    bash "$OBJ_DIR/link.sh"
fi

export HOME="$OBJ_DIR/home"
mkdir -p "$HOME"
WORK="$OBJ_DIR/work"
rm -rf "$WORK"
mkdir -p "$WORK"

# Copies each project to its own directory; prints "<copy> <label>" argument pairs.
stage() {
    local n=0
    while IFS=$'\t' read -r label src; do
        n=$((n + 1))
        mkdir -p "$WORK/$n"
        cp "$src" "$WORK/$n/$(basename "$src")"
        printf '%s\n%s\n' "$WORK/$n/$(basename "$src")" "$label"
    done
}

FULL=()
if [ "${1:-}" = "--full" ]; then
    shift
    for l in "$@"; do FULL+=( "$l" ); done
fi

cd "$KICAD_SRC"
CORPUS_LIST="$OBJ_DIR/corpus.txt"
find qa/data demos -name '*.kicad_pro' | LC_ALL=C sort | awk '{ print $0 "\t" $0 }' > "$CORPUS_LIST"

if [ "${#FULL[@]}" -gt 0 ]; then
    printf '%s\n' "${FULL[@]}" | awk '{ print $0 "\t" $0 }' | stage > "$OBJ_DIR/args.txt"
    mapfile -t ARGS < "$OBJ_DIR/args.txt"
    "$OBJ_DIR/bds_json_oracle" --full "${ARGS[@]}" 2>/dev/null
    exit 0
fi

stage < "$CORPUS_LIST" > "$OBJ_DIR/args.txt"
mapfile -t ARGS < "$OBJ_DIR/args.txt"
"$OBJ_DIR/bds_json_oracle" "${ARGS[@]}" 2>/dev/null > "$CRATE/tests/golden/bds_json_corpus.txt"

rm -rf "$WORK"; mkdir -p "$WORK"
ls "$CRATE"/tests/data/bds_json/*.kicad_pro | LC_ALL=C sort \
    | while read -r f; do printf 'scenario/%s\t%s\n' "$(basename "$f")" "$f"; done | stage > "$OBJ_DIR/args.txt"
mapfile -t ARGS < "$OBJ_DIR/args.txt"
"$OBJ_DIR/bds_json_oracle" --full "${ARGS[@]}" 2>/dev/null > "$CRATE/tests/golden/bds_json_scenarios.txt"
rm -rf "$WORK"

wc -l "$CRATE/tests/golden/bds_json_corpus.txt" "$CRATE/tests/golden/bds_json_scenarios.txt"
