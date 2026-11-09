#!/bin/sh
# Builds tests/oracle/gerber_plotter_oracle.cpp against common/libcommon.a (plotter.cpp,
# drill_markers.cpp, gbr_metadata.cpp, GERBER_plotter.cpp) of the reference build (see
# rust/docs/ORACLE.md) and, with "regen" as second argument, regenerates
# tests/golden/gerber_plotter.txt (replayed by tests/test_gerber_plotter_oracle.rs).
#
#   tests/oracle/build_gerber_plotter_oracle.sh [outdir] [regen]
#
# "full" output for debugging: <outdir>/gerber_plotter_oracle full
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/plot-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include -I$KICAD/thirdparty/clipper2/Clipper2Lib/include -I$KICAD/thirdparty/dynamic_bitset"

# Link against everything kicad-cli links (libcommon.a pulls in much of the tree).
LIBS=$(cd "$BUILD" && grep -A3 "^build kicad/kicad-cli:" build.ninja | grep LINK_LIBRARIES | sed 's/^ *LINK_LIBRARIES = //')

# shellcheck disable=SC2086
(cd "$BUILD" && ${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG $(wx-config --cxxflags core,base) $INC \
    -o "$OUT/gerber_plotter_oracle" "$HERE/gerber_plotter_oracle.cpp" -Wl,--start-group $LIBS -Wl,--end-group)

echo "built $OUT/gerber_plotter_oracle"

if [ "${2:-}" = "regen" ]; then
    "$OUT/gerber_plotter_oracle" 2>/dev/null > "$GOLDEN/gerber_plotter.txt"
    echo "regenerated $GOLDEN/gerber_plotter.txt"
fi
