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

//! Port of `pcbnew/drc/drc_test_provider_text_mirroring.cpp`.
//!
//! Text mirroring tests.
//! Errors generated:
//! - `DRCE_MIRRORED_TEXT_ON_FRONT_LAYER`
//! - `DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER`

use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_pcb::drc_api::{DRCE_MIRRORED_TEXT_ON_FRONT_LAYER, DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER};
use kicad_pcb::{BoardCtx, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, for_each_geometry_item, no_path};

/// Port of `DRC_TEST_PROVIDER_TEXT_MIRRORING`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderTextMirroring {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderTextMirroring {
    fn default() -> Self {
        DrcTestProviderTextMirroring {
            base: DrcTestProviderBase::new("text_mirroring"),
        }
    }
}

/// The `itemTypes` of `Run` (shared with the text dimensions provider).
pub(crate) const TEXT_ITEM_TYPES: [KicadT; 5] = [
    KicadT::PcbFieldT,
    KicadT::PcbTextT,
    KicadT::PcbTextboxT,
    KicadT::PcbTablecellT,
    KicadT::PcbDimensionT,
];

impl DrcTestProviderTextMirroring {
    /// Port of `DRC_TEST_PROVIDER_TEXT_MIRRORING::Run`: false when the run was cancelled.
    pub fn run(&self, _engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_MIRRORED_TEXT_ON_FRONT_LAYER)
            && run.is_error_limit_exceeded(DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER)
        {
            report_aux(run, "Text mirroring violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking text mirroring...")
        {
            return false; // DRC cancelled
        }

        let top_layers = Lset::from_layers(&[
            PcbLayerId::FCu,
            PcbLayerId::FSilkS,
            PcbLayerId::FMask,
            PcbLayerId::FFab,
        ]);
        let bottom_layers = Lset::from_layers(&[
            PcbLayerId::BCu,
            PcbLayerId::BSilkS,
            PcbLayerId::BMask,
            PcbLayerId::BFab,
        ]);
        let both_layers = &top_layers | &bottom_layers;

        let progress_delta = 500;
        let mut count = 0;
        let mut progress_index = 0;

        for_each_geometry_item(ctx, &TEXT_ITEM_TYPES, &both_layers, |_item| {
            count += 1;
            true
        });

        for_each_geometry_item(ctx, &TEXT_ITEM_TYPES, &both_layers, |item| {
            let index = progress_index;
            progress_index += 1;

            if !self
                .base
                .report_progress(&mut host, index, count, progress_delta)
            {
                return false;
            }

            if let Some(text) = item.as_eda_text() {
                let layer = item.layer();

                if !text.eda_text().is_visible()
                    || !ctx.board.is_layer_enabled(layer)
                    || !ctx.board.is_layer_visible(layer)
                {
                    return true;
                }

                let mirrored = text.eda_text().is_mirrored();

                self.check_text_mirroring(
                    &mut host,
                    item,
                    mirrored,
                    true,
                    DRCE_MIRRORED_TEXT_ON_FRONT_LAYER,
                    (&top_layers, &bottom_layers),
                );
                self.check_text_mirroring(
                    &mut host,
                    item,
                    mirrored,
                    false,
                    DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER,
                    (&top_layers, &bottom_layers),
                );
            }

            true
        });

        !host.is_cancelled()
    }

    /// The `checkTextMirroring` lambda of `Run` (`text_is_mirrored` is
    /// `text->IsMirrored()`).
    fn check_text_mirroring(
        &self,
        host: &mut RunHost<'_, '_>,
        item: ItemRef<'_>,
        text_is_mirrored: bool,
        is_mirrored: bool,
        error_code: i32,
        (top_layers, bottom_layers): (&Lset, &Lset),
    ) {
        if host.is_error_limit_exceeded(error_code) {
            return;
        }

        let layer = item.layer();
        let layer_match = (is_mirrored && top_layers.contains(layer))
            || (!is_mirrored && bottom_layers.contains(layer));

        if layer_match && text_is_mirrored == is_mirrored {
            let mut drc_item = item::create(error_code).expect("text mirroring item");
            drc_item.set_item_ids(vec![item.uuid()]);

            self.base.report_violation(
                host,
                Arc::new(drc_item),
                item.position(),
                layer.0,
                &no_path,
            );
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderTextMirroring {
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
