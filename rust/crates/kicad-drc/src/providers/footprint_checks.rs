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

//! Port of `pcbnew/drc/drc_test_provider_footprint_checks.cpp`.
//!
//! Footprint tests:
//!
//! - `DRCE_FOOTPRINT_TYPE_MISMATCH`,
//! - `DRCE_OVERLAPPING_PADS`,
//! - `DRCE_PAD_TH_WITH_NO_HOLE`,
//! - `DRCE_PADSTACK`,
//! - `DRCE_PADSTACK_INVALID`,
//! - `DRCE_FOOTPRINT` (unknown or duplicate pads in net-tie pad groups),
//! - `DRCE_SHORTING_ITEMS`,
//! - `DRCE_FOOTPRINT_SCALED_WITH_PADS`

use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_core::kiid::Kiid;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{
    DRCE_FOOTPRINT, DRCE_FOOTPRINT_SCALED_WITH_PADS, DRCE_FOOTPRINT_TYPE_MISMATCH,
    DRCE_PAD_TH_WITH_NO_HOLE, DRCE_PADSTACK, DRCE_SHORTING_ITEMS,
};
use kicad_pcb::ids::ItemId;
use kicad_pcb::BoardCtx;

use super::run_host::RunHost;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::footprint_utils::{
    check_footprint_attributes, check_net_tie_pad_groups, check_net_ties, check_pads,
};
use crate::item::{self, convert_pad_check_to_drc_code};
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_FOOTPRINT_CHECKS`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderFootprintChecks {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderFootprintChecks {
    /// `DRC_TEST_PROVIDER_FOOTPRINT_CHECKS()`: not rule driven.
    fn default() -> Self {
        let mut base = DrcTestProviderBase::new("footprint checks");
        base.is_rule_driven = false;

        DrcTestProviderFootprintChecks { base }
    }
}

impl DrcTestProviderFootprintChecks {
    /// The `errorHandler` lambda of `Run`: an item of `error_code` with the detail `msg` (when
    /// not empty) naming the items present.
    #[allow(clippy::too_many_arguments)]
    fn error_handler(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        items: [Option<ItemId>; 3],
        error_code: i32,
        msg: &str,
        pt: Vector2I,
        layer: i32,
    ) {
        let mut drc_item = item::create(error_code).expect("footprint checks DRC item");

        if !msg.is_empty() {
            drc_item.set_error_detail(msg);
        }

        let ids: Vec<Kiid> = items
            .iter()
            .flatten()
            .filter_map(|&id| ctx.board.uuid_of(id))
            .collect();
        drc_item.set_item_ids(ids);

        self.base
            .report_violation(host, Arc::new(drc_item), pt, layer, &no_path);
    }

    /// Port of `DRC_TEST_PROVIDER_FOOTPRINT_CHECKS::Run`: false when the run was cancelled.
    pub fn run(&self, _engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking footprints...")
        {
            return false; // DRC cancelled
        }

        // DRC_ENGINE as the UNITS_PROVIDER of CheckPads: the units of the run.
        let units = self.base.units_provider();

        for (fp_id, footprint) in ctx.board.footprints() {
            let fp_item: ItemId = fp_id.into();
            let fp_pos = footprint.position(ctx);
            let fp_layer = footprint.layer(ctx).0;

            if !host.is_error_limit_exceeded(DRCE_FOOTPRINT_TYPE_MISMATCH) {
                let mut messages: Vec<String> = Vec::new();
                check_footprint_attributes(ctx, fp_id, &mut |msg: &str| {
                    messages.push(msg.to_string())
                });

                for msg in messages {
                    self.error_handler(
                        &mut host,
                        ctx,
                        [Some(fp_item), None, None],
                        DRCE_FOOTPRINT_TYPE_MISMATCH,
                        &msg,
                        fp_pos,
                        fp_layer,
                    );
                }
            }

            if !host.is_error_limit_exceeded(DRCE_FOOTPRINT_SCALED_WITH_PADS) {
                let xform = footprint.transform();

                if !xform.has_unit_scale() && ctx.board.footprint_pads(fp_id).next().is_some() {
                    self.error_handler(
                        &mut host,
                        ctx,
                        [Some(fp_item), None, None],
                        DRCE_FOOTPRINT_SCALED_WITH_PADS,
                        "",
                        fp_pos,
                        fp_layer,
                    );
                }
            }

            if !host.is_error_limit_exceeded(DRCE_PAD_TH_WITH_NO_HOLE)
                || !host.is_error_limit_exceeded(DRCE_PADSTACK)
            {
                let mut found: Vec<(ItemId, i32, String, Vector2I, i32)> = Vec::new();

                check_pads(ctx, fp_id, &units, &mut |pad_id, pad, result| {
                    let detail = if !result.message.is_empty() {
                        format!("({})", result.message)
                    } else {
                        String::new()
                    };

                    found.push((
                        pad_id.into(),
                        convert_pad_check_to_drc_code(result.code),
                        detail,
                        pad.position(ctx),
                        pad.principal_layer(ctx).0,
                    ));
                });

                for (pad_item, drc_pad_error_code, detail, pos, layer) in found {
                    if !host.is_error_limit_exceeded(drc_pad_error_code) {
                        self.error_handler(
                            &mut host,
                            ctx,
                            [Some(pad_item), None, None],
                            drc_pad_error_code,
                            &detail,
                            pos,
                            layer,
                        );
                    }
                }
            }

            // Don't call FOOTPRINT_UTILS::CheckShortingPads().  At the board level we know
            // about nets, and the pads may have the same net even though they're distinct pads.

            if footprint.is_net_tie() {
                if !host.is_error_limit_exceeded(DRCE_SHORTING_ITEMS) {
                    let mut found: Vec<([Option<ItemId>; 3], Vector2I)> = Vec::new();

                    check_net_ties(ctx, fp_id, &mut |a, b, c, position| {
                        found.push(([a, b, c], position))
                    });

                    for (items, position) in found {
                        self.error_handler(
                            &mut host,
                            ctx,
                            items,
                            DRCE_SHORTING_ITEMS,
                            "",
                            position,
                            fp_layer,
                        );
                    }
                }

                let mut messages: Vec<String> = Vec::new();
                check_net_tie_pad_groups(ctx, fp_id, &mut |msg: &str| {
                    messages.push(msg.to_string())
                });

                for msg in messages {
                    self.error_handler(
                        &mut host,
                        ctx,
                        [Some(fp_item), None, None],
                        DRCE_FOOTPRINT,
                        &msg,
                        fp_pos,
                        fp_layer,
                    );
                }
            }

        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderFootprintChecks {
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
