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

//! Port of `pcbnew/drc/drc_test_provider_text_dims.cpp`.
//!
//! Text dimensions tests.
//! Errors generated:
//! - `DRCE_TEXT_HEIGHT`
//! - `DRCE_TEXT_THICKNESS`
//!
//! The outline (TrueType) font branch works on the glyphs of the text's render cache
//! (`EDA_TEXT::GetRenderCache`). The port has no outline font engine: the glyphs stored in the
//! board file stand in for the regenerated ones while they are valid for the text (kicad-pcb
//! package FONT-RC-1); otherwise (C++ regenerates them with `OUTLINE_FONT::GetLinesAsGlyphs`)
//! the provider stops with `port_pending!`.

use std::cell::Cell;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::lset::Lset;
use kicad_core::text_vars::ResolutionContext;
use kicad_geom::Vector2I;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_pcb::drc_api::{DRCE_TEXT_HEIGHT, DRCE_TEXT_THICKNESS};
use kicad_pcb::{BoardCtx, EdaTextItem, ItemRef, eda_text};

use super::run_host::{RunHost, report_aux};
use super::text_mirroring::TEXT_ITEM_TYPES;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, for_each_geometry_item, no_path};

/// Port of `DRC_TEST_PROVIDER_TEXT_DIMS`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderTextDims {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderTextDims {
    fn default() -> Self {
        DrcTestProviderTextDims {
            base: DrcTestProviderBase::new("text_dimensions"),
        }
    }
}

impl DrcTestProviderTextDims {
    /// Port of `DRC_TEST_PROVIDER_TEXT_DIMS::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let progress_delta = 250;
        let mut count = 0;
        // `ii` is shared by the progress counter and the outline loops of
        // `checkTextThickness` in C++ (the lambda captures it by reference), which resets it;
        // only the progress values depend on it.
        let ii = Cell::new(0usize);

        if run.is_error_limit_exceeded(DRCE_TEXT_HEIGHT)
            && run.is_error_limit_exceeded(DRCE_TEXT_THICKNESS)
        {
            report_aux(run, "Text dimension violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        if !engine.has_rules_for_constraint_type(DrcConstraintT::TextHeightConstraint)
            && !engine.has_rules_for_constraint_type(DrcConstraintT::TextThicknessConstraint)
        {
            report_aux(
                run,
                "No text height or text thickness constraints found. Tests not run.",
            );
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking text dimensions...")
        {
            return false; // DRC cancelled
        }

        let all_layers = Lset::all_layers_mask();

        for_each_geometry_item(ctx, &TEXT_ITEM_TYPES, all_layers, |_item| {
            count += 1;
            true
        });

        for_each_geometry_item(ctx, &TEXT_ITEM_TYPES, all_layers, |item| {
            let index = ii.get();
            ii.set(index + 1);

            if !self
                .base
                .report_progress(&mut host, index, count, progress_delta)
            {
                return false;
            }

            if let Some(text) = item.as_eda_text() {
                let mut strikes = 0;

                if !text.eda_text().is_visible() {
                    return true;
                }

                if host.is_error_limit_exceeded(DRCE_TEXT_THICKNESS) {
                    strikes += 1;
                } else {
                    self.check_text_thickness(engine, &mut host, ctx, item, text, &ii);
                }

                if host.is_error_limit_exceeded(DRCE_TEXT_HEIGHT) {
                    strikes += 1;
                } else {
                    self.check_text_height(engine, &mut host, ctx, item, text);
                }

                if strikes >= 2 {
                    return false;
                }
            }

            true
        });

        !host.is_cancelled()
    }

    /// The `checkTextHeight` lambda of `Run`: false when the error limit is reached.
    pub fn check_text_height(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        text: &dyn EdaTextItem,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_TEXT_HEIGHT) {
            return false;
        }

        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::TextHeightConstraint,
            Some(item),
            None,
            item.layer(),
        );

        if constraint.severity() == Severity::Ignore {
            return true;
        }

        let actual_height = text.text_size(ctx).y;

        if constraint.value().has_min() && actual_height < constraint.value().min() {
            let mut drc_item = item::create(DRCE_TEXT_HEIGHT).expect("DRCE_TEXT_HEIGHT item");
            drc_item.set_error_detail(&self.base.format_msg(
                "(%s min height %s; actual %s)",
                &constraint.name(),
                constraint.value().min() as f64,
                actual_height as f64,
                EdaDataType::Distance,
            ));
            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            self.base.report_violation(
                host,
                Arc::new(drc_item),
                item.position(),
                item.layer().0,
                &no_path,
            );
        }

        if constraint.value().has_max() && actual_height > constraint.value().max() {
            let mut drc_item = item::create(DRCE_TEXT_HEIGHT).expect("DRCE_TEXT_HEIGHT item");
            drc_item.set_error_detail(&self.base.format_msg(
                "(%s max height %s; actual %s)",
                &constraint.name(),
                constraint.value().max() as f64,
                actual_height as f64,
                EdaDataType::Distance,
            ));
            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();

            self.base.report_violation(
                host,
                Arc::new(drc_item),
                item.position(),
                item.layer().0,
                &no_path,
            );
        }

        true
    }

