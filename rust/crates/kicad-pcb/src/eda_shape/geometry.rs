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

//! Port of the geometry part of `common/eda_shape.cpp` and the inline members of
//! `include/eda_shape.h` (setters, arc / ellipse / rectangle / polygon / bezier maths, move /
//! rotate / flip / mirror / scale, the width-free part of the bounding box, `Compare`,
//! `operator==`, `Similarity`). Package M7.
//!
//! Everything here is an inherent method of the data struct [`EdaShape`]: these `EDA_SHAPE`
//! bodies read no virtual except `getMaxError()` (passed in as `max_error` by the host, which
//! reads its board) and the setters, whose `EDA_SHAPE` bodies are called directly (a `PCB_SHAPE`
//! host re-syncs its library frame after calling them, `board-model.md` 6.2a). The bodies that
//! read a width or another virtual are the generic functions in `mod.rs`.

use kicad_geom::bezier_curves::BezierPoly;
use kicad_geom::geometry::roundrect::Roundrect;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, ANGLE_360, Box2I, EdaAngle, EdaAngleT, FlipDirection,
    Seg, Shape, ShapeLineChain, ShapePolySet, ShapeRect, Vector2D, Vector2I, calc_arc_center,
    cmath, is_straight_arc, ki_round, mirror_point, rotate_point_around, test_segment_hit,
};

use super::{EdaShape, EdaShapeExt, FillT, ShapeT};
use crate::ctx::BoardCtx;
use crate::line_ending::LineEndingStyle;
use crate::traits::EdaShapeItem;
use kicad_core::LineStyle;
// M8: the line-ending branches of the hit tests.
use kicad_geom::ShapeCompound;
use kicad_geom::geometry::geometry_utils::shape_hit_test;
use kicad_geom::geometry::shape_utils::box_to_line_chain;

/// `alg::longest_common_subset` (`libs/core/include/core/kicad_algo.h`): the length of the
/// longest common contiguous run of the two sequences.
fn longest_common_subset(c1: &[Vector2I], c2: &[Vector2I]) -> usize {
    let c1_size = c1.len();
    let c2_size = c2.len();

    if c1_size == 0 || c2_size == 0 {
        return 0;
    }

    let mut table = vec![vec![0usize; c2_size + 1]; c1_size + 1];
    let mut longest = 0usize;

    for i in 1..=c1_size {
        for j in 1..=c2_size {
            if c1[i - 1] == c2[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
                longest = longest.max(table[i][j]);
            }
        }
    }

    longest
}

