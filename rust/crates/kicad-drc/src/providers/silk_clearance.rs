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

//! Port of `pcbnew/drc/drc_test_provider_silk_clearance.cpp`.
//!
//! Silk to silk clearance test. Check all silkscreen features against each other.
//! Errors generated:
//! - `DRCE_SILK_CLEARANCE`
//! - `DRCE_SILK_MASK_CLEARANCE` (silk against the mask layers)
//!
//! Sequential in C++ as well: the violations come in the visit order of
//! `DRC_RTREE::QueryCollidingPairs` (layer pair, silk entry storage order, target tree search
//! order).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_SILK_CLEARANCE, DRCE_SILK_MASK_CLEARANCE};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rtree::{ATOMIC_TABLES, DrcRtree, ItemWithShape, LayerPair};
use crate::test_provider::{
    DrcTestProviderBase, all_basic_items, for_each_geometry_item, is_invisible_text,
};

/// Port of `DRC_TEST_PROVIDER_SILK_CLEARANCE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderSilkClearance {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_largestClearance`
    pub largest_clearance: i32,
}

impl Default for DrcTestProviderSilkClearance {
    /// `DRC_TEST_PROVIDER_SILK_CLEARANCE()`.
    fn default() -> Self {
        DrcTestProviderSilkClearance {
            base: DrcTestProviderBase::new("silk_clearance"),
            largest_clearance: 0,
        }
    }
}

/// The 18 `DRC_RTREE::LAYER_PAIR`s of `Run` (silk layer, target layer), in C++ order.
const LAYER_PAIRS: [LayerPair; 18] = [
    (PcbLayerId::FSilkS, PcbLayerId::FSilkS),
    (PcbLayerId::FSilkS, PcbLayerId::FMask),
    (PcbLayerId::FSilkS, PcbLayerId::FAdhes),
    (PcbLayerId::FSilkS, PcbLayerId::FPaste),
    (PcbLayerId::FSilkS, PcbLayerId::FCrtYd),
    (PcbLayerId::FSilkS, PcbLayerId::FFab),
    (PcbLayerId::FSilkS, PcbLayerId::FCu),
    (PcbLayerId::FSilkS, PcbLayerId::EdgeCuts),
    (PcbLayerId::FSilkS, PcbLayerId::Margin),
    (PcbLayerId::BSilkS, PcbLayerId::BSilkS),
    (PcbLayerId::BSilkS, PcbLayerId::BMask),
    (PcbLayerId::BSilkS, PcbLayerId::BAdhes),
    (PcbLayerId::BSilkS, PcbLayerId::BPaste),
    (PcbLayerId::BSilkS, PcbLayerId::BCrtYd),
    (PcbLayerId::BSilkS, PcbLayerId::BFab),
    (PcbLayerId::BSilkS, PcbLayerId::BCu),
    (PcbLayerId::BSilkS, PcbLayerId::EdgeCuts),
    (PcbLayerId::BSilkS, PcbLayerId::Margin),
];

impl DrcTestProviderSilkClearance {
    /// Port of `DRC_TEST_PROVIDER_SILK_CLEARANCE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        let progress_delta: usize = 500;
        let board = ctx.board;

        // If the soldermask min width is greater than 0 then we must use a healing algorithm to
        // generate a whole-board soldermask poly, and then test against that.  However, that
        // can't deal well with DRC exclusions (as any change anywhere on the board that affects
        // the soldermask will null the associated exclusions), so we only use that when
        // soldermask min width is > 0.
        let check_individual_mask_items = board.design_settings().solder_mask_min_width <= 0;

        if run.is_error_limit_exceeded(DRCE_SILK_CLEARANCE)
            && run.is_error_limit_exceeded(DRCE_SILK_MASK_CLEARANCE)
        {
            return true; // continue with other tests
        }

        self.largest_clearance = 0;

