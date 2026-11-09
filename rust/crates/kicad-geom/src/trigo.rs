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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2014 Jean-Pierre Charras, jp.charras at wanadoo.fr

//! Port of `libs/kimath/include/trigo.h` and `libs/kimath/src/trigo.cpp`: trigonometric and
//! geometric basic functions.
//!
//! Overload mapping:
//!
//! | C++ | Rust |
//! |---|---|
//! | `RotatePoint( VECTOR2I&, angle )`, `RotatePoint( VECTOR2D&, angle )` | [`rotate_point`] |
//! | `RotatePoint( VECTOR2I&, centre, angle )`, `RotatePoint( VECTOR2D&, centre, angle )` | [`rotate_point_around`] |
//! | `RotatePoint( int*, int*, angle )` | [`rotate_point_xy`] |
//! | `RotatePoint( int*, int*, cx, cy, angle )` | [`rotate_point_xy_around`] |
//! | `RotatePoint( double*, double*, angle )` | [`rotate_point_xy_d`] |
//! | `RotatePoint( double*, double*, cx, cy, angle )` | [`rotate_point_xy_around_d`] |
//! | `GetRotated( v, angle )` | [`get_rotated`] |
//! | `GetRotated( v, centre, angle )` | [`get_rotated_around`] |
//! | `CalcArcCenter( VECTOR2I, VECTOR2I, VECTOR2I )` | [`calc_arc_center`] |
//! | `CalcArcCenter( VECTOR2D, VECTOR2D, VECTOR2D )` | [`calc_arc_center_d`] |
//! | `CalcArcCenter( VECTOR2D, VECTOR2D, EDA_ANGLE )` | [`calc_arc_center_from_angle`] |
//! | `SegmentIntersectsSegment( ..., nullptr )` | [`segment_intersects_segment`] |
//! | `SegmentIntersectsSegment( ..., &pt )` | [`segment_intersects_segment_with_point`] |

use core::ops::{AddAssign, SubAssign};
use std::f64::consts::PI;

use crate::geometry::eda_angle::{ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, ANGLE_360, EdaAngle};
use crate::geometry::seg::Seg;
use crate::math::util::{ki_round, std_clamp};
use crate::math::vector2::{Vector2D, Vector2I, Vector2L};
use crate::math::wide_int::{KiInt128, cross_wide, to_double};

/// Port of `IsPointOnSegment`.
///
/// Test if `a_test_point` is on line defined by `a_seg_start` and `a_seg_end`.
///
/// This function is faster than [`test_segment_hit`] because `a_test_point` should be exactly
/// on the line.  This only works for horizontal, vertical, and 45 degree line segments.
///
/// * `a_seg_start` - The first point of the line segment.
/// * `a_seg_end` - The second point of the line segment.
/// * `a_test_point` - The point to test.
///
/// Returns true if the point is on the line segment.
pub fn is_point_on_segment(a_seg_start: Vector2I, a_seg_end: Vector2I, a_test_point: Vector2I) -> bool {
    let vect_seg: Vector2I = a_seg_end - a_seg_start; // Vector from S1 to S2
    let vect_point: Vector2I = a_test_point - a_seg_start; // Vector from S1 to P

    // Use long long here to avoid overflow in calculations
    if (vect_seg.x as i64 * vect_point.y as i64 - vect_seg.y as i64 * vect_point.x as i64) != 0 {
        return false; /* Cross product non-zero, vectors not parallel */
    }

    if (vect_seg.x as i64 * vect_point.x as i64 + vect_seg.y as i64 * vect_point.y as i64)
        < (vect_point.x as i64 * vect_point.x as i64 + vect_point.y as i64 * vect_point.y as i64)
    {
        return false; /* Point not on segment */
    }

    true
}

/// Shared implementation of the two forms of `SegmentIntersectsSegment`.
fn segment_intersects_segment_impl(
    a_p1_l1: Vector2I,
    a_p2_l1: Vector2I,
    a_p1_l2: Vector2I,
    a_p2_l2: Vector2I,
    a_want_intersection_point: bool,
) -> (bool, Option<Vector2I>) {
    // We are forced to use 64bit ints because the internal units can overflow 32bit ints when
    // multiplied with each other, the alternative would be to scale the units down (i.e. divide
    // by a fixed number).

    // Test for intersection within the bounds of both line segments using line equations of the
    // form:
    // x_k(u_k) = u_k * dX_k + x_k(0)
    // y_k(u_k) = u_k * dY_k + y_k(0)
    // with  0 <= u_k <= 1 and k = [ a, b ]

    let d_x_a: i64 = a_p2_l1.x as i64 - a_p1_l1.x as i64;
    let d_y_a: i64 = a_p2_l1.y as i64 - a_p1_l1.y as i64;
    let d_x_b: i64 = a_p2_l2.x as i64 - a_p1_l2.x as i64;
    let d_y_b: i64 = a_p2_l2.y as i64 - a_p1_l2.y as i64;
    let d_x_ab: i64 = a_p1_l2.x as i64 - a_p1_l1.x as i64;
    let d_y_ab: i64 = a_p1_l2.y as i64 - a_p1_l1.y as i64;

    let mut den: i64 = d_y_a * d_x_b - d_y_b * d_x_a;

    // Check if lines are parallel.
    if den == 0 {
        return (false, None);
    }

    let mut num_a: i64 = d_y_ab * d_x_b - d_y_b * d_x_ab;
    let mut num_b: i64 = d_y_ab * d_x_a - d_y_a * d_x_ab;

    // Only compute the intersection point if requested.
    let mut intersection_point: Option<Vector2I> = None;

    if a_want_intersection_point {
        // (The y coordinate really is computed from num_b in the C++ source.)
        let mut pt: Vector2I = a_p1_l1;
        pt.x += ki_round(d_x_a as f64 * num_a as f64 / den as f64);
        pt.y += ki_round(d_y_a as f64 * num_b as f64 / den as f64);
        intersection_point = Some(pt);
    }

    if den < 0 {
        den = -den;
        num_a = -num_a;
        num_b = -num_b;
    }

    // Test sign( u_a ) and return false if negative.
    if num_a < 0 {
        return (false, intersection_point);
    }

    // Test sign( u_b ) and return false if negative.
    if num_b < 0 {
        return (false, intersection_point);
    }

    // Test to ensure (u_a <= 1).
    if num_a > den {
        return (false, intersection_point);
    }

    // Test to ensure (u_b <= 1).
    if num_b > den {
        return (false, intersection_point);
    }

    (true, intersection_point)
}

