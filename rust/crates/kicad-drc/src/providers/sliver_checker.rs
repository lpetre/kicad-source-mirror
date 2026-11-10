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

//! Port of `pcbnew/drc/drc_test_provider_sliver_checker.cpp`.
//!
//! Checks for slivers in copper layers. Errors generated:
//! - `DRCE_COPPER_SLIVER`
//!
//! The C++ builds the copper union of each layer on the thread pool (`submit_loop`, one task
//! per layer) and then scans the layers sequentially in layer order; the port builds them
//! sequentially in the same layer order. Only the scan reports, so the report order is the
//! C++ one (layer order, outline order, vertex order).

use std::sync::Arc;

use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::EdaUnits;
use kicad_core::typeinfo::KicadT;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::cmath;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::trigo::deg2rad;
use kicad_pcb::drc_api::DRCE_COPPER_SLIVER;
use kicad_pcb::eda_shape::shapes::EdaShapeShapesExt;
use kicad_pcb::{BoardCtx, ItemRef};

use super::run_host::RunHost;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, all_basic_items, for_each_geometry_item, no_path};

/// Default of `ADVANCED_CFG::m_SliverWidthTolerance` (mm, `common/advanced_config.cpp`); the
/// port has no advanced configuration file.
pub const ADVANCED_CFG_SLIVER_WIDTH_TOLERANCE: f64 = 0.08;

/// Default of `ADVANCED_CFG::m_SliverMinimumLength` (mm).
pub const ADVANCED_CFG_SLIVER_MINIMUM_LENGTH: f64 = 0.0008;

/// Default of `ADVANCED_CFG::m_SliverAngleTolerance` (degrees).
pub const ADVANCED_CFG_SLIVER_ANGLE_TOLERANCE: f64 = 20.0;

/// Port of the static `addItemPolysWithEndings`: a `PCB_SHAPE` with its line endings, any other
/// item through `TransformShapeToPolygon`.
pub(crate) fn add_item_polys_with_endings(
    item: ItemRef<'_>,
    buffer: &mut ShapePolySet,
    layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
) {
    if item.type_() == KicadT::PcbShapeT {
        let shape = item.as_shape().expect("PCB_SHAPE_T is a PCB_SHAPE");
        shape.transform_with_line_endings_to_polygon(
            item.ctx, buffer, clearance, error, error_loc, false,
        );
    } else {
        item.transform_shape_to_polygon(buffer, layer, clearance, error, error_loc, false);
    }
}

/// Port of `DRC_TEST_PROVIDER_SLIVER_CHECKER`.
#[derive(Clone)]
pub struct DrcTestProviderSliverChecker {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderSliverChecker {
    fn default() -> Self {
        DrcTestProviderSliverChecker {
            base: DrcTestProviderBase::new("sliver checker"),
        }
    }
}

impl DrcTestProviderSliverChecker {
    /// Port of `DRC_TEST_PROVIDER_SLIVER_CHECKER::layerDesc`.
    fn layer_desc(ctx: &BoardCtx<'_>, layer: PcbLayerId) -> String {
        format!("({})", ctx.board.layer_name(layer))
    }

    /// The `build_layer_polys` lambda of `Run`: the union of the copper of `layer` (left
    /// incomplete when the run was cancelled, as the C++ task leaves it).
    fn build_layer_polys(
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
    ) -> ShapePolySet {
        let mut poly = ShapePolySet::new();

        if host.is_cancelled() {
            return poly;
        }

        for_each_geometry_item(
            ctx,
            all_basic_items(),
            &Lset::from_layers(&[layer]),
            |item| {
                if let Some(zone) = item.as_zone() {
                    if !zone.is_rule_area()
                        && let Some(zone_fill) = zone.get_fill(layer)
                    {
                        let fill = zone_fill.clone_drop_triangulation();
                        poly.append_set(&fill);
                    }

                    // (progress is reported on board zones only)
                } else {
                    add_item_polys_with_endings(
                        item,
                        &mut poly,
                        layer,
                        0,
                        ARC_LOW_DEF,
                        ErrorLoc::Inside,
                    );
                }

                if host.is_cancelled() {
                    return false;
                }

                true
            },
        );

        if host.is_cancelled() {
            return poly;
        }

        poly.simplify();

        poly
    }

