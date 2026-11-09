#!/bin/sh
# Builds the C++ differential-test oracle for SHAPE_LINE_CHAIN (package G3) against the
# static libraries of the reference KiCad build and regenerates ../golden/slc_oracle.txt.
# See slc_oracle.cpp and rust/docs/ORACLE.md.
#
# Usage: ./build_slc_oracle.sh [--dump FILE]
#
# Requires the reference build (KICAD_ORACLE_BUILD, default /home/user/kicad-oracle/build) and
# the wxWidgets development files it was built with (wx-config).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
build=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
out=${GEOM_ORACLE_BUILD_DIR:-$kicad/rust/target/geom-oracle}

mkdir -p "$out"

# shellcheck disable=SC2046
${CXX:-g++} -std=c++20 -O2 -ffp-contract=off -DWXUSINGDLL -D__WXGTK__ -DUSINGZ \
    -I "$build" \
    -I "$kicad/libs/kimath/include" \
    -I "$kicad/libs/core/include" \
    -I "$kicad/include" \
    -I "$kicad/thirdparty/clipper2/Clipper2Lib/include" \
    -I "$kicad/thirdparty/rtree" \
    $(wx-config --cxxflags base) \
    "$here/slc_oracle.cpp" \
    "$build/libs/kimath/libkimath.a" \
    "$build/libs/core/libcore.a" \
    "$build/thirdparty/clipper2/libclipper2.a" \
    "$build/common/libkicommon.so" \
    $(wx-config --libs base) \
    -Wl,-rpath,"$build/common" \
    -o "$out/slc_oracle"

"$out/slc_oracle" "$@" > "$here/../golden/slc_oracle.txt"
echo "wrote $here/../golden/slc_oracle.txt"