/// Port of `SegmentIntersectsSegment` without the optional `aIntersectionPoint`.
///
/// Test if two lines intersect.
///
/// * `a_p1_l1` - The first point of the first line.
/// * `a_p2_l1` - The second point of the first line.
/// * `a_p1_l2` - The first point of the second line.
/// * `a_p2_l2` - The second point of the second line.
///
/// Returns true if the two segments defined by four points intersect.
/// (i.e. if the 2 segments have at least a common point)
pub fn segment_intersects_segment(
    a_p1_l1: Vector2I,
    a_p2_l1: Vector2I,
    a_p1_l2: Vector2I,
    a_p2_l2: Vector2I,
) -> bool {
    segment_intersects_segment_impl(a_p1_l1, a_p2_l1, a_p1_l2, a_p2_l2, false).0
}

/// Port of `SegmentIntersectsSegment` with a non-null `aIntersectionPoint`.
///
/// Returns `( intersects, point )`.  As in C++, the point is computed whenever the two lines
/// are not parallel, even if the segments themselves do not intersect (the C++ writes to
/// `*aIntersectionPoint` before the range tests); it is `None` for parallel lines, where the
/// C++ leaves the out-parameter untouched.
pub fn segment_intersects_segment_with_point(
    a_p1_l1: Vector2I,
    a_p2_l1: Vector2I,
    a_p1_l2: Vector2I,
    a_p2_l2: Vector2I,
) -> (bool, Option<Vector2I>) {
    segment_intersects_segment_impl(a_p1_l1, a_p2_l1, a_p1_l2, a_p2_l2, true)
}

/// Port of `TestSegmentHit`.
///
/// Test if `a_ref_point` is with `a_dist` on the line defined by `a_start` and `a_end`.
///
/// * `a_ref_point` - reference point to test
/// * `a_start` - is the first end-point of the line segment
/// * `a_end` - is the second end-point of the line segment
/// * `a_dist` - maximum distance for hit
pub fn test_segment_hit(a_ref_point: Vector2I, a_start: Vector2I, a_end: Vector2I, a_dist: i32) -> bool {
    let mut xmin: i32 = a_start.x;
    let mut xmax: i32 = a_end.x;
    let mut ymin: i32 = a_start.y;
    let mut ymax: i32 = a_end.y;
    let delta: Vector2I = a_start - a_ref_point;

    if xmax < xmin {
        core::mem::swap(&mut xmax, &mut xmin);
    }

    if ymax < ymin {
        core::mem::swap(&mut ymax, &mut ymin);
    }

    // Check if we are outside of the bounding box.
    if (ymin - a_ref_point.y > a_dist) || (a_ref_point.y - ymax > a_dist) {
        return false;
    }

    if (xmin - a_ref_point.x > a_dist) || (a_ref_point.x - xmax > a_dist) {
        return false;
    }

    // Eliminate easy cases.
    if a_start.x == a_end.x && a_ref_point.y > ymin && a_ref_point.y < ymax {
        return delta.x.abs() <= a_dist;
    }

    if a_start.y == a_end.y && a_ref_point.x > xmin && a_ref_point.x < xmax {
        return delta.y.abs() <= a_dist;
    }

    let segment = Seg::new(a_start, a_end);

    segment.squared_distance(a_ref_point) < Seg::square(a_dist + 1)
}

/// Port of `CalcArcMid`.
///
/// Return the middle point of an arc, half-way between aStart and aEnd.
///
/// There are two possible solutions which can be found by toggling aMinArcAngle. The behavior
/// is undefined for semicircles (i.e. 180 degree arcs).
///
/// * `a_start` - The starting point of the arc (for calculating the radius).
/// * `a_end` - The end point of the arc (for determining the arc angle).
/// * `a_center` - The center point of the arc.
/// * `a_min_arc_angle` - If true, returns the point that results in the smallest arc angle
///   (the C++ default).
pub fn calc_arc_mid(a_start: Vector2I, a_end: Vector2I, a_center: Vector2I, a_min_arc_angle: bool) -> Vector2I {
    let start_vector: Vector2I = a_start - a_center;
    let end_vector: Vector2I = a_end - a_center;

    let start_angle = EdaAngle::from_vector(start_vector);
    let end_angle = EdaAngle::from_vector(end_vector);
    let mut mid_point_rot_angle: EdaAngle = (start_angle - end_angle).normalize180() / 2;

    if !a_min_arc_angle {
        mid_point_rot_angle += ANGLE_180;
    }

    let mut new_mid: Vector2I = a_start;
    rotate_point_around(&mut new_mid, a_center, mid_point_rot_angle);

    new_mid
}