        if let Some(worst) =
            engine.query_worst_constraint(DrcConstraintT::SilkClearanceConstraint, false)
        {
            self.largest_clearance = worst.value().min();
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking silkscreen for overlapping items...")
        {
            return false; // DRC cancelled
        }

        let base = &self.base;
        let mut silk_tree = DrcRtree::new();
        let mut target_tree = DrcRtree::new();
        let mut ii: usize = 0;
        let mut items: usize = 0;
        let silk_layers = Lset::from_layers(&[PcbLayerId::FSilkS, PcbLayerId::BSilkS]);
        let mut target_layers = Lset::front_mask().clone();
        target_layers |= Lset::back_mask();
        target_layers |= &Lset::from_layers(&[PcbLayerId::EdgeCuts, PcbLayerId::Margin]);

        // Rule areas are purely logical (no physical copper, mask, or silk) so they must never
        // participate in silk-clearance collisions.  Their effective shape now follows the
        // outline (so disallow checks can collide against them), which would otherwise cause
        // false silk-to-rule-area violations.
        let is_rule_area = |item: &kicad_pcb::ItemRef<'_>| -> bool {
            item.type_() == KicadT::PcbZoneT && item.as_zone().is_some_and(|z| z.is_rule_area())
        };

        // countItems
        for_each_geometry_item(ctx, all_basic_items(), &silk_layers, |_item| {
            items += 1;
            true
        });
        for_each_geometry_item(ctx, all_basic_items(), &target_layers, |_item| {
            items += 1;
            true
        });

        // addToSilkTree
        for_each_geometry_item(ctx, all_basic_items(), &silk_layers, |item| {
            let i = ii;
            ii += 1;

            if !base.report_progress(&mut host, i, items, progress_delta) {
                return false;
            }

            if is_rule_area(&item) {
                return true;
            }

            for layer in [PcbLayerId::FSilkS, PcbLayerId::BSilkS] {
                if item.is_on_layer(layer) {
                    silk_tree.insert(
                        item,
                        layer,
                        DrcConstraintT::SilkClearanceConstraint,
                        0,
                        ATOMIC_TABLES,
                    );
                }
            }

            true
        });

        // addToTargetTree
        for_each_geometry_item(ctx, all_basic_items(), &target_layers, |item| {
            let i = ii;
            ii += 1;

            if !base.report_progress(&mut host, i, items, progress_delta) {
                return false;
            }

            if is_rule_area(&item) {
                return true;
            }

            let mut layers = item.layer_set();
            layers &= &target_layers;

            for layer in layers.iter() {
                target_tree.insert(
                    item,
                    layer,
                    DrcConstraintT::SilkClearanceConstraint,
                    0,
                    ATOMIC_TABLES,
                );
            }

            true
        });

        silk_tree.build();
        target_tree.build();

        report_aux(
            host.0,
            &format!(
                "Testing {} silkscreen features against {} board items.",
                silk_tree.len(),
                target_tree.len()
            ),
        );

        // Cache the board-outline bounding box and per-subshape collision results so that each
        // subshape is only tested against the outline once during the visitor sweep.  Without
        // caching, QueryCollidingPairs invokes the visitor O(silk * target) times and the
        // outline Collide (which walks the outline's triangulation) was dominating DRC runtime
        // on boards with many silkscreen/mask polygons (see issue 24007).
        //
        // `BoardOutline()->GetOutline()` with the box caches the cache generator built
        // (`BuildBBoxCaches()`): built here on a copy.
        let board_outline = {
            let outline = board.board_outline(ctx);

            // if( boardOutline && !boardOutline->HasOutline() ) boardOutline = nullptr;
            if outline.is_empty() {
                None
            } else {
                let mut outline = (*outline).clone();
                outline.build_bbox_caches();
                Some(outline)
            }
        };
        let outline_bbox = board_outline.as_ref().map(|o| o.bbox_from_caches());

        // Keyed by the tree entry (C++: its `const SHAPE*`); a pure memo of the collision.
        let mut outline_collision_cache: HashMap<*const ItemWithShape, bool> = HashMap::new();

        let host = RefCell::new(host);

