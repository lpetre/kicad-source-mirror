#!/bin/sh
# Builds the C3 oracles (rust/docs/ORACLE.md) and, with "regen" as second argument, regenerates
# their golden files:
#
#   json_oracle.cpp     bundled nlohmann/json 3.11.3     -> tests/golden/c3_json.txt
#   pattern_oracle.cpp  libkicommon.so (eda_pattern_match.cpp, wxRegEx/PCRE2, common.cpp,
#                       json_settings.cpp, parameters.h)  -> tests/golden/c3_pattern.txt
#
#   tests/oracle/build_c3_oracle.sh [outdir] [regen]
#
# Replayed by tests/test_c3_json_oracle.rs and tests/test_c3_pattern_oracle.rs.  For debugging,
# "<outdir>/json_oracle <kicad root> <list> full" prints every line (same for pattern_oracle).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/core-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

${CXX:-c++} -std=c++20 -O2 -w -I"$KICAD/thirdparty/nlohmann_json" \
    -o "$OUT/json_oracle" "$HERE/json_oracle.cpp"
echo "built $OUT/json_oracle"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include"

if [ -f "$HERE/pattern_oracle.cpp" ]; then
    # shellcheck disable=SC2086
    ${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG -DFMT_HEADER_ONLY $(wx-config --cxxflags core,base) $INC \
        -o "$OUT/pattern_oracle" "$HERE/pattern_oracle.cpp" \
        -L"$BUILD/common" -lkicommon -Wl,-rpath,"$BUILD/common" $(wx-config --libs core,base)
    echo "built $OUT/pattern_oracle"
fi

# The JSON file list: every .kicad_pro, .kicad_prl, .json and .gbrjob file under qa/data and
# demos, relative to the source root, in byte order (the Rust test builds the same list).
(cd "$KICAD" && find qa/data demos -type f \( -name '*.kicad_pro' -o -name '*.kicad_prl' \
    -o -name '*.json' -o -name '*.gbrjob' \) | LC_ALL=C sort) > "$OUT/c3_json_files.txt"

if [ "${2:-}" = "regen" ]; then
    "$OUT/json_oracle" "$KICAD" "$OUT/c3_json_files.txt" > "$GOLDEN/c3_json.txt"
    echo "regenerated $GOLDEN/c3_json.txt"

    if [ -x "$OUT/pattern_oracle" ]; then
        "$OUT/pattern_oracle" "$KICAD" "$OUT/c3_json_files.txt" > "$GOLDEN/c3_pattern.txt"
        echo "regenerated $GOLDEN/c3_pattern.txt"
    fi
fi
