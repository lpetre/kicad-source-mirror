#!/bin/sh
# Builds the mirror oracle (package CLEAN-1, see mirror_oracle.cpp) against the static kimath
# library of the reference KiCad build and regenerates ../golden/mirror_oracle.txt.  The
# compile runs under the machine-wide C++ build lock (rust/PORTING.md section 7).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
build=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
out=${GEOM_ORACLE_BUILD_DIR:-$kicad/rust/target/geom-oracle}

mkdir -p "$out"

# shellcheck disable=SC2046
flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    ${CXX:-g++} -std=c++20 -O2 -ffp-contract=off -DWXUSINGDLL -D__WXGTK__ \
    -I "$build" \
    -I "$kicad/libs/kimath/include" \
    -I "$kicad/libs/core/include" \
    -I "$kicad/include" \
    -I "$kicad/thirdparty/clipper2/Clipper2Lib/include" \
    $(wx-config --cxxflags base) \
    "$here/mirror_oracle.cpp" \
    "$build/libs/kimath/libkimath.a" \
    "$build/libs/core/libcore.a" \
    "$build/thirdparty/clipper2/libclipper2.a" \
    "$build/common/libkicommon.so" \
    $(wx-config --libs base) \
    -Wl,-rpath,"$build/common" \
    -o "$out/mirror_oracle"

"$out/mirror_oracle" > "$here/../golden/mirror_oracle.txt" 2>/dev/null
echo "wrote $here/../golden/mirror_oracle.txt"
