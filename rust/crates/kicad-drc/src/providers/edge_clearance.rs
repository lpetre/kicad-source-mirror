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

//! Port of `pcbnew/drc/drc_test_provider_edge_clearance.cpp`.
//!
//! Board edge clearance test. Checks all items for their mechanical clearances against the
//! board edge.
//! Errors generated:
//! - `DRCE_EDGE_CLEARANCE`
//! - `DRCE_SILK_EDGE_CLEARANCE`
//! - `DRCE_COPPER_OUTSIDE_BOARD`
//!
//! Sequential port of the thread-pool loops (`drc.md` 8): the work units and the copper items
//! are processed in index order, which is one of the orders the C++ can produce. The C++
//! report order of all three codes is scheduling dependent; so is which work unit of a
//! multi-layer pad / via wins the `m_emittedEdgeReports` de-duplication (only the marker layer
//! differs) and, through `DRC_RTREE::GetObjectsAt` (an `std::unordered_set`), which edge is
//! named by a silk item crossing the board edge when several edges pass the hit test at the
//! nearest outline point (the port takes the first in tree search order).
//!
//! The edge tree holds *copies* of the Edge.Cuts / Margin shapes (unfilled rectangles and
//! polygons split into segments, Edge.Cuts strokes set to width 0). Each copy is a distinct
//! `BOARD_ITEM*` in C++ with the UUID of its original; the shared [`crate::rtree::DrcRtree`]
//! keys its entries by board item handles, which copies do not have, so this module keeps its
//! own [`EdgeTree`]: the `DRC_RTREE::Insert`, `Build`, `QueryColliding` (no filter) and
//! `GetObjectsAt` code paths a `PCB_SHAPE` takes, over the same packed r-tree, with the copy's
//! index as the parent identity.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::kiid::Kiid;
use kicad_core::layer_ids::{Flashing, PcbLayerId, UNDEFINED_LAYER};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_geom::geometry::shape::{Shape, ShapeRef, ShapeType};
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{
    DRCE_COPPER_OUTSIDE_BOARD, DRCE_EDGE_CLEARANCE, DRCE_SILK_EDGE_CLEARANCE,
};
use kicad_pcb::eda_shape::{ShapeT, make_effective_shapes};
use kicad_pcb::item::SharedShape;
use kicad_pcb::padstack::{PadAttrib, PadProp};
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{AnyItem, BoardCtx, DetachedCopy, ItemId, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{
    DrcTestProviderBase, all_basic_items, for_each_geometry_item, is_invisible_text, no_path,
};

/// Port of `SILK_DISPOSITION`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SilkDisposition {
    Unknown,
    Resolving,
    OnBoard,
    OffBoard,
    CrossesEdge,
}

/// One entry of the edge tree (`DRC_RTREE::ITEM_WITH_SHAPE` of an edge copy).
#[derive(Clone, Debug)]
struct EdgeEntry {
    /// `parent`: the index of the edge copy.
    edge: usize,
    /// `parentShape`
    parent_shape: SharedShape,
    /// Index of the indexed sub-shape in `parent_shape`, or `None` for the whole shape.
    sub: Option<u32>,
}

impl EdgeEntry {
    /// `ITEM_WITH_SHAPE::shape`.
    fn shape(&self) -> &dyn Shape {
        match self.sub {
            None => self.parent_shape.as_ref(),
            Some(i) => self
                .parent_shape
                .indexable_subshape(i as usize)
                .expect("edge tree entry refers to an existing sub-shape"),
        }
    }
}

/// `m_edgesTree`: the `DRC_RTREE` of the edge copies (see the module documentation).
#[derive(Clone, Debug, Default)]
struct EdgeTree {
    /// `m_tree`
    tree: BTreeMap<i32, PackedRTree<u32, 2, 16>>,
    /// `m_builders`
    builders: BTreeMap<i32, PackedRTreeBuilder<u32, 2, 16>>,
    /// `m_owned`
    owned: Vec<EdgeEntry>,
}

/// The query box `{ x, y }`, `{ right, bottom }` of a [`Box2I`].
fn min_max(b: &Box2I) -> ([i32; 2], [i32; 2]) {
    ([b.x(), b.y()], [b.right(), b.bottom()])
}

