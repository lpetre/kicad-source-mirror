#!/bin/sh
# Builds tests/oracle/font_oracle.cpp against libkigal.so / libkicommon.so of the reference build
# (the pen size helpers of common/gr_text.cpp are compiled from the C++ source) (see rust/docs/ORACLE.md) and, with "regen" as
# second argument, regenerates tests/golden/font_oracle.txt (replayed by
# tests/test_font_oracle.rs).
#
#   tests/oracle/build_oracle.sh [outdir] [regen]
#
# Debugging: <outdir>/font_oracle full [KIND IDX]   (KIND is M, T, D or H)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/font-oracle}
BUILD=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"
# The pure helpers of common/gr_text.cpp (GetPenSizeFor* .. ClampTextPenSize), verbatim; the
# rest of the file needs the wxDC printing code of libcommon.a.
{
    echo '#include <gr_text.h>'
    echo '#include <wx/gdicmn.h>'
    sed -n '/^int GetPenSizeForBold( int aTextSize )/,/^int GRTextWidth/p' "$KICAD/common/gr_text.cpp" | sed '$d'
} > "$OUT/gr_text_helpers.cpp"

INC="-I$BUILD -I$BUILD/common -I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include -I$KICAD/thirdparty/pegtl \
     -I$KICAD/thirdparty/glm -I$KICAD/thirdparty/expected/include -I$KICAD/thirdparty/clipper2/Clipper2Lib/include"

# shellcheck disable=SC2086
${CXX:-c++} -std=c++20 -O1 -w -DNDEBUG -DFMT_HEADER_ONLY $(wx-config --cxxflags core,base) $INC \
    -o "$OUT/font_oracle" "$HERE/font_oracle.cpp" "$OUT/gr_text_helpers.cpp" \
    -L"$BUILD/common/gal" -L"$BUILD/common" -lkigal -lkicommon \
    "$BUILD/libs/kimath/libkimath.a" "$BUILD/libs/core/libcore.a" \
    -Wl,-rpath,"$BUILD/common/gal:$BUILD/common:$BUILD/api" \
    $(wx-config --libs core,base)

echo "built $OUT/font_oracle"

if [ "${2:-}" = "regen" ]; then
    "$OUT/font_oracle" golden > "$GOLDEN/font_oracle.txt"
    echo "regenerated $GOLDEN/font_oracle.txt"
fi
