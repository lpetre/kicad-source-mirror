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

//! Port of `pcbnew/drc/drc_test_provider_hole_size.cpp`.
//!
//! Drill sizes test. Errors generated:
//! - `DRCE_DRILL_OUT_OF_RANGE`
//! - `DRCE_MICROVIA_DRILL_OUT_OF_RANGE`

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{DRCE_DRILL_OUT_OF_RANGE, DRCE_MICROVIA_DRILL_OUT_OF_RANGE};
use kicad_pcb::pad::Pad;
use kicad_pcb::pcb_track::{Track, Viatype};
use kicad_pcb::{AnyItem, BoardCtx};

use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::providers::run_host::RunHost;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_HOLE_SIZE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderHoleSize {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderHoleSize {
    fn default() -> Self {
        DrcTestProviderHoleSize {
            base: DrcTestProviderBase::new("hole_size"),
        }
    }
}

impl DrcTestProviderHoleSize {
    /// Port of `DRC_TEST_PROVIDER_HOLE_SIZE::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);
        let board = ctx.board;

        if !host.is_error_limit_exceeded(DRCE_DRILL_OUT_OF_RANGE) {
            if !self.base.report_phase(&mut host, "Checking pad holes...") {
                return false; // DRC cancelled
            }

            for (fp_id, _) in board.footprints() {
                for (_, pad) in board.footprint_pads(fp_id) {
                    if !host.is_error_limit_exceeded(DRCE_DRILL_OUT_OF_RANGE) {
                        self.check_pad_hole(engine, &mut host, ctx, pad);
                    }
                }
            }
        }

        if !host.is_error_limit_exceeded(DRCE_MICROVIA_DRILL_OUT_OF_RANGE)
            || !host.is_error_limit_exceeded(DRCE_DRILL_OUT_OF_RANGE)
        {
            if !host.is_error_limit_exceeded(DRCE_DRILL_OUT_OF_RANGE) {
                if !self.base.report_phase(&mut host, "Checking via holes...") {
                    return false; // DRC cancelled
                }
            } else if !self
                .base
                .report_phase(&mut host, "Checking micro-via holes...")
            {
                return false; // DRC cancelled
            }

            for (_, track) in board.tracks() {
                if let Track::Via(_) = track {
                    let exceed_micro =
                        host.is_error_limit_exceeded(DRCE_MICROVIA_DRILL_OUT_OF_RANGE);
                    let exceed_std = host.is_error_limit_exceeded(DRCE_DRILL_OUT_OF_RANGE);

                    if exceed_micro && exceed_std {
                        break;
                    }

                    self.check_via_hole(engine, &mut host, ctx, track, exceed_micro, exceed_std);
                }
            }
        }

        !host.is_cancelled()
    }

    /// Port of `DRC_TEST_PROVIDER_HOLE_SIZE::checkPadHole`.
    fn check_pad_hole(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        pad: &Pad,
    ) {
        let drill_size = pad.drill_size(ctx);
        let hole_minor = drill_size.x.min(drill_size.y);
        let hole_major = drill_size.x.max(drill_size.y);

        if hole_minor == 0 {
            return;
        }

        let item = ctx.view(AnyItem::Pad(pad));
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::HoleSizeConstraint,
            Some(item),
            None,
            PcbLayerId::UndefinedLayer, // holes are not layer-specific
        );
        let mut fail_min = false;
        let mut fail_max = false;
        let mut constraint_value = 0;
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();

        if constraint.severity() == Severity::Ignore {
            return;
        }

        if constraint.value().has_max() && hole_major > constraint.value().max() {
            fail_max = true;
            constraint_value = constraint.value().max();

            pt_a = pad.position(ctx);

            if drill_size.x == drill_size.y {
                pt_b = pt_a - drill_size.resize(drill_size.x / 2);
            } else if drill_size.x > drill_size.y {
                pt_b = pt_a - Vector2I::new(drill_size.x / 2, 0);
            } else {
                pt_b = pt_a - Vector2I::new(0, drill_size.y / 2);
            }
        }

        if constraint.value().has_min() && hole_minor < constraint.value().min() {
            fail_min = true;
            constraint_value = constraint.value().min();

            pt_a = pad.position(ctx);

            if drill_size.x == drill_size.y {
                pt_b = pt_a - drill_size.resize(drill_size.x / 2);
            } else if drill_size.x < drill_size.y {
                pt_b = pt_a - Vector2I::new(drill_size.x / 2, 0);
            } else {
                pt_b = pt_a - Vector2I::new(0, drill_size.y / 2);
            }
        }

        if fail_min || fail_max {
            let mut drc_item =
                item::create(DRCE_DRILL_OUT_OF_RANGE).expect("DRCE_DRILL_OUT_OF_RANGE item");
            let mut constraint_name = constraint.name();

            if fail_min {
                if constraint.parent_rule.as_ref().is_some_and(|r| r.implicit) {
                    constraint_name = "board setup constraints".to_string();
                }

                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min hole %s; actual %s)",
                    &constraint_name,
                    constraint_value as f64,
                    hole_minor as f64,
                    EdaDataType::Distance,
                ));
            } else {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max hole %s; actual %s)",
                    &constraint_name,
                    constraint_value as f64,
                    hole_major as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();
            self.base.report_two_point_geometry(
                host,
                Arc::new(drc_item),
                pt_a,
                pt_a,
                pt_b,
                PcbLayerId::UndefinedLayer,
            );
        }
    }

    /// Port of `DRC_TEST_PROVIDER_HOLE_SIZE::checkViaHole`.
    fn check_via_hole(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        track: &Track,
        exceed_micro: bool,
        exceed_std: bool,
    ) {
        let Track::Via(via) = track else {
            return;
        };

        let error_code = if via.via_type() == Viatype::Microvia {
            if exceed_micro {
                return;
            }

            DRCE_MICROVIA_DRILL_OUT_OF_RANGE
        } else {
            if exceed_std {
                return;
            }

            DRCE_DRILL_OUT_OF_RANGE
        };

        let item = ctx.view(AnyItem::Track(track));
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::HoleSizeConstraint,
            Some(item),
            None,
            PcbLayerId::UndefinedLayer, // holes are not layer-specific
        );
        let mut fail_min = false;
        let mut fail_max = false;
        let mut constraint_value = 0;
        let drill = via.drill_value(ctx);

        if constraint.severity() == Severity::Ignore {
            return;
        }

        if constraint.value().has_min() && drill < constraint.value().min() {
            fail_min = true;
            constraint_value = constraint.value().min();
        }

        if constraint.value().has_max() && drill > constraint.value().max() {
            fail_max = true;
            constraint_value = constraint.value().max();
        }

        if fail_min || fail_max {
            let mut drc_item = item::create(error_code).expect("hole size item");
            let mut constraint_name = constraint.name();

            if fail_min {
                if constraint.implicit_min {
                    constraint_name = "board setup constraints".to_string();
                }

                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min hole %s; actual %s)",
                    &constraint_name,
                    constraint_value as f64,
                    drill as f64,
                    EdaDataType::Distance,
                ));
            } else {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max hole %s; actual %s)",
                    &constraint_name,
                    constraint_value as f64,
                    drill as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            let pt_a = via.position(ctx);

            let pt_b = pt_a - Vector2I::new(drill, drill).resize(drill / 2);
            self.base.report_two_point_geometry(
                host,
                Arc::new(drc_item),
                pt_a,
                pt_a,
                pt_b,
                PcbLayerId::UndefinedLayer,
            );
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderHoleSize {
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
