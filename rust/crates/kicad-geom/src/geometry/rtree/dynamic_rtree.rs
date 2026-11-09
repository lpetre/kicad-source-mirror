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

//! Port of `libs/kimath/include/geometry/rtree/dynamic_rtree.h` (`KIRTREE::DYNAMIC_RTREE`).
//!
//! The order in which [`DynamicRTree::search`] reports items depends on the exact shape of the
//! tree, which depends on every tie-break of the R*-tree heuristics.  All `std::sort` calls go
//! through [`crate::stdsort`] (their comparators leave ties) and the `std::priority_queue` of
//! the nearest-neighbour search is [`crate::stdsort::PriorityQueue`].
//!
//! 64-bit area / overlap / distance arithmetic wraps on overflow: the C++ overflows `int64_t`
//! there for boxes spanning most of the `int` range (undefined behaviour, a wrapping
//! instruction on the reference platform).  Likewise the `double` to `int64_t` conversion of
//! the forced-reinsert distance reproduces the x86-64 `cvttsd2si` result for out-of-range
//! values.
//!
//! Data items are moved, never cloned, while the tree reorganises itself: the C++ copies its
//! `DATATYPE` (a pointer in every KiCad use) through temporary entry arrays; the port moves
//! the items into a side table and sorts plain-data entries that refer to it.  Because the
//! libstdc++ sort permutes its range as a function of the comparison results only, this gives
//! the same arrangement.

use crate::geometry::rtree::rtree_node::{NO_NODE, NodeArena, NodeId, RtreeNode, hilbert_nd2d};
use crate::stdsort::{PriorityQueue, std_sort};

/// The x86-64 `cvttsd2si` conversion behind `static_cast<int64_t>( double )`: truncation,
/// with the "integer indefinite" value `INT64_MIN` for NaN and out-of-range inputs (undefined
/// behaviour in C++).
fn cvttsd2si(x: f64) -> i64 {
    if (-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&x) {
        x as i64
    } else {
        i64::MIN
    }
}

/// Port of `DYNAMIC_RTREE::BULK_ENTRY`: entry type for [`DynamicRTree::bulk_load`].
#[derive(Clone, Debug, PartialEq)]
pub struct BulkEntry<T, const D: usize = 2> {
    pub min: [i32; D],
    pub max: [i32; D],
    pub data: T,
}

/// Port of `DYNAMIC_RTREE::SPLIT_ENTRY` (and of the `ENTRY` of `forcedReinsert`, which adds
/// `distSq`).  `data` indexes the side table of moved-out data items.
#[derive(Clone, Copy, Debug)]
struct SplitEntry<const D: usize> {
    min: [i32; D],
    max: [i32; D],
    insert_min: [i32; D],
    insert_max: [i32; D],
    data: usize,
    child: NodeId,
    dist_sq: i64,
}

impl<const D: usize> SplitEntry<D> {
    /// Value-initialised entry (`std::vector<ENTRY>( n )`).
    fn zeroed() -> Self {
        SplitEntry {
            min: [0; D],
            max: [0; D],
            insert_min: [0; D],
            insert_max: [0; D],
            data: usize::MAX,
            child: NO_NODE,
            dist_sq: 0,
        }
    }
}

/// Port of `KIRTREE::DYNAMIC_RTREE<DATATYPE, int, NUMDIMS, TMAXNODES>`: a dynamic R*-tree with
/// stored insertion bounding boxes.
///
/// Supports O(log N) insert, remove, and query operations.  Uses R*-tree split and forced
/// reinsert heuristics for optimal tree quality.
///
/// `D` is the number of dimensions (2 or 3 in KiCad), `M` the maximum number of children per
/// node (16 everywhere in KiCad, at most 31).  Coordinates are `i32` (`ELEMTYPE = int`).
///
/// ```
/// use kicad_geom::geometry::rtree::DynamicRTree;
///
/// let mut tree = DynamicRTree::<u32>::new();
/// tree.insert(&[0, 0], &[10, 10], 1);
/// tree.insert(&[20, 20], &[30, 30], 2);
///
/// let mut hits = Vec::new();
/// tree.search(&[5, 5], &[25, 25], |&id| {
///     hits.push(id);
///     true
/// });
/// assert_eq!(hits.len(), 2);
/// assert!(tree.remove(&[0, 0], &[10, 10], &1));
/// assert_eq!(tree.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct DynamicRTree<T, const D: usize = 2, const M: usize = 16> {
    root: NodeId,
    count: usize,
    allocator: NodeArena<T, D, M>,
}

impl<T, const D: usize, const M: usize> Default for DynamicRTree<T, D, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const D: usize, const M: usize> DynamicRTree<T, D, M> {
    /// `MAXNODES`
    pub const MAXNODES: usize = M;

    /// `MINNODES`
    pub const MINNODES: usize = RtreeNode::<T, D, M>::MINNODES;

    /// `REINSERT_COUNT`: fraction of entries to reinsert on overflow (30% per R*-tree paper)
    pub const REINSERT_COUNT: usize = M * 3 / 10;

    /// `DYNAMIC_RTREE()`: an empty tree.  The C++ move constructor / move assignment (which
    /// leave the source empty) correspond to `std::mem::take`.
    pub fn new() -> Self {
        const {
            assert!(M >= 2 && M <= 31, "the fanout must be in 2..=31");
            assert!(D >= 1, "at least one dimension is needed");
        }

        DynamicRTree {
            root: NO_NODE,
            count: 0,
            allocator: NodeArena::default(),
        }
    }

    fn node(&self, id: NodeId) -> &RtreeNode<T, D, M> {
        self.allocator.node(id)
    }

