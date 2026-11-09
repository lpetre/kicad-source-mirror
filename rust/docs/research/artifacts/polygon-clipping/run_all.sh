#!/usr/bin/env bash
# Reproduces the Clipper2 differential experiment (scratch research; run from this directory).
set -euo pipefail
K=/home/user/kicad-source-mirror/thirdparty/clipper2/Clipper2Lib
REG=$(ls -d ~/.cargo/registry/src/index.crates.io-*/ | head -1)
U=$REG/clipper2-sys-1.0.0/Clipper2/Tests/Polygons.txt      # upstream test data (cargo fetch clipper2-sys)
mkdir -p bin out data
# C++ references: KiCad bundled copy (USINGZ, like thirdparty/clipper2/CMakeLists.txt) and a
# variant with std::stable_sort for the intersection list (cpp/kicad_stable).
g++ -O2 -std=c++17 -DUSINGZ -I$K/include cpp/driver.cpp $K/src/clipper.engine.cpp \
    $K/src/clipper.offset.cpp $K/src/clipper.rectclip.cpp -o bin/drv_kicad
cargo build --release
cargo build --release -p wasmdrv --target wasm32-unknown-unknown
python3 -I gen_quirks.py data/quirks.txt
for seed in "${@:-20261109}"; do
  python3 -I gen_cases.py data/cases_$seed.txt $U $seed > /dev/null
  ./bin/drv_kicad data/cases_$seed.txt out/kicad_$seed.txt
  for v in clipper2_rs clipper2_rs_gnusort clipper2_rust; do
    ./target/release/drv_$v data/cases_$seed.txt out/${v}_$seed.txt
    echo "== seed $seed: $v vs KiCad C++"
    python3 -I compare.py data/cases_$seed.txt out/kicad_$seed.txt out/${v}_$seed.txt | tail -1
  done
  node run_wasm.mjs target/wasm32-unknown-unknown/release/wasmdrv.wasm 1 data/cases_$seed.txt out/wasm_$seed.txt
  cmp out/kicad_$seed.txt out/wasm_$seed.txt && echo "wasm32 (patched clipper2-rs) == KiCad C++: byte-identical"
done
