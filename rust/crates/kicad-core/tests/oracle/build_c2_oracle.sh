#!/bin/sh
# Builds tests/oracle/units_strings_oracle.cpp against libkicommon.so of the reference build
# (see rust/docs/ORACLE.md) and, with "regen" as second argument, regenerates
# tests/golden/c2_units_strings.txt (replayed by tests/test_c2_oracle.rs).
#
#   tests/oracle/build_c2_oracle.sh [outdir] [regen]
#
# "full" output for debugging: <outdir>/units_strings_oracle full
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/core-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include"

# shellcheck disable=SC2086
${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG -DFMT_HEADER_ONLY $(wx-config --cxxflags core,base) $INC \
    -o "$OUT/units_strings_oracle" "$HERE/units_strings_oracle.cpp" \
    -L"$BUILD/common" -lkicommon -Wl,-rpath,"$BUILD/common" $(wx-config --libs core,base)

echo "built $OUT/units_strings_oracle"

if [ "${2:-}" = "regen" ]; then
    "$OUT/units_strings_oracle" > "$GOLDEN/c2_units_strings.txt"
    echo "regenerated $GOLDEN/c2_units_strings.txt"
fi