    fn node_mut(&mut self, id: NodeId) -> &mut RtreeNode<T, D, M> {
        self.allocator.node_mut(id)
    }

    /// Port of `Insert`: inserts an item with the given bounding box.
    pub fn insert(&mut self, min: &[i32; D], max: &[i32; D], data: T) {
        if self.root == NO_NODE {
            self.root = self.alloc_node();
            let root = self.root;
            self.node_mut(root).level = 0;
        }

        // Bitmask tracking which levels have had forced reinsert this insertion.
        // 32 bits handles tree depth up to 31, which covers > 16^31 items.
        let mut reinserted_levels: u32 = 0;
        self.insert_impl(min, max, data, &mut reinserted_levels);
        self.count += 1;
    }

    /// Port of `Search`: reports every item whose bounding box overlaps the query box
    /// `[min, max]` (closed intervals) to `visitor`, in tree order.  The visitor returns
    /// `false` to stop early.
    ///
    /// Returns the number of items reported to the visitor.
    pub fn search<F>(&self, min: &[i32; D], max: &[i32; D], mut visitor: F) -> i32
    where
        F: FnMut(&T) -> bool,
    {
        let mut found = 0;

        if self.root != NO_NODE {
            self.search_impl(self.root, min, max, &mut visitor, &mut found);
        }

        found
    }

    /// Port of `RemoveAll`: removes all items from the tree.
    pub fn remove_all(&mut self) {
        let root = self.root;
        self.remove_all_nodes(root);
        self.root = NO_NODE;
        self.count = 0;
    }

    /// Port of `BulkLoad`: builds the tree from a batch of entries using Hilbert-curve packed
    /// bulk loading.
    ///
    /// This is dramatically faster than inserting items individually because it avoids the
    /// R*-tree forced-reinsertion cascades.  Items are sorted by Hilbert curve index of their
    /// bounding box centers, then packed bottom-up into nodes.
    ///
    /// Any existing tree contents are discarded.
    pub fn bulk_load(&mut self, entries: Vec<BulkEntry<T, D>>) {
        if entries.is_empty() {
            self.remove_all();
            return;
        }

        // Find global bounds for Hilbert normalization
        let mut global_min = [i32::MAX; D];
        let mut global_max = [i32::MIN; D];

        for entry in &entries {
            for d in 0..D {
                if entry.min[d] < global_min[d] {
                    global_min[d] = entry.min[d];
                }

                if entry.max[d] > global_max[d] {
                    global_max[d] = entry.max[d];
                }
            }
        }

        // Sort entries by Hilbert index of bbox center
        let mut range = [0.0f64; D];

        for d in 0..D {
            range[d] = global_max[d] as f64 - global_min[d] as f64;

            if range[d] <= 0.0 {
                range[d] = 1.0;
            }
        }

        // Derive each curve index once.  A comparator that derives them re-derives both
        // operands on every comparison, which for three axes costs more than the load
        let n = entries.len();
        let mut order: Vec<(u64, usize)> = Vec::with_capacity(n);

        for (i, entry) in entries.iter().enumerate() {
            let mut coords = [0u32; D];

            for d in 0..D {
                let center = (entry.min[d] as f64 + entry.max[d] as f64) / 2.0;
                coords[d] =
                    (((center - global_min[d] as f64) / range[d]) * (u32::MAX as f64)) as u32;
            }

            order.push((hilbert_nd2d::<D>(32, &coords), i));
        }

        // std::sort on pairs: a total order (the indices are distinct)
        std_sort(&mut order, |a, b| a < b);

        // Drop the old tree only after the last large allocation, so a caller that runs out
        // of memory keeps its index instead of an empty tree that reports a size
        let root = self.root;
        self.remove_all_nodes(root);
        self.root = NO_NODE;
        self.count = n;

        let mut items: Vec<Option<BulkEntry<T, D>>> = entries.into_iter().map(Some).collect();

        // Pack entries into leaf nodes
        let mut current_level: Vec<NodeId> = Vec::with_capacity(n.div_ceil(M));
        let mut i = 0;

        while i < n {
            let leaf = self.alloc_node();
            self.node_mut(leaf).level = 0;
            let cnt = M.min(n - i);

            for j in 0..cnt {
                if let Some(entry) = items[order[i + j].1].take() {
                    let min = entry.min;
                    let max = entry.max;
                    self.node_mut(leaf)
                        .push_leaf_entry(&min, &max, &min, &max, entry.data);
                }
            }

            current_level.push(leaf);
            i += M;
        }

        // Build internal levels bottom-up
        let mut level = 1;

        while current_level.len() > 1 {
            let level_size = current_level.len();
            let mut next_level: Vec<NodeId> = Vec::with_capacity(level_size.div_ceil(M));
            let mut i = 0;

            while i < level_size {
                let internal = self.alloc_node();
                self.node_mut(internal).level = level;
                let cnt = M.min(level_size - i);

                for j in 0..cnt {
                    let child = current_level[i + j];
                    let (child_min, child_max) = self.node(child).compute_enclosing_bounds();
                    self.node_mut(internal)
                        .push_child_entry(&child_min, &child_max, child);
                }

                next_level.push(internal);
                i += M;
            }

            current_level = next_level;
            level += 1;
        }

        self.root = current_level[0];
    }

    /// Port of `size()`.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Port of `empty()`.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Port of `MemoryUsage`: approximate memory usage in bytes.
    pub fn memory_usage(&self) -> usize {
        self.allocator.memory_usage()
    }

