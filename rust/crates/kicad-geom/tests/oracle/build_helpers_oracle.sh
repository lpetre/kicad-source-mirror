#!/bin/sh
# Builds the C++ differential-test oracle for the kicad-geom helpers (package G6a: polygon-independent helpers) against the
# static libraries of the reference KiCad build and regenerates ../golden/helpers_oracle.txt.
# See helpers_oracle.cpp and rust/docs/ORACLE.md.
#
# Usage: ./build_helpers_oracle.sh [--dump FILE]
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
${CXX:-g++} -std=c++20 -O2 -ffp-contract=off -DWXUSINGDLL -D__WXGTK__ \
    -I "$build" \
    -I "$kicad/libs/kimath/include" \
    -I "$kicad/libs/core/include" \
    -I "$kicad/include" \
    -I "$kicad/thirdparty/clipper2/Clipper2Lib/include" \
    -I "$kicad/thirdparty/rtree" \
    $(wx-config --cxxflags base) \
    "$here/helpers_oracle.cpp" \
    "$build/libs/kimath/libkimath.a" \
    "$build/libs/core/libcore.a" \
    "$build/thirdparty/clipper2/libclipper2.a" \
    "$build/common/libkicommon.so" \
    $(wx-config --libs base) \
    -Wl,-rpath,"$build/common" \
    -o "$out/helpers_oracle"

"$out/helpers_oracle" "$@" > "$here/../golden/helpers_oracle.txt"
echo "wrote $here/../golden/helpers_oracle.txt"
