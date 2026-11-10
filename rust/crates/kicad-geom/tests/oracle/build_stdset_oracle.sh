#!/bin/sh
# Builds the std::set differential-test oracle (package CLEAN-1, see stdset_oracle.cpp) and
# regenerates ../golden/stdset_oracle.txt.  Only the C++ standard library (g++ / libstdc++,
# the reference platform) is needed.  The compile runs under the machine-wide C++ build lock
# (rust/PORTING.md section 7).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
out=${GEOM_ORACLE_BUILD_DIR:-$kicad/rust/target/geom-oracle}

mkdir -p "$out"

flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    ${CXX:-g++} -std=c++20 -O2 "$here/stdset_oracle.cpp" -o "$out/stdset_oracle"

"$out/stdset_oracle" > "$here/../golden/stdset_oracle.txt"
echo "wrote $here/../golden/stdset_oracle.txt"