/// Port of `RotatePoint( int *pX, int *pY, const EDA_ANGLE& aAngle )`.
///
/// Calculate the new point of coord coord pX, pY, for a rotation center 0, 0.
pub fn rotate_point_xy(p_x: &mut i32, p_y: &mut i32, a_angle: EdaAngle) {
    let pt: Vector2I;
    let mut angle: EdaAngle = a_angle;

    angle.normalize();

    // Cheap and dirty optimizations for 0, 90, 180, and 270 degrees.
    if angle == ANGLE_0 {
        pt = Vector2I::new(*p_x, *p_y);
    } else if angle == ANGLE_90 {
        /* sin = 1, cos = 0 */
        pt = Vector2I::new(*p_y, -*p_x);
    } else if angle == ANGLE_180 {
        /* sin = 0, cos = -1 */
        pt = Vector2I::new(-*p_x, -*p_y);
    } else if angle == ANGLE_270 {
        /* sin = -1, cos = 0 */
        pt = Vector2I::new(-*p_y, *p_x);
    } else {
        let sinus: f64 = angle.sin();
        let cosinus: f64 = angle.cos();

        pt = Vector2I::new(
            ki_round((*p_y as f64 * sinus) + (*p_x as f64 * cosinus)),
            ki_round((*p_y as f64 * cosinus) - (*p_x as f64 * sinus)),
        );
    }

    *p_x = pt.x;
    *p_y = pt.y;
}

/// Port of `RotatePoint( int *pX, int *pY, int cx, int cy, const EDA_ANGLE& aAngle )`.
///
/// Calculate the new point of coord coord pX, pY, for a rotation center cx, cy.
pub fn rotate_point_xy_around(p_x: &mut i32, p_y: &mut i32, cx: i32, cy: i32, angle: EdaAngle) {
    let mut ox: i32 = *p_x - cx;
    let mut oy: i32 = *p_y - cy;

    rotate_point_xy(&mut ox, &mut oy, angle);

    *p_x = ox + cx;
    *p_y = oy + cy;
}

/// Port of `RotatePoint( double* pX, double* pY, double cx, double cy, const EDA_ANGLE& )`.
pub fn rotate_point_xy_around_d(p_x: &mut f64, p_y: &mut f64, cx: f64, cy: f64, angle: EdaAngle) {
    let mut ox: f64 = *p_x - cx;
    let mut oy: f64 = *p_y - cy;

    rotate_point_xy_d(&mut ox, &mut oy, angle);

    *p_x = ox + cx;
    *p_y = oy + cy;
}

/// Port of `RotatePoint( double* pX, double* pY, const EDA_ANGLE& aAngle )`.
///
/// Calculate the new coord point point for a rotation center 0, 0.
pub fn rotate_point_xy_d(p_x: &mut f64, p_y: &mut f64, a_angle: EdaAngle) {
    let mut angle: EdaAngle = a_angle;
    let pt: Vector2D;

    angle.normalize();

    // Cheap and dirty optimizations for 0, 90, 180, and 270 degrees.
    if angle == ANGLE_0 {
        pt = Vector2D::new(*p_x, *p_y);
    } else if angle == ANGLE_90 {
        /* sin = 1, cos = 0 */
        pt = Vector2D::new(*p_y, -*p_x);
    } else if angle == ANGLE_180 {
        /* sin = 0, cos = -1 */
        pt = Vector2D::new(-*p_x, -*p_y);
    } else if angle == ANGLE_270 {
        /* sin = -1, cos = 0 */
        pt = Vector2D::new(-*p_y, *p_x);
    } else {
        let sinus: f64 = angle.sin();
        let cosinus: f64 = angle.cos();

        pt = Vector2D::new((*p_y * sinus) + (*p_x * cosinus), (*p_y * cosinus) - (*p_x * sinus));
    }

    *p_x = pt.x;
    *p_y = pt.y;
}

/// The point types the vector overloads of `RotatePoint` and `GetRotated` exist for
/// (`VECTOR2I` and `VECTOR2D`).
pub trait Rotatable: Copy {
    /// `RotatePoint( point, aAngle )`.
    fn rotate_point(&mut self, a_angle: EdaAngle);

    /// `RotatePoint( point, aCentre, aAngle )`.
    fn rotate_point_around(&mut self, a_centre: Self, a_angle: EdaAngle);
}

impl Rotatable for Vector2I {
    #[inline]
    fn rotate_point(&mut self, a_angle: EdaAngle) {
        rotate_point_xy(&mut self.x, &mut self.y, a_angle);
    }

    #[inline]
    fn rotate_point_around(&mut self, a_centre: Vector2I, a_angle: EdaAngle) {
        rotate_point_xy_around(&mut self.x, &mut self.y, a_centre.x, a_centre.y, a_angle);
    }
}

impl Rotatable for Vector2D {
    #[inline]
    fn rotate_point(&mut self, a_angle: EdaAngle) {
        rotate_point_xy_d(&mut self.x, &mut self.y, a_angle);
    }

