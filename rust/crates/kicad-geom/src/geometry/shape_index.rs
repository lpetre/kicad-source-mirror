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

//! Port of `libs/kimath/include/geometry/shape_index.h` (`SHAPE_INDEX` and its helper templates
//! `shapeFunctor`, `boundingBox`, `acceptVisitor`, `collide`, `queryCallback`).
//!
//! The C++ class stores its items in a `KIRTREE::COW_RTREE` (copy-on-write variant of the
//! dynamic R*-tree, not ported, see `PORT_STATUS.md`); the port uses the ported
//! [`DynamicRTree`] with the same node size.  `Clone()` therefore makes a deep copy instead of
//! sharing nodes.  The item type `T` (C++: a pointer such as `PNS::ITEM*` or `SHAPE*`) provides
//! its shape through [`ShapeIndexItem`] (C++: `T::Shape( aLayer )`).

use std::sync::Arc;

use crate::geometry::rtree::dynamic_rtree::{BulkEntry, DynamicRTree};
use crate::geometry::shape::Shape;
use crate::math::box2::Box2I;

/// The shape of an indexed item (C++: the default `shapeFunctor()` / `defaultShapeFunctor()`,
/// which call `aItem->Shape( aLayer )`).  `None` is the C++ `nullptr`.
pub trait ShapeIndexItem {
    /// The shape of this item on layer `a_layer` (-1: any layer).
    fn index_shape(&self, a_layer: i32) -> Option<&dyn Shape>;
}

/// A shared shape compared by identity: the Rust counterpart of a `SHAPE*` item, as used by
/// `SHAPE_INDEX<SHAPE*>` (the default template argument).
#[derive(Debug, Clone)]
pub struct SharedShape(pub Arc<dyn Shape>);

impl PartialEq for SharedShape {
    fn eq(&self, a_other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &a_other.0)
    }
}

impl ShapeIndexItem for SharedShape {
    fn index_shape(&self, _a_layer: i32) -> Option<&dyn Shape> {
        Some(self.0.as_ref())
    }
}

impl<S: ShapeIndexItem + ?Sized> ShapeIndexItem for &S {
    fn index_shape(&self, a_layer: i32) -> Option<&dyn Shape> {
        (**self).index_shape(a_layer)
    }
}

/// Port of the template `boundingBox()`: the bounding box of the item's shape, or an empty box
/// when it has none (`wxCHECK`).
pub fn bounding_box<T: ShapeIndexItem>(a_object: &T, a_layer: i32) -> Box2I {
    match a_object.index_shape(a_layer) {
        Some(shape) => shape.bbox(0),
        None => Box2I::default(),
    }
}

/// Port of the template `acceptVisitor()`.
pub fn accept_visitor<T, V: FnMut(&T)>(a_object: &T, a_visitor: &mut V) {
    a_visitor(a_object);
}

/// Port of the template `collide()`: whether the item's shape collides with `a_another_object`
/// within `a_min_distance` (false when it has no shape, `wxCHECK`).
pub fn collide<T: ShapeIndexItem>(
    a_object: &T,
    a_another_object: &dyn Shape,
    a_layer: i32,
    a_min_distance: i32,
) -> bool {
    match a_object.index_shape(a_layer) {
        Some(shape) => shape.collide(a_another_object, a_min_distance, None, None),
        None => false,
    }
}

/// Port of the template `queryCallback()`: visits the item and continues.
pub fn query_callback<T, V: FnMut(&T)>(a_shape: &T, a_context: &mut V) -> bool {
    accept_visitor(a_shape, a_context);
    true
}

/// Port of `SHAPE_INDEX<T>`: a spatial index of items with shapes.
#[derive(Debug)]
pub struct ShapeIndex<T> {
    tree: DynamicRTree<T, 2, 16>,
    shape_layer: i32,
}

/// The `[min]` / `[max]` arrays of a box (`GetX()`, `GetY()`, `GetRight()`, `GetBottom()`).
fn box_bounds(a_box: &Box2I) -> ([i32; 2], [i32; 2]) {
    ([a_box.x(), a_box.y()], [a_box.right(), a_box.bottom()])
}