impl EdgeTree {
    /// `DRC_RTREE::Insert( aItem, aLayer, aConstraintType, aWorstClearance )` for the edge copy
    /// `edge` (a `PCB_SHAPE`: no field, table cell or pad special case applies).
    fn insert(
        &mut self,
        ctx: &BoardCtx<'_>,
        edge: usize,
        shape_item: &PcbShape,
        layer: PcbLayerId,
        constraint_type: DrcConstraintT,
        worst_clearance: i32,
    ) {
        let item = ctx.view(AnyItem::Shape(shape_item));
        let shape: SharedShape = item.effective_shape(layer, Flashing::Default, constraint_type);

        let entries: Vec<Option<u32>> = if shape.has_indexable_subshapes() {
            (0..shape.indexable_subshape_count() as u32)
                .map(Some)
                .collect()
        } else {
            vec![None]
        };

        for sub in entries {
            let entry = EdgeEntry {
                edge,
                parent_shape: Arc::clone(&shape),
                sub,
            };

            if entry.shape().type_() == ShapeType::Null {
                continue;
            }

            let mut bbox = entry.shape().bbox(0);
            bbox.inflate(worst_clearance);

            let (mmin, mmax) = min_max(&bbox);
            let index = self.owned.len() as u32;
            self.owned.push(entry);
            self.builders
                .entry(layer.0)
                .or_default()
                .add(&mmin, &mmax, index);
        }
    }

    /// `DRC_RTREE::Build`.
    fn build(&mut self) {
        for (layer, builder) in self.builders.iter_mut() {
            self.tree.insert(*layer, builder.build());
        }

        self.builders.clear();
    }

    /// `DRC_RTREE::QueryColliding( aRefItem, aRefLayer, aTargetLayer, nullptr, aVisitor,
    /// aClearance )`: the visitor receives the index of the colliding edge copy. (The
    /// `aItem->parent == aRefItem` test never fires: the tree holds copies only.)
    fn query_colliding(
        &self,
        ref_item: ItemRef<'_>,
        ref_layer: PcbLayerId,
        target_layer: PcbLayerId,
        visitor: &mut dyn FnMut(usize) -> bool,
        clearance: i32,
    ) -> i32 {
        let mut colliding_compounds: HashSet<usize> = HashSet::new();

        let ref_shape =
            ref_item.effective_shape(ref_layer, Flashing::Default, DrcConstraintT::NullConstraint);

        // A mask aperture reaches past the item's copper bounding box, so cover both
        let mut b = ref_item.bounding_box();
        b.merge(&ref_shape.bbox(0));
        b.inflate(clearance);

        let (min, max) = min_max(&b);
        let mut count = 0;

        if let Some(tree) = self.tree.get(&target_layer.0) {
            tree.search(&min, &max, |&i: &u32| -> bool {
                let entry = &self.owned[i as usize];

                if colliding_compounds.contains(&entry.edge) {
                    return true;
                }

                if ref_shape.collide(entry.shape(), clearance, None, None) {
                    colliding_compounds.insert(entry.edge);
                    count += 1;
                    return visitor(entry.edge);
                }

                true
            });
        }

        count
    }

    /// `DRC_RTREE::GetObjectsAt`: the edge copies without duplicates in tree search order
    /// (C++: an `std::unordered_set`, hash order).
    fn get_objects_at(&self, pt: Vector2I, layer: PcbLayerId, clearance: i32) -> Vec<usize> {
        let mut retval: Vec<usize> = Vec::new();
        let min = [pt.x - clearance, pt.y - clearance];
        let max = [pt.x + clearance, pt.y + clearance];

        if let Some(tree) = self.tree.get(&layer.0) {
            tree.search(&min, &max, |&i: &u32| -> bool {
                let edge = self.owned[i as usize].edge;

                if !retval.contains(&edge) {
                    retval.push(edge);
                }

                true
            });
        }

        retval
    }
}

/// `WORK_UNIT`
struct WorkUnit<'c> {
    item: ItemRef<'c>,
    shape_layer: PcbLayerId,
    shape: SharedShape,
}

/// `COPPER_ITEM_UNITS`: half-open range of an item's per-layer entries in the work unit list.
struct CopperItemUnits<'c> {
    item: ItemRef<'c>,
    first: usize,
    last: usize,
}