    #[inline]
    fn rotate_point_around(&mut self, a_centre: Vector2D, a_angle: EdaAngle) {
        rotate_point_xy_around_d(&mut self.x, &mut self.y, a_centre.x, a_centre.y, a_angle);
    }
}

/// Port of `RotatePoint( VECTOR2I& point, const EDA_ANGLE& aAngle )` and of
/// `RotatePoint( VECTOR2D& point, const EDA_ANGLE& aAngle )`: rotate a vector in place by
/// `a_angle` (around the origin).
///
/// The rotation follows KiCad's convention: with the y axis pointing down (as on screen and
/// in board files) a positive angle turns the point counter-clockwise; 90 degrees maps
/// `( x, y )` to `( y, -x )`.
#[inline]
pub fn rotate_point<P: Rotatable>(point: &mut P, a_angle: EdaAngle) {
    point.rotate_point(a_angle);
}

/// Port of `RotatePoint( VECTOR2I& aPoint, const VECTOR2I& aCentre, const EDA_ANGLE& aAngle )`
/// and of `RotatePoint( VECTOR2D& point, const VECTOR2D& aCenter, const EDA_ANGLE& aAngle )`:
/// rotate a vector in place by `a_angle` about `a_centre`.
#[inline]
pub fn rotate_point_around<P: Rotatable>(a_point: &mut P, a_centre: P, a_angle: EdaAngle) {
    a_point.rotate_point_around(a_centre, a_angle);
}

/// Port of `GetRotated( const VECTOR2I& aVector, const EDA_ANGLE& aAngle )` and of
/// `GetRotated( const VECTOR2D& aVector, const EDA_ANGLE& aAngle )`: return a new vector that
/// is the result of rotating `a_vector` by `a_angle`.
#[inline]
pub fn get_rotated<P: Rotatable>(a_vector: P, a_angle: EdaAngle) -> P {
    let mut result: P = a_vector;
    result.rotate_point(a_angle);
    result
}

/// Port of `GetRotated( const VECTOR2I& aVector, const VECTOR2I& aCentre, const EDA_ANGLE& )`:
/// return a new vector that is the result of rotating `a_vector` by `a_angle` about
/// `a_centre`.
#[inline]
pub fn get_rotated_around<P: Rotatable>(a_vector: P, a_centre: P, a_angle: EdaAngle) -> P {
    let mut result: P = a_vector;
    result.rotate_point_around(a_centre, a_angle);
    result
}

/// Port of `CalcArcCenter( const VECTOR2D& aStart, const VECTOR2D& aEnd, const EDA_ANGLE& )`.
pub fn calc_arc_center_from_angle(a_start: Vector2D, a_end: Vector2D, a_angle: EdaAngle) -> Vector2D {
    let mut angle: EdaAngle = a_angle;
    let mut start: Vector2D = a_start;
    let mut end: Vector2D = a_end;

    if angle < ANGLE_0 {
        core::mem::swap(&mut start, &mut end);
        angle = -angle;
    }

    if angle > ANGLE_180 {
        core::mem::swap(&mut start, &mut end);
        angle = ANGLE_360 - angle;
    }

    let chord: f64 = (start - end).euclidean_norm();
    let sin_half_angle: f64 = (angle / 2.0).sin();

    // A zero arc angle has no defined center, so fall back to the chord midpoint
    if sin_half_angle == 0.0 {
        return (start + end) / 2.0;
    }

    // The center sits (chord/2) * cot(angle/2) off the chord; sqrt(r^2 - chord^2/4) cancels near 180 degrees
    let mut d: f64 = (chord / 2.0) * (angle / 2.0).cos() / sin_half_angle;

    if !d.is_finite() {
        d = 0.0;
    }

    let mut vec2: Vector2D = (end - start).resize(d);
    let vc: Vector2D = (end - start).resize(chord / 2.0);

    rotate_point(&mut vec2, -ANGLE_90);

    start + vc + vec2
}

// No unique circumcircle exists if any two of the three points coincide
// Bbox below catches all three; pairwise checks below catch just one pair
const K_CLUSTER_EXTENT: f64 = 5.0;

// A pair separated by more than integer rounding is a real, if small, arc.  This is not the
// cluster extent above: three points inside a 5 IU box are all noise, but two points 4 IU
// apart with a distant third still span a healthy triangle
const K_COINCIDENT_RADIUS: f64 = 2.0;
const K_COINCIDENT_RADIUS_SQUARED: f64 = K_COINCIDENT_RADIUS * K_COINCIDENT_RADIUS;

// Radius agreement required of a snapped center, in IU
const K_SNAP_RADIUS_TOLERANCE: f64 = 1.0;

// Stand-in radius for collinear points, whose true center is at infinity
const K_COLLINEAR_RADIUS: f64 = 1e17;

// Largest magnitude at which every integer is still a double
const K_MAX_EXACT_INTEGER: f64 = 9007199254740992.0;

// Integer arc centers are clamped to this range
const K_MIN_ARC_CENTER: f64 = i32::MIN as f64 + 100.0;
const K_MAX_ARC_CENTER: f64 = i32::MAX as f64 - 100.0;

