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

//! Port of `pcbnew/drc/drc_test_provider_physical_clearance.cpp`.
//!
//! Physical clearance tests.
//!
//! Errors generated:
//! - `DRCE_PHYSICAL_CLEARANCE` (reported as `DRCE_CLEARANCE`)
//! - `DRCE_PHYSICAL_HOLE_CLEARANCE` (reported as `DRCE_HOLE_CLEARANCE`)
//!
//! Sequential in C++ as well. The pair de-duplication map (`std::unordered_map` keyed by the
//! two item addresses in address order) is only used for lookups; the port keys it by the two
//! item handles in handle order (`drc.md` 4.12). The board needs the per-run caches of the
//! cache generator ([`DrcBoardCaches`]: `m_DRCMaxPhysicalClearance`, `m_DRCMaxClearance`,
//! `m_DRCZones`, the copper zone trees), which the provider borrows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{Flashing, PcbLayerId, is_copper_layer};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::eda_angle::EdaAngle;
use kicad_geom::geometry::geometry_utils::get_arc_to_segment_count;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::trigo::{deg2rad, rotate_point, rotate_point_around};
use kicad_pcb::drc_api::{DRCE_CLEARANCE, DRCE_HOLE_CLEARANCE, DrcConstraint};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::item::SharedShape;
use kicad_pcb::{BoardCtx, ItemId, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rtree::{ATOMIC_TABLES, DrcRtree};
use crate::test_provider::{
    DrcTestProviderBase, for_each_geometry_item, is_invisible_text, no_path,
};

/// Default of `ADVANCED_CFG::m_SliverAngleTolerance` (degrees, `common/advanced_config.cpp`);
/// the port has no advanced configuration file.
pub const ADVANCED_CFG_SLIVER_ANGLE_TOLERANCE: f64 = 20.0;

/// The `static const std::vector<KICAD_T> itemTypes` of `Run`.
const ITEM_TYPES: &[KicadT] = &[
    KicadT::PcbTraceT,
    KicadT::PcbArcT,
    KicadT::PcbViaT,
    KicadT::PcbFootprintT,
    KicadT::PcbPadT,
    KicadT::PcbShapeT,
    KicadT::PcbFieldT,
    KicadT::PcbTextT,
    KicadT::PcbTextboxT,
    KicadT::PcbTableT,
    KicadT::PcbTablecellT,
    KicadT::PcbDimensionT,
    KicadT::PcbBarcodeT,
];

/// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE`.
///
/// `'c` is the lifetime of the borrowed per-run board caches.
pub struct DrcTestProviderPhysicalClearance<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches built by the cache generator for this run.
    caches: &'c DrcBoardCaches,
    /// `m_itemTree`
    item_tree: DrcRtree,
}

/// The layers a hole pierces (`Run` and `testItemAgainstItem`): the front board tech layers
/// and `F_CrtYd` for an item on `F_Cu`, the back ones for `B_Cu`, every board copper layer
/// for both.
fn add_hole_layers(layers: &mut Lset, board_copper_layers: &Lset) {
    if layers.contains(PcbLayerId::FCu) {
        let mut front = Lset::front_board_tech_mask().clone();
        front.set(PcbLayerId::FCrtYd);
        *layers |= &front;
    }

    if layers.contains(PcbLayerId::BCu) {
        let mut back = Lset::back_board_tech_mask().clone();
        back.set(PcbLayerId::BCrtYd);
        *layers |= &back;
    }

    if layers.contains(PcbLayerId::FCu) && layers.contains(PcbLayerId::BCu) {
        *layers |= board_copper_layers;
    }
}

/// The two parents of a pair in canonical order (C++: by address; any canonical order gives
/// the same de-duplication).
fn canonical(a: ItemId, b: ItemId) -> (ItemId, ItemId) {
    if a > b { (b, a) } else { (a, b) }
}

/// The `angleDiff` lambda of `testShapeLineChain`.
fn angle_diff(mut a: f64, mut b: f64) -> f64 {
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let diff = b - a;

    if diff > std::f64::consts::PI {
        2.0 * std::f64::consts::PI - diff
    } else {
        diff
    }
}

