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

//! Port of `pcbnew/drc/drc_test_provider_disallow.cpp`.
//!
//! "Disallow" test. Goes through all items, matching types/conditions drop errors.
//! Errors generated:
//! - `DRCE_ALLOWED_ITEMS`
//! - `DRCE_TEXT_ON_EDGECUTS`
//!
//! Sequential port of the two thread-pool loops (`drc.md` 4.10.2): the items are processed in
//! `forEachGeometryItem` order, which is one of the orders the C++ can produce. The C++ report
//! order is scheduling dependent (compare as a set); the violation set is deterministic.
//!
//! The first C++ loop pre-computes `board->m_IntersectsKeepoutCache[ (keepout, copper zone,
//! UNDEFINED_LAYER) ]` for every (no-zone-fills keepout, copper zone) pair. The expression
//! function `intersectsKeepout` looks that cache up per concrete layer only, so the entries are
//! never read: the pre-computation has no observable effect and is not ported (see
//! `PORT_STATUS.md`, DRC-10).

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::{Flashing, PcbLayerId, UNDEFINED_LAYER};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{DRCE_ALLOWED_ITEMS, DRCE_TEXT_ON_EDGECUTS, DrcConstraint};
use kicad_pcb::{BoardCtx, ItemId, ItemRef};

use super::run_host::RunHost;
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rtree::DrcRtree;
use crate::test_provider::{DrcTestProviderBase, for_each_geometry_item, no_path};

/// Port of `DRC_TEST_PROVIDER_DISALLOW`.
///
/// Holds the run's board caches for `board->m_DRCMaxPhysicalClearance` (the search distance of
/// the keepout r-tree query).
#[derive(Clone, Debug)]
pub struct DrcTestProviderDisallow<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches of the run (`DRC_CACHE_GENERATOR`).
    caches: &'c DrcBoardCaches,
}

/// The `checkTextOnEdgeCuts` lambda: items that plot geometry onto Edge.Cuts corrupt the board
/// outline. Reference images are excluded on purpose because they are never plotted.
fn check_text_on_edge_cuts(item: &ItemRef<'_>) -> bool {
    let t = item.type_();

    if t == KicadT::PcbFieldT
        || t == KicadT::PcbTextT
        || t == KicadT::PcbTextboxT
        || t.base_type() == KicadT::PcbTableT
        || t == KicadT::PcbBarcodeT
        || t.base_type() == KicadT::PcbDimensionT
    {
        return item.layer() == PcbLayerId::EdgeCuts;
    }

    false
}