impl<T: ShapeIndexItem> ShapeIndex<T> {
    /// Port of `SHAPE_INDEX( int aLayer )`.
    pub fn new(a_layer: i32) -> Self {
        Self {
            tree: DynamicRTree::new(),
            shape_layer: a_layer,
        }
    }

    /// Port of `Add( T aShape )`: inserts the item with the bounding box of its shape.
    pub fn add(&mut self, a_shape: T) {
        let bx = bounding_box(&a_shape, self.shape_layer);
        let (min, max) = box_bounds(&bx);

        self.tree.insert(&min, &max, a_shape);
    }

    /// Port of `Add( T aShape, const BOX2I& aBbox )`: inserts the item with a given box.
    pub fn add_with_bbox(&mut self, a_shape: T, a_bbox: &Box2I) {
        let (min, max) = box_bounds(a_bbox);

        self.tree.insert(&min, &max, a_shape);
    }

    /// Port of `RemoveAll()`.
    pub fn remove_all(&mut self) {
        self.tree.remove_all();
    }

    /// Port of `Accept( V aVisitor )`: visits every item in tree order.
    pub fn accept<V: FnMut(&T)>(&self, mut a_visitor: V) {
        for item in &self.tree {
            accept_visitor(item, &mut a_visitor);
        }
    }

    /// Port of `BulkLoad()`: (re)builds the index from items with their boxes.
    pub fn bulk_load(&mut self, a_items: Vec<(T, Box2I)>) {
        let entries: Vec<BulkEntry<T, 2>> = a_items
            .into_iter()
            .map(|(item, bx)| {
                let (min, max) = box_bounds(&bx);
                BulkEntry {
                    min,
                    max,
                    data: item,
                }
            })
            .collect();

        self.tree.bulk_load(entries);
    }

    /// Port of `Reindex()`: removes all items and re-adds them (in iteration order) with the
    /// current bounding boxes of their shapes.
    pub fn reindex(&mut self)
    where
        T: Clone,
    {
        let items: Vec<T> = self.tree.iter().cloned().collect();

        self.tree.remove_all();

        for item in items {
            self.add(item);
        }
    }

    /// Port of `Query( const SHAPE* aShape, int aMinDistance, V& aVisitor )`: visits the items
    /// whose boxes overlap the bounding box of `a_shape` inflated by `a_min_distance`.  The
    /// visitor returns false to stop.  Returns the number of items visited.
    pub fn query<V: FnMut(&T) -> bool>(
        &self,
        a_shape: &dyn Shape,
        a_min_distance: i32,
        a_visitor: V,
    ) -> i32 {
        let mut bx = a_shape.bbox(0);
        bx.inflate(a_min_distance);

        let (min, max) = box_bounds(&bx);

        self.tree.search(&min, &max, a_visitor)
    }

    /// Port of `Begin()` / `begin()`: iteration over all items in tree order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.tree.iter()
    }

    /// Port of `Size()`.
    pub fn size(&self) -> usize {
        self.tree.len()
    }

    /// The layer given to the constructor (`m_shapeLayer`).
    pub fn shape_layer(&self) -> i32 {
        self.shape_layer
    }
}

impl<T: ShapeIndexItem + PartialEq> ShapeIndex<T> {
    /// Port of `Remove( T aShape )`: removes the item, looked up with the bounding box of its
    /// shape.
    pub fn remove(&mut self, a_shape: &T) {
        let bx = bounding_box(a_shape, self.shape_layer);
        let (min, max) = box_bounds(&bx);

        self.tree.remove(&min, &max, a_shape);
    }
}

impl<T: ShapeIndexItem + Clone> ShapeIndex<T> {
    /// Port of `Clone()`: an independent copy of the index (C++: shares the nodes until one of
    /// them is modified; the observable contents are the same).
    pub fn clone_index(&self) -> Self {
        let mut clone = Self::new(self.shape_layer);
        let entries: Vec<(T, Box2I)> = self
            .tree
            .iter()
            .map(|item| (item.clone(), bounding_box(item, self.shape_layer)))
            .collect();

        for (item, bx) in entries {
            clone.add_with_bbox(item, &bx);
        }

        clone
    }
}
