//! Research prototype: port of KiCad's own `KIRTREE::PACKED_RTREE`
//! (`libs/kimath/include/geometry/rtree/packed_rtree.h` and the Hilbert helpers of
//! `rtree_node.h`) for `ELEMTYPE = int`, `FANOUT = 16`, `NUMDIMS = D` (2 or 3).
//!
//! The point of the prototype is to show that the *visit order* of `Search` (which KiCad's DRC
//! depends on) can be reproduced exactly, including the order of items whose Hilbert keys tie,
//! provided the Hilbert sort uses a port of libstdc++'s `std::sort`.

use crate::stdsort::std_sort;

pub const FANOUT: usize = 16;

/// Port of `KIRTREE::HilbertXY2D`.
pub fn hilbert_xy2d(order: i32, ax: u32, ay: u32) -> u64 {
    let mut d: u64 = 0;
    let mut x = ax;
    let mut y = ay;

    let mut s: u32 = 1u32 << (order - 1);
    while s > 0 {
        let rx: u32 = if (x & s) != 0 { 1 } else { 0 };
        let ry: u32 = if (y & s) != 0 { 1 } else { 0 };

        d = d.wrapping_add((s as u64).wrapping_mul(s as u64).wrapping_mul(((3 * rx) ^ ry) as u64));

        // Rotate quadrant
        if ry == 0 {
            if rx == 1 {
                // uint32_t arithmetic: `s * 2 - 1 - x` wraps for s == 2^31
                x = s.wrapping_mul(2).wrapping_sub(1).wrapping_sub(x);
                y = s.wrapping_mul(2).wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

/// Port of `KIRTREE::HilbertND2D<NUMDIMS>`.
pub fn hilbert_nd2d<const D: usize>(order: i32, coords: &[u32; D]) -> u64 {
    if D == 2 {
        hilbert_xy2d(order, coords[0], coords[1])
    } else {
        // For 3+ dimensions, fall back to Z-order (Morton) curve interleaving.
        let bits = order.min(64 / D as i32);
        let drop = order - bits;
        let mut d: u64 = 0;
        let mut bit = bits - 1;
        while bit >= 0 {
            let mut dim = D as i32 - 1;
            while dim >= 0 {
                d <<= 1;
                d |= ((coords[dim as usize] >> (bit + drop)) & 1) as u64;
                dim -= 1;
            }
            bit -= 1;
        }
        d
    }
}

#[derive(Debug, Default)]
pub struct PackedRTree<T, const D: usize> {
    /// SoA bounding boxes: `bounds[axis * 2 + side][globalNodeIdx * FANOUT + slot]`
    bounds: Vec<Vec<i32>>,
    counts: Vec<i32>,
    data: Vec<T>,
    level_offsets: Vec<usize>,
    size: usize,
}

struct Item<T, const D: usize> {
    data: T,
    min: [i32; D],
    max: [i32; D],
}

pub struct Builder<T, const D: usize> {
    items: Vec<Item<T, D>>,
}

impl<T: Clone, const D: usize> Default for Builder<T, D> {
    fn default() -> Self {
        Builder { items: Vec::new() }
    }
}

impl<T: Clone, const D: usize> Builder<T, D> {
    pub fn add(&mut self, min: [i32; D], max: [i32; D], data: T) {
        self.items.push(Item { data, min, max });
    }

    /// Port of `PACKED_RTREE::Builder::Build`.
    pub fn build(&mut self) -> PackedRTree<T, D> {
        let mut tree = PackedRTree {
            bounds: vec![Vec::new(); D * 2],
            counts: Vec::new(),
            data: Vec::new(),
            level_offsets: Vec::new(),
            size: 0,
        };
        let n = self.items.len();
        tree.size = n;

        if n == 0 {
            return tree;
        }

        // Compute global bounding box of all item centers for Hilbert normalization
        let mut global_min = [i32::MAX; D];
        let mut global_max = [i32::MIN; D];

        for item in &self.items {
            for d in 0..D {
                let center = item.min[d] / 2 + item.max[d] / 2;
                if center < global_min[d] {
                    global_min[d] = center;
                }
                if center > global_max[d] {
                    global_max[d] = center;
                }
            }
        }

        // Compute Hilbert indices for sorting
        let mut hilbert_values = vec![0u64; n];

        for i in 0..n {
            let mut coords = [0u32; D];
            for d in 0..D {
                let center = self.items[i].min[d] / 2 + self.items[i].max[d] / 2;
                let range = global_max[d] as f64 - global_min[d] as f64;
                if range > 0.0 {
                    let normalized = (center as f64 - global_min[d] as f64) / range;
                    coords[d] = (normalized * (u32::MAX as f64)) as u32;
                } else {
                    coords[d] = 0;
                }
            }
            hilbert_values[i] = hilbert_nd2d::<D>(32, &coords);
        }

        // Sort items by Hilbert value (std::sort: unstable, libstdc++ order for ties)
        let mut indices: Vec<usize> = (0..n).collect();
        std_sort(&mut indices, |&a, &b| hilbert_values[a] < hilbert_values[b]);

        // Store sorted data
        tree.data = indices.iter().map(|&i| self.items[i].data.clone()).collect();

        // Build leaf nodes: group every FANOUT items
        let num_leaf_nodes = (n + FANOUT - 1) / FANOUT;

        let mut nodes_per_level = vec![num_leaf_nodes];
        let mut level_nodes = num_leaf_nodes;
        while level_nodes > 1 {
            level_nodes = (level_nodes + FANOUT - 1) / FANOUT;
            nodes_per_level.push(level_nodes);
        }

        let total_nodes: usize = nodes_per_level.iter().sum();

        for axis in 0..D * 2 {
            tree.bounds[axis] = vec![i32::MAX; total_nodes * FANOUT];
        }
        tree.counts = vec![0; total_nodes];

        // Compute level offsets (leaf level = 0, root = highest)
        tree.level_offsets = vec![0; nodes_per_level.len()];
        let mut offset = 0;
        for lev in 0..nodes_per_level.len() {
            tree.level_offsets[lev] = offset;
            offset += nodes_per_level[lev];
        }

        // Fill leaf nodes with item bounding boxes
        for i in 0..n {
            let node_idx = i / FANOUT;
            let slot = i % FANOUT;
            let global_node_idx = tree.level_offsets[0] + node_idx;
            let item = &self.items[indices[i]];
            for d in 0..D {
                tree.bounds[d * 2][global_node_idx * FANOUT + slot] = item.min[d];
                tree.bounds[d * 2 + 1][global_node_idx * FANOUT + slot] = item.max[d];
            }
            tree.counts[global_node_idx] = slot as i32 + 1;
        }

        // Build internal levels bottom-up
        for lev in 1..nodes_per_level.len() {
            let child_level_offset = tree.level_offsets[lev - 1];
            let child_level_count = nodes_per_level[lev - 1];
            let parent_level_offset = tree.level_offsets[lev];

            for ci in 0..child_level_count {
                let parent_idx = ci / FANOUT;
                let slot = ci % FANOUT;
                let global_parent_idx = parent_level_offset + parent_idx;
                let global_child_idx = child_level_offset + ci;

                let mut child_min = [i32::MAX; D];
                let mut child_max = [i32::MIN; D];
                let child_count = tree.counts[global_child_idx] as usize;

                for s in 0..child_count {
                    for d in 0..D {
                        let mn = tree.bounds[d * 2][global_child_idx * FANOUT + s];
                        let mx = tree.bounds[d * 2 + 1][global_child_idx * FANOUT + s];
                        if mn < child_min[d] {
                            child_min[d] = mn;
                        }
                        if mx > child_max[d] {
                            child_max[d] = mx;
                        }
                    }
                }

                for d in 0..D {
                    tree.bounds[d * 2][global_parent_idx * FANOUT + slot] = child_min[d];
                    tree.bounds[d * 2 + 1][global_parent_idx * FANOUT + slot] = child_max[d];
                }
                tree.counts[global_parent_idx] = slot as i32 + 1;
            }
        }

        self.items.clear();
        tree
    }
}

impl<T, const D: usize> PackedRTree<T, D> {
    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Sequential access in packed (Hilbert) order, like `begin()`/`end()`.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// Port of `PACKED_RTREE::Search`.  The visitor returns `false` to stop early.
    pub fn search<F: FnMut(&T) -> bool>(&self, min: &[i32; D], max: &[i32; D], mut visitor: F) -> i32 {
        if self.size == 0 {
            return 0;
        }
        let mut found = 0;
        let root_level = self.level_offsets.len() - 1;
        self.search_node(root_level, self.level_offsets[root_level], min, max, &mut visitor, &mut found);
        found
    }

    /// Scalar equivalent of `OverlapMask2D` / `childOverlaps` (the SSE2/NEON paths compute the
    /// same closed-interval predicate).
    fn overlap_mask(&self, node_idx: usize, min: &[i32; D], max: &[i32; D]) -> u32 {
        let base = node_idx * FANOUT;
        let count = self.counts[node_idx] as usize;
        let mut mask = 0u32;
        for i in 0..count {
            let mut ok = true;
            for d in 0..D {
                if self.bounds[d * 2][base + i] > max[d] || self.bounds[d * 2 + 1][base + i] < min[d] {
                    ok = false;
                    break;
                }
            }
            if ok {
                mask |= 1 << i;
            }
        }
        mask
    }

    fn search_node<F: FnMut(&T) -> bool>(
        &self,
        level: usize,
        node_idx: usize,
        min: &[i32; D],
        max: &[i32; D],
        visitor: &mut F,
        found: &mut i32,
    ) -> bool {
        let mut mask = self.overlap_mask(node_idx, min, max);

        if level == 0 {
            let data_base = (node_idx - self.level_offsets[0]) * FANOUT;
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let data_idx = data_base + i;
                if data_idx < self.size {
                    *found += 1;
                    if !visitor(&self.data[data_idx]) {
                        return false;
                    }
                }
            }
        } else {
            let child_level_offset = self.level_offsets[level - 1];
            let node_offset = node_idx - self.level_offsets[level];
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let child_global_idx = child_level_offset + node_offset * FANOUT + i;
                if !self.search_node(level - 1, child_global_idx, min, max, visitor, found) {
                    return false;
                }
            }
        }
        true
    }
}
