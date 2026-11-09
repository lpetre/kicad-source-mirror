// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2013 CERN
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


//! Port of `libs/kimath/include/geometry/shape_index_list.h` (`SHAPE_INDEX_LIST`): a plain
//! list of items with shapes and cached bounding boxes, queried linearly in insertion order.
//!
//! The shape functor (C++ template argument, default `aItem->Shape( -1 )`) is
//! [`ShapeIndexItem::index_shape`] with layer -1.  The C++ caches the `SHAPE*` in each entry;
//! the port asks the item again where the C++ dereferences the cached pointer.

use crate::geometry::shape::Shape;
use crate::geometry::shape_index::ShapeIndexItem;
use crate::math::box2::Box2I;

/// Port of `SHAPE_INDEX_LIST::SHAPE_ENTRY`.
#[derive(Debug, Clone)]
pub struct ShapeEntry<T> {
    /// The item (`parent`).
    pub parent: T,
    /// The bounding box of its shape when it was added (`bbox`).
    pub bbox: Box2I,
}

impl<T: ShapeIndexItem> ShapeEntry<T> {
    /// Port of `SHAPE_ENTRY( T aParent )`.
    ///
    /// The C++ dereferences the functor's result unconditionally; an item without a shape gets
    /// an empty box here.
    pub fn new(a_parent: T) -> Self {
        let bbox = match a_parent.index_shape(-1) {
            Some(shape) => shape.bbox(0),
            None => Box2I::default(),
        };

        Self {
            parent: a_parent,
            bbox,
        }
    }

    fn shape(&self) -> Option<&dyn Shape> {
        self.parent.index_shape(-1)
    }
}

/// Port of `SHAPE_INDEX_LIST<T>`.
#[derive(Debug, Clone)]
pub struct ShapeIndexList<T> {
    shapes: Vec<ShapeEntry<T>>,
}

impl<T> Default for ShapeIndexList<T> {
    fn default() -> Self {
        Self { shapes: Vec::new() }
    }
}

/// Port of `SHAPE_INDEX_LIST::query_iterator`: iterates over the items whose cached box lies
/// within `min_distance` of the reference shape's box (and, if `exact`, whose shape collides
/// with it).
pub struct QueryIterator<'a, T> {
    entries: std::slice::Iter<'a, ShapeEntry<T>>,
    shape: &'a dyn Shape,
    ref_bbox: Box2I,
    min_distance: i32,
    exact: bool,
}

impl<'a, T: ShapeIndexItem> Iterator for QueryIterator<'a, T> {
    type Item = &'a T;

    /// Port of `query_iterator::next()` / `operator++`.
    fn next(&mut self) -> Option<&'a T> {
        for entry in self.entries.by_ref() {
            if self.ref_bbox.distance_box(&entry.bbox) <= self.min_distance as i64
                && (!self.exact
                    || entry
                        .shape()
                        .is_some_and(|s| s.collide(self.shape, self.min_distance, None, None)))
            {
                return Some(&entry.parent);
            }
        }

        None
    }
}

impl<T: ShapeIndexItem> ShapeIndexList<T> {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `Add( T aItem )`.
    pub fn add(&mut self, a_item: T) {
        self.shapes.push(ShapeEntry::new(a_item));
    }

    /// Port of `Size()`.
    pub fn size(&self) -> i32 {
        self.shapes.len() as i32
    }

    /// Port of `Query( const SHAPE* aShape, int aMinDistance, Visitor& aV, bool aExact )`:
    /// visits, in insertion order, the items whose box is within `a_min_distance` of the box of
    /// `a_shape` (and that collide with it if `a_exact`).  The visitor returns false to stop.
    /// Returns the number of items visited.
    pub fn query<V: FnMut(&T) -> bool>(
        &self,
        a_shape: &dyn Shape,
        a_min_distance: i32,
        mut a_v: V,
        a_exact: bool,
    ) -> i32 {
        let mut n = 0;
        let min_dist_sq: i64 = a_min_distance as i64 * a_min_distance as i64;

        let ref_bbox = a_shape.bbox(0);

        for entry in &self.shapes {
            if ref_bbox.squared_distance_box(&entry.bbox) <= min_dist_sq
                && (!a_exact
                    || entry
                        .shape()
                        .is_some_and(|s| s.collide(a_shape, a_min_distance, None, None)))
            {
                n += 1;

                if !a_v(&entry.parent) {
                    return n;
                }
            }
        }

        n
    }

    /// Port of `Clear()`.
    pub fn clear(&mut self) {
        self.shapes.clear();
    }

    /// Port of `qbegin()` (with `qend()` as the end of the iterator).
    pub fn qbegin<'a>(
        &'a self,
        a_shape: &'a dyn Shape,
        a_min_distance: i32,
        a_exact: bool,
    ) -> QueryIterator<'a, T> {
        QueryIterator {
            entries: self.shapes.iter(),
            shape: a_shape,
            ref_bbox: a_shape.bbox(0),
            min_distance: a_min_distance,
            exact: a_exact,
        }
    }

    /// Port of `begin()` / `end()`: the items in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.shapes.iter().map(|e| &e.parent)
    }
}

impl<T: ShapeIndexItem + PartialEq> ShapeIndexList<T> {
    /// Port of `Remove( const T aItem )`: removes the first entry of `a_item`.
    pub fn remove(&mut self, a_item: &T) {
        if let Some(i) = self.shapes.iter().position(|e| e.parent == *a_item) {
            self.shapes.remove(i);
        }
    }
}
