//! wasm32-unknown-unknown exports used by `node run_wasm.mjs` (scratch code, not part of the port).

static CASES: &str = include_str!("../data/delaunay_cases.txt");

#[unsafe(no_mangle)]
pub extern "C" fn delaunay_checksum() -> u64 {
    crate::checksum::delaunay_checksum(CASES)
}

#[unsafe(no_mangle)]
pub extern "C" fn packed_rtree_checksum() -> u64 {
    crate::checksum::packed_rtree_checksum(0x5254_5245_4500, 5000)
}

#[unsafe(no_mangle)]
pub extern "C" fn nanoflann_checksum() -> u64 {
    crate::checksum::nanoflann_checksum(0x4e41_4e4f)
}

#[unsafe(no_mangle)]
pub extern "C" fn dynamic_rtree_checksum() -> u64 {
    crate::checksum::dynamic_rtree_checksum(0x5254_5245_4500, 5000)
}
