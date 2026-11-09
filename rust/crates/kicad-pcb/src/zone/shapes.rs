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

//! Port of the shape part of `pcbnew/zone.cpp`: bounding box, `GetEffectiveShape`,
//! `TransformShapeToPolygon`, `TransformSmoothedOutlineToPolygon`, hit tests (package M18).

use kicad_core::PcbLayerId;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, ShapePolySet, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::traits::Flashing;
use crate::zone::Zone;

impl Zone {
    /// `ZONE::GetBoundingBox()`: cached per board time stamp (`bbox_cache`).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("ZONE::GetBoundingBox", ctx)
    }

    /// `ZONE::GetEffectiveShape( aLayer, aFlash, aUsage )`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("ZONE::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `ZONE::TransformShapeToPolygon()`: the filled area of `layer`.
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
            "ZONE::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `ZONE::TransformSmoothedOutlineToPolygon( aBuffer, aClearance, aError, aErrorLoc,
    /// aBoardOutline )`.
    pub fn transform_smoothed_outline_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        board_outline: Option<&ShapePolySet>,
    ) {
        crate::port_pending!(
            "ZONE::TransformSmoothedOutlineToPolygon",
            ctx,
            buffer,
            clearance,
            error,
            error_loc,
            board_outline
        )
    }

    /// `ZONE::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("ZONE::HitTest", ctx, pos, accuracy)
    }

    /// `ZONE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("ZONE::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `ZONE::HitTestFilledArea( aLayer, aRefPos, aAccuracy )`.
    pub fn hit_test_filled_area(&self, layer: PcbLayerId, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("ZONE::HitTestFilledArea", layer, pos, accuracy)
    }
}
