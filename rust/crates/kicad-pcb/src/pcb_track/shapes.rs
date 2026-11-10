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

//! Port of the via and hole shape part of `pcbnew/pcb_track.cpp` (`PCB_VIA::GetEffectiveShape`,
//! `GetEffectiveHoleShape`, `TransformShapeToPolygon` for vias). Package M17; M0a wrote the
//! signatures.

use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_core::layer_ids::is_solder_mask_layer;
use kicad_geom::convert_basic_shapes_to_polygon::transform_circle_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Seg, ShapeCircle, ShapePolySet, ShapeSegment};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::pcb_track::PcbVia;
use crate::traits::Flashing;

impl PcbVia {
    /// Port of `PCB_VIA::GetEffectiveShape( aLayer, aFlash, aUsage )`: a `SHAPE_CIRCLE` of the
    /// pad (the largest non-ghost pad for `UNDEFINED_LAYER`, plus twice the mask expansion on
    /// mask layers) where the via is flashed, of the drill where it is not; widened to the
    /// largest hole for physical / silk clearance on backdrilled or post-machined layers.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        let mut diameter = 0;

        if flash == Flashing::AlwaysFlashed
            || (flash == Flashing::Default && self.flash_layer(ctx, layer))
        {
            if layer == PcbLayerId::UndefinedLayer {
                self.padstack.for_each_unique_layer(|l| {
                    if self.is_ghost_layer(ctx, l) {
                        return;
                    }

                    diameter = diameter.max(self.width(l));
                });
            } else {
                let cu_layer = self.padstack.effective_layer_for(layer);
                diameter = self.width(cu_layer);

                if is_solder_mask_layer(layer.0) {
                    diameter += 2 * self.solder_mask_expansion(ctx);
                }
            }
        } else {
            diameter = self.drill_value(ctx);
        }

        // In some cases we want to add in any backdrill or post-machining
        if (usage == DrcConstraintT::PhysicalClearanceConstraint
            && self.is_backdrilled_or_post_machined(ctx, layer))
            || (usage == DrcConstraintT::SilkClearanceConstraint
                && self.is_backdrilled_or_post_machined(ctx, layer))
        {
            diameter = diameter.max(self.padstack.max_hole_size());
        }

        Arc::new(ShapeCircle::new(self.track.start, diameter / 2))
    }

    /// Port of `PCB_VIA::GetEffectiveHoleShape( aLayer, aUsage )`: a zero-length
    /// `SHAPE_SEGMENT` of the drill (netclass fallback for an unset drill), widened to the
    /// largest hole for hole-to-hole and, on backdrilled / post-machined layers, for hole
    /// clearance, annular width, physical and silk clearance.
    pub fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        // An unset padstack drill falls back to the netclass value
        let mut hole_size = self.drill_value(ctx);

        // See if we want to include a larger backdrill or post-machining hole
        if usage == DrcConstraintT::HoleToHoleConstraint
            || (usage == DrcConstraintT::HoleClearanceConstraint
                && self.is_backdrilled_or_post_machined(ctx, layer))
            || (usage == DrcConstraintT::AnnularWidthConstraint
                && self.is_backdrilled_or_post_machined(ctx, layer))
            || (usage == DrcConstraintT::PhysicalClearanceConstraint
                && self.is_backdrilled_or_post_machined(ctx, layer))
            || (usage == DrcConstraintT::SilkClearanceConstraint
                && self.is_backdrilled_or_post_machined(ctx, layer))
        {
            hole_size = hole_size.max(self.padstack.max_hole_size());
        }

        Some(Arc::new(ShapeSegment::from_seg(
            Seg::new(self.track.start, self.track.start),
            hole_size,
        )))
    }

    /// Port of `PCB_TRACK::TransformShapeToPolygon (PCB_VIA)`: `TransformCircleToPolygon` of
    /// the pad on `layer` (`GetWidth( aLayer )`) grown by the clearance.
    #[allow(clippy::too_many_arguments)]
    pub fn transform_shape_to_polygon(
        &self,
        _ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        debug_assert!(
            !ignore_line_width,
            "IgnoreLineWidth has no meaning for tracks."
        );

        let radius = (self.width(layer) / 2) + clearance;
        transform_circle_to_polygon(buffer, self.track.start, radius, error, error_loc, 0);
    }
}
