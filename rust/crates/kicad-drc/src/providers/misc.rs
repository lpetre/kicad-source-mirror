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

//! Port of `pcbnew/drc/drc_test_provider_misc.cpp` (and, as private helpers, of
//! `ExpandEnvVarSubstitutions` / `KIwxExpandEnvVars` of `common/common.cpp` and the
//! `ENV_VAR` helpers of `common/env_vars.cpp` that it calls).
//!
//! Miscellaneous tests:
//!
//! - `DRCE_DISABLED_LAYER_ITEM`, item on a disabled layer
//! - `DRCE_INVALID_OUTLINE`, invalid board outline
//! - `DRCE_UNRESOLVED_VARIABLE`
//! - `DRCE_ASSERTION_FAILURE`, user-defined assertions
//! - `DRCE_GENERIC_WARNING`, user-defined warnings, net classes that rules cannot name
//! - `DRCE_GENERIC_ERROR`, user-defined errors
//! - `DRCE_MISSING_TUNING_PROFILES`, tuning profile for net class not defined
//!
//! The C++ checks the assertions on the thread pool; the port checks the items sequentially in
//! the `forEachGeometryItem` order (`rust/docs/analysis/drc.md` section 4.12).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::kiid::Kiid;
use kicad_core::layer_ids::{LAYER_DRAWINGSHEET, PcbLayerId, UNDEFINED_LAYER, layer_name};
use kicad_core::lset::Lset;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::typeinfo::KicadT;
use kicad_geom::math::vector2::Vector2I;
use kicad_libeval::libeval_compiler::escape_string_literal;
use kicad_pcb::convert_shape_list_to_polygon::{
    build_board_polygon_outlines, test_board_outlines_graphic_items,
};
use kicad_pcb::drc_api::{
    DRCE_ASSERTION_FAILURE, DRCE_DISABLED_LAYER_ITEM, DRCE_GENERIC_ERROR, DRCE_GENERIC_WARNING,
    DRCE_INVALID_OUTLINE, DRCE_MISSING_TUNING_PROFILE, DRCE_UNRESOLVED_VARIABLE, DrcConstraint,
};
use kicad_pcb::ids::{ItemId, ShapeId};
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{BoardCtx, ItemRef};

use super::run_host::RunHost;
use crate::constraint_types::DrcConstraintT;
use crate::engine::implicit_rules::TuningProfiles;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, all_basic_items, for_each_geometry_item, no_path};

/// `pcbIUScale.mmToIU( 0.001 )`: the minimum size of a valid Edge.Cuts graphic item.
const MIN_SIZE_FOR_VALID_GRAPHICS: i32 = 1000;

/// `pcbIUScale.mmToIU( 0.05 )`: the arc approximation error of the (unused) outline.
const OUTLINE_MAX_ERROR: i32 = 50_000;

/// One text of the drawing sheet as `testTextVars` reads it from `DS_DRAW_ITEM_LIST` (built
/// with `FOR_ERC_DRC`, page "1" of 1, file name "dummyFilename", sheet "dummySheet", layer
/// "dummyLayer" and the board's project).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingSheetText {
    /// `DS_DRAW_ITEM_TEXT::GetText()`.
    pub text: String,
    /// `DS_DRAW_ITEM_TEXT::GetShownText( FOR_ERC_DRC )`.
    pub shown_text: String,
    /// `DS_DRAW_ITEM_TEXT::GetPosition()`.
    pub position: Vector2I,
}

/// The drawing sheet the engine was given (`DRC_ENGINE::GetDrawingSheet()`, a
/// `DS_PROXY_VIEW_ITEM`), reduced to what `testTextVars` uses. The drawing-sheet engine
/// (`DS_DRAW_ITEM_LIST::BuildDrawItemsList`) is not part of the port: the host supplies the
/// texts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingSheetTexts {
    /// `m_Uuid` of the `DS_PROXY_VIEW_ITEM` (the item of the violations).
    pub proxy_uuid: Kiid,
    /// The text items of the draw item list, in list order.
    pub texts: Vec<DrawingSheetText>,
}

/// Port of `DRC_TEST_PROVIDER_MISC`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderMisc {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_board->GetProject()->GetProjectFile().TuningProfileParameters()` (read only when the
    /// board has a project).
    pub tuning_profiles: TuningProfiles,
    /// `m_drcEngine->GetDrawingSheet()`: `None` (the default) skips the drawing-sheet part of
    /// `testTextVars`, as a run without a drawing sheet does in C++.
    pub drawing_sheet: Option<DrawingSheetTexts>,
    /// The process environment read by `ExpandEnvVarSubstitutions` (`wxGetEnv`). Library code
    /// has no environment: empty by default.
    pub environment: BTreeMap<String, String>,
}

impl Default for DrcTestProviderMisc {
    /// `DRC_TEST_PROVIDER_MISC()`: not rule driven.
    fn default() -> Self {
        let mut base = DrcTestProviderBase::new("miscellaneous");
        base.is_rule_driven = false;

        DrcTestProviderMisc {
            base,
            tuning_profiles: TuningProfiles::default(),
            drawing_sheet: None,
            environment: BTreeMap::new(),
        }
    }
}

