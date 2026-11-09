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

//! Port of `pcbnew/pcb_board_outline.cpp` and the board-outline functions of `board.cpp`
//! (`GetBoardPolygonOutlines`). Package M21; M0d wrote the signatures.

use std::sync::Arc;

use kicad_geom::ShapePolySet;

use crate::board::Board;
pub use crate::convert_shape_list_to_polygon::OutlineErrorHandler;
use crate::ctx::BoardCtx;

impl Board {
    /// `BOARD::GetBoardPolygonOutlines( aOutlines, aInferOutlineIfNecessary, aErrorHandler,
    /// aAllowUseArcsInPolygons, aIncludeNPTHAsOutlines )`: (outlines, success).
    pub fn get_board_polygon_outlines(
        &self,
        ctx: &BoardCtx<'_>,
        infer_outline_if_necessary: bool,
        error_handler: Option<&mut dyn OutlineErrorHandler>,
        allow_use_arcs_in_polygons: bool,
        include_npth_as_outlines: bool,
    ) -> (ShapePolySet, bool) {
        crate::port_pending!(
            "BOARD::GetBoardPolygonOutlines",
            ctx.board.time_stamp(),
            infer_outline_if_necessary,
            error_handler.is_some(),
            allow_use_arcs_in_polygons,
            include_npth_as_outlines
        )
    }

    /// `PCB_BOARD_OUTLINE` (cached in `BoardCaches::board_outline` per time stamp).
    pub fn board_outline(&self, ctx: &BoardCtx<'_>) -> Arc<ShapePolySet> {
        crate::port_pending!("PCB_BOARD_OUTLINE (board_outline)", ctx.board.time_stamp())
    }
}
