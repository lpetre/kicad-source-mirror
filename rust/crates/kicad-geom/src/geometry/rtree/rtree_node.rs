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

//! Port of `libs/kimath/include/geometry/rtree/rtree_node.h` (namespace `KIRTREE`): the
//! space-filling curve helpers, the overlap mask, the r-tree node and the node allocator.
//!
//! The coordinate type (`ELEMTYPE`) is always `int` in KiCad and is fixed to `i32` here.
//!
//! Layout differences that cannot influence results:
//!
//! - The C++ node stores its bounding boxes as a structure of arrays
//!   (`bounds[axis * 2 + side][slot]`) for SIMD scanning; the port stores one `min` and one
//!   `max` corner per slot.  The SSE2 / NEON / scalar variants of `OverlapMask2D` all evaluate
//!   the same closed-interval predicate, and so does the port.
//! - Leaf data is a `Vec<T>` (one element per used slot) instead of a fixed array inside a
//!   union, so `T` needs no default value.
//! - `SLAB_ALLOCATOR` hands out raw node pointers from 256-node pages.  The trees only ever
//!   compare node pointers for identity, so the port uses indices into a [`NodeArena`]
//!   (a `Vec` plus a free list).  The reference count of the node (used by `COW_RTREE` only) is
//!   not ported.

