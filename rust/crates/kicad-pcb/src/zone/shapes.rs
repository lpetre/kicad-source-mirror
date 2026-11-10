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
//! `TransformShapeToPolygon`, `TransformSolidAreasShapesToPolygon`,
//! `TransformSmoothedOutlineToPolygon`, `BuildSmoothedPoly`, hit tests.

use std::sync::Arc;

use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::shape_poly_set::VertexIndex;
use kicad_geom::{Box2I, Shape, ShapeLineChain, ShapeNull, ShapePolySet, Vector2I};

use crate::board_item;
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::traits::Flashing;
use crate::zone::Zone;
use crate::zone_settings::CornerSmoothing;

impl Zone {
    /// Port of `ZONE::computeBoundingBox()`: the board-frame outline's box for a footprint
    /// zone, else the stored outline's.
    pub(crate) fn compute_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        if self.parent_transform(ctx).is_some() {
            return self.board_outline(ctx).bbox(0);
        }

        self.lib_outline.bbox(0)
    }

    /// Port of `ZONE::GetBoundingBox()`: cached per board time stamp (`bbox_cache`, the
    /// C++ lock-free copy and `BOARD::m_ZoneBBoxCache` in one); computed every time without
    /// a board.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        if board_item::has_board(self, ctx) {
            let stamp = ctx.board.time_stamp();
            let cached = self.bbox_cache.get_or_build(
                |(_, cached_stamp)| *cached_stamp == stamp,
                || (self.compute_bounding_box(ctx), stamp),
            );

            return cached.0;
        }

        self.compute_bounding_box(ctx)
    }

    /// Port of `ZONE::GetEffectiveShape( aLayer, aFlash, aUsage )`: the outline (board
    /// frame) for a rule area, else the layer's fill (shared, not copied), `SHAPE_NULL`
    /// without one.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        let _ = (flash, usage);

        // Rule areas are never filled, so fall back to the outline.  DRC relies on this to collide tracks,
        // vias and pads against keepout areas.
        if self.is_rule_area() {
            // GetBoardOutline() is expensive.  Don't use it in DRC if we don't have to.
            if self.parent_transform(ctx).is_some() {
                return Arc::new(self.board_outline(ctx));
            }

            return Arc::new(self.lib_outline.clone());
        }

        match self.fill.filled_polys_list.get(&layer) {
            None => Arc::new(ShapeNull),
            Some(poly) => Arc::clone(poly) as SharedShape,
        }
    }

    /// Port of `ZONE::TransformShapeToPolygon()`: the filled area of `layer`, inflated (with
    /// linked holes) by `clearance` (+ `error` for `ErrorLoc::Outside`).
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
        let _ = ctx;
        // wxASSERT_MSG: IgnoreLineWidth has no meaning for zones.
        debug_assert!(
            !ignore_line_width,
            "IgnoreLineWidth has no meaning for zones."
        );

        let Some(fill_polys) = self.fill.filled_polys_list.get(&layer) else {
            return;
        };

        let mut clearance = clearance;

        if clearance == 0 {
            buffer.append_set(fill_polys);
            return;
        }

        let mut temp_buf = fill_polys.clone_drop_triangulation();

        // Rebuild filled areas only if clearance is not 0
        if clearance > 0 || error_loc == ErrorLoc::Outside {
            if error_loc == ErrorLoc::Outside {
                clearance += error;
            }

            temp_buf.inflate_with_linked_holes(clearance, CornerStrategy::RoundAllCorners, error);
        }

        buffer.append_set(&temp_buf);
    }

    /// Port of `ZONE::TransformSolidAreasShapesToPolygon( aLayer, aBuffer )`: appends the
    /// layer's fill when it is not empty.
    pub fn transform_solid_areas_shapes_to_polygon(
        &self,
        layer: PcbLayerId,
        buffer: &mut ShapePolySet,
    ) {
        if let Some(poly) = self.fill.filled_polys_list.get(&layer) {
            if !poly.is_empty() {
                buffer.append_set(poly);
            }
        }
    }

    /// Port of `ZONE::TransformSmoothedOutlineToPolygon( aBuffer, aLayer, aClearance,
    /// aMaxError, aErrorLoc, aBoardOutline )`: the smoothed outline (`BuildSmoothedPoly`),
    /// inflated by `clearance` (+ the board max error for `ErrorLoc::Outside`; the inflation
    /// uses the board max error, not `error`, as in C++), fractured, appended to `buffer`.
    ///
    /// The C++ signature has the layer; the M0 stub omitted it, it is added here.
    #[allow(clippy::too_many_arguments)]
    pub fn transform_smoothed_outline_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        board_outline: Option<&ShapePolySet>,
    ) {
        let _ = error;

        // Creates the zone outline polygon (with holes if any)
        let mut polybuffer = ShapePolySet::new();

        self.build_smoothed_poly(ctx, &mut polybuffer, layer, board_outline, None);

        // Calculate the polygon with clearance
        // holes are linked to the main outline, so only one polygon is created.
        let max_error = board_item::max_error(self, ctx);
        let mut clearance = clearance;

        if clearance != 0 {
            if error_loc == ErrorLoc::Outside {
                clearance += max_error;
            }

            polybuffer.inflate(clearance, CornerStrategy::RoundAllCorners, max_error, false);
        }

        polybuffer.fracture(true);
        buffer.append_set(&polybuffer);
    }

    /// Port of `ZONE::BuildSmoothedPoly( aSmoothedPoly, aLayer, aBoardOutline,
    /// aSmoothedPolyWithApron )`: false if the outline is degenerate (two corners or fewer).
    ///
    /// The board-frame outline with arcs flattened; rule areas are returned as they are.
    /// Same-net zones colliding on `layer` (unless enclosed by higher-priority zones of
    /// another net, #12797 / #13915) are added before the corner smoothing so that no divots
    /// appear at the junction (#2752), and the result is clipped back to the zone (or, with
    /// `ZoneKeepExternalFillets`, to the zone plus its external fillets).
    pub fn build_smoothed_poly(
        &self,
        ctx: &BoardCtx<'_>,
        smoothed_poly: &mut ShapePolySet,
        layer: PcbLayerId,
        board_outline: Option<&ShapePolySet>,
        smoothed_poly_with_apron: Option<&mut ShapePolySet>,
    ) -> bool {
        if self.num_corners() <= 2 {
            // malformed zone. polygon calculations will not like it ...
            return false;
        }

        // Processing of arc shapes in zones is not yet supported because Clipper can't do boolean
        // operations on them.  The poly outline must be converted to segments first.
        let mut flattened = self.board_outline(ctx);
        flattened.clear_arcs();

        if self.is_rule_area() {
            // We like keepouts just the way they are....
            *smoothed_poly = flattened;
            return true;
        }

        let has_board = board_item::has_board(self, ctx);
        let mut keep_external_fillets = false;

        let mut smooth_requested = self.corner_smoothing_type == CornerSmoothing::Chamfer
            || self.corner_smoothing_type == CornerSmoothing::Fillet;

        if self.is_teardrop_area() {
            // We use teardrop shapes with no smoothing; these shapes are already optimized
            smooth_requested = false;
        }

        if has_board {
            keep_external_fillets = ctx.board.design_settings().zone_keep_external_fillets;
        }

        let max_error = board_item::max_error(self, ctx);

        let smooth = |poly: &mut ShapePolySet| {
            if !smooth_requested {
                return;
            }

            match self.corner_smoothing_type {
                CornerSmoothing::Chamfer => {
                    *poly = poly.chamfer(self.corner_radius as i32);
                }
                CornerSmoothing::Fillet => {
                    *poly = poly.fillet(self.corner_radius as i32, max_error);
                }
                _ => {}
            }
        };

        let mut with_fillets = ShapePolySet::new();
        let mut use_with_fillets = false;

        *smoothed_poly = flattened.clone();

        // Should external fillets (that is, those applied to concave corners) be kept?  While it
        // seems safer to never have copper extend outside the zone outline, 5.1.x and prior did
        // indeed fill them so we leave the mode available.
        if keep_external_fillets && smooth_requested {
            with_fillets = flattened.clone();
            smooth(&mut with_fillets);
            with_fillets.boolean_add(&flattened);
            use_with_fillets = true;
        }

        let max_extents: &ShapePolySet = if use_with_fillets {
            &with_fillets
        } else {
            &flattened
        };

        // We now add in the areas of any same-net, intersecting zones.  This keeps us from smoothing
        // corners at an intersection (which often produces undesired divots between the intersecting
        // zones -- see #2752).
        //
        // After smoothing, we'll subtract back out everything outside of our zone.
        let (same_net_colliding_zones, diff_net_intersecting_zones) =
            self.interacting_zones(ctx, layer);

        for (_, same_net_zone) in &same_net_colliding_zones {
            let same_net_bounding_box = same_net_zone.bounding_box(ctx);

            // Note: a two-pass algorithm could use sameNetZone's actual fill instead of its outline.
            // This would obviate the need for the below wrinkles, in addition to fixing both issues
            // in #16095.
            // (And we wouldn't need to collect all the diffNetIntersectingZones either.)

            let mut same_net_poly = same_net_zone.board_outline(ctx);
            same_net_poly.clear_arcs();

            let mut diff_net_poly = ShapePolySet::new();

            // Of course there's always a wrinkle.  The same-net intersecting zone *might* get knocked
            // out along the border by a higher-priority, different-net zone.  #12797
            for (_, diff_net_zone) in &diff_net_intersecting_zones {
                if diff_net_zone.higher_priority(same_net_zone)
                    && diff_net_zone
                        .bounding_box(ctx)
                        .intersects(&same_net_bounding_box)
                {
                    let mut diff_net_outline = diff_net_zone.board_outline(ctx);
                    diff_net_outline.clear_arcs();

                    diff_net_poly.boolean_add(&diff_net_outline);
                }
            }

            // Second wrinkle.  After unioning the higher priority, different net zones together, we
            // need to check to see if they completely enclose our zone.  If they do, then we need to
            // treat the enclosed zone as isolated, not connected to the outer zone.  #13915
            let mut isolated = false;

            if diff_net_poly.outline_count() != 0 {
                let mut this_poly = self.board_outline(ctx);
                this_poly.clear_arcs();

                this_poly.boolean_subtract(&diff_net_poly);
                isolated = this_poly.outline_count() == 0;
            }

            if !isolated {
                smoothed_poly.boolean_add(&same_net_poly);
            }
        }

        if let Some(board_outline) = board_outline {
            let mut board_outline = board_outline.clone_drop_triangulation();
            board_outline.clear_arcs();

            smoothed_poly.boolean_intersection(&board_outline);
        }

        let with_same_net_intersecting_zones = smoothed_poly.clone_drop_triangulation();

        smooth(smoothed_poly);

        if let Some(apron) = smoothed_poly_with_apron {
            // The same-net intersecting-zone code above makes sure the corner-smoothing algorithm
            // doesn't produce divots.  But the min-thickness algorithm applied in fillCopperZone()
            // is *also* going to perform a deflate/inflate cycle, again leading to divots.  So we
            // pre-inflate the contour by the min-thickness within the same-net-intersecting-zones
            // envelope.
            let mut poly = max_extents.clone_drop_triangulation();
            poly.inflate(
                self.zone_min_thickness,
                CornerStrategy::RoundAllCorners,
                max_error,
                false,
            );

            if !keep_external_fillets {
                poly.boolean_intersection(&with_same_net_intersecting_zones);
            }

            *apron = smoothed_poly.clone();
            apron.boolean_intersection(&poly);
        }

        smoothed_poly.boolean_intersection(max_extents);

        true
    }

    /// Port of `ZONE::HitTest( aPosition, aAccuracy )`: a corner within twice the accuracy or
    /// an edge within the accuracy (at least 0.1 mm).
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        // When looking for an "exact" hit aAccuracy will be 0 which works poorly for very thin
        // lines.  Give it a floor.
        let accuracy = accuracy.max(PCB_IU_SCALE.mm_to_iu(0.1));

        self.hit_test_for_corner(ctx, pos, accuracy * 2).is_some()
            || self.hit_test_for_edge(ctx, pos, accuracy).is_some()
    }

    /// Port of `ZONE::HitTestForCorner( refPos, aAccuracy, aCornerHit )`: the index of the
    /// corner hit (stored outline, tested in its own frame).
    pub fn hit_test_for_corner(
        &self,
        ctx: &BoardCtx<'_>,
        ref_pos: Vector2I,
        accuracy: i32,
    ) -> Option<VertexIndex> {
        let lib_pos = self.lib_pos(ctx, ref_pos);
        let mut corner_hit = VertexIndex::default();

        self.lib_outline
            .collide_vertex(lib_pos, Some(&mut corner_hit), accuracy)
            .then_some(corner_hit)
    }

    /// Port of `ZONE::HitTestForEdge( refPos, aAccuracy, aCornerHit )`: the index of the
    /// first corner of the edge hit.
    pub fn hit_test_for_edge(
        &self,
        ctx: &BoardCtx<'_>,
        ref_pos: Vector2I,
        accuracy: i32,
    ) -> Option<VertexIndex> {
        let lib_pos = self.lib_pos(ctx, ref_pos);
        let mut corner_hit = VertexIndex::default();

        self.lib_outline
            .collide_edge(lib_pos, Some(&mut corner_hit), accuracy)
            .then_some(corner_hit)
    }

    /// A board-frame position in the frame of the stored outline.
    fn lib_pos(&self, ctx: &BoardCtx<'_>, pos: Vector2I) -> Vector2I {
        match self.parent_transform(ctx) {
            Some(xform) => xform.inverse_apply(pos),
            None => pos,
        }
    }

    /// Port of `ZONE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        // Calculate bounding box for zone
        let mut bbox = self.bounding_box(ctx);
        bbox.normalize();

        let mut arect = *rect;
        arect.normalize();
        arect.inflate(accuracy);

        if contained {
            return arect.contains_box(&bbox);
        }

        // Fast test: if aBox is outside the polygon bounding box, rectangles cannot intersect
        if !arect.intersects(&bbox) {
            return false;
        }

        let board_outline = self.board_outline(ctx);
        let count = board_outline.total_vertices();

        for ii in 0..count {
            let vertex = board_outline.c_vertex_global(ii);
            let vertex_next = board_outline.c_vertex_global((ii + 1) % count);

            // Test if the point is within the rect
            if arect.contains(vertex) {
                return true;
            }

            // Test if this edge intersects the rect
            if arect.intersects_segment(vertex, vertex_next) {
                return true;
            }
        }

        false
    }

    /// Port of `ZONE::HitTest( const SHAPE_LINE_CHAIN& aPoly, bool aContained )`.
    pub fn hit_test_poly(
        &self,
        ctx: &BoardCtx<'_>,
        poly: &ShapeLineChain,
        contained: bool,
    ) -> bool {
        let outline = self.board_outline(ctx);

        if contained {
            let outline_intersecting_selection = || {
                outline
                    .c_iterate_segments()
                    .any(|segment| poly.intersects_seg(&segment))
            };

            // In the case of contained selection, all vertices of the zone outline must be inside
            // the selection polygon, so we can check only the first vertex.
            let vertex_inside_selection =
                || poly.point_inside(outline.c_vertex_global(0), 0, false);

            return vertex_inside_selection() && !outline_intersecting_selection();
        }

        // Touching selection - check if any segment of the zone contours collides with the
        // selection shape.
        for segment in outline.c_iterate_segments_with_holes() {
            if poly.point_inside(segment.a, 0, false) {
                return true;
            }

            if poly.intersects_seg(&segment) {
                return true;
            }

            // Note: aPoly.Collide() could be used instead of two test above, but it is 3x slower.
        }

        false
    }

    /// Port of `ZONE::HitTestFilledArea( aLayer, aRefPos, aAccuracy )`: rule areas test their
    /// outline (as if filled), other zones the layer's fill.
    ///
    /// The M0 stub had no context; a rule area in a footprint needs the parent transform.
    pub fn hit_test_filled_area(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        pos: Vector2I,
        accuracy: i32,
    ) -> bool {
        // Rule areas have no filled area, but it's generally nice to treat their interior as if it were
        // filled so that people don't have to select them by their outline (which is min-width)
        if self.is_rule_area() {
            let lib_pos = self.lib_pos(ctx, pos);
            return self.lib_outline.contains(lib_pos, -1, accuracy, false);
        }

        let Some(fill_polys) = self.fill.filled_polys_list.get(&layer) else {
            return false;
        };

        fill_polys.contains(pos, -1, accuracy, false)
    }

    /// Port of `ZONE::HitTestCutout( aRefPos, aOutlineIdx, aHoleIdx )`: (outline, hole) of
    /// the first hole containing the point.
    pub fn hit_test_cutout(&self, ctx: &BoardCtx<'_>, pos: Vector2I) -> Option<(i32, i32)> {
        let lib_pos = self.lib_pos(ctx, pos);

        // Iterate over each outline polygon in the zone and then iterate over
        // each hole it has to see if the point is in it.
        for i in 0..self.lib_outline.outline_count() {
            for j in 0..self.lib_outline.hole_count(i) {
                if self
                    .lib_outline
                    .c_hole(i, j)
                    .point_inside(lib_pos, 0, false)
                {
                    return Some((i, j));
                }
            }
        }

        None
    }
}