impl DrcTestProviderMisc {
    /// A provider reading the project's tuning profiles.
    pub fn new(tuning_profiles: TuningProfiles) -> Self {
        DrcTestProviderMisc {
            tuning_profiles,
            ..Self::default()
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);

        // Ahead of the text variable checks, which share the generic warning limit
        if !host.is_error_limit_exceeded(DRCE_GENERIC_WARNING) {
            if !self
                .base
                .report_phase(&mut host, "Checking net class names...")
            {
                return false; // DRC cancelled
            }

            self.test_unquotable_netclasses(&mut host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_INVALID_OUTLINE) {
            if !self
                .base
                .report_phase(&mut host, "Checking board outline...")
            {
                return false; // DRC cancelled
            }

            self.test_outline(&mut host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_DISABLED_LAYER_ITEM) {
            if !self
                .base
                .report_phase(&mut host, "Checking disabled layers...")
            {
                return false; // DRC cancelled
            }

            self.test_disabled_layers(&mut host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_UNRESOLVED_VARIABLE) {
            if !self
                .base
                .report_phase(&mut host, "Checking text variables...")
            {
                return false; // DRC cancelled
            }

            self.test_text_vars(&mut host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_ASSERTION_FAILURE)
            || !host.is_error_limit_exceeded(DRCE_GENERIC_WARNING)
            || !host.is_error_limit_exceeded(DRCE_GENERIC_ERROR)
        {
            if !self.base.report_phase(&mut host, "Checking assertions...") {
                return false; // DRC cancelled
            }

            self.test_assertions(engine, &mut host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_MISSING_TUNING_PROFILE) {
            if !self
                .base
                .report_phase(&mut host, "Checking for missing tuning profiles...")
            {
                return false; // DRC cancelled
            }

            self.test_missing_tuning_profiles(&mut host, ctx);
        }

        !host.is_cancelled()
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testOutline`.
    fn test_outline(&self, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        let error_handled = Cell::new(false);
        let units = self.base.units_provider();
        let base = &self.base;
        let host = RefCell::new(host);

        let mut error_handler =
            |msg: &str, item_a: Option<ItemId>, item_b: Option<ItemId>, pt: Vector2I| {
                error_handled.set(true);

                if host.borrow().is_error_limit_exceeded(DRCE_INVALID_OUTLINE) {
                    return;
                }

                let (mut item_a, mut item_b) = (item_a, item_b);

                if item_a.is_none() {
                    std::mem::swap(&mut item_a, &mut item_b);
                }

                let mut marker_pos = pt;
                let mut gap = 0;
                let mut used_gap = false;
                let as_shape = |id: Option<ItemId>| id.and_then(|id| id.typed::<PcbShape>());

                let (shape_a, shape_b): (Option<ShapeId>, Option<ShapeId>) =
                    if as_shape(item_a).is_some() && as_shape(item_b).is_some() {
                        (as_shape(item_a), as_shape(item_b))
                    } else if item_a.is_some() && item_b.is_none() && as_shape(item_a).is_some() {
                        // A single item was flagged (e.g. a degenerate shape).  Keep the marker
                        // at that item so the user can locate the bad geometry.
                        (as_shape(item_a), None)
                    } else {
                        // No usable item pair identified.  Look for the most likely culprits.
                        let (a, b, midpoint, distance) = find_closest_outline_gap(ctx);
                        marker_pos = midpoint;
                        gap = distance;
                        item_a = a.map(ItemId::from);
                        item_b = b.map(ItemId::from);
                        used_gap = a.is_some() && b.is_some();
                        (a, b)
                    };

                if let (Some(sa), Some(sb)) = (shape_a, shape_b) {
                    let effective_shape_a = shape_effective_shape(ctx, sa);
                    let effective_shape_b = shape_effective_shape(ctx, sb);

                    if let (Some(esa), Some(esb)) = (effective_shape_a, effective_shape_b) {
                        let bbox_a = esa.bbox(0);
                        let bbox_b = esb.bbox(0);
                        let overlap = bbox_a.intersect(&bbox_b);

                        if overlap.width() > 0 && overlap.height() > 0 {
                            marker_pos = overlap.centre();
                            used_gap = false;
                        } else {
                            let mut pt_a = Vector2I::default();
                            let mut pt_b = Vector2I::default();

                            if esa.nearest_points(&*esb, &mut pt_a, &mut pt_b) {
                                gap = (pt_a - pt_b).euclidean_norm();
                                marker_pos = (pt_a + pt_b) / 2;
                                used_gap = true;
                            }
                        }
                    }
                }

                let mut drc_item =
                    item::create(DRCE_INVALID_OUTLINE).expect("DRCE_INVALID_OUTLINE item");

                if item_a.is_some() && item_b.is_some() && used_gap {
                    drc_item.set_error_detail(&format!(
                        "{} (gap {})",
                        msg,
                        units.message_text_from_value(gap as f64, true, EdaDataType::Distance)
                    ));
                } else {
                    drc_item.set_error_detail(msg);
                }

                drc_item.set_item_ids(item_uuids(ctx, &[item_a, item_b]));

                base.report_violation(
                    &mut **host.borrow_mut(),
                    Arc::new(drc_item),
                    marker_pos,
                    PcbLayerId::EdgeCuts.0,
                    &no_path,
                );
            };

        // Test for very small graphic items (a few nm size) that can create issues
        // when trying to build the board outlines, and they are not easy to locate on screen.
        if !test_board_outlines_graphic_items(
            ctx,
            MIN_SIZE_FOR_VALID_GRAPHICS,
            Some(&mut error_handler),
        ) {
            if error_handled.get() {
                // if there are invalid items on Edge.Cuts, they are already reported
            } else {
                let mut drc_item =
                    item::create(DRCE_INVALID_OUTLINE).expect("DRCE_INVALID_OUTLINE item");
                drc_item.set_error_detail("(Suspicious items found on Edge.Cuts layer)");
                drc_item.set_item_ids(vec![board_uuid()]);

                let centre = ctx.board.compute_bounding_box(ctx, false, false).centre();
                self.base.report_violation(
                    &mut **host.borrow_mut(),
                    Arc::new(drc_item),
                    centre,
                    PcbLayerId::EdgeCuts.0,
                    &no_path,
                );
            }
        }

        // Use the standard chaining epsilon here so that we report errors that might affect
        // other tools (such as 3D viewer).
        let chaining_epsilon = ctx.board.outlines_chaining_epsilon;

        // Arc to segment approximation error (not critical here: we do not use the outline
        // shape):
        let max_error = OUTLINE_MAX_ERROR;
        let mut dummy_outline = kicad_geom::ShapePolySet::new();

        if !build_board_polygon_outlines(
            ctx,
            &mut dummy_outline,
            max_error,
            chaining_epsilon,
            true,
            Some(&mut error_handler),
            false,
        ) {
            if error_handled.get() {
                // if there is an invalid outline, then there must be an outline
            } else {
                let mut drc_item =
                    item::create(DRCE_INVALID_OUTLINE).expect("DRCE_INVALID_OUTLINE item");
                drc_item.set_error_detail("(no edges found on Edge.Cuts layer)");
                drc_item.set_item_ids(vec![board_uuid()]);

                let centre = ctx.board.compute_bounding_box(ctx, false, false).centre();
                self.base.report_violation(
                    &mut **host.borrow_mut(),
                    Arc::new(drc_item),
                    centre,
                    PcbLayerId::EdgeCuts.0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testDisabledLayers`.
    fn test_disabled_layers(&self, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        let progress_delta = 2000;
        let mut ii = 0;
        let mut items = 0;

        let mut disabled_layers = !ctx.board.enabled_layers();

        // Perform the test only for copper layers
        disabled_layers = &disabled_layers & Lset::all_cu_mask();

        for_each_geometry_item(ctx, all_basic_items(), Lset::all_layers_mask(), |_item| {
            items += 1;
            true
        });

        for_each_geometry_item(ctx, all_basic_items(), Lset::all_layers_mask(), |item| {
            if host.is_error_limit_exceeded(DRCE_DISABLED_LAYER_ITEM) {
                return false;
            }

            let index = ii;
            ii += 1;

            if !self
                .base
                .report_progress(host, index, items, progress_delta)
            {
                return false;
            }

            let mut bad_layer = UNDEFINED_LAYER;

            if let Some(pad) = item.as_pad() {
                if matches!(
                    pad.attribute(),
                    PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent
                ) {
                    if disabled_layers.contains(pad.principal_layer(ctx)) {
                        bad_layer = item.layer();
                    }
                } else {
                    // Through hole pad pierces all physical layers.
                }
            } else if let Some(via) = item.as_via() {
                let (top, bottom) = via.layer_pair();

                if disabled_layers.contains(top) {
                    bad_layer = top;
                } else if disabled_layers.contains(bottom) {
                    bad_layer = bottom;
                }
            } else if item.type_() == KicadT::PcbZoneT {
                // Footprint zones just get a top/bottom/inner setting, so they're on
                // whatever inner layers there are.
            } else {
                let bad_layers = &disabled_layers & &item.layer_set();

                if bad_layers.any() {
                    bad_layer = bad_layers.seq()[0];
                }
            }

            if bad_layer != UNDEFINED_LAYER {
                let mut drc_item =
                    item::create(DRCE_DISABLED_LAYER_ITEM).expect("DRCE_DISABLED_LAYER_ITEM item");
                drc_item.set_error_detail(&format!("(layer {})", layer_name(bad_layer)));
                drc_item.set_item_ids(vec![item.uuid()]);

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    item.position(),
                    UNDEFINED_LAYER.0,
                    &no_path,
                );
            }

            true
        });
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testAssertions` (sequential: the items in walk order).
    fn test_assertions(&self, engine: &DrcEngine, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        if !engine.has_rules_for_constraint_type(DrcConstraintT::AssertionConstraint) {
            return;
        }

        let mut all_items: Vec<ItemRef<'_>> = Vec::new();

        for_each_geometry_item(ctx, &[], Lset::all_layers_mask(), |item| {
            all_items.push(item);
            true
        });

        let item_count = all_items.len();

        for (items_done, item) in all_items.into_iter().enumerate() {
            if !host.is_error_limit_exceeded(DRCE_ASSERTION_FAILURE) && !host.is_cancelled() {
                let mut failures: Vec<DrcConstraint> = Vec::new();

                engine.process_assertions(item, &mut |c: &DrcConstraint| failures.push(c.clone()), None);

                for c in failures {
                    let mut drc_item =
                        item::create(DRCE_ASSERTION_FAILURE).expect("DRCE_ASSERTION_FAILURE item");
                    drc_item.set_error_detail(&format!("({})", c.name()));
                    drc_item.set_item_ids(vec![item.uuid()]);
                    drc_item.violating_rule = c.parent_rule.clone();

                    self.base.report_violation(
                        host,
                        Arc::new(drc_item),
                        item.position(),
                        item.layer().0,
                        &no_path,
                    );
                }
            }

            // reportProgress( itemsDone, itemCount ) of the waiting loop
            let _ = (items_done, item_count);
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testTextVars`.
    fn test_text_vars(&self, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        let progress_delta = 2000;
        let mut ii = 0;
        let mut items = 0;

        let item_types = [
            KicadT::PcbFieldT,
            KicadT::PcbTextT,
            KicadT::PcbTextboxT,
            KicadT::PcbTablecellT,
            KicadT::PcbDimensionT,
        ];

        for_each_geometry_item(ctx, &item_types, Lset::all_layers_mask(), |_item| {
            items += 1;
            true
        });

        for_each_geometry_item(ctx, &item_types, Lset::all_layers_mask(), |item| {
            if host.is_error_limit_exceeded(DRCE_UNRESOLVED_VARIABLE) {
                return false;
            }

            let index = ii;
            ii += 1;

            if !self
                .base
                .report_progress(host, index, items, progress_delta)
            {
                return false;
            }

            if let Some(text_item) = item.as_eda_text() {
                // A matched ${DRC_ERROR}/${DRC_WARNING} is the intended signal; the resolved
                // text still carries a literal ${...}, so flag the unresolved variable only
                // when no marker fired (matching the drawing-sheet path below).
                if self.test_assertion(
                    host,
                    Some(item.uuid()),
                    &text_item.eda_text().text,
                    item.position(),
                    item.layer().0,
                ) {
                    // Skip unresolved-variable test.
                    // Yes, the assertion message could have an unresolved variable in it, but
                    // it's going to end up in the DRC window anyway, so there's not much sense
                    // in double-reporting it.
                } else if var_ref_matches(&expand_env_var_substitutions(
                    &text_item.shown_text(ctx, ResolutionContext::ForErcDrc, 0),
                    &self.environment,
                )) {
                    let mut drc_item = item::create(DRCE_UNRESOLVED_VARIABLE)
                        .expect("DRCE_UNRESOLVED_VARIABLE item");
                    drc_item.set_item_ids(vec![item.uuid()]);

                    self.base.report_violation(
                        host,
                        Arc::new(drc_item),
                        item.position(),
                        item.layer().0,
                        &no_path,
                    );
                }
            }

            true
        });

        let Some(drawing_sheet) = self.drawing_sheet.as_ref() else {
            return;
        };

        if host.is_error_limit_exceeded(DRCE_UNRESOLVED_VARIABLE) {
            return;
        }

        for text in &drawing_sheet.texts {
            if host.is_error_limit_exceeded(DRCE_UNRESOLVED_VARIABLE) {
                break;
            }

            if host.is_cancelled() {
                return;
            }

            if self.test_assertion(host, None, &text.text, text.position, LAYER_DRAWINGSHEET) {
                // Don't run unresolved test
            } else if var_ref_matches(&text.shown_text) {
                let mut drc_item = item::create(DRCE_UNRESOLVED_VARIABLE)
                    .expect("DRCE_UNRESOLVED_VARIABLE item");
                drc_item.set_item_ids(vec![drawing_sheet.proxy_uuid]);

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    text.position,
                    LAYER_DRAWINGSHEET,
                    &no_path,
                );
            }
        }
    }

    /// The `testAssertion` lambda of `testTextVars`: reports every `${DRC_WARNING ...}` and
    /// `${DRC_ERROR ...}` marker of `text`; true when any was matched. `item` is `None` for a
    /// drawing-sheet text.
    fn test_assertion(
        &self,
        host: &mut RunHost<'_, '_>,
        item: Option<Kiid>,
        text: &str,
        pos: Vector2I,
        layer: i32,
    ) -> bool {
        let found_warning = self.report_each(
            host,
            item,
            text,
            pos,
            layer,
            "DRC_WARNING",
            DRCE_GENERIC_WARNING,
        );
        let found_error =
            self.report_each(host, item, text, pos, layer, "DRC_ERROR", DRCE_GENERIC_ERROR);

        found_warning || found_error
    }

    /// The `reportEach` lambda of `testAssertion` for the expression
    /// `(^|[^\\])\$\{<keyword>\s*([^}]*)\}`.
    #[allow(clippy::too_many_arguments)]
    fn report_each(
        &self,
        host: &mut RunHost<'_, '_>,
        item: Option<Kiid>,
        text: &str,
        pos: Vector2I,
        layer: i32,
        keyword: &str,
        error_code: i32,
    ) -> bool {
        // Return true if any token of this kind was *matched*, even when the per-code error
        // limit had already been exceeded.  The caller uses the return value to suppress an
        // unrelated DRCE_UNRESOLVED_VARIABLE marker; if we let the limit-exceeded case fall
        // through to false we'd raise the wrong error.
        let mut found = false;
        let mut remaining: &str = text;

        while let Some(m) = match_marker(remaining, keyword) {
            found = true;

            if !host.is_error_limit_exceeded(error_code) {
                let mut drc_text = remaining[m.message.0..m.message.1].to_string();

                let mut drc_item = item::create(error_code).expect("generic DRC item");

                if let Some(uuid) = item {
                    drc_item.set_item_ids(vec![uuid]);
                } else {
                    drc_text += " (in drawing sheet)";
                }

                drc_item.set_error_message(&drc_text);

                self.base
                    .report_violation(host, Arc::new(drc_item), pos, layer, &no_path);
            }

            if m.whole.1 == m.whole.0 {
                break;
            }

            remaining = &remaining[m.whole.1..];
        }

        found
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testMissingTuningProfiles`.
    fn test_missing_tuning_profiles(&self, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        if ctx.board.project.is_none() {
            return;
        }

        let net_settings = &ctx.board.design_settings().net_settings;

        let mut profile_names: BTreeSet<&str> = BTreeSet::new();

        for tuning_profile in &self.tuning_profiles.tuning_profiles {
            let name = tuning_profile.profile_name.as_str();

            if !name.is_empty() {
                profile_names.insert(name);
            }
        }

        for (name, netclass) in net_settings.netclasses() {
            if host.is_error_limit_exceeded(DRCE_MISSING_TUNING_PROFILE) {
                return;
            }

            let profile_name = netclass.tuning_profile();

            if netclass.has_tuning_profile() && !profile_names.contains(profile_name) {
                let mut drc_item = item::create(DRCE_MISSING_TUNING_PROFILE)
                    .expect("DRCE_MISSING_TUNING_PROFILE item");
                drc_item.set_error_detail(&format!(
                    "(Net Class: {name}, Tuning Profile: {profile_name})"
                ));

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    Vector2I::default(),
                    UNDEFINED_LAYER.0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MISC::testUnquotableNetclasses`.
    fn test_unquotable_netclasses(&self, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) {
        let net_settings = &ctx.board.design_settings().net_settings;
        let mut names: BTreeSet<String> = BTreeSet::new();
        names.insert(net_settings.default_netclass().name.clone());

        for netclass in net_settings.netclasses().values() {
            names.insert(netclass.name.clone());
        }

        // A label can assign a class nobody declared, so name the parts of a composite
        // DRC_ENGINE dropped
        for netclass in net_settings.composite_netclasses().values() {
            if escape_string_literal(&netclass.name).is_some() {
                continue;
            }

            for constituent in netclass.constituent_netclasses() {
                names.insert(constituent.name.clone());
            }
        }

        for name in &names {
            if host.is_error_limit_exceeded(DRCE_GENERIC_WARNING) {
                return;
            }

            // DRC_ENGINE drops the implicit rules of such a class, so its limits would go
            // unchecked without a word
            if escape_string_literal(name).is_none() {
                let mut drc_item =
                    item::create(DRCE_GENERIC_WARNING).expect("DRCE_GENERIC_WARNING item");
                drc_item.set_error_message(&format!(
                    "Net class '{name}' cannot be used in rules; its clearance and width are not \
                     checked"
                ));

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    Vector2I::default(),
                    UNDEFINED_LAYER.0,
                    &no_path,
                );
            }
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderMisc {
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

/// `m_board->m_Uuid` for `SetItems( m_board )`. The board model of the port has no UUID of its
/// own (`BOARD::m_Uuid` is a fresh random KIID per run in C++, and `ResolveItem` maps it to the
/// board): the nil UUID stands for it, which the report writers cannot resolve
/// (`PORT_STATUS.md`, deviations).
fn board_uuid() -> Kiid {
    Kiid::default()
}

/// `RC_ITEM::SetItems( aItem, bItem )` with item pointers: the UUIDs of the items present.
fn item_uuids(ctx: &BoardCtx<'_>, ids: &[Option<ItemId>]) -> Vec<Kiid> {
    ids.iter()
        .flatten()
        .filter_map(|&id| ctx.board.uuid_of(id))
        .collect()
}

/// `PCB_SHAPE::GetEffectiveShape()` with the default arguments.
fn shape_effective_shape(
    ctx: &BoardCtx<'_>,
    shape: ShapeId,
) -> Option<kicad_pcb::item::SharedShape> {
    let item = ctx.item(shape.into())?;

    Some(item.effective_shape(
        UNDEFINED_LAYER,
        kicad_core::layer_ids::Flashing::Default,
        DrcConstraintT::NullConstraint,
    ))
}

/// Port of the static `findClosestOutlineGap( aBoard, aItemA, aItemB, aMidpoint, aDistance )`:
/// the closest pair of Edge.Cuts shapes (first pair of equals), their nearest points' midpoint
/// and distance; `(None, None, (0, 0), 0)` without a pair.
fn find_closest_outline_gap(
    ctx: &BoardCtx<'_>,
) -> (Option<ShapeId>, Option<ShapeId>, Vector2I, i32) {
    // PCB_TYPE_COLLECTOR::Collect( aBoard, { PCB_SHAPE_T } ): BOARD::Visit scans the
    // footprints (each footprint's drawings) before the board drawings.
    let mut items: Vec<ShapeId> = Vec::new();

    for (fp, _) in ctx.board.footprints() {
        items.extend(
            ctx.board
                .footprint_drawings(fp)
                .filter_map(|(id, _)| id.typed::<PcbShape>()),
        );
    }

    items.extend(
        ctx.board
            .drawings()
            .filter_map(|(id, _)| id.typed::<PcbShape>()),
    );

    let shapes: Vec<ShapeId> = items
        .into_iter()
        .filter(|&id| {
            ctx.item(id.into())
                .is_some_and(|item| item.layer() == PcbLayerId::EdgeCuts)
        })
        .collect();

    let mut best = i32::MAX;
    let mut best_a = Vector2I::default();
    let mut best_b = Vector2I::default();
    let mut item_a: Option<ShapeId> = None;
    let mut item_b: Option<ShapeId> = None;

    for ii in 0..shapes.len() {
        let shape_a = shape_effective_shape(ctx, shapes[ii]);

        for jj in ii + 1..shapes.len() {
            let shape_b = shape_effective_shape(ctx, shapes[jj]);
            let mut pt_a = Vector2I::default();
            let mut pt_b = Vector2I::default();

            if let (Some(sa), Some(sb)) = (&shape_a, &shape_b) {
                if sa.nearest_points(&**sb, &mut pt_a, &mut pt_b) {
                    let dist = (pt_a - pt_b).euclidean_norm();

                    if dist < best {
                        best = dist;
                        best_a = pt_a;
                        best_b = pt_b;
                        item_a = Some(shapes[ii]);
                        item_b = Some(shapes[jj]);
                    }
                }
            }
        }
    }

    if item_a.is_some() && item_b.is_some() {
        (item_a, item_b, (best_a + best_b) / 2, best)
    } else {
        (item_a, item_b, Vector2I::default(), 0)
    }
}

// ---- regular expressions of testTextVars ------------------------------------------------------

/// Byte ranges of one match of a `testAssertion` expression in the searched text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MarkerMatch {
    /// Group 0: the whole match (including the leading character of group 1).
    whole: (usize, usize),
    /// Group 2: the message.
    message: (usize, usize),
}

/// `[[:space:]]` of the wx regular expression engine (`\s`).
fn is_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// `wxRegEx( "(^|[^\\\\])\\$\\{<keyword>\\s*([^}]*)\\}" ).Matches( text )` with the
/// `GetMatch` ranges of the leftmost match. `.` and `[^...]` match newlines (no
/// `wxRE_NEWLINE`), `^` matches only at the start of the text.
fn match_marker(text: &str, keyword: &str) -> Option<MarkerMatch> {
    let prefix = format!("${{{keyword}");

    // Candidate match starts, leftmost first: a start at the beginning of the text with an
    // empty group 1, or any character but a backslash followed by the marker.
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (match start, marker start)

    if text.starts_with(&prefix) {
        starts.push((0, 0));
    }

    for (i, c) in text.char_indices() {
        if c != '\\' {
            let marker = i + c.len_utf8();

            if text[marker..].starts_with(&prefix) {
                starts.push((i, marker));
            }
        }
    }

    starts.sort();

    for (start, marker) in starts {
        let mut p = marker + prefix.len();

        // \s* (greedy; the following [^}]* takes the rest)
        while let Some(c) = text[p..].chars().next() {
            if !is_regex_space(c) {
                break;
            }

            p += c.len_utf8();
        }

        let message_start = p;

        // ([^}]*)\}
        if let Some(close) = text[p..].find('}') {
            let message_end = p + close;

            return Some(MarkerMatch {
                whole: (start, message_end + 1),
                message: (message_start, message_end),
            });
        }
    }

    None
}

/// `varRefRegEx.Matches( text )` for `(^|[^\\])\$\{.*\}.*`: a `${` at the start of the text
/// or after any character but a backslash, followed somewhere by a `}`.
fn var_ref_matches(text: &str) -> bool {
    let mut prev: Option<char> = None;

    for (i, c) in text.char_indices() {
        if text[i..].starts_with("${") && prev.is_none_or(|p| p != '\\') {
            if text[i + 2..].contains('}') {
                return true;
            }
        }

        let _ = c;
        prev = Some(c);
    }

    false
}

// ---- ExpandEnvVarSubstitutions (common/common.cpp) -------------------------------------------

/// `ENV_VAR::GetVersionedEnvVarName( aBaseName )` for KiCad 10.
fn versioned_env_var_name(base_name: &str) -> String {
    format!("KICAD10_{base_name}")
}

/// `ENV_VAR::GetPredefinedEnvVars()`.
fn predefined_env_vars() -> Vec<String> {
    vec![
        "KIPRJMOD".to_string(),
        versioned_env_var_name("SYMBOL_DIR"),
        versioned_env_var_name("3DMODEL_DIR"),
        versioned_env_var_name("FOOTPRINT_DIR"),
        versioned_env_var_name("TEMPLATE_DIR"),
        "KICAD_USER_TEMPLATE_DIR".to_string(),
        "KICAD_PTEMPLATES".to_string(),
        versioned_env_var_name("3RD_PARTY"),
    ]
}

/// Port of `ENV_VAR::IsVersionedEnvVar`: `KICAD<digits>_<base name>`.
fn is_versioned_env_var(name: &str, base_name: &str) -> bool {
    let prefix = "KICAD";
    let suffix = format!("_{base_name}");

    if !name.starts_with(prefix) || !name.ends_with(&suffix) {
        return false;
    }

    if name.len() < prefix.len() + suffix.len() {
        return false;
    }

    let version = &name[prefix.len()..name.len() - suffix.len()];

    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit())
}

/// `wxString::Matches( aMask )` for the masks of `getVersionedEnvVar` (`*` = any run).
fn wx_matches_star(text: &str, mask: &str) -> bool {
    match mask.split_once('*') {
        None => text == mask,
        Some((head, tail)) => {
            text.len() >= head.len() + tail.len() && text.starts_with(head) && text.ends_with(tail)
        }
    }
}

/// Port of `ExpandEnvVarSubstitutions( aString, aProject = nullptr, aAllowEscapes = true )`
/// (`KIwxExpandEnvVars` without a project), with `environment` standing for `wxGetEnv`.
/// Unix behaviour (`%` is not a control character). The "missing bracket" `wxLogWarning` is
/// not reproduced.
pub(crate) fn expand_env_var_substitutions(
    text: &str,
    environment: &BTreeMap<String, String>,
) -> String {
    let mut loop_check = BTreeSet::new();
    kiwx_expand_env_vars(text, environment, &mut loop_check, true)
}

/// The `Bracket` of `KIwxExpandEnvVars`: none, `(`, `{`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bracket {
    None,
    Normal,
    Curly,
}

impl Bracket {
    /// The enumerator value (`Bracket_None` = 0, `Bracket_Normal = ')'`,
    /// `Bracket_Curly = '}'`).
    fn value(self) -> i32 {
        match self {
            Bracket::None => 0,
            Bracket::Normal => ')' as i32,
            Bracket::Curly => '}' as i32,
        }
    }

    /// The closing character.
    fn close(self) -> char {
        match self {
            Bracket::None => '\0',
            Bracket::Normal => ')',
            Bracket::Curly => '}',
        }
    }
}

/// Port of `KIwxExpandEnvVars( str, nullptr, aSet, aAllowEscapes )`.
fn kiwx_expand_env_vars(
    text: &str,
    environment: &BTreeMap<String, String>,
    set: &mut BTreeSet<String>,
    allow_escapes: bool,
) -> String {
    // If the same string is inserted twice, we have a loop
    if !set.insert(text.to_string()) {
        return text.to_string();
    }

    let s: Vec<char> = text.chars().collect();
    let strlen = s.len();
    let mut result = String::with_capacity(text.len());

    let read_environment = |name: &str| environment.get(name).cloned();

    let get_versioned_env_var = |mask: &str, result: &mut String| -> bool {
        for var in predefined_env_vars() {
            if wx_matches_star(&var, mask) {
                let Some(value) = read_environment(&var) else {
                    continue;
                };

                result.push_str(&value);
                return true;
            }
        }

        false
    };

    let is_name_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':';

    let mut n = 0;

    while n < strlen {
        let mut str_n = s[n];

        match str_n {
            '$' => {
                let control_char = str_n;
                let bracket;

                if n == strlen - 1 {
                    bracket = Bracket::None;
                } else {
                    match s[n + 1] {
                        '(' => {
                            bracket = Bracket::Normal;
                            n += 1;
                            str_n = s[n]; // skip the bracket
                        }
                        '{' => {
                            bracket = Bracket::Curly;
                            n += 1;
                            str_n = s[n]; // skip the bracket
                        }
                        _ => bracket = Bracket::None,
                    }
                }

                let mut m = n + 1;

                if m >= strlen {
                    // `break` out of the switch: nothing is appended for the control
                    // character (and the opening bracket).
                    n += 1;
                    continue;
                }

                let mut str_m = s[m];

                while is_name_char(str_m) {
                    m += 1;

                    if m == strlen {
                        str_m = '\0';
                        break;
                    }

                    str_m = s[m];
                }

                let var_name: String = s[n + 1..m].iter().collect();

                if control_char == '$'
                    && bracket == Bracket::Curly
                    && matches!(
                        var_name.as_str(),
                        "DRC_WARNING" | "DRC_ERROR" | "ERC_WARNING" | "ERC_ERROR"
                    )
                {
                    // These aren't environment variables; pass them through unchanged
                    // (`strResult << controlChar << bracket << strVarName << str_m`: the
                    // unscoped `Bracket` enum promotes to `int`, so its value is streamed as
                    // a decimal number, "125" for `Bracket_Curly`).
                    result.push(control_char);
                    result.push_str(&bracket.value().to_string());
                    result.push_str(&var_name);
                    result.push(str_m);
                    n = m + 1;
                    continue;
                }

                // NB: use wxGetEnv instead of wxGetenv as otherwise variables
                //     set through wxSetEnv may not be read correctly!
                let mut expanded = false;

                if let Some(value) = read_environment(&var_name) {
                    result.push_str(&value);
                    expanded = true;
                }
                // Replace unmatched older variables with current locations
                // If the user has the older location defined, that will be matched
                // first above.  But if they do not, this will ensure that their board still
                // displays correctly
                else if var_name.contains("KISYS3DMOD")
                    || is_versioned_env_var(&var_name, "3DMODEL_DIR")
                {
                    if get_versioned_env_var("KICAD*_3DMODEL_DIR", &mut result) {
                        expanded = true;
                    }
                } else if var_name == "KICAD_SYMBOL_DIR"
                    || is_versioned_env_var(&var_name, "SYMBOL_DIR")
                {
                    if get_versioned_env_var("KICAD*_SYMBOL_DIR", &mut result) {
                        expanded = true;
                    }
                } else if is_versioned_env_var(&var_name, "FOOTPRINT_DIR") {
                    if get_versioned_env_var("KICAD*_FOOTPRINT_DIR", &mut result) {
                        expanded = true;
                    }
                } else if is_versioned_env_var(&var_name, "3RD_PARTY") {
                    if get_versioned_env_var("KICAD*_3RD_PARTY", &mut result) {
                        expanded = true;
                    }
                } else {
                    // variable doesn't exist => don't change anything
                    if bracket != Bracket::None {
                        result.push(s[n - 1]);
                    }

                    result.push(str_n);
                    result.push_str(&var_name);
                }

                // When a versioned-wildcard branch matched but no env var was found, emit
                // the original ${VARNAME} text so the closing-bracket handler can append the
                // closing bracket.  Without this, the handler emits only '}', producing a
                // garbage path like "}/Device.kicad_sym" instead of the full unexpanded var.
                if !expanded && bracket != Bracket::None {
                    let is_versioned_wildcard = var_name.contains("KISYS3DMOD")
                        || var_name == "KICAD_SYMBOL_DIR"
                        || is_versioned_env_var(&var_name, "3DMODEL_DIR")
                        || is_versioned_env_var(&var_name, "SYMBOL_DIR")
                        || is_versioned_env_var(&var_name, "FOOTPRINT_DIR")
                        || is_versioned_env_var(&var_name, "3RD_PARTY");

                    if is_versioned_wildcard {
                        result.push(s[n - 1]);
                        result.push(str_n);
                        result.push_str(&var_name);
                    }
                }

                // check the closing bracket
                if bracket != Bracket::None {
                    if m == strlen || str_m != bracket.close() {
                        // under Unix this warning could be useful for the user to
                        // understand why isn't the variable expanded as intended
                        // (wxLogWarning: not reproduced)
                    } else {
                        // skip closing bracket unless the variables wasn't expanded
                        if !expanded {
                            result.push(bracket.close());
                        }

                        m += 1;
                    }
                }

                n = m; // skip variable name (n = m - 1, then the loop's n++)
                continue;
            }

            '\\' => {
                // backslash can be used to suppress special meaning of % and $
                if allow_escapes && n < strlen - 1 && (s[n + 1] == '%' || s[n + 1] == '$') {
                    result.push(str_n);
                    n += 1;
                    result.push(s[n]);
                    n += 1;
                    continue;
                }

                result.push(str_n);
            }

            _ => result.push(str_n),
        }

        n += 1;
    }

    let first_pos = result.find(['{', '(', '%']);
    let last_pos = result.rfind(['}', ')', '%']);

    if let (Some(first), Some(last)) = (first_pos, last_pos) {
        if first != last {
            return kiwx_expand_env_vars(&result, environment, set, allow_escapes);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Undoes the escaping of `drc23_text_oracle.cpp` (`\\`, `\n`, `\t`, `\0`).
    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();

        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('0') => out.push('\0'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }

        out
    }

    /// The oracle lines of one input, computed by the port.
    fn port_lines(text: &str, environment: &BTreeMap<String, String>) -> Vec<String> {
        let esc = |s: &str| {
            s.replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
                .replace('\0', "\\0")
        };
        let mut lines = vec![
            format!("input {}", esc(text)),
            format!(
                "expand {}",
                esc(&expand_env_var_substitutions(text, environment))
            ),
            format!("varref {}", i32::from(var_ref_matches(text))),
        ];

        for keyword in ["DRC_WARNING", "DRC_ERROR"] {
            let mut remaining = text;

            while let Some(m) = match_marker(remaining, keyword) {
                let chars = |a: usize, b: usize| remaining[a..b].chars().count();
                let start = chars(0, m.whole.0);
                let len = chars(m.whole.0, m.whole.1);

                lines.push(format!(
                    "{keyword} {start} {len} {}",
                    esc(&remaining[m.message.0..m.message.1])
                ));

                if len == 0 {
                    break;
                }

                remaining = &remaining[m.whole.1..];
            }
        }

        lines
    }

    /// `ExpandEnvVarSubstitutions`, `varRefRegEx` and the `reportEach` matching loop against
    /// the reference build (`tests/oracle/build_drc23_text_oracle.sh`, golden
    /// `tests/golden/drc23/text_oracle.txt`).
    #[test]
    fn text_helpers_match_reference() {
        let golden = include_str!("../../tests/golden/drc23/text_oracle.txt");
        let environment: BTreeMap<String, String> = [
            ("KICAD10_FOOTPRINT_DIR".to_string(), "/fp".to_string()),
            ("DRC23_VAR".to_string(), "value".to_string()),
        ]
        .into_iter()
        .collect();

        let mut blocks: Vec<Vec<String>> = Vec::new();

        for line in golden.lines() {
            if line.starts_with("input ") {
                blocks.push(Vec::new());
            }

            blocks.last_mut().unwrap().push(line.to_string());
        }

        assert!(blocks.len() > 40);

        for block in blocks {
            let text = unescape(&block[0]["input ".len()..]);
            assert_eq!(port_lines(&text, &environment), block, "input {text:?}");
        }
    }
}
