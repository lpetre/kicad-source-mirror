#!/bin/sh
# Builds the C++ reference programs behind tests/golden/*.txt and, with "regen" as second
# argument, regenerates those golden files.
#
# Requirements: a C++20 compiler, wxWidgets 3.2 (wx-config) and Boost development files.
# The golden files in the repository were made with g++ 13, wxWidgets 3.2.4 and Boost 1.83 on
# x86-64 Linux.  Nothing but the golden files is written into the source tree: the build
# products go to the directory given as $1 (default: <kicad>/rust/target/core-oracle).
#
#   tests/oracle/build_oracle.sh [outdir] [regen]
#
# Programs:
#   core_oracle       common/lset.cpp, layer_id.cpp, lseq.cpp, kiid.cpp, lib_id.cpp
#                     -> tests/golden/core_oracle_dump.txt    (tests/test_cpp_oracle.rs)
#   legacy_oracle     remapLegacyLayerLSET, leg_layer2new, leg_mask2new (source text cut out
#                     of pcbnew files that cannot be compiled on their own)
#                     -> tests/golden/legacy_layers_dump.txt  (tests/test_legacy_layers.rs)
#   layer_ids_values  GUI layer enum values of include/layer_ids.h
#                     -> tests/golden/layer_ids_values.txt    (tests/test_layer_ids.rs)
#   typeinfo_values   KICAD_T values and classification helpers of include/core/typeinfo.h
#                     -> tests/golden/typeinfo_values.txt     (tests/test_typeinfo.rs)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/core-oracle}
GOLDEN=$HERE/../golden

mkdir -p "$OUT"

WX_CXX=$(wx-config --cxxflags base)
WX_LIBS=$(wx-config --libs base)
INC="-I$KICAD/include -I$KICAD/libs/core/include -I$KICAD/libs/kimath/include \
     -I$KICAD/thirdparty/dynamic_bitset -I$KICAD/thirdparty/libpopcnt \
     -I$KICAD/thirdparty/magic_enum/include/magic_enum -I$KICAD/thirdparty/magic_enum/include \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty/fmt/include"
# -DNDEBUG: release behaviour of assert() / wxASSERT; -DKICOMMON_DLL: KICOMMON_API = export.
CXX="${CXX:-c++} -std=c++20 -O0 -w -DNDEBUG -DKICOMMON_DLL -DFMT_HEADER_ONLY $WX_CXX $INC"

# 1. core_oracle
$CXX -o "$OUT/core_oracle" "$HERE/core_oracle.cpp" \
    "$KICAD/common/lset.cpp" "$KICAD/common/layer_id.cpp" "$KICAD/common/lseq.cpp" \
    "$KICAD/common/kiid.cpp" "$KICAD/common/lib_id.cpp" "$KICAD/common/exceptions.cpp" \
    "$KICAD/libs/core/utf8.cpp" $WX_LIBS

# 2. legacy_oracle: cut the functions under test out of their pcbnew translation units.
sed -n '/^enum LEGACY_PCB_LAYER_ID/,/^void PCB_PLOT_PARAMS_PARSER::Parse/p' \
    "$KICAD/pcbnew/pcb_plot_params.cpp" | sed '$d' > "$OUT/legacy_snip_plot_params.inc"

LEGACY=$KICAD/pcbnew/pcb_io/kicad_legacy/pcb_io_kicad_legacy.cpp
{
    sed -n '/^typedef uint32_t  *LEG_MASK/,/^#define FRONT_LAYERS/p' "$LEGACY"
    sed -n '/^inline bool is_leg_copperlayer_valid/,/^LSET PCB_IO_KICAD_LEGACY::leg_mask2new/p' "$LEGACY"
    sed -n '/^LSET PCB_IO_KICAD_LEGACY::leg_mask2new/,/^}/p' "$LEGACY" | sed 1d
} | sed 's/PCB_IO_KICAD_LEGACY:://' > "$OUT/legacy_snip_kicad_legacy.inc"

$CXX -I"$OUT" -o "$OUT/legacy_oracle" "$HERE/legacy_oracle.cpp" \
    "$KICAD/common/lset.cpp" "$KICAD/common/layer_id.cpp" "$KICAD/common/lseq.cpp" $WX_LIBS

# 3. enum value dumpers
$CXX -o "$OUT/layer_ids_values" "$HERE/layer_ids_values.cpp"
$CXX -o "$OUT/typeinfo_values" "$HERE/typeinfo_values.cpp"

echo "built core_oracle, legacy_oracle, layer_ids_values and typeinfo_values in $OUT"

if [ "${2:-}" = "regen" ]; then
    "$OUT/core_oracle" > "$GOLDEN/core_oracle_dump.txt"
    "$OUT/legacy_oracle" > "$GOLDEN/legacy_layers_dump.txt"
    "$OUT/layer_ids_values" > "$GOLDEN/layer_ids_values.txt"
    "$OUT/typeinfo_values" > "$GOLDEN/typeinfo_values.txt"
    echo "regenerated the golden files in $GOLDEN"
fi