/// Port of `std::minmax( { a, b, c } )` for three doubles, following the comparison sequence
/// of libstdc++'s `std::minmax_element` so that even unordered (NaN) input gives the same
/// result as the reference build.
#[inline]
fn minmax3(a: f64, b: f64, c: f64) -> (f64, f64) {
    let (mut min, mut max) = if b < a { (b, a) } else { (a, b) };

    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if c < min {
        min = c;
    } else if !(c < max) {
        max = c;
    }

    (min, max)
}

/// Port of `degenerateArcCenter` (anonymous namespace in `trigo.cpp`): returns the center
/// for degenerate input, `None` where the C++ returns `false`.
fn degenerate_arc_center(a_start: Vector2D, a_mid: Vector2D, a_end: Vector2D) -> Option<Vector2D> {
    let (min_x, max_x) = minmax3(a_start.x, a_mid.x, a_end.x);
    let (min_y, max_y) = minmax3(a_start.y, a_mid.y, a_end.y);

    if max_x - min_x < K_CLUSTER_EXTENT && max_y - min_y < K_CLUSTER_EXTENT {
        return Some(Vector2D::new(
            (a_start.x + a_mid.x + a_end.x) / 3.0,
            (a_start.y + a_mid.y + a_end.y) / 3.0,
        ));
    }

    let coincident =
        |a: Vector2D, b: Vector2D| -> bool { (a - b).squared_euclidean_norm() < K_COINCIDENT_RADIUS_SQUARED };

    // Two distinct points fall back to the chord midpoint, same as the diameter-arc paths below
    if coincident(a_start, a_mid) || coincident(a_mid, a_end) {
        return Some(Vector2D::new((a_start.x + a_end.x) / 2.0, (a_start.y + a_end.y) / 2.0));
    }

    if coincident(a_start, a_end) {
        return Some(Vector2D::new((a_start.x + a_mid.x) / 2.0, (a_start.y + a_mid.y) / 2.0));
    }

    None
}

/// Port of `collinearArcCenter` (anonymous namespace in `trigo.cpp`).
fn collinear_arc_center(a_start: Vector2D, a_end: Vector2D) -> Vector2D {
    let chord: Vector2D = a_end - a_start;
    let mid = Vector2D::new((a_start.x + a_end.x) / 2.0, (a_start.y + a_end.y) / 2.0);

    mid + Vector2D::new(chord.y, -chord.x).resize(K_COLLINEAR_RADIUS)
}

/// Port of `det2` (anonymous namespace in `trigo.cpp`).
///
/// Kahan's a*d - b*c, accurate to about one ulp despite the cancellation.  The C++ uses
/// `std::fma` explicitly here, hence `mul_add`.
#[inline]
fn det2(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let w: f64 = b * c;
    let e: f64 = (-b).mul_add(c, w);
    let f: f64 = a.mul_add(d, -w);

    f + e
}

/// Port of `snapArcCenter` (anonymous namespace in `trigo.cpp`).
///
/// Prefer a nice 100 nm or 10 nm center when it still lies on the circle to within a
/// nanometer.
fn snap_arc_center(a_center: Vector2D, a_start: Vector2D, a_mid: Vector2D, a_end: Vector2D) -> Vector2D {
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(a_center.x.abs() < K_MAX_EXACT_INTEGER) || !(a_center.y.abs() < K_MAX_EXACT_INTEGER) {
        return a_center;
    }

    let radii_agree = |a_candidate: Vector2D| -> bool {
        let rs: f64 = (a_candidate - a_start).euclidean_norm();
        let rm: f64 = (a_candidate - a_mid).euclidean_norm();
        let re: f64 = (a_candidate - a_end).euclidean_norm();
        let (min_r, max_r) = minmax3(rs, rm, re);

        max_r - min_r <= K_SNAP_RADIUS_TOLERANCE
    };

    for grid in [100.0, 10.0] {
        let candidate = Vector2D::new(
            (a_center.x / grid + 0.5).floor() * grid,
            (a_center.y / grid + 0.5).floor() * grid,
        );

        if radii_agree(candidate) {
            return candidate;
        }
    }

    a_center
}

/// Port of `unclampedArcCenter` (anonymous namespace in `trigo.cpp`).
fn unclamped_arc_center(a_start: Vector2I, a_mid: Vector2I, a_end: Vector2I) -> Vector2D {
    let d_start = Vector2D::new(a_start.x as f64, a_start.y as f64);
    let d_mid = Vector2D::new(a_mid.x as f64, a_mid.y as f64);
    let d_end = Vector2D::new(a_end.x as f64, a_end.y as f64);

    if let Some(d_center) = degenerate_arc_center(d_start, d_mid, d_end) {
        return d_center;
    }

    // Deltas are exact in 64 bits and the numerators in 128, so only the final divisions round
    let b = Vector2L::new(a_mid.x as i64 - a_start.x as i64, a_mid.y as i64 - a_start.y as i64);
    let c = Vector2L::new(a_end.x as i64 - a_start.x as i64, a_end.y as i64 - a_start.y as i64);

    let b2: KiInt128 = b.x as i128 * b.x as i128 + b.y as i128 * b.y as i128;
    let c2: KiInt128 = c.x as i128 * c.x as i128 + c.y as i128 * c.y as i128;
    let d: KiInt128 = cross_wide(b, c) * 2;

    if d == 0 {
        return collinear_arc_center(d_start, d_end);
    }

    // Fold start into the numerator so the sum is rounded once
    let dd: f64 = to_double(d);
    let cx: f64 = to_double(a_start.x as i128 * d + b2 * c.y as i128 - c2 * b.y as i128) / dd;
    let cy: f64 = to_double(a_start.y as i128 * d + c2 * b.x as i128 - b2 * c.x as i128) / dd;

    snap_arc_center(Vector2D::new(cx, cy), d_start, d_mid, d_end)
}

