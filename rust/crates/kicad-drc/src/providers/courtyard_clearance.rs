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

//! Port of `pcbnew/drc/drc_test_provider_courtyard_clearance.cpp`.
//!
//! Courtyard clearance. Tests for malformed component courtyards and overlapping footprints.
//! Generated errors:
//! - `DRCE_OVERLAPPING_FOOTPRINTS`
//! - `DRCE_MISSING_COURTYARD`
//! - `DRCE_MALFORMED_COURTYARD`
//! - `DRCE_PTH_IN_COURTYARD`
//! - `DRCE_NPTH_IN_COURTYARD`
//!
//! The courtyards come from the footprint courtyard caches (`FOOTPRINT::GetCourtyard`, package
//! M20 of `kicad-pcb`). `GetCourtyard` returns a reference to the cache in C++, and the
//! `BuildBBoxCaches()` calls of the first pass write the cached outline boxes into that shared
//! cache; the port gets the courtyards by value, so it keeps the copies of the first pass (with
//! their box caches) for the second pass, see [`FootprintCourtyards`].

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{PcbLayerId, UNDEFINED_LAYER};
use kicad_geom::{ShapePolySet, Vector2I};
use kicad_pcb::drc_api::{
    DRCE_MALFORMED_COURTYARD, DRCE_MISSING_COURTYARD, DRCE_NPTH_IN_COURTYARD,
    DRCE_OVERLAPPING_FOOTPRINTS, DRCE_PTH_IN_COURTYARD,
};
use kicad_pcb::footprint::Footprint;
use kicad_pcb::ids::{FootprintId, ItemId};
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadAttrib, PadProp};
use kicad_pcb::{AnyItem, BoardCtx, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rule::DrcConstraintExt;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderCourtyardClearance {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_largestCourtyardClearance`
    pub largest_courtyard_clearance: i32,
}

impl Default for DrcTestProviderCourtyardClearance {
    /// `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE()`: `m_isRuleDriven = false`.
    fn default() -> Self {
        let mut base = DrcTestProviderBase::new("courtyard_clearance");
        base.is_rule_driven = false;

        DrcTestProviderCourtyardClearance {
            base,
            largest_courtyard_clearance: 0,
        }
    }
}

/// The front and back courtyards of one footprint as the C++ cache holds them during the run
/// (`GetCourtyard( F_CrtYd )` / `GetCourtyard( B_CrtYd )`), including the outline box caches
/// built by the first pass.
#[derive(Clone, Debug, Default)]
pub struct FootprintCourtyards {
    /// `GetCourtyard( F_CrtYd )`
    pub front: ShapePolySet,
    /// `GetCourtyard( B_CrtYd )`
    pub back: ShapePolySet,
}

impl FootprintCourtyards {
    /// Fetches both courtyards of `footprint` (`GetCourtyard`).
    fn fetch(ctx: &BoardCtx<'_>, footprint: &Footprint) -> Self {
        FootprintCourtyards {
            front: footprint.courtyard(ctx, PcbLayerId::FCrtYd),
            back: footprint.courtyard(ctx, PcbLayerId::BCrtYd),
        }
    }
}

impl DrcTestProviderCourtyardClearance {
    /// Port of `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE::testFootprintCourtyardDefinitions`:
    /// `None` when the run was cancelled, else the courtyards the pass looked at, per
    /// footprint (with the box caches the pass built).
    fn test_footprint_courtyard_definitions(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) -> Option<Vec<Option<FootprintCourtyards>>> {
        let board = ctx.board;
        let footprint_count = board.footprints().count();
        let mut courtyards: Vec<Option<FootprintCourtyards>> = vec![None; footprint_count];

        // Detects missing (or malformed) footprint courtyards
        if !host.is_error_limit_exceeded(DRCE_MALFORMED_COURTYARD)
            || !host.is_error_limit_exceeded(DRCE_MISSING_COURTYARD)
        {
            if !self
                .base
                .report_phase(host, "Checking footprint courtyard definitions...")
            {
                return None; // DRC cancelled
            }
        } else if !host.is_error_limit_exceeded(DRCE_OVERLAPPING_FOOTPRINTS) {
            if !self
                .base
                .report_phase(host, "Gathering footprint courtyards...")
            {
                return None; // DRC cancelled
            }
        } else {
            report_aux(host.0, "All courtyard violations ignored. Tests not run.");
            return Some(courtyards); // continue with other tests
        }

        let progress_delta = 500;

        for (ii, (_, footprint)) in board.footprints().enumerate() {
            if !self
                .base
                .report_progress(host, ii, footprint_count, progress_delta)
            {
                return None; // DRC cancelled
            }

            let flags = footprint.courtyard_flags(ctx);
            let fp_item = ctx.view(AnyItem::Footprint(footprint));

            if flags.malformed_front || flags.malformed_back {
                if host.is_error_limit_exceeded(DRCE_MALFORMED_COURTYARD) {
                    continue;
                }

                let base = &self.base;
                let fp_uuid = fp_item.uuid();
                let mut error_handler =
                    |msg: &str, _a: Option<ItemId>, _b: Option<ItemId>, pt: Vector2I| {
                        let mut drc_item = item::create(DRCE_MALFORMED_COURTYARD)
                            .expect("DRCE_MALFORMED_COURTYARD item");
                        drc_item.set_error_detail(msg);
                        drc_item.set_item_ids(vec![fp_uuid]);
                        base.report_violation(
                            host,
                            Arc::new(drc_item),
                            pt,
                            UNDEFINED_LAYER.0,
                            &no_path,
                        );
                    };

                // Re-run courtyard tests to generate DRC_ITEMs
                footprint.build_courtyard_caches(ctx, Some(&mut error_handler));
            } else {
                let mut fp_courtyards = FootprintCourtyards::fetch(ctx, footprint);

                if fp_courtyards.front.outline_count() == 0
                    && fp_courtyards.back.outline_count() == 0
                {
                    if host.is_error_limit_exceeded(DRCE_MISSING_COURTYARD) {
                        continue;
                    }

                    if footprint.allow_missing_courtyard() {
                        continue;
                    }

                    let mut drc_item =
                        item::create(DRCE_MISSING_COURTYARD).expect("DRCE_MISSING_COURTYARD item");
                    drc_item.set_item_ids(vec![fp_item.uuid()]);
                    self.base.report_violation(
                        host,
                        Arc::new(drc_item),
                        fp_item.position(),
                        UNDEFINED_LAYER.0,
                        &no_path,
                    );
                } else {
                    fp_courtyards.front.build_bbox_caches();
                    fp_courtyards.back.build_bbox_caches();
                    courtyards[ii] = Some(fp_courtyards);
                }
            }
        }

        if host.is_cancelled() {
            None
        } else {
            Some(courtyards)
        }
    }

    /// Port of `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE::testCourtyardClearances`: false when
    /// the run was cancelled. `cached` holds the courtyards of the first pass by board
    /// footprint index (`None`: not looked at there, fetched again).
    fn test_courtyard_clearances(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        cached: Vec<Option<FootprintCourtyards>>,
    ) -> bool {
        if !self
            .base
            .report_phase(host, "Checking footprints for overlapping courtyards...")
        {
            return false; // DRC cancelled
        }

        let progress_delta = 100;

        // Stable sorting gives stable violation generation (and stable comparisons to
        // previously-generated violations for exclusion checking).
        struct Entry<'b> {
            id: FootprintId,
            item: ItemRef<'b>,
            courtyards: FootprintCourtyards,
        }

        let mut footprints: Vec<Entry<'_>> = Vec::new();
        let mut cached = cached.into_iter();

        for (id, footprint) in ctx.board.footprints() {
            let courtyards = match cached.next().flatten() {
                Some(c) => c,
                None => FootprintCourtyards::fetch(ctx, footprint),
            };

            footprints.push(Entry {
                id,
                item: ctx.view(AnyItem::Footprint(footprint)),
                courtyards,
            });
        }

        kicad_geom::stdsort::std_sort(&mut footprints, |a, b| a.item.uuid() < b.item.uuid());

        for (ii, entry_a) in footprints.iter().enumerate() {
            if !self
                .base
                .report_progress(host, ii, footprints.len(), progress_delta)
            {
                return false; // DRC cancelled
            }

            // Ensure tests realted to courtyard constraints are not fully disabled:
            if host.is_error_limit_exceeded(DRCE_OVERLAPPING_FOOTPRINTS)
                && host.is_error_limit_exceeded(DRCE_PTH_IN_COURTYARD)
                && host.is_error_limit_exceeded(DRCE_NPTH_IN_COURTYARD)
            {
                return true; // continue with other tests
            }

            let fp_a = entry_a.item;
            let front_a = &entry_a.courtyards.front;
            let back_a = &entry_a.courtyards.back;

            if front_a.outline_count() == 0
                && back_a.outline_count() == 0
                && host.is_error_limit_exceeded(DRCE_PTH_IN_COURTYARD)
                && host.is_error_limit_exceeded(DRCE_NPTH_IN_COURTYARD)
            {
                // No courtyards defined and no hole testing against other footprint's
                // courtyards
                continue;
            }

            let mut front_a_worst_case_bbox = front_a.bbox_from_caches();
            let mut back_a_worst_case_bbox = back_a.bbox_from_caches();

            front_a_worst_case_bbox.inflate(self.largest_courtyard_clearance);
            back_a_worst_case_bbox.inflate(self.largest_courtyard_clearance);

            let fp_a_bbox = fp_a.bounding_box();

            for entry_b in &footprints[ii + 1..] {
                let fp_b = entry_b.item;
                let front_b = &entry_b.courtyards.front;
                let back_b = &entry_b.courtyards.back;

                if front_b.outline_count() == 0
                    && back_b.outline_count() == 0
                    && host.is_error_limit_exceeded(DRCE_PTH_IN_COURTYARD)
                    && host.is_error_limit_exceeded(DRCE_NPTH_IN_COURTYARD)
                {
                    // No courtyards defined and no hole testing against other footprint's
                    // courtyards
                    continue;
                }

                let mut front_b_worst_case_bbox = front_b.bbox_from_caches();
                let mut back_b_worst_case_bbox = back_b.bbox_from_caches();

                front_b_worst_case_bbox.inflate(self.largest_courtyard_clearance);
                back_b_worst_case_bbox.inflate(self.largest_courtyard_clearance);

                let fp_b_bbox = fp_b.bounding_box();

                // Check courtyard-to-courtyard collisions on front of board,
                // if DRCE_OVERLAPPING_FOOTPRINTS is not diasbled
                if front_a.outline_count() > 0
                    && front_b.outline_count() > 0
                    && front_a_worst_case_bbox.intersects(&front_b.bbox_from_caches())
                    && !host.is_error_limit_exceeded(DRCE_OVERLAPPING_FOOTPRINTS)
                {
                    self.test_courtyard_overlap(
                        engine,
                        host,
                        ctx,
                        (fp_a, fp_b),
                        (front_a, front_b),
                        PcbLayerId::FCu,
                        PcbLayerId::FCrtYd,
                    );
                }

                // Check courtyard-to-courtyard collisions on back of board,
                // if DRCE_OVERLAPPING_FOOTPRINTS is not disabled
                if back_a.outline_count() > 0
                    && back_b.outline_count() > 0
                    && back_a_worst_case_bbox.intersects(&back_b.bbox_from_caches())
                    && !host.is_error_limit_exceeded(DRCE_OVERLAPPING_FOOTPRINTS)
                {
                    self.test_courtyard_overlap(
                        engine,
                        host,
                        ctx,
                        (fp_a, fp_b),
                        (back_a, back_b),
                        PcbLayerId::BCu,
                        PcbLayerId::BCrtYd,
                    );
                }

                //
                // Check pad-hole-to-courtyard collisions on front and back of board.
                //
                // NB: via holes are not checked.  There is a presumption that a physical object
                // goes through a pad hole, which is not the case for via holes.
                //
                let mut check_front = false;
                let mut check_back = false;

                // checkFront/checkBack are only consulted by the pad-hole tests below, which
                // run only when a footprint bbox overlaps the other's inflated courtyard.
                // Evaluating the courtyard-clearance rule for every pair up front is the
                // dominant cost on dense boards, so gate it behind the same bbox test and skip
                // it for far-apart pairs.
                let a_against_b = (front_a.outline_count() > 0
                    && front_a_worst_case_bbox.intersects(&fp_b_bbox))
                    || (back_a.outline_count() > 0
                        && back_a_worst_case_bbox.intersects(&fp_b_bbox));
                let b_against_a = (front_b.outline_count() > 0
                    && front_b.bbox_from_caches().intersects(&fp_a_bbox))
                    || (back_b.outline_count() > 0
                        && back_b.bbox_from_caches().intersects(&fp_a_bbox));
                let pad_check_possible = a_against_b || b_against_a;

                if pad_check_possible {
                    let checks = [
                        (fp_a, fp_b, PcbLayerId::FCu),
                        (fp_b, fp_a, PcbLayerId::FCu),
                        (fp_a, fp_b, PcbLayerId::BCu),
                        (fp_b, fp_a, PcbLayerId::BCu),
                    ];

                    for (first, second, layer) in checks {
                        let constraint = engine.eval_rules(
                            ctx,
                            DrcConstraintT::CourtyardClearanceConstraint,
                            Some(first),
                            Some(second),
                            layer,
                        );
                        let clearance = constraint.value().min();

                        if constraint.severity() != Severity::Ignore && clearance >= 0 {
                            if layer == PcbLayerId::FCu {
                                check_front = true;
                            } else {
                                check_back = true;
                            }
                        }
                    }
                }

                let checks = (check_front, check_back);

                // The two `if` statements of the C++ evaluate their conditions afresh; the
                // values cannot change in between (no violation changes a courtyard or a bbox).
                if a_against_b {
                    for (_, pad_b) in ctx.board.footprint_pads(entry_b.id) {
                        self.test_pad_against_courtyards(
                            host,
                            ctx,
                            pad_b,
                            fp_a,
                            &entry_a.courtyards,
                            checks,
                        );
                    }
                }

                if b_against_a {
                    for (_, pad_a) in ctx.board.footprint_pads(entry_a.id) {
                        self.test_pad_against_courtyards(
                            host,
                            ctx,
                            pad_a,
                            fp_b,
                            &entry_b.courtyards,
                            checks,
                        );
                    }
                }

                if host.is_cancelled() {
                    return false;
                }
            }
        }

        !host.is_cancelled()
    }

    /// One courtyard-to-courtyard block of `testCourtyardClearances` (front: `F_Cu` /
    /// `F_CrtYd`, back: `B_Cu` / `B_CrtYd`).
    #[allow(clippy::too_many_arguments)]
    fn test_courtyard_overlap(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        (fp_a, fp_b): (ItemRef<'_>, ItemRef<'_>),
        (courtyard_a, courtyard_b): (&ShapePolySet, &ShapePolySet),
        rule_layer: PcbLayerId,
        marker_layer: PcbLayerId,
    ) {
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::CourtyardClearanceConstraint,
            Some(fp_a),
            Some(fp_b),
            rule_layer,
        );
        let clearance = constraint.value().min();
        let mut actual = 0;
        let mut pos = Vector2I::default();

        if constraint.severity() != Severity::Ignore
            && clearance >= 0
            && courtyard_a.collide_shape(courtyard_b, clearance, Some(&mut actual), Some(&mut pos))
        {
            let mut drc_item = item::create(DRCE_OVERLAPPING_FOOTPRINTS)
                .expect("DRCE_OVERLAPPING_FOOTPRINTS item");

            if clearance > 0 {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s clearance %s; actual %s)",
                    &constraint.name(),
                    clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.violating_rule = constraint.parent_rule.clone();
            drc_item.set_item_ids(vec![fp_a.uuid(), fp_b.uuid()]);
            self.base.report_two_shape_geometry(
                host,
                Arc::new(drc_item),
                pos,
                courtyard_a,
                courtyard_b,
                marker_layer,
                actual,
            );
        }
    }

    /// The `testPadAgainstCourtyards` lambda of `testCourtyardClearances`: `fp` is the
    /// footprint whose courtyards (`courtyards`) the pad is tested against, `checks` the
    /// `( checkFront, checkBack )` of the pair.
    fn test_pad_against_courtyards(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        pad: &Pad,
        fp: ItemRef<'_>,
        courtyards: &FootprintCourtyards,
        (check_front, check_back): (bool, bool),
    ) {
        let error_code;

        if pad.property() == PadProp::Heatsink {
            return;
        } else if pad.attribute() == PadAttrib::Pth {
            error_code = DRCE_PTH_IN_COURTYARD;
        } else if pad.attribute() == PadAttrib::Npth {
            error_code = DRCE_NPTH_IN_COURTYARD;
        } else {
            return;
        }

        if host.is_error_limit_exceeded(error_code) {
            return;
        }

        let pad_item = ctx.view(AnyItem::Pad(pad));

        if pad_item.has_hole() {
            let hole = pad_item
                .effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::NullConstraint)
                .expect("PAD::GetEffectiveHoleShape is never null");
            let front = &courtyards.front;
            let back = &courtyards.back;

            if check_front
                && front.outline_count() > 0
                && front.collide_shape(hole.as_ref(), 0, None, None)
            {
                let mut drce = item::create(error_code).expect("courtyard hole item");
                drce.set_item_ids(vec![pad_item.uuid(), fp.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drce),
                    pad_item.position(),
                    PcbLayerId::FCrtYd.0,
                    &no_path,
                );
            } else if check_back
                && back.outline_count() > 0
                && back.collide_shape(hole.as_ref(), 0, None, None)
            {
                let mut drce = item::create(error_code).expect("courtyard hole item");
                drce.set_item_ids(vec![pad_item.uuid(), fp.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drce),
                    pad_item.position(),
                    PcbLayerId::BCrtYd.0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        if let Some(constraint) =
            engine.query_worst_constraint(DrcConstraintT::CourtyardClearanceConstraint, false)
        {
            self.largest_courtyard_clearance = constraint.value().min();
        }

        let mut host = RunHost(run);

        let Some(courtyards) = self.test_footprint_courtyard_definitions(&mut host, ctx) else {
            return false;
        };

        if !self.test_courtyard_clearances(engine, &mut host, ctx, courtyards) {
            return false;
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderCourtyardClearance {
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
