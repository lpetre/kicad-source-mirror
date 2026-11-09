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

//! Port of `libs/kimath/include/geometry/shape_arc.h` and
//! `libs/kimath/src/geometry/shape_arc.cpp`, plus the `BOX2I_MINMAX( const SHAPE_ARC& )`
//! constructor of `libs/kimath/include/math/box2_minmax.h`.

use std::fmt;

use crate::cmath::f64_to_i32_x86;
use crate::geometry::approximation::ErrorLoc;
use crate::geometry::circle::Circle;
use crate::geometry::eda_angle::{ANGLE_0, ANGLE_90, ANGLE_360, EdaAngle};
use crate::geometry::geometry_utils::{
    ARC_HIGH_DEF, circle_to_end_segment_delta_radius, get_arc_to_segment_count,
};
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeRef, ShapeType};
use crate::geometry::shape_circle::ShapeCircle;
use crate::geometry::shape_collisions;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::geometry::shape_rect::ShapeRect;
use crate::kicore::mirror::FlipDirection;
use crate::math::box2::Box2I;
use crate::math::box2_minmax::Box2IMinMax;
use crate::math::util::ki_round;
use crate::math::vector2::{Vector2D, Vector2I, Vector2L};
use crate::trigo::{calc_arc_center, calc_arc_center_from_angle, rotate_point_around};

/// `(double) std::numeric_limits<int>::max() / 2.0`: radii at or above this value are treated
/// as (nearly) straight lines.
const LARGE_RADIUS: f64 = i32::MAX as f64 / 2.0;

/// `VECTOR2I` subtraction with the two's complement wrapping of the reference build, for the
/// few places where the C++ subtracts a clamped arc center and can overflow.
fn wrapping_sub(a: Vector2I, b: Vector2I) -> Vector2I {
    Vector2I::new(a.x.wrapping_sub(b.x), a.y.wrapping_sub(b.y))
}

/// Port of `SHAPE_ARC`: a circular arc given by its start, mid and end points.
#[derive(Clone, Debug)]
pub struct ShapeArc {
    start: Vector2I,
    mid: Vector2I,
    end: Vector2I,
    width: i32,
    /// Calculated value
    bbox: Box2I,
    /// Calculated value
    center: Vector2I,
    /// Calculated value
    radius: f64,
}

impl Default for ShapeArc {
    /// Port of `SHAPE_ARC()`: all points at the origin, nothing calculated.
    fn default() -> Self {
        ShapeArc {
            start: Vector2I::default(),
            mid: Vector2I::default(),
            end: Vector2I::default(),
            width: 0,
            bbox: Box2I::default(),
            center: Vector2I::default(),
            radius: 0.0,
        }
    }
}

impl PartialEq for ShapeArc {
    /// Port of `SHAPE_ARC::operator==`: compares the points and the width.
    fn eq(&self, a_arc: &ShapeArc) -> bool {
        (a_arc.start == self.start)
            && (a_arc.end == self.end)
            && (a_arc.mid == self.mid)
            && (a_arc.width == self.width)
    }
}

impl fmt::Display for ShapeArc {
    /// Port of `operator<<( std::ostream&, const SHAPE_ARC& )`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Arc( P0={} P1={} Mid={} Width={} )",
            self.get_p0().to_stream_string(),
            self.get_p1().to_stream_string(),
            self.get_arc_mid().to_stream_string(),
            self.width
        )
    }
}

impl ShapeArc {
    /// Port of `SHAPE_ARC()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `SHAPE_ARC( const VECTOR2I& aArcCenter, const VECTOR2I& aArcStartPoint,
    /// const EDA_ANGLE& aCenterAngle, int aWidth )`: construct an arc using center, start,
    /// angle.  Center and angle are used to calculate the mid and end points of the arc, and
    /// are not stored.
    pub fn from_center_start_angle(
        a_arc_center: Vector2I,
        a_arc_start_point: Vector2I,
        a_center_angle: EdaAngle,
        a_width: i32,
    ) -> Self {
        let mut arc = ShapeArc {
            width: a_width,
            ..ShapeArc::default()
        };

        arc.start = a_arc_start_point;

        let mut mid: Vector2D = a_arc_start_point.into();
        let mut end: Vector2D = a_arc_start_point.into();
        let center: Vector2D = a_arc_center.into();

        rotate_point_around(&mut mid, center, -a_center_angle / 2.0);
        rotate_point_around(&mut end, center, -a_center_angle);

        arc.mid = ki_round(mid);
        arc.end = ki_round(end);

        arc.update_values();
        arc
    }

    /// Port of `SHAPE_ARC( const VECTOR2I& aArcStart, const VECTOR2I& aArcMid,
    /// const VECTOR2I& aArcEnd, int aWidth )`.
    pub fn from_start_mid_end(
        a_arc_start: Vector2I,
        a_arc_mid: Vector2I,
        a_arc_end: Vector2I,
        a_width: i32,
    ) -> Self {
        let mut arc = ShapeArc {
            start: a_arc_start,
            mid: a_arc_mid,
            end: a_arc_end,
            width: a_width,
            ..ShapeArc::default()
        };

        arc.update_values();
        arc
    }