impl<'c> DrcTestProviderDisallow<'c> {
    /// `DRC_TEST_PROVIDER_DISALLOW()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderDisallow {
            base: DrcTestProviderBase::new("disallow"),
            caches,
        }
    }

    /// Port of `DRC_TEST_PROVIDER_DISALLOW::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking keepouts & disallow constraints...")
        {
            return false; // DRC cancelled
        }

        // First build out the board's cache of copper-keepout to copper-zone caches. (The
        // pre-computation itself is not ported: its cache entries are never read, see the
        // module documentation. The r-tree of the no-tracks keepouts is.)
        let mut anti_track_keepouts = DrcRtree::new();

        for_each_geometry_item(ctx, &[KicadT::PcbZoneT], &Lset::all_layers_mask(), |item| {
            let zone = item.as_zone().expect("PCB_ZONE_T item is a zone");

            if zone.is_rule_area() && zone.do_not_allow_tracks {
                for layer in item.layer_set().seq() {
                    anti_track_keepouts.insert(
                        item,
                        layer,
                        DrcConstraintT::ClearanceConstraint,
                        0,
                        false,
                    );
                }
            }

            true
        });

        anti_track_keepouts.build();

        if host.is_cancelled() {
            return false;
        }

        // Now go through all the board objects calling the DRC_ENGINE to run the actual
        // disallow tests.
        //
        // Collect items first, then process them (in parallel in C++).
        let mut all_items: Vec<ItemRef<'_>> = Vec::new();

        for_each_geometry_item(ctx, &[], &Lset::all_layers_mask(), |item| {
            all_items.push(item);
            true
        });

        let item_count = all_items.len();
        let max_physical_clearance = self.caches.max_physical_clearance;

        for (items_done, item) in all_items.into_iter().enumerate() {
            self.base
                .report_progress(&mut host, items_done, item_count, 1);

            if host.is_cancelled() {
                break;
            }

            self.process_item(
                engine,
                &mut host,
                ctx,
                item,
                &anti_track_keepouts,
                max_physical_clearance,
            );
        }

        !host.is_cancelled()
    }

    /// The `processItem` lambda of `Run` for one item.
    fn process_item<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        anti_track_keepouts: &DrcRtree,
        max_physical_clearance: i32,
    ) {
        let test_text_on_edge = !host.is_error_limit_exceeded(DRCE_TEXT_ON_EDGECUTS);
        let test_disallow = !host.is_error_limit_exceeded(DRCE_ALLOWED_ITEMS);

        if !test_text_on_edge && !test_disallow {
            return;
        }

        if test_text_on_edge && check_text_on_edge_cuts(&item) {
            let mut drc = item::create(DRCE_TEXT_ON_EDGECUTS).expect("DRCE_TEXT_ON_EDGECUTS item");
            drc.set_item_ids(vec![item.uuid()]);
            self.base.report_violation(
                host,
                Arc::new(drc),
                item.position(),
                PcbLayerId::EdgeCuts.0,
                &no_path,
            );
        }

        if !test_disallow {
            return;
        }

        if let Some(zone) = item.as_zone() {
            if zone.is_rule_area() && zone.has_keepout_parameters_set() {
                return;
            }
        }

        // item->ClearFlags( HOLE_PROXY ): `item` is a plain reference here.
        let is_track_or_arc = item.type_() == KicadT::PcbTraceT || item.type_() == KicadT::PcbArcT;

        if is_track_or_arc {
            let layer = item.layer();

            let mut accept_all = |_: ItemId| -> bool { true };
            let mut visitor = |other: ItemId| -> bool {
                let shape = item.effective_shape(
                    UNDEFINED_LAYER,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );
                let mut dummy_actual = 0;
                let mut pos = Vector2I::default();
                let other = ctx.item(other).expect("keepout tree entry is a board item");
                let zone = other.as_zone().expect("keepout tree entry is a zone");

                // GetBoardOutline() is expensive.  Only use it in DRC where we have to.
                let zone_outline_storage: ShapePolySet;
                let zone_outline: &ShapePolySet = if other.parent_footprint().is_some() {
                    zone_outline_storage = zone.board_outline(ctx);
                    &zone_outline_storage
                } else {
                    zone.outline()
                };

                if zone_outline.collide_shape(
                    shape.as_ref(),
                    0,
                    Some(&mut dummy_actual),
                    Some(&mut pos),
                ) {
                    let mut drc_item =
                        item::create(DRCE_ALLOWED_ITEMS).expect("DRCE_ALLOWED_ITEMS item");
                    drc_item.set_item_ids(vec![item.uuid()]);
                    self.base.report_violation(
                        host,
                        Arc::new(drc_item),
                        pos,
                        item.layer_set().extract_layer().0,
                        &no_path,
                    );
                }

                !host.is_cancelled()
            };

            anti_track_keepouts.query_colliding(
                item,
                layer,
                layer,
                Some(&mut accept_all),
                Some(&mut visitor),
                max_physical_clearance,
            );
        }

        // Tracks and arcs against keepout areas that disallow tracks are already reported
        // above via antiTrackKeepouts (which collides every crossing, not just one per rule
        // match).  Skip the track/arc case for implicit keepout rules here to avoid duplicate
        // markers, but still let EvalRules produce markers for all other item types against
        // implicit keepout rules.
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::DisallowConstraint,
            Some(item),
            None,
            UNDEFINED_LAYER,
        );

        if constraint.disallow_flags != 0 && constraint.severity() != Severity::Ignore {
            self.report_disallow(engine, host, ctx, item, is_track_or_arc, &constraint);
        }

        // N.B. HOLE_PROXY is set/cleared on the item's flags in C++ for EvalRules to
        // distinguish hole-specific disallow constraints; the port evaluates a hole-proxy
        // reference of the item instead (the board is not mutated).
        if item.has_hole() {
            let hole_proxy = item.as_hole_proxy();

            let constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::DisallowConstraint,
                Some(hole_proxy),
                None,
                UNDEFINED_LAYER,
            );

            if constraint.disallow_flags != 0 && constraint.severity() != Severity::Ignore {
                self.report_disallow(engine, host, ctx, hole_proxy, is_track_or_arc, &constraint);
            }
        }
    }

    /// The `reportDisallow` lambda of `Run`.
    fn report_disallow<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        is_track_or_arc: bool,
        constraint: &DrcConstraint,
    ) {
        let Some(rule) = constraint.parent_rule.as_ref() else {
            return;
        };

        if is_track_or_arc && rule.implicit {
            return;
        }

        let mut drc_item = item::create(DRCE_ALLOWED_ITEMS).expect("DRCE_ALLOWED_ITEMS item");
        let layer = item.layer_set().extract_layer();
        let mut pos = item.position();

        // Provide a better location for keepout area collisions by snapping to where the item
        // actually crosses the keepout outline.  Use the cached BOARD_ITEM* (the rule's
        // m_ImplicitItem) rather than a UUID lookup.
        if rule.implicit {
            let keepout = rule
                .rule_index
                .and_then(|index| engine.rule(index))
                .and_then(|r| r.implicit_item)
                .and_then(|id| ctx.item(id))
                .filter(|z| z.as_zone().is_some());

            if let Some(keepout) = keepout {
                let zone = keepout.as_zone().expect("filtered to zones");
                let shape =
                    item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
                let mut dummy_actual = 0;

                // This is only done when reporting collisions, so we can afford the more
                // expensive GetBoardOutline().
                let keepout_outline = zone.board_outline(ctx);
                keepout_outline.collide_shape(
                    shape.as_ref(),
                    0,
                    Some(&mut dummy_actual),
                    Some(&mut pos),
                );
            }
        }

        drc_item.set_error_detail(&format!("({})", constraint.name()));
        drc_item.set_item_ids(vec![item.uuid()]);
        drc_item.violating_rule = Some(rule.clone());
        self.base
            .report_violation(host, Arc::new(drc_item), pos, layer.0, &no_path);
    }
}

impl DrcTestProviderRun for DrcTestProviderDisallow<'_> {
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