    /// Port of `begin()` / `end()` (`DYNAMIC_RTREE::Iterator`): all data items, depth first.
    pub fn iter(&self) -> Iter<'_, T, D, M> {
        Iter::new(self, None)
    }

    /// Port of `Overlapping` (`SearchRange` / `SearchIterator`): a lazy iterator over the
    /// items whose bounding boxes overlap the query box.  More efficient than
    /// [`search`](Self::search) when callers may break early or only need to check
    /// emptiness.
    ///
    /// Note that it visits the children of a node in slot order (like the C++ iterator), which
    /// is the same order as [`search`](Self::search).
    pub fn overlapping(&self, min: &[i32; D], max: &[i32; D]) -> Iter<'_, T, D, M> {
        Iter::new(self, Some((*min, *max)))
    }

    /// Port of `NearestNeighbors`: nearest-neighbour search using Hjaltason & Samet's
    /// algorithm.
    ///
    /// Returns at most `k` `(squared_distance, data)` pairs, sorted by distance (between the
    /// query point and the item's bounding box; equal distances in the libstdc++
    /// `std::priority_queue` order).
    pub fn nearest_neighbors(&self, point: &[i32; D], k: i32) -> Vec<(i64, &T)> {
        let mut results: Vec<(i64, &T)> = Vec::new();

        if self.root == NO_NODE || k <= 0 {
            return results;
        }

        // Min-heap: smallest distance first
        let mut pq =
            PriorityQueue::new(|a: &(i64, (NodeId, usize)), b: &(i64, (NodeId, usize))| a.0 > b.0);

        // Seed with root's children
        let root = self.node(self.root);

        for i in 0..root.count {
            let dist = Self::min_dist_sq(root, i, point);
            pq.push((dist, (self.root, i)));
        }

        while (results.len() as i32) < k {
            let Some((dist, (node_id, slot))) = pq.pop() else {
                break;
            };

            let node = self.node(node_id);

            if node.is_leaf() {
                results.push((dist, &node.data[slot]));
            } else {
                let child_id = node.children[slot];
                let child = self.node(child_id);

                for i in 0..child.count {
                    let child_dist = Self::min_dist_sq(child, i, point);
                    pq.push((child_dist, (child_id, i)));
                }
            }
        }

        results
    }

    /// Port of `allocNode`.
    fn alloc_node(&mut self) -> NodeId {
        self.allocator.allocate()
    }

    /// Port of `freeNode`.
    fn free_node(&mut self, node: NodeId) {
        self.allocator.free(node);
    }

    /// Port of `removeAllNodes`.
    fn remove_all_nodes(&mut self, node: NodeId) {
        if node == NO_NODE {
            return;
        }

        if self.node(node).is_internal() {
            for i in 0..self.node(node).count {
                let child = self.node(node).children[i];
                self.remove_all_nodes(child);
            }
        }

        self.free_node(node);
    }

    /// Port of `insertImpl`: core insertion with forced reinsert tracking.
    fn insert_impl(
        &mut self,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        reinserted_levels: &mut u32,
    ) {
        // ChooseSubtree to find the leaf
        let mut path: Vec<NodeId> = Vec::new();
        let leaf = self.choose_subtree(self.root, min, max, &mut path);

        // Insert into leaf
        if !self.node(leaf).is_full() {
            self.node_mut(leaf)
                .push_leaf_entry(min, max, min, max, data);
            self.adjust_path(&path, leaf);
        } else {
            self.overflow_treatment(leaf, min, max, data, &mut path, reinserted_levels);
        }
    }

    /// Port of `chooseSubtree`: R*-tree ChooseSubtree.
    ///
    /// At leaf level (level==1): minimize overlap increase.
    /// At higher levels: minimize area increase, tie-break by smallest area.
    fn choose_subtree(
        &self,
        start: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        path: &mut Vec<NodeId>,
    ) -> NodeId {
        path.clear();
        let mut node_id = start;

        while self.node(node_id).is_internal() {
            path.push(node_id);
            let node = self.node(node_id);

            if node.level == 1 {
                // At the level just above leaves: minimize overlap increase
                let mut best_idx = 0;
                let mut best_overlap_inc = i64::MAX;
                let mut best_area_inc = i64::MAX;
                let mut best_area = i64::MAX;

                for i in 0..node.count {
                    let overlap_before = Self::compute_overlap(node, i);
                    let overlap_after = Self::compute_overlap_enlarged(node, i, min, max);
                    let overlap_inc = overlap_after.wrapping_sub(overlap_before);
                    let area_inc = node.child_enlargement(i, min, max);
                    let area = node.child_area(i);

                    if overlap_inc < best_overlap_inc
                        || (overlap_inc == best_overlap_inc && area_inc < best_area_inc)
                        || (overlap_inc == best_overlap_inc
                            && area_inc == best_area_inc
                            && area < best_area)
                    {
                        best_idx = i;
                        best_overlap_inc = overlap_inc;
                        best_area_inc = area_inc;
                        best_area = area;
                    }
                }

                node_id = node.children[best_idx];
            } else {
                // Higher levels: minimize area increase, tie-break by smallest area
                node_id = node.children[Self::least_enlargement(node, min, max)];
            }
        }

        node_id
    }

    /// The "minimize area increase, tie-break by smallest area" loop shared by
    /// `chooseSubtree` and `chooseSubtreeAtLevel`.
    fn least_enlargement(node: &RtreeNode<T, D, M>, min: &[i32; D], max: &[i32; D]) -> usize {
        let mut best_idx = 0;
        let mut best_area_inc = i64::MAX;
        let mut best_area = i64::MAX;

        for i in 0..node.count {
            let area_inc = node.child_enlargement(i, min, max);
            let area = node.child_area(i);

            if area_inc < best_area_inc || (area_inc == best_area_inc && area < best_area) {
                best_idx = i;
                best_area_inc = area_inc;
                best_area = area;
            }
        }

        best_idx
    }

    /// Port of `overflowTreatment`: handle overflow by forced reinsert or split.
    fn overflow_treatment(
        &mut self,
        node: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<NodeId>,
        reinserted_levels: &mut u32,
    ) {
        let level = self.node(node).level;

        // Guard against UB from shifting by >= 32. At fanout 16 this would
        // require > 16^32 items, but protect the generic template regardless.
        if level >= 32 {
            self.split_node(node, min, max, data, path, reinserted_levels);
            return;
        }

        let level_mask: u32 = 1u32 << level;

        if (*reinserted_levels & level_mask) == 0 {
            *reinserted_levels |= level_mask;
            self.forced_reinsert(node, min, max, data, path, reinserted_levels);
        } else {
            self.split_node(node, min, max, data, path, reinserted_levels);
        }
    }

    /// Collects the entries of `node` plus one overflow entry (the C++ `entries` arrays of
    /// `forcedReinsert` and `splitNode`).  Leaf data is moved into `pool`; the node keeps its
    /// bounds, so the caller must refill it.
    fn gather_entries(
        &mut self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        data: Option<T>,
        pool: &mut Vec<Option<T>>,
    ) -> Vec<SplitEntry<D>> {
        let node = self.node_mut(node_id);
        let count = node.count;
        let mut entries = vec![SplitEntry::<D>::zeroed(); count + 1];
        let mut node_data = std::mem::take(&mut node.data).into_iter();

        for (i, entry) in entries.iter_mut().enumerate().take(count) {
            (entry.min, entry.max) = node.get_child_bounds(i);

            if node.is_leaf() {
                (entry.insert_min, entry.insert_max) = node.get_insert_bounds(i);
                entry.data = pool.len();
                pool.push(node_data.next());
                entry.child = NO_NODE;
            } else {
                entry.child = node.children[i];
            }
        }

        // The overflow entry
        let last = &mut entries[count];
        last.min = *min;
        last.max = *max;
        last.insert_min = *min;
        last.insert_max = *max;
        last.data = pool.len();
        pool.push(data);
        last.child = NO_NODE;

        entries
    }

    /// Refills `node_id` (count reset to 0 first) with `entries`, like the distribution loops
    /// of `forcedReinsert` and `splitNode`: `leaf` is the `IsLeaf()` of the node being split.
    fn fill_node(
        &mut self,
        node_id: NodeId,
        entries: &[SplitEntry<D>],
        leaf: bool,
        pool: &mut [Option<T>],
    ) {
        let node = self.node_mut(node_id);
        node.clear_entries();

        for entry in entries {
            if leaf {
                if let Some(data) = pool[entry.data].take() {
                    node.push_leaf_entry(
                        &entry.min,
                        &entry.max,
                        &entry.insert_min,
                        &entry.insert_max,
                        data,
                    );
                }
            } else {
                node.push_child_entry(&entry.min, &entry.max, entry.child);
            }
        }
    }

    /// Port of `forcedReinsert`: R*-tree forced reinsert.
    ///
    /// Temporarily adds the new entry, then removes the REINSERT_COUNT entries farthest from
    /// the node center, and reinserts them.
    fn forced_reinsert(
        &mut self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<NodeId>,
        reinserted_levels: &mut u32,
    ) {
        // Collect all entries including the new one
        let total_entries = self.node(node_id).count + 1;

        // Keep close entries in the node, reinsert far ones.  (The C++ computes this after
        // gathering and sorting the entries, which does not modify the node, so the early
        // fallback is equivalent.)
        let reinsert_count =
            (Self::REINSERT_COUNT as i64).min(total_entries as i64 - Self::MINNODES as i64);

        if reinsert_count <= 0 {
            // Can't reinsert without underflow, fall back to split
            self.split_node(node_id, min, max, data, path, reinserted_levels);
            return;
        }

        // Compute node center
        let (node_min, node_max) = self.node(node_id).compute_enclosing_bounds();
        let mut center = [0.0f64; D];

        for d in 0..D {
            center[d] = (node_min[d] as f64 + node_max[d] as f64) / 2.0;
        }

        let mut pool: Vec<Option<T>> = Vec::with_capacity(total_entries);
        let mut entries = self.gather_entries(node_id, min, max, Some(data), &mut pool);

        // Distance from entry center to node center (the new entry is computed the same way
        // from aMin / aMax, which are its bounds)
        for entry in entries.iter_mut() {
            let mut dist_sq: i64 = 0;

            for d in 0..D {
                let entry_center = (entry.min[d] as f64 + entry.max[d] as f64) / 2.0;
                let diff = entry_center - center[d];
                dist_sq = dist_sq.wrapping_add(cvttsd2si(diff * diff));
            }

            entry.dist_sq = dist_sq;
        }

        // Sort by distance descending to find the farthest
        std_sort(&mut entries, |a, b| a.dist_sq > b.dist_sq);

        let reinsert_count = reinsert_count as usize;

        // Rebuild the node with the close entries
        let leaf = self.node(node_id).is_leaf();
        self.fill_node(node_id, &entries[reinsert_count..], leaf, &mut pool);
        self.adjust_path(path, node_id);

        // Reinsert the far entries
        for entry in &entries[..reinsert_count] {
            if self.node(node_id).is_leaf() {
                if let Some(data) = pool[entry.data].take() {
                    self.insert_impl(
                        &entry.insert_min,
                        &entry.insert_max,
                        data,
                        reinserted_levels,
                    );
                }
            } else {
                let level = self.node(node_id).level - 1;
                self.reinsert_node(
                    entry.child,
                    &entry.min,
                    &entry.max,
                    level,
                    reinserted_levels,
                );
            }
        }
    }

    /// Port of `reinsertNode`: reinsert an internal node's child at its correct level.
    fn reinsert_node(
        &mut self,
        child: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        level: i32,
        reinserted_levels: &mut u32,
    ) {
        // Find a node at the correct level
        let mut path: Vec<NodeId> = Vec::new();
        let target = self.choose_subtree_at_level(self.root, min, max, level + 1, &mut path);

        if !self.node(target).is_full() {
            self.node_mut(target).push_child_entry(min, max, child);
            self.adjust_path(&path, target);
        } else {
            self.split_node_internal(target, min, max, child, &mut path, reinserted_levels);
        }
    }

    /// Port of `chooseSubtreeAtLevel`: ChooseSubtree targeting a specific level.
    fn choose_subtree_at_level(
        &self,
        start: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        target_level: i32,
        path: &mut Vec<NodeId>,
    ) -> NodeId {
        path.clear();
        let mut node_id = start;

        while self.node(node_id).level > target_level {
            path.push(node_id);
            let node = self.node(node_id);
            node_id = node.children[Self::least_enlargement(node, min, max)];
        }

        node_id
    }

    /// The comparator `a.min[axis] < b.min[axis] || ( a.min[axis] == b.min[axis] &&
    /// a.max[axis] < b.max[axis] )` of `splitNode`.
    fn by_min_then_max(axis: usize) -> impl FnMut(&SplitEntry<D>, &SplitEntry<D>) -> bool {
        move |a, b| {
            a.min[axis] < b.min[axis] || (a.min[axis] == b.min[axis] && a.max[axis] < b.max[axis])
        }
    }

    /// The comparator `a.max[axis] < b.max[axis] || ( a.max[axis] == b.max[axis] &&
    /// a.min[axis] < b.min[axis] )` of `splitNode`.
    fn by_max_then_min(axis: usize) -> impl FnMut(&SplitEntry<D>, &SplitEntry<D>) -> bool {
        move |a, b| {
            a.max[axis] < b.max[axis] || (a.max[axis] == b.max[axis] && a.min[axis] < b.min[axis])
        }
    }

    /// Port of `splitNode`: R*-tree split.
    ///
    /// ChooseSplitAxis: for each axis, sort by min then max. Choose axis minimizing sum of
    /// perimeters across all valid distributions.
    ///
    /// ChooseSplitIndex: along chosen axis, choose distribution minimizing overlap.
    fn split_node(
        &mut self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<NodeId>,
        reinserted_levels: &mut u32,
    ) {
        // Collect all entries including the overflow entry
        let total_entries = self.node(node_id).count + 1;
        let mut pool: Vec<Option<T>> = Vec::with_capacity(total_entries);
        let mut entries = self.gather_entries(node_id, min, max, Some(data), &mut pool);

        // ChooseSplitAxis: minimize sum of perimeters
        let mut best_axis = 0;
        let mut best_perimeter_sum = i64::MAX;

        for axis in 0..D {
            let mut perimeter_sum: i64 = 0;

            // Sort by min bound on this axis
            std_sort(&mut entries, Self::by_min_then_max(axis));
            perimeter_sum =
                perimeter_sum.wrapping_add(Self::compute_split_perimeters(&entries, total_entries));

            // Sort by max bound on this axis
            std_sort(&mut entries, Self::by_max_then_min(axis));
            perimeter_sum =
                perimeter_sum.wrapping_add(Self::compute_split_perimeters(&entries, total_entries));

            if perimeter_sum < best_perimeter_sum {
                best_perimeter_sum = perimeter_sum;
                best_axis = axis;
            }
        }

        // ChooseSplitIndex along bestAxis: minimize overlap
        // Re-sort by min on best axis
        std_sort(&mut entries, Self::by_min_then_max(best_axis));
        let mut best_split = Self::find_best_split_index(&entries, total_entries);

        // Also try sorting by max
        let mut entries_by_max = entries.clone();
        std_sort(&mut entries_by_max, Self::by_max_then_min(best_axis));
        let best_split_max = Self::find_best_split_index(&entries_by_max, total_entries);

        let overlap_min = Self::compute_split_overlap(&entries, best_split, total_entries);
        let overlap_max =
            Self::compute_split_overlap(&entries_by_max, best_split_max, total_entries);

        if overlap_max < overlap_min {
            entries = entries_by_max;
            best_split = best_split_max;
        }

        // Create new sibling node
        let sibling = self.alloc_node();
        let level = self.node(node_id).level;
        self.node_mut(sibling).level = level;

        // Distribute entries
        let leaf = self.node(node_id).is_leaf();
        self.fill_node(node_id, &entries[..best_split], leaf, &mut pool);
        self.fill_node(sibling, &entries[best_split..], leaf, &mut pool);

        // Propagate the split upward
        self.propagate_split(node_id, sibling, path, reinserted_levels);
    }

    /// The common tail of `splitNode` and `splitNodeInternal`: hang the new `sibling` of
    /// `node_id` into the tree (new root, free parent slot or recursive parent split).
    fn propagate_split(
        &mut self,
        node_id: NodeId,
        sibling: NodeId,
        path: &mut Vec<NodeId>,
        reinserted_levels: &mut u32,
    ) {
        let (sib_min, sib_max) = self.node(sibling).compute_enclosing_bounds();

        match path.last().copied() {
            None => {
                // Splitting the root: create new root
                let new_root = self.alloc_node();
                let level = self.node(self.root).level + 1;
                let (node_min, node_max) = self.node(node_id).compute_enclosing_bounds();
                let root = self.node_mut(new_root);
                root.level = level;
                root.push_child_entry(&node_min, &node_max, node_id);
                root.push_child_entry(&sib_min, &sib_max, sibling);
                self.root = new_root;
            }
            Some(parent) => {
                // Update the existing child's bbox in parent
                if let Some(child_slot) = self.find_child_slot(parent, node_id) {
                    let (node_min, node_max) = self.node(node_id).compute_enclosing_bounds();
                    self.node_mut(parent)
                        .set_child_bounds(child_slot, &node_min, &node_max);
                }

                // Insert sibling into parent
                if !self.node(parent).is_full() {
                    self.node_mut(parent)
                        .push_child_entry(&sib_min, &sib_max, sibling);
                    path.pop();
                    self.adjust_path(path, parent);
                } else {
                    path.pop();
                    self.split_node_internal(
                        parent,
                        &sib_min,
                        &sib_max,
                        sibling,
                        path,
                        reinserted_levels,
                    );
                }
            }
        }
    }

    /// Port of `splitNodeInternal`: split an internal node to insert a new child.
    fn split_node_internal(
        &mut self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        child: NodeId,
        path: &mut Vec<NodeId>,
        reinserted_levels: &mut u32,
    ) {
        let node = self.node(node_id);
        let total_entries = node.count + 1;
        let mut entries = vec![SplitEntry::<D>::zeroed(); total_entries];

        for (i, entry) in entries.iter_mut().enumerate().take(node.count) {
            (entry.min, entry.max) = node.get_child_bounds(i);
            entry.child = node.children[i];
        }

        entries[total_entries - 1].min = *min;
        entries[total_entries - 1].max = *max;
        entries[total_entries - 1].child = child;

        // Choose best axis
        let mut best_axis = 0;
        let mut best_perimeter_sum = i64::MAX;

        for axis in 0..D {
            std_sort(&mut entries, |a, b| a.min[axis] < b.min[axis]);
            let perim_sum = Self::compute_split_perimeters(&entries, total_entries);

            if perim_sum < best_perimeter_sum {
                best_perimeter_sum = perim_sum;
                best_axis = axis;
            }
        }

        // Re-sort on best axis, find best split
        std_sort(&mut entries, |a, b| a.min[best_axis] < b.min[best_axis]);
        let best_split = Self::find_best_split_index(&entries, total_entries);

        // Create sibling
        let sibling = self.alloc_node();
        let level = self.node(node_id).level;
        self.node_mut(sibling).level = level;

        self.fill_node(node_id, &entries[..best_split], false, &mut []);
        self.fill_node(sibling, &entries[best_split..], false, &mut []);

        self.propagate_split(node_id, sibling, path, reinserted_levels);
    }

    /// Bounds of the entries `[start, end)`, per axis (`std::numeric_limits` start values).
    fn group_bounds(entries: &[SplitEntry<D>], start: usize, end: usize) -> ([i32; D], [i32; D]) {
        let mut mn = [i32::MAX; D];
        let mut mx = [i32::MIN; D];

        for entry in &entries[start..end] {
            for d in 0..D {
                if entry.min[d] < mn[d] {
                    mn[d] = entry.min[d];
                }

                if entry.max[d] > mx[d] {
                    mx[d] = entry.max[d];
                }
            }
        }

        (mn, mx)
    }

    /// The valid split positions `MINNODES..=total - MINNODES` (empty if there are none).
    fn split_range(total_entries: usize) -> std::ops::RangeInclusive<usize> {
        if total_entries >= Self::MINNODES {
            Self::MINNODES..=total_entries - Self::MINNODES
        } else {
            #[allow(clippy::reversed_empty_ranges)]
            {
                1..=0
            }
        }
    }

    /// Port of `computeSplitPerimeters`: sum of perimeters for all valid split distributions.
    fn compute_split_perimeters(entries: &[SplitEntry<D>], total_entries: usize) -> i64 {
        let mut sum: i64 = 0;

        for k in Self::split_range(total_entries) {
            // Group 1: entries [0, k)
            // Group 2: entries [k, totalEntries)
            for (start, end) in [(0, k), (k, total_entries)] {
                let (mn, mx) = Self::group_bounds(entries, start, end);
                let mut perimeter: i64 = 0;

                for d in 0..D {
                    perimeter = perimeter.wrapping_add(mx[d] as i64 - mn[d] as i64);
                }

                sum = sum.wrapping_add(perimeter.wrapping_mul(2));
            }
        }

        sum
    }

    /// Port of `findBestSplitIndex`: the split index that minimizes overlap between the two
    /// groups.
    fn find_best_split_index(entries: &[SplitEntry<D>], total_entries: usize) -> usize {
        let mut best_split = Self::MINNODES;
        let mut best_overlap = i64::MAX;
        let mut best_area_sum = i64::MAX;

        for k in Self::split_range(total_entries) {
            let overlap = Self::compute_split_overlap(entries, k, total_entries);

            // Compute area sum for tie-breaking
            let mut area_sum: i64 = 0;

            for (start, end) in [(0, k), (k, total_entries)] {
                let (mn, mx) = Self::group_bounds(entries, start, end);
                let mut area: i64 = 1;

                for d in 0..D {
                    area = area.wrapping_mul(mx[d] as i64 - mn[d] as i64);
                }

                area_sum = area_sum.wrapping_add(area);
            }

            if overlap < best_overlap || (overlap == best_overlap && area_sum < best_area_sum) {
                best_split = k;
                best_overlap = overlap;
                best_area_sum = area_sum;
            }
        }

        best_split
    }

    /// Port of `computeSplitOverlap`: the overlap area between two split groups.
    fn compute_split_overlap(
        entries: &[SplitEntry<D>],
        split_idx: usize,
        total_entries: usize,
    ) -> i64 {
        let (g1_min, g1_max) = Self::group_bounds(entries, 0, split_idx);
        let (g2_min, g2_max) = Self::group_bounds(entries, split_idx, total_entries);

        // Compute overlap volume
        let mut overlap: i64 = 1;

        for d in 0..D {
            let lo = g1_min[d].max(g2_min[d]);
            let hi = g1_max[d].min(g2_max[d]);

            if lo >= hi {
                return 0;
            }

            overlap = overlap.wrapping_mul(hi as i64 - lo as i64);
        }

        overlap
    }

    /// Port of `computeOverlap`: total overlap of child `idx` with all other children in the
    /// node.
    fn compute_overlap(node: &RtreeNode<T, D, M>, idx: usize) -> i64 {
        let mut total: i64 = 0;
        let (i_min, i_max) = node.get_child_bounds(idx);

        for j in 0..node.count {
            if j == idx {
                continue;
            }

            total = total.wrapping_add(node.child_overlap_area(j, &i_min, &i_max));
        }

        total
    }

    /// Port of `computeOverlapEnlarged`: total overlap of child `idx` (enlarged to include the
    /// query box) with the other children.
    fn compute_overlap_enlarged(
        node: &RtreeNode<T, D, M>,
        idx: usize,
        min: &[i32; D],
        max: &[i32; D],
    ) -> i64 {
        let (mut enlarged_min, mut enlarged_max) = node.get_child_bounds(idx);

        for d in 0..D {
            if min[d] < enlarged_min[d] {
                enlarged_min[d] = min[d];
            }

            if max[d] > enlarged_max[d] {
                enlarged_max[d] = max[d];
            }
        }

        let mut total: i64 = 0;

        for j in 0..node.count {
            if j == idx {
                continue;
            }

            total = total.wrapping_add(node.child_overlap_area(j, &enlarged_min, &enlarged_max));
        }

        total
    }

    /// Port of `adjustPath`: adjust bounding boxes for all nodes in the path (root to leaf).
    ///
    /// `path` is the path from root to the parent of the modified node, `bottom_child` the
    /// modified node (leaf or internal) whose parent's bbox needs updating.
    fn adjust_path(&mut self, path: &[NodeId], bottom_child: NodeId) {
        let mut child_to_update = bottom_child;

        for &parent in path.iter().rev() {
            if child_to_update != NO_NODE
                && let Some(slot) = self.find_child_slot(parent, child_to_update)
            {
                let (child_min, child_max) = self.node(child_to_update).compute_enclosing_bounds();
                self.node_mut(parent)
                    .set_child_bounds(slot, &child_min, &child_max);
            }

            child_to_update = parent;
        }
    }

    /// Port of `findChildSlot` (`None` for the C++ -1).
    fn find_child_slot(&self, parent: NodeId, child: NodeId) -> Option<usize> {
        let p = self.node(parent);
        (0..p.count).find(|&i| p.children[i] == child)
    }

    /// Port of `condenseRoot`: if the root has only one child, replace it with that child.
    fn condense_root(&mut self) {
        while self.root != NO_NODE
            && self.node(self.root).is_internal()
            && self.node(self.root).count == 1
        {
            let old_root = self.root;
            self.root = self.node(old_root).children[0];
            self.free_node(old_root);
        }

        if self.root != NO_NODE && self.node(self.root).count == 0 {
            let root = self.root;
            self.free_node(root);
            self.root = NO_NODE;
        }
    }

    /// Port of `searchImpl`: recursive search.  Returns `true` if search should continue,
    /// `false` if the visitor stopped early.
    fn search_impl<F>(
        &self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        visitor: &mut F,
        found: &mut i32,
    ) -> bool
    where
        F: FnMut(&T) -> bool,
    {
        let node = self.node(node_id);
        let mut mask = node.child_overlap_mask(min, max);

        if node.is_leaf() {
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                *found += 1;

                if !visitor(&node.data[i]) {
                    return false;
                }
            }
        } else {
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;

                if !self.search_impl(node.children[i], min, max, visitor, found) {
                    return false;
                }
            }
        }

        true
    }

    /// Port of `minDistSq`: minimum squared distance from a point to a child's bounding box.
    fn min_dist_sq(node: &RtreeNode<T, D, M>, slot: usize, point: &[i32; D]) -> i64 {
        let mut dist: i64 = 0;

        for d in 0..D {
            let lo = node.min[slot][d];
            let hi = node.max[slot][d];

            if point[d] < lo {
                let diff = lo as i64 - point[d] as i64;
                dist = dist.wrapping_add(diff.wrapping_mul(diff));
            } else if point[d] > hi {
                let diff = point[d] as i64 - hi as i64;
                dist = dist.wrapping_add(diff.wrapping_mul(diff));
            }
        }

        dist
    }
}