/// Port of `KIRTREE::HilbertXY2D`: the 64-bit Hilbert curve index of 2D unsigned coordinates.
///
/// Uses the standard iterative bit-manipulation algorithm for an order-32 Hilbert curve,
/// mapping two `u32` coordinates to a single `u64` index.
///
/// `order` is the Hilbert curve order (bits per axis, typically 32; valid range 1..=32), `ax` /
/// `ay` are coordinates in `[0, 2^order)`.
pub fn hilbert_xy2d(order: i32, ax: u32, ay: u32) -> u64 {
    let mut d: u64 = 0;
    let mut x = ax;
    let mut y = ay;

    if !(1..=32).contains(&order) {
        // C++: undefined shift
        return 0;
    }

    let mut s: u32 = 1u32 << (order - 1);

    while s > 0 {
        let rx: u32 = if (x & s) != 0 { 1 } else { 0 };
        let ry: u32 = if (y & s) != 0 { 1 } else { 0 };

        // Cannot overflow: the terms add up to at most 3 * ( 4^order - 1 ) / 3 <= 2^64 - 1
        d += (s as u64) * (s as u64) * (((3 * rx) ^ ry) as u64);

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

/// Port of `KIRTREE::HilbertND2D<NUMDIMS>`: the curve index of N-dimensional coordinates.
///
/// For 2D, delegates to [`hilbert_xy2d`].  For higher dimensions it falls back to Z-order
/// (Morton) curve interleaving, which gives good-enough clustering without the complexity of
/// a generalized Hilbert curve in N dimensions.
pub fn hilbert_nd2d<const D: usize>(order: i32, coords: &[u32; D]) -> u64 {
    if D == 2 {
        hilbert_xy2d(order, coords[0], coords[1])
    } else {
        // A 64-bit index holds only 64/NUMDIMS bits per axis.  Interleaving every bit
        // shifts the significant ones back out of the index, so take the high bits.
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

/// Port of `KIRTREE::OverlapMask2D` generalised to `D` dimensions (the scalar fallback; the
/// SIMD paths of the C++ compute the same thing): bit `i` of the result is set when the
/// bounding box in slot `i` overlaps the query box (closed intervals).  Only the first `count`
/// slots are considered.
pub fn overlap_mask<const D: usize>(
    bounds_min: &[[i32; D]],
    bounds_max: &[[i32; D]],
    count: usize,
    min: &[i32; D],
    max: &[i32; D],
) -> u32 {
    let mut mask = 0u32;

    for i in 0..count {
        let mut overlaps = true;

        for d in 0..D {
            if bounds_min[i][d] > max[d] || bounds_max[i][d] < min[d] {
                overlaps = false;
                break;
            }
        }

        if overlaps {
            mask |= 1u32 << i;
        }
    }

    mask
}

/// Index of a node in a [`NodeArena`] (the port of a `RTREE_NODE*`).
pub type NodeId = u32;

/// The null node (`nullptr`).
pub const NO_NODE: NodeId = u32::MAX;

/// Port of `KIRTREE::RTREE_NODE<DATATYPE, int, NUMDIMS, MAXNODES>`.
///
/// `D` is the number of dimensions (2 or 3), `M` the maximum number of children per node
/// (fanout, at most 31 because overlap masks are 32-bit).
#[derive(Clone, Debug)]
pub struct RtreeNode<T, const D: usize, const M: usize> {
    /// Number of valid children
    pub(crate) count: usize,
    /// 0 = leaf, higher = internal
    pub(crate) level: i32,
    /// Bounding boxes of the children: minimum corners
    pub(crate) min: [[i32; D]; M],
    /// Bounding boxes of the children: maximum corners
    pub(crate) max: [[i32; D]; M],
    /// Internal node children
    pub(crate) children: [NodeId; M],
    /// Leaf node data (`count` elements)
    pub(crate) data: Vec<T>,
    /// Stored insertion bboxes for leaf entries, enabling O(log N) removal even when the item
    /// has moved since insertion
    pub(crate) insert_min: [[i32; D]; M],
    pub(crate) insert_max: [[i32; D]; M],
}

impl<T, const D: usize, const M: usize> Default for RtreeNode<T, D, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const D: usize, const M: usize> RtreeNode<T, D, M> {
    /// `MINNODES`: ~40%, R*-tree convention
    pub const MINNODES: usize = M * 2 / 5;

    /// `RTREE_NODE::RTREE_NODE()`
    pub fn new() -> Self {
        const {
            assert!(M >= 2 && M <= 31, "the fanout must be in 2..=31");
            assert!(D >= 1, "at least one dimension is needed");
        }

        RtreeNode {
            count: 0,
            level: 0,
            min: [[0; D]; M],
            max: [[0; D]; M],
            children: [NO_NODE; M],
            data: Vec::new(),
            insert_min: [[0; D]; M],
            insert_max: [[0; D]; M],
        }
    }

    /// Number of valid children.
    pub fn count(&self) -> usize {
        self.count
    }

    /// 0 = leaf, higher = internal.
    pub fn level(&self) -> i32 {
        self.level
    }

    pub fn is_leaf(&self) -> bool {
        self.level == 0
    }

    pub fn is_internal(&self) -> bool {
        self.level > 0
    }

    pub fn is_full(&self) -> bool {
        self.count >= M
    }

    pub fn is_underflow(&self) -> bool {
        self.count < Self::MINNODES
    }

    /// Port of `ComputeEnclosingBounds`: the bounding box that encloses all children in this
    /// node, as `( min, max )`.
    pub fn compute_enclosing_bounds(&self) -> ([i32; D], [i32; D]) {
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

    /// Port of `ChildArea`: the area (or volume for 3D) of child slot `i`'s bounding box.
    ///
    /// Like every 64-bit product in the r-tree code this wraps on overflow: the C++ overflows
    /// `int64_t` for boxes spanning most of the `int` range in 3D (undefined behaviour, a
    /// wrapping multiplication on the reference platform).
    pub fn child_area(&self, i: usize) -> i64 {
        let mut area: i64 = 1;

        for d in 0..D {
            area = area.wrapping_mul(self.max[i][d] as i64 - self.min[i][d] as i64);
        }

        area
    }

    /// Port of `ChildOverlaps`: whether child slot `i`'s bounding box overlaps the query box.
    pub fn child_overlaps(&self, i: usize, min: &[i32; D], max: &[i32; D]) -> bool {
        for d in 0..D {
            if self.min[i][d] > max[d] || self.max[i][d] < min[d] {
                return false;
            }
        }

        true
    }

    /// Port of `ChildOverlapMask`: bitmask of children whose bounding boxes overlap the query
    /// box.  Bit `i` is set if child `i` overlaps.
    pub fn child_overlap_mask(&self, min: &[i32; D], max: &[i32; D]) -> u32 {
        overlap_mask(&self.min, &self.max, self.count, min, max)
    }

    /// Port of `ChildOverlapArea`: the overlap area between child slot `i` and the given box.
    pub fn child_overlap_area(&self, i: usize, min: &[i32; D], max: &[i32; D]) -> i64 {
        let mut overlap: i64 = 1;

        for d in 0..D {
            let lo = self.min[i][d].max(min[d]);
            let hi = self.max[i][d].min(max[d]);

            if lo > hi {
                return 0;
            }

            overlap = overlap.wrapping_mul(hi as i64 - lo as i64);
        }

        overlap
    }

    /// Port of `ChildEnlargement`: how much child slot `i`'s area would increase if it were
    /// enlarged to include the given bounding box.
    pub fn child_enlargement(&self, i: usize, min: &[i32; D], max: &[i32; D]) -> i64 {
        let original_area = self.child_area(i);
        let mut enlarged_area: i64 = 1;

        for d in 0..D {
            let lo = self.min[i][d].min(min[d]);
            let hi = self.max[i][d].max(max[d]);
            enlarged_area = enlarged_area.wrapping_mul(hi as i64 - lo as i64);
        }

        enlarged_area.wrapping_sub(original_area)
    }

    /// Port of `ChildPerimeter`: the perimeter (or margin for 3D) of child slot `i`'s bounding
    /// box.  Used for split axis selection in R*-tree.
    pub fn child_perimeter(&self, i: usize) -> i64 {
        let mut perimeter: i64 = 0;

        for d in 0..D {
            perimeter += self.max[i][d] as i64 - self.min[i][d] as i64;
        }

        2 * perimeter
    }

    /// Port of `SetChildBounds`.
    pub fn set_child_bounds(&mut self, i: usize, min: &[i32; D], max: &[i32; D]) {
        self.min[i] = *min;
        self.max[i] = *max;
    }

    /// Port of `GetChildBounds`: `( min, max )` of child slot `i`.
    pub fn get_child_bounds(&self, i: usize) -> ([i32; D], [i32; D]) {
        (self.min[i], self.max[i])
    }

    /// Port of `SetInsertBounds`: store the insertion bounding box for leaf entry `i`.
    pub fn set_insert_bounds(&mut self, i: usize, min: &[i32; D], max: &[i32; D]) {
        self.insert_min[i] = *min;
        self.insert_max[i] = *max;
    }

    /// Port of `GetInsertBounds`: the stored insertion bounding box for leaf entry `i`.
    pub fn get_insert_bounds(&self, i: usize) -> ([i32; D], [i32; D]) {
        (self.insert_min[i], self.insert_max[i])
    }

    /// Port of `RemoveChild`: remove the child at slot `i` by moving the last entry into it.
    pub fn remove_child(&mut self, i: usize) {
        debug_assert!(i < self.count);

        if i >= self.count {
            return;
        }

        let last = self.count - 1;

        if i != last {
            self.min[i] = self.min[last];
            self.max[i] = self.max[last];
            self.insert_min[i] = self.insert_min[last];
            self.insert_max[i] = self.insert_max[last];

            if !self.is_leaf() {
                self.children[i] = self.children[last];
            }
        }

        if self.is_leaf() {
            // data[i] = data[last]
            self.data.swap_remove(i);
        }

        self.count -= 1;
    }

    /// Appends a leaf entry in slot `count` (the C++ code writes the slot's bounds, insertion
    /// bounds and data, then increments `count`).
    pub(crate) fn push_leaf_entry(
        &mut self,
        min: &[i32; D],
        max: &[i32; D],
        insert_min: &[i32; D],
        insert_max: &[i32; D],
        data: T,
    ) {
        let slot = self.count;
        self.min[slot] = *min;
        self.max[slot] = *max;
        self.insert_min[slot] = *insert_min;
        self.insert_max[slot] = *insert_max;
        self.data.push(data);
        self.count += 1;
    }

    /// Appends a child node in slot `count`.
    pub(crate) fn push_child_entry(&mut self, min: &[i32; D], max: &[i32; D], child: NodeId) {
        let slot = self.count;
        self.min[slot] = *min;
        self.max[slot] = *max;
        self.children[slot] = child;
        self.count += 1;
    }

    /// `count = 0` before a node is refilled.
    pub(crate) fn clear_entries(&mut self) {
        self.count = 0;
        self.data.clear();
    }
}

/// The node store of a tree: the port of `KIRTREE::SLAB_ALLOCATOR<NODE>`.
///
/// Freed nodes are recycled via a free list, like in the C++ allocator.
#[derive(Clone, Debug)]
pub struct NodeArena<T, const D: usize, const M: usize> {
    nodes: Vec<RtreeNode<T, D, M>>,
    free_list: Vec<NodeId>,
}

impl<T, const D: usize, const M: usize> Default for NodeArena<T, D, M> {
    fn default() -> Self {
        NodeArena {
            nodes: Vec::new(),
            free_list: Vec::new(),
        }
    }
}

impl<T, const D: usize, const M: usize> NodeArena<T, D, M> {
    /// `SLAB_ALLOCATOR::NODES_PER_PAGE`
    pub const NODES_PER_PAGE: usize = 256;

    /// Port of `Allocate`: a new, empty node, either from the free list or from new storage.
    pub fn allocate(&mut self) -> NodeId {
        if let Some(id) = self.free_list.pop() {
            self.nodes[id as usize] = RtreeNode::new();
            return id;
        }

        self.nodes.push(RtreeNode::new());
        (self.nodes.len() - 1) as NodeId
    }

    /// Port of `Free`: return a node to the free list for reuse.
    pub fn free(&mut self, id: NodeId) {
        if id == NO_NODE {
            return;
        }

        // aNode->~NODE()
        self.nodes[id as usize].clear_entries();
        self.nodes[id as usize].data = Vec::new();
        self.free_list.push(id);
    }

    /// Port of `MemoryUsage`: approximate memory usage in bytes (whole pages of nodes plus the
    /// free list, like the C++; the heap storage of leaf data is added).
    pub fn memory_usage(&self) -> usize {
        let pages = self.nodes.len().div_ceil(Self::NODES_PER_PAGE);
        let data: usize = self
            .nodes
            .iter()
            .map(|n| n.data.capacity() * std::mem::size_of::<T>())
            .sum();

        pages * Self::NODES_PER_PAGE * std::mem::size_of::<RtreeNode<T, D, M>>()
            + self.free_list.capacity() * std::mem::size_of::<NodeId>()
            + data
    }

    /// The node with the given id.
    pub fn node(&self, id: NodeId) -> &RtreeNode<T, D, M> {
        &self.nodes[id as usize]
    }

    /// The node with the given id, mutably.
    pub fn node_mut(&mut self, id: NodeId) -> &mut RtreeNode<T, D, M> {
        &mut self.nodes[id as usize]
    }

    /// Drops all nodes.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.free_list.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hilbert_2d_basics() {
        assert_eq!(hilbert_xy2d(32, 0, 0), 0);
        // order 1: (0,0) (0,1) (1,1) (1,0)
        assert_eq!(hilbert_xy2d(1, 0, 0), 0);
        assert_eq!(hilbert_xy2d(1, 0, 1), 1);
        assert_eq!(hilbert_xy2d(1, 1, 1), 2);
        assert_eq!(hilbert_xy2d(1, 1, 0), 3);
        assert_eq!(hilbert_xy2d(32, u32::MAX, 0), u64::MAX);
        assert_eq!(
            hilbert_nd2d::<2>(32, &[123, 456]),
            hilbert_xy2d(32, 123, 456)
        );

        // order 2 is a bijection onto 0..16
        let mut seen = [false; 16];

        for x in 0..4 {
            for y in 0..4 {
                seen[hilbert_xy2d(2, x, y) as usize] = true;
            }
        }

        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn node_helpers() {
        let mut n: RtreeNode<u32, 2, 16> = RtreeNode::new();
        assert_eq!(RtreeNode::<u32, 2, 16>::MINNODES, 6);
        n.push_leaf_entry(&[0, 0], &[10, 20], &[0, 0], &[10, 20], 7);
        n.push_leaf_entry(&[5, 5], &[30, 6], &[5, 5], &[30, 6], 8);
        n.push_leaf_entry(&[-5, -5], &[-1, -1], &[-5, -5], &[-1, -1], 9);
        assert_eq!(n.compute_enclosing_bounds(), ([-5, -5], [30, 20]));
        assert_eq!(n.child_area(0), 200);
        assert_eq!(n.child_perimeter(0), 60);
        assert_eq!(n.child_overlap_area(0, &[5, 5], &[30, 6]), 5);
        assert_eq!(n.child_overlap_area(0, &[11, 5], &[30, 6]), 0);
        assert_eq!(n.child_enlargement(0, &[5, 5], &[30, 6]), 600 - 200);
        assert_eq!(n.child_overlap_mask(&[10, 6], &[10, 6]), 0b011);
        assert!(n.child_overlaps(2, &[-1, -1], &[0, 0]));
        n.remove_child(0);
        assert_eq!(n.count(), 2);
        assert_eq!(n.data, [9, 8]);
        assert_eq!(n.get_child_bounds(0), ([-5, -5], [-1, -1]));
        assert_eq!(n.get_insert_bounds(1), ([5, 5], [30, 6]));
    }
}
