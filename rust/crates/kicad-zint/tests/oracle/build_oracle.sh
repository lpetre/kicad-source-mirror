#!/bin/sh
# Builds the zint differential oracle (zint_oracle.c linked against the libzint.a of the reference
# KiCad build, i.e. the very zint 2.15.0 objects kicad-cli uses) and regenerates
# ../golden/zint_hashes.txt. Runs under the machine-wide C/C++ build lock (rust/PORTING.md 7).
#
# Usage: ./build_oracle.sh [--no-golden]
set -eu

here=$(cd "$(dirname "$0")" && pwd)
kicad=$(cd "$here/../../../../.." && pwd)
build=${KICAD_ORACLE_BUILD:-/home/user/kicad-oracle/build}
out=${ZINT_ORACLE_BUILD_DIR:-$kicad/rust/target/zint-oracle}

mkdir -p "$out"

flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    ${CC:-cc} -std=c99 -O2 -Wall \
    -I "$kicad/thirdparty/zint/backend" \
    "$here/zint_oracle.c" \
    "$build/thirdparty/zint/libzint.a" -lm \
    -o "$out/zint_oracle"

echo "built $out/zint_oracle"

if [ "${1:-}" != "--no-golden" ]; then
    "$out/zint_oracle" hashes > "$here/../golden/zint_hashes.txt"
    echo "wrote $here/../golden/zint_hashes.txt"
fi