    /// Port of `SHAPE_ARC( const SEG& aSegmentA, const SEG& aSegmentB, int aRadius,
    /// int aWidth )`: build an arc which is tangent to two segments and has a given radius.
    pub fn from_tangent_segments(
        a_segment_a: &Seg,
        a_segment_b: &Seg,
        a_radius: i32,
        a_width: i32,
    ) -> Self {
        let mut arc = ShapeArc {
            width: a_width,
            ..ShapeArc::default()
        };

        // Construct an arc that is tangent to two segments with a given radius.
        //
        //               p
        //                A
        //             A   \
        //            /     \
        //           /  . .  \ segB
        //          /.       .\
        //   segA  /     c     \
        //        /             B
        //       /
        //      /
        //     B
        //
        //
        // segA is the fist segment (with its points A and B)
        // segB is the second segment (with its points A and B)
        // p is the point at which segA and segB would intersect if they were projected
        // c is the centre of the arc to be constructed
        // rad is the radius of the arc to be constructed
        //
        // We can create two vectors, between point p and segA /segB
        //    pToA = p - segA.B   //< note that segA.A would also be valid as it is colinear
        //    pToB = p - segB.B   //< note that segB.A would also be valid as it is colinear
        //
        // Let the angle formed by segA and segB be called 'alpha':
        //   alpha = angle( pToA ) - angle( pToB )
        //
        // The distance PC can be computed as
        //   distPC = rad / abs( sin( alpha / 2 ) )
        //
        // The polar angle of the vector PC can be computed as:
        //   anglePC = angle( pToA ) + alpha / 2
        //
        // Therefore:
        //    C.x = P.x + distPC*cos( anglePC )
        //    C.y = P.y + distPC*sin( anglePC )

        let p = a_segment_a.intersect_with(a_segment_b, true, true);

        match p {
            Some(p) if a_segment_a.length() != 0 && a_segment_b.length() != 0 => {
                let mut p_to_a = a_segment_a.b - p;
                let mut p_to_b = a_segment_b.b - p;

                if p_to_a.euclidean_norm() == 0 {
                    p_to_a = a_segment_a.a - p;
                }

                if p_to_b.euclidean_norm() == 0 {
                    p_to_b = a_segment_b.a - p;
                }

                let p_to_a_angle = EdaAngle::from_vector(p_to_a);
                let p_to_b_angle = EdaAngle::from_vector(p_to_b);

                let alpha = (p_to_a_angle - p_to_b_angle).normalize180();

                let dist_pc: f64 =
                    a_radius as f64 / crate::cmath::sin(alpha.as_radians() / 2.0).abs();
                let ang_pc = p_to_a_angle - alpha / 2;
                let arc_center = Vector2I::new(
                    p.x + ki_round(dist_pc * ang_pc.cos()),
                    p.y + ki_round(dist_pc * ang_pc.sin()),
                );

                // The end points of the arc are the orthogonal projected lines from the line segments
                // to the center of the arc
                arc.start = a_segment_a.line_project(arc_center);
                arc.end = a_segment_b.line_project(arc_center);

                //The mid point is rotated start point around center, half the angle of the arc.
                let start_vector = arc.start - arc_center;
                let end_vector = arc.end - arc_center;

                let start_angle = EdaAngle::from_vector(start_vector);
                let end_angle = EdaAngle::from_vector(end_vector);
                let mid_point_rot_angle = (start_angle - end_angle).normalize180() / 2;

                arc.mid = arc.start;
                rotate_point_around(&mut arc.mid, arc_center, mid_point_rot_angle);
            }
            _ => {
                // The input segments do not intersect or one is zero length (a wxASSERT_MSG
                // in C++ debug builds).

                // Make a 180 degree arc around aSegmentA in case we end up here in release
                arc.start = a_segment_a.a;
                arc.end = a_segment_a.b;
                arc.mid = arc.start;

                let arc_center = a_segment_a.center();
                rotate_point_around(&mut arc.mid, arc_center, ANGLE_90); // mid point at 90 degrees
            }
        }

        arc.update_values();
        arc
    }

    /// Port of `SHAPE_ARC( const SHAPE_ARC& aOther, int aWidth )`.
    pub fn with_width(a_other: &ShapeArc, a_width: i32) -> Self {
        let mut arc = a_other.clone();
        arc.width = a_width;
        arc
    }

    /// Port of `SHAPE_ARC::ConstructFromStartEndAngle()`: construct this arc from the given
    /// start, end and included angle.
    pub fn construct_from_start_end_angle(
        &mut self,
        a_start: Vector2I,
        a_end: Vector2I,
        a_angle: EdaAngle,
        a_width: f64,
    ) -> &mut Self {
        self.start = a_start;
        self.mid = a_start;
        self.end = a_end;
        // double to int conversion
        self.width = f64_to_i32_x86(a_width);

        let center: Vector2I =
            calc_arc_center_from_angle(a_start.into(), a_end.into(), a_angle).cast();

        rotate_point_around(&mut self.mid, center, -a_angle / 2.0);

        self.update_values();

        self
    }

    /// Port of `SHAPE_ARC::ConstructFromStartEndCenter()`: construct this arc from the given
    /// start, end and center; `a_clockwise` determines which of the two solutions to construct.
    /// (`a_width` is ignored, as in C++.)
    pub fn construct_from_start_end_center(
        &mut self,
        a_start: Vector2I,
        a_end: Vector2I,
        a_center: Vector2I,
        a_clockwise: bool,
        _a_width: f64,
    ) -> &mut Self {
        let start_line = a_start - a_center;
        let end_line = a_end - a_center;

        let mut start_angle = EdaAngle::from_vector(start_line);
        let mut end_angle = EdaAngle::from_vector(end_line);

        start_angle.normalize();
        end_angle.normalize();

        let mut angle = end_angle - start_angle;

        if a_clockwise {
            angle = angle.normalize() - ANGLE_360;
        } else {
            angle = angle.normalize();
        }

        self.start = a_start;
        self.end = a_end;
        self.mid = a_start;

        rotate_point_around(&mut self.mid, a_center, -angle / 2.0);

        self.update_values();

        self
    }

    /// Port of `GetP0()`.
    pub fn get_p0(&self) -> Vector2I {
        self.start
    }

    /// Port of `GetP1()`.
    pub fn get_p1(&self) -> Vector2I {
        self.end
    }

    /// Port of `GetArcMid()`.
    pub fn get_arc_mid(&self) -> Vector2I {
        self.mid
    }

    /// Port of `SHAPE_ARC::GetCenter()`.
    pub fn get_center(&self) -> Vector2I {
        self.center
    }

