//! Research prototype: port of KiCad's own `KIRTREE::DYNAMIC_RTREE`
//! (`libs/kimath/include/geometry/rtree/dynamic_rtree.h` + `RTREE_NODE` of `rtree_node.h`) for
//! `ELEMTYPE = int`, `TMAXNODES = 16`, `NUMDIMS = D` (2 or 3): Hilbert/Morton bulk load, R*-tree
//! insert with forced reinsert and split, remove with orphan reinsertion, and search.
//!
//! Nodes live in an arena (`Vec`) instead of the C++ slab allocator; node identity is only ever
//! compared for equality in the C++ code, so the allocation order cannot influence results.
//! All `std::sort` calls go through the libstdc++ port, because the comparators leave ties.
//! 64-bit area/overlap arithmetic uses wrapping operations: C++ overflows `int64_t` there for
//! boxes spanning most of the `int` range (undefined behaviour, wraps in practice).

use crate::packed_rtree::hilbert_nd2d;
use crate::stdsort::std_sort;

pub const MAXNODES: usize = 16;
/// `MAXNODES * 2 / 5` (~40%, R*-tree convention)
pub const MINNODES: usize = MAXNODES * 2 / 5;
/// Fraction of entries to reinsert on overflow (30% per R*-tree paper)
pub const REINSERT_COUNT: usize = MAXNODES * 3 / 10;

#[derive(Clone)]
struct Node<T, const D: usize> {
    count: usize,
    /// 0 = leaf, higher = internal
    level: i32,
    min: [[i32; D]; MAXNODES],
    max: [[i32; D]; MAXNODES],
    children: [usize; MAXNODES],
    data: [T; MAXNODES],
    /// Stored insertion bboxes for leaf entries
    ins_min: [[i32; D]; MAXNODES],
    ins_max: [[i32; D]; MAXNODES],
}

impl<T: Copy + Default, const D: usize> Node<T, D> {
    fn new() -> Self {
        Node {
            count: 0,
            level: 0,
            min: [[0; D]; MAXNODES],
            max: [[0; D]; MAXNODES],
            children: [usize::MAX; MAXNODES],
            data: [T::default(); MAXNODES],
            ins_min: [[0; D]; MAXNODES],
            ins_max: [[0; D]; MAXNODES],
        }
    }

    fn is_leaf(&self) -> bool {
        self.level == 0
    }

    fn is_internal(&self) -> bool {
        self.level > 0
    }

    fn is_full(&self) -> bool {
        self.count >= MAXNODES
    }

    /// `RTREE_NODE::ComputeEnclosingBounds`
    fn compute_enclosing_bounds(&self) -> ([i32; D], [i32; D]) {
        let mut mn = [i32::MAX; D];
        let mut mx = [i32::MIN; D];
        for i in 0..self.count {
            for d in 0..D {
                if self.min[i][d] < mn[d] {
                    mn[d] = self.min[i][d];
                }
                if self.max[i][d] > mx[d] {
                    mx[d] = self.max[i][d];
                }
            }
        }
        (mn, mx)
    }

    /// `RTREE_NODE::ChildArea`
    fn child_area(&self, i: usize) -> i64 {
        let mut area: i64 = 1;
        for d in 0..D {
            area = area.wrapping_mul(self.max[i][d] as i64 - self.min[i][d] as i64);
        }
        area
    }

    /// `RTREE_NODE::ChildOverlaps`
    fn child_overlaps(&self, i: usize, mn: &[i32; D], mx: &[i32; D]) -> bool {
        for d in 0..D {
            if self.min[i][d] > mx[d] || self.max[i][d] < mn[d] {
                return false;
            }
        }
        true
    }

