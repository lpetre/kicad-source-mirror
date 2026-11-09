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

//! Port of the geometry part of `pcbnew/footprint.cpp`: bounding boxes (cached per board
//! time stamp in `geometry_cache`), hull, courtyard caches, `GetEffectiveShape` and hit
//! tests (package M20).

use kicad_core::{Lset, PcbLayerId};
use kicad_geom::{Box2I, ShapePolySet, Vector2I};

use crate::convert_shape_list_to_polygon::OutlineErrorHandler;
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::footprint::Footprint;
use crate::item::SharedShape;
use crate::traits::Flashing;

/// The malformed-courtyard flags of `BuildCourtyardCaches` (C++ item flags
/// `MALFORMED_F_COURTYARD` / `MALFORMED_B_COURTYARD`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CourtyardFlags {
    pub malformed_front: bool,
    pub malformed_back: bool,
}

impl Footprint {
    /// `FOOTPRINT::GetBoundingBox()` (texts included).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("FOOTPRINT::GetBoundingBox", ctx)
    }

    /// `FOOTPRINT::GetBoundingBox( aIncludeText )`.
    pub fn bounding_box_with_text(&self, ctx: &BoardCtx<'_>, include_text: bool) -> Box2I {
        crate::port_pending!(
            "FOOTPRINT::GetBoundingBox (aIncludeText)",
            ctx,
            include_text
        )
    }

    /// `FOOTPRINT::GetCenter()`: `GetBoundingBox( false ).GetCenter()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.bounding_box_with_text(ctx, false).centre()
    }

    /// `FOOTPRINT::GetLayerBoundingBox( aLayers )`.
    pub fn layer_bounding_box(&self, ctx: &BoardCtx<'_>, layers: &Lset) -> Box2I {
        crate::port_pending!("FOOTPRINT::GetLayerBoundingBox", ctx, layers)
    }

    /// `FOOTPRINT::GetBoundingHull()`.
    pub fn bounding_hull(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        crate::port_pending!("FOOTPRINT::GetBoundingHull", ctx)
    }

    /// `FOOTPRINT::GetBoundingHull( aLayer )`.
    pub fn bounding_hull_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> ShapePolySet {
        crate::port_pending!("FOOTPRINT::GetBoundingHull (layer)", ctx, layer)
    }

    /// `FOOTPRINT::GetEffectiveShape( aLayer, aFlash, aUsage )`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("FOOTPRINT::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `FOOTPRINT::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("FOOTPRINT::HitTest", ctx, pos, accuracy)
    }

    /// `FOOTPRINT::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("FOOTPRINT::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `FOOTPRINT::BuildCourtyardCaches( aErrorHandler )`: build the front / back courtyard
    /// polygons into `courtyard_cache` (a class-1 cache, so `&self`).
    pub fn build_courtyard_caches(
        &self,
        ctx: &BoardCtx<'_>,
        handler: Option<&mut dyn OutlineErrorHandler>,
    ) {
        crate::port_pending!("FOOTPRINT::BuildCourtyardCaches", ctx, handler.is_some())
    }

    /// `FOOTPRINT::GetCourtyard( aLayer )`: the cached courtyard of F_CrtYd / B_CrtYd,
    /// rebuilt when the stored hashes are stale.
    pub fn courtyard(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> ShapePolySet {
        crate::port_pending!("FOOTPRINT::GetCourtyard", ctx, layer)
    }

    /// The MALFORMED_F/B_COURTYARD flags of the courtyard cache (C++ `GetFlags()`).
    pub fn courtyard_flags(&self, ctx: &BoardCtx<'_>) -> CourtyardFlags {
        crate::port_pending!("FOOTPRINT::GetFlags (MALFORMED_*_COURTYARD)", ctx)
    }
}