    /// Port of `SHAPE_ARC::GetRadius()`.
    pub fn get_radius(&self) -> f64 {
        self.radius
    }

    /// Port of `GetChord()`.
    pub fn get_chord(&self) -> Seg {
        Seg::new(self.start, self.end)
    }

    /// Port of `SHAPE_ARC::DefaultAccuracyForPCB()`: `ARC_HIGH_DEF` in Pcbnew units.
    pub const fn default_accuracy_for_pcb() -> i32 {
        ARC_HIGH_DEF
    }

    /// Port of `SHAPE_ARC::IsCCW()`: true if the arc is counter-clockwise.
    pub fn is_ccw(&self) -> bool {
        let mid = Vector2L::from(self.mid);
        let v1: Vector2L = self.end - mid;
        let v2: Vector2L = self.start - mid;
        v1.cross(v2) > 0
    }

    /// Port of `SHAPE_ARC::IsClockwise()`.
    pub fn is_clockwise(&self) -> bool {
        !self.is_ccw()
    }

    /// Port of `SHAPE_ARC::IsEffectiveLine()`.
    pub fn is_effective_line(&self) -> bool {
        let v1 = Seg::new(self.start, self.mid);
        let v2 = Seg::new(self.mid, self.end);

        v1.approx_collinear(&v2) && (v1.b - v1.a).dot(v2.b - v2.a) > 0
    }

