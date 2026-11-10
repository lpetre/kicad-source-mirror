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

//! Port of `pcbnew/drc/drc_test_provider_hole_to_hole.cpp`.
//!
//! Holes clearance test. Checks pad and via holes for their mechanical clearances.
//! Generated errors:
//! - `DRCE_DRILLED_HOLES_TOO_CLOSE`
//! - `DRCE_DRILLED_HOLES_COLOCATED`
//!
//! Sequential in C++ as well. The pair de-duplication map (`std::unordered_map` keyed by the
//! two item addresses in address order) is only used for lookups; the port keys it by the two
//! item handles in handle order (`drc.md` 4.12), which de-duplicates the same unordered pairs.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{PcbLayerId, UNDEFINED_LAYER};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_pcb::drc_api::{DRCE_DRILLED_HOLES_COLOCATED, DRCE_DRILLED_HOLES_TOO_CLOSE};
use kicad_pcb::pcb_track::Viatype;
use kicad_pcb::{AnyItem, BoardCtx, ItemId, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rtree::DrcRtree;
use crate::test_provider::{DrcTestProviderBase, for_each_geometry_item};

/// Port of `DRC_TEST_PROVIDER_HOLE_TO_HOLE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderHoleToHole {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_holeTree`
    hole_tree: DrcRtree,
    /// `m_largestHoleToHoleClearance`
    largest_hole_to_hole_clearance: i32,
}

impl Default for DrcTestProviderHoleToHole {
    fn default() -> Self {
        DrcTestProviderHoleToHole {
            base: DrcTestProviderBase::new("hole_to_hole_clearance"),
            hole_tree: DrcRtree::new(),
            largest_hole_to_hole_clearance: 0,
        }
    }
}

/// The two parents of a pair in canonical order (C++: by address; any canonical order gives
/// the same de-duplication).
fn canonical(a: ItemId, b: ItemId) -> (ItemId, ItemId) {
    if a > b { (b, a) } else { (a, b) }
}

impl DrcTestProviderHoleToHole {
    /// Port of `DRC_TEST_PROVIDER_HOLE_TO_HOLE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        if run.is_error_limit_exceeded(DRCE_DRILLED_HOLES_TOO_CLOSE)
            && run.is_error_limit_exceeded(DRCE_DRILLED_HOLES_COLOCATED)
        {
            report_aux(run, "Hole to hole violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        let board = ctx.board;

        match engine.query_worst_constraint(DrcConstraintT::HoleToHoleConstraint, false) {
            Some(worst) => self.largest_hole_to_hole_clearance = worst.value().min(),
            None => {
                report_aux(run, "No hole to hole constraints found. Skipping check.");
                return true; // continue with other tests
            }
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking hole to hole clearances...")
        {
            return false; // DRC cancelled
        }

        let progress_delta: usize = 200;
        let mut count: usize = 0;
        let mut ii: usize = 0;
        let largest = self.largest_hole_to_hole_clearance;
        let base = self.base.clone();

        self.hole_tree.clear();

        let pad_via = [KicadT::PcbPadT, KicadT::PcbViaT];

        for_each_geometry_item(ctx, &pad_via, Lset::all_layers_mask(), |_item| {
            count += 1;
            true
        });

        count *= 2; // One for adding to the rtree; one for checking

        {
            let hole_tree = &mut self.hole_tree;

            for_each_geometry_item(ctx, &pad_via, Lset::all_layers_mask(), |item| {
                let i = ii;
                ii += 1;

                if !base.report_progress(&mut host, i, count, progress_delta) {
                    return false;
                }

                if item.type_() == KicadT::PcbPadT {
                    // Index every drilled or milled hole, including oval (slotted) holes.  A
                    // slot too close to another hole or slot is still a manufacturing defect.
                    if item.has_hole() {
                        hole_tree.insert(
                            item,
                            PcbLayerId::EdgeCuts,
                            DrcConstraintT::HoleClearanceConstraint,
                            largest,
                            false,
                        );
                    }
                } else if item.type_() == KicadT::PcbViaT {
                    // Blind/buried/microvias will be drilled/burned _prior_ to lamination, so
                    // subsequently drilled holes need to avoid them.
                    hole_tree.insert(
                        item,
                        PcbLayerId::EdgeCuts,
                        DrcConstraintT::HoleClearanceConstraint,
                        largest,
                        false,
                    );
                }

                true
            });
        }

        self.hole_tree.build();

        // std::unordered_map<PTR_PTR_CACHE_KEY, int>: lookups only. Kept across both passes so
        // a via/pad pair tested in the via pass is not reported a second time when the pad
        // queries the via below.
        let checked_pairs: RefCell<HashSet<(ItemId, ItemId)>> = RefCell::new(HashSet::new());
        let this = &*self;

        // The shared body of both passes: query the hole tree with `item` (`hole_shape` is its
        // effective hole shape).
        let query = |host: &mut RunHost<'_, '_>, item: ItemRef<'_>, hole_shape: &ShapeSegment| {
            let item_id = item.id().expect("board items have a handle");

            // Filter:
            let mut filter = |other: ItemId| -> bool {
                // store canonical order so we don't collide in both directions (a:b and b:a)
                checked_pairs.borrow_mut().insert(canonical(item_id, other))
            };

            // Visitor:
            let mut visitor = |other: ItemId| -> bool {
                let other_ref = ctx
                    .item(other)
                    .expect("DRC_RTREE entries refer to board items");
                this.test_hole_against_hole(engine, host, ctx, item, hole_shape, other_ref)
            };

            this.hole_tree.query_colliding(
                item,
                PcbLayerId::EdgeCuts,
                PcbLayerId::EdgeCuts,
                Some(&mut filter),
                Some(&mut visitor),
                largest,
            );
        };

        for (_, track) in board.tracks() {
            let item = ctx.view(AnyItem::Track(track));

            if item.type_() != KicadT::PcbViaT {
                continue;
            }

            let via = item.as_via().expect("PCB_VIA_T is a via");

            let i = ii;
            ii += 1;

            if !base.report_progress(&mut host, i, count, progress_delta) {
                return false; // DRC cancelled
            }

            // We only care about mechanically drilled (ie: non-laser) holes.  These include both
            // blind/buried via holes (drilled prior to lamination) and through-via and drilled
            // pad holes (which are generally drilled post laminataion).
            if via.via_type() != Viatype::Microvia {
                let hole_shape = item
                    .effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::HoleToHoleConstraint)
                    .expect("a via has a hole shape");

                query(&mut host, item, hole_shape.as_ref());
            }
        }

        for (fp_id, _) in board.footprints() {
            for (_, pad) in board.footprint_pads(fp_id) {
                let item = ctx.view(AnyItem::Pad(pad));

                let i = ii;
                ii += 1;

                if !base.report_progress(&mut host, i, count, progress_delta) {
                    return false; // DRC cancelled
                }

                // Test every drilled or milled hole, including oval (slotted) holes
                if item.has_hole() {
                    let hole_shape = item
                        .effective_hole_shape(
                            UNDEFINED_LAYER,
                            DrcConstraintT::HoleToHoleConstraint,
                        )
                        .expect("a pad with a hole has a hole shape");

                    query(&mut host, item, hole_shape.as_ref());
                }
            }

            if host.is_cancelled() {
                return false;
            }
        }

        !host.is_cancelled()
    }

