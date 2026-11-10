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

//! Port of `pcbnew/drc/drc_test_provider_track_width.cpp`.
//!
//! Track width test. As the name says, checks width of the tracks (including segments and
//! arcs). Errors generated:
//! - `DRCE_TRACK_WIDTH`

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::DRCE_TRACK_WIDTH;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_TRACK_WIDTH`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderTrackWidth {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderTrackWidth {
    fn default() -> Self {
        DrcTestProviderTrackWidth {
            base: DrcTestProviderBase::new("width"),
        }
    }
}

impl DrcTestProviderTrackWidth {
    /// Port of `DRC_TEST_PROVIDER_TRACK_WIDTH::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_TRACK_WIDTH) {
            report_aux(run, "Track width violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        if !engine.has_rules_for_constraint_type(DrcConstraintT::TrackWidthConstraint) {
            report_aux(run, "No track width constraints found. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking track widths...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let progress_delta = 250;
        let tracks_size = board.tracks().count();

        for (ii, (_, track)) in board.tracks().enumerate() {
            if !self
                .base
                .report_progress(&mut host, ii, tracks_size, progress_delta)
            {
                break;
            }

            if !self.check_track_width(engine, &mut host, ctx, track) {
                break;
            }
        }

        !host.is_cancelled()
    }

    /// The `checkTrackWidth` lambda of `Run`: false when the error limit is reached.
    fn check_track_width(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        track: &Track,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_TRACK_WIDTH) {
            return false;
        }

        let actual: i32;
        let p0: Vector2I;

        match track {
            Track::Arc(arc) => {
                actual = arc.track.width();
                p0 = arc.track.start();
            }
            Track::Trace(t) => {
                actual = t.width();
                p0 = (t.start() + t.end()) / 2;
            }
            Track::Via(_) => return true,
        }

        let item = ctx.view(AnyItem::Track(track));
        let layer = item.layer();
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::TrackWidthConstraint,
            Some(item),
            None,
            layer,
        );
        let mut fail_min = false;
        let mut fail_max = false;
        let mut constraint_width = 0;

        if constraint.severity() != Severity::Ignore {
            if constraint.value().has_min() && actual < constraint.value().min() {
                fail_min = true;
                constraint_width = constraint.value().min();
            }

            if constraint.value().has_max() && actual > constraint.value().max() {
                fail_max = true;
                constraint_width = constraint.value().max();
            }
        }

        if fail_min || fail_max {
            let mut drc_item = item::create(DRCE_TRACK_WIDTH).expect("DRCE_TRACK_WIDTH item");
            let mut constraint_name = constraint.name();

            if fail_min {
                if constraint.implicit_min {
                    constraint_name = "board setup constraints".to_string();
                }

                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min width %s; actual %s)",
                    &constraint_name,
                    constraint_width as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
            } else {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max width %s; actual %s)",
                    &constraint_name,
                    constraint_width as f64,
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

impl DrcTestProviderRun for DrcTestProviderTrackWidth {
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
