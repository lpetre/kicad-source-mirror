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

//! Port of `pcbnew/drc/drc_test_provider_via_diameter.cpp`.
//!
//! Via diameter test. Errors generated:
//! - `DRCE_VIA_DIAMETER`

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::PcbLayerId;
use kicad_pcb::drc_api::DRCE_VIA_DIAMETER;
use kicad_pcb::padstack::PadstackMode;
use kicad_pcb::pcb_track::{PcbVia, Track};
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rule::DrcConstraintExt;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_VIA_DIAMETER`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderViaDiameter {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderViaDiameter {
    fn default() -> Self {
        DrcTestProviderViaDiameter {
            base: DrcTestProviderBase::new("diameter"),
        }
    }
}

/// The `layerDesc` lambdas of the via diameter and annular width providers: the board name of
/// `F_Cu` / `B_Cu`, "Inner Layers" for the inner key of a front / inner / back padstack, else
/// the board layer name.
pub(crate) fn padstack_layer_desc(
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    mode: PadstackMode,
) -> String {
    if layer == PcbLayerId::FCu {
        ctx.board.layer_name(PcbLayerId::FCu)
    } else if layer == PcbLayerId::BCu {
        ctx.board.layer_name(PcbLayerId::BCu)
    } else if mode == PadstackMode::FrontInnerBack {
        "Inner Layers".to_string()
    } else {
        ctx.board.layer_name(layer)
    }
}

impl DrcTestProviderViaDiameter {
    /// Port of `DRC_TEST_PROVIDER_VIA_DIAMETER::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_VIA_DIAMETER) {
            report_aux(run, "Via diameter violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        if !engine.has_rules_for_constraint_type(DrcConstraintT::ViaDiameterConstraint) {
            report_aux(run, "No via diameter constraints found. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking via diameters...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let progress_delta = 500;
        let tracks_size = board.tracks().count();

        for (ii, (_, track)) in board.tracks().enumerate() {
            if !self
                .base
                .report_progress(&mut host, ii, tracks_size, progress_delta)
            {
                break;
            }

            if !self.check_via_diameter(engine, &mut host, ctx, track) {
                break;
            }
        }

        !host.is_cancelled()
    }

    /// The `checkViaDiameter` lambda of `Run`: false when the error limit is reached.
    fn check_via_diameter(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        track: &Track,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_VIA_DIAMETER) {
            return false;
        }

        let Track::Via(via) = track else {
            return true;
        };

        let item = ctx.view(AnyItem::Track(track));

        via.padstack().for_each_unique_layer(|layer| {
            if via.is_ghost_layer(ctx, layer) {
                return;
            }

            self.check_layer(engine, host, ctx, item, via, layer);
        });

        true
    }

    /// The body of the `ForEachUniqueLayer` lambda of `checkViaDiameter`.
    fn check_layer(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        item: kicad_pcb::ItemRef<'_>,
        via: &PcbVia,
        layer: PcbLayerId,
    ) {
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::ViaDiameterConstraint,
            Some(item),
            None,
            layer,
        );
        let mut fail_min = false;
        let mut fail_max = false;
        let mut constraint_diameter = 0;
        let actual = via.width(layer);
        let mode = via.padstack().mode();

        if constraint.severity() != Severity::Ignore {
            if constraint.value().has_min() && actual < constraint.value().min() {
                fail_min = true;
                constraint_diameter = constraint.value().min();
            }

            if constraint.value().has_max() && actual > constraint.value().max() {
                fail_max = true;
                constraint_diameter = constraint.value().max();
            }
        }

        if fail_min || fail_max {
            let mut drc_item = item::create(DRCE_VIA_DIAMETER).expect("DRCE_VIA_DIAMETER item");
            let mut constraint_name = constraint.name();
            let mut msg = String::new();

            if fail_min {
                if constraint.implicit_min {
                    constraint_name = "board setup constraints".to_string();
                }

                if mode == PadstackMode::Normal {
                    msg = self.base.format_msg(
                        "(%s min diameter %s; actual %s)",
                        &constraint_name,
                        constraint_diameter as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    );
                } else {
                    msg = self.base.format_msg_extra(
                        "(%s min diameter %s; actual %s on %s)",
                        &constraint_name,
                        constraint_diameter as f64,
                        actual as f64,
                        &padstack_layer_desc(ctx, layer, mode),
                        EdaDataType::Distance,
                    );
                }
            } else if fail_max {
                if mode == PadstackMode::Normal {
                    msg = self.base.format_msg(
                        "(%s max diameter %s; actual %s)",
                        &constraint_name,
                        constraint_diameter as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    );
                } else {
                    msg = self.base.format_msg_extra(
                        "(%s max diameter %s; actual %s on %s)",
                        &constraint_name,
                        constraint_diameter as f64,
                        actual as f64,
                        &padstack_layer_desc(ctx, layer, mode),
                        EdaDataType::Distance,
                    );
                }
            }

            drc_item.set_error_detail(&msg);
            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            self.base.report_violation(
                host,
                Arc::new(drc_item),
                via.position(ctx),
                layer.0,
                &crate::test_provider::no_path,
            );
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderViaDiameter {
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
