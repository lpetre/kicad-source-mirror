#!/bin/sh
# Builds tests/oracle/plot_base_oracle.cpp against common/libcommon.a (plotter.cpp,
# drill_markers.cpp, gbr_metadata.cpp, GERBER_plotter.cpp) of the reference build (see
# rust/docs/ORACLE.md) and, with "regen" as second argument, regenerates
# tests/golden/plot_base.txt (replayed by tests/test_plot_base_oracle.rs).
#
#   tests/oracle/build_plot_base_oracle.sh [outdir] [regen]
#
# "full" output for debugging: <outdir>/plot_base_oracle full
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
    -o "$OUT/plot_base_oracle" "$HERE/plot_base_oracle.cpp" -Wl,--start-group $LIBS -Wl,--end-group)

echo "built $OUT/plot_base_oracle"

if [ "${2:-}" = "regen" ]; then
    "$OUT/plot_base_oracle" 2>/dev/null > "$GOLDEN/plot_base.txt"
    echo "regenerated $GOLDEN/plot_base.txt"
fi