impl<'c> DrcTestProviderPhysicalClearance<'c> {
    /// `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderPhysicalClearance {
            base: DrcTestProviderBase::new("physical_clearance"),
            caches,
            item_tree: DrcRtree::new(),
        }
    }

    /// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        let board = ctx.board;
        let caches = self.caches;
        self.item_tree.clear();

        let error_max = board.design_settings().max_error();
        let board_copper_layers = Lset::all_cu_mask_n(board.copper_layer_count());

        if caches.max_physical_clearance <= 0 {
            report_aux(
                run,
                "No physical clearance constraints found. Tests not run.",
            );
            return true; // continue with other tests
        }

        let mut progress_delta: usize = 250;
        let mut count: usize = 0;
        let mut ii: usize = 0;

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Gathering physical items...")
        {
            return false; // DRC cancelled
        }

        let courtyards = Lset::from_layers(&[PcbLayerId::FCrtYd, PcbLayerId::BCrtYd]);
        let base = self.base.clone();

        //
        // Generate a count for use in progress reporting.
        //

        for_each_geometry_item(ctx, ITEM_TYPES, Lset::all_layers_mask(), |item| {
            if is_invisible_text(item) {
                return true;
            }

            count += 1;
            true
        });

        //
        // Generate a BOARD_ITEM RTree.
        //

        {
            let item_tree = &mut self.item_tree;

            for_each_geometry_item(ctx, ITEM_TYPES, Lset::all_layers_mask(), |item| {
                if is_invisible_text(item) {
                    return true;
                }

                let i = ii;
                ii += 1;

                if !base.report_progress(&mut host, i, count, progress_delta) {
                    return false;
                }

                let mut layers = item.layer_set();

                // Special-case holes and edge-cuts which pierce all physical layers
                if item.has_hole() {
                    add_hole_layers(&mut layers, &board_copper_layers);
                } else if item.type_() == KicadT::PcbFootprintT {
                    layers = courtyards.clone();
                } else if item.is_on_layer(PcbLayerId::EdgeCuts) {
                    layers |= Lset::physical_layers_mask();
                    layers |= &courtyards;
                }

                for layer in layers.iter() {
                    item_tree.insert(
                        item,
                        layer,
                        DrcConstraintT::PhysicalClearanceConstraint,
                        caches.max_physical_clearance,
                        ATOMIC_TABLES,
                    );
                }

                true
            });
        }

        self.item_tree.build();

        // std::unordered_map<PTR_PTR_CACHE_KEY, LSET>: lookups only.
        let checked_pairs: RefCell<HashMap<(ItemId, ItemId), Lset>> = RefCell::new(HashMap::new());
        progress_delta = 100;
        ii = 0;

        //
        // Run clearance checks -between- items.
        //

        if !host.is_error_limit_exceeded(DRCE_CLEARANCE)
            || !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE)
        {
            if !self
                .base
                .report_phase(&mut host, "Checking physical clearances...")
            {
                return false; // DRC cancelled
            }

            let this = &*self;

            for_each_geometry_item(ctx, ITEM_TYPES, Lset::all_layers_mask(), |item| {
                if is_invisible_text(item) {
                    return true;
                }

                let i = ii;
                ii += 1;

                if !base.report_progress(&mut host, i, count, progress_delta) {
                    return false;
                }

                let mut layers = item.layer_set();

                if item.type_() == KicadT::PcbFootprintT {
                    layers = courtyards.clone();
                }

                let item_id = item.id().expect("board items have a handle");

                for layer in layers.iter() {
                    let item_shape = item.effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );

                    // Filter:
                    let mut filter = |other: ItemId| -> bool {
                        if item.type_() == KicadT::PcbTablecellT
                            && item.parent().and_then(|p| p.id()) == Some(other)
                        {
                            return false;
                        }

                        // store canonical order so we don't collide in both directions (a:b
                        // and b:a)
                        let key = canonical(item_id, other);
                        let mut pairs = checked_pairs.borrow_mut();

                        if pairs.get(&key).is_some_and(|l| l.test(layer)) {
                            false
                        } else {
                            pairs.entry(key).or_default().set(layer);
                            true
                        }
                    };

                    // Visitor:
                    let mut visitor = |other: ItemId| -> bool {
                        let other_ref = ctx
                            .item(other)
                            .expect("DRC_RTREE entries refer to board items");

                        if this.test_item_against_item(
                            engine,
                            &mut host,
                            ctx,
                            item,
                            item_shape.as_ref(),
                            layer,
                            other_ref,
                        ) > 0
                        {
                            // store canonical order; once we record one DRC for error for
                            // physical clearance we don't need to record more
                            checked_pairs
                                .borrow_mut()
                                .entry(canonical(item_id, other))
                                .or_default()
                                .set_all();
                        }

                        !host.is_cancelled()
                    };

                    this.item_tree.query_colliding(
                        item,
                        layer,
                        layer,
                        Some(&mut filter),
                        Some(&mut visitor),
                        caches.max_physical_clearance,
                    );

                    this.test_item_against_zones(engine, &mut host, ctx, item, layer);
                }

                true
            });
        }

        progress_delta = 100;
        count = 0;
        ii = 0;

        //
        // Generate a count for progress reporting.
        //

        let zone_and_shape = [KicadT::PcbZoneT, KicadT::PcbShapeT];

        for_each_geometry_item(ctx, &zone_and_shape, &board_copper_layers, |item| {
            if item.as_zone().is_some_and(|z| z.is_rule_area()) {
                return true; // Continue with other items
            }

            let mut on = item.layer_set();
            on &= &board_copper_layers;
            count += on.count();
            true
        });

        //
        // Run clearance checks -within- polygonal items.
        //

        let this = &*self;

        for_each_geometry_item(ctx, &zone_and_shape, &board_copper_layers, |item| {
            let shape = if item.type_() == KicadT::PcbShapeT {
                item.as_shape()
            } else {
                None
            };
            let zone = item.as_zone();

            if zone.is_some_and(|z| z.is_rule_area()) {
                return true; // Continue with other items
            }

            for layer in item.layer_set().iter() {
                if is_copper_layer(layer) {
                    let i = ii;
                    ii += 1;

                    if !base.report_progress(&mut host, i, count, progress_delta) {
                        return false;
                    }

                    let c = engine.eval_rules(
                        ctx,
                        DrcConstraintT::PhysicalClearanceConstraint,
                        Some(item),
                        None,
                        layer,
                    );

                    if let Some(shape) = shape {
                        match shape.shape.shape() {
                            ShapeT::Poly => {
                                if !shape.shape.poly_shape().is_empty() {
                                    this.test_shape_line_chain(
                                        &mut host,
                                        ctx,
                                        shape.shape.poly_shape().c_outline(0),
                                        shape.width(ctx),
                                        layer,
                                        item,
                                        &c,
                                    );
                                }
                            }

                            ShapeT::Bezier => {
                                let mut as_poly = ShapeLineChain::new();

                                // RebuildBezierToSegmentsPointsList( errorMax ) on the board
                                // item in C++ (a derived cache); on a copy here.
                                let mut rebuilt = shape.shape.clone();
                                rebuilt.rebuild_bezier_to_segments_points_list(error_max);

                                for &pt in rebuilt.bezier_points() {
                                    as_poly.append_point(pt, false);
                                }

                                this.test_shape_line_chain(
                                    &mut host,
                                    ctx,
                                    &as_poly,
                                    shape.width(ctx),
                                    layer,
                                    item,
                                    &c,
                                );
                            }

                            ShapeT::Arc => {
                                let mut as_poly = ShapeLineChain::new();

                                let center = shape.center(ctx);
                                let angle = -shape.shape.arc_angle();
                                let r = shape.shape.radius() as f64;
                                let steps = get_arc_to_segment_count(r as i32, error_max, angle);

                                as_poly.append_point(shape.shape.start(), false);

                                for step in 1..=steps {
                                    let rotation: EdaAngle = (angle * step) / steps;
                                    let mut pt = shape.shape.start();

                                    rotate_point_around(&mut pt, center, rotation);
                                    as_poly.append_point(pt, false);
                                }

                                this.test_shape_line_chain(
                                    &mut host,
                                    ctx,
                                    &as_poly,
                                    shape.width(ctx),
                                    layer,
                                    item,
                                    &c,
                                );
                            }

                            // Simple shapes can't create self-intersections, and I'm not sure a
                            // user would want a report that one side of their rectangle was too
                            // close to the other side.
                            ShapeT::Rectangle
                            | ShapeT::Segment
                            | ShapeT::Circle
                            | ShapeT::Ellipse
                            | ShapeT::EllipseArc => {}

                            ShapeT::Undefined => {
                                // UNIMPLEMENTED_FOR( shape->SHAPE_T_asString() ): a debug
                                // assertion in C++.
                                debug_assert!(false, "physical clearance: undefined shape");
                            }
                        }
                    }

                    if zone.is_some() {
                        this.test_zone_layer(&mut host, ctx, item, layer, &c);
                    }
                }

                if host.is_cancelled() {
                    return false;
                }
            }

            !host.is_cancelled()
        });

        self.item_tree.clear();

        !host.is_cancelled()
    }

    /// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE::testShapeLineChain`.
    #[allow(clippy::too_many_arguments)]
    fn test_shape_line_chain(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        outline: &ShapeLineChain,
        line_width: i32,
        layer: PcbLayerId,
        parent_item: ItemRef<'_>,
        constraint: &DrcConstraint,
    ) {
        // We don't want to collide with neighboring segments forming a curve until the
        // concavity approaches 180 degrees.
        let angle_tolerance = deg2rad(180.0 - ADVANCED_CFG_SLIVER_ANGLE_TOLERANCE);
        let epsilon = ctx.board.design_settings().get_drc_epsilon();
        let count = outline.segment_count();
        let clearance = constraint.value().min();

        if constraint.severity() == Severity::Ignore || clearance - epsilon <= 0 {
            return;
        }

        // Trigonometry is not cheap; cache seg angles
        let mut angles: Vec<f64> = Vec::with_capacity(count.max(0) as usize);
        let min_sq = Seg::square(epsilon * 2);

        for ii in 0..count {
            let seg = outline.c_segment(ii);

            // NB: don't store angles of really short segments (which could point anywhere)

            if seg.squared_length() > min_sq {
                angles.push(EdaAngle::from_vector(seg.b - seg.a).as_radians());
            } else if ii > 0 {
                let back = *angles.last().expect("one angle per earlier segment");
                angles.push(back);
            } else {
                for jj in 1..count {
                    let following = outline.c_segment(jj);

                    if following.squared_length() > min_sq || jj == count - 1 {
                        angles.push(EdaAngle::from_vector(following.b - following.a).as_radians());
                        break;
                    }
                }
            }
        }

        // Find collisions before reporting so that we can condense them into fewer reports.
        let mut collisions: Vec<(Vector2I, i32)> = Vec::new();

        for ii in 0..count {
            let seg = outline.c_segment(ii);
            // A single short segment leaves `angles` empty (C++ reads past the end); its angle
            // is never compared then (no candidates).
            let seg_angle = angles.get(ii as usize).copied().unwrap_or(0.0);

            // Exclude segments on either side of us until we reach the angle tolerance
            let mut first_candidate = ii + 1;
            let mut last_candidate = count - 1;

            while first_candidate < count {
                if angle_diff(seg_angle, angles[first_candidate as usize]) < angle_tolerance {
                    first_candidate += 1;
                } else {
                    break;
                }
            }

            if outline.is_closed() {
                if ii > 0 {
                    last_candidate = ii - 1;
                }

                while last_candidate != first_candidate.min(count - 1) {
                    if angle_diff(seg_angle, angles[last_candidate as usize]) < angle_tolerance {
                        last_candidate = if last_candidate == 0 {
                            count - 1
                        } else {
                            last_candidate - 1
                        };
                    } else {
                        break;
                    }
                }
            }

            // Now run the collision between seg and each candidate seg in the candidate range.
            if last_candidate < ii {
                last_candidate = count - 1;
            }

            for jj in first_candidate..=last_candidate {
                let candidate = outline.c_segment(jj);
                let (collides, actual) =
                    seg.collide_actual(&candidate, clearance + line_width - epsilon);

                if collides {
                    let first_point = seg.nearest_point_seg(&candidate);
                    let second_point = candidate.nearest_point_seg(&seg);
                    let pos = (first_point + second_point) / 2;

                    if let Some(last) = collisions.last_mut()
                        && (pos - last.0).euclidean_norm() < clearance * 2
                    {
                        if actual < last.1 {
                            last.0 = pos;
                            last.1 = actual;
                        }

                        continue;
                    }

                    collisions.push((pos, actual));
                }
            }
        }

        for &(collision_pos, collision_actual) in &collisions {
            let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
            let mut pt = collision_pos;

            if let Some(parent_fp) = parent_item.parent_footprint() {
                let fp = parent_fp.as_footprint().expect("a footprint");
                rotate_point(&mut pt, fp.orientation());
                pt += parent_fp.position();
            }

            let msg = self.base.format_msg(
                "Internal clearance violation (%s clearance %s; actual %s)",
                &constraint.name(),
                clearance as f64,
                collision_actual as f64,
                EdaDataType::Distance,
            );

            drc_item.set_error_message(&msg);
            drc_item.set_item_ids(vec![parent_item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            self.base
                .report_violation(host, Arc::new(drc_item), pt, layer.0, &no_path);
        }
    }

    /// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE::testZoneLayer`.
    fn test_zone_layer(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        zone_item: ItemRef<'_>,
        layer: PcbLayerId,
        constraint: &DrcConstraint,
    ) {
        let zone = zone_item.as_zone().expect("a zone");
        let epsilon = ctx.board.design_settings().get_drc_epsilon();
        let clearance = constraint.value().min();

        if constraint.severity() == Severity::Ignore || clearance - epsilon <= 0 {
            return;
        }

        let mut fill = zone.filled_polys_list(layer).clone_drop_triangulation();

        // Turn fractured fill into outlines and holes
        fill.simplify();

        for outline_idx in 0..fill.outline_count() {
            let first_outline = fill.c_outline(outline_idx);

            //
            // Step one: outline to outline clearance violations
            //

            for ii in (outline_idx + 1)..fill.outline_count() {
                let second_outline = fill.c_outline(ii);

                for jj in 0..second_outline.segment_count() {
                    let second_seg = second_outline.segment(jj);
                    let mut actual = 0;
                    let mut pos = Vector2I::default();

                    if first_outline.collide_seg(
                        &second_seg,
                        clearance - epsilon,
                        Some(&mut actual),
                        Some(&mut pos),
                    ) {
                        let mut drc_item =
                            item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                        drc_item.set_error_detail(&self.base.format_msg(
                            "(%s clearance %s; actual %s)",
                            &constraint.name(),
                            clearance as f64,
                            actual as f64,
                            EdaDataType::Distance,
                        ));
                        drc_item.set_item_ids(vec![zone_item.uuid()]);
                        drc_item.violating_rule = constraint.parent_rule.clone();
                        self.base.report_violation(
                            host,
                            Arc::new(drc_item),
                            pos,
                            layer.0,
                            &no_path,
                        );
                    }
                }

                if host.is_cancelled() {
                    return;
                }
            }

            //
            // Step two: interior hole clearance violations
            //

            for hole_idx in 0..fill.hole_count(outline_idx) {
                self.test_shape_line_chain(
                    host,
                    ctx,
                    fill.c_hole(outline_idx, hole_idx),
                    0,
                    layer,
                    zone_item,
                    constraint,
                );

                if host.is_cancelled() {
                    return;
                }
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE::testItemAgainstItem`: the number of
    /// violations reported.
    #[allow(clippy::too_many_arguments)]
    fn test_item_against_item<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        item_shape: &dyn Shape,
        layer: PcbLayerId,
        other: ItemRef<'x>,
    ) -> i32 {
        let mut a_item = item;
        let mut a_other = other;
        let mut a_item_shape = item_shape;
        let test_clearance = !host.is_error_limit_exceeded(DRCE_CLEARANCE);
        let test_holes = !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE);
        let mut constraint = DrcConstraint::default();
        let mut clearance = 0;
        let mut actual = 0;
        let mut violations = 0;
        let mut pos = Vector2I::default();
        let board_copper_layers = Lset::all_cu_mask_n(ctx.board.copper_layer_count());

        let other_shape_storage =
            a_other.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
        let mut other_shape: &dyn Shape = other_shape_storage.as_ref();

        if test_clearance {
            constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::PhysicalClearanceConstraint,
                Some(a_item),
                Some(a_other),
                layer,
            );
            clearance = constraint.value().min();
        }

        if constraint.severity() != Severity::Ignore && clearance > 0 {
            // Collide (and generate violations) based on a well-defined order so that exclusion
            // checking against previously-generated violations will work.
            if a_item.uuid() > a_other.uuid() {
                std::mem::swap(&mut a_item, &mut a_other);
                std::mem::swap(&mut a_item_shape, &mut other_shape);
            }

            if a_item_shape.collide(other_shape, clearance, Some(&mut actual), Some(&mut pos)) {
                let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s clearance %s; actual %s)",
                    &constraint.name(),
                    clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![a_item.uuid(), a_other.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();
                self.base.report_two_shape_geometry(
                    host,
                    Arc::new(drc_item),
                    pos,
                    a_item_shape,
                    other_shape,
                    layer,
                    actual,
                );
                violations += 1;
            }
        }

        if test_holes {
            let mut item_hole_shape: Option<Arc<ShapeSegment>> = None;
            let mut other_hole_shape: Option<Arc<ShapeSegment>> = None;
            clearance = 0;

            if a_item.type_() == KicadT::PcbViaT {
                let mut layers = a_item.layer_set();
                add_hole_layers(&mut layers, &board_copper_layers);

                if !layers.contains(layer) {
                    debug_assert!(
                        false,
                        "Bug!  Vias should only be checked for layers on which they exist"
                    );
                    return violations;
                }

                item_hole_shape =
                    a_item.effective_hole_shape(layer, DrcConstraintT::PhysicalClearanceConstraint);
            } else if a_item.has_hole() {
                item_hole_shape =
                    a_item.effective_hole_shape(layer, DrcConstraintT::PhysicalClearanceConstraint);
            }

            if a_other.type_() == KicadT::PcbViaT {
                let mut layers = a_other.layer_set();
                add_hole_layers(&mut layers, &board_copper_layers);

                if !layers.contains(layer) {
                    debug_assert!(
                        false,
                        "Bug!  Vias should only be checked for layers on which they exist"
                    );
                    return violations;
                }

                other_hole_shape = a_other
                    .effective_hole_shape(layer, DrcConstraintT::PhysicalClearanceConstraint);
            } else if a_other.has_hole() {
                other_hole_shape = a_other
                    .effective_hole_shape(layer, DrcConstraintT::PhysicalClearanceConstraint);
            }

            if item_hole_shape.is_some() || other_hole_shape.is_some() {
                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::PhysicalHoleClearanceConstraint,
                    Some(a_other),
                    Some(a_item),
                    layer,
                );
                clearance = constraint.value().min();
            }

            if constraint.severity() != Severity::Ignore && clearance > 0 {
                if let Some(hole) = &item_hole_shape
                    && hole.collide(other_shape, clearance, Some(&mut actual), Some(&mut pos))
                {
                    let mut drc_item =
                        item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s clearance %s; actual %s)",
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![a_item.uuid(), a_other.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_shape_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        hole.as_ref(),
                        other_shape,
                        layer,
                        actual,
                    );
                    violations += 1;
                }

                if let Some(hole) = &other_hole_shape
                    && hole.collide(a_item_shape, clearance, Some(&mut actual), Some(&mut pos))
                {
                    let mut drc_item =
                        item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s clearance %s; actual %s)",
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![a_item.uuid(), a_other.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_shape_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        hole.as_ref(),
                        a_item_shape,
                        layer,
                        actual,
                    );
                    violations += 1;
                }
            }
        }

        violations
    }

    /// Port of `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE::testItemAgainstZones`.
    fn test_item_against_zones<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        layer: PcbLayerId,
    ) {
        for &zone_id in &self.caches.zones {
            let zone_ref = ctx
                .item(zone_id.into())
                .expect("m_DRCZones holds board zones");
            let zone = zone_ref.as_zone().expect("a zone");

            if !zone_ref.layer_set().test(layer) {
                continue;
            }

            let item_bbox = item.bounding_box();
            let mut worst_case_bbox = item_bbox;

            worst_case_bbox.inflate(self.caches.max_clearance);

            if !worst_case_bbox.intersects(&zone_ref.bounding_box()) {
                continue;
            }

            let test_clearance = !host.is_error_limit_exceeded(DRCE_CLEARANCE);
            let test_holes = !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE);

            if !test_clearance && !test_holes {
                return;
            }

            let zone_rtree = self.caches.copper_zone_rtree(zone_id);
            let mut constraint = DrcConstraint::default();
            let mut clearance = -1;

            // The three collision tests of the C++ (zone tree, footprint zone outline, board
            // zone outline): (colliding, actual, pos).
            let collide_zone = |shape: &dyn Shape, clearance: i32| -> Option<(i32, Vector2I)> {
                // If we have an RTree it's the fastest way to do a collision.  After that, avoid
                // GetBoardOutline() if we can, which makes a copy of the poly.
                if is_copper_layer(layer)
                    && let Some(tree) = zone_rtree
                {
                    tree.query_colliding_box_actual(&item_bbox, shape, layer, clearance)
                } else {
                    let mut actual = 0;
                    let mut pos = Vector2I::default();
                    let colliding = if zone_ref.parent_footprint().is_some() {
                        Shape::collide(
                            &zone.board_outline(ctx),
                            shape,
                            clearance,
                            Some(&mut actual),
                            Some(&mut pos),
                        )
                    } else {
                        Shape::collide(
                            zone.outline(),
                            shape,
                            clearance,
                            Some(&mut actual),
                            Some(&mut pos),
                        )
                    };

                    colliding.then_some((actual, pos))
                }
            };

            if test_clearance {
                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::PhysicalClearanceConstraint,
                    Some(item),
                    Some(zone_ref),
                    layer,
                );
                clearance = constraint.value().min();
            }

            if constraint.severity() != Severity::Ignore && clearance > 0 {
                let mut item_shape: SharedShape =
                    item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

                if item.type_() == KicadT::PcbPadT {
                    let pad = item.as_pad().expect("PCB_PAD_T is a PAD");

                    if !pad.flash_layer(ctx, layer) {
                        let drill = pad.drill_size(ctx);

                        if drill.x == 0 && drill.y == 0 {
                            continue;
                        }

                        let hole = item
                            .effective_hole_shape(
                                layer,
                                DrcConstraintT::PhysicalClearanceConstraint,
                            )
                            .expect("a pad with a drill has a hole shape");
                        let size = hole.get_width();

                        item_shape = Arc::new(ShapeSegment::from_seg(*hole.get_seg(), size));
                    }
                }

                if let Some((actual, pos)) = collide_zone(item_shape.as_ref(), clearance) {
                    let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s clearance %s; actual %s)",
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![item.uuid(), zone_ref.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_item_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        item,
                        zone_ref,
                        layer,
                        actual,
                    );
                }
            }

            if test_holes {
                let mut hole_shape: Option<Arc<ShapeSegment>> = None;

                if item.type_() == KicadT::PcbViaT {
                    if item.layer_set().contains(layer) {
                        hole_shape = item.effective_hole_shape(
                            layer,
                            DrcConstraintT::PhysicalClearanceConstraint,
                        );
                    }
                } else if item.has_hole() {
                    hole_shape = item
                        .effective_hole_shape(layer, DrcConstraintT::PhysicalClearanceConstraint);
                }

                if let Some(hole_shape) = hole_shape {
                    constraint = engine.eval_rules(
                        ctx,
                        DrcConstraintT::PhysicalHoleClearanceConstraint,
                        Some(item),
                        Some(zone_ref),
                        layer,
                    );
                    clearance = constraint.value().min();

                    if constraint.severity() != Severity::Ignore
                        && clearance > 0
                        && let Some((actual, pos)) = collide_zone(hole_shape.as_ref(), clearance)
                    {
                        let mut drc_item =
                            item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                        drc_item.set_error_detail(&self.base.format_msg(
                            "(%s clearance %s; actual %s)",
                            &constraint.name(),
                            clearance as f64,
                            actual as f64,
                            EdaDataType::Distance,
                        ));
                        drc_item.set_item_ids(vec![item.uuid(), zone_ref.uuid()]);
                        drc_item.violating_rule = constraint.parent_rule.clone();

                        let zone_shape = zone_ref.effective_shape(
                            layer,
                            Flashing::Default,
                            DrcConstraintT::NullConstraint,
                        );
                        self.base.report_two_shape_geometry(
                            host,
                            Arc::new(drc_item),
                            pos,
                            hole_shape.as_ref(),
                            zone_shape.as_ref(),
                            layer,
                            actual,
                        );
                    }
                }
            }

            if host.is_cancelled() {
                return;
            }
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderPhysicalClearance<'_> {
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
