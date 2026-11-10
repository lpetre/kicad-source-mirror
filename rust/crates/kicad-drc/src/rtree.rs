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

//! Port of `pcbnew/drc/drc_rtree.h` (`DRC_RTREE`).
//!
//! A non-owning spatial index of board items, one static Hilbert-packed r-tree
//! ([`PackedRTree`], the C++ `KIRTREE::PACKED_RTREE<ITEM_WITH_SHAPE*, int, 2>`) per layer.
//! The search order of the packed tree is result-relevant (first hits stop many loops); the
//! kicad-geom port reproduces the C++ visit order, including the libstdc++ `std::sort` tie
//! order of the Hilbert sort.
//!
//! `ITEM_WITH_SHAPE` keeps a raw pointer into its parent's effective shape. Here an entry keeps
//! the parent's handle, the parent shape and the index of the sub-shape
//! (`board-model.md` section 13); [`ItemWithShape::shape`] resolves it with
//! `Shape::indexable_subshape`. Visitors and filters receive the parent's [`ItemId`] (C++
//! `BOARD_ITEM*`) and resolve it with `ctx.item(id)` when they need the item.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use kicad_core::layer_ids::{Flashing, PcbLayerId, UNDEFINED_LAYER};
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use kicad_geom::geometry::shape_poly_set::Tri;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::item::SharedShape;
use kicad_pcb::{ItemId, ItemRef};

use crate::constraint_types::DrcConstraintT;

/// `ATOMIC_TABLES`: value for the `atomic_tables` argument of [`DrcRtree::insert`].
pub const ATOMIC_TABLES: bool = true;

/// Port of `DRC_RTREE::ITEM_WITH_SHAPE`: one tree entry.
///
/// C++ keeps `shape` (a raw pointer to the indexed shape, which is either the parent shape,
/// one of its indexable sub-shapes, or the separately owned `shapeStorage`), `shapeStorage`
/// and `parentShape`. Here `shape` is derived: [`ItemWithShape::shape`].
#[derive(Clone, Debug)]
pub struct ItemWithShape {
    /// `parent`: the item (for a table cell inserted with atomic tables: its table).
    pub parent: ItemId,
    /// `parentShape`: the whole effective shape of the item the entry was built from.
    pub parent_shape: SharedShape,
    /// Index of the indexed sub-shape in `parent_shape` (`GetIndexableSubshapes()` order), or
    /// `None` when the entry is the whole parent shape (or the `shape_storage`).
    pub sub: Option<u32>,
    /// `shapeStorage`: a shape that is not part of `parent_shape` (the hole of a pad).
    pub shape_storage: Option<SharedShape>,
}

impl ItemWithShape {
    /// `ITEM_WITH_SHAPE::shape`: the indexed shape.
    pub fn shape(&self) -> &dyn Shape {
        if let Some(storage) = &self.shape_storage {
            return storage.as_ref();
        }

        match self.sub {
            None => self.parent_shape.as_ref(),
            Some(i) => self
                .parent_shape
                .indexable_subshape(i as usize)
                .expect("DRC_RTREE entry refers to an existing sub-shape"),
        }
    }
}

/// `DRC_RTREE::LAYER_PAIR`: (reference layer, target layer).
pub type LayerPair = (PcbLayerId, PcbLayerId);

/// The payload is the index of the entry in `DrcRtree::owned` (C++: the `ITEM_WITH_SHAPE*`
/// owned by `m_owned`).
type DrcTree = PackedRTree<u32, 2, 16>;
type DrcTreeBuilder = PackedRTreeBuilder<u32, 2, 16>;

/// Port of `DRC_RTREE`.
///
/// Inserts are staged in per-layer builders; [`DrcRtree::build`] bulk-loads the packed trees.
/// The tree owns `Arc`s of the item shapes, so it is `Send + Sync` and can live in the per-run
/// caches.
#[derive(Clone, Debug, Default)]
pub struct DrcRtree {
    /// `m_tree`
    tree: BTreeMap<i32, DrcTree>,
    /// `m_builders`
    builders: BTreeMap<i32, DrcTreeBuilder>,
    /// `m_minPadding`
    min_padding: BTreeMap<i32, i32>,
    /// `m_owned`
    owned: Vec<ItemWithShape>,
    /// `m_count`
    count: usize,
}