    /// `RTREE_NODE::ChildOverlapMask` (SIMD in C++ for 2D; same closed-interval predicate)
    fn child_overlap_mask(&self, mn: &[i32; D], mx: &[i32; D]) -> u32 {
        let mut mask = 0u32;
        for i in 0..self.count {
            if self.child_overlaps(i, mn, mx) {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// `RTREE_NODE::ChildOverlapArea`
    fn child_overlap_area(&self, i: usize, mn: &[i32; D], mx: &[i32; D]) -> i64 {
        let mut overlap: i64 = 1;
        for d in 0..D {
            let lo = self.min[i][d].max(mn[d]);
            let hi = self.max[i][d].min(mx[d]);
            if lo > hi {
                return 0;
            }
            overlap = overlap.wrapping_mul(hi as i64 - lo as i64);
        }
        overlap
    }

    /// `RTREE_NODE::ChildEnlargement`
    fn child_enlargement(&self, i: usize, mn: &[i32; D], mx: &[i32; D]) -> i64 {
        let original_area = self.child_area(i);
        let mut enlarged_area: i64 = 1;
        for d in 0..D {
            let lo = self.min[i][d].min(mn[d]);
            let hi = self.max[i][d].max(mx[d]);
            enlarged_area = enlarged_area.wrapping_mul(hi as i64 - lo as i64);
        }
        enlarged_area.wrapping_sub(original_area)
    }

    /// `RTREE_NODE::RemoveChild`: remove child at slot i by swapping with last entry.
    fn remove_child(&mut self, i: usize) {
        let last = self.count - 1;
        if i != last {
            self.min[i] = self.min[last];
            self.max[i] = self.max[last];
            self.ins_min[i] = self.ins_min[last];
            self.ins_max[i] = self.ins_max[last];
            if self.is_leaf() {
                self.data[i] = self.data[last];
            } else {
                self.children[i] = self.children[last];
            }
        }
        self.count -= 1;
    }
}

/// `DYNAMIC_RTREE::SPLIT_ENTRY` (also used for the forced-reinsert `ENTRY`, with `dist_sq`)
#[derive(Clone)]
struct Entry<T, const D: usize> {
    min: [i32; D],
    max: [i32; D],
    insert_min: [i32; D],
    insert_max: [i32; D],
    data: T,
    child: usize,
    dist_sq: i64,
}

impl<T: Copy + Default, const D: usize> Entry<T, D> {
    fn zeroed() -> Self {
        Entry {
            min: [0; D],
            max: [0; D],
            insert_min: [0; D],
            insert_max: [0; D],
            data: T::default(),
            child: usize::MAX,
            dist_sq: 0,
        }
    }
}

/// `DYNAMIC_RTREE::BULK_ENTRY`
#[derive(Clone)]
pub struct BulkEntry<T, const D: usize> {
    pub min: [i32; D],
    pub max: [i32; D],
    pub data: T,
}

pub struct DynamicRTree<T, const D: usize> {
    nodes: Vec<Node<T, D>>,
    free: Vec<usize>,
    root: Option<usize>,
    count: usize,
}

impl<T: Copy + Default + PartialEq, const D: usize> Default for DynamicRTree<T, D> {
    fn default() -> Self {
        DynamicRTree { nodes: Vec::new(), free: Vec::new(), root: None, count: 0 }
    }
}

impl<T: Copy + Default + PartialEq, const D: usize> DynamicRTree<T, D> {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn alloc_node(&mut self) -> usize {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = Node::new();
            id
        } else {
            self.nodes.push(Node::new());
            self.nodes.len() - 1
        }
    }

    fn free_node(&mut self, id: usize) {
        self.free.push(id);
    }

    fn remove_all_nodes(&mut self, node: Option<usize>) {
        let Some(n) = node else { return };
        if self.nodes[n].is_internal() {
            for i in 0..self.nodes[n].count {
                let c = self.nodes[n].children[i];
                self.remove_all_nodes(Some(c));
            }
        }
        self.free_node(n);
    }

    /// `DYNAMIC_RTREE::RemoveAll`
    pub fn remove_all(&mut self) {
        let root = self.root;
        self.remove_all_nodes(root);
        self.root = None;
        self.count = 0;
    }

    /// `DYNAMIC_RTREE::Insert`
    pub fn insert(&mut self, min: &[i32; D], max: &[i32; D], data: T) {
        if self.root.is_none() {
            let r = self.alloc_node();
            self.nodes[r].level = 0;
            self.root = Some(r);
        }
        // Bitmask tracking which levels have had forced reinsert this insertion.
        let mut reinserted_levels: u32 = 0;
        self.insert_impl(min, max, data, &mut reinserted_levels);
        self.count += 1;
    }

    /// `DYNAMIC_RTREE::Remove`
    pub fn remove(&mut self, min: &[i32; D], max: &[i32; D], data: T) -> bool {
        let Some(root) = self.root else { return false };

        // Try removal using the provided bbox first
        let mut reinsert_list: Vec<usize> = Vec::new();

        if self.remove_impl(root, min, max, data, &mut reinsert_list) {
            self.count -= 1;
            self.reinsert_orphans(&reinsert_list);
            self.condense_root();
            return true;
        }

        // Fall back to full-tree search using stored insertion bboxes
        let full_min = [i32::MIN; D];
        let full_max = [i32::MAX; D];
        reinsert_list.clear();

        if self.remove_impl(root, &full_min, &full_max, data, &mut reinsert_list) {
            self.count -= 1;
            self.reinsert_orphans(&reinsert_list);
            self.condense_root();
            return true;
        }
        false
    }

    /// `DYNAMIC_RTREE::Search`; the visitor returns `false` to stop early.
    pub fn search<F: FnMut(&T) -> bool>(&self, min: &[i32; D], max: &[i32; D], mut visitor: F) -> i32 {
        let mut found = 0;
        if let Some(root) = self.root {
            self.search_impl(root, min, max, &mut visitor, &mut found);
        }
        found
    }

    /// `DYNAMIC_RTREE::BulkLoad`
    pub fn bulk_load(&mut self, entries: &[BulkEntry<T, D>]) {
        if entries.is_empty() {
            self.remove_all();
            return;
        }

        // Find global bounds for Hilbert normalization
        let mut global_min = [i32::MAX; D];
        let mut global_max = [i32::MIN; D];
        for entry in entries {
            for d in 0..D {
                if entry.min[d] < global_min[d] {
                    global_min[d] = entry.min[d];
                }
                if entry.max[d] > global_max[d] {
                    global_max[d] = entry.max[d];
                }
            }
        }

        let mut range = [0.0f64; D];
        for d in 0..D {
            range[d] = global_max[d] as f64 - global_min[d] as f64;
            if range[d] <= 0.0 {
                range[d] = 1.0;
            }
        }

        let n = entries.len();
        let mut order: Vec<(u64, usize)> = Vec::with_capacity(n);
        for (i, entry) in entries.iter().enumerate() {
            let mut coords = [0u32; D];
            for d in 0..D {
                let center = (entry.min[d] as f64 + entry.max[d] as f64) / 2.0;
                coords[d] = (((center - global_min[d] as f64) / range[d]) * (u32::MAX as f64)) as u32;
            }
            order.push((hilbert_nd2d::<D>(32, &coords), i));
        }

        // `std::sort( order )` on (key, index) pairs: a total order, any sort gives the same result
        order.sort();

        self.remove_all();
        self.count = n;

        // Pack entries into leaf nodes
        let mut current_level: Vec<usize> = Vec::with_capacity(n.div_ceil(MAXNODES));
        let mut i = 0;
        while i < n {
            let leaf = self.alloc_node();
            self.nodes[leaf].level = 0;
            let cnt = MAXNODES.min(n - i);
            for j in 0..cnt {
                let entry = &entries[order[i + j].1];
                let nd = &mut self.nodes[leaf];
                nd.min[j] = entry.min;
                nd.max[j] = entry.max;
                nd.ins_min[j] = entry.min;
                nd.ins_max[j] = entry.max;
                nd.data[j] = entry.data;
            }
            self.nodes[leaf].count = cnt;
            current_level.push(leaf);
            i += MAXNODES;
        }

        // Build internal levels bottom-up
        let mut level = 1;
        while current_level.len() > 1 {
            let level_size = current_level.len();
            let mut next_level: Vec<usize> = Vec::with_capacity(level_size.div_ceil(MAXNODES));
            let mut i = 0;
            while i < level_size {
                let internal = self.alloc_node();
                self.nodes[internal].level = level;
                let cnt = MAXNODES.min(level_size - i);
                for j in 0..cnt {
                    let child = current_level[i + j];
                    let (cmin, cmax) = self.nodes[child].compute_enclosing_bounds();
                    let nd = &mut self.nodes[internal];
                    nd.min[j] = cmin;
                    nd.max[j] = cmax;
                    nd.children[j] = child;
                }
                self.nodes[internal].count = cnt;
                next_level.push(internal);
                i += MAXNODES;
            }
            current_level = next_level;
            level += 1;
        }
        self.root = Some(current_level[0]);
    }

    /// `DYNAMIC_RTREE::insertImpl`
    fn insert_impl(&mut self, min: &[i32; D], max: &[i32; D], data: T, reinserted_levels: &mut u32) {
        let mut path: Vec<usize> = Vec::new();
        let root = self.root.expect("root exists");
        let leaf = self.choose_subtree(root, min, max, &mut path);

        if !self.nodes[leaf].is_full() {
            let nd = &mut self.nodes[leaf];
            let slot = nd.count;
            nd.min[slot] = *min;
            nd.max[slot] = *max;
            nd.ins_min[slot] = *min;
            nd.ins_max[slot] = *max;
            nd.data[slot] = data;
            nd.count += 1;
            self.adjust_path(&path, Some(leaf));
        } else {
            self.overflow_treatment(leaf, min, max, data, &mut path, reinserted_levels);
        }
    }

    /// `DYNAMIC_RTREE::chooseSubtree`
    fn choose_subtree(&self, start: usize, min: &[i32; D], max: &[i32; D], path: &mut Vec<usize>) -> usize {
        path.clear();
        let mut node = start;

        while self.nodes[node].is_internal() {
            path.push(node);
            let nd = &self.nodes[node];

            if nd.level == 1 {
                // At the level just above leaves: minimize overlap increase
                let mut best_idx = 0;
                let mut best_overlap_inc = i64::MAX;
                let mut best_area_inc = i64::MAX;
                let mut best_area = i64::MAX;

                for i in 0..nd.count {
                    let overlap_before = self.compute_overlap(node, i);
                    let overlap_after = self.compute_overlap_enlarged(node, i, min, max);
                    let overlap_inc = overlap_after.wrapping_sub(overlap_before);
                    let area_inc = nd.child_enlargement(i, min, max);
                    let area = nd.child_area(i);

                    if overlap_inc < best_overlap_inc
                        || (overlap_inc == best_overlap_inc && area_inc < best_area_inc)
                        || (overlap_inc == best_overlap_inc && area_inc == best_area_inc && area < best_area)
                    {
                        best_idx = i;
                        best_overlap_inc = overlap_inc;
                        best_area_inc = area_inc;
                        best_area = area;
                    }
                }
                node = nd.children[best_idx];
            } else {
                // Higher levels: minimize area increase, tie-break by smallest area
                let mut best_idx = 0;
                let mut best_area_inc = i64::MAX;
                let mut best_area = i64::MAX;

                for i in 0..nd.count {
                    let area_inc = nd.child_enlargement(i, min, max);
                    let area = nd.child_area(i);
                    if area_inc < best_area_inc || (area_inc == best_area_inc && area < best_area) {
                        best_idx = i;
                        best_area_inc = area_inc;
                        best_area = area;
                    }
                }
                node = nd.children[best_idx];
            }
        }
        node
    }

    /// `DYNAMIC_RTREE::overflowTreatment`
    fn overflow_treatment(
        &mut self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<usize>,
        reinserted_levels: &mut u32,
    ) {
        let level = self.nodes[node].level;
        if level >= 32 {
            self.split_node(node, min, max, data, path, reinserted_levels);
            return;
        }
        let level_mask = 1u32 << level;
        if (*reinserted_levels & level_mask) == 0 {
            *reinserted_levels |= level_mask;
            self.forced_reinsert(node, min, max, data, path, reinserted_levels);
        } else {
            self.split_node(node, min, max, data, path, reinserted_levels);
        }
    }

    /// `DYNAMIC_RTREE::forcedReinsert`
    fn forced_reinsert(
        &mut self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<usize>,
        reinserted_levels: &mut u32,
    ) {
        let total_entries = self.nodes[node].count + 1;
        let mut entries: Vec<Entry<T, D>> = vec![Entry::zeroed(); total_entries];

        // Compute node center
        let (node_min, node_max) = self.nodes[node].compute_enclosing_bounds();
        let mut center = [0.0f64; D];
        for d in 0..D {
            center[d] = (node_min[d] as f64 + node_max[d] as f64) / 2.0;
        }

        let is_leaf = self.nodes[node].is_leaf();
        let old_count = self.nodes[node].count;

        // Gather existing entries
        for i in 0..old_count {
            let nd = &self.nodes[node];
            entries[i].min = nd.min[i];
            entries[i].max = nd.max[i];
            if is_leaf {
                entries[i].insert_min = nd.ins_min[i];
                entries[i].insert_max = nd.ins_max[i];
                entries[i].data = nd.data[i];
                entries[i].child = usize::MAX;
            } else {
                entries[i].child = nd.children[i];
            }

            // Distance from entry center to node center
            let mut dist_sq: i64 = 0;
            for d in 0..D {
                let entry_center = (entries[i].min[d] as f64 + entries[i].max[d] as f64) / 2.0;
                let diff = entry_center - center[d];
                dist_sq = dist_sq.wrapping_add((diff * diff) as i64);
            }
            entries[i].dist_sq = dist_sq;
        }

        // Add the new entry
        {
            let ne = &mut entries[old_count];
            ne.min = *min;
            ne.max = *max;
            ne.insert_min = *min;
            ne.insert_max = *max;
            ne.data = data;
            ne.child = usize::MAX;
            let mut dist_sq: i64 = 0;
            for d in 0..D {
                let entry_center = (min[d] as f64 + max[d] as f64) / 2.0;
                let diff = entry_center - center[d];
                dist_sq = dist_sq.wrapping_add((diff * diff) as i64);
            }
            ne.dist_sq = dist_sq;
        }

        // Sort by distance descending to find the farthest
        std_sort(&mut entries, |a, b| a.dist_sq > b.dist_sq);

        // Keep close entries in the node, reinsert far ones
        let reinsert_count = (REINSERT_COUNT as i64).min(total_entries as i64 - MINNODES as i64);

        if reinsert_count <= 0 {
            // Can't reinsert without underflow, fall back to split
            self.split_node(node, min, max, data, path, reinserted_levels);
            return;
        }
        let reinsert_count = reinsert_count as usize;

        // Rebuild the node with the close entries
        self.nodes[node].count = 0;
        for e in entries.iter().take(total_entries).skip(reinsert_count) {
            let nd = &mut self.nodes[node];
            let slot = nd.count;
            nd.min[slot] = e.min;
            nd.max[slot] = e.max;
            if is_leaf {
                nd.ins_min[slot] = e.insert_min;
                nd.ins_max[slot] = e.insert_max;
                nd.data[slot] = e.data;
            } else {
                nd.children[slot] = e.child;
            }
            nd.count += 1;
        }

        self.adjust_path(path, Some(node));

        // Reinsert the far entries
        for e in entries.iter().take(reinsert_count) {
            if is_leaf {
                self.insert_impl(&e.insert_min, &e.insert_max, e.data, reinserted_levels);
            } else {
                let lvl = self.nodes[node].level - 1;
                self.reinsert_node(e.child, &e.min, &e.max, lvl, reinserted_levels);
            }
        }
    }

    /// `DYNAMIC_RTREE::reinsertNode`
    fn reinsert_node(&mut self, child: usize, min: &[i32; D], max: &[i32; D], level: i32, reinserted_levels: &mut u32) {
        let mut path: Vec<usize> = Vec::new();
        let root = self.root.expect("root exists");
        let target = self.choose_subtree_at_level(root, min, max, level + 1, &mut path);

        if !self.nodes[target].is_full() {
            let nd = &mut self.nodes[target];
            let slot = nd.count;
            nd.min[slot] = *min;
            nd.max[slot] = *max;
            nd.children[slot] = child;
            nd.count += 1;
            self.adjust_path(&path, Some(target));
        } else {
            self.split_node_internal(target, min, max, child, &mut path, reinserted_levels);
        }
    }

    /// `DYNAMIC_RTREE::chooseSubtreeAtLevel`
    fn choose_subtree_at_level(
        &self,
        start: usize,
        min: &[i32; D],
        max: &[i32; D],
        target_level: i32,
        path: &mut Vec<usize>,
    ) -> usize {
        path.clear();
        let mut node = start;
        while self.nodes[node].level > target_level {
            path.push(node);
            let nd = &self.nodes[node];
            let mut best_idx = 0;
            let mut best_area_inc = i64::MAX;
            let mut best_area = i64::MAX;
            for i in 0..nd.count {
                let area_inc = nd.child_enlargement(i, min, max);
                let area = nd.child_area(i);
                if area_inc < best_area_inc || (area_inc == best_area_inc && area < best_area) {
                    best_idx = i;
                    best_area_inc = area_inc;
                    best_area = area;
                }
            }
            node = nd.children[best_idx];
        }
        node
    }

    /// `DYNAMIC_RTREE::splitNode`
    fn split_node(
        &mut self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        path: &mut Vec<usize>,
        reinserted_levels: &mut u32,
    ) {
        // Collect all entries including the overflow entry
        let old_count = self.nodes[node].count;
        let total_entries = old_count + 1;
        let is_leaf = self.nodes[node].is_leaf();
        let mut entries: Vec<Entry<T, D>> = vec![Entry::zeroed(); total_entries];

        for i in 0..old_count {
            let nd = &self.nodes[node];
            entries[i].min = nd.min[i];
            entries[i].max = nd.max[i];
            if is_leaf {
                entries[i].insert_min = nd.ins_min[i];
                entries[i].insert_max = nd.ins_max[i];
                entries[i].data = nd.data[i];
                entries[i].child = usize::MAX;
            } else {
                entries[i].child = nd.children[i];
            }
        }

        // The overflow entry
        entries[old_count].min = *min;
        entries[old_count].max = *max;
        entries[old_count].insert_min = *min;
        entries[old_count].insert_max = *max;
        entries[old_count].data = data;
        entries[old_count].child = usize::MAX;

        // ChooseSplitAxis: minimize sum of perimeters
        let mut best_axis = 0;
        let mut best_perimeter_sum = i64::MAX;

        for axis in 0..D {
            let mut perimeter_sum: i64 = 0;

            // Sort by min bound on this axis
            std_sort(&mut entries, |a, b| {
                a.min[axis] < b.min[axis] || (a.min[axis] == b.min[axis] && a.max[axis] < b.max[axis])
            });
            perimeter_sum = perimeter_sum.wrapping_add(Self::compute_split_perimeters(&entries, total_entries));

            // Sort by max bound on this axis
            std_sort(&mut entries, |a, b| {
                a.max[axis] < b.max[axis] || (a.max[axis] == b.max[axis] && a.min[axis] < b.min[axis])
            });
            perimeter_sum = perimeter_sum.wrapping_add(Self::compute_split_perimeters(&entries, total_entries));

            if perimeter_sum < best_perimeter_sum {
                best_perimeter_sum = perimeter_sum;
                best_axis = axis;
            }
        }

        // ChooseSplitIndex along bestAxis: minimize overlap
        // Re-sort by min on best axis
        std_sort(&mut entries, |a, b| {
            a.min[best_axis] < b.min[best_axis]
                || (a.min[best_axis] == b.min[best_axis] && a.max[best_axis] < b.max[best_axis])
        });

        let mut best_split = Self::find_best_split_index(&entries, total_entries);

        // Also try sorting by max
        let mut entries_by_max = entries.clone();
        std_sort(&mut entries_by_max, |a, b| {
            a.max[best_axis] < b.max[best_axis]
                || (a.max[best_axis] == b.max[best_axis] && a.min[best_axis] < b.min[best_axis])
        });

        let best_split_max = Self::find_best_split_index(&entries_by_max, total_entries);
        let overlap_min = Self::compute_split_overlap(&entries, best_split, total_entries);
        let overlap_max = Self::compute_split_overlap(&entries_by_max, best_split_max, total_entries);

        if overlap_max < overlap_min {
            entries = entries_by_max;
            best_split = best_split_max;
        }

        // Create new sibling node
        let sibling = self.alloc_node();
        self.nodes[sibling].level = self.nodes[node].level;

        // Distribute entries
        self.nodes[node].count = 0;
        for (i, e) in entries.iter().enumerate() {
            let target = if i < best_split { node } else { sibling };
            let nd = &mut self.nodes[target];
            let slot = nd.count;
            nd.min[slot] = e.min;
            nd.max[slot] = e.max;
            if is_leaf {
                nd.ins_min[slot] = e.insert_min;
                nd.ins_max[slot] = e.insert_max;
                nd.data[slot] = e.data;
            } else {
                nd.children[slot] = e.child;
            }
            nd.count += 1;
        }

        self.propagate_split(node, sibling, path, reinserted_levels);
    }

    /// Common tail of `splitNode` / `splitNodeInternal`: propagate the split upward.
    fn propagate_split(&mut self, node: usize, sibling: usize, path: &mut Vec<usize>, reinserted_levels: &mut u32) {
        let (sib_min, sib_max) = self.nodes[sibling].compute_enclosing_bounds();

        if path.is_empty() {
            // Splitting the root: create new root
            let new_root = self.alloc_node();
            let root = self.root.expect("root exists");
            self.nodes[new_root].level = self.nodes[root].level + 1;

            let (node_min, node_max) = self.nodes[node].compute_enclosing_bounds();
            let nr = &mut self.nodes[new_root];
            nr.min[0] = node_min;
            nr.max[0] = node_max;
            nr.children[0] = node;
            nr.min[1] = sib_min;
            nr.max[1] = sib_max;
            nr.children[1] = sibling;
            nr.count = 2;
            self.root = Some(new_root);
        } else {
            let parent = *path.last().unwrap();

            // Update the existing child's bbox in parent
            if let Some(child_slot) = self.find_child_slot(parent, node) {
                let (node_min, node_max) = self.nodes[node].compute_enclosing_bounds();
                self.nodes[parent].min[child_slot] = node_min;
                self.nodes[parent].max[child_slot] = node_max;
            }

            // Insert sibling into parent
            if !self.nodes[parent].is_full() {
                let pn = &mut self.nodes[parent];
                let slot = pn.count;
                pn.min[slot] = sib_min;
                pn.max[slot] = sib_max;
                pn.children[slot] = sibling;
                pn.count += 1;

                path.pop();
                self.adjust_path(path, Some(parent));
            } else {
                path.pop();
                self.split_node_internal(parent, &sib_min, &sib_max, sibling, path, reinserted_levels);
            }
        }
    }

    /// `DYNAMIC_RTREE::splitNodeInternal`
    fn split_node_internal(
        &mut self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        child: usize,
        path: &mut Vec<usize>,
        reinserted_levels: &mut u32,
    ) {
        let old_count = self.nodes[node].count;
        let total_entries = old_count + 1;
        let mut entries: Vec<Entry<T, D>> = vec![Entry::zeroed(); total_entries];

        for i in 0..old_count {
            let nd = &self.nodes[node];
            entries[i].min = nd.min[i];
            entries[i].max = nd.max[i];
            entries[i].child = nd.children[i];
        }
        entries[old_count].min = *min;
        entries[old_count].max = *max;
        entries[old_count].child = child;

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
        self.nodes[sibling].level = self.nodes[node].level;

        self.nodes[node].count = 0;
        for (i, e) in entries.iter().enumerate() {
            let target = if i < best_split { node } else { sibling };
            let nd = &mut self.nodes[target];
            let slot = nd.count;
            nd.min[slot] = e.min;
            nd.max[slot] = e.max;
            nd.children[slot] = e.child;
            nd.count += 1;
        }

        self.propagate_split(node, sibling, path, reinserted_levels);
    }

    /// `DYNAMIC_RTREE::computeSplitPerimeters`
    fn compute_split_perimeters(entries: &[Entry<T, D>], total_entries: usize) -> i64 {
        let mut sum: i64 = 0;
        for k in MINNODES..=total_entries - MINNODES {
            for grp in 0..2 {
                let (start, end) = if grp == 0 { (0, k) } else { (k, total_entries) };
                let mut perimeter: i64 = 0;
                for d in 0..D {
                    let mut mn = i32::MAX;
                    let mut mx = i32::MIN;
                    for e in &entries[start..end] {
                        if e.min[d] < mn {
                            mn = e.min[d];
                        }
                        if e.max[d] > mx {
                            mx = e.max[d];
                        }
                    }
                    perimeter = perimeter.wrapping_add(mx as i64 - mn as i64);
                }
                sum = sum.wrapping_add(perimeter.wrapping_mul(2));
            }
        }
        sum
    }

    /// `DYNAMIC_RTREE::findBestSplitIndex`
    fn find_best_split_index(entries: &[Entry<T, D>], total_entries: usize) -> usize {
        let mut best_split = MINNODES;
        let mut best_overlap = i64::MAX;
        let mut best_area_sum = i64::MAX;

        for k in MINNODES..=total_entries - MINNODES {
            let overlap = Self::compute_split_overlap(entries, k, total_entries);

            // Compute area sum for tie-breaking
            let mut area_sum: i64 = 0;
            for grp in 0..2 {
                let (start, end) = if grp == 0 { (0, k) } else { (k, total_entries) };
                let mut area: i64 = 1;
                for d in 0..D {
                    let mut mn = i32::MAX;
                    let mut mx = i32::MIN;
                    for e in &entries[start..end] {
                        if e.min[d] < mn {
                            mn = e.min[d];
                        }
                        if e.max[d] > mx {
                            mx = e.max[d];
                        }
                    }
                    area = area.wrapping_mul(mx as i64 - mn as i64);
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

    /// `DYNAMIC_RTREE::computeSplitOverlap`
    fn compute_split_overlap(entries: &[Entry<T, D>], split_idx: usize, total_entries: usize) -> i64 {
        let mut g1_min = [i32::MAX; D];
        let mut g2_min = [i32::MAX; D];
        let mut g1_max = [i32::MIN; D];
        let mut g2_max = [i32::MIN; D];

        for e in &entries[..split_idx] {
            for d in 0..D {
                if e.min[d] < g1_min[d] {
                    g1_min[d] = e.min[d];
                }
                if e.max[d] > g1_max[d] {
                    g1_max[d] = e.max[d];
                }
            }
        }
        for e in &entries[split_idx..total_entries] {
            for d in 0..D {
                if e.min[d] < g2_min[d] {
                    g2_min[d] = e.min[d];
                }
                if e.max[d] > g2_max[d] {
                    g2_max[d] = e.max[d];
                }
            }
        }

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

    /// `DYNAMIC_RTREE::computeOverlap`
    fn compute_overlap(&self, node: usize, idx: usize) -> i64 {
        let nd = &self.nodes[node];
        let mut total: i64 = 0;
        let (i_min, i_max) = (nd.min[idx], nd.max[idx]);
        for j in 0..nd.count {
            if j == idx {
                continue;
            }
            total = total.wrapping_add(nd.child_overlap_area(j, &i_min, &i_max));
        }
        total
    }

    /// `DYNAMIC_RTREE::computeOverlapEnlarged`
    fn compute_overlap_enlarged(&self, node: usize, idx: usize, min: &[i32; D], max: &[i32; D]) -> i64 {
        let nd = &self.nodes[node];
        let (mut e_min, mut e_max) = (nd.min[idx], nd.max[idx]);
        for d in 0..D {
            if min[d] < e_min[d] {
                e_min[d] = min[d];
            }
            if max[d] > e_max[d] {
                e_max[d] = max[d];
            }
        }
        let mut total: i64 = 0;
        for j in 0..nd.count {
            if j == idx {
                continue;
            }
            total = total.wrapping_add(nd.child_overlap_area(j, &e_min, &e_max));
        }
        total
    }

    /// `DYNAMIC_RTREE::adjustPath`
    fn adjust_path(&mut self, path: &[usize], bottom_child: Option<usize>) {
        let mut child_to_update = bottom_child;
        for i in (0..path.len()).rev() {
            let parent = path[i];
            if let Some(child) = child_to_update {
                if let Some(slot) = self.find_child_slot(parent, child) {
                    let (cmin, cmax) = self.nodes[child].compute_enclosing_bounds();
                    self.nodes[parent].min[slot] = cmin;
                    self.nodes[parent].max[slot] = cmax;
                }
            }
            child_to_update = Some(parent);
        }
    }

    fn find_child_slot(&self, parent: usize, child: usize) -> Option<usize> {
        let pn = &self.nodes[parent];
        (0..pn.count).find(|&i| pn.children[i] == child)
    }

    /// `DYNAMIC_RTREE::removeImpl`
    fn remove_impl(
        &mut self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        data: T,
        reinsert_list: &mut Vec<usize>,
    ) -> bool {
        if self.nodes[node].is_leaf() {
            for i in 0..self.nodes[node].count {
                if self.nodes[node].data[i] == data && self.nodes[node].child_overlaps(i, min, max) {
                    self.nodes[node].remove_child(i);
                    return true;
                }
            }
            return false;
        }

        // Internal node: recurse into children whose bbox overlaps the query
        let mut i = 0;
        while i < self.nodes[node].count {
            if !self.nodes[node].child_overlaps(i, min, max) {
                i += 1;
                continue;
            }
            let child = self.nodes[node].children[i];

            if self.remove_impl(child, min, max, data, reinsert_list) {
                // Update child's bbox in parent
                if self.nodes[child].count > 0 {
                    let (cmin, cmax) = self.nodes[child].compute_enclosing_bounds();
                    self.nodes[node].min[i] = cmin;
                    self.nodes[node].max[i] = cmax;

                    // Check for underflow
                    if self.nodes[child].count < MINNODES && Some(node) != self.root {
                        reinsert_list.push(child);
                        self.nodes[node].remove_child(i);
                    }
                } else {
                    self.free_node(child);
                    self.nodes[node].remove_child(i);
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// `DYNAMIC_RTREE::reinsertOrphans`
    fn reinsert_orphans(&mut self, reinsert_list: &[usize]) {
        for &orphan in reinsert_list {
            if self.nodes[orphan].is_leaf() {
                for i in 0..self.nodes[orphan].count {
                    let (mn, mx, d) =
                        (self.nodes[orphan].ins_min[i], self.nodes[orphan].ins_max[i], self.nodes[orphan].data[i]);
                    let mut reinserted_levels: u32 = 0;
                    self.insert_impl(&mn, &mx, d, &mut reinserted_levels);
                }
            } else {
                for i in 0..self.nodes[orphan].count {
                    let (mn, mx, c) =
                        (self.nodes[orphan].min[i], self.nodes[orphan].max[i], self.nodes[orphan].children[i]);
                    let lvl = self.nodes[orphan].level - 1;
                    let mut reinserted_levels: u32 = 0;
                    self.reinsert_node(c, &mn, &mx, lvl, &mut reinserted_levels);
                }
            }
            self.free_node(orphan);
        }
    }

    /// `DYNAMIC_RTREE::condenseRoot`
    fn condense_root(&mut self) {
        while let Some(root) = self.root {
            if self.nodes[root].is_internal() && self.nodes[root].count == 1 {
                self.root = Some(self.nodes[root].children[0]);
                self.free_node(root);
            } else {
                break;
            }
        }
        if let Some(root) = self.root {
            if self.nodes[root].count == 0 {
                self.free_node(root);
                self.root = None;
            }
        }
    }

    /// `DYNAMIC_RTREE::searchImpl`
    fn search_impl<F: FnMut(&T) -> bool>(
        &self,
        node: usize,
        min: &[i32; D],
        max: &[i32; D],
        visitor: &mut F,
        found: &mut i32,
    ) -> bool {
        let nd = &self.nodes[node];
        let mut mask = nd.child_overlap_mask(min, max);

        if nd.is_leaf() {
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                *found += 1;
                if !visitor(&nd.data[i]) {
                    return false;
                }
            }
        } else {
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                if !self.search_impl(nd.children[i], min, max, visitor, found) {
                    return false;
                }
            }
        }
        true
    }
}
