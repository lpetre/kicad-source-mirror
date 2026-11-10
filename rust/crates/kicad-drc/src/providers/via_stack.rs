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

//! Port of `pcbnew/drc/drc_test_provider_via_stack.cpp`.
//!
//! Microvia stack topology tests. Errors generated:
//! - `DRCE_MALFORMED_MICROVIA_STACK_SPAN`
//! - `DRCE_MICROVIA_STACK_NOT_FILLED`
//! - `DRCE_MICROVIA_STACK_DEPTH`
//! - `DRCE_HOLE_ASPECT_RATIO`
//! - `DRCE_MICROVIA_CROSSES_CORE`
//!
//! The span test asks whether a generator's hops still tile the span it declares. Nothing
//! rebuilds a stack on load, so a file can carry one that does not. The others ask whether
//! the copper can be built. The aspect ratio test applies to every plated hole, the rest
//! to every microvia however it was placed.
//!
//! Where these rules come from:
//!
//! IPC-2226 defines a microvia as a hole of aspect ratio 1:1 or better, no deeper than
//! 0.25 mm. That is why a hop spans one dielectric and why deeper structures are built by
//! stacking.
//!
//! IPC-4761 names the via protection types. A filled via is Type V and a filled and capped via
//! is Type VII, which is the via in pad treatment used for BGA fanout.
//!
//! IPC-6012 carries the reliability requirements for microvias, including the thermal cycling
//! a stacked structure has to survive. Unfilled stacked microvias separate at the target pad.
//!
//! Maximum stack depth is deliberately not standardised. It is a fabricator capability, so it
//! is a board setting rather than a fixed number here.
//!
//! The C++ runs sequentially (no thread pool): the report order is deterministic and the port
//! reproduces it (generators in board order, microvia columns in
//! `PCB_VIA::CollectMicroviaColumns` order, tracks, then footprints x pads).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, is_external_copper_layer};
use kicad_core::layer_range::LayerRange;
use kicad_core::{Lset, PcbLayerId, Severity};
use kicad_pcb::board::Board;
use kicad_pcb::board_stackup::{BoardStackupItem, BoardStackupItemType, KEY_CORE};
use kicad_pcb::drc_api::{
    DRCE_HOLE_ASPECT_RATIO, DRCE_MALFORMED_MICROVIA_STACK_SPAN, DRCE_MICROVIA_CROSSES_CORE,
    DRCE_MICROVIA_STACK_DEPTH, DRCE_MICROVIA_STACK_NOT_FILLED,
};
use kicad_pcb::ids::TrackId;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_generator::{GENERATOR_TYPE_VIA_STACK, PcbGenerator};
use kicad_pcb::pcb_track::{PcbVia, Track, Viatype};
use kicad_pcb::{AnyItem, BoardCtx};

