#!/bin/sh
# Builds the wxString::Matches oracle (package CLEAN-1, see wx_matches_oracle.cpp) against the
# wxWidgets base library of the reference build and regenerates
# ../golden/wx_matches_oracle.txt.  The compile runs under the machine-wide C++ build lock
# (rust/PORTING.md section 7).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
out=${CORE_ORACLE_BUILD_DIR:-$kicad/rust/target/core-oracle}

mkdir -p "$out"

# shellcheck disable=SC2046
flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    ${CXX:-g++} -std=c++20 -O2 $(wx-config --cxxflags base) "$here/wx_matches_oracle.cpp" \
    $(wx-config --libs base) -o "$out/wx_matches_oracle"

"$out/wx_matches_oracle" > "$here/../golden/wx_matches_oracle.txt"
echo "wrote $here/../golden/wx_matches_oracle.txt"