impl<T: PartialEq, const D: usize, const M: usize> DynamicRTree<T, D, M> {
    /// Port of `Remove`: removes an item, first searching with the given bounding box, then
    /// (if the item has moved since insertion) through the whole tree using the stored
    /// insertion bounding boxes.
    ///
    /// Returns `true` if the item was found and removed.
    pub fn remove(&mut self, min: &[i32; D], max: &[i32; D], data: &T) -> bool {
        if self.root == NO_NODE {
            return false;
        }

        // Try removal using the provided bbox first
        let mut reinsert_list: Vec<NodeId> = Vec::new();

        if self.remove_impl(self.root, min, max, data, &mut reinsert_list) {
            self.count -= 1;
            self.reinsert_orphans(&reinsert_list);
            self.condense_root();
            return true;
        }

        // Fall back to full-tree search using stored insertion bboxes
        let full_min = [i32::MIN; D];
        let full_max = [i32::MAX; D];
        reinsert_list.clear();

        if self.remove_impl(self.root, &full_min, &full_max, data, &mut reinsert_list) {
            self.count -= 1;
            self.reinsert_orphans(&reinsert_list);
            self.condense_root();
            return true;
        }

        false
    }

    /// Port of `removeImpl`: remove an item from the tree, collecting underflowing nodes for
    /// reinsertion.
    fn remove_impl(
        &mut self,
        node_id: NodeId,
        min: &[i32; D],
        max: &[i32; D],
        data: &T,
        reinsert_list: &mut Vec<NodeId>,
    ) -> bool {
        let node = self.node(node_id);

        if node.is_leaf() {
            for i in 0..node.count {
                if node.data[i] == *data && node.child_overlaps(i, min, max) {
                    self.node_mut(node_id).remove_child(i);
                    return true;
                }
            }

            return false;
        }

        // Internal node: recurse into children whose bbox overlaps the query
        let mut i = 0;

        while i < self.node(node_id).count {
            if !self.node(node_id).child_overlaps(i, min, max) {
                i += 1;
                continue;
            }

            let child = self.node(node_id).children[i];

            if self.remove_impl(child, min, max, data, reinsert_list) {
                // Update child's bbox in parent
                if self.node(child).count > 0 {
                    let (child_min, child_max) = self.node(child).compute_enclosing_bounds();
                    self.node_mut(node_id)
                        .set_child_bounds(i, &child_min, &child_max);

                    // Check for underflow
                    if self.node(child).count < Self::MINNODES && node_id != self.root {
                        reinsert_list.push(child);
                        self.node_mut(node_id).remove_child(i);
                    }
                } else {
                    self.free_node(child);
                    self.node_mut(node_id).remove_child(i);
                }

                return true;
            }

            i += 1;
        }

        false
    }

