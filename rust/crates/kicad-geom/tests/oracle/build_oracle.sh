#!/bin/sh
# Builds the C++ differential-test oracle for kicad-geom from the original kimath sources and
# regenerates ../golden/geom_oracle.txt.  See geom_oracle.cpp.
#
# Usage: ./build_oracle.sh [--dump FILE]
#
# Requires a C++20 compiler (the committed golden file was produced with g++ 13 on x86-64
# Linux / glibc).  Floating point contraction is disabled so that no fused multiply-add is
# generated behind the source's back; the Rust port never contracts either.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
out=${GEOM_ORACLE_BUILD_DIR:-$kicad/rust/target/geom-oracle}

mkdir -p "$out"

${CXX:-g++} -std=c++20 -O2 -ffp-contract=off \
    -I "$here/stub" \
    -I "$kicad/libs/kimath/include" \
    -I "$kicad/libs/core/include" \
    -I "$kicad/include" \
    "$here/geom_oracle.cpp" \
    "$kicad/libs/kimath/src/math/util.cpp" \
    "$kicad/libs/kimath/src/geometry/seg.cpp" \
    "$kicad/libs/kimath/src/geometry/eda_angle.cpp" \
    "$kicad/libs/kimath/src/geometry/vector_utils.cpp" \
    "$kicad/libs/kimath/src/trigo.cpp" \
    -o "$out/geom_oracle"

"$out/geom_oracle" "$@" > "$here/../golden/geom_oracle.txt"
echo "wrote $here/../golden/geom_oracle.txt"
