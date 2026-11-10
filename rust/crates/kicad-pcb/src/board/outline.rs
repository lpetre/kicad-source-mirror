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
use kicad_geom::geometry::approximation::ErrorLoc;

use crate::board::Board;
pub use crate::convert_shape_list_to_polygon::OutlineErrorHandler;
use crate::convert_shape_list_to_polygon::build_board_polygon_outlines;
use crate::ctx::BoardCtx;
use crate::padstack::PadAttrib;

impl Board {
    /// Port of `BOARD::GetBoardPolygonOutlines( aOutlines, aInferOutlineIfNecessary,
    /// aErrorHandler, aAllowUseArcsInPolygons, aIncludeNPTHAsOutlines )`: (outlines, success).
    ///
    /// The outlines are built with the board's chaining epsilon and `m_MaxError`; with
    /// `include_npth_as_outlines` the NPTH pad holes are subtracted. The C++ output parameter
    /// starts empty here (every C++ caller passes an empty set).
    pub fn get_board_polygon_outlines(
        &self,
        ctx: &BoardCtx<'_>,
        infer_outline_if_necessary: bool,
        error_handler: Option<&mut dyn OutlineErrorHandler>,
        allow_use_arcs_in_polygons: bool,
        include_npth_as_outlines: bool,
    ) -> (ShapePolySet, bool) {
        let mut outlines = ShapePolySet::new();

        // max dist from one endPt to next startPt: use the current value
        let chaining_epsilon = self.outlines_chaining_epsilon;

        let success = build_board_polygon_outlines(
            ctx,
            &mut outlines,
            self.design_settings().max_error(),
            chaining_epsilon,
            infer_outline_if_necessary,
            error_handler,
            allow_use_arcs_in_polygons,
        );

        // Now subtract NPTH oval holes from outlines if required
        if include_npth_as_outlines {
            for (fp, _) in self.footprints() {
                for (_, pad) in self.footprint_pads(fp) {
                    if pad.attribute() != PadAttrib::Npth {
                        continue;
                    }

                    let mut hole = ShapePolySet::new();
                    pad.transform_hole_to_polygon(
                        ctx,
                        &mut hole,
                        0,
                        crate::board_item::max_error(pad, ctx),
                        ErrorLoc::Inside,
                    );

                    // can be not the case for malformed NPTH holes
                    if hole.outline_count() > 0 {
                        // Issue #20159: BooleanSubtract correctly clips holes extending past
                        // board edges (common with oval holes near irregular boards). O(n log n)
                        // per hole vs O(1) for AddHole, but only used for 3D viewer
                        // generation, not a hot path.
                        outlines.boolean_subtract(&hole);
                    }
                }
            }
        }

        // Make polygon strictly simple to avoid issues (especially in 3D viewer)
        if outlines.arc_count() > 0 && outlines.outline_count() > 1 {
            // Boolean ops keep arcs only within a single outline, so simplify each island alone
            let mut simplified = ShapePolySet::new();

            for ii in 0..outlines.outline_count() {
                let mut island = ShapePolySet::from_polygon(outlines.c_polygon(ii));
                island.simplify();
                simplified.append_set(&island);
            }

            outlines = simplified;
        } else {
            outlines.simplify();
        }

        (outlines, success)
    }

    /// `BoardOutline()->GetOutline()` as `BOARD::UpdateBoardOutline()` leaves it: the board
    /// polygon outlines (no inference), fractured when they were built successfully. Cached in
    /// `BoardCaches::board_outline` until the next time stamp bump (the C++ rebuilds it on
    /// every commit that touches Edge.Cuts; `m_boardOutlineGeneration` is the time stamp).
    pub fn board_outline(&self, ctx: &BoardCtx<'_>) -> Arc<ShapePolySet> {
        let cached = self.caches.board_outline.get_or_build(
            |_| true,
            || {
                // m_boardOutline->GetOutline().RemoveAllContours()
                let (mut outline, has_outline) =
                    self.get_board_polygon_outlines(ctx, false, None, false, false);

                if has_outline {
                    outline.fracture(true);
                }

                Arc::new(outline)
            },
        );

        Arc::clone(&cached)
    }
}
