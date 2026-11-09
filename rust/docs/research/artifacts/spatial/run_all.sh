#!/bin/sh
# Reproduce the spatial-indexing / triangulation research experiments.
# Run from this directory:  sh run_all.sh
set -e
K=/home/user/kicad-source-mirror
mkdir -p data
g++ -std=c++20 -O2 -I $K/thirdparty/delaunator cpp/delaunator_driver.cpp $K/thirdparty/delaunator/delaunator.cpp -o cpp/delaunator_driver 2>/dev/null
g++ -std=c++20 -O2 -I $K/libs/kimath/include cpp/rtree_driver.cpp -o cpp/rtree_driver
g++ -std=c++20 -O2 -I $K/thirdparty/nanoflann cpp/nanoflann_driver.cpp -o cpp/nanoflann_driver
g++ -std=c++20 -O2 -I $K/thirdparty/dynamic_bitset -I $K/thirdparty/libpopcnt cpp/bitset_driver.cpp -o cpp/bitset_driver
g++ -std=c++20 -O2 cpp/sort_driver.cpp -o cpp/sort_driver
# the wasm exports embed data/delaunay_cases.txt, so generate it before building the library
[ -f data/delaunay_cases.txt ] || echo 0 > data/delaunay_cases.txt
cargo build --release
B=target/release

echo "== libstdc++ std::sort port"
$B/sort_cmp gen data/sort_cases.txt
cpp/sort_driver data/sort_cases.txt data/sort_cpp.out
$B/sort_cmp compare data/sort_cases.txt data/sort_cpp.out

echo "== delaunator"
$B/delaunay_cmp gen data/delaunay_cases.txt
cpp/delaunator_driver data/delaunay_cases.txt data/delaunay_cpp.out
$B/delaunay_cmp run-crate data/delaunay_cases.txt data/delaunay_crate.out
$B/delaunay_cmp run-port data/delaunay_cases.txt data/delaunay_port.out
cmp data/delaunay_cpp.out data/delaunay_port.out && echo "hand port: byte-identical to bundled C++"
echo "-- crate delaunator vs bundled C++"
$B/delaunay_cmp compare data/delaunay_cases.txt data/delaunay_cpp.out data/delaunay_crate.out
$B/delaunay_cmp mst-compare data/delaunay_cases.txt data/delaunay_cpp.out data/delaunay_crate.out

echo "== KIRTREE packed / dynamic r-trees"
$B/rtree_cmp gen data/rtree_cases.txt
cpp/rtree_driver data/rtree_cases.txt data/rtree_cpp.out
$B/rtree_cmp compare data/rtree_cases.txt data/rtree_cpp.out 2>/dev/null
$B/rstar_int 2>&1

echo "== nanoflann"
$B/knn_cmp gen data/knn_cases.txt
cpp/nanoflann_driver data/knn_cases.txt data/knn_cpp.out
$B/knn_cmp compare data/knn_cases.txt data/knn_cpp.out 2>/dev/null

echo "== dynamic_bitset"
$B/bitset_cmp gen data/bitset_script.txt
cpp/bitset_driver data/bitset_script.txt data/bitset_cpp.out
$B/bitset_cmp compare data/bitset_script.txt data/bitset_cpp.out

echo "== alternatives smoke tests"
$B/tri_alt
$B/pack_alt

echo "== wasm32-unknown-unknown"
cargo build --release --target wasm32-unknown-unknown
$B/checksums
node run_wasm.mjs target/wasm32-unknown-unknown/release/spatial_research.wasm