/// Port of `DRC_TEST_PROVIDER_EDGE_CLEARANCE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderEdgeClearance {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_largestEdgeClearance`
    largest_edge_clearance: i32,
    /// `m_epsilon`
    epsilon: i32,
}

impl Default for DrcTestProviderEdgeClearance {
    fn default() -> Self {
        DrcTestProviderEdgeClearance {
            base: DrcTestProviderBase::new("edge_clearance"),
            largest_edge_clearance: 0,
            epsilon: 0,
        }
    }
}

/// The per-run state of `Run` (C++ members cleared at the start of `Run`).
struct RunData<'c> {
    /// The edge copies (`std::vector<std::unique_ptr<PCB_SHAPE>> edges`).
    edges: Vec<PcbShape>,
    /// `m_edgesTree`
    edges_tree: EdgeTree,
    /// `m_castellatedPads`
    castellated_pads: Vec<ItemRef<'c>>,
    /// `m_silkDisposition` (`std::map<BOARD_ITEM*, SILK_DISPOSITION>`, lookups only), keyed by
    /// the handle of the work unit item.
    silk_disposition: HashMap<ItemId, SilkDisposition>,
    /// `m_emittedEdgeReports`
    emitted_edge_reports: HashSet<(Kiid, Kiid, (i32, i32))>,
}

/// The `m_silkDisposition` key of an item (work unit items are board items).
fn silk_key(item: &ItemRef<'_>) -> ItemId {
    item.id().expect("board items have a handle")
}

/// Whether `item` is a track segment or an arc (`PCB_TRACE_T || PCB_ARC_T`).
fn is_track(item: &ItemRef<'_>) -> bool {
    item.type_() == KicadT::PcbTraceT || item.type_() == KicadT::PcbArcT
}

