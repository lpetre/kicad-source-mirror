// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/geometry/rtree/packed_rtree.h` (`KIRTREE::PACKED_RTREE`).
//!
//! The order in which [`PackedRTree::search`] reports items and the order of
//! [`PackedRTree::iter`] are part of the behaviour KiCad's DRC depends on (the first
//! violation / the first intersecting triangle wins).  Both follow from the Hilbert order
//! of the items, and items with equal Hilbert keys (same bounding box centre) are ordered by
//! libstdc++'s unstable `std::sort`, reproduced with [`crate::stdsort::std_sort`].

use crate::geometry::rtree::rtree_node::{hilbert_nd2d, overlap_mask};
use crate::stdsort::std_sort;

/// Port of `KIRTREE::PACKED_RTREE<DATATYPE, int, NUMDIMS, FANOUT>`: a static (immutable)
/// packed r-tree built via Hilbert-curve bulk loading.
///
/// Provides optimal query performance for build-once-query-many workloads such as DRC spatial
/// indexing.  The tree is constructed in O(N log N) time via Hilbert curve sorting and stored
/// in contiguous buffers with bottom-up level ordering.
///
/// `D` is the number of dimensions (2 or 3), `FANOUT` the maximum number of children per node
/// (16 everywhere in KiCad; at most 31).  Coordinates are `i32` (`ELEMTYPE = int`).
///
/// Build one with a [`PackedRTreeBuilder`].  The C++ move constructor / move assignment
/// (which leave the source empty) correspond to `std::mem::take`.
#[derive(Clone, Debug)]
pub struct PackedRTree<T, const D: usize = 2, const FANOUT: usize = 16> {
    /// Bounding boxes of all node slots: `bounds_min[globalNodeIdx * FANOUT + slot]`
    bounds_min: Vec<[i32; D]>,
    bounds_max: Vec<[i32; D]>,

    /// Number of valid children per node
    counts: Vec<i32>,

    /// Sorted data items (Hilbert-ordered)
    data: Vec<T>,

    /// Node index where each level starts (level 0 = leaves, last = root)
    level_offsets: Vec<usize>,

    /// Number of data items
    size: usize,
}

impl<T, const D: usize, const FANOUT: usize> Default for PackedRTree<T, D, FANOUT> {
    fn default() -> Self {
        PackedRTree {
            bounds_min: Vec::new(),
            bounds_max: Vec::new(),
            counts: Vec::new(),
            data: Vec::new(),
            level_offsets: Vec::new(),
            size: 0,
        }
    }
}

impl<T, const D: usize, const FANOUT: usize> PackedRTree<T, D, FANOUT> {
    /// `PACKED_RTREE()`: an empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// A new [`PackedRTreeBuilder`] for this tree type.
    pub fn builder() -> PackedRTreeBuilder<T, D, FANOUT> {
        PackedRTreeBuilder::new()
    }

    /// Port of `Search`: reports all items whose bounding boxes overlap the query box
    /// `[min, max]` (closed intervals) to `visitor`, in tree order.  The visitor returns
    /// `false` to stop early.
    ///
    /// Returns the number of items reported to the visitor.
    pub fn search<F>(&self, min: &[i32; D], max: &[i32; D], mut visitor: F) -> i32
    where
        F: FnMut(&T) -> bool,
    {
        if self.size == 0 {
            return 0;
        }

        let mut found = 0;
        let root_level = self.level_offsets.len() - 1;
        self.search_node(root_level, self.level_offsets[root_level], min, max, &mut visitor, &mut found);
        found
    }