    /// Port of `reinsertOrphans`: reinsert all entries from orphaned underflowing nodes.
    fn reinsert_orphans(&mut self, reinsert_list: &[NodeId]) {
        for &orphan in reinsert_list {
            if self.node(orphan).is_leaf() {
                let count = self.node(orphan).count;
                let bounds: Vec<([i32; D], [i32; D])> = (0..count)
                    .map(|i| self.node(orphan).get_insert_bounds(i))
                    .collect();
                let data = std::mem::take(&mut self.node_mut(orphan).data);

                for ((mn, mx), item) in bounds.into_iter().zip(data) {
                    let mut reinserted_levels: u32 = 0;
                    self.insert_impl(&mn, &mx, item, &mut reinserted_levels);
                }
            } else {
                for i in 0..self.node(orphan).count {
                    let (mn, mx) = self.node(orphan).get_child_bounds(i);
                    let child = self.node(orphan).children[i];
                    let level = self.node(orphan).level - 1;
                    let mut reinserted_levels: u32 = 0;
                    self.reinsert_node(child, &mn, &mx, level, &mut reinserted_levels);
                }
            }

            self.free_node(orphan);
        }
    }
}

impl<'a, T, const D: usize, const M: usize> IntoIterator for &'a DynamicRTree<T, D, M> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T, D, M>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Port of `DYNAMIC_RTREE::Iterator` (no query box) and `DYNAMIC_RTREE::SearchIterator` (with
/// a query box): a depth-first traversal with a small explicit stack.
#[derive(Clone, Debug)]
pub struct Iter<'a, T, const D: usize, const M: usize> {
    tree: &'a DynamicRTree<T, D, M>,
    /// `STACK_ENTRY { node, childIdx }`
    stack: Vec<(NodeId, usize)>,
    query: Option<([i32; D], [i32; D])>,
}