use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::providers::run_host::RunHost;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_VIA_STACK`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderViaStack {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderViaStack {
    fn default() -> Self {
        DrcTestProviderViaStack {
            base: DrcTestProviderBase::new("microvia_stack"),
        }
    }
}

/// Port of the static `PCB_VIA_STACK::IsSpanValid( aBoard, aStart, aEnd )`
/// (`pcbnew/generators/pcb_via_stack.cpp`; not in kicad-pcb yet): both ends are copper layers
/// enabled on the board and they differ.
pub fn is_span_valid(board: &Board, start: PcbLayerId, end: PcbLayerId) -> bool {
    let enabled = Lset::all_cu_mask_n(board.copper_layer_count());

    let present =
        |layer: PcbLayerId| layer.0 >= 0 && layer.0 < PCB_LAYER_ID_COUNT && enabled.contains(layer);

    present(start) && present(end) && start != end
}

/// `stackThickness( aStack, aFirst, aLast )`: copper and dielectric only, so soldermask does
/// not count.
fn stack_thickness(stack: &[BoardStackupItem], first: i32, last: i32) -> i32 {
    let mut thickness = 0;

    for i in first..last {
        let item = &stack[i as usize];

        if !item.is_enabled() {
            continue;
        }

        if item.type_() == BoardStackupItemType::Dielectric {
            for sub in 0..item.sublayers_count() {
                thickness += item.thickness(sub);
            }
        } else if item.type_() == BoardStackupItemType::Copper {
            thickness += item.thickness(0);
        }
    }

    thickness
}

/// The via behind a track handle (`static_cast<PCB_VIA*>`).
fn via_of(board: &Board, id: TrackId) -> Option<&PcbVia> {
    match board.track(id) {
        Some(Track::Via(via)) => Some(via),
        _ => None,
    }
}

impl DrcTestProviderViaStack {
    /// Port of `DRC_TEST_PROVIDER_VIA_STACK::checkSpan`.
    fn check_span(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        stack: &PcbGenerator,
        copper_layers: i32,
    ) {
        let Some(data) = stack.via_stack_data() else {
            return;
        };

        // GetBoardItems() is an unordered set in C++; the outcome below does not depend on
        // the order.
        let mut vias: Vec<&PcbVia> = Vec::new();

        for item in stack.group.board_items(ctx) {
            if let Some(via) = item.as_via() {
                if via.via_type() == Viatype::Microvia {
                    vias.push(via);
                }
            }
        }

        if vias.is_empty() {
            return;
        }

        let stack_ref = ctx.view(AnyItem::Generator(stack));

        // IPC-2226 confines a microvia to one dielectric, so the hops must tile the declared
        // span with single layer steps and no gap. A span naming layers absent from the board
        // is broken by definition, and iterating it would never terminate.
        if !host.is_error_limit_exceeded(DRCE_MALFORMED_MICROVIA_STACK_SPAN) {
            if !is_span_valid(ctx.board, data.start_layer, data.end_layer) {
                let mut drc_item =
                    item::create(DRCE_MALFORMED_MICROVIA_STACK_SPAN).expect("span item");
                drc_item.set_item_ids(vec![stack_ref.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    stack.position(ctx),
                    stack.layer(ctx).0,
                    &no_path,
                );
                return;
            }

            let mut layer_to_index: BTreeMap<i32, i32> = BTreeMap::new();
            let mut span_len = 0;

            // IsSpanValid() made both ends copper layers, so the range cannot fail.
            if let Ok(range) = LayerRange::new(data.start_layer, data.end_layer, copper_layers) {
                for layer in range.iter() {
                    layer_to_index.insert(layer.0, span_len);
                    span_len += 1;
                }
            }

            let mut covered: BTreeSet<i32> = BTreeSet::new();
            let mut broken = false;

            for via in &vias {
                let top = layer_to_index.get(&via.top_layer().0);
                let bot = layer_to_index.get(&via.bottom_layer().0);

                let (Some(&top), Some(&bot)) = (top, bot) else {
                    broken = true;
                    break;
                };

                if (top - bot).abs() != 1 {
                    broken = true;
                    break;
                }

                if !covered.insert(top.min(bot)) {
                    broken = true;
                    break;
                }
            }

            if broken || covered.len() as i32 != span_len - 1 {
                let mut drc_item =
                    item::create(DRCE_MALFORMED_MICROVIA_STACK_SPAN).expect("span item");
                drc_item.set_item_ids(vec![stack_ref.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    stack.position(ctx),
                    stack.layer(ctx).0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_VIA_STACK::checkColumn`. Depth and fill are properties of
    /// the copper, not of the object that placed it.
    fn check_column(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        column: &[TrackId],
    ) {
        let board = ctx.board;

        // Not standardised. A fabricator quotes its own limit, and a designer may accept a
        // deeper stack in one area than another, so the limit is resolved per via rather than
        // read once.
        let Some(&top_id) = column.first() else {
            return;
        };
        let (Some(top_track), Some(top_via)) = (board.track(top_id), via_of(board, top_id)) else {
            return;
        };
        let top = ctx.view(AnyItem::Track(top_track));
        let depth_constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::MicroviaStackDepthConstraint,
            Some(top),
            None,
            top.layer(),
        );

        if depth_constraint.severity() != Severity::Ignore
            && depth_constraint.value().has_max()
            && column.len() as i32 > depth_constraint.value().max()
            && !host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_DEPTH)
        {
            let mut drc_item = item::create(DRCE_MICROVIA_STACK_DEPTH).expect("depth item");
            drc_item.set_error_message(&format!(
                "Stack is {} hops deep, limit is {}",
                column.len() as i32,
                depth_constraint.value().max()
            ));
            drc_item.set_item_ids(vec![top.uuid()]);
            drc_item.violating_rule = depth_constraint.parent_rule.clone();
            let pos = top_via.position(ctx);
            self.base
                .report_violation(host, Arc::new(drc_item), pos, top.layer().0, &no_path);
        }

        if host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_NOT_FILLED) {
            return;
        }

        // IPC-6012 requires the hops of a stacked microvia to be filled. An unfilled hop is a
        // dimple, and the joint separates under thermal cycling.
        // An unset fill takes the board default.
        let board_fills_vias = board.design_settings().fill_vias;

        for &id in column {
            let Some(via) = via_of(board, id) else {
                continue;
            };

            if !via.padstack().drill().is_filled.unwrap_or(board_fills_vias) {
                let track = board.track(id).expect("column via");
                let via_ref = ctx.view(AnyItem::Track(track));
                let mut drc_item =
                    item::create(DRCE_MICROVIA_STACK_NOT_FILLED).expect("not filled item");
                drc_item.set_item_ids(vec![via_ref.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    via.position(ctx),
                    via_ref.layer().0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_VIA_STACK::checkLooseMicrovias`.
    fn check_loose_microvias(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        let columns = PcbVia::collect_microvia_columns(ctx);

        let progress_delta = 250;

        for (ii, column) in columns.iter().enumerate() {
            if !self
                .base
                .report_progress(host, ii, columns.len(), progress_delta)
            {
                return;
            }

            self.check_column(engine, host, ctx, column);
        }
    }

    /// Port of `DRC_TEST_PROVIDER_VIA_STACK::checkAspectRatio`.
    fn check_aspect_ratio(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        let board = ctx.board;
        let mut check_ratio = !host.is_error_limit_exceeded(DRCE_HOLE_ASPECT_RATIO)
            && engine.has_rules_for_constraint_type(DrcConstraintT::HoleAspectRatioConstraint);
        let check_core = !host.is_error_limit_exceeded(DRCE_MICROVIA_CROSSES_CORE);

        if !check_ratio && !check_core {
            return;
        }

        let stackup = board.get_stackup_or_default();
        let stack = stackup.list();

        let progress_delta = 500;
        let tracks_size = board.tracks().count();

        for (ii, (_, track)) in board.tracks().enumerate() {
            if !self
                .base
                .report_progress(host, ii, tracks_size, progress_delta)
            {
                return;
            }

            let Track::Via(via) = track else {
                continue;
            };

            if via.drill_value(ctx) <= 0 {
                continue;
            }

            let mut first: i32 = -1;
            let mut last: i32 = -1;

            for (i, layer_item) in stack.iter().enumerate() {
                if layer_item.type_() != BoardStackupItemType::Copper {
                    continue;
                }

                let layer = layer_item.brd_layer_id();

                if layer == via.top_layer() || layer == via.bottom_layer() {
                    if first < 0 {
                        first = i as i32;
                    } else {
                        last = i as i32;
                    }
                }
            }

            if first < 0 || last < 0 {
                continue;
            }

            let via_ref = ctx.view(AnyItem::Track(track));

            // IPC-2226 builds microvias in the layers laminated onto the core, not through it.
            // Laser drilling relies on the resin ablating, and core is glass reinforced.
            if via.via_type() == Viatype::Microvia && check_core {
                for i in (first + 1)..last {
                    let layer_item = &stack[i as usize];

                    if layer_item.is_enabled()
                        && layer_item.type_() == BoardStackupItemType::Dielectric
                        && layer_item.type_name() == KEY_CORE
                    {
                        let mut drc_item =
                            item::create(DRCE_MICROVIA_CROSSES_CORE).expect("core item");

                        drc_item.set_error_message(&format!(
                            "The stackup gives the dielectric between {} and {} as core",
                            Lset::name(via.top_layer()),
                            Lset::name(via.bottom_layer())
                        ));
                        drc_item.set_item_ids(vec![via_ref.uuid()]);
                        self.base.report_violation(
                            host,
                            Arc::new(drc_item),
                            via.position(ctx),
                            via_ref.layer().0,
                            &no_path,
                        );
                        break;
                    }
                }
            }

            if !check_ratio {
                continue;
            }

            let ratio_constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::HoleAspectRatioConstraint,
                Some(via_ref),
                None,
                PcbLayerId::UndefinedLayer, // holes are not layer-specific
            );

            if ratio_constraint.severity() == Severity::Ignore
                || !ratio_constraint.value().has_max()
            {
                continue;
            }

            // Ratios are parsed to three decimals, so the stored maximum is scaled by 1000.
            let limit = f64::from(ratio_constraint.value().max()) / 1000.0;

            if limit <= 0.0 {
                continue;
            }

            let top_external = is_external_copper_layer(stack[first as usize].brd_layer_id());
            let bot_external = is_external_copper_layer(stack[last as usize].brd_layer_id());
            let from_back = bot_external && !top_external;

            let mut thickness = stack_thickness(stack, first + 1, last)
                + stack[if from_back { last } else { first } as usize].thickness(0);

            // The laser stops on the foil it lands on, and so does a blind drill. Any other
            // drill leaves through the far foil.
            if via.via_type() != Viatype::Microvia && top_external == bot_external {
                thickness += stack[if from_back { first } else { last } as usize].thickness(0);
            }

            let ratio = f64::from(thickness) / f64::from(via.drill_value(ctx));

            if ratio <= limit {
                continue;
            }

            if host.is_error_limit_exceeded(DRCE_HOLE_ASPECT_RATIO) {
                if !check_core {
                    return;
                }

                check_ratio = false;
                continue;
            }

            let mut drc_item = item::create(DRCE_HOLE_ASPECT_RATIO).expect("aspect item");

            drc_item.set_error_message(&format!(
                "Aspect ratio {ratio:.2} exceeds the {limit:.2} limit"
            ));

            drc_item.set_item_ids(vec![via_ref.uuid()]);
            drc_item.violating_rule = ratio_constraint.parent_rule.clone();
            self.base.report_violation(
                host,
                Arc::new(drc_item),
                via.position(ctx),
                via_ref.layer().0,
                &no_path,
            );
        }

        if !check_ratio || host.is_cancelled() {
            return;
        }

        let board_depth = stack_thickness(stack, 0, stack.len() as i32);

        let footprints_size = board.footprints().count();

        for (ii, (fp_id, _)) in board.footprints().enumerate() {
            if !self
                .base
                .report_progress(host, ii, footprints_size, progress_delta)
            {
                return;
            }

            for (_, pad) in board.footprint_pads(fp_id) {
                if host.is_error_limit_exceeded(DRCE_HOLE_ASPECT_RATIO) {
                    return;
                }

                if pad.attribute() != PadAttrib::Pth {
                    continue;
                }

                // The chemistry sees the narrow dimension of a slot.
                let drill_size = pad.drill_size(ctx);
                let drill = drill_size.x.min(drill_size.y);

                if drill <= 0 {
                    continue;
                }

                let pad_ref = ctx.view(AnyItem::Pad(pad));
                let ratio_constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::HoleAspectRatioConstraint,
                    Some(pad_ref),
                    None,
                    PcbLayerId::UndefinedLayer, // holes are not layer-specific
                );

                if ratio_constraint.severity() == Severity::Ignore
                    || !ratio_constraint.value().has_max()
                {
                    continue;
                }

                let limit = f64::from(ratio_constraint.value().max()) / 1000.0;

                if limit <= 0.0 {
                    continue;
                }

                let ratio = f64::from(board_depth) / f64::from(drill);

                if ratio <= limit {
                    continue;
                }

                let mut drc_item = item::create(DRCE_HOLE_ASPECT_RATIO).expect("aspect item");

                drc_item.set_error_message(&format!(
                    "Aspect ratio {ratio:.2} exceeds the {limit:.2} limit"
                ));

                drc_item.set_item_ids(vec![pad_ref.uuid()]);
                drc_item.violating_rule = ratio_constraint.parent_rule.clone();
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    pad.position(ctx),
                    pad.layer(ctx).0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_VIA_STACK::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);

        if host.is_error_limit_exceeded(DRCE_MALFORMED_MICROVIA_STACK_SPAN)
            && host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_NOT_FILLED)
            && host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_DEPTH)
            && host.is_error_limit_exceeded(DRCE_HOLE_ASPECT_RATIO)
            && host.is_error_limit_exceeded(DRCE_MICROVIA_CROSSES_CORE)
        {
            return true;
        }

        if !self.base.report_phase(
            &mut host,
            "Checking hole aspect ratios and microvia stacks...",
        ) {
            return false;
        }

        let board = ctx.board;
        let copper_layers = board.copper_layer_count();

        if !host.is_error_limit_exceeded(DRCE_MALFORMED_MICROVIA_STACK_SPAN) {
            for (_, generator) in board.generators() {
                if host.is_cancelled() {
                    break;
                }

                // dynamic_cast<PCB_VIA_STACK*>
                if generator.generator_type() == GENERATOR_TYPE_VIA_STACK {
                    self.check_span(&mut host, ctx, generator, copper_layers);
                }
            }
        }

        if !host.is_cancelled()
            && !(host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_NOT_FILLED)
                && host.is_error_limit_exceeded(DRCE_MICROVIA_STACK_DEPTH))
        {
            self.check_loose_microvias(engine, &mut host, ctx);
        }

        if !host.is_cancelled()
            && !(host.is_error_limit_exceeded(DRCE_HOLE_ASPECT_RATIO)
                && host.is_error_limit_exceeded(DRCE_MICROVIA_CROSSES_CORE))
        {
            self.check_aspect_ratio(engine, &mut host, ctx);
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderViaStack {
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