/// Port of `CalcArcCenter( const VECTOR2D& aStart, const VECTOR2D& aMid, const VECTOR2D& aEnd )`.
///
/// Determine the center of an arc or circle given three points on its circumference.
///
/// * `a_start` - The starting point of the circle (equivalent to aEnd).
/// * `a_mid` - The point on the arc, half-way between aStart and aEnd.
/// * `a_end` - The ending point of the circle (equivalent to aStart).
///
/// Returns the center of the circle.
pub fn calc_arc_center_d(a_start: Vector2D, a_mid: Vector2D, a_end: Vector2D) -> Vector2D {
    if let Some(center) = degenerate_arc_center(a_start, a_mid, a_end) {
        return center;
    }

    // Work relative to start so the result cannot depend on where the arc sits
    let bx: f64 = a_mid.x - a_start.x;
    let by: f64 = a_mid.y - a_start.y;
    let cx: f64 = a_end.x - a_start.x;
    let cy: f64 = a_end.y - a_start.y;

    let b2: f64 = bx.mul_add(bx, by * by);
    let c2: f64 = cx.mul_add(cx, cy * cy);
    let d: f64 = 2.0 * det2(bx, by, cx, cy);

    if d == 0.0 {
        return collinear_arc_center(a_start, a_end);
    }

    let ux: f64 = det2(b2, c2, by, cy) / d;
    let uy: f64 = det2(c2, b2, cx, bx) / d;

    snap_arc_center(Vector2D::new(a_start.x + ux, a_start.y + uy), a_start, a_mid, a_end)
}

/// Port of `CalcArcCenter( const VECTOR2I& aStart, const VECTOR2I& aMid, const VECTOR2I& aEnd )`.
///
/// Determine the center of an arc or circle given three points on its circumference.
///
/// * `a_start` - The starting point of the circle (equivalent to aEnd).
/// * `a_mid` - The point on the arc, half-way between aStart and aEnd.
/// * `a_end` - The ending point of the circle (equivalent to aStart).
///
/// Returns the center of the circle.
pub fn calc_arc_center(a_start: Vector2I, a_mid: Vector2I, a_end: Vector2I) -> Vector2I {
    let d_center: Vector2D = unclamped_arc_center(a_start, a_mid, a_end);

    Vector2I::new(
        ki_round(std_clamp(d_center.x, K_MIN_ARC_CENTER, K_MAX_ARC_CENTER)),
        ki_round(std_clamp(d_center.y, K_MIN_ARC_CENTER, K_MAX_ARC_CENTER)),
    )
}

/// Port of `IsStraightArc`.
///
/// Returns true if the three points describe no arc whose center fits in VECTOR2I, which is
/// the case for collinear points.  Coincident start and end make a full circle, not a line.
pub fn is_straight_arc(a_start: Vector2I, a_mid: Vector2I, a_end: Vector2I) -> bool {
    if a_start == a_end {
        return false;
    }

    if cross_wide(
        Vector2L::from(a_mid) - Vector2L::from(a_start),
        Vector2L::from(a_end) - Vector2L::from(a_start),
    ) == 0
    {
        return true;
    }

    let center: Vector2D = unclamped_arc_center(a_start, a_mid, a_end);

    // Near-collinear points put the center outside the clamp
    !(center.x >= K_MIN_ARC_CENTER
        && center.x <= K_MAX_ARC_CENTER
        && center.y >= K_MIN_ARC_CENTER
        && center.y <= K_MAX_ARC_CENTER)
}

// These are the usual degrees <-> radians conversion routines

/// Port of `DEG2RAD`.
#[inline]
pub fn deg2rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

/// Port of `RAD2DEG`.
#[inline]
pub fn rad2deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

/// Port of `RAD2DECIDEG`: the same as [`rad2deg`] *but* works with the internal 'decidegrees'
/// unit.
#[inline]
pub fn rad2decideg(rad: f64) -> f64 {
    rad * 1800.0 / PI
}

/* These are templated over T (and not simply double) because Eeschema
is still using int for angles in some place */

/// Port of `NormalizeAnglePos<T>`.
///
/// Normalize angle to be in the 0.0 .. 360.0 range: angle is in 1/10 degrees.
#[inline]
pub fn normalize_angle_pos<T>(a_angle: T) -> T
where
    T: Copy + PartialOrd + AddAssign + SubAssign + From<i16>,
{
    let mut angle = a_angle;

    while angle < T::from(0) {
        angle += T::from(3600);
    }

    while angle >= T::from(3600) {
        angle -= T::from(3600);
    }

    angle
}

/// Port of `NORMALIZE_ANGLE_POS<T>`: [`normalize_angle_pos`] in place.
#[inline]
pub fn normalize_angle_pos_in_place<T>(a_angle: &mut T)
where
    T: Copy + PartialOrd + AddAssign + SubAssign + From<i16>,
{
    *a_angle = normalize_angle_pos(*a_angle);
}

