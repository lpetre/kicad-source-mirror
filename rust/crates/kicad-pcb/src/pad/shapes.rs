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

//! Port of the shape part of `pcbnew/pad.cpp`: `BuildEffectiveShapes`,
//! `BuildEffectivePolygon`, `GetEffectiveShape`, `TransformShapeToPolygon`,
//! `TransformHoleToPolygon`, mask shapes, `MergePrimitivesAsPolygon`, bounding box and hit
//! tests (package M15). Results come from `Pad::caches` (`board-model.md` 9).

use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, ShapeCompound, ShapePolySet, ShapeSegment, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::pad::Pad;
use crate::traits::Flashing;

impl Pad {
    /// `PAD::GetBoundingBox()`: the effective bounding box (from the shape cache).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PAD::GetBoundingBox", ctx)
    }

    /// `PAD::GetEffectiveShape( aLayer, aFlash, aUsage )`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PAD::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PAD::GetEffectiveHoleShape()`.
    pub fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        crate::port_pending!("PAD::GetEffectiveHoleShape", ctx, layer, usage)
    }

    /// `PAD::GetEffectivePolygon( aLayer, aErrorLoc )`.
    pub fn effective_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        error_loc: ErrorLoc,
    ) -> Arc<ShapePolySet> {
        crate::port_pending!("PAD::GetEffectivePolygon", ctx, layer, error_loc)
    }

    /// `PAD::GetBoundingRadius()`.
    pub fn bounding_radius(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PAD::GetBoundingRadius", ctx)
    }

    /// `PAD::TransformShapeToPolygon()`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PAD::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PAD::TransformHoleToPolygon()`: false if the pad has no hole.
    pub fn transform_hole_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) -> bool {
        crate::port_pending!(
            "PAD::TransformHoleToPolygon",
            ctx,
            buffer,
            clearance,
            error,
            error_loc
        )
    }

    /// `PAD::GetEffectiveMaskShape( aLayer, aExpansion )` (`getMaskShape`, cached in
    /// `caches.mask_shapes`).
    pub fn mask_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        expansion: i32,
    ) -> Arc<ShapeCompound> {
        crate::port_pending!("PAD::getMaskShape", ctx, layer, expansion)
    }

    /// `PAD::MergePrimitivesAsPolygon( aLayer, aMergedPolygon, aErrorLoc )`.
    pub fn merge_primitives_as_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        merged_polygon: &mut ShapePolySet,
        error_loc: ErrorLoc,
    ) {
        crate::port_pending!(
            "PAD::MergePrimitivesAsPolygon",
            ctx,
            layer,
            merged_polygon,
            error_loc
        )
    }

    /// `PAD::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PAD::HitTest", ctx, pos, accuracy)
    }

    /// `PAD::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PAD::HitTest (rect)", ctx, rect, contained, accuracy)
    }
}