    /// Port of `SHAPE_ARC::IntersectLine()`: intersection points between this arc and `a_seg`
    /// treated as an infinite line (ignoring the arc width), appended to `a_ips_buffer`.
    /// Returns the number of points found.
    pub fn intersect_line(&self, a_seg: &Seg, a_ips_buffer: &mut Vec<Vector2I>) -> i32 {
        if a_seg.a == a_seg.b {
            // One point does not define a line....
            return 0;
        }

        if self.get_radius() >= LARGE_RADIUS {
            return 0;
        }

        let circ = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));

        let intersections = circ.intersect_line(a_seg);

        let original_size = a_ips_buffer.len();

        for intersection in intersections {
            if self.slice_contains_point(intersection) {
                a_ips_buffer.push(intersection);
            }
        }

        (a_ips_buffer.len() - original_size) as i32
    }

    /// Port of `SHAPE_ARC::Intersect( const CIRCLE& )`: intersection points between this arc
    /// and a circle (ignoring the arc width).
    pub fn intersect_circle(&self, a_circle: &Circle, a_ips_buffer: &mut Vec<Vector2I>) -> i32 {
        if self.get_radius() >= LARGE_RADIUS {
            return 0;
        }

        let thiscirc = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));

        let intersections = thiscirc.intersect(a_circle);

        let original_size = a_ips_buffer.len();

        for intersection in intersections {
            if self.slice_contains_point(intersection) {
                a_ips_buffer.push(intersection);
            }
        }

        (a_ips_buffer.len() - original_size) as i32
    }

    /// Port of `SHAPE_ARC::Intersect( const SHAPE_ARC& )`: intersection points between this arc
    /// and another arc (ignoring the arc widths).
    pub fn intersect_arc(&self, a_arc: &ShapeArc, a_ips_buffer: &mut Vec<Vector2I>) -> i32 {
        if self.get_radius() >= LARGE_RADIUS || a_arc.get_radius() >= LARGE_RADIUS {
            return 0;
        }

        let thiscirc = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));
        let othercirc = Circle::new(a_arc.get_center(), f64_to_i32_x86(a_arc.get_radius()));

        let intersections = thiscirc.intersect(&othercirc);

        let original_size = a_ips_buffer.len();

        for intersection in intersections {
            if self.slice_contains_point(intersection) && a_arc.slice_contains_point(intersection) {
                a_ips_buffer.push(intersection);
            }
        }

        (a_ips_buffer.len() - original_size) as i32
    }

    /// Port of `SHAPE_ARC::update_values()`: recompute the center, radius and bounding box.
    fn update_values(&mut self) {
        self.center = calc_arc_center(self.start, self.mid, self.end);
        self.radius = (Vector2D::from(self.start) - self.center)
            .squared_euclidean_norm()
            .sqrt();

        // Put start, mid, and end points in the point list
        let mut points: Vec<Vector2I> = vec![self.start, self.mid, self.end];

        let mut start_angle = self.get_start_angle();
        let mut end_angle = start_angle + self.get_central_angle();

        // we always count quadrants clockwise (increasing angle)
        if start_angle > end_angle {
            std::mem::swap(&mut start_angle, &mut end_angle);
        }

        let quad_angle_start: i32 = (start_angle.as_degrees() / 90.0).ceil() as i32;
        let quad_angle_end: i32 = (end_angle.as_degrees() / 90.0).floor() as i32;

        // very large radius means the arc is similar to a segment
        // so do not try to add more points, center cannot be handled
        // Very large is here > INT_MAX/2
        if self.radius < LARGE_RADIUS {
            let radius: i32 = ki_round(self.radius);

            // count through quadrants included in arc
            for quad_angle in quad_angle_start..=quad_angle_end {
                let mut quad_pt = self.center;

                match quad_angle % 4 {
                    0 => quad_pt += Vector2I::new(radius, 0),
                    1 | -3 => quad_pt += Vector2I::new(0, radius),
                    2 | -2 => quad_pt += Vector2I::new(-radius, 0),
                    3 | -1 => quad_pt += Vector2I::new(0, -radius),
                    _ => debug_assert!(false),
                }

                points.push(quad_pt);
            }
        }

        self.bbox.compute(&points);
    }

    /// Port of `SHAPE_ARC::NearestPoint()`: the point on the arc nearest to `a_p`.
    pub fn nearest_point(&self, a_p: Vector2I) -> Vector2I {
        const S_EPSILON: Ecoord = 8;

        let full_circle = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));
        let nearest_pt = full_circle.nearest_point(a_p);

        if (nearest_pt - self.start).squared_euclidean_norm() <= S_EPSILON {
            return self.start;
        }

        if (nearest_pt - self.end).squared_euclidean_norm() <= S_EPSILON {
            return self.end;
        }

        if self.slice_contains_point(nearest_pt) {
            return nearest_pt;
        }

        if (a_p - self.start).squared_euclidean_norm() <= (a_p - self.end).squared_euclidean_norm()
        {
            self.start
        } else {
            self.end
        }
    }

    /// Port of `SHAPE_ARC::NearestPoints( const SHAPE_CIRCLE&, ... )`: closest points between
    /// this arc and `a_circle`.
    ///
    /// The out-parameters are in/out as in C++: when no candidate is found the incoming values
    /// of `a_pt_a` / `a_pt_b` are used for the width adjustment.
    pub fn nearest_points_circle(
        &self,
        a_circle: &ShapeCircle,
        a_pt_a: &mut Vector2I,
        a_pt_b: &mut Vector2I,
        a_dist_sq: &mut i64,
    ) -> bool {
        if self.get_center() == a_circle.get_center()
            && self.get_radius() == a_circle.get_radius() as f64
        {
            *a_pt_a = self.get_p0();
            *a_pt_b = self.get_p0();
            *a_dist_sq = 0;
            return true;
        }

        *a_dist_sq = i64::MAX;

        let circle1 = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));
        let circle2 = a_circle.get_circle();
        let intersections = circle1.intersect(&circle2);

        for pt in intersections {
            if self.slice_contains_point(pt) {
                *a_pt_a = pt;
                *a_pt_b = pt;
                *a_dist_sq = 0;
                return true;
            }
        }

        let pts = [
            self.start,
            self.end,
            circle1.nearest_point(a_circle.get_center()),
        ];

        for pt in pts {
            if self.slice_contains_point(pt) {
                let nearest_pt2 = circle2.nearest_point(pt);
                let dist_sq: i64 = pt.squared_distance(nearest_pt2);

                if dist_sq < *a_dist_sq {
                    *a_dist_sq = dist_sq;
                    *a_pt_a = pt;
                    *a_pt_b = nearest_pt2;
                }
            }
        }

        // Adjust point A by half the arc width towards point B
        let dir = (*a_pt_b - *a_pt_a).resize(self.get_width() / 2);
        *a_pt_a += dir;

        if *a_dist_sq < Seg::square(self.get_width() / 2) {
            *a_dist_sq = 0;
        } else {
            *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);
        }

        true
    }

    /// Port of `SHAPE_ARC::NearestPoints( const SEG&, ... )`: closest points between this arc
    /// and `a_seg` (in/out parameters as in C++).
    pub fn nearest_points_seg(
        &self,
        a_seg: &Seg,
        a_pt_a: &mut Vector2I,
        a_pt_b: &mut Vector2I,
        a_dist_sq: &mut i64,
    ) -> bool {
        *a_dist_sq = i64::MAX;
        let circle = Circle::new(self.get_center(), f64_to_i32_x86(self.get_radius()));

        // First check for intersections on the circle
        let intersections = circle.intersect_seg(a_seg);

        for pt in intersections {
            if self.slice_contains_point(pt) {
                *a_pt_a = pt;
                *a_pt_b = pt;
                *a_dist_sq = 0;
                return true;
            }
        }

        // Check the endpoints of the segment against the nearest point on the arc
        for pt in [a_seg.a, a_seg.b] {
            if self.slice_contains_point(pt) {
                let nearest_pt = circle.nearest_point(pt);
                let dist_sq: i64 = pt.squared_distance(nearest_pt);

                if dist_sq < *a_dist_sq {
                    *a_dist_sq = dist_sq;
                    *a_pt_a = nearest_pt;
                    *a_pt_b = pt;
                }
            }
        }

        // Check the endpoints of the arc against the nearest point on the segment
        for pt in [self.start, self.end] {
            let nearest_pt = a_seg.nearest_point(pt);
            let dist_sq: i64 = pt.squared_distance(nearest_pt);

            if dist_sq < *a_dist_sq {
                *a_dist_sq = dist_sq;
                *a_pt_a = pt;
                *a_pt_b = nearest_pt;
            }
        }

        // Check the closest points on the segment to the circle (for segments outside the arc)
        let seg_nearest_pt = a_seg.nearest_point(self.get_center());

        if self.slice_contains_point(seg_nearest_pt) {
            let circle_nearest_pt = circle.nearest_point(seg_nearest_pt);
            let dist_sq: i64 = seg_nearest_pt.squared_distance(circle_nearest_pt);

            if dist_sq < *a_dist_sq {
                *a_dist_sq = dist_sq;
                *a_pt_a = seg_nearest_pt;
                *a_pt_b = circle_nearest_pt;
            }
        }

        // Adjust point A by half the arc width towards point B
        let dir = (*a_pt_b - *a_pt_a).resize(self.get_width() / 2);
        *a_pt_a += dir;

        if *a_dist_sq < Seg::square(self.get_width() / 2) {
            *a_dist_sq = 0;
        } else {
            *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);
        }

        true
    }

    /// Port of `SHAPE_ARC::NearestPoints( const SHAPE_RECT&, ... )`: closest points between this
    /// arc and `a_rect`.
    ///
    /// The C++ defers to `SHAPE::NearestPoints()` (`shape_nearest_points.cpp`, package G5),
    /// reached here through [`shape_collisions::nearest_points`]; see `PORT_STATUS.md`.
    pub fn nearest_points_rect(
        &self,
        a_rect: &ShapeRect,
        a_pt_a: &mut Vector2I,
        a_pt_b: &mut Vector2I,
        a_dist_sq: &mut i64,
    ) -> bool {
        let line_chain = a_rect.outline();

        // Reverse the output points to match the rect_outline/arc order
        shape_collisions::nearest_points(
            ShapeRef::LineChain(&line_chain),
            ShapeRef::Arc(self),
            a_pt_b,
            a_pt_a,
        );
        *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);
        true
    }

    /// Port of `SHAPE_ARC::NearestPoints( const SHAPE_ARC&, ... )`: closest points between this
    /// arc and `a_arc` (in/out parameters as in C++).
    pub fn nearest_points_arc(
        &self,
        a_arc: &ShapeArc,
        a_pt_a: &mut Vector2I,
        a_pt_b: &mut Vector2I,
        a_dist_sq: &mut i64,
    ) -> bool {
        let adjust_for_arc_widths =
            |a_pt_a: &mut Vector2I, a_pt_b: &mut Vector2I, a_dist_sq: &mut i64| {
                // Adjust point A by half the arc-width towards point B
                let mut dir = (*a_pt_b - *a_pt_a).resize(self.get_width() / 2);
                *a_pt_a += dir;

                // Adjust point B by half the other arc-width towards point A
                dir = (*a_pt_a - *a_pt_b).resize(a_arc.get_width() / 2);
                *a_pt_b += dir;

                if *a_dist_sq < Seg::square(self.get_width() / 2 + a_arc.get_width() / 2) {
                    *a_dist_sq = 0;
                } else {
                    *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);
                }
            };

        *a_dist_sq = i64::MAX;

        let center1 = self.get_center();
        let center2 = a_arc.get_center();

        // Centers aren't exact, so center_dist_sq won't be exact either
        let center_dist_sq: i64 = center1.squared_distance(center2);
        let center_epsilon: i64 =
            ki_round(crate::math::util::std_min(self.radius, a_arc.get_radius()) / 1000.0) as i64;
        let colocated = center_dist_sq < center_epsilon * center_epsilon;

        // Start by checking endpoints
        let pts1 = [self.start, self.end];
        let pts2 = [a_arc.get_p0(), a_arc.get_p1()];

        for pt1 in pts1 {
            for pt2 in pts2 {
                let dist_sq: i64 = pt1.squared_distance(pt2);

                if dist_sq < *a_dist_sq {
                    *a_dist_sq = dist_sq;
                    *a_pt_a = pt1;
                    *a_pt_b = pt2;

                    if *a_dist_sq == 0 {
                        return true;
                    }
                }
            }
        }

        for pt in pts1 {
            if a_arc.slice_contains_point(pt) {
                let circle = Circle::new(center2, f64_to_i32_x86(a_arc.get_radius()));
                *a_pt_a = pt;
                *a_pt_b = circle.nearest_point(pt);
                *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);

                if colocated || *a_dist_sq == 0 {
                    if *a_dist_sq != 0 {
                        adjust_for_arc_widths(a_pt_a, a_pt_b, a_dist_sq);
                    }

                    return true;
                }
            }
        }

        for pt in pts2 {
            if self.slice_contains_point(pt) {
                let circle = Circle::new(center1, f64_to_i32_x86(self.get_radius()));
                *a_pt_a = circle.nearest_point(pt);
                *a_pt_b = pt;
                *a_dist_sq = a_pt_a.squared_distance(*a_pt_b);

                if colocated || *a_dist_sq == 0 {
                    if *a_dist_sq != 0 {
                        adjust_for_arc_widths(a_pt_a, a_pt_b, a_dist_sq);
                    }

                    return true;
                }
            }
        }

        // The remaining checks are require the arcs to be on non-concentric circles
        if colocated {
            return true;
        }

        let circle1 = Circle::new(center1, f64_to_i32_x86(self.get_radius()));
        let circle2 = Circle::new(center2, f64_to_i32_x86(a_arc.get_radius()));

        // First check for intersections on the circles
        let intersections = circle1.intersect(&circle2);

        for pt in intersections {
            if self.slice_contains_point(pt) && a_arc.slice_contains_point(pt) {
                *a_pt_a = pt;
                *a_pt_b = pt;
                *a_dist_sq = 0;
                return true;
            }
        }

        // Closest pair of points on the two full circles. For external the pair
        // faces each other between the centers, so each is the nearest point on
        // its circle to the other center. For one circle strictly inside the
        // other the pair lies on the same side, so the outer circle's pt is
        // nearest to the inner center and the inner circle's pt is furthest
        // from the outer center. Intersecting is handled by Intersect() above.
        let r1: f64 = self.get_radius();
        let r2: f64 = a_arc.get_radius();
        let contained = (center_dist_sq as f64) < (r1 - r2) * (r1 - r2);

        let pt1: Vector2I;
        let pt2: Vector2I;

        if contained && r1 > r2 {
            pt1 = circle1.nearest_point(center2);
            pt2 = circle2.furthest_point(center1);
        } else if contained {
            pt1 = circle1.furthest_point(center2);
            pt2 = circle2.nearest_point(center1);
        } else {
            pt1 = circle1.nearest_point(center2);
            pt2 = circle2.nearest_point(center1);
        }

        let pt1_in_slice = self.slice_contains_point(pt1);
        let pt2_in_slice = a_arc.slice_contains_point(pt2);

        if pt1_in_slice && pt2_in_slice {
            let dist_sq: i64 = pt1.squared_distance(pt2);

            if dist_sq < *a_dist_sq {
                *a_dist_sq = dist_sq;
                *a_pt_a = pt1;
                *a_pt_b = pt2;
            }

            adjust_for_arc_widths(a_pt_a, a_pt_b, a_dist_sq);
            return true;
        }

        // Check the endpoints of arc 1 against the nearest point on arc 2
        if pt2_in_slice {
            for pt in pts1 {
                let dist_sq: i64 = pt.squared_distance(pt2);

                if dist_sq < *a_dist_sq {
                    *a_dist_sq = dist_sq;
                    *a_pt_a = pt;
                    *a_pt_b = pt2;
                }
            }
        }

        // Check the endpoints of arc 2 against the nearest point on arc 1
        if pt1_in_slice {
            for pt in pts2 {
                let dist_sq: i64 = pt.squared_distance(pt1);

                if dist_sq < *a_dist_sq {
                    *a_dist_sq = dist_sq;
                    *a_pt_a = pt1;
                    *a_pt_b = pt;
                }
            }
        }

        adjust_for_arc_widths(a_pt_a, a_pt_b, a_dist_sq);
        true
    }

    /// Port of `SHAPE_ARC::GetStartAngle()`: the start angle, normalized to 0..360 degrees.
    pub fn get_start_angle(&self) -> EdaAngle {
        let center = Vector2L::from(self.get_center());
        let mut angle = EdaAngle::from_vector(Vector2D::from_vec(self.start - center));
        angle.normalize()
    }

    /// Port of `SHAPE_ARC::GetEndAngle()`: the end angle, normalized to 0..360 degrees.
    pub fn get_end_angle(&self) -> EdaAngle {
        let center = Vector2L::from(self.get_center());
        let mut angle = EdaAngle::from_vector(Vector2D::from_vec(self.end - center));
        angle.normalize()
    }

    /// Port of `SHAPE_ARC::GetLength()`: the length of the arc.
    pub fn get_length(&self) -> f64 {
        let included_angle = self.get_central_angle();

        // A straight arc sweeps nothing, so radius times angle would report zero for a run of
        // real length.  The chord is all the length it has
        if included_angle == ANGLE_0 {
            return self.get_chord().length() as f64;
        }

        (self.get_radius() * included_angle.as_radians()).abs()
    }

    /// Port of `SHAPE_ARC::GetCentralAngle()`: the "central angle" of the arc (the angle at the
    /// point of the "pie slice"), normalized between -360..360 degrees.
    pub fn get_central_angle(&self) -> EdaAngle {
        // Arcs with same start and end points can be 0 deg or 360 deg arcs.
        // However, they are expected to be circles.
        // So return 360 degrees as central arc:
        if self.start == self.end {
            return ANGLE_360;
        }

        // A straight arc has no circumcircle, so CalcArcCenter() hands back a bounded stand-in and
        // any angle measured about it is fabricated.  The chord midpoint stand-in in particular
        // puts start and end antipodal, which reads as a clean half turn
        if self.is_effective_line() {
            return ANGLE_0;
        }

        let center = Vector2L::from(self.get_center());
        let mut angle = EdaAngle::from_vector(Vector2D::from_vec(self.end - center))
            - EdaAngle::from_vector(Vector2D::from_vec(self.start - center));

        // Using only m_start and m_end arc points to calculate the central arc is not enough
        // there are 2 arcs having the same center and end points.
        // Using the middle point is mandatory to know what arc is the right one.
        // IsCCW() uses m_start, m_middle and m_end arc points to know the arc orientation
        if self.is_ccw() {
            if angle < ANGLE_0 {
                angle += ANGLE_360;
            }
        } else if angle > ANGLE_0 {
            angle -= ANGLE_360;
        }

        angle
    }

    /// Port of `SHAPE_ARC::ConvertToPolyline()` with the default `aActualError = nullptr`:
    /// a polyline approximating the arc with a maximum divergence of `a_max_error`.
    pub fn convert_to_polyline(&self, a_max_error: i32) -> ShapeLineChain {
        self.convert_to_polyline_with_error(a_max_error).0
    }

    /// Port of `SHAPE_ARC::ConvertToPolyline()` with an `aActualError` out-parameter: also
    /// returns the actual divergence from the true arc (the approximation error is between
    /// -actual/2 and +actual/2).
    pub fn convert_to_polyline_with_error(&self, a_max_error: i32) -> (ShapeLineChain, i32) {
        let mut rv = ShapeLineChain::new();
        let mut r: f64 = self.get_radius();
        let sa = self.get_start_angle();
        let c = self.get_center();
        let ca = self.get_central_angle();

        let start_to_end = Seg::new(self.get_p0(), self.get_p1());
        let half_max_error: f64 = crate::math::util::std_max(1.0, a_max_error as f64 / 2.0);

        let mut n: i32;

        // To calculate the arc to segment count, use the external radius instead of the radius.
        // for a arc with small radius and large width, the difference can be significant
        let external_radius: f64 = r + (self.width as f64 / 2.0);
        let effective_error: f64;

        // A zero sweep would divide by zero below, and there is no curve left to approximate anyway
        if external_radius < half_max_error
            || ca == ANGLE_0
            || (start_to_end.distance(self.get_arc_mid()) as f64) < half_max_error
        {
            // Should be a very rare case
            // In this case, the arc is approximated by one segment, with a effective error
            // between -aMaxError/2 and +aMaxError/2, as expected.
            n = 0;
            effective_error = external_radius;
        } else {
            n = get_arc_to_segment_count(f64_to_i32_x86(external_radius), a_max_error, ca);

            // Recalculate the effective error of approximation, that can be < aMaxError
            let seg360: i32 = f64_to_i32_x86(n as f64 * 360.0 / ca.as_degrees().abs());
            effective_error =
                circle_to_end_segment_delta_radius(f64_to_i32_x86(external_radius), seg360) as f64;
        }

        // Split the error on either side of the arc.  Since we want the start and end points
        // to be exactly on the arc, the first and last segments need to be shorter to stay within
        // the error band (since segments normally start 1/2 the error band outside the arc).
        r += effective_error / 2.0;
        n *= 2;

        rv.append_point(self.start, false);

        let mut i = 1;

        while i < n {
            let mut a = sa;

            if n != 0 {
                a += (ca * i) / n;
            }

            let x: f64 = c.x as f64 + r * a.cos();
            let y: f64 = c.y as f64 + r * a.sin();

            rv.append_xy(ki_round(x), ki_round(y), false);
            i += 2;
        }

        rv.append_point(self.end, false);

        (rv, ki_round(effective_error))
    }

    /// Port of `SHAPE_ARC::Mirror( const VECTOR2I&, FLIP_DIRECTION )`.
    pub fn mirror(&mut self, a_vector: Vector2I, a_flip_direction: FlipDirection) {
        if a_flip_direction == FlipDirection::LeftRight {
            self.start.x = -self.start.x + 2 * a_vector.x;
            self.end.x = -self.end.x + 2 * a_vector.x;
            self.mid.x = -self.mid.x + 2 * a_vector.x;
        } else {
            self.start.y = -self.start.y + 2 * a_vector.y;
            self.end.y = -self.end.y + 2 * a_vector.y;
            self.mid.y = -self.mid.y + 2 * a_vector.y;
        }

        self.update_values();
    }

    /// Port of `SHAPE_ARC::Mirror( const SEG& axis )`.
    pub fn mirror_seg(&mut self, axis: &Seg) {
        self.start = axis.reflect_point(self.start);
        self.end = axis.reflect_point(self.end);
        self.mid = axis.reflect_point(self.mid);

        self.update_values();
    }

    /// Port of `SHAPE_ARC::Reverse()`: swaps start and end (the calculated values are kept, as
    /// in C++).
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.start, &mut self.end);
    }

    /// Port of `SHAPE_ARC::Reversed()`.
    pub fn reversed(&self) -> ShapeArc {
        ShapeArc::from_start_mid_end(self.end, self.mid, self.start, self.width)
    }

    /// Port of `SHAPE_ARC::sliceContainsPoint()`: whether the direction from the center to `p`
    /// lies within the angular range of the arc.
    pub fn slice_contains_point(&self, p: Vector2I) -> bool {
        let sa = self.get_start_angle().normalize();
        let ca = self.get_central_angle();
        let ea = sa + ca;

        let mut phi = EdaAngle::from_vector(p - self.get_center()); // Angle from center to the point
        phi.normalize();

        if ca >= ANGLE_0 {
            while phi < sa {
                phi += ANGLE_360;
            }

            phi >= sa && phi <= ea
        } else {
            while phi > sa {
                phi -= ANGLE_360;
            }

            phi <= sa && phi >= ea
        }
    }
}

