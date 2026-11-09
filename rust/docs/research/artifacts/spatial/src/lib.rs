//! Scratch research crate (not part of the port): spatial indexing / triangulation
//! third-party equivalents.
pub mod stdsort;
pub mod rng;
pub mod delaunator_port;
pub mod packed_rtree;
pub mod rtree_cases;
pub mod checksum;
#[cfg(target_arch = "wasm32")]
pub mod wasm_exports;
pub mod nanoflann_port;
pub mod dynamic_rtree;