impl EdaShape {
    /// `ShowShape()` (untranslated texts).
    pub fn show_shape(&self, is_proxy: bool) -> &'static str {
        if is_proxy {
            match self.shape {
                ShapeT::Segment => "Thermal Spoke",
                ShapeT::Rectangle => "Number Box",
                _ => "??",
            }
        } else {
            match self.shape {
                ShapeT::Segment => "Line",
                ShapeT::Rectangle => "Rect",
                ShapeT::Arc => "Arc",
                ShapeT::Circle => "Circle",
                ShapeT::Bezier => "Bezier Curve",
                ShapeT::Poly => "Polygon",
                ShapeT::Ellipse => "Ellipse",
                ShapeT::EllipseArc => "Elliptical Arc",
                ShapeT::Undefined => "??",
            }
        }
    }

    // ---- inline setters of eda_shape.h ------------------------------------------------

    /// `SetStartY()`.
    pub fn set_start_y(&mut self, y: i32) {
        let mut s = self.start;
        s.y = y;
        self.set_start(s);
    }

    /// `SetStartX()`.
    pub fn set_start_x(&mut self, x: i32) {
        let mut s = self.start;
        s.x = x;
        self.set_start(s);
    }

    /// `SetCenterY()`.
    pub fn set_center_y(&mut self, y: i32) {
        let mut new_start = self.start;
        let mut new_end = self.end;
        new_end.y += y - self.start.y;
        new_start.y = y;
        self.set_start(new_start);
        self.set_end(new_end);
        self.set_hatching_dirty();
    }

    /// `SetCenterX()`.
    pub fn set_center_x(&mut self, x: i32) {
        let mut new_start = self.start;
        let mut new_end = self.end;
        new_end.x += x - self.start.x;
        new_start.x = x;
        self.set_start(new_start);
        self.set_end(new_end);
        self.set_hatching_dirty();
    }

    /// `SetEndY()`.
    pub fn set_end_y(&mut self, y: i32) {
        let mut e = self.end;
        e.y = y;
        self.set_end(e);
    }

    /// `SetEndX()`.
    pub fn set_end_x(&mut self, x: i32) {
        let mut e = self.end;
        e.x = x;
        self.set_end(e);
    }

    /// `SetRadius()`.
    pub fn set_radius(&mut self, x: i32) {
        self.set_end(self.start + Vector2I::new(x, 0));
        self.set_hatching_dirty();
    }

    /// `SetTop()` (the `EDA_SHAPE` body).
    pub fn set_top(&mut self, val: i32) {
        self.set_start_y(val);
    }

    /// `SetLeft()` (the `EDA_SHAPE` body).
    pub fn set_left(&mut self, val: i32) {
        self.set_start_x(val);
    }

    /// `SetRight()` (the `EDA_SHAPE` body).
    pub fn set_right(&mut self, val: i32) {
        self.set_end_x(val);
    }

    /// `SetBottom()` (the `EDA_SHAPE` body).
    pub fn set_bottom(&mut self, val: i32) {
        self.set_end_y(val);
    }

    /// `SetBezierC1()` (the `EDA_SHAPE` body).
    pub fn set_bezier_c1(&mut self, pt: Vector2I) {
        self.bezier_c1 = pt;
    }

    /// `SetBezierC2()` (the `EDA_SHAPE` body).
    pub fn set_bezier_c2(&mut self, pt: Vector2I) {
        self.bezier_c2 = pt;
    }

    /// `SetEllipseCenter()` (the `EDA_SHAPE` body).
    pub fn set_ellipse_center(&mut self, pt: Vector2I) {
        self.ellipse.center = pt;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetEllipseMajorRadius()` (the `EDA_SHAPE` body).
    pub fn set_ellipse_major_radius(&mut self, r: i32) {
        self.ellipse.major_radius = r;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetEllipseMinorRadius()` (the `EDA_SHAPE` body).
    pub fn set_ellipse_minor_radius(&mut self, r: i32) {
        self.ellipse.minor_radius = r;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetEllipseRotation()` (the `EDA_SHAPE` body).
    pub fn set_ellipse_rotation(&mut self, a: EdaAngle) {
        self.ellipse.rotation = a;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetEllipseStartAngle()` (the `EDA_SHAPE` body; meaningful for `ELLIPSE_ARC` only).
    pub fn set_ellipse_start_angle(&mut self, a: EdaAngle) {
        self.ellipse.start_angle = a;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetEllipseEndAngle()` (the `EDA_SHAPE` body).
    pub fn set_ellipse_end_angle(&mut self, a: EdaAngle) {
        self.ellipse.end_angle = a;
        self.set_hatching_dirty();
        self.recalc_ellipse_arc_endpoints();
    }

    /// `SetFilled()` / `setFilled()` (the `EDA_SHAPE` bodies).
    pub fn set_filled(&mut self, flag: bool) {
        self.fill = if flag {
            FillT::FilledShape
        } else {
            FillT::NoFill
        };
    }

    /// `SetLineStyle()`.
    pub fn set_line_style(&mut self, style: LineStyle) {
        self.stroke.set_line_style(style);
    }

    /// `GetLineStyle()`: `DEFAULT` reads as `SOLID`.
    pub fn line_style(&self) -> LineStyle {
        if self.stroke.line_style() != LineStyle::Default {
            return self.stroke.line_style();
        }

        LineStyle::Solid
    }

    /// `SetStartEnding()`.
    pub fn set_start_ending(&mut self, ending: crate::line_ending::LineEnding) {
        self.start_ending = ending;
    }

    /// `SetEndEnding()`.
    pub fn set_end_ending(&mut self, ending: crate::line_ending::LineEnding) {
        self.end_ending = ending;
    }

    // ---- rectangle --------------------------------------------------------------------

    /// `GetRectangleHeight()`: 0 (C++ `UNIMPLEMENTED_FOR`) for other shapes.
    pub fn rectangle_height(&self) -> i32 {
        match self.shape {
            ShapeT::Rectangle => self.end.y - self.start.y,
            _ => 0,
        }
    }

    /// `GetRectangleWidth()`: 0 (C++ `UNIMPLEMENTED_FOR`) for other shapes.
    pub fn rectangle_width(&self) -> i32 {
        match self.shape {
            ShapeT::Rectangle => self.end.x - self.start.x,
            _ => 0,
        }
    }

    /// `SetRectangleHeight()`.
    pub fn set_rectangle_height(&mut self, height: i32) {
        if self.shape == ShapeT::Rectangle {
            self.rectangle_height = height as i64;
            // SetEndY( GetStartY() + m_rectangleHeight ): long long sum narrowed to int
            self.set_end_y((self.start.y as i64 + self.rectangle_height) as i32);
        }
    }

    /// `SetRectangleWidth()`.
    pub fn set_rectangle_width(&mut self, width: i32) {
        if self.shape == ShapeT::Rectangle {
            self.rectangle_width = width as i64;
            self.set_end_x((self.start.x as i64 + self.rectangle_width) as i32);
        }
    }

    /// `SetRectangle( aHeight, aWidth )`: the `long long` arguments bind to the `const int&`
    /// parameters of the two setters through a narrowing temporary.
    pub fn set_rectangle(&mut self, height: i64, width: i64) {
        if self.shape == ShapeT::Rectangle {
            self.set_rectangle_height(height as i32);
            self.set_rectangle_width(width as i32);
        }
    }

    /// `GetRectCorners()`: start, (end.x, start.y), end, (start.x, end.y).
    pub fn rect_corners(&self) -> Vec<Vector2I> {
        let top_left = self.start;
        let bot_right = self.end;

        vec![
            top_left,
            Vector2I::new(bot_right.x, top_left.y),
            bot_right,
            Vector2I::new(top_left.x, bot_right.y),
        ]
    }

    /// `ROUNDRECT( SHAPE_RECT( GetStart(), GetRectangleWidth(), GetRectangleHeight() ),
    /// m_cornerRadius )` as used by rotate / mirror / hit tests.
    pub(crate) fn roundrect(&self) -> Roundrect {
        Roundrect::new(
            ShapeRect::from_pos_size(self.start, self.rectangle_width(), self.rectangle_height()),
            self.corner_radius,
            false,
        )
    }

    // ---- polygon ----------------------------------------------------------------------

    /// `GetPolyPoints()`: every outline's points, in order.
    pub fn poly_points(&self) -> Vec<Vector2I> {
        let poly = &self.poly;
        let mut points = Vec::new();

        for ii in 0..poly.outline_count() {
            points.extend_from_slice(poly.c_outline(ii).c_points());
        }

        points
    }

    /// `GetPointCount()`: the vertex count of the first polygon (with its holes).
    pub fn point_count(&self) -> i32 {
        if self.poly.outline_count() != 0 {
            self.poly.vertex_count(0, -1)
        } else {
            0
        }
    }

    /// `IsPolyShapeValid()`: more than 2 points in the first outline.
    pub fn is_poly_shape_valid(&self) -> bool {
        self.poly.outline_count() > 0 && self.poly.c_outline(0).point_count() > 2
    }

    /// `SetPolyPoints()`.
    pub fn set_poly_points(&mut self, points: &[Vector2I]) {
        self.poly.remove_all_contours();
        self.poly.new_outline();

        for p in points {
            self.poly.append(p.x, p.y, -1, -1, false);
        }

        self.set_hatching_dirty();
    }

    // ---- bezier -----------------------------------------------------------------------

    /// `RebuildBezierToSegmentsPointsList( aMaxError )`: has meaning only for `BEZIER`
    /// (other shapes get an empty list).
    pub fn rebuild_bezier_to_segments_points_list(&mut self, max_error: i32) {
        if self.shape != ShapeT::Bezier {
            self.bezier_points.clear();
            return;
        }

        self.bezier_points = self.build_bezier_to_segments_points_list(max_error);
    }

    /// `buildBezierToSegmentsPointsList( aMaxError )`.
    pub fn build_bezier_to_segments_points_list(&self, max_error: i32) -> Vec<Vector2I> {
        let ctrl_points = [self.start, self.bezier_c1, self.bezier_c2, self.end];
        let converter = BezierPoly::from_points_i(&ctrl_points);
        let mut bezier_points = Vec::new();
        converter.get_poly_i(&mut bezier_points, max_error);
        bezier_points
    }

    // ---- ellipse ----------------------------------------------------------------------

    /// `buildShapeEllipse()`.
    pub fn build_shape_ellipse(&self) -> ShapeEllipse {
        let e = &self.ellipse;

        if self.shape == ShapeT::EllipseArc {
            return ShapeEllipse::new_arc(
                e.center,
                e.major_radius,
                e.minor_radius,
                e.rotation,
                e.start_angle,
                e.end_angle,
            );
        }

        ShapeEllipse::new(e.center, e.major_radius, e.minor_radius, e.rotation)
    }

    /// `recalcEllipseArcEndpoints()`: recompute `m_start` / `m_end` (and the arc centre) from
    /// `m_ellipse`. (`m_editState` is not ported: always 0 here.)
    pub fn recalc_ellipse_arc_endpoints(&mut self) {
        if self.shape == ShapeT::Ellipse {
            let phi = self.ellipse.rotation.as_radians();
            self.start = self.ellipse.center;
            self.end = self.start
                + Vector2I::new(
                    ki_round(self.ellipse.major_radius as f64 * cmath::cos(phi)),
                    ki_round(self.ellipse.major_radius as f64 * cmath::sin(phi)),
                );
            return;
        }

        if self.shape != ShapeT::EllipseArc {
            return;
        }

        self.arc_center = self.ellipse.center;

        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;
        let phi = self.ellipse.rotation.as_radians();
        let cos_phi = cmath::cos(phi);
        let sin_phi = cmath::sin(phi);
        let c = self.ellipse.center;

        let eval = |theta: f64| -> Vector2I {
            let lx = a * cmath::cos(theta);
            let ly = b * cmath::sin(theta);
            c + Vector2I::new(
                ki_round(lx * cos_phi - ly * sin_phi),
                ki_round(lx * sin_phi + ly * cos_phi),
            )
        };

        self.start = eval(self.ellipse.start_angle.as_radians());
        self.end = eval(self.ellipse.end_angle.as_radians());
    }

    // ---- position, centre, arc maths ----------------------------------------------------

    /// `getPosition()`. An empty polygon (C++: `CVertex( 0 )` throws `std::out_of_range`)
    /// reads as (0, 0).
    pub fn position(&self) -> Vector2I {
        if matches!(
            self.shape,
            ShapeT::Arc | ShapeT::Ellipse | ShapeT::EllipseArc
        ) {
            self.center()
        } else if self.shape == ShapeT::Poly {
            if self.poly.total_vertices() == 0 {
                return Vector2I::default();
            }

            self.poly.c_vertex_global(0)
        } else {
            self.start
        }
    }

    /// `getCenter()`. For `POLY` / `RECTANGLE` / `BEZIER` the C++ takes the centre of
    /// `getBoundingBox()`, which is inflated symmetrically by `GetWidth() / 2`; the centre of
    /// the box does not depend on that inflation (see [`EdaShape::bounding_box_core`]), so no
    /// width (and no context) is needed.
    pub fn center(&self) -> Vector2I {
        match self.shape {
            ShapeT::Arc => self.arc_center,
            ShapeT::Circle => self.start,
            // Midpoint of the line
            ShapeT::Segment => (self.start + self.end) / 2,
            ShapeT::Poly | ShapeT::Rectangle | ShapeT::Bezier => {
                let mut bbox = self.bounding_box_core();
                bbox.normalize();
                bbox.centre()
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => self.ellipse.center,
            ShapeT::Undefined => Vector2I::default(),
        }
    }

    /// `setPosition()`: `move( aPos - getPosition() )`.
    pub fn set_position(&mut self, pos: Vector2I) {
        let delta = pos - self.position();
        self.move_(delta);
    }

    /// `GetArcMid()`: the cached mid point if start, end and centre are unchanged since it was
    /// stored (to minimise churn), else the computed half-angle point.
    pub fn arc_mid(&self) -> Vector2I {
        if self.arc_mid_data.start == self.start
            && self.arc_mid_data.end == self.end
            && self.arc_mid_data.center == self.arc_center
        {
            return self.arc_mid_data.mid;
        }

        let mut mid = self.start;
        rotate_point_around(&mut mid, self.arc_center, -self.arc_angle() / 2.0);
        mid
    }

    /// `CalcArcAngles( aStartAngle, aEndAngle )`: (start, end) with start < end, each between
    /// -360 and 360.
    pub fn calc_arc_angles(&self) -> (EdaAngle, EdaAngle) {
        let start_radial = Vector2D::from(self.start - self.center());
        let end_radial = Vector2D::from(self.end - self.center());

        let start_angle = EdaAngle::from_vector(start_radial);
        let mut end_angle = EdaAngle::from_vector(end_radial);

        if end_angle == start_angle {
            end_angle = start_angle + ANGLE_360; // ring, not null
        }

        while end_angle < start_angle {
            end_angle += ANGLE_360;
        }

        (start_angle, end_angle)
    }

    /// `GetRadius()`: at least 1, at most `INT_MAX / 2`; 1 for shapes other than arcs and
    /// circles (C++ `UNIMPLEMENTED_FOR`).
    pub fn radius(&self) -> i32 {
        let mut radius = match self.shape {
            ShapeT::Arc => self.arc_center.distance(self.start),
            ShapeT::Circle => self.start.distance(self.end),
            _ => 0.0,
        };

        // don't allow degenerate circles/arcs
        if radius > i32::MAX as f64 / 2.0 {
            radius = i32::MAX as f64 / 2.0;
        }

        1.max(ki_round(radius))
    }

    /// `SetCachedArcData( aStart, aMid, aEnd, aCenter )`.
    pub fn set_cached_arc_data(
        &mut self,
        start: Vector2I,
        mid: Vector2I,
        end: Vector2I,
        center: Vector2I,
    ) {
        self.arc_mid_data.start = start;
        self.arc_mid_data.end = end;
        self.arc_mid_data.center = center;
        self.arc_mid_data.mid = mid;
    }

    /// `SetArcOrSegment( aStart, aMid, aEnd )`: a `SEGMENT` from start to end when the three
    /// points are collinear (no finite arc centre). Calls the `EDA_SHAPE` setter bodies.
    pub fn set_arc_or_segment(&mut self, start: Vector2I, mid: Vector2I, end: Vector2I) {
        if is_straight_arc(start, mid, end) {
            self.set_shape(ShapeT::Segment);
            self.set_start(start);
            self.set_end(end);
        } else {
            self.set_arc_geometry(start, mid, end);
        }
    }

    /// `SetArcGeometry()` body (called by the inherent setter in `mod.rs`).
    pub(super) fn set_arc_geometry_impl(&mut self, start: Vector2I, mid: Vector2I, end: Vector2I) {
        self.arc_mid_data = Default::default();
        self.start = start;
        self.end = end;
        self.arc_center = calc_arc_center(start, mid, end);
        let new_mid = self.arc_mid();

        self.ends_swapped = false;

        // If the input winding doesn't match our internal winding, the calculated midpoint will
        // end up on the other side of the arc.  In this case, we need to flip the start/end
        // points and flag this change for the system.
        let dist = Vector2D::from(new_mid - mid);
        let dist2 = Vector2D::from(new_mid - self.arc_center);

        if dist.squared_euclidean_norm() > dist2.squared_euclidean_norm() {
            std::mem::swap(&mut self.start, &mut self.end);
            self.ends_swapped = true;
        }

        // Watch the ordering here.  GetArcMid above needs to be called prior to initializing
        // the m_arcMidData structure in order to ensure we get the calculated variant, not the
        // cached. The cache is keyed on the stored ends, so it must follow any swap or GetArcMid
        // never hits it
        self.set_cached_arc_data(self.start, mid, self.end, self.arc_center);
    }

    /// `GetSegmentAngle()`: the angle of start -> end, y up.
    pub fn segment_angle(&self) -> EdaAngle {
        EdaAngle::new(
            cmath::atan2(
                (self.start.y - self.end.y) as f64,
                (self.end.x - self.start.x) as f64,
            ),
            EdaAngleT::Radians,
        )
    }

    /// `GetArcAngle()`: end angle - start angle of `CalcArcAngles`.
    pub fn arc_angle(&self) -> EdaAngle {
        let (start_angle, end_angle) = self.calc_arc_angles();
        end_angle - start_angle
    }

    /// `IsClockwiseArc()`: false for shapes other than arcs (C++ `UNIMPLEMENTED_FOR`).
    pub fn is_clockwise_arc(&self) -> bool {
        if self.shape == ShapeT::Arc {
            let mid = Vector2D::from(self.arc_mid());

            let orient = (mid.x - self.start.x as f64) * (self.end.y - self.start.y) as f64
                - (mid.y - self.start.y as f64) * (self.end.x - self.start.x) as f64;

            return orient < 0.0;
        }

        false
    }

    /// `SetArcAngleAndEnd()` body (called by the inherent setter in `mod.rs`).
    pub(super) fn set_arc_angle_and_end_impl(&mut self, a_angle: EdaAngle, check_negative: bool) {
        let mut angle = a_angle;

        self.end = self.start;
        rotate_point_around(&mut self.end, self.arc_center, -angle.normalize720());

        if check_negative && a_angle < ANGLE_0 {
            std::mem::swap(&mut self.start, &mut self.end);
            self.ends_swapped = true;
        }
    }

    // ---- length -----------------------------------------------------------------------

    /// `GetLength()`: 0 for circles, rectangles and undefined shapes (C++
    /// `UNIMPLEMENTED_FOR`) and for a polygon without outline (C++: undefined `COutline( 0 )`).
    pub fn length(&self) -> f64 {
        let mut length = 0.0;

        match self.shape {
            ShapeT::Bezier => {
                for ii in 1..self.bezier_points.len() {
                    length += self.bezier_points[ii - 1].distance(self.bezier_points[ii]);
                }

                length
            }
            ShapeT::Segment => self.start.distance(self.end),
            ShapeT::Poly => {
                if self.poly.outline_count() == 0 {
                    return 0.0;
                }

                let outline = self.poly.c_outline(0);

                for ii in 0..outline.segment_count() {
                    length += outline.c_segment(ii).length() as f64;
                }

                length
            }
            ShapeT::Arc => self.radius() as f64 * self.arc_angle().as_radians(),
            ShapeT::Ellipse | ShapeT::EllipseArc => self.build_shape_ellipse().get_length(),
            _ => 0.0,
        }
    }

    // ---- transforms -------------------------------------------------------------------

    /// `move( aMoveVector )`. The C++ also translates an existing hatching cache before
    /// marking it dirty; here dirty means dropped, so only the invalidation is kept.
    pub fn move_(&mut self, delta: Vector2I) {
        match self.shape {
            ShapeT::Arc | ShapeT::Segment | ShapeT::Rectangle | ShapeT::Circle => {
                if self.shape == ShapeT::Arc {
                    self.arc_center += delta;
                    self.arc_mid_data.center += delta;
                    self.arc_mid_data.start += delta;
                    self.arc_mid_data.end += delta;
                    self.arc_mid_data.mid += delta;
                }

                self.start += delta;
                self.end += delta;
            }
            ShapeT::Poly => self.poly.move_(delta),
            ShapeT::Bezier => {
                self.start += delta;
                self.end += delta;
                self.bezier_c1 += delta;
                self.bezier_c2 += delta;

                for pt in &mut self.bezier_points {
                    *pt += delta;
                }
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                self.ellipse.center += delta;
                self.start += delta;
                self.end += delta;
            }
            ShapeT::Undefined => {}
        }

        self.set_hatching_dirty();
    }

    /// `scale( aScale )`; `max_error` is the host's `getMaxError()` (bezier rebuild).
    pub fn scale(&mut self, scale: f64, max_error: i32) {
        let scale_pt = |pt: &mut Vector2I| {
            pt.x = ki_round(pt.x as f64 * scale);
            pt.y = ki_round(pt.y as f64 * scale);
        };

        match self.shape {
            ShapeT::Arc | ShapeT::Segment | ShapeT::Rectangle | ShapeT::Circle => {
                if self.shape == ShapeT::Arc {
                    scale_pt(&mut self.arc_center);
                }

                scale_pt(&mut self.start);
                scale_pt(&mut self.end);
            }
            ShapeT::Poly => {
                let mut pts: Vec<Vector2I> = Vec::new();

                for ii in 0..self.poly.outline_count() {
                    for pt in self.poly.c_outline(ii).c_points() {
                        let mut p = *pt;
                        scale_pt(&mut p);
                        pts.push(p);
                    }
                }

                self.set_poly_points(&pts);
            }
            ShapeT::Bezier => {
                scale_pt(&mut self.start);
                scale_pt(&mut self.end);
                scale_pt(&mut self.bezier_c1);
                scale_pt(&mut self.bezier_c2);
                self.rebuild_bezier_to_segments_points_list(max_error);
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                scale_pt(&mut self.ellipse.center);
                self.ellipse.major_radius =
                    ki_round((self.ellipse.major_radius as f64 * scale).abs());
                self.ellipse.minor_radius =
                    ki_round((self.ellipse.minor_radius as f64 * scale).abs());
                self.recalc_ellipse_arc_endpoints();
            }
            ShapeT::Undefined => {}
        }

        self.set_hatching_dirty();
    }

    /// `rotate( aRotCentre, aAngle )`; `max_error` is the host's `getMaxError()` (a
    /// rectangle rotated by a non-cardinal angle becomes a polygon).
    pub fn rotate(&mut self, centre: Vector2I, angle: EdaAngle, max_error: i32) {
        match self.shape {
            ShapeT::Segment | ShapeT::Circle => {
                rotate_point_around(&mut self.start, centre, angle);
                rotate_point_around(&mut self.end, centre, angle);
            }
            ShapeT::Arc => {
                rotate_point_around(&mut self.start, centre, angle);
                rotate_point_around(&mut self.end, centre, angle);
                rotate_point_around(&mut self.arc_center, centre, angle);
                rotate_point_around(&mut self.arc_mid_data.start, centre, angle);
                rotate_point_around(&mut self.arc_mid_data.end, centre, angle);
                rotate_point_around(&mut self.arc_mid_data.mid, centre, angle);
                rotate_point_around(&mut self.arc_mid_data.center, centre, angle);
            }
            ShapeT::Rectangle => {
                if angle.is_cardinal() {
                    rotate_point_around(&mut self.start, centre, angle);
                    rotate_point_around(&mut self.end, centre, angle);
                } else {
                    // Convert non-cardinally-rotated rect to a polygon.
                    let rr = self.roundrect();
                    self.set_shape(ShapeT::Poly);
                    rr.transform_to_polygon(&mut self.poly, max_error);
                    self.poly.rotate(angle, centre);
                }
            }
            ShapeT::Poly => self.poly.rotate(angle, centre),
            ShapeT::Bezier => {
                rotate_point_around(&mut self.start, centre, angle);
                rotate_point_around(&mut self.end, centre, angle);
                rotate_point_around(&mut self.bezier_c1, centre, angle);
                rotate_point_around(&mut self.bezier_c2, centre, angle);

                for pt in &mut self.bezier_points {
                    rotate_point_around(pt, centre, angle);
                }
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                rotate_point_around(&mut self.ellipse.center, centre, angle);

                // Ellipse rotation is the CCW angle of the major axis in standard math
                // coordinates (Y-up).  RotatePoint uses KiCad's Y-down screen convention,
                // so a positive aAngle rotates visually CCW on screen but corresponds to
                // a negative rotation in the math frame.  Hence -= rather than +=.
                self.ellipse.rotation -= angle;
                self.recalc_ellipse_arc_endpoints();
            }
            ShapeT::Undefined => {}
        }

        self.set_hatching_dirty();
    }

    /// `flip( aCentre, aFlipDirection )`; `max_error` is the host's `getMaxError()` (bezier
    /// rebuild).
    pub fn flip(&mut self, centre: Vector2I, dir: FlipDirection, max_error: i32) {
        match self.shape {
            ShapeT::Segment | ShapeT::Rectangle | ShapeT::Circle => {
                mirror_point(&mut self.start, centre, dir);
                mirror_point(&mut self.end, centre, dir);
            }
            ShapeT::Arc => {
                mirror_point(&mut self.start, centre, dir);
                mirror_point(&mut self.end, centre, dir);
                mirror_point(&mut self.arc_center, centre, dir);

                std::mem::swap(&mut self.start, &mut self.end);
            }
            ShapeT::Poly => self.poly.mirror(centre, dir),
            ShapeT::Bezier => {
                mirror_point(&mut self.start, centre, dir);
                mirror_point(&mut self.end, centre, dir);
                mirror_point(&mut self.bezier_c1, centre, dir);
                mirror_point(&mut self.bezier_c2, centre, dir);

                self.rebuild_bezier_to_segments_points_list(max_error);
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                self.ellipse.mirror(centre, dir);
                self.recalc_ellipse_arc_endpoints();
            }
            ShapeT::Undefined => {}
        }

        self.set_hatching_dirty();
    }

    /// `mirror( const SEG& aAxis )`; `max_error` is the host's `getMaxError()`.
    pub fn mirror(&mut self, axis: &Seg, max_error: i32) {
        match self.shape {
            ShapeT::Segment | ShapeT::Circle => {
                self.start = axis.reflect_point(self.start);
                self.end = axis.reflect_point(self.end);
            }
            ShapeT::Rectangle => {
                if axis.a.x == axis.b.x || axis.a.y == axis.b.y {
                    // An axis-aligned rectangle stays a rectangle.
                    self.start = axis.reflect_point(self.start);
                    self.end = axis.reflect_point(self.end);
                } else {
                    // Convert non-cardinally-mirrored rect to a polygon.
                    let rr = self.roundrect();
                    self.set_shape(ShapeT::Poly);
                    rr.transform_to_polygon(&mut self.poly, max_error);
                    self.poly.mirror_seg(axis);
                }
            }
            ShapeT::Arc => {
                let s = axis.reflect_point(self.start);
                let m = axis.reflect_point(self.arc_mid());
                let e = axis.reflect_point(self.end);
                self.set_arc_geometry(s, m, e);
            }
            ShapeT::Poly => self.poly.mirror_seg(axis),
            ShapeT::Bezier => {
                self.start = axis.reflect_point(self.start);
                self.end = axis.reflect_point(self.end);
                self.bezier_c1 = axis.reflect_point(self.bezier_c1);
                self.bezier_c2 = axis.reflect_point(self.bezier_c2);

                self.rebuild_bezier_to_segments_points_list(max_error);
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                let axis_angle = EdaAngle::from_vector(Vector2D::from(axis.b - axis.a));
                let old_start = self.ellipse.start_angle;

                self.ellipse.center = axis.reflect_point(self.ellipse.center);
                self.ellipse.rotation = self.ellipse.rotation.reflect(axis_angle);
                self.ellipse.rotation.normalize();

                // The mirrored parameterisation runs the other way round.
                self.ellipse.start_angle = -self.ellipse.end_angle;
                self.ellipse.end_angle = -old_start;

                self.recalc_ellipse_arc_endpoints();
            }
            ShapeT::Undefined => {}
        }

        self.set_hatching_dirty();
    }

    // ---- bounding box -----------------------------------------------------------------

    /// The width-independent part of `getBoundingBox()` (everything before
    /// `bbox.Inflate( std::max( 0, GetWidth() ) / 2 )`), not normalized. The generic
    /// `shape_bounding_box` adds the host's width.
    ///
    /// Inflating by `d >= 0` and normalizing gives the same centre as normalizing alone
    /// (`BOX2::Inflate` moves the origin by `-d` and grows the size by `2d` on a normalized
    /// box, and mirrors that on a negative one), which is why `getCenter()` can use this.
    pub fn bounding_box_core(&self) -> Box2I {
        let mut bbox = Box2I::default();

        match self.shape {
            ShapeT::Rectangle => {
                for pt in self.rect_corners() {
                    bbox.merge_point(pt);
                }
            }
            ShapeT::Segment => {
                bbox.set_origin(self.start);
                bbox.set_end(self.end);
            }
            ShapeT::Circle => {
                bbox.set_origin(self.start);
                bbox.inflate(self.radius());
            }
            ShapeT::Arc => self.compute_arc_bbox(&mut bbox),
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                bbox = self.build_shape_ellipse().bbox(0);
            }
            ShapeT::Poly => {
                if !self.poly.is_empty() {
                    let mut iter = self.poly.c_iterate();

                    while iter.is_valid() {
                        bbox.merge_point(iter.get());
                        iter.advance();
                    }
                }
            }
            ShapeT::Bezier => {
                // Bezier BBoxes are not trivial to compute, so we approximate it by
                // using the bounding box of the curve (not control!) points.
                for pt in &self.bezier_points {
                    bbox.merge_point(*pt);
                }
            }
            ShapeT::Undefined => {}
        }

        bbox
    }

    /// `computeArcBBox( aBBox )`.
    pub fn compute_arc_bbox(&self, bbox: &mut Box2I) {
        // Start, end, and each inflection point the arc crosses will enclose the entire arc.
        // Only include the center when filled; it's not necessarily inside the BB of an
        // unfilled arc with a small included angle.
        bbox.set_origin(self.start);
        bbox.merge_point(self.end);

        if self.is_any_fill() {
            bbox.merge_point(self.arc_center);
        }

        let radius = self.radius();
        let (mut t1, mut t2) = self.calc_arc_angles();

        t1.normalize();
        t2.normalize();

        let c = self.arc_center;
        let right = Vector2I::new(c.x + radius, c.y);
        let down = Vector2I::new(c.x, c.y + radius);
        let left = Vector2I::new(c.x - radius, c.y);
        let up = Vector2I::new(c.x, c.y - radius);

        if t2 > t1 {
            if t1 < ANGLE_0 && t2 > ANGLE_0 {
                bbox.merge_point(right);
            }

            if t1 < ANGLE_90 && t2 > ANGLE_90 {
                bbox.merge_point(down);
            }

            if t1 < ANGLE_180 && t2 > ANGLE_180 {
                bbox.merge_point(left);
            }

            if t1 < ANGLE_270 && t2 > ANGLE_270 {
                bbox.merge_point(up);
            }
        } else {
            if t1 < ANGLE_0 || t2 > ANGLE_0 {
                bbox.merge_point(right);
            }

            if t1 < ANGLE_90 || t2 > ANGLE_90 {
                bbox.merge_point(down);
            }

            if t1 < ANGLE_180 || t2 > ANGLE_180 {
                bbox.merge_point(left);
            }

            if t1 < ANGLE_270 || t2 > ANGLE_270 {
                bbox.merge_point(up);
            }
        }
    }

    // ---- comparison -------------------------------------------------------------------

    /// `SwapShape( aImage )`: swaps the geometry and style members (not the proxy flag, the
    /// rectangle size cache or the arc mid cache, as in C++).
    pub fn swap_shape(&mut self, image: &mut EdaShape) {
        std::mem::swap(&mut self.stroke, &mut image.stroke);
        std::mem::swap(&mut self.start, &mut image.start);
        std::mem::swap(&mut self.end, &mut image.end);
        std::mem::swap(&mut self.arc_center, &mut image.arc_center);
        std::mem::swap(&mut self.shape, &mut image.shape);
        std::mem::swap(&mut self.bezier_c1, &mut image.bezier_c1);
        std::mem::swap(&mut self.bezier_c2, &mut image.bezier_c2);
        std::mem::swap(&mut self.bezier_points, &mut image.bezier_points);
        std::mem::swap(&mut self.poly, &mut image.poly);
        std::mem::swap(&mut self.ellipse, &mut image.ellipse);
        std::mem::swap(&mut self.corner_radius, &mut image.corner_radius);
        std::mem::swap(&mut self.fill, &mut image.fill);
        std::mem::swap(&mut self.fill_color, &mut image.fill_color);
        std::mem::swap(&mut self.ends_swapped, &mut image.ends_swapped);
        std::mem::swap(&mut self.start_ending, &mut image.start_ending);
        std::mem::swap(&mut self.end_ending, &mut image.end_ending);

        self.set_hatching_dirty();
    }

    /// `Compare( aOther )`: 0 when equal within `EPSILON` = 2 IU on coordinates, else the
    /// signed difference of the first differing member.
    pub fn compare(&self, other: &EdaShape) -> i32 {
        // Should be enough for rounding errors on calculated items
        const EPSILON: i32 = 2;

        macro_rules! test {
            ($a:expr, $b:expr) => {{
                let (a, b): (i32, i32) = ($a, $b);
                if a != b {
                    return a.wrapping_sub(b);
                }
            }};
        }
        macro_rules! test_e {
            ($a:expr, $b:expr) => {{
                let (a, b): (i32, i32) = ($a, $b);
                if a.wrapping_sub(b).wrapping_abs() > EPSILON {
                    return a.wrapping_sub(b);
                }
            }};
        }
        macro_rules! test_pt {
            ($a:expr, $b:expr) => {{
                let (a, b): (Vector2I, Vector2I) = ($a, $b);
                test_e!(a.x, b.x);
                test_e!(a.y, b.y);
            }};
        }

        test_pt!(self.start, other.start);
        test_pt!(self.end, other.end);

        test!(self.shape as i32, other.shape as i32);

        match self.shape {
            ShapeT::Rectangle => test!(self.corner_radius, other.corner_radius),
            ShapeT::Arc => test_pt!(self.arc_mid(), other.arc_mid()),
            ShapeT::Bezier => {
                test_pt!(self.bezier_c1, other.bezier_c1);
                test_pt!(self.bezier_c2, other.bezier_c2);
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                test_pt!(self.ellipse.center, other.ellipse.center);
                test_e!(self.ellipse.major_radius, other.ellipse.major_radius);
                test_e!(self.ellipse.minor_radius, other.ellipse.minor_radius);
                test_e!(
                    self.ellipse.rotation.as_tenths_of_a_degree(),
                    other.ellipse.rotation.as_tenths_of_a_degree()
                );

                if self.shape == ShapeT::EllipseArc {
                    test_e!(
                        self.ellipse.start_angle.as_tenths_of_a_degree(),
                        other.ellipse.start_angle.as_tenths_of_a_degree()
                    );
                    test_e!(
                        self.ellipse.end_angle.as_tenths_of_a_degree(),
                        other.ellipse.end_angle.as_tenths_of_a_degree()
                    );
                }
            }
            ShapeT::Poly => test!(self.poly.total_vertices(), other.poly.total_vertices()),
            _ => {}
        }

        if self.bezier_points.len() != other.bezier_points.len() {
            return if self.bezier_points.len() < other.bezier_points.len() {
                -1
            } else {
                1
            };
        }

        for ii in 0..self.bezier_points.len() {
            test_pt!(self.bezier_points[ii], other.bezier_points[ii]);
        }

        for ii in 0..self.poly.total_vertices() {
            test_pt!(
                self.poly.c_vertex_global(ii),
                other.poly.c_vertex_global(ii)
            );
        }

        test_e!(self.stroke.width(), other.stroke.width());
        test!(
            self.stroke.line_style() as i32,
            other.stroke.line_style() as i32
        );
        test!(self.fill as i32, other.fill as i32);

        0
    }

    /// `Similarity( aOther )`: 1.0 for identical shapes, 0.0 for different shape types.
    pub fn similarity(&self, other: &EdaShape) -> f64 {
        if self.shape != other.shape {
            return 0.0;
        }

        let mut similarity = 1.0;

        if self.fill != other.fill {
            similarity *= 0.9;
        }

        if self.stroke.width() != other.stroke.width() {
            similarity *= 0.9;
        }

        if self.stroke.line_style() != other.stroke.line_style() {
            similarity *= 0.9;
        }

        if self.fill_color != other.fill_color {
            similarity *= 0.9;
        }

        if self.start != other.start {
            similarity *= 0.9;
        }

        if self.end != other.end {
            similarity *= 0.9;
        }

        if self.arc_center != other.arc_center {
            similarity *= 0.9;
        }

        if self.bezier_c1 != other.bezier_c1 {
            similarity *= 0.9;
        }

        if self.bezier_c2 != other.bezier_c2 {
            similarity *= 0.9;
        }

        {
            let m = self.bezier_points.len();
            let n = other.bezier_points.len();

            let longest = longest_common_subset(&self.bezier_points, &other.bezier_points);

            similarity *= cmath::pow(0.9, (m + n - 2 * longest) as f64);
        }

        {
            let m = self.poly.total_vertices();
            let n = other.poly.total_vertices();
            let mut poly: Vec<Vector2I> = Vec::new();
            let mut other_poly: Vec<Vector2I> = Vec::new();
            let mut last_pt = Vector2I::new(0, 0);

            // We look for the longest common subset of the two polygons, but we need to
            // offset each point because we're actually looking for overall similarity, not
            // just exact matches.  So if the zone is moved by 1IU, we only want one point to be
            // considered "moved" rather than the entire polygon.  In this case, the first
            // point will not be a match but the rest of the sequence will.
            for ii in 0..m {
                poly.push(last_pt - self.poly.c_vertex_global(ii));
                last_pt = self.poly.c_vertex_global(ii);
            }

            last_pt = Vector2I::new(0, 0);

            for ii in 0..n {
                other_poly.push(last_pt - other.poly.c_vertex_global(ii));
                last_pt = other.poly.c_vertex_global(ii);
            }

            let longest = longest_common_subset(&poly, &other_poly);

            similarity *= cmath::pow(0.9, (m as usize + n as usize - 2 * longest) as f64);
        }

        if self.start_ending != other.start_ending {
            similarity *= 0.9;
        }

        if self.end_ending != other.end_ending {
            similarity *= 0.9;
        }

        similarity
    }
}

impl PartialEq for EdaShape {
    /// `EDA_SHAPE::operator==`: shape, fill, stroke width / style, fill colour, the geometry
    /// members that matter for the shape type and the line endings. `UNDEFINED` shapes are
    /// never equal.
    fn eq(&self, other: &EdaShape) -> bool {
        if self.shape != other.shape {
            return false;
        }

        if self.fill != other.fill {
            return false;
        }

        if self.stroke.width() != other.stroke.width() {
            return false;
        }

        if self.stroke.line_style() != other.stroke.line_style() {
            return false;
        }

        if self.fill_color != other.fill_color {
            return false;
        }

        match self.shape {
            ShapeT::Segment | ShapeT::Rectangle | ShapeT::Circle => {
                if self.start != other.start || self.end != other.end {
                    return false;
                }
            }
            ShapeT::Arc => {
                if self.start != other.start
                    || self.end != other.end
                    || self.arc_center != other.arc_center
                {
                    return false;
                }
            }
            ShapeT::Poly => {
                if self.poly.total_vertices() != other.poly.total_vertices() {
                    return false;
                }

                for ii in 0..self.poly.total_vertices() {
                    if self.poly.c_vertex_global(ii) != other.poly.c_vertex_global(ii) {
                        return false;
                    }
                }
            }
            ShapeT::Bezier => {
                if self.start != other.start
                    || self.end != other.end
                    || self.bezier_c1 != other.bezier_c1
                    || self.bezier_c2 != other.bezier_c2
                    || self.bezier_points != other.bezier_points
                {
                    return false;
                }
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                let (a, b) = (&self.ellipse, &other.ellipse);

                if a.center != b.center
                    || a.major_radius != b.major_radius
                    || a.minor_radius != b.minor_radius
                    || a.rotation != b.rotation
                {
                    return false;
                }

                if self.shape == ShapeT::EllipseArc
                    && (a.start_angle != b.start_angle || a.end_angle != b.end_angle)
                {
                    return false;
                }
            }
            ShapeT::Undefined => return false,
        }

        if self.start_ending != other.start_ending {
            return false;
        }

        if self.end_ending != other.end_ending {
            return false;
        }

        true
    }
}

/// Helper of the hit tests: a fresh `SHAPE_POLY_SET` holding one outline of `pts`.
pub(crate) fn poly_from_points(pts: &[Vector2I]) -> ShapePolySet {
    let mut poly = ShapePolySet::default();
    poly.new_outline();

    for pt in pts {
        poly.append_point(*pt, -1, -1);
    }

    poly
}

/// `hasLineEnding( aStartEnding, aEndEnding )` (static, `common/eda_shape.cpp`): reads the
/// stored endings, not `GetStartEnding()`.
pub(crate) fn has_line_ending(es: &EdaShape) -> bool {
    es.start_ending.style() != LineEndingStyle::None
        || es.end_ending.style() != LineEndingStyle::None
}

/// The point list a bezier hit test walks: `m_bezierPoints`, or a fresh conversion with
/// `aAccuracy / 2` as maximum error when it is empty.
fn bezier_hit_points(es: &EdaShape, accuracy: i32) -> Vec<Vector2I> {
    if es.bezier_points.is_empty() {
        let converter = BezierPoly::new(es.start, es.bezier_c1, es.bezier_c2, es.end);
        let mut updated = Vec::new();
        converter.get_poly_i(&mut updated, accuracy / 2);
        return updated;
    }

    es.bezier_points.clone()
}

/// `EDA_SHAPE::hitTest( aPosition, aAccuracy )` over the host (`GetWidth`,
/// `IsFilledForHitTesting`, `IsProxyItem`, `getMaxError` and `GetHatching` are the host's).
pub(crate) fn hit_test_point<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    pos: Vector2I,
    accuracy: i32,
) -> bool {
    let es = host.eda_shape();

    if has_line_ending(es) {
        let shape =
            ShapeCompound::from_shapes(super::shapes::make_effective_shapes_with_line_endings(
                host,
                ctx,
                host.effective_width(ctx),
            ));
        let hit_shape: &dyn Shape = &shape;

        return hit_shape.collide_point(pos, 0.max(accuracy), None, None);
    }

    let mut maxdist = accuracy as f64;

    if host.width(ctx) > 0 {
        maxdist += host.width(ctx) as f64 / 2.0;
    }

    // C++ passes the double `maxdist` to `int` parameters: truncation.
    let maxdist_i = maxdist as i32;

    match es.shape {
        ShapeT::Circle => {
            let radius = es.radius() as f64;
            let dist = pos.distance(es.center());

            if host.is_filled_for_hit_testing() {
                return dist <= radius + maxdist; // Filled circle hit-test
            } else if (radius - dist).abs() <= maxdist {
                return true; // Ring hit-test
            }

            if es.is_hatched_fill() && host.hatching(ctx).collide_point(pos, maxdist_i, None, None)
            {
                return true;
            }

            false
        }
        ShapeT::Arc => {
            if pos.distance(es.start) <= maxdist {
                return true;
            }

            if pos.distance(es.end) <= maxdist {
                return true;
            }

            let radius = es.radius() as f64;
            let rel_pos = Vector2D::from(pos) - Vector2D::from(es.center());
            let dist = rel_pos.euclidean_norm();

            if host.is_filled_for_hit_testing() {
                // Check distance from arc center
                if dist > radius + maxdist {
                    return false;
                }
            } else {
                // Check distance from arc circumference
                if (radius - dist).abs() > maxdist {
                    return false;
                }
            }

            // Finally, check to see if it's within arc's swept angle.
            let (mut start_angle, mut end_angle) = es.calc_arc_angles();
            let mut rel_pos_angle = EdaAngle::from_vector(rel_pos);

            start_angle.normalize();
            end_angle.normalize();
            rel_pos_angle.normalize();

            if end_angle > start_angle {
                rel_pos_angle >= start_angle && rel_pos_angle <= end_angle
            } else {
                rel_pos_angle >= start_angle || rel_pos_angle <= end_angle
            }
        }
        ShapeT::Bezier => {
            let pts = bezier_hit_points(es, accuracy);

            for i in 1..pts.len() {
                if test_segment_hit(pos, pts[i - 1], pts[i], maxdist_i) {
                    return true;
                }
            }

            false
        }
        ShapeT::Segment => test_segment_hit(pos, es.start, es.end, maxdist_i),
        ShapeT::Rectangle => {
            if host.is_proxy_item() || host.is_filled_for_hit_testing() {
                // Filled rect hit-test
                let poly = poly_from_points(&es.rect_corners());
                return poly.collide_point(pos, maxdist_i, None, None);
            } else if es.corner_radius > 0 {
                let mut poly = ShapePolySet::default();
                es.roundrect()
                    .transform_to_polygon(&mut poly, host.shape_max_error(ctx));

                if poly.collide_edge(pos, None, maxdist_i) {
                    return true;
                }
            } else {
                let pts = es.rect_corners();

                if test_segment_hit(pos, pts[0], pts[1], maxdist_i)
                    || test_segment_hit(pos, pts[1], pts[2], maxdist_i)
                    || test_segment_hit(pos, pts[2], pts[3], maxdist_i)
                    || test_segment_hit(pos, pts[3], pts[0], maxdist_i)
                {
                    return true;
                }
            }

            if es.is_hatched_fill() && host.hatching(ctx).collide_point(pos, maxdist_i, None, None)
            {
                return true;
            }

            false
        }
        ShapeT::Poly => {
            if es.poly.outline_count() < 1 {
                // empty poly
                return false;
            }

            if host.is_filled_for_hit_testing() {
                if !es.poly.c_outline(0).is_closed() {
                    // Only one outline is expected
                    let mut copy = es.poly.c_outline(0).clone();
                    copy.set_closed(true);
                    copy.collide_point(pos, maxdist_i, None, None)
                } else {
                    es.poly.collide_point(pos, maxdist_i, None, None)
                }
            } else {
                if es.poly.collide_edge(pos, None, maxdist_i) {
                    return true;
                }

                if es.is_hatched_fill()
                    && host.hatching(ctx).collide_point(pos, maxdist_i, None, None)
                {
                    return true;
                }

                false
            }
        }
        ShapeT::Ellipse | ShapeT::EllipseArc => {
            let e = es.build_shape_ellipse();

            let maxdist_sq = maxdist * maxdist;

            if es.shape == ShapeT::Ellipse && host.is_filled_for_hit_testing() {
                // Filled closed ellipse
                if e.squared_distance(pos, false) as f64 <= maxdist_sq {
                    return true;
                }
            } else {
                // Unfilled ring or arc
                if e.squared_distance(pos, true) as f64 <= maxdist_sq {
                    return true;
                }
            }

            if es.is_hatched_fill() && host.hatching(ctx).collide_point(pos, maxdist_i, None, None)
            {
                return true;
            }

            false
        }
        ShapeT::Undefined => false,
    }
}

/// `checkOutline` lambda of `hitTest( aRect, .. )`: a vertex inside `rect` or an edge
/// crossing it.
fn check_outline(rect: &Box2I, outline: &ShapeLineChain) -> bool {
    let count = outline.point_count();

    for ii in 0..count {
        let vertex = outline.c_point(ii);

        // Test if the point is within rect
        if rect.contains(vertex) {
            return true;
        }

        if ii + 1 < count {
            let vertex_next = outline.c_point(ii + 1);

            // Test if this edge intersects rect
            if rect.intersects_segment(vertex, vertex_next) {
                return true;
            }
        } else if outline.is_closed() {
            let vertex_next = outline.c_point(0);

            // Test if this edge intersects rect
            if rect.intersects_segment(vertex, vertex_next) {
                return true;
            }
        }
    }

    false
}

/// `EDA_SHAPE::hitTest( aRect, aContained, aAccuracy )` over the host.
pub(crate) fn hit_test_rect<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    a_rect: &Box2I,
    contained: bool,
    accuracy: i32,
) -> bool {
    let es = host.eda_shape();
    let mut rect = *a_rect;
    rect.normalize();
    rect.inflate(accuracy);

    if has_line_ending(es) {
        let selection = box_to_line_chain(&rect);
        let shape =
            ShapeCompound::from_shapes(super::shapes::make_effective_shapes_with_line_endings(
                host,
                ctx,
                host.effective_width(ctx),
            ));

        return shape_hit_test(&selection, &shape, contained);
    }

    let bbox = super::shape_bounding_box(host, ctx);

    match es.shape {
        ShapeT::Circle => {
            // Test if area intersects or contains the circle:
            if contained {
                rect.contains_box(&bbox)
            } else {
                // If the rectangle does not intersect the bounding box, this is a much quicker
                // test
                if !rect.intersects(&bbox) {
                    false
                } else {
                    rect.intersects_circle_edge(es.center(), es.radius(), host.width(ctx))
                }
            }
        }
        ShapeT::Arc => {
            // Test for full containment of this arc in the rect
            if contained {
                rect.contains_box(&bbox)
            }
            // Test if the rect crosses the arc
            else {
                if !rect.intersects(&bbox) {
                    return false;
                }

                if es.is_any_fill() {
                    rect.intersects_segment(es.center(), es.start)
                        || rect.intersects_segment(es.center(), es.end)
                        || rect.intersects_circle_edge(es.center(), es.radius(), host.width(ctx))
                } else {
                    rect.intersects_circle_edge(es.center(), es.radius(), host.width(ctx))
                }
            }
        }
        ShapeT::Rectangle => {
            if contained {
                rect.contains_box(&bbox)
            } else if es.corner_radius > 0 {
                let mut poly = ShapePolySet::default();
                es.roundrect()
                    .transform_to_polygon(&mut poly, host.shape_max_error(ctx));

                // Account for the width of the line
                rect.inflate(host.width(ctx) / 2);

                check_outline(&rect, poly.c_outline(0))
            } else {
                let pts = es.rect_corners();

                // Account for the width of the lines
                rect.inflate(host.width(ctx) / 2);
                rect.intersects_segment(pts[0], pts[1])
                    || rect.intersects_segment(pts[1], pts[2])
                    || rect.intersects_segment(pts[2], pts[3])
                    || rect.intersects_segment(pts[3], pts[0])
            }
        }
        ShapeT::Segment => {
            if contained {
                rect.contains(es.start) && rect.contains(es.end)
            } else {
                // Account for the width of the line
                rect.inflate(host.width(ctx) / 2);
                rect.intersects_segment(es.start, es.end)
            }
        }
        ShapeT::Poly => {
            if contained {
                rect.contains_box(&bbox)
            } else {
                // Fast test: if rect is outside the polygon bounding box,
                // rectangles cannot intersect
                if !rect.intersects(&bbox) {
                    return false;
                }

                // Account for the width of the line
                rect.inflate(host.width(ctx) / 2);

                for ii in 0..es.poly.outline_count() {
                    if check_outline(&rect, es.poly.c_outline(ii)) {
                        return true;
                    }
                }

                false
            }
        }
        ShapeT::Bezier => {
            if contained {
                rect.contains_box(&bbox)
            } else {
                // Fast test: if rect is outside the polygon bounding box,
                // rectangles cannot intersect
                if !rect.intersects(&bbox) {
                    return false;
                }

                // Account for the width of the line
                rect.inflate(host.width(ctx) / 2);
                let pts = bezier_hit_points(es, accuracy);

                for ii in 1..pts.len() {
                    let vertex = pts[ii - 1];
                    let vertex_next = pts[ii];

                    // Test if the point is within rect
                    if rect.contains(vertex) {
                        return true;
                    }

                    // Test if this edge intersects rect
                    if rect.intersects_segment(vertex, vertex_next) {
                        return true;
                    }
                }

                false
            }
        }
        ShapeT::Ellipse | ShapeT::EllipseArc => {
            if contained {
                rect.contains_box(&bbox)
            } else {
                if !rect.intersects(&bbox) {
                    return false;
                }

                let e = es.build_shape_ellipse();

                let tess_error = 1.max(accuracy / 2);
                let chain = e.convert_to_polyline(tess_error);

                // Account for the width of the line
                rect.inflate(host.width(ctx) / 2);
                check_outline(&rect, &chain)
            }
        }
        ShapeT::Undefined => false,
    }
}
