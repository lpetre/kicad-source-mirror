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

//! Port of `pcbnew/drc/drc_test_provider_track_angle.cpp`.
//!
//! Track angle test. Checks the angle between two connected track segments. Errors
//! generated:
//! - `DRCE_TRACK_ANGLE`
//!
//! The C++ checks the tracks on the thread pool (one task per track index); the port walks
//! them sequentially in index order. Within one track the connected tracks come from
//! `CONNECTIVITY_DATA::GetConnectedTracks`, a `std::set<PCB_TRACK*>` (address order) in C++
//! and `ItemId` order (board track order) in the port. The report order is therefore
//! scheduling and address dependent in C++; the set of violations is the same.

use std::sync::Arc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::eda_units::EdaUnits;
use kicad_core::{Lset, Severity};
use kicad_geom::geometry::eda_angle::{ANGLE_0, ANGLE_90, EdaAngle, EdaAngleT};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::math::vector2::Vector2D;
use kicad_pcb::drc_api::DRCE_TRACK_ANGLE;
use kicad_pcb::ids::ItemId;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_TRACK_ANGLE`. The board's connectivity
/// (`BOARD::GetConnectivity()`) is passed in at construction.
#[derive(Clone)]
pub struct DrcTestProviderTrackAngle<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_drcEngine->GetBoard()->GetConnectivity()`.
    connectivity: &'c ConnectivityData,
}

impl<'c> DrcTestProviderTrackAngle<'c> {
    /// `DRC_TEST_PROVIDER_TRACK_ANGLE()` with the board's connectivity.
    pub fn new(connectivity: &'c ConnectivityData) -> Self {
        DrcTestProviderTrackAngle {
            base: DrcTestProviderBase::new("track angle"),
            connectivity,
        }
    }

    /// Port of `DRC_TEST_PROVIDER_TRACK_ANGLE::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_TRACK_ANGLE) {
            report_aux(run, "Track angle violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        if !engine.has_rules_for_constraint_type(DrcConstraintT::TrackAngleConstraint) {
            report_aux(run, "No track angle constraints found. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking track angles...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let progress_delta = 250;
        let tracks_size = board.tracks().count();

        // tp.submit_loop( 0, Tracks().size(), checkTrackAngle ), sequentially. A task that
        // returns false (error limit reached) only ends its own index; every later index
        // returns false as well, so stopping here is the same.
        for (ii, (track_id, track)) in board.tracks().enumerate() {
            if !self
                .base
                .report_progress(&mut host, ii, tracks_size, progress_delta)
            {
                break;
            }

            if !self.check_track_angle(engine, &mut host, ctx, ItemId::from(track_id), track) {
                break;
            }
        }

        !host.is_cancelled()
    }

    /// The `checkTrackAngle` lambda of `Run`: false when the error limit is reached.
    fn check_track_angle(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        track_id: ItemId,
        track: &Track,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_TRACK_ANGLE) {
            return false;
        }

        let Track::Trace(trace) = track else {
            return true;
        };

        let segment = Seg::new(trace.start(), trace.end());
        let item = ctx.view(AnyItem::Track(track));
        let item_layer = item.layer();

        for other_id in self.connectivity.connected_tracks(track_id) {
            let Some(other_ref) = ctx.item(other_id) else {
                continue;
            };

            let Some(Track::Trace(other)) = other_ref.as_track() else {
                continue;
            };

            let other_segment = Seg::new(other.start(), other.end());

            let Some(p0) = segment.intersect(&other_segment) else {
                continue;
            };

            if ctx
                .board
                .get_pad_at(ctx, p0, Lset::from_layers(&[item_layer]))
                .is_some()
            {
                continue;
            }

            let constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::TrackAngleConstraint,
                Some(item),
                Some(other_ref),
                item_layer,
            );

            let mut direction = Vector2D::from(trace.end() - trace.start()).resize(1.0);
            let mut other_direction = Vector2D::from(other.end() - other.start()).resize(1.0);
            let mut actual: EdaAngle;
            let mut angle_below_90 = false;

            if segment.b == p0 {
                direction *= -1.0;
            } else if segment.a != p0 {
                angle_below_90 = true;
            }

            if other_segment.b == p0 {
                other_direction *= -1.0;
            } else if other_segment.a != p0 {
                angle_below_90 = true;
            }

            actual = EdaAngle::arccos(direction.dot(other_direction));

            if angle_below_90 && actual > ANGLE_90 {
                actual -= ANGLE_90;
            }

            let mut fail_min = false;
            let mut fail_max = false;
            let mut constraint_angle = ANGLE_0;

            if constraint.severity() != Severity::Ignore {
                if constraint.value().has_min()
                    && actual.as_degrees() < f64::from(constraint.value().min())
                {
                    fail_min = true;
                    constraint_angle =
                        EdaAngle::new(f64::from(constraint.value().min()), EdaAngleT::Degrees);
                }

                if constraint.value().has_max()
                    && actual.as_degrees() > f64::from(constraint.value().max())
                {
                    fail_max = true;
                    constraint_angle =
                        EdaAngle::new(f64::from(constraint.value().max()), EdaAngleT::Degrees);
                }
            }

            if fail_min || fail_max {
                let mut drc_item = item::create(DRCE_TRACK_ANGLE).expect("DRCE_TRACK_ANGLE item");
                let constraint_name = constraint.name();

                if fail_min {
                    drc_item.set_error_detail(&self.base.format_msg_angle(
                        "(%s min angle %s; actual %s)",
                        &constraint_name,
                        constraint_angle.as_degrees(),
                        actual.as_degrees(),
                    ));
                } else {
                    drc_item.set_error_detail(&self.base.format_msg_angle(
                        "(%s max angle %s; actual %s)",
                        &constraint_name,
                        constraint_angle.as_degrees(),
                        actual.as_degrees(),
                    ));
                }

                drc_item.set_item_ids(vec![item.uuid(), other_ref.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    p0,
                    item_layer.0,
                    &crate::test_provider::no_path,
                );
            }
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderTrackAngle<'_> {
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
