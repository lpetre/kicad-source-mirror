#!/bin/sh
# Builds the C++ differential-test oracle for the spatial part of kicad-geom (std::sort,
# KIRTREE r-trees, delaunator, nanoflann) from the original KiCad sources and regenerates
# ../golden/spatial_oracle.txt.  See spatial_oracle.cpp.
#
# Usage: ./build_spatial_oracle.sh [--dump FILE]
#
# Requires g++ with libstdc++ (the reference platform; the committed golden file was produced
# with g++ 13 on x86-64 Linux).  The std::sort tie order is libstdc++-specific.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
out=${GEOM_ORACLE_BUILD_DIR:-$kicad/rust/target/geom-oracle}

mkdir -p "$out"

${CXX:-g++} -std=c++20 -O2 -ffp-contract=off \
    -I "$kicad/libs/kimath/include" \
    -I "$kicad/thirdparty/delaunator" \
    -I "$kicad/thirdparty/nanoflann" \
    "$here/spatial_oracle.cpp" \
    "$kicad/thirdparty/delaunator/delaunator.cpp" \
    -o "$out/spatial_oracle"

"$out/spatial_oracle" "$@" > "$here/../golden/spatial_oracle.txt"
echo "wrote $here/../golden/spatial_oracle.txt"