    /// The `checkTextThickness` lambda of `Run` (`ii`: the shared counter of `Run`, see
    /// there).
    pub fn check_text_thickness(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        text: &dyn EdaTextItem,
        ii: &Cell<usize>,
    ) -> bool {
        let constraint = engine.eval_rules(
            ctx,
            DrcConstraintT::TextThicknessConstraint,
            Some(item),
            None,
            item.layer(),
        );

        if constraint.severity() == Severity::Ignore {
            return true;
        }

        // KIFONT::FONT* font = text->GetDrawFont( nullptr ); font->IsOutline()
        let is_outline = text.eda_text().draw_font_lookup().is_outline_unavailable();

        if is_outline {
            if !constraint.value().has_min() || constraint.value().min() <= 0 {
                return true;
            }

            let shown = text.shown_text(ctx, ResolutionContext::ForCanvas, 0);
            let Some(glyphs) = eda_text::render_cache(text, ctx, &shown, Vector2I::default())
            else {
                crate::port_pending!(
                    "EDA_TEXT::GetRenderCache regeneration (KIFONT::OUTLINE_FONT::GetLinesAsGlyphs)",
                    shown
                )
            };
            let mut collapsed_stroke = false;
            let mut collapsed_area = false;
            let half_min = constraint.value().min() / 2;

            for glyph in &glyphs.glyphs {
                // Ensure the glyph is a OUTLINE_GLYPH (for instance, overbars in outline
                // font text are represented as STROKE_GLYPHs).
                let Some(outline_glyph) = glyph.as_outline() else {
                    continue;
                };

                let outline_count = outline_glyph.poly.outline_count();
                let mut hole_count = 0;

                if outline_count == 0 {
                    continue; // ignore spaces
                }

                ii.set(0);

                while (ii.get() as i32) < outline_count {
                    hole_count += outline_glyph.poly.hole_count(ii.get() as i32);
                    ii.set(ii.get() + 1);
                }

                let mut poly = outline_glyph.poly.clone_drop_triangulation();
                poly.deflate(half_min, CornerStrategy::ChamferAllCorners, ARC_LOW_DEF);
                poly.simplify();

                let resulting_outline_count = poly.outline_count();
                let mut resulting_hole_count = 0;

                ii.set(0);

                while (ii.get() as i32) < resulting_outline_count {
                    resulting_hole_count += poly.hole_count(ii.get() as i32);
                    ii.set(ii.get() + 1);
                }

                if resulting_outline_count != outline_count || resulting_hole_count != hole_count {
                    collapsed_stroke = true;
                    break;
                }

                let glyph_area = outline_glyph.poly.area();

                if glyph_area == 0.0 {
                    continue;
                }

                poly.inflate(
                    half_min,
                    CornerStrategy::ChamferAllCorners,
                    ARC_LOW_DEF,
                    true,
                );

                let resulting_glyph_area = poly.area();

                if ((resulting_glyph_area - glyph_area).abs() / glyph_area) > 0.1 {
                    collapsed_area = true;
                    break;
                }
            }

            if collapsed_stroke || collapsed_area {
                let mut drc_item =
                    item::create(DRCE_TEXT_THICKNESS).expect("DRCE_TEXT_THICKNESS item");
                drc_item
                    .set_error_detail("(TrueType font characters with insufficient stroke weight)");
                drc_item.set_item_ids(vec![item.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    item.position(),
                    item.layer().0,
                    &no_path,
                );
            }
        } else {
            let actual_thickness = eda_text::effective_text_pen_width(text, ctx, 0);

            if constraint.value().has_min() && actual_thickness < constraint.value().min() {
                let mut drc_item =
                    item::create(DRCE_TEXT_THICKNESS).expect("DRCE_TEXT_THICKNESS item");
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s min thickness %s; actual %s)",
                    &constraint.name(),
                    constraint.value().min() as f64,
                    actual_thickness as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![item.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    item.position(),
                    item.layer().0,
                    &no_path,
                );
            }

            if constraint.value().has_max() && actual_thickness > constraint.value().max() {
                let mut drc_item =
                    item::create(DRCE_TEXT_THICKNESS).expect("DRCE_TEXT_THICKNESS item");
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max thickness %s; actual %s)",
                    &constraint.name(),
                    constraint.value().max() as f64,
                    actual_thickness as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![item.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();

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
    }
}

impl DrcTestProviderRun for DrcTestProviderTextDims {
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
