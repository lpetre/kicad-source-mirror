#!/bin/sh
# Builds the CC-1 oracle (cc1_oracle.cpp, against libkicommon.so of the reference build) and,
# with "regen" as second argument, regenerates tests/golden/cc1_component_class.txt.
#
#   tests/oracle/build_cc1_oracle.sh [outdir] [regen]
#
# Replayed by tests/cc1_oracle.rs. Every C++ compile runs under the machine-wide lock
# (rust/PORTING.md section 7).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/cc1-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include -I$KICAD/thirdparty/dynamic_bitset -I$KICAD/common"

# shellcheck disable=SC2086
flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    ${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG -DFMT_HEADER_ONLY $(wx-config --cxxflags core,base) $INC \
    -o "$OUT/cc1_oracle" "$HERE/cc1_oracle.cpp" \
    -L"$BUILD/common" -lkicommon -Wl,-rpath,"$BUILD/common" $(wx-config --libs core,base)
echo "built $OUT/cc1_oracle"

if [ "${2:-}" = "regen" ]; then
    mkdir -p "$OUT/scratch"
    "$OUT/cc1_oracle" "$KICAD" "$HERE/cc1_cases.txt" "$OUT/scratch" > "$GOLDEN/cc1_component_class.txt"
    echo "regenerated $GOLDEN/cc1_component_class.txt"
fi