    /// Port of `DRC_TEST_PROVIDER_HOLE_TO_HOLE::testHoleAgainstHole`: false stops the query.
    fn test_hole_against_hole(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        hole: &ShapeSegment,
        other: ItemRef<'_>,
    ) -> bool {
        let report_co_location = !host.is_error_limit_exceeded(DRCE_DRILLED_HOLES_COLOCATED);
        let report_hole2hole = !host.is_error_limit_exceeded(DRCE_DRILLED_HOLES_TOO_CLOSE);

        if !report_co_location && !report_hole2hole {
            return false;
        }

        let Some(other_hole) =
            other.effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::HoleToHoleConstraint)
        else {
            // C++ dereferences the result: every item of the hole tree has a hole.
            debug_assert!(false, "hole tree item without a hole shape");
            return !host.is_cancelled();
        };
        let epsilon = ctx.board.design_settings().get_drc_epsilon();
        let epsilon_sq: i64 = epsilon as i64 * epsilon as i64;

        // Blind-buried vias are drilled prior to stackup; they're only an issue if they share
        // layers
        if item.type_() == KicadT::PcbViaT && other.type_() == KicadT::PcbViaT {
            let via_hole_layers = &item.layer_set() & Lset::all_cu_mask();

            if (&via_hole_layers & &other.layer_set()).none() {
                return false;
            }
        }

        let (mut a_item, mut a_other) = (item, other);

        // Holes at same location generate a separate violation
        if (hole.get_center() - other_hole.get_center()).squared_euclidean_norm() < epsilon_sq {
            if report_co_location {
                // Generate violations based on a well-defined order so that exclusion checking
                // against previously-generated violations will work.
                if a_item.uuid() > a_other.uuid() {
                    std::mem::swap(&mut a_item, &mut a_other);
                }

                let mut drc_item = item::create(DRCE_DRILLED_HOLES_COLOCATED)
                    .expect("DRCE_DRILLED_HOLES_COLOCATED item");
                drc_item.set_item_ids(vec![a_item.uuid(), a_other.uuid()]);
                self.base.report_two_point_geometry(
                    host,
                    Arc::new(drc_item),
                    hole.get_center(),
                    hole.get_center(),
                    hole.get_center(),
                    UNDEFINED_LAYER,
                );
            }
        } else if report_hole2hole {
            // Measure between the hole axes, then back off the two half-widths.  For a round
            // hole the segment is zero-length and its width is the drill diameter, so this
            // reduces to the centre-to-centre distance less the two radii; for a slot it follows
            // the milled oval correctly.
            let mut actual = hole.get_seg().distance_seg(other_hole.get_seg());
            actual = 0.max(actual - hole.get_width() / 2 - other_hole.get_width() / 2);

            let constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::HoleToHoleConstraint,
                Some(a_item),
                Some(a_other),
                UNDEFINED_LAYER, /* holes pierce all layers */
            );
            let min_clearance = constraint.value().min();

            // Relax the comparison by the epsilon, but quote the rule as the user entered it
            if constraint.severity() != Severity::Ignore
                && min_clearance >= 0
                && actual < 0.max(min_clearance - epsilon)
            {
                // Generate violations based on a well-defined order so that exclusion checking
                // against previously-generated violations will work.
                if a_item.uuid() > a_other.uuid() {
                    std::mem::swap(&mut a_item, &mut a_other);
                }

                let mut drc_item = item::create(DRCE_DRILLED_HOLES_TOO_CLOSE)
                    .expect("DRCE_DRILLED_HOLES_TOO_CLOSE item");
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min %s; actual %s)",
                    &constraint.name(),
                    min_clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![a_item.uuid(), a_other.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();
                self.base.report_two_shape_geometry(
                    host,
                    Arc::new(drc_item),
                    hole.get_center(),
                    hole,
                    other_hole.as_ref(),
                    UNDEFINED_LAYER,
                    actual,
                );
            }
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderHoleToHole {
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