/// The query box `{ x, y }`, `{ right, bottom }` of a [`Box2I`].
fn min_max(b: &Box2I) -> ([i32; 2], [i32; 2]) {
    ([b.x(), b.y()], [b.right(), b.bottom()])
}

/// Whether `shape` stands for the null `std::shared_ptr<SHAPE>` the C++ `PAD::GetEffectiveShape`
/// returns for a layer without a cached shape (kicad-pcb returns a bare `SHAPE_NULL` there; the
/// pad never returns a bare `SHAPE_NULL` otherwise).
fn is_null_pad_shape(item: &ItemRef<'_>, shape: &SharedShape) -> bool {
    item.type_() == KicadT::PcbPadT && shape.type_() == ShapeType::Null
}

impl DrcRtree {
    /// `DRC_RTREE()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `Insert( aItem, aLayer, aConstraintType, aWorstClearance, aAtomicTables )`: the
    /// same layer is the reference and the target layer.
    pub fn insert(
        &mut self,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        constraint_type: DrcConstraintT,
        worst_clearance: i32,
        atomic_tables: bool,
    ) {
        self.insert_on(
            item,
            layer,
            layer,
            constraint_type,
            worst_clearance,
            atomic_tables,
        );
    }

    /// Port of `Insert( aItem, aRefLayer, aTargetLayer, aConstraintType, aWorstClearance,
    /// aAtomicTables )`: the shape is taken on `ref_layer` and indexed on `target_layer`.
    ///
    /// An item without a handle (a detached value) cannot be referred to by a tree entry; it is
    /// skipped (debug builds assert).
    pub fn insert_on(
        &mut self,
        item: ItemRef<'_>,
        ref_layer: PcbLayerId,
        target_layer: PcbLayerId,
        constraint_type: DrcConstraintT,
        worst_clearance: i32,
        atomic_tables: bool,
    ) {
        // wxCHECK( aTargetLayer != UNDEFINED_LAYER, /* void */ );
        if target_layer == UNDEFINED_LAYER {
            debug_assert!(false, "DRC_RTREE::Insert on UNDEFINED_LAYER");
            return;
        }

        // wxCHECK_MSG( !m_tree.count( aTargetLayer ), ..., "Insert after Build() is silently
        // wrong" )
        if self.tree.contains_key(&target_layer.0) {
            debug_assert!(false, "Insert after Build() is silently wrong");
            return;
        }

        if item.type_() == KicadT::PcbFieldT
            && !item
                .as_eda_text()
                .is_some_and(|text| text.eda_text().is_visible())
        {
            return;
        }

        let mut parent = item.id();

        if atomic_tables && item.type_() == KicadT::PcbTablecellT {
            parent = item.base().parent.item();
        }

        let Some(parent) = parent else {
            debug_assert!(false, "DRC_RTREE::Insert of an item without a handle");
            return;
        };

        let shape: SharedShape =
            item.effective_shape(ref_layer, Flashing::Default, constraint_type);

        // wxCHECK2_MSG( shape, return, "Item does not have a valid shape for this layer" ):
        // `PAD::GetEffectiveShape()` returns null when a layer has no cached shape, which
        // kicad-pcb represents as a bare null shape. Returning here also skips the hole entry,
        // like the C++.
        if is_null_pad_shape(&item, &shape) {
            return;
        }

        let entries: Vec<Option<u32>> = if shape.has_indexable_subshapes() {
            (0..shape.indexable_subshape_count() as u32)
                .map(Some)
                .collect()
        } else {
            vec![None]
        };

        for sub in entries {
            let entry = ItemWithShape {
                parent,
                parent_shape: Arc::clone(&shape),
                sub,
                shape_storage: None,
            };

            if entry.shape().type_() == ShapeType::Null {
                continue;
            }

            let mut bbox = entry.shape().bbox(0);
            bbox.inflate(worst_clearance);

            self.add_entry(target_layer, &bbox, entry, worst_clearance);
        }

        if item.type_() == KicadT::PcbPadT && item.has_hole() {
            let Some(hole) =
                item.effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::NullConstraint)
            else {
                // C++ dereferences the result; a pad with a hole always has one.
                debug_assert!(false, "pad with a hole has no hole shape");
                return;
            };

            let mut bbox = hole.bbox(0);
            bbox.inflate(worst_clearance);

            let entry = ItemWithShape {
                parent,
                parent_shape: Arc::clone(&shape),
                sub: None,
                shape_storage: Some(hole),
            };

            self.add_entry(target_layer, &bbox, entry, worst_clearance);
        }
    }

    /// The staging part shared by both entries of `Insert`.
    fn add_entry(
        &mut self,
        target_layer: PcbLayerId,
        bbox: &Box2I,
        entry: ItemWithShape,
        worst_clearance: i32,
    ) {
        let (mmin, mmax) = min_max(bbox);
        let index = self.owned.len() as u32;

        self.owned.push(entry);
        self.builders
            .entry(target_layer.0)
            .or_default()
            .add(&mmin, &mmax, index);
        self.record_padding(target_layer, worst_clearance);
        self.count += 1;
    }

    /// Port of `Build`: finalises all pending inserts by bulk-building the packed r-trees.
    pub fn build(&mut self) {
        for (layer, builder) in self.builders.iter_mut() {
            self.tree.insert(*layer, builder.build());
        }

        self.builders.clear();
    }

    /// Port of `clear`: removes all items.
    pub fn clear(&mut self) {
        self.owned.clear();
        self.tree.clear();
        self.builders.clear();
        self.min_padding.clear();
        self.count = 0;
    }

    /// Port of `CheckColliding`: whether `ref_shape` collides (within `clearance`) with any
    /// entry on `target_layer` whose parent passes `filter`.
    pub fn check_colliding(
        &self,
        ref_shape: &dyn Shape,
        target_layer: PcbLayerId,
        clearance: i32,
        mut filter: Option<&mut dyn FnMut(ItemId) -> bool>,
    ) -> bool {
        let mut b = ref_shape.bbox(0);
        b.inflate(clearance);

        let (min, max) = min_max(&b);
        let mut count = 0;

        let mut visit = |item: &ItemWithShape| -> bool {
            if filter.as_mut().is_none_or(|f| f(item.parent)) {
                let mut actual = 0;

                if ref_shape.collide(item.shape(), clearance, Some(&mut actual), None) {
                    count += 1;
                    return false;
                }
            }

            true
        };

        if let Some(tree) = self.tree.get(&target_layer.0) {
            tree.search(&min, &max, |&i: &u32| visit(&self.owned[i as usize]));
        }

        count > 0
    }

    /// Port of `QueryColliding( aRefItem, aRefLayer, aTargetLayer, aFilter, aVisitor,
    /// aClearance )`: counts the items on `target_layer` whose shapes collide with the effective
    /// shape of `ref_item` on `ref_layer`. Each parent is counted (and visited) once; the
    /// filter is asked once per parent. A visitor returning false stops the search.
    pub fn query_colliding(
        &self,
        ref_item: ItemRef<'_>,
        ref_layer: PcbLayerId,
        target_layer: PcbLayerId,
        mut filter: Option<&mut dyn FnMut(ItemId) -> bool>,
        mut visitor: Option<&mut dyn FnMut(ItemId) -> bool>,
        clearance: i32,
    ) -> i32 {
        // keep track of BOARD_ITEMs that have already been found to collide (some items might
        // be built of COMPOUND/triangulated shapes and a single subshape collision means we have
        // a hit)
        let mut colliding_compounds: HashSet<ItemId> = HashSet::new();

        // keep track of results of client filter so we don't ask more than once for compound
        // shapes
        let mut filter_results: HashMap<ItemId, bool> = HashMap::new();

        let ref_shape =
            ref_item.effective_shape(ref_layer, Flashing::Default, DrcConstraintT::NullConstraint);
        let ref_id = ref_item.id();

        // A mask aperture reaches past the item's copper bounding box, so cover both
        let mut b = ref_item.bounding_box();
        b.merge(&ref_shape.bbox(0));
        b.inflate(clearance);

        let (min, max) = min_max(&b);
        let mut count = 0;

        let mut visit = |item: &ItemWithShape| -> bool {
            if Some(item.parent) == ref_id {
                return true;
            }

            if colliding_compounds.contains(&item.parent) {
                return true;
            }

            let filtered = match filter_results.get(&item.parent) {
                Some(&f) => f,
                None => {
                    let f = filter.as_mut().is_some_and(|f| !f(item.parent));
                    filter_results.insert(item.parent, f);
                    f
                }
            };

            if filtered {
                return true;
            }

            if ref_shape.collide(item.shape(), clearance, None, None) {
                colliding_compounds.insert(item.parent);
                count += 1;

                if let Some(v) = visitor.as_mut() {
                    return v(item.parent);
                }
            }

            true
        };

        if let Some(tree) = self.tree.get(&target_layer.0) {
            tree.search(&min, &max, |&i: &u32| visit(&self.owned[i as usize]));
        }

        count
    }

    /// Port of `QueryCollidingPreparedCopper`: the broad phase of
    /// [`DrcRtree::query_colliding`] with a caller-supplied reference shape; the visitor also
    /// receives the tree-owned effective shape of the colliding item's parent.
    ///
    /// Entry boxes were already inflated by the worst clearance passed to `insert`, so with
    /// `use_padding_credit` the query box only has to make up the difference. Opting in
    /// requires a side-effect-free `filter` and a `ref_item` whose bounding box contains
    /// `ref_shape`'s.
    pub fn query_colliding_prepared_copper(
        &self,
        ref_item: ItemRef<'_>,
        ref_shape: &SharedShape,
        target_layer: PcbLayerId,
        mut filter: Option<&mut dyn FnMut(ItemId) -> bool>,
        mut visitor: Option<&mut dyn FnMut(ItemId, &SharedShape) -> bool>,
        clearance: i32,
        use_padding_credit: bool,
    ) -> i32 {
        // PAD::GetEffectiveShape() returns null when a layer has no cached shape
        // (wxCHECK( aRefShape, 0 ); see `is_null_pad_shape`)
        if is_null_pad_shape(&ref_item, ref_shape) {
            return 0;
        }

        let mut colliding_compounds: HashSet<ItemId> = HashSet::new();
        let mut filter_results: HashMap<ItemId, bool> = HashMap::new();
        let mut b = ref_item.bounding_box();
        let mut inflation = clearance;
        let ref_id = ref_item.id();

        if use_padding_credit && clearance >= 0 {
            // Checked before the box grows, because the credit can leave inflation at 0.  A
            // reference whose bounds do not contain its own shape would shrink the query past a
            // real candidate and silently drop a clearance violation, so it pays full inflation
            // instead of getting a wrong answer in a release build
            let contained = b.contains_box(&ref_shape.bbox(0));

            debug_assert!(
                contained,
                "Padding credit needs the item bbox to contain its own shape"
            );

            if let Some(&padding) = self.min_padding.get(&target_layer.0)
                && contained
                && padding >= 0
            {
                inflation = 0.max(clearance - padding);
            }
        }

        b.inflate(inflation);

        let (min, max) = min_max(&b);
        let mut count = 0;

        let mut visit = |item: &ItemWithShape| -> bool {
            if Some(item.parent) == ref_id || colliding_compounds.contains(&item.parent) {
                return true;
            }

            let filtered = *filter_results
                .entry(item.parent)
                .or_insert_with(|| filter.as_mut().is_some_and(|f| !f(item.parent)));

            if filtered {
                return true;
            }

            if ref_shape.collide(item.shape(), clearance, None, None) {
                colliding_compounds.insert(item.parent);
                count += 1;

                if let Some(v) = visitor.as_mut() {
                    return v(item.parent, &item.parent_shape);
                }
            }

            true
        };

        if let Some(tree) = self.tree.get(&target_layer.0) {
            tree.search(&min, &max, |&i: &u32| visit(&self.owned[i as usize]));
        }

        count
    }

    /// Port of `QueryColliding( aBox, aRefShape, aLayer, aClearance, aActual, aPos )`, for
    /// tessellated items (all shapes in the tree come from a single item): the minimal actual
    /// distance over all colliding entries and its position (the first minimum in search order
    /// wins), clamped at 0. `None` when nothing collides. Stops at the first true collision.
    pub fn query_colliding_box_actual(
        &self,
        a_box: &Box2I,
        ref_shape: &dyn Shape,
        layer: PcbLayerId,
        clearance: i32,
    ) -> Option<(i32, Vector2I)> {
        let mut bbox = *a_box;
        bbox.inflate(clearance);

        let (min, max) = min_max(&bbox);

        let mut collision = false;
        let mut actual = i32::MAX;
        let mut pos = Vector2I::default();

        let mut visit = |item: &ItemWithShape| -> bool {
            let mut cur_actual = 0;
            let mut cur_pos = Vector2I::default();

            if ref_shape.collide(
                item.shape(),
                clearance,
                Some(&mut cur_actual),
                Some(&mut cur_pos),
            ) {
                collision = true;

                if cur_actual < actual {
                    actual = cur_actual;
                    pos = cur_pos;
                }

                // Stop looking after we have a true collision
                if actual <= 0 {
                    return false;
                }
            }

            true
        };

        if let Some(tree) = self.tree.get(&layer.0) {
            tree.search(&min, &max, |&i: &u32| visit(&self.owned[i as usize]));
        }

        if collision {
            Some((0.max(actual), pos))
        } else {
            None
        }
    }

    /// Port of `QueryColliding( aBox, aRefShape, aLayer )`: the quicker yes / no version.
    /// A single-outline polygon without holes is tested against fill triangles with segment
    /// tests and a point-inside test instead of a full collision.
    pub fn query_colliding_box(
        &self,
        a_box: &Box2I,
        ref_shape: &dyn Shape,
        layer: PcbLayerId,
    ) -> bool {
        let poly = match ref_shape.as_shape_ref() {
            ShapeRef::PolySet(p) => Some(p),
            _ => None,
        };

        let (min, max) = min_max(a_box);
        let mut collision = false;

        let Some(tree) = self.tree.get(&layer.0) else {
            return false;
        };

        if let Some(poly) = poly
            && poly.outline_count() == 1
            && poly.hole_count(0) == 0
        {
            // Special case the polygon case.  Otherwise we'll call its Collide() method which
            // will triangulate it as well and then do triangle/triangle collisions.  This ends
            // up being *much* slower than 3 segment Collide()s and a PointInside().
            let outline = poly.c_outline(0);

            tree.search(&min, &max, |&i: &u32| -> bool {
                let item = &self.owned[i as usize];
                // There are certain degenerate cases that result in empty zone fills, which
                // will be represented in the rtree with only a root (and no triangles).
                // https://gitlab.com/kicad/code/kicad/-/issues/18600
                let ShapeRef::PolySetTriangle(tri) = item.shape().as_shape_ref() else {
                    return true;
                };

                let tri: &Tri = tri;

                for ii in 0..tri.get_segment_count() as i32 {
                    if outline.collide_seg(&tri.get_segment(ii), 0, None, None) {
                        collision = true;
                        return false;
                    }
                }

                // Also must check for poly being completely inside the triangle
                if tri.point_inside(outline.c_point(0), 0, false) {
                    collision = true;
                    return false;
                }

                true
            });
        } else {
            tree.search(&min, &max, |&i: &u32| -> bool {
                let item = &self.owned[i as usize];
                if ref_shape.collide(item.shape(), 0, None, None) {
                    collision = true;
                    return false;
                }

                true
            });
        }

        collision
    }

    /// Port of `GetObjectsAt`: the parents of the entries on `layer` whose boxes contain the
    /// square of half-size `clearance` around `pt`.
    ///
    /// C++ returns an `std::unordered_set` (hash order); here the parents are returned without
    /// duplicates in tree search order (`drc.md` 4.12).
    pub fn get_objects_at(&self, pt: Vector2I, layer: PcbLayerId, clearance: i32) -> Vec<ItemId> {
        let mut retval: Vec<ItemId> = Vec::new();
        let mut seen: HashSet<ItemId> = HashSet::new();
        let min = [pt.x - clearance, pt.y - clearance];
        let max = [pt.x + clearance, pt.y + clearance];

        if let Some(tree) = self.tree.get(&layer.0) {
            tree.search(&min, &max, |&i: &u32| -> bool {
                let item = &self.owned[i as usize];
                if seen.insert(item.parent) {
                    retval.push(item.parent);
                }

                true
            });
        }

        retval
    }

    /// Port of `QueryCollidingPairs`: collects every bbox-overlapping pair of an entry of
    /// `ref_tree` (layer pair -> reference entries in storage order -> search order of this
    /// tree, entries of the same parent skipped) and then visits them in that order, skipping
    /// parent pairs already found colliding. The visitor sets its `&mut bool` when it found a
    /// collision and returns false to stop; `progress_reporter( done, count )` returning false
    /// stops as well. Returns 0, like the C++.
    pub fn query_colliding_pairs(
        &self,
        ref_tree: &DrcRtree,
        layer_pairs: &[LayerPair],
        mut visitor: impl FnMut(&LayerPair, &ItemWithShape, &ItemWithShape, &mut bool) -> bool,
        max_clearance: i32,
        mut progress_reporter: impl FnMut(i32, i32) -> bool,
    ) -> i32 {
        // PAIR_INFO
        let mut pairs_to_visit: Vec<(LayerPair, &ItemWithShape, &ItemWithShape)> = Vec::new();

        for layer_pair in layer_pairs {
            let (ref_layer, target_layer) = *layer_pair;

            for ref_item in ref_tree.on_layer(ref_layer) {
                let mut b = ref_item.shape().bbox(0);
                b.inflate(max_clearance);

                let (min, max) = min_max(&b);

                if let Some(tree) = self.tree.get(&target_layer.0) {
                    tree.search(&min, &max, |&i: &u32| -> bool {
                        let item_to_test = &self.owned[i as usize];
                        // don't collide items against themselves
                        if item_to_test.parent == ref_item.parent {
                            return true;
                        }

                        pairs_to_visit.push((*layer_pair, ref_item, item_to_test));
                        true
                    });
                }
            }
        }

        // keep track of BOARD_ITEMs pairs that have been already found to collide (some items
        // might be build of COMPOUND/triangulated shapes and a single subshape collision
        // means we have a hit)
        let mut colliding_compounds: HashSet<(ItemId, ItemId)> = HashSet::new();

        let count = pairs_to_visit.len() as i32;

        for (progress, (layer_pair, ref_item, test_item)) in pairs_to_visit.iter().enumerate() {
            if !progress_reporter(progress as i32, count) {
                break;
            }

            let mut a = ref_item.parent;
            let mut b = test_item.parent;

            // store canonical order so we don't collide in both directions (a:b and b:a).
            // C++ orders by address; any canonical order gives the same dedup (drc.md 4.12).
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }

            // don't report multiple collisions for compound or triangulated shapes
            if colliding_compounds.contains(&(a, b)) {
                continue;
            }

            let mut collision_detected = false;

            if !visitor(layer_pair, ref_item, test_item, &mut collision_detected) {
                break;
            }

            if collision_detected {
                colliding_compounds.insert((a, b));
            }
        }

        0
    }

    /// Port of `size`: the number of entries.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Port of `empty`.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Port of `OnLayer`: all entries of `layer` in tree storage order (`DRC_LAYER`).
    pub fn on_layer(&self, layer: PcbLayerId) -> Vec<&ItemWithShape> {
        match self.tree.get(&layer.0) {
            Some(tree) => tree.iter().map(|&i| &self.owned[i as usize]).collect(),
            None => Vec::new(),
        }
    }

    /// Port of `Overlapping( aLayer, aPoint, aAccuracy )`: the entries of `layer` whose boxes
    /// touch the square of half-size `accuracy` around `point`, in search order.
    pub fn overlapping_point(
        &self,
        layer: PcbLayerId,
        point: Vector2I,
        accuracy: i32,
    ) -> Vec<&ItemWithShape> {
        let mut rect = Box2I::new(point, Vector2I::new(0, 0));
        rect.inflate(accuracy);
        self.overlapping(layer, &rect)
    }

    /// Port of `Overlapping( aLayer, aRect )`: the entries of `layer` whose boxes overlap
    /// `rect`, in search order.
    pub fn overlapping(&self, layer: PcbLayerId, rect: &Box2I) -> Vec<&ItemWithShape> {
        let mut items = Vec::new();

        if let Some(tree) = self.tree.get(&layer.0) {
            let (min, max) = min_max(rect);

            tree.search(&min, &max, |&i: &u32| -> bool {
                let item = &self.owned[i as usize];
                items.push(item);
                true
            });
        }

        items
    }

    /// The layers that have a built tree, in `std::map` order (no C++ counterpart; for tests
    /// and dumps).
    pub fn layers(&self) -> impl Iterator<Item = PcbLayerId> + '_ {
        self.tree.keys().map(|&l| PcbLayerId(l))
    }

    /// `m_minPadding[ aLayer ]` (no C++ accessor; for tests and dumps).
    pub fn min_padding(&self, layer: PcbLayerId) -> Option<i32> {
        self.min_padding.get(&layer.0).copied()
    }

    /// Port of `recordPadding`.
    fn record_padding(&mut self, layer: PcbLayerId, padding: i32) {
        self.min_padding
            .entry(layer.0)
            .and_modify(|p| *p = (*p).min(padding))
            .or_insert(padding);
    }
}