        target_tree.query_colliding_pairs(
            &silk_tree,
            &LAYER_PAIRS,
            |layers: &LayerPair,
             ref_item_shape: &ItemWithShape,
             test_item_shape: &ItemWithShape,
             collision_detected: &mut bool|
             -> bool {
                let mut host = host.borrow_mut();
                let mut ref_item = ctx
                    .item(ref_item_shape.parent)
                    .expect("DRC_RTREE entries refer to board items");
                let mut ref_shape: &dyn Shape = ref_item_shape.shape();
                let mut test_item = ctx
                    .item(test_item_shape.parent)
                    .expect("DRC_RTREE entries refer to board items");
                let mut test_shape: &dyn Shape = test_item_shape.shape();

                let hole: Option<Arc<ShapeSegment>>;

                if host.is_error_limit_exceeded(DRCE_SILK_CLEARANCE)
                    && host.is_error_limit_exceeded(DRCE_SILK_MASK_CLEARANCE)
                {
                    return false;
                }

                if is_invisible_text(ref_item) || is_invisible_text(test_item) {
                    return true;
                }

                if test_item.is_tented(layers.0) {
                    if test_item.has_hole() {
                        hole = test_item.effective_hole_shape(
                            PcbLayerId::UndefinedLayer,
                            DrcConstraintT::NullConstraint,
                        );

                        // testShape = hole.get(): a null hole pointer is never returned for
                        // an item with a hole.
                        match &hole {
                            Some(h) => test_shape = h.as_ref(),
                            None => {
                                debug_assert!(false, "HasHole() without a hole shape");
                                return true;
                            }
                        }
                    } else {
                        return true;
                    }
                }

                if let (Some(outline), Some(outline_bbox)) = (&board_outline, &outline_bbox) {
                    if !test_item.bounding_box().intersects(outline_bbox) {
                        return true;
                    }

                    // Only cache for shapes owned by the R-tree (stable pointers).  Hole
                    // shapes are freshly created per visitor call via shared_ptr, so their
                    // raw addresses cannot be safely used as cache keys.
                    let collides_outline = if std::ptr::addr_eq(
                        test_shape as *const dyn Shape,
                        test_item_shape.shape() as *const dyn Shape,
                    ) {
                        *outline_collision_cache
                            .entry(std::ptr::from_ref(test_item_shape))
                            .or_insert_with(|| test_shape.collide(outline, 0, None, None))
                    } else {
                        test_shape.collide(outline, 0, None, None)
                    };

                    if !collides_outline {
                        return true;
                    }
                }

                let mut error_code = DRCE_SILK_CLEARANCE;
                let constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::SilkClearanceConstraint,
                    Some(ref_item),
                    Some(test_item),
                    layers.1,
                );
                let mut min_clearance = -1;

                if !constraint.is_null() && constraint.severity() != Severity::Ignore {
                    min_clearance = constraint.value().min();
                }

                if layers.1 == PcbLayerId::FMask || layers.1 == PcbLayerId::BMask {
                    if check_individual_mask_items {
                        min_clearance = min_clearance.max(0);
                    }

                    error_code = DRCE_SILK_MASK_CLEARANCE;
                }

                if min_clearance < 0 || host.is_error_limit_exceeded(error_code) {
                    return true;
                }

                let mut actual = 0;
                let mut pos = Vector2I::default();

                // Graphics are often compound shapes so ignore collisions between shapes in a
                // single footprint or on the board (both parent footprints will be nullptr).
                if ref_item.type_() == KicadT::PcbShapeT
                    && test_item.type_() == KicadT::PcbShapeT
                    && ref_item.parent_footprint().and_then(|f| f.id())
                        == test_item.parent_footprint().and_then(|f| f.id())
                {
                    return true;
                }

                // Collide (and generate violations) based on a well-defined order so that
                // exclusion checking against previously-generated violations will work.
                if layers.0 == layers.1 && ref_item.uuid() > test_item.uuid() {
                    std::mem::swap(&mut ref_item, &mut test_item);
                    std::mem::swap(&mut ref_shape, &mut test_shape);
                }

                if ref_shape.collide(test_shape, min_clearance, Some(&mut actual), Some(&mut pos)) {
                    let mut drc_item = item::create(error_code).expect("silk clearance item");

                    if min_clearance > 0 {
                        drc_item.set_error_detail(&base.format_msg(
                            "(%s clearance %s; actual %s)",
                            &constraint.name(),
                            min_clearance as f64,
                            actual as f64,
                            EdaDataType::Distance,
                        ));
                    }

                    drc_item.set_item_ids(vec![ref_item.uuid(), test_item.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    base.report_two_shape_geometry(
                        &mut *host,
                        Arc::new(drc_item),
                        pos,
                        ref_shape,
                        test_shape,
                        layers.1,
                        actual,
                    );
                    *collision_detected = true;
                }

                true
            },
            self.largest_clearance,
            |count, size| {
                base.report_progress(
                    &mut *host.borrow_mut(),
                    count as usize,
                    size as usize,
                    progress_delta,
                )
            },
        );

        !host.borrow().is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderSilkClearance {
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