impl DrcTestProviderEdgeClearance {
    /// Port of `DRC_TEST_PROVIDER_EDGE_CLEARANCE::resolveSilkDisposition`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_silk_disposition(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        data: &mut RunData<'_>,
        item: ItemRef<'_>,
        item_shape: &dyn Shape,
        board_outline: &ShapePolySet,
    ) {
        let mut disposition = SilkDisposition::Unknown;

        if let ShapeRef::Compound(compound) = item_shape.as_shape_ref() {
            for elem in compound.shapes() {
                let elem_disposition = if board_outline.contains(elem.centre(), -1, 0, false) {
                    SilkDisposition::OnBoard
                } else {
                    SilkDisposition::OffBoard
                };

                if disposition == SilkDisposition::Unknown {
                    disposition = elem_disposition;
                } else if disposition != elem_disposition {
                    disposition = SilkDisposition::CrossesEdge;
                    break;
                }
            }
        } else {
            disposition = if board_outline.contains(item_shape.centre(), -1, 0, false) {
                SilkDisposition::OnBoard
            } else {
                SilkDisposition::OffBoard
            };
        }

        data.silk_disposition.insert(silk_key(&item), disposition);

        if disposition == SilkDisposition::CrossesEdge {
            let mut nearest_edge: Option<usize> = None;
            let item_pos = item.as_dyn().center(ctx);
            let mut nearest_edge_pt = board_outline.c_outline(0).nearest_point(item_pos, false);

            for outline_idx in 1..board_outline.outline_count() {
                let other_edge_pt = board_outline
                    .c_outline(outline_idx)
                    .nearest_point(item_pos, false);

                if other_edge_pt.squared_distance(item_pos)
                    < nearest_edge_pt.squared_distance(item_pos)
                {
                    nearest_edge_pt = other_edge_pt;
                }
            }

            for edge in
                data.edges_tree
                    .get_objects_at(nearest_edge_pt, PcbLayerId::EdgeCuts, self.epsilon)
            {
                if data.edges[edge].hit_test(ctx, nearest_edge_pt, self.epsilon) {
                    nearest_edge = Some(edge);
                    break;
                }
            }

            let Some(nearest_edge) = nearest_edge else {
                return;
            };

            let nearest_edge = ctx.view(AnyItem::Shape(&data.edges[nearest_edge]));
            let constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::SilkClearanceConstraint,
                Some(nearest_edge),
                Some(item),
                UNDEFINED_LAYER,
            );
            let min_clearance = constraint.value().min();

            if constraint.severity() != Severity::Ignore && min_clearance >= 0 {
                let mut drc_item =
                    item::create(DRCE_SILK_EDGE_CLEARANCE).expect("DRCE_SILK_EDGE_CLEARANCE item");

                // Report clearance info if there is any, even though crossing is just a
                // straight-up collision
                if min_clearance > 0 {
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s clearance %s; actual %s)",
                        &constraint.name(),
                        min_clearance as f64,
                        0.0,
                        EdaDataType::Distance,
                    ));
                }

                drc_item.set_item_ids(vec![nearest_edge.uuid(), item.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();
                self.base.report_two_point_geometry(
                    host,
                    Arc::new(drc_item),
                    nearest_edge_pt,
                    nearest_edge_pt,
                    nearest_edge_pt,
                    item.layer(),
                );
            }
        }
        // (The C++ "Silk outside board edge" branch for OFF_BOARD is disabled with #if 0.)
    }

    /// Port of `DRC_TEST_PROVIDER_EDGE_CLEARANCE::testAgainstEdge`: false stops the query.
    #[allow(clippy::too_many_arguments)]
    fn test_against_edge(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        data: &mut RunData<'_>,
        item: ItemRef<'_>,
        item_shape: &dyn Shape,
        shape_layer: PcbLayerId,
        edge_index: usize,
        constraint_type: DrcConstraintT,
        error_code: i32,
    ) -> bool {
        let edge = ctx.view(AnyItem::Shape(&data.edges[edge_index]));
        let shape = edge.effective_shape(
            PcbLayerId::EdgeCuts,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );

        let constraint = engine.eval_rules(
            ctx,
            constraint_type,
            Some(edge),
            Some(item),
            UNDEFINED_LAYER,
        );
        let min_clearance = constraint.value().min();
        let mut actual = 0;
        let mut pos = Vector2I::default();

        if constraint.severity() != Severity::Ignore
            && min_clearance >= 0
            && item_shape.collide(
                shape.as_ref(),
                0.max(min_clearance - self.epsilon),
                Some(&mut actual),
                Some(&mut pos),
            )
        {
            if is_track(&item) {
                // Edge collisions are allowed inside the holes of castellated pads
                for castellated_pad in &data.castellated_pads {
                    let hole = castellated_pad
                        .effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::NullConstraint)
                        .expect("a castellated pad has a hole shape");

                    if hole.collide_point(pos, 0, None, None) {
                        return true;
                    }
                }
            }

            if !data
                .emitted_edge_reports
                .insert((item.uuid(), edge.uuid(), (pos.x, pos.y)))
            {
                // Same (item, edge, pos) already reported from another work unit.
                if is_track(&item) {
                    return host.report_all_track_errors();
                } else {
                    return false;
                }
            }

            let mut drc_item = item::create(error_code).expect("edge clearance item");

            // Only report clearance info if there is any; otherwise it's just a straight
            // collision
            if min_clearance > 0 {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s clearance %s; actual %s)",
                    &constraint.name(),
                    min_clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.set_item_ids(vec![edge.uuid(), item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();
            self.base.report_two_item_geometry(
                host,
                Arc::new(drc_item),
                pos,
                edge,
                item,
                shape_layer,
                actual,
            );

            if error_code == DRCE_SILK_EDGE_CLEARANCE {
                data.silk_disposition
                    .insert(silk_key(&item), SilkDisposition::CrossesEdge);
            }

            if is_track(&item) {
                return host.report_all_track_errors();
            } else {
                return false; // don't report violations with multiple edges; one is enough
            }
        }

        true
    }

    /// Port of `DRC_TEST_PROVIDER_EDGE_CLEARANCE::testCopperOutsideBoard` (the thread-pool
    /// loop in item order).
    fn test_copper_outside_board(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        work_units: &[WorkUnit<'_>],
        copper_items: &[CopperItemUnits<'_>],
    ) {
        // Rebuilt rather than read from BoardOutline(), which keeps partial contours of a
        // malformed outline
        let (mut outline, ok) = ctx
            .board
            .get_board_polygon_outlines(ctx, false, None, false, false);

        if !ok || outline.outline_count() == 0 {
            return;
        }

        outline.fracture(true);
        outline.build_bbox_caches();

        // Collide() triangulates lazily, which would stall every worker behind the first one
        outline.cache_triangulation_with(false, false, true);

        'items: for copper_item in copper_items {
            if host.is_cancelled() || host.is_error_limit_exceeded(DRCE_COPPER_OUTSIDE_BOARD) {
                continue;
            }

            for unit in &work_units[copper_item.first..copper_item.last] {
                if outline.collide_shape(unit.shape.as_ref(), 0, None, None) {
                    continue 'items;
                }
            }

            let item = copper_item.item;
            let mut layer = work_units[copper_item.first].shape_layer;

            if layer == UNDEFINED_LAYER {
                layer = item.layer();
            }

            let mut drc_item =
                item::create(DRCE_COPPER_OUTSIDE_BOARD).expect("DRCE_COPPER_OUTSIDE_BOARD item");
            drc_item.set_item_ids(vec![item.uuid()]);
            self.base.report_violation(
                host,
                Arc::new(drc_item),
                item.position(),
                layer.0,
                &no_path,
            );
        }
    }

    /// Port of `DRC_TEST_PROVIDER_EDGE_CLEARANCE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        {
            let mut host = RunHost(&mut *run);

            if !host.is_error_limit_exceeded(DRCE_EDGE_CLEARANCE)
                || !host.is_error_limit_exceeded(DRCE_COPPER_OUTSIDE_BOARD)
            {
                if !self
                    .base
                    .report_phase(&mut host, "Checking copper to board edge clearances...")
                {
                    return false; // DRC cancelled
                }
            } else if !host.is_error_limit_exceeded(DRCE_SILK_EDGE_CLEARANCE) {
                if !self
                    .base
                    .report_phase(&mut host, "Checking silk to board edge clearances...")
                {
                    return false; // DRC cancelled
                }
            } else {
                report_aux(host.0, "Edge clearance violations ignored. Tests not run.");
                return true; // continue with other tests
            }
        }

        let board = ctx.board;
        self.epsilon = board.design_settings().get_drc_epsilon();

        let mut data = RunData {
            edges: Vec::new(),
            edges_tree: EdgeTree::default(),
            castellated_pads: Vec::new(),
            silk_disposition: HashMap::new(),
            emitted_edge_reports: HashSet::new(),
        };

        if let Some(worst) =
            engine.query_worst_constraint(DrcConstraintT::EdgeClearanceConstraint, false)
        {
            self.largest_edge_clearance = worst.value().min();
        }

        //
        // Build an RTree of the various edges and margins found on the board.
        //
        let edge_layers = Lset::from_layers(&[PcbLayerId::EdgeCuts, PcbLayerId::Margin]);
        let edges = &mut data.edges;

        for_each_geometry_item(ctx, &[KicadT::PcbShapeT], &edge_layers, |item| {
            let shape = item.as_shape().expect("PCB_SHAPE_T is a PCB_SHAPE");
            let env = board.mut_env(item.id().expect("board items have a handle"));
            let mut stroke = shape.stroke(ctx);

            if item.is_on_layer(PcbLayerId::EdgeCuts) {
                stroke.set_width(0);
            }

            let push = |edges: &mut Vec<PcbShape>, f: &dyn Fn(&mut PcbShape)| {
                let mut clone = shape.detached_copy();
                f(&mut clone);
                clone.set_stroke(&env, stroke.clone());
                edges.push(clone);
            };

            if shape.shape.shape() == ShapeT::Rectangle && !shape.shape.is_solid_fill() {
                // A single rectangle for the board would defeat the RTree, so convert to edges
                if shape.shape.corner_radius() > 0 {
                    for subshape in make_effective_shapes(shape, ctx, true, false, false) {
                        match subshape.as_shape_ref() {
                            ShapeRef::Segment(segment) => {
                                let seg = *segment.get_seg();
                                push(edges, &|c: &mut PcbShape| {
                                    c.set_shape(&env, ShapeT::Segment);
                                    c.set_start(&env, seg.a);
                                    c.set_end(&env, seg.b);
                                });
                            }
                            ShapeRef::Arc(arc) => {
                                let (p0, mid, p1) = (arc.get_p0(), arc.get_arc_mid(), arc.get_p1());
                                push(edges, &|c: &mut PcbShape| {
                                    c.set_shape(&env, ShapeT::Arc);
                                    c.set_arc_geometry(&env, p0, mid, p1);
                                });
                            }
                            _ => {
                                // wxFAIL_MSG( "Unexpected effective shape type %d for rounded
                                // rectangle" )
                                debug_assert!(
                                    false,
                                    "Unexpected effective shape type {:?} for rounded rectangle",
                                    subshape.type_()
                                );
                                continue;
                            }
                        }
                    }
                } else {
                    let start = shape.shape.start();
                    let end = shape.shape.end();

                    // SetEndX( GetStartX() )
                    push(edges, &|c: &mut PcbShape| {
                        c.set_shape(&env, ShapeT::Segment);
                        c.set_end(&env, Vector2I::new(start.x, c.shape.end().y));
                    });
                    // SetEndY( GetStartY() )
                    push(edges, &|c: &mut PcbShape| {
                        c.set_shape(&env, ShapeT::Segment);
                        c.set_end(&env, Vector2I::new(c.shape.end().x, start.y));
                    });
                    // SetStartX( GetEndX() )
                    push(edges, &|c: &mut PcbShape| {
                        c.set_shape(&env, ShapeT::Segment);
                        c.set_start(&env, Vector2I::new(end.x, c.shape.start().y));
                    });
                    // SetStartY( GetEndY() )
                    push(edges, &|c: &mut PcbShape| {
                        c.set_shape(&env, ShapeT::Segment);
                        c.set_start(&env, Vector2I::new(c.shape.start().x, end.y));
                    });
                }
            } else if shape.shape.shape() == ShapeT::Poly
                && !shape.shape.is_solid_fill()
                && !shape.shape.poly_shape().is_empty()
            {
                // A single polygon for the board would defeat the RTree, so convert to edges.
                let poly = shape.shape.poly_shape().c_outline(0);

                for ii in 0..poly.segment_count() {
                    let seg = poly.c_segment(ii);
                    push(edges, &|c: &mut PcbShape| {
                        c.set_shape(&env, ShapeT::Segment);
                        c.set_start(&env, seg.a);
                        c.set_end(&env, seg.b);
                    });
                }
            } else {
                push(edges, &|_c: &mut PcbShape| {});
            }

            true
        });

        for (index, edge) in data.edges.iter().enumerate() {
            for layer in [PcbLayerId::EdgeCuts, PcbLayerId::Margin] {
                if edge.is_on_layer(ctx, layer) {
                    data.edges_tree.insert(
                        ctx,
                        index,
                        edge,
                        layer,
                        DrcConstraintT::EdgeClearanceConstraint,
                        self.largest_edge_clearance,
                    );
                }
            }
        }

        for (fp_id, _) in board.footprints() {
            for (_, pad) in board.footprint_pads(fp_id) {
                if pad.property() == PadProp::Castellated {
                    data.castellated_pads.push(ctx.view(AnyItem::Pad(pad)));
                }
            }
        }

        data.edges_tree.build();

        //
        // Collect all testable (item, layer, shape) tuples, then test against edges (in
        // parallel in C++). Flattening to per-layer work units ensures even distribution
        // across threads, since zones with many layers become many separate work units rather
        // than one heavy item.
        //
        let mut work_units: Vec<WorkUnit<'_>> = Vec::new();
        let mut copper_items: Vec<CopperItemUnits<'_>> = Vec::new();

        for_each_geometry_item(ctx, all_basic_items(), Lset::all_layers_mask(), |item| {
            if is_invisible_text(item) {
                return true;
            }

            if let Some(zone) = item.as_zone()
                && item.type_() == KicadT::PcbZoneT
            {
                // Rule areas have no copper and are purely logical -- skip edge clearance.
                if zone.is_rule_area() {
                    return true;
                }
            }

            if item.type_() == KicadT::PcbPadT {
                let pad = item.as_pad().expect("PCB_PAD_T is a PAD");

                if pad.property() == PadProp::Castellated || pad.attribute() == PadAttrib::Conn {
                    return true;
                }
            }

            let layers_to_test: Vec<PcbLayerId> = match item.type_() {
                KicadT::PcbPadT => item
                    .as_pad()
                    .expect("PCB_PAD_T is a PAD")
                    .lib_padstack()
                    .unique_layers(),
                KicadT::PcbViaT => item
                    .as_via()
                    .expect("PCB_VIA_T is a via")
                    .padstack()
                    .unique_layers(),
                KicadT::PcbZoneT => item.layer_set().iter().collect(),
                _ => vec![UNDEFINED_LAYER],
            };

            let first_unit = work_units.len();

            for layer in layers_to_test {
                work_units.push(WorkUnit {
                    item,
                    shape_layer: layer,
                    shape: item.effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    ),
                });
            }

            // Zone fills are already clipped to the board outline
            if item.type_() != KicadT::PcbZoneT && item.as_dyn().is_on_copper_layer(ctx) {
                copper_items.push(CopperItemUnits {
                    item,
                    first: first_unit,
                    last: work_units.len(),
                });
            }

            true
        });

        let count = work_units.len();
        let board_outline = board.board_outline(ctx);
        let mut host = RunHost(run);

        // The thread-pool loop, in unit order.
        for (done, wu) in work_units.iter().enumerate() {
            // while( done < count ) reportProgress( done, count )
            self.base.report_progress(&mut host, done, count, 1);

            if host.is_cancelled() {
                break;
            }

            let test_copper = !host.is_error_limit_exceeded(DRCE_EDGE_CLEARANCE);
            let test_silk = !host.is_error_limit_exceeded(DRCE_SILK_EDGE_CLEARANCE);

            if !test_copper && !test_silk {
                continue;
            }

            let item = wu.item;
            let on_silk =
                item.is_on_layer(PcbLayerId::FSilkS) || item.is_on_layer(PcbLayerId::BSilkS);

            for test_layer in [PcbLayerId::EdgeCuts, PcbLayerId::Margin] {
                if test_copper && item.as_dyn().is_on_copper_layer(ctx) {
                    let tree = std::mem::take(&mut data.edges_tree);
                    tree.query_colliding(
                        item,
                        wu.shape_layer,
                        test_layer,
                        &mut |edge| {
                            self.test_against_edge(
                                engine,
                                &mut host,
                                ctx,
                                &mut data,
                                item,
                                wu.shape.as_ref(),
                                wu.shape_layer,
                                edge,
                                DrcConstraintT::EdgeClearanceConstraint,
                                DRCE_EDGE_CLEARANCE,
                            )
                        },
                        self.largest_edge_clearance,
                    );
                    data.edges_tree = tree;
                }

                if test_silk && on_silk {
                    let tree = std::mem::take(&mut data.edges_tree);
                    tree.query_colliding(
                        item,
                        wu.shape_layer,
                        test_layer,
                        &mut |edge| {
                            self.test_against_edge(
                                engine,
                                &mut host,
                                ctx,
                                &mut data,
                                item,
                                wu.shape.as_ref(),
                                wu.shape_layer,
                                edge,
                                DrcConstraintT::SilkClearanceConstraint,
                                DRCE_SILK_EDGE_CLEARANCE,
                            )
                        },
                        self.largest_edge_clearance,
                    );
                    data.edges_tree = tree;
                }
            }

            if test_silk && on_silk {
                let mut needs_resolution = false;

                // try_emplace( item, RESOLVING )
                let key = silk_key(&item);
                let inserted = !data.silk_disposition.contains_key(&key);
                let it = data
                    .silk_disposition
                    .entry(key)
                    .or_insert(SilkDisposition::Resolving);

                if inserted || *it == SilkDisposition::Unknown {
                    *it = SilkDisposition::Resolving;
                    needs_resolution = true;
                }

                if needs_resolution && !board_outline.is_empty() {
                    self.resolve_silk_disposition(
                        engine,
                        &mut host,
                        ctx,
                        &mut data,
                        item,
                        wu.shape.as_ref(),
                        &board_outline,
                    );
                }
            }
        }

        if !host.is_cancelled() && !host.is_error_limit_exceeded(DRCE_COPPER_OUTSIDE_BOARD) {
            self.test_copper_outside_board(&mut host, ctx, &work_units, &copper_items);
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderEdgeClearance {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(engine, run, ctx)
    }
}
