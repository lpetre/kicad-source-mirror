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

//! Port of `pcbnew/connectivity/connectivity_rtree.h`: `CN_RTREE`, an r-tree for fast spatial
//! indexing of connectivity items over `{ copper layer ordinal, x, y }` boxes.
//!
//! The C++ template takes the item and reads `BBox()`, `StartLayer()` and `EndLayer()` from it;
//! the port takes those three values explicitly next to the (copyable) payload, so that the tree
//! does not borrow the item list that owns the items.

use kicad_geom::geometry::rtree::dynamic_rtree::{BulkEntry, DynamicRTree};
use kicad_geom::math::box2::Box2I;

/// Port of `CN_RTREE<T>`: `KIRTREE::DYNAMIC_RTREE<T, int, 3>` keyed by
/// `{ start layer, x, y } .. { end layer, right, bottom }`.  Non-owning (the payload is a
/// handle).  The layer bounds are copper layer **ordinals** (F_Cu = 0, B_Cu = 31), the same as
/// `CN_ITEM::StartLayer()` / `EndLayer()`.
pub struct CnRtree<T> {
    /// `m_tree`
    tree: DynamicRTree<T, 3>,
}

impl<T> Default for CnRtree<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Port of `CN_RTREE::BULK_ENTRY`: entry type accepted by [`CnRtree::bulk_load`].
pub type CnBulkEntry<T> = BulkEntry<T, 3>;

/// The `{ layer, x, y }` box of an item, as `CN_RTREE::Insert` / `Remove` / `Query` build it.
fn bounds(bbox: &Box2I, start_layer: i32, end_layer: i32) -> ([i32; 3], [i32; 3]) {
    (
        [start_layer, bbox.x(), bbox.y()],
        [end_layer, bbox.right(), bbox.bottom()],
    )
}

impl<T> CnRtree<T> {
    /// `CN_RTREE()`: an empty tree.
    pub fn new() -> Self {
        CnRtree {
            tree: DynamicRTree::new(),
        }
    }

    /// Port of `CN_RTREE::Insert`: inserts an item with its bounding box and copper ordinal
    /// range.
    pub fn insert(&mut self, bbox: &Box2I, start_layer: i32, end_layer: i32, item: T) {
        let (mmin, mmax) = bounds(bbox, start_layer, end_layer);
        self.tree.insert(&mmin, &mmax, item);
    }

    /// Port of `CN_RTREE::RemoveAll`: removes all items from the tree.
    pub fn remove_all(&mut self) {
        self.tree.remove_all();
    }

    /// Port of `CN_RTREE::BulkLoad`: replaces the tree contents with `entries`, packed
    /// bottom-up.  Much cheaper than the same number of `insert()` calls, which each pay for an
    /// R*-tree subtree choice and may cascade into forced reinsertion.
    pub fn bulk_load(&mut self, entries: Vec<CnBulkEntry<T>>) {
        self.tree.bulk_load(entries);
    }

    /// Port of `CN_RTREE::Query`: calls `visitor` for each item whose box intersects `bounds`
    /// within the copper ordinal range `start_layer ..= end_layer`, in tree order.  The visitor
    /// returns `false` to stop the search (the C++ visitor's `bool`).
    pub fn query<F>(&self, bbox: &Box2I, start_layer: i32, end_layer: i32, visitor: F)
    where
        F: FnMut(&T) -> bool,
    {
        // The layer bounds are copper layer ordinals, the same as Insert() stores.
        let (mmin, mmax) = bounds(bbox, start_layer, end_layer);
        self.tree.search(&mmin, &mmax, visitor);
    }

    /// Number of items in the tree.
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// `true` when the tree holds no item.
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }
}

impl<T: PartialEq> CnRtree<T> {
    /// Port of `CN_RTREE::Remove`: removes an item from the tree.  Removal compares payloads
    /// (C++: pointers).  `DYNAMIC_RTREE::Remove` tries the provided box first, then falls back
    /// to a full-tree search if the item has moved since insertion.
    pub fn remove(&mut self, bbox: &Box2I, start_layer: i32, end_layer: i32, item: &T) {
        let (mmin, mmax) = bounds(bbox, start_layer, end_layer);
        self.tree.remove(&mmin, &mmax, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::math::vector2::Vector2I;

    fn bx(x: i32, y: i32, w: i32, h: i32) -> Box2I {
        Box2I::new(Vector2I::new(x, y), Vector2I::new(w, h))
    }

    fn found(t: &CnRtree<u32>, b: &Box2I, s: i32, e: i32) -> Vec<u32> {
        let mut v = Vec::new();
        t.query(b, s, e, |&i| {
            v.push(i);
            true
        });
        v.sort_unstable();
        v
    }

    #[test]
    fn layer_ordinals_separate_items() {
        let mut t = CnRtree::new();
        t.insert(&bx(0, 0, 10, 10), 0, 0, 1); // F_Cu
        t.insert(&bx(0, 0, 10, 10), 31, 31, 2); // B_Cu
        t.insert(&bx(5, 5, 10, 10), 0, 31, 3); // through
        t.insert(&bx(100, 100, 1, 1), 0, 31, 4); // far away

        assert_eq!(found(&t, &bx(0, 0, 1, 1), 0, 0), vec![1]);
        assert_eq!(found(&t, &bx(0, 0, 6, 6), 0, 0), vec![1, 3]);
        assert_eq!(found(&t, &bx(0, 0, 6, 6), 31, 31), vec![2, 3]);
        assert_eq!(found(&t, &bx(0, 0, 6, 6), 1, 30), vec![3]);
        // touching boxes intersect (closed intervals)
        assert_eq!(found(&t, &bx(15, 15, 0, 0), 0, 31), vec![3]);

        t.remove(&bx(5, 5, 10, 10), 0, 31, &3);
        assert_eq!(found(&t, &bx(0, 0, 1, 1), 0, 31), vec![1, 2]);
        assert_eq!(t.len(), 3);

        let entries = vec![CnBulkEntry {
            min: [0, 0, 0],
            max: [31, 5, 5],
            data: 9,
        }];
        t.bulk_load(entries);
        assert_eq!(found(&t, &bx(0, 0, 1, 1), 2, 2), vec![9]);
        t.remove_all();
        assert!(t.is_empty());
    }
}
