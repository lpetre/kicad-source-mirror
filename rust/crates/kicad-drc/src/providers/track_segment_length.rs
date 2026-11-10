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

//! Port of `pcbnew/drc/drc_test_provider_track_segment_length.cpp`.
//!
//! Track segment length test. As the name says, checks segment length of the tracks
//! (including segments and arcs). Errors generated:
//! - `DRCE_TRACK_SEGMENT_LENGTH`
//!
//! The C++ checks the tracks on the thread pool (one task per track index); the port walks
//! them sequentially in index order. Only the report order differs from a pooled C++ run
//! (`drc.md` 4.x: scheduling dependent), the set of violations is the same.

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::DRCE_TRACK_SEGMENT_LENGTH;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_TRACK_SEGMENT_LENGTH`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderTrackSegmentLength {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderTrackSegmentLength {
    fn default() -> Self {
        DrcTestProviderTrackSegmentLength {
            base: DrcTestProviderBase::new("segment_length"),
        }
    }
}

impl DrcTestProviderTrackSegmentLength {
    /// Port of `DRC_TEST_PROVIDER_TRACK_SEGMENT_LENGTH::Run`: false when the run was
    /// cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_TRACK_SEGMENT_LENGTH) {
            report_aux(
                run,
                "Track segment length violations ignored. Tests not run.",
            );
            return true; // continue with other tests
        }

        if !engine.has_rules_for_constraint_type(DrcConstraintT::TrackSegmentLengthConstraint) {
            report_aux(
                run,
                "No track segment length constraints found. Tests not run.",
            );
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking track segment lengths...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let progress_delta = 250;
        let tracks_size = board.tracks().count();

        // tp.submit_loop( 0, Tracks().size(), checkTrackSegmentLength ), sequentially. A
        // task that returns false (error limit reached) only ends its own index; every later
        // index returns false as well, so stopping here is the same.
        for (ii, (_, track)) in board.tracks().enumerate() {
            if !self
                .base
                .report_progress(&mut host, ii, tracks_size, progress_delta)
            {
                break;
            }

            if !self.check_track_segment_length(engine, &mut host, ctx, track) {
                break;
            }
        }

        !host.is_cancelled()
    }

    /// The `checkTrackSegmentLength` lambda of `Run`: false when the error limit is reached.
    fn check_track_segment_length(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        track: &Track,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_TRACK_SEGMENT_LENGTH) {
            return false;
        }

        let actual: i32;
        let p0: Vector2I;

        match track {
            Track::Arc(arc) => {
                // actual = arc->GetLength(): an implicit double to int conversion (x86-64
                // `cvttsd2si` for an out-of-range length of a degenerate arc).
                actual = f64_to_i32_x86(arc.length());
                p0 = arc.track.start();
            }
            Track::Trace(t) => {
                actual = f64_to_i32_x86(t.length());
                p0 = (t.start() + t.end()) / 2;
            }
            Track::Via(_) => return true,
        }

        let item = ctx.view(AnyItem::Track(track));
        let layer = item.layer();
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::TrackSegmentLengthConstraint,
            Some(item),
            None,
            layer,
        );
        let mut fail_min = false;
        let mut fail_max = false;
        let mut constraint_length = 0;

        if constraint.severity() != Severity::Ignore {
            if constraint.value().has_min() && actual < constraint.value().min() {
                fail_min = true;
                constraint_length = constraint.value().min();
            }

            if constraint.value().has_max() && actual > constraint.value().max() {
                fail_max = true;
                constraint_length = constraint.value().max();
            }
        }

        if fail_min || fail_max {
            let mut drc_item =
                item::create(DRCE_TRACK_SEGMENT_LENGTH).expect("DRCE_TRACK_SEGMENT_LENGTH item");
            let mut constraint_name = constraint.name();

            if fail_min {
                if constraint.implicit_min {
                    constraint_name = "board setup constraints".to_string();
                }

                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min length %s; actual %s)",
                    &constraint_name,
                    constraint_length as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
            } else {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max length %s; actual %s)",
                    &constraint_name,
                    constraint_length as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            self.base.report_violation(
                host,
                Arc::new(drc_item),
                p0,
                layer.0,
                &crate::test_provider::no_path,
            );
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderTrackSegmentLength {
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