    /// Port of `DRC_TEST_PROVIDER_SLIVER_CHECKER::Run`: false when the run was cancelled.
    pub fn run(&self, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_COPPER_SLIVER) {
            return true; // Continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Running sliver detection on copper layers...")
        {
            return false; // DRC cancelled
        }

        let width_tolerance = PCB_IU_SCALE.mm_to_iu(ADVANCED_CFG_SLIVER_WIDTH_TOLERANCE) as i64;
        let squared_width = width_tolerance * width_tolerance;

        let angle_tolerance = ADVANCED_CFG_SLIVER_ANGLE_TOLERANCE;
        let cosangle_tol = 2.0 * cmath::cos(deg2rad(angle_tolerance));
        let copper_layers = Lset::all_cu_mask_n(ctx.board.copper_layer_count()).seq();
        let layer_count = copper_layers.0.len();

        if host.is_cancelled() {
            return false; // DRC cancelled
        }

        // tp.submit_loop( 0, copperLayers.size(), build_layer_polys ), sequentially.
        let mut layer_polys: Vec<ShapePolySet> = Vec::with_capacity(layer_count);

        for &layer in &copper_layers.0 {
            layer_polys.push(Self::build_layer_polys(&mut host, ctx, layer));
        }

        for ii in 0..layer_count {
            let layer = copper_layers.0[ii];
            let poly = &layer_polys[ii];

            if host.is_error_limit_exceeded(DRCE_COPPER_SLIVER) {
                continue;
            }

            // Frequently, in filled areas, some points of the polygons are very near (dist is
            // only a few internal units, like 2 or 3 units.
            // We skip very small vertices: one cannot really compute a valid orientation of
            // such a vertex
            // So skip points near than min_len (in internal units).
            let min_len = PCB_IU_SCALE.mm_to_iu(ADVANCED_CFG_SLIVER_MINIMUM_LENGTH);

            for jj in 0..poly.outline_count() {
                let pts = poly.c_outline(jj).c_points();
                let pt_count = pts.len() as i32;
                let mut offset;

                let area = |p: Vector2I, q: Vector2I, r: Vector2I| -> i64 {
                    (q.y - p.y) as i64 * (r.x - q.x) as i64
                        - (q.x - p.x) as i64 * (r.y - q.y) as i64
                };

                let is_locally_inside = |a_a: i32, a_b: i32| -> bool {
                    let prev = (pt_count + a_a - 1) % pt_count;
                    let next = (a_a + 1) % pt_count;
                    let p = |i: i32| pts[i as usize];

                    if area(p(prev), p(a_a), p(next)) < 0 {
                        area(p(a_a), p(a_b), p(next)) >= 0 && area(p(a_a), p(prev), p(a_b)) >= 0
                    } else {
                        area(p(a_a), p(a_b), p(prev)) < 0 || area(p(a_a), p(next), p(a_b)) < 0
                    }
                };

                if pt_count <= 5 {
                    continue;
                }

                let mut kk = 0;

                // for( int kk = 0; kk < ptCount; kk += offset ): every `continue` below goes
                // through the increment.
                while kk < pt_count {
                    let prior_index = (pt_count + kk - 1) % pt_count;
                    let mut next_index = (kk + 1) % pt_count;
                    let mut pt = pts[kk as usize];
                    let pt_prior = pts[prior_index as usize];
                    let mut v_prior = pt_prior - pt;
                    let mut forward_offset = 1;

                    offset = 1;

                    while v_prior.x.abs() < min_len
                        && v_prior.y.abs() < min_len
                        && offset < pt_count
                    {
                        pt = pts[((kk + offset) % pt_count) as usize];
                        offset += 1;
                        v_prior = pt_prior - pt;
                    }

                    if offset >= pt_count {
                        break;
                    }

                    let mut pt_after = pts[next_index as usize];
                    let mut v_after = pt_after - pt;

                    while v_after.x.abs() < min_len
                        && v_after.y.abs() < min_len
                        && forward_offset < pt_count
                    {
                        next_index = (kk + forward_offset) % pt_count;
                        forward_offset += 1;
                        pt_after = pts[next_index as usize];
                        v_after = pt_after - pt;
                    }

                    if offset >= pt_count {
                        break;
                    }

                    // Negative dot product means that the angle is > 90 degrees
                    if v_prior.dot(v_after) <= 0 {
                        kk += offset;
                        continue;
                    }

                    if !is_locally_inside(prior_index, next_index) {
                        kk += offset;
                        continue;
                    }

                    let v_included = pt_after - pt_prior;
                    let arm1 = v_prior.squared_euclidean_norm() as f64;
                    let arm2 = v_after.squared_euclidean_norm() as f64;
                    let opp = v_included.squared_euclidean_norm() as f64;

                    let cos_ang = ((opp - arm1 - arm2) / (arm1.sqrt() * arm2.sqrt())).abs();

                    if cos_ang > cosangle_tol
                        && 2.0 - cos_ang > f32::EPSILON as f64
                        && opp > squared_width as f64
                    {
                        let mut drce =
                            item::create(DRCE_COPPER_SLIVER).expect("DRCE_COPPER_SLIVER");
                        drce.set_error_detail(&Self::layer_desc(ctx, layer));
                        self.base.report_violation(
                            &mut host,
                            Arc::new(drce),
                            pt,
                            layer.0,
                            &no_path,
                        );
                    }

                    kk += offset;
                }
            }
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderSliverChecker {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        _engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(run, ctx)
    }
}