impl<'a, T, const D: usize, const M: usize> Iter<'a, T, D, M> {
    fn new(tree: &'a DynamicRTree<T, D, M>, query: Option<([i32; D], [i32; D])>) -> Self {
        let mut stack = Vec::new();

        if tree.root != NO_NODE && tree.node(tree.root).count > 0 {
            stack.push((tree.root, 0));
        }

        Iter { tree, stack, query }
    }

    fn overlaps(&self, node: &RtreeNode<T, D, M>, i: usize) -> bool {
        match &self.query {
            None => true,
            Some((min, max)) => node.child_overlaps(i, min, max),
        }
    }
}

impl<'a, T, const D: usize, const M: usize> Iterator for Iter<'a, T, D, M> {
    type Item = &'a T;

    /// Port of `Iterator::advance` / `SearchIterator::advance`.
    fn next(&mut self) -> Option<&'a T> {
        let tree = self.tree;

        while let Some(&(node_id, child_idx)) = self.stack.last() {
            let node = tree.node(node_id);
            let mut idx = child_idx;
            let mut descend = NO_NODE;

            while idx < node.count {
                let hit = self.overlaps(node, idx);
                idx += 1;

                if hit {
                    if node.is_leaf() {
                        if let Some(top) = self.stack.last_mut() {
                            top.1 = idx;
                        }

                        return Some(&node.data[idx - 1]);
                    }

                    descend = node.children[idx - 1];
                    break;
                }
            }

            if descend != NO_NODE {
                if let Some(top) = self.stack.last_mut() {
                    top.1 = idx;
                }

                self.stack.push((descend, 0));
            } else {
                self.stack.pop();
            }
        }

        None
    }
}