impl Shape for ShapeArc {
    fn type_(&self) -> ShapeType {
        ShapeType::Arc
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Arc(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    /// Port of `SHAPE_ARC::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let mut bbox = self.bbox;

        if self.width != 0 {
            bbox.inflate(ki_round(self.width as f64 / 2.0) + 1);
        }

        if a_clearance != 0 {
            bbox.inflate(a_clearance);
        }

        bbox
    }

    /// Port of `SHAPE_ARC::Collide( const SEG&, ... )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        mut a_actual: Option<&mut i32>,
        mut a_location: Option<&mut Vector2I>,
    ) -> bool {
        let center = self.get_center();
        // The center of a nearly straight arc is clamped near the int limits, so this VECTOR2I
        // subtraction overflows in C++ (two's complement wrapping in the reference build); the
        // radius check below then catches the arc.  See deviation D11 in PORT_STATUS.md.
        let radius: f64 = Vector2D::from(wrapping_sub(center, self.start)).euclidean_norm();

        // CIRCLE and SHAPE_CIRCLE store radius as int.  When the radius exceeds representable
        // range, fall back to segment-based candidate generation to avoid integer overflow.
        if radius >= LARGE_RADIUS {
            let arc_seg1 = Seg::new(self.start, self.mid);
            let arc_seg2 = Seg::new(self.mid, self.end);

            let candidate_pts: [Vector2I; 9] = [
                a_seg.nearest_point(self.start),
                a_seg.nearest_point(self.mid),
                a_seg.nearest_point(self.end),
                arc_seg1.nearest_point(a_seg.a),
                arc_seg1.nearest_point(a_seg.b),
                arc_seg2.nearest_point(a_seg.a),
                arc_seg2.nearest_point(a_seg.b),
                a_seg.a,
                a_seg.b,
            ];

            let mut any_collides = false;

            for candidate in candidate_pts {
                let collides = self.collide_point(
                    candidate,
                    a_clearance,
                    a_actual.as_deref_mut(),
                    a_location.as_deref_mut(),
                );
                any_collides |= collides;

                if collides && a_actual.as_deref().is_none_or(|a| *a == 0) {
                    return true;
                }
            }

            return any_collides;
        }

        // SHAPE_CIRCLE( center, radius ): the double radius is truncated to int
        let circle = ShapeCircle::new(center, f64_to_i32_x86(radius));
        let clearance_sq: Ecoord = Seg::square(a_clearance);

        // Circle or at least an arc with less space remaining than the clearance
        if self.get_central_angle().as_degrees() > 180.0
            && (self.start - self.end).squared_euclidean_norm() < clearance_sq
        {
            let a_dist_sq: Ecoord = wrapping_sub(a_seg.a, center).squared_euclidean_norm();
            let b_dist_sq: Ecoord = wrapping_sub(a_seg.b, center).squared_euclidean_norm();
            // SEG::Square takes an int: the double difference is truncated
            let radius_sq: Ecoord = Seg::square(f64_to_i32_x86(radius - a_clearance as f64));

            if a_dist_sq < radius_sq && b_dist_sq < radius_sq {
                return false;
            }

            return circle.collide_seg(a_seg, a_clearance, a_actual, a_location);
        }

        // Possible points of the collision are:
        // 1. Intersetion of the segment with the full circle
        // 2. Closest point on the segment to the center of the circle
        // 3. Closest point on the segment to the end points of the arc
        // 4. End points of the segment

        let mut candidate_pts = circle.get_circle().intersect_seg(a_seg);

        candidate_pts.push(a_seg.nearest_point(center));
        candidate_pts.push(a_seg.nearest_point(self.start));
        candidate_pts.push(a_seg.nearest_point(self.end));
        candidate_pts.push(a_seg.a);
        candidate_pts.push(a_seg.b);

        let mut any_collides = false;

        for candidate in candidate_pts {
            let collides = self.collide_point(
                candidate,
                a_clearance,
                a_actual.as_deref_mut(),
                a_location.as_deref_mut(),
            );
            any_collides |= collides;

            if collides && a_actual.as_deref().is_none_or(|a| *a == 0) {
                return true;
            }
        }

        any_collides
    }

