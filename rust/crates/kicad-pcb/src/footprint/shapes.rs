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

//! Port of `FOOTPRINT::TransformPadsToPolySet` and `TransformFPShapesToPolySet` of
//! `pcbnew/footprint.cpp` (package M20).

use kicad_core::PcbLayerId;
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::approximation::ErrorLoc;

use crate::ctx::BoardCtx;
use crate::footprint::Footprint;

impl Footprint {
    /// `FOOTPRINT::TransformPadsToPolySet( aBuffer, aLayer, aClearance, aMaxError, aErrorLoc )`.
    pub fn transform_pads_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        max_error: i32,
        error_loc: ErrorLoc,
    ) {
        crate::port_pending!(
            "FOOTPRINT::TransformPadsToPolySet",
            ctx,
            buffer,
            layer,
            clearance,
            max_error,
            error_loc
        )
    }

    /// `FOOTPRINT::TransformFPShapesToPolySet( aBuffer, aLayer, aClearance, aError, aErrorLoc,
    /// aIncludeText, aIncludeShapes, aIncludePrivateItems )` (C++ defaults true, true, false).
    #[allow(clippy::too_many_arguments)]
    pub fn transform_fp_shapes_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        include_text: bool,
        include_shapes: bool,
        include_private_items: bool,
    ) {
        crate::port_pending!(
            "FOOTPRINT::TransformFPShapesToPolySet",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            include_text,
            include_shapes,
            include_private_items
        )
    }
}
