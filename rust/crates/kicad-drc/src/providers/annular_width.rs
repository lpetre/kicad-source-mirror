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

//! Port of `pcbnew/drc/drc_test_provider_annular_width.cpp`.
//!
//! Via/pad annular ring width test. Checks if there's sufficient copper ring around PTH/NPTH
//! holes (vias/pads). Errors generated:
//! - `DRCE_ANNULAR_WIDTH`

use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::trigo::rotate_point_around;
use kicad_pcb::drc_api::{DRCE_ANNULAR_WIDTH, DrcConstraint};
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadAttrib, PadShape, PadstackMode};
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx, ItemRef};

use super::run_host::{RunHost, report_aux};
use super::via_diameter::padstack_layer_desc;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rule::DrcConstraintExt;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_ANNULAR_WIDTH`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderAnnularWidth {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderAnnularWidth {
    fn default() -> Self {
        DrcTestProviderAnnularWidth {
            base: DrcTestProviderBase::new("annular_width"),
        }
    }
}

/// The item a check runs on (the C++ `BOARD_ITEM*`, a via or a pad).
#[derive(Clone, Copy)]
enum Checked<'a> {
    Track(&'a Track),
    Pad(&'a Pad),
}

/// The `calcEffort` lambda of `Run`: the progress weight of an item.
fn calc_effort(ctx: &BoardCtx<'_>, item: Checked<'_>) -> usize {
    match item {
        Checked::Track(Track::Via(_)) => 1,
        Checked::Track(_) => 0,
        Checked::Pad(pad) => {
            if !pad.has_hole(ctx) || pad.attribute() != PadAttrib::Pth {
                return 0;
            }

            let mut effort = 0;

            pad.lib_padstack().for_each_unique_layer(|layer| {
                if pad.offset(ctx, layer) == Vector2I::new(0, 0) {
                    match pad.shape(layer) {
                        PadShape::ChamferedRect => {
                            // C++: `if( ratio > 0.30 ) break;` then falls through.
                            #[allow(clippy::neg_cmp_op_on_partial_ord)]
                            if !(pad.chamfer_rect_ratio(layer) > 0.30) {
                                effort += 1;
                            }
                        }
                        PadShape::Circle
                        | PadShape::Oval
                        | PadShape::Rectangle
                        | PadShape::Roundrect => {
                            effort += 1;
                        }
                        _ => {}
                    }
                }

                effort += 5;
            });

            effort
        }
    }
}

/// The `getPadAnnulusPts` lambda of `Run`: the two points whose distance is the annular width
/// of `pad` on `layer`. The points are left untouched (the caller's zero vectors) when none of
/// the branches sets them, as in C++.
fn get_pad_annulus_pts(
    ctx: &BoardCtx<'_>,
    pad: &Pad,
    layer: PcbLayerId,
    constraint: &DrcConstraint,
    same_num_pads: &[&Pad],
    pt_a: &mut Vector2I,
    pt_b: &mut Vector2I,
) {
    let mut handled = false;

    if pad.offset(ctx, layer) == Vector2I::new(0, 0) {
        let size = pad.size(ctx, layer);
        let drill_size = pad.drill_size(ctx);
        let x_dist: i32 = ki_round((size.x - drill_size.x) as f64 / 2.0);
        let y_dist: i32 = ki_round((size.y - drill_size.y) as f64 / 2.0);
        let position = pad.position(ctx);

        if y_dist < x_dist {
            *pt_a = position - Vector2I::new(0, drill_size.y / 2);
            *pt_b = position - Vector2I::new(0, size.y / 2);
        } else {
            *pt_a = position - Vector2I::new(drill_size.x / 2, 0);
            *pt_b = position - Vector2I::new(size.x / 2, 0);
        }

        rotate_point_around(pt_a, position, pad.orientation(ctx));
        rotate_point_around(pt_b, position, pad.orientation(ctx));

        match pad.shape(layer) {
            PadShape::ChamferedRect => {
                handled = pad.chamfer_rect_ratio(layer) <= 0.30;
            }
            PadShape::Circle | PadShape::Oval | PadShape::Rectangle | PadShape::Roundrect => {
                handled = true;
            }
            _ => {}
        }
    }

    let pad_bbox = pad.bounding_box_layer(ctx, layer);
    let mut overlapping_same_num_pads: Vec<&Pad> = Vec::new();

    for &p in same_num_pads {
        if p.is_on_layer(ctx, layer) && pad_bbox.intersects(&p.bounding_box_layer(ctx, layer)) {
            overlapping_same_num_pads.push(p);
        }
    }

    // Same-number pads only add copper. Skip the slow path unless one fully covers this pad
    // (combined outline is then bigger than this pad alone) or one's drill cuts into this pad
    // (drill-to-drill copper becomes the real limit).
    let mut overlap_has_constraining_hole = false;
    let mut overlap_covers_this_pad = false;

    for p in &overlapping_same_num_pads {
        if p.bounding_box_layer(ctx, layer).contains_box(&pad_bbox) {
            overlap_covers_this_pad = true;
        }

        if p.has_hole(ctx) {
            let hole_bbox = p
                .effective_hole_shape(ctx, layer, DrcConstraintT::AnnularWidthConstraint)
                .expect("PAD::GetEffectiveHoleShape")
                .bbox(0);

            if pad_bbox.intersects(&hole_bbox) {
                overlap_has_constraining_hole = true;
            }
        }

        if overlap_covers_this_pad && overlap_has_constraining_hole {
            break;
        }
    }

    if handled
        && !overlapping_same_num_pads.is_empty()
        && !overlap_has_constraining_hole
        && !overlap_covers_this_pad
        && constraint.value().has_min()
        && !constraint.value().has_max()
    {
        // Circle: same annular width all around, so the fast value is exact whenever any
        // direction is uncovered. Non-circle has a narrow side an SMD can rescue by itself, so
        // trust the fast value here only when it already passes.
        if pad.shape(layer) == PadShape::Circle {
            return;
        } else {
            let width = (*pt_a - *pt_b).euclidean_norm();

            if width >= constraint.value().min() {
                return;
            }
        }
    }

    if !handled || !overlapping_same_num_pads.is_empty() {
        // Slow (but general purpose) method.
        let pad_item = ctx.view(AnyItem::Pad(pad));
        let max_error = pad_item.max_error();
        let position = pad.position(ctx);
        let mut pad_outline = ShapePolySet::new();
        let slot = pad
            .effective_hole_shape(ctx, layer, DrcConstraintT::AnnularWidthConstraint)
            .expect("PAD::GetEffectiveHoleShape");

        pad.transform_shape_to_polygon(
            ctx,
            &mut pad_outline,
            layer,
            0,
            max_error,
            ErrorLoc::Inside,
            false,
        );

        if same_num_pads.is_empty() {
            if !pad_outline.collide_point(position, 0, None, None) {
                // Hole outside pad
                *pt_a = position;
                *pt_b = position;
            } else {
                pad_outline.nearest_points(slot.as_ref(), pt_a, pt_b);
            }
        } else if constraint.value().has_min() {
            let mut aggregate_pad_outline = pad_outline.clone();
            let mut other_pad_holes = ShapePolySet::new();
            let mut slot_polygon = ShapePolySet::new();

            // C++ builds the slot polygon but does not use it.
            slot.transform_to_polygon(&mut slot_polygon, 0, ErrorLoc::Inside);

            for &same_num_pad in same_num_pads {
                // Construct the full pad with outline and hole.
                same_num_pad.transform_shape_to_polygon(
                    ctx,
                    &mut aggregate_pad_outline,
                    layer,
                    0,
                    max_error,
                    ErrorLoc::Outside,
                    false,
                );

                same_num_pad.transform_hole_to_polygon(
                    ctx,
                    &mut other_pad_holes,
                    0,
                    max_error,
                    ErrorLoc::Inside,
                );
            }

            aggregate_pad_outline.boolean_subtract(&other_pad_holes);

            if !aggregate_pad_outline.collide_point(position, 0, None, None) {
                // Hole outside pad
                *pt_a = position;
                *pt_b = position;
            } else {
                aggregate_pad_outline.nearest_points(slot.as_ref(), pt_a, pt_b);
            }
        }
    }
}

impl DrcTestProviderAnnularWidth {
    /// Port of `DRC_TEST_PROVIDER_ANNULAR_WIDTH::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_ANNULAR_WIDTH) {
            report_aux(run, "Annular width violations ignored. Skipping check.");
            return true; // continue with other tests
        }

        let progress_delta = 500;

        if !engine.has_rules_for_constraint_type(DrcConstraintT::AnnularWidthConstraint) {
            report_aux(run, "No annular width constraints found. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking pad & via annular rings...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let mut ii: usize = 0;
        let mut total: usize = 0;

        for (_, item) in board.tracks() {
            total += calc_effort(ctx, Checked::Track(item));
        }

        for (fp_id, _) in board.footprints() {
            for (_, pad) in board.footprint_pads(fp_id) {
                total += calc_effort(ctx, Checked::Pad(pad));
            }
        }

        for (_, item) in board.tracks() {
            ii += calc_effort(ctx, Checked::Track(item));

            if !self
                .base
                .report_progress(&mut host, ii, total, progress_delta)
            {
                return false; // DRC cancelled
            }

            if !self.check_annular_width(engine, &mut host, ctx, Checked::Track(item)) {
                break;
            }
        }

        for (fp_id, _) in board.footprints() {
            for (_, pad) in board.footprint_pads(fp_id) {
                ii += calc_effort(ctx, Checked::Pad(pad));

                if !self
                    .base
                    .report_progress(&mut host, ii, total, progress_delta)
                {
                    return false; // DRC cancelled
                }

                // C++: this `break` only leaves the pad loop of the current footprint.
                if !self.check_annular_width(engine, &mut host, ctx, Checked::Pad(pad)) {
                    break;
                }
            }
        }

        !host.is_cancelled()
    }

    /// The `checkConstraint` lambda of `Run`.
    #[allow(clippy::too_many_arguments)]
    fn check_constraint(
        &self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        item: ItemRef<'_>,
        padstack_mode: PadstackMode,
        pt_a: Vector2I,
        pt_b: Vector2I,
        layer: PcbLayerId,
    ) {
        if constraint.severity() == Severity::Ignore {
            return;
        }

        let mut v_min = 0;
        let mut v_max = 0;
        let mut fail_min = false;
        let mut fail_max = false;
        let width = (pt_a - pt_b).euclidean_norm();

        if constraint.value().has_min() {
            v_min = constraint.value().min();
            fail_min = width < v_min;
        }

        if constraint.value().has_max() {
            v_max = constraint.value().max();
            fail_max = width > v_max;
        }

        if fail_min || fail_max {
            let mut drc_item = item::create(DRCE_ANNULAR_WIDTH).expect("DRCE_ANNULAR_WIDTH item");

            if fail_min {
                if padstack_mode == PadstackMode::Normal {
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s min annular width %s; actual %s)",
                        &constraint.name(),
                        v_min as f64,
                        width as f64,
                        EdaDataType::Distance,
                    ));
                } else {
                    drc_item.set_error_detail(&self.base.format_msg_extra(
                        "(%s min annular width %s; actual %s on %s)",
                        &constraint.name(),
                        v_min as f64,
                        width as f64,
                        &padstack_layer_desc(ctx, layer, padstack_mode),
                        EdaDataType::Distance,
                    ));
                }
            }

            if fail_max {
                drc_item.set_error_detail(&self.base.format_msg(
                    "(%s max annular width %s; actual %s)",
                    &constraint.name(),
                    v_max as f64,
                    width as f64,
                    EdaDataType::Distance,
                ));
            }

            drc_item.set_item_ids(vec![item.uuid()]);
            drc_item.violating_rule = constraint.parent_rule.clone();
            self.base.report_two_point_geometry(
                host,
                Arc::new(drc_item),
                item.position(),
                pt_a,
                pt_b,
                layer,
            );
        }
    }

    /// The `checkAnnularWidth` lambda of `Run`: false when the error limit is reached.
    fn check_annular_width(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        checked: Checked<'_>,
    ) -> bool {
        if host.is_error_limit_exceeded(DRCE_ANNULAR_WIDTH) {
            return false;
        }

        match checked {
            Checked::Track(track) => {
                let Track::Via(via) = track else {
                    return true;
                };

                let item = ctx.view(AnyItem::Track(track));

                via.padstack().for_each_unique_layer(|layer| {
                    if via.is_ghost_layer(ctx, layer) {
                        return;
                    }

                    let constraint = engine.eval_rules(
                        ctx,
                        DrcConstraintT::AnnularWidthConstraint,
                        Some(item),
                        None,
                        layer,
                    );

                    let position = via.position(ctx);
                    let pt_a = position - Vector2I::new(via.drill_value(ctx) / 2, 0);
                    let pt_b = position - Vector2I::new(via.width(layer) / 2, 0);
                    self.check_constraint(
                        host,
                        ctx,
                        &constraint,
                        item,
                        via.padstack().mode(),
                        pt_a,
                        pt_b,
                        layer,
                    );
                });
            }
            Checked::Pad(pad) => {
                if !pad.has_hole(ctx) || pad.attribute() != PadAttrib::Pth {
                    return true;
                }

                let item = ctx.view(AnyItem::Pad(pad));
                let mut same_num_pads: Vec<&Pad> = Vec::new();

                if let Some(fp) = item.parent_footprint().and_then(|f| f.as_footprint()) {
                    same_num_pads = fp
                        .pads_by_number(ctx, pad.number(), item.id().and_then(|id| id.typed()))
                        .into_iter()
                        .filter_map(|id| ctx.board.pad(id))
                        .collect();
                }

                pad.lib_padstack().for_each_unique_layer(|layer| {
                    let constraint = engine.eval_rules(
                        ctx,
                        DrcConstraintT::AnnularWidthConstraint,
                        Some(item),
                        None,
                        layer,
                    );

                    let mut pt_a = Vector2I::default();
                    let mut pt_b = Vector2I::default();
                    get_pad_annulus_pts(
                        ctx,
                        pad,
                        layer,
                        &constraint,
                        &same_num_pads,
                        &mut pt_a,
                        &mut pt_b,
                    );
                    self.check_constraint(
                        host,
                        ctx,
                        &constraint,
                        item,
                        pad.lib_padstack().mode(),
                        pt_a,
                        pt_b,
                        layer,
                    );
                });
            }
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderAnnularWidth {
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