    /// Port of `SHAPE_ARC::Collide( const VECTOR2I&, ... )`.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        let min_dist: i32 = a_clearance + self.width / 2;
        let bbox = self.bbox(min_dist);

        // Fast check using bounding box:
        if !bbox.contains(a_p) {
            return false;
        }

        let center = Vector2L::from(self.get_center());
        let radius: f64 = Vector2D::from_vec(center - self.start).euclidean_norm();

        // CIRCLE stores radius as int.  When the radius exceeds representable range the arc is
        // nearly straight, so approximate it as two segments through the midpoint.
        if radius >= LARGE_RADIUS {
            let seg1 = Seg::new(self.start, self.mid);
            let seg2 = Seg::new(self.mid, self.end);
            let dist1: i32 = seg1.distance(a_p);
            let dist2: i32 = seg2.distance(a_p);
            let dist: i32 = dist1.min(dist2);

            if dist <= min_dist {
                if let Some(a) = a_actual {
                    *a = 0.max(dist - self.width / 2);
                }

                if let Some(l) = a_location {
                    *l = if dist1 <= dist2 {
                        seg1.nearest_point(a_p)
                    } else {
                        seg2.nearest_point(a_p)
                    };
                }

                return true;
            }

            return false;
        }

        // CIRCLE( center, radius ): VECTOR2L to VECTOR2I conversion, double radius truncated
        let full_circle = Circle::new(Vector2I::from_vec(center), f64_to_i32_x86(radius));
        let mut nearest_pt: Vector2D = full_circle.nearest_point_d(Vector2D::from(a_p));
        let mut dist: i32 = ki_round(nearest_pt.distance(Vector2D::from(a_p)));
        let mut angle_to_pt = EdaAngle::from_vector(a_p - full_circle.center); // Angle from center to the point