    /// Port of `begin()` / `end()`: sequential access to all stored data items in packed
    /// (Hilbert) order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.data[..self.size].iter()
    }

    /// Port of `size()`.
    pub fn len(&self) -> usize {
        self.size
    }

    /// Port of `empty()`.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Port of `MemoryUsage`: approximate memory usage in bytes.
    pub fn memory_usage(&self) -> usize {
        // One vector of ELEMTYPE per axis and side in the C++
        let mut usage = (self.bounds_min.capacity() + self.bounds_max.capacity()) * D * std::mem::size_of::<i32>();

        usage += self.counts.capacity() * std::mem::size_of::<i32>();
        usage += self.data.capacity() * std::mem::size_of::<T>();
        usage += self.level_offsets.capacity() * std::mem::size_of::<usize>();
        usage
    }

    /// Port of `searchNode`.  Returns `true` if the search should continue, `false` if the
    /// visitor terminated early.
    ///
    /// The C++ has a SIMD mask path for 2D and a scalar per-child loop for other dimensions;
    /// both visit the overlapping slots in ascending order.
    fn search_node<F>(
        &self,
        level: usize,
        node_idx: usize,
        min: &[i32; D],
        max: &[i32; D],
        visitor: &mut F,
        found: &mut i32,
    ) -> bool
    where
        F: FnMut(&T) -> bool,
    {
        let base = node_idx * FANOUT;
        let mut mask = overlap_mask(
            &self.bounds_min[base..base + FANOUT],
            &self.bounds_max[base..base + FANOUT],
            self.counts[node_idx] as usize,
            min,
            max,
        );

        if level == 0 {
            let data_base = (node_idx - self.level_offsets[0]) * FANOUT;

            // Iterate set bits to visit only overlapping leaf children
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

            // Iterate set bits to recurse only into overlapping internal children
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

impl<'a, T, const D: usize, const FANOUT: usize> IntoIterator for &'a PackedRTree<T, D, FANOUT> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// `PACKED_RTREE::Builder::ITEM`
#[derive(Clone, Debug)]
struct Item<T, const D: usize> {
    data: T,
    min: [i32; D],
    max: [i32; D],
}

/// Port of `KIRTREE::PACKED_RTREE::Builder`: collects items and builds a [`PackedRTree`].
///
/// ```
/// use kicad_geom::geometry::rtree::PackedRTreeBuilder;
///
/// let mut builder = PackedRTreeBuilder::<u32>::new();
/// builder.reserve(2);
/// builder.add(&[0, 0], &[10, 10], 1);
/// builder.add(&[20, 20], &[30, 30], 2);
/// let tree = builder.build();
///
/// let mut hits = Vec::new();
/// tree.search(&[5, 5], &[6, 6], |&id| {
///     hits.push(id);
///     true
/// });
/// assert_eq!(hits, [1]);
/// ```
#[derive(Clone, Debug)]
pub struct PackedRTreeBuilder<T, const D: usize = 2, const FANOUT: usize = 16> {
    items: Vec<Item<T, D>>,
}

impl<T, const D: usize, const FANOUT: usize> Default for PackedRTreeBuilder<T, D, FANOUT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const D: usize, const FANOUT: usize> PackedRTreeBuilder<T, D, FANOUT> {
    /// `Builder()`
    pub fn new() -> Self {
        const {
            // OverlapMask2D: static_assert( FANOUT <= 31 ) (32-bit masks)
            assert!(FANOUT >= 2 && FANOUT <= 31, "the fanout must be in 2..=31");
            assert!(D >= 1, "at least one dimension is needed");
        }

        PackedRTreeBuilder { items: Vec::new() }
    }

    /// Port of `Reserve`.
    pub fn reserve(&mut self, count: usize) {
        self.items.reserve(count);
    }

    /// Port of `Add`: adds an item with the bounding box `[min, max]`.
    pub fn add(&mut self, min: &[i32; D], max: &[i32; D], data: T) {
        self.items.push(Item {
            data,
            min: *min,
            max: *max,
        });
    }

    /// Number of items added so far.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no items have been added.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Port of `Build`: builds the tree from the added items and leaves the builder empty.
    pub fn build(&mut self) -> PackedRTree<T, D, FANOUT> {
        let mut tree = PackedRTree::default();
        let items = std::mem::take(&mut self.items);
        let n = items.len();
        tree.size = n;

        if n == 0 {
            return tree;
        }

        // Compute global bounding box of all item centers for Hilbert normalization
        let mut global_min = [i32::MAX; D];
        let mut global_max = [i32::MIN; D];

        for item in &items {
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
                let center = items[i].min[d] / 2 + items[i].max[d] / 2;
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

        // Sort items by Hilbert value (std::sort: the order of equal keys is libstdc++'s)
        let mut indices: Vec<usize> = (0..n).collect();
        std_sort(&mut indices, |&a, &b| hilbert_values[a] < hilbert_values[b]);

        // Build leaf nodes: group every FANOUT items
        let num_leaf_nodes = n.div_ceil(FANOUT);

        // We'll build the tree bottom-up, tracking nodes per level
        let mut nodes_per_level = vec![num_leaf_nodes];
        let mut level_nodes = num_leaf_nodes;

        while level_nodes > 1 {
            level_nodes = level_nodes.div_ceil(FANOUT);
            nodes_per_level.push(level_nodes);
        }

        // If we had exactly one item, we have one leaf node and that's the root
        let total_nodes: usize = nodes_per_level.iter().sum();

        // Allocate bounds storage and counts
        tree.bounds_min = vec![[i32::MAX; D]; total_nodes * FANOUT];
        tree.bounds_max = vec![[i32::MAX; D]; total_nodes * FANOUT];
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

            let item = &items[indices[i]];

            tree.bounds_min[global_node_idx * FANOUT + slot] = item.min;
            tree.bounds_max[global_node_idx * FANOUT + slot] = item.max;
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

                // Compute the bounding box of the child node (union of all its slots)
                let mut child_min = [i32::MAX; D];
                let mut child_max = [i32::MIN; D];

                let child_count = tree.counts[global_child_idx] as usize;

                for s in 0..child_count {
                    for d in 0..D {
                        let mn = tree.bounds_min[global_child_idx * FANOUT + s][d];
                        let mx = tree.bounds_max[global_child_idx * FANOUT + s][d];

                        if mn < child_min[d] {
                            child_min[d] = mn;
                        }

                        if mx > child_max[d] {
                            child_max[d] = mx;
                        }
                    }
                }

                // Store this child node's bbox in the parent
                tree.bounds_min[global_parent_idx * FANOUT + slot] = child_min;
                tree.bounds_max[global_parent_idx * FANOUT + slot] = child_max;
                tree.counts[global_parent_idx] = slot as i32 + 1;
            }
        }

        // Store sorted data
        let mut slots: Vec<Option<T>> = items.into_iter().map(|item| Some(item.data)).collect();
        tree.data = Vec::with_capacity(n);

        for &idx in &indices {
            if let Some(data) = slots[idx].take() {
                tree.data.push(data);
            }
        }

        tree
    }
}
