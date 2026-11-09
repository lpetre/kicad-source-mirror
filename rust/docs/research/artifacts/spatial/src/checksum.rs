//! Checksums of the hand-port results, computed the same way natively and in WebAssembly, to
//! show that the ports are target independent.

use crate::delaunator_port::Delaunator;
use crate::packed_rtree;
use crate::rtree_cases;

fn fnv(h: &mut u64, v: u64) {
    for b in v.to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

/// FNV-1a over triangles / halfedges / hull links of every case in the embedded case file.
pub fn delaunay_checksum(cases_text: &str) -> u64 {
    let mut it = cases_text.split_ascii_whitespace();
    let ncases: usize = it.next().unwrap().parse().unwrap();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for _ in 0..ncases {
        let n: usize = it.next().unwrap().parse().unwrap();
        let coords: Vec<f64> = (0..2 * n).map(|_| it.next().unwrap().parse::<f64>().unwrap()).collect();
        match Delaunator::new(&coords) {
            Err(_) => fnv(&mut h, 0xdead),
            Ok(d) => {
                fnv(&mut h, d.triangles.len() as u64);
                for &t in &d.triangles {
                    fnv(&mut h, t as u64);
                }
                for &e in &d.halfedges {
                    // usize::MAX differs between 32 and 64 bit targets: normalise
                    fnv(&mut h, if e == usize::MAX { u64::MAX } else { e as u64 });
                }
                fnv(&mut h, d.hull_start as u64);
                for &e in &d.hull_next {
                    fnv(&mut h, e as u64);
                }
            }
        }
    }
    h
}

/// FNV-1a over the packed iteration order and the visit order of every query of every 2D case.
pub fn packed_rtree_checksum(seed: u64, max_items: usize) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for case in rtree_cases::generate(seed) {
        if case.dim != 2 || case.items.len() > max_items {
            continue;
        }
        let mut b = packed_rtree::Builder::<u32, 2>::default();
        for (i, it) in case.items.iter().enumerate() {
            b.add([it.min[0], it.min[1]], [it.max[0], it.max[1]], i as u32);
        }
        let tree = b.build();
        for &id in tree.iter() {
            fnv(&mut h, id as u64);
        }
        for q in &case.queries {
            let found = tree.search(&[q.min[0], q.min[1]], &[q.max[0], q.max[1]], |&id| {
                fnv(&mut h, id as u64);
                true
            });
            fnv(&mut h, found as u64);
        }
    }
    h
}

/// FNV-1a over knn(8) / radius results of the nanoflann port on integer grid points with many
/// ties (integer-only generator, so the inputs are the same on every target).
pub fn nanoflann_checksum(seed: u64) -> u64 {
    use crate::nanoflann_port::KdTree;
    use crate::rng::Rng;
    let mut rng = Rng::new(seed);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for _ in 0..300 {
        let n = rng.range(1, 600) as usize;
        let k = rng.range(1, 40);
        let step = [1i64, 1000, 250_000][rng.range(0, 2) as usize];
        let pts: Vec<[f64; 2]> =
            (0..n).map(|_| [(rng.range(-k, k) * step) as f64, (rng.range(-k, k) * step) as f64]).collect();
        let tree = KdTree::new(pts.clone());
        for _ in 0..40 {
            let q = pts[rng.range(0, n as i64 - 1) as usize];
            for (i, d) in tree.knn_search(&q, 8) {
                fnv(&mut h, i as u64);
                fnv(&mut h, d.to_bits());
            }
            let eps = (step * rng.range(1, 6)) as f64;
            for (i, d) in tree.radius_search_unsorted(&q, eps * eps) {
                fnv(&mut h, i as u64);
                fnv(&mut h, d.to_bits());
            }
        }
    }
    h
}

/// FNV-1a over the visit orders of the DYNAMIC_RTREE port (bulk load, insert, remove) for every
/// generated case with at most `max_items` items, 2D and 3D.
pub fn dynamic_rtree_checksum(seed: u64, max_items: usize) -> u64 {
    use crate::dynamic_rtree::{BulkEntry, DynamicRTree};
    fn run<const D: usize>(case: &rtree_cases::Case, h: &mut u64) {
        let arr = |v: &[i32; 3]| -> [i32; D] { std::array::from_fn(|d| v[d]) };
        let queries = |tree: &DynamicRTree<u32, D>, h: &mut u64| {
            for q in &case.queries {
                let found = tree.search(&arr(&q.min), &arr(&q.max), |&id| {
                    fnv(h, id as u64);
                    true
                });
                fnv(h, found as u64);
            }
        };
        let entries: Vec<BulkEntry<u32, D>> = case
            .items
            .iter()
            .enumerate()
            .map(|(i, b)| BulkEntry { min: arr(&b.min), max: arr(&b.max), data: i as u32 })
            .collect();
        let mut bulk = DynamicRTree::<u32, D>::default();
        bulk.bulk_load(&entries);
        queries(&bulk, h);
        let mut inc = DynamicRTree::<u32, D>::default();
        for e in &entries {
            inc.insert(&e.min, &e.max, e.data);
        }
        queries(&inc, h);
        for e in entries.iter().skip(1).step_by(2) {
            let ok = inc.remove(&e.min, &e.max, e.data);
            fnv(h, ok as u64);
        }
        queries(&inc, h);
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for case in rtree_cases::generate(seed) {
        if case.items.len() > max_items {
            continue;
        }
        if case.dim == 2 { run::<2>(&case, &mut h) } else { run::<3>(&case, &mut h) }
    }
    h
}
