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

//! Port of `pcbnew/drc/drc_test_provider_via_stitch.cpp` (package DRC-34).
//!
//! Same-net via-stitch zone overlap test. Errors generated:
//! - `DRCE_VIA_STITCH_OVERLAP`
//!
//! The stitches are the `PCB_VIA_STITCH` generators of the board (`generator_type ==
//! "via_stitch"`) in `BOARD::Generators()` order; their outline and net come from the typed
//! members of the generator ([`kicad_pcb::pcb_generator::ViaStitchData`]).

use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_geom::Shape;
use kicad_pcb::drc_api::DRCE_VIA_STITCH_OVERLAP;
use kicad_pcb::pcb_generator::{PcbGenerator, ViaStitchData};
use kicad_pcb::{Board, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_VIA_STITCH`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderViaStitch {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderViaStitch {
    fn default() -> Self {
        DrcTestProviderViaStitch {
            base: DrcTestProviderBase::new("via_stitch_overlap"),
        }
    }
}

/// Port of `PCB_VIA_STITCH::GetNetCode()` after a board load: `m_netCode` is 0 (only
/// `SetNetCode` and the editing paths set it), so the code of the board net named
/// `m_lastNetName` (`BOARD::FindNet( name )`), 0 without one.
pub fn via_stitch_net_code(board: &Board, data: &ViaStitchData) -> i32 {
    if data.last_net_name.is_empty() {
        return 0;
    }

    board
        .find_net_by_name(&data.last_net_name)
        .map_or(0, |net| net.net_code().0)
}

impl DrcTestProviderViaStitch {
    /// Port of `DRC_TEST_PROVIDER_VIA_STITCH::Run`: false when the run was cancelled.
    pub fn run(&self, _engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_VIA_STITCH_OVERLAP) {
            report_aux(run, "Via-stitch overlap tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking via-stitch zone overlaps...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;

        // Collect the stitch generators present on the board.
        let stitches: Vec<(&PcbGenerator, ViaStitchData)> = board
            .generators()
            .filter_map(|(_, gen_)| gen_.via_stitch_data().map(|data| (gen_, data)))
            .collect();

        for i in 0..stitches.len() {
            if host.is_error_limit_exceeded(DRCE_VIA_STITCH_OVERLAP) {
                break;
            }

            let (a, a_data) = &stitches[i];
            let a_net = via_stitch_net_code(board, a_data);

            if a_net == 0 {
                continue;
            }

            for (b, b_data) in &stitches[i + 1..] {
                if via_stitch_net_code(board, b_data) != a_net {
                    continue;
                }

                let mut intersection = a_data.outline_set();
                intersection.boolean_intersection(&b_data.outline_set());

                if intersection.outline_count() == 0 {
                    continue;
                }

                let mut drc_item =
                    item::create(DRCE_VIA_STITCH_OVERLAP).expect("DRCE_VIA_STITCH_OVERLAP item");
                drc_item.set_item_ids(vec![a.group.base.uuid, b.group.base.uuid]);

                // Report at the center of intersection
                let pos = intersection.bbox(0).centre();

                self.base.report_violation(
                    &mut host,
                    Arc::new(drc_item),
                    pos,
                    a.layer(ctx).0,
                    &no_path,
                );
            }
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderViaStitch {
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
