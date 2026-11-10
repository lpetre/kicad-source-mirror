#!/bin/sh
# Builds tests/oracle/ds_oracle.cpp against the kicad-cli link libraries of the reference
# build (the drawing sheet code and the plotters live in common/libcommon.a, see
# rust/docs/ORACLE.md) and, with "regen" as second argument, regenerates
# tests/golden/drawing_sheet.txt (replayed by tests/test_drawing_sheet_oracle.rs) from the
# committed case list tests/oracle/ds_cases.txt.
#
#   tests/oracle/build_ds_oracle.sh [outdir] [regen]
#
# Debugging: <outdir>/ds_oracle <kicad root> <cases> full   prints every plotted file.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/plot-oracle}
OUT=${OUT:-$KICAD/rust/target/plot-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include -I$KICAD/thirdparty/clipper2/Clipper2Lib/include -I$KICAD/thirdparty/dynamic_bitset \
     -I/usr/include/freetype2 -I/usr/include/harfbuzz"

# Link against everything kicad-cli links (libcommon.a pulls in much of the tree).
LIBS=$(cd "$BUILD" && grep -A3 "^build kicad/kicad-cli:" build.ninja | grep LINK_LIBRARIES | sed 's/^ *LINK_LIBRARIES = //')

# shellcheck disable=SC2086
(cd "$BUILD" && flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 ${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG $(wx-config --cxxflags core,base) $INC \
    -o "$OUT/ds_oracle" "$HERE/ds_oracle.cpp" -Wl,--start-group $LIBS -Wl,--end-group)

echo "built $OUT/ds_oracle"

if [ "${2:-}" = "regen" ]; then
    "$OUT/ds_oracle" "$KICAD" "$HERE/ds_cases.txt" 2>/dev/null > "$GOLDEN/drawing_sheet.txt"
    echo "regenerated $GOLDEN/drawing_sheet.txt"
fi