/// Port of `NormalizeAngle180<T>`.
///
/// Normalize angle to be in the -180.0 .. 180.0 range (angle is in 1/10 degrees).
#[inline]
pub fn normalize_angle180<T>(a_angle: T) -> T
where
    T: Copy + PartialOrd + AddAssign + SubAssign + From<i16>,
{
    let mut angle = a_angle;

    while angle <= T::from(-1800) {
        angle += T::from(3600);
    }

    while angle > T::from(1800) {
        angle -= T::from(3600);
    }

    angle
}

/// Port of `InterceptsPositiveX`.
///
/// Test if an arc from `a_start_angle` to `a_end_angle` crosses the positive X axis
/// (0 degrees).
///
/// Testing is performed in the quadrant 1 to quadrant 4 direction (counter-clockwise).
///
/// * `a_start_angle` - The arc start angle in degrees.
/// * `a_end_angle` - The arc end angle in degrees.
#[inline]
pub fn intercepts_positive_x(a_start_angle: f64, a_end_angle: f64) -> bool {
    let mut end: f64 = a_end_angle;

    if a_start_angle > a_end_angle {
        end += 360.0;
    }

    a_start_angle < 360.0 && end > 360.0
}

/// Port of `InterceptsNegativeX`.
///
/// Test if an arc from `a_start_angle` to `a_end_angle` crosses the negative X axis
/// (180 degrees).
///
/// Testing is performed in the quadrant 1 to quadrant 4 direction (counter-clockwise).
///
/// * `a_start_angle` - The arc start angle in degrees.
/// * `a_end_angle` - The arc end angle in degrees.
#[inline]
pub fn intercepts_negative_x(a_start_angle: f64, a_end_angle: f64) -> bool {
    let mut end: f64 = a_end_angle;

    if a_start_angle > a_end_angle {
        end += 360.0;
    }

    a_start_angle < 180.0 && end > 180.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::eda_angle::{ANGLE_45, DEGREES_T};

    #[test]
    fn rotate_point_cardinal_angles_are_exact() {
        let p = Vector2I::new(1000, 200);

        assert_eq!(get_rotated(p, ANGLE_0), Vector2I::new(1000, 200));
        assert_eq!(get_rotated(p, ANGLE_90), Vector2I::new(200, -1000));
        assert_eq!(get_rotated(p, ANGLE_180), Vector2I::new(-1000, -200));
        assert_eq!(get_rotated(p, ANGLE_270), Vector2I::new(-200, 1000));
        assert_eq!(get_rotated(p, ANGLE_360), Vector2I::new(1000, 200));
        assert_eq!(get_rotated(p, -ANGLE_90), Vector2I::new(-200, 1000));
        assert_eq!(get_rotated(p, EdaAngle::new(450.0, DEGREES_T)), Vector2I::new(200, -1000));
        assert_eq!(get_rotated(p, EdaAngle::new(-720.0, DEGREES_T)), Vector2I::new(1000, 200));

        // Large coordinates stay exact for cardinal angles (no trip through double)
        let big = Vector2I::new(2147483647, -2147483647);
        assert_eq!(get_rotated(big, ANGLE_90), Vector2I::new(-2147483647, -2147483647));
        assert_eq!(get_rotated(big, ANGLE_180), Vector2I::new(-2147483647, 2147483647));

        let d = Vector2D::new(1.5, -2.5);
        assert_eq!(get_rotated(d, ANGLE_90), Vector2D::new(-2.5, -1.5));
        assert_eq!(get_rotated(d, ANGLE_180), Vector2D::new(-1.5, 2.5));
        assert_eq!(get_rotated(d, ANGLE_270), Vector2D::new(2.5, 1.5));
    }

    #[test]
    fn rotate_point_other_angles_round() {
        // 45 degrees uses the exact M_SQRT1_2 from EDA_ANGLE::Sin/Cos
        let p = Vector2I::new(1000, 0);
        assert_eq!(get_rotated(p, ANGLE_45), Vector2I::new(707, -707));
        assert_eq!(get_rotated(p, -ANGLE_45), Vector2I::new(707, 707));
        assert_eq!(get_rotated(Vector2I::new(0, 1000), ANGLE_45), Vector2I::new(707, 707));

        let q = Vector2I::new(1000, 0);
        let a30 = EdaAngle::new(30.0, DEGREES_T);
        assert_eq!(get_rotated(q, a30), Vector2I::new(866, -500));

        // Rotation about a centre
        assert_eq!(
            get_rotated_around(Vector2I::new(1100, 100), Vector2I::new(100, 100), ANGLE_90),
            Vector2I::new(100, -900)
        );

        let mut m = Vector2I::new(10, 0);
        rotate_point(&mut m, ANGLE_90);
        assert_eq!(m, Vector2I::new(0, -10));
        rotate_point_around(&mut m, Vector2I::new(0, 0), ANGLE_180);
        assert_eq!(m, Vector2I::new(0, 10));

        let (mut x, mut y) = (3, 4);
        rotate_point_xy(&mut x, &mut y, ANGLE_270);
        assert_eq!((x, y), (-4, 3));
        rotate_point_xy_around(&mut x, &mut y, 1, 1, ANGLE_180);
        assert_eq!((x, y), (6, -1));

        let (mut fx, mut fy) = (3.0, 4.0);
        rotate_point_xy_d(&mut fx, &mut fy, ANGLE_90);
        assert_eq!((fx, fy), (4.0, -3.0));
        rotate_point_xy_around_d(&mut fx, &mut fy, 1.0, 1.0, ANGLE_180);
        assert_eq!((fx, fy), (-2.0, 5.0));
    }

    #[test]
    fn point_on_segment() {
        let a = Vector2I::new(0, 0);
        let b = Vector2I::new(10, 10);

        assert!(is_point_on_segment(a, b, Vector2I::new(5, 5)));
        assert!(is_point_on_segment(a, b, a));
        assert!(is_point_on_segment(a, b, b));
        assert!(!is_point_on_segment(a, b, Vector2I::new(11, 11)));
        assert!(!is_point_on_segment(a, b, Vector2I::new(5, 6)));
    }

    #[test]
    fn segment_intersection() {
        let r = segment_intersects_segment_with_point(
            Vector2I::new(0, 0),
            Vector2I::new(10, 10),
            Vector2I::new(0, 10),
            Vector2I::new(10, 0),
        );
        assert_eq!(r, (true, Some(Vector2I::new(5, 5))));

        // Parallel: no intersection point is produced
        let r = segment_intersects_segment_with_point(
            Vector2I::new(0, 0),
            Vector2I::new(10, 0),
            Vector2I::new(0, 5),
            Vector2I::new(10, 5),
        );
        assert_eq!(r, (false, None));

        // The lines cross outside the segments: false, but the point is still computed
        let (hit, pt) = segment_intersects_segment_with_point(
            Vector2I::new(0, 0),
            Vector2I::new(2, 0),
            Vector2I::new(5, -5),
            Vector2I::new(5, 5),
        );
        assert!(!hit);
        assert!(pt.is_some());

        assert!(segment_intersects_segment(
            Vector2I::new(0, 0),
            Vector2I::new(10, 0),
            Vector2I::new(10, 0),
            Vector2I::new(10, 10)
        ));
    }

    #[test]
    fn segment_hit() {
        let a = Vector2I::new(0, 0);
        let b = Vector2I::new(100, 0);

        assert!(test_segment_hit(Vector2I::new(50, 5), a, b, 5));
        assert!(!test_segment_hit(Vector2I::new(50, 6), a, b, 5));
        assert!(test_segment_hit(Vector2I::new(103, 4), a, b, 5));
        assert!(!test_segment_hit(Vector2I::new(110, 0), a, b, 5));
        assert!(test_segment_hit(Vector2I::new(52, 48), a, Vector2I::new(100, 100), 3));
    }

    #[test]
    fn arc_center_simple_cases() {
        // Quarter of a circle of radius 1000 around the origin
        let c = calc_arc_center(Vector2I::new(1000, 0), Vector2I::new(707, 707), Vector2I::new(0, 1000));
        assert_eq!(c, Vector2I::new(0, 0));

        // Full circle: coincident start and end, center half-way to mid
        let c = calc_arc_center(Vector2I::new(0, 0), Vector2I::new(2000, 0), Vector2I::new(0, 0));
        assert_eq!(c, Vector2I::new(1000, 0));

        // Collinear points are clamped
        let c = calc_arc_center(Vector2I::new(0, 0), Vector2I::new(1000, 0), Vector2I::new(2000, 0));
        assert_eq!(c, Vector2I::new(1000, -2147483548));

        assert!(is_straight_arc(Vector2I::new(0, 0), Vector2I::new(1000, 0), Vector2I::new(2000, 0)));
        assert!(!is_straight_arc(Vector2I::new(1000, 0), Vector2I::new(707, 707), Vector2I::new(0, 1000)));
        assert!(!is_straight_arc(Vector2I::new(0, 0), Vector2I::new(2000, 0), Vector2I::new(0, 0)));

        let c = calc_arc_center_d(Vector2D::new(1000.0, 0.0), Vector2D::new(0.0, 1000.0), Vector2D::new(-1000.0, 0.0));
        assert_eq!(c, Vector2D::new(0.0, 0.0));

        let c = calc_arc_center_from_angle(Vector2D::new(1000.0, 0.0), Vector2D::new(-1000.0, 0.0), ANGLE_180);
        assert_eq!(c, Vector2D::new(0.0, 0.0));

        let mid = calc_arc_mid(Vector2I::new(1000, 0), Vector2I::new(0, 1000), Vector2I::new(0, 0), true);
        assert_eq!(mid, Vector2I::new(707, 707));
        let mid = calc_arc_mid(Vector2I::new(1000, 0), Vector2I::new(0, 1000), Vector2I::new(0, 0), false);
        assert_eq!(mid, Vector2I::new(-707, -707));
    }

    #[test]
    fn angle_helpers() {
        assert_eq!(deg2rad(180.0), PI);
        assert_eq!(rad2deg(PI), 180.0);
        assert_eq!(rad2decideg(PI), 1800.0);
        assert_eq!(normalize_angle_pos(-10), 3590);
        assert_eq!(normalize_angle_pos(3600), 0);
        assert_eq!(normalize_angle_pos(7250.5), 50.5);
        assert_eq!(normalize_angle180(1800), 1800);
        assert_eq!(normalize_angle180(-1800), 1800);
        assert_eq!(normalize_angle180(1801.0), -1799.0);

        let mut a = -900;
        normalize_angle_pos_in_place(&mut a);
        assert_eq!(a, 2700);
    }
}
