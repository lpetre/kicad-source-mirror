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
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{ShapePolySet, ShapeSegment};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::pcb_track::PcbVia;
use crate::traits::Flashing;

impl PcbVia {
    /// `PCB_VIA::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_VIA::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_VIA::GetEffectiveHoleShape`.
    pub fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        crate::port_pending!("PCB_VIA::GetEffectiveHoleShape", ctx, layer, usage)
    }

    /// `PCB_TRACK::TransformShapeToPolygon (PCB_VIA)`.
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
            "PCB_TRACK::TransformShapeToPolygon (PCB_VIA)",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }
}