        if dist == 0 {
            // Be sure to keep the sqrt of the squared distance instead of allowing a EuclideanNorm
            // because this trucates the distance to an integer before subtracting
            dist = ki_round(radius - ((a_p - center).squared_euclidean_norm() as f64).sqrt());
            // VECTOR2I( radius, 0 ): the double radius is truncated to int
            nearest_pt = Vector2D::from_vec(center + Vector2I::new(f64_to_i32_x86(radius), 0));
            rotate_point_around(&mut nearest_pt, Vector2D::from_vec(center), -angle_to_pt);
        }

        // If not a 360 degree arc, need to use arc angles to decide if point collides
        if self.start != self.end {
            let ccw = self.get_central_angle() > ANGLE_0;
            let rotated_pt_angle = (angle_to_pt.normalize() - self.get_start_angle()).normalize();
            let rotated_end_angle = (self.get_end_angle() - self.get_start_angle()).normalize();

            if (ccw && rotated_pt_angle > rotated_end_angle)
                || (!ccw && rotated_pt_angle < rotated_end_angle)
            {
                let dist_startpt: i32 = (a_p - self.start).euclidean_norm();
                let dist_endpt: i32 = (a_p - self.end).euclidean_norm();

                if dist_startpt < dist_endpt {
                    dist = dist_startpt;
                    nearest_pt = self.start.into();
                } else {
                    dist = dist_endpt;
                    nearest_pt = self.end.into();
                }
            }
        }

        if dist <= min_dist {
            if let Some(l) = a_location {
                // VECTOR2I = VECTOR2D: the converting constructor (clamp, truncate)
                *l = Vector2I::from_vec(nearest_pt);
            }

            if let Some(a) = a_actual {
                *a = 0.max(dist - self.width / 2);
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_ARC::TransformToPolygon()`.
    ///
    /// The C++ calls `TransformArcToPolygon()` (`convert_basic_shapes_to_polygon.cpp`), which is
    /// not ported yet: the buffer is left unchanged (see `PORT_STATUS.md`).
    fn transform_to_polygon(
        &self,
        _a_buffer: &mut ShapePolySet,
        _a_error: i32,
        _a_error_loc: ErrorLoc,
    ) {
    }

    /// Port of `SHAPE_ARC::Rotate()`: rotate the arc by a given angle about a point.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        rotate_point_around(&mut self.start, a_center, a_angle);
        rotate_point_around(&mut self.end, a_center, a_angle);
        rotate_point_around(&mut self.mid, a_center, a_angle);

        self.update_values();
    }

    fn get_start(&self) -> Vector2I {
        self.start
    }

    fn get_end(&self) -> Vector2I {
        self.end
    }

    fn get_width(&self) -> i32 {
        self.width
    }

    fn set_width(&mut self, a_width: i32) {
        self.width = a_width;
    }

    /// Port of `SHAPE_ARC::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.start += a_vector;
        self.end += a_vector;
        self.mid += a_vector;
        self.update_values();
    }

    fn is_solid(&self) -> bool {
        true
    }
}

impl Box2IMinMax {
    /// Port of `BOX2I_MINMAX( const SHAPE_ARC& aArc )`: `BOX2I_MINMAX( aArc.BBox() )`.
    pub fn from_arc(a_arc: &ShapeArc) -> Self {
        Box2IMinMax::from_box(&a_arc.bbox(0))
    }
}

impl From<&ShapeArc> for Box2IMinMax {
    fn from(a_arc: &ShapeArc) -> Self {
        Box2IMinMax::from_arc(a_arc)
    }
}
