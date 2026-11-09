// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2018 Jean-Pierre Charras, jp.charras at wanadoo.fr
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

//! Port of `libs/kimath/include/convert_basic_shapes_to_polygon.h` and
//! `libs/kimath/src/convert_basic_shapes_to_polygon.cpp`: polygonal approximations of circles,
//! ovals, (rounded / chamfered) rectangles, trapezoids, arcs and rings.  Pads, tracks and zone
//! knockouts are built from these, so every vertex follows the C++ arithmetic exactly.
//!
//! | C++ | Rust |
//! |---|---|
//! | `TransformCircleToPolygon( SHAPE_LINE_CHAIN&, ... )` | [`transform_circle_to_polygon_chain`] |
//! | `TransformCircleToPolygon( SHAPE_POLY_SET&, ... )` | [`transform_circle_to_polygon`] |
//! | `ConvertArcToPolyline( ..., aCenter, aRadius, ... )` | [`convert_arc_to_polyline`] |
//! | `ConvertArcToPolyline( ..., aStart, aMid, aEnd, ... )` | [`convert_arc_to_polyline_3pt`] |
//!
//! Defaulted C++ arguments (`aMinSegCount = 0`, `aRadialOffset = 0.0`) are explicit.
//!
//! Out-of-range `double` to `int` conversions that the C++ does implicitly (`VECTOR2I::Resize`
//! of a `double`, `GetArcToSegmentCount( ..., double aAccuracy, ... )`) use
//! `cmath::f64_to_i32_x86` (PORTING.md section 9).

use crate::cmath;
use crate::geometry::approximation::{ERROR_INSIDE, ERROR_OUTSIDE, ErrorLoc};
use crate::geometry::arc_chord_params::ArcChordParams;
use crate::geometry::eda_angle::{
    ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_360, EdaAngle, FULL_CIRCLE, RADIANS_T,
};
use crate::geometry::geometry_utils::{
    circle_to_end_segment_delta_radius, get_arc_to_segment_count, get_circle_to_poly_correction,
};
use crate::geometry::seg::Seg;
use crate::geometry::shape::Shape;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::util::ki_round;
use crate::math::vector2::Vector2I;
use crate::trigo::rotate_point;

/// Port of `RECT_CHAMFER_POSITIONS::RECT_NO_CHAMFER`.
pub const RECT_NO_CHAMFER: i32 = 0;
/// Port of `RECT_CHAMFER_POSITIONS::RECT_CHAMFER_TOP_LEFT`.
pub const RECT_CHAMFER_TOP_LEFT: i32 = 1;
/// Port of `RECT_CHAMFER_POSITIONS::RECT_CHAMFER_TOP_RIGHT`.
pub const RECT_CHAMFER_TOP_RIGHT: i32 = 2;
/// Port of `RECT_CHAMFER_POSITIONS::RECT_CHAMFER_BOTTOM_LEFT`.
pub const RECT_CHAMFER_BOTTOM_LEFT: i32 = 4;
/// Port of `RECT_CHAMFER_POSITIONS::RECT_CHAMFER_BOTTOM_RIGHT`.
pub const RECT_CHAMFER_BOTTOM_RIGHT: i32 = 8;
/// Port of `RECT_CHAMFER_POSITIONS::RECT_CHAMFER_ALL`.
pub const RECT_CHAMFER_ALL: i32 = RECT_CHAMFER_BOTTOM_RIGHT
    | RECT_CHAMFER_BOTTOM_LEFT
    | RECT_CHAMFER_TOP_RIGHT
    | RECT_CHAMFER_TOP_LEFT;

/// `VECTOR2I::Resize( double )`: the implicit `double` to `int` conversion of the argument
/// (truncation; `INT_MIN` out of range like the reference build).
fn resize_d(a_vec: Vector2I, a_len: f64) -> Vector2I {
    a_vec.resize(cmath::f64_to_i32_x86(a_len))
}

/// Shared part of both `TransformCircleToPolygon` overloads: segment count, angle step and the
/// corrected radius.
fn circle_params(
    a_radius: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
    a_min_seg_count: i32,
) -> (i32, EdaAngle, i32) {
    let mut num_segs = get_arc_to_segment_count(a_radius, a_error, FULL_CIRCLE);
    num_segs = a_min_seg_count.max(num_segs);

    // Round up to 8 to make segment approximations align properly at 45-degrees
    num_segs = (num_segs + 7) / 8 * 8;

    let delta = ANGLE_360 / num_segs;
    let mut radius = a_radius;

    if a_error_loc == ERROR_OUTSIDE {
        // The outer radius should be radius+aError
        // Recalculate the actual approx error, as it can be smaller than aError
        // because numSegs is clamped to a minimal value
        let actual_delta_radius = circle_to_end_segment_delta_radius(radius, num_segs);
        radius += get_circle_to_poly_correction(actual_delta_radius);
    }

    (num_segs, delta, radius)
}

/// Port of `TransformCircleToPolygon( SHAPE_LINE_CHAIN& aBuffer, ... )`: convert a circle to a
/// closed polyline (appended to `a_buffer`).
///
/// `a_min_seg_count` is the min count of segments to approximate (C++ default 0).
pub fn transform_circle_to_polygon_chain(
    a_buffer: &mut ShapeLineChain,
    a_center: Vector2I,
    a_radius: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
    a_min_seg_count: i32,
) {
    let (_num_segs, delta, radius) = circle_params(a_radius, a_error, a_error_loc, a_min_seg_count);

    let mut angle = delta / 2;

    while angle < ANGLE_360 {
        let mut corner_position = Vector2I::new(radius, 0);
        rotate_point(&mut corner_position, angle);
        corner_position += a_center;
        a_buffer.append_xy(corner_position.x, corner_position.y, false);
        angle += delta;
    }

    a_buffer.set_closed(true);
}

/// Port of `TransformCircleToPolygon( SHAPE_POLY_SET& aBuffer, ... )`: convert a circle to a
/// polygon (a new outline of `a_buffer`; the first point is repeated at the end).
///
/// `a_min_seg_count` is the min count of segments to approximate (C++ default 0).
pub fn transform_circle_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_center: Vector2I,
    a_radius: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
    a_min_seg_count: i32,
) {
    let (_num_segs, delta, radius) = circle_params(a_radius, a_error, a_error_loc, a_min_seg_count);

    a_buffer.new_outline();

    let mut angle = delta / 2;

    while angle < ANGLE_360 {
        let mut corner_position = Vector2I::new(radius, 0);
        rotate_point(&mut corner_position, angle);
        corner_position += a_center;
        a_buffer.append(corner_position.x, corner_position.y, -1, -1, false);
        angle += delta;
    }

    // Finish circle
    let mut corner_position = Vector2I::new(radius, 0);
    rotate_point(&mut corner_position, delta / 2);
    corner_position += a_center;
    a_buffer.append(corner_position.x, corner_position.y, -1, -1, false);
}

/// Port of `TransformOvalToPolygon()`: convert an oblong shape (a segment with rounded ends)
/// to a polygon appended to `a_buffer`.
///
/// To build the polygonal shape outside the actual shape, a bigger radius is used for the
/// rounded ends; the result is then clamped to the bounding box of the segment.
pub fn transform_oval_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_start: Vector2I,
    a_end: Vector2I,
    a_width: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
    a_min_seg_count: i32,
) {
    // To build the polygonal shape outside the actual shape, we use a bigger
    // radius to build rounded ends.
    // However, the width of the segment is too big.
    // so, later, we will clamp the polygonal shape with the bounding box
    // of the segment.
    let mut radius = a_width / 2;
    let mut num_segs = get_arc_to_segment_count(radius, a_error, FULL_CIRCLE);
    num_segs = a_min_seg_count.max(num_segs);

    // Round up to 8 to make segment approximations align properly at 45-degrees
    num_segs = (num_segs + 7) / 8 * 8;

    let delta = ANGLE_360 / num_segs;

    if a_error_loc == ERROR_OUTSIDE {
        // The outer radius should be radius+aError
        // Recalculate the actual approx error, as it can be smaller than aError
        // because numSegs is clamped to a minimal value
        let actual_delta_radius = circle_to_end_segment_delta_radius(radius, num_segs);
        let correction = get_circle_to_poly_correction(actual_delta_radius);
        radius += correction;
    }

    // end point is the coordinate relative to aStart
    let mut endp = a_end - a_start;
    let mut startp = a_start;
    let mut polyshape = ShapePolySet::new();

    polyshape.new_outline();

    // normalize the position in order to have endp.x >= 0
    // it makes calculations more easy to understand
    if endp.x < 0 {
        endp = a_start - a_end;
        startp = a_end;
    }

    let delta_angle = EdaAngle::from_vector(endp);
    let seg_len = endp.euclidean_norm();

    // Compute the outlines of the segment, and creates a polygon
    // Note: the polygonal shape is built from the equivalent horizontal
    // segment starting at {0,0}, and ending at {seg_len,0}

    // add right rounded end:

    // Right arc start:
    let mut corner = Vector2I::new(seg_len, radius);
    polyshape.append(corner.x, corner.y, -1, -1, false);

    let mut angle = delta / 2;

    while angle < ANGLE_180 {
        corner = Vector2I::new(0, radius);
        rotate_point(&mut corner, angle);
        corner.x += seg_len;
        polyshape.append(corner.x, corner.y, -1, -1, false);
        angle += delta;
    }

    // Finish right arc:
    corner = Vector2I::new(seg_len, -radius);
    polyshape.append(corner.x, corner.y, -1, -1, false);

    // Left arc start:
    corner = Vector2I::new(0, -radius);
    polyshape.append(corner.x, corner.y, -1, -1, false);

    // add left rounded end:
    angle = delta / 2;

    while angle < ANGLE_180 {
        corner = Vector2I::new(0, -radius);
        rotate_point(&mut corner, angle);
        polyshape.append(corner.x, corner.y, -1, -1, false);
        angle += delta;
    }

    // Finish left arc:
    corner = Vector2I::new(0, radius);
    polyshape.append(corner.x, corner.y, -1, -1, false);

    // Now trim the edges of the polygonal shape which will be slightly outside the
    // track width.
    let mut bbox = ShapePolySet::new();
    bbox.new_outline();
    // Build the bbox (a horizontal rectangle).
    let halfwidth = a_width / 2; // Use the exact segment width for the bbox height
    corner.x = -radius - 2; // use a bbox width slightly bigger to avoid
    // creating useless corner at segment ends
    corner.y = halfwidth;
    bbox.append(corner.x, corner.y, -1, -1, false);
    corner.y = -halfwidth;
    bbox.append(corner.x, corner.y, -1, -1, false);
    corner.x = radius + seg_len + 2;
    bbox.append(corner.x, corner.y, -1, -1, false);
    corner.y = halfwidth;
    bbox.append(corner.x, corner.y, -1, -1, false);

    // Now, clamp the shape
    polyshape.boolean_intersection(&bbox);
    // Note the final polygon is a simple, convex polygon with no hole
    // due to the shape of initial polygons

    // Rotate and move the polygon to its right location
    polyshape.rotate(-delta_angle, Vector2I::new(0, 0));
    polyshape.move_(startp);

    a_buffer.append_set(&polyshape);
}

/// Port of the file-local `struct ROUNDED_CORNER`: a polygon corner with an optional radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundedCorner {
    /// `m_position`
    pub position: Vector2I,
    /// `m_radius`
    pub radius: i32,
}

impl RoundedCorner {
    /// Port of `ROUNDED_CORNER( int x, int y )` (radius 0).
    pub fn new(x: i32, y: i32) -> Self {
        RoundedCorner {
            position: Vector2I::new(x, y),
            radius: 0,
        }
    }

    /// Port of `ROUNDED_CORNER( int x, int y, int radius )`.
    pub fn with_radius(x: i32, y: i32, radius: i32) -> Self {
        RoundedCorner {
            position: Vector2I::new(x, y),
            radius,
        }
    }
}

/// Port of `CornerListToPolygon()`: build a new outline of `outline` from a corner list,
/// rounding the corners with a radius and / or inflating the polygon by `a_inflate` (>= 0).
///
/// Corner list requirements: no concave shape, corners in clockwise order, no duplicate
/// corners.  An empty list is a no-op apart from the new outline (C++: undefined behaviour).
pub fn corner_list_to_polygon(
    outline: &mut ShapePolySet,
    a_corners: &[RoundedCorner],
    a_inflate: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
) {
    debug_assert!(a_inflate >= 0);
    outline.new_outline();

    if a_corners.is_empty() {
        return;
    }

    let mut incoming = a_corners[0].position - a_corners[a_corners.len() - 1].position;
    let count = a_corners.len();

    for n in 0..count {
        let cur = a_corners[n];
        let next = a_corners[(n + 1) % count];
        let outgoing = next.position - cur.position;

        if !(a_inflate != 0 || cur.radius != 0) {
            outline.append_point(cur.position, -1, -1);
        } else {
            let mut corner_position = cur.position;
            let mut radius = cur.radius;
            let mut end_angle: EdaAngle;
            let tan_angle2: f64;

            if (incoming.x == 0 && outgoing.y == 0) || (incoming.y == 0 && outgoing.x == 0) {
                end_angle = ANGLE_90;
                tan_angle2 = 1.0;
            } else {
                let cos_num =
                    incoming.x as f64 * outgoing.x as f64 + incoming.y as f64 * outgoing.y as f64;
                let cos_den = incoming.euclidean_norm() as f64 * outgoing.euclidean_norm() as f64;
                let angle = cmath::acos(cos_num / cos_den);
                tan_angle2 = cmath::tan((std::f64::consts::PI - angle) / 2.0);
                end_angle = EdaAngle::new(angle, RADIANS_T);
            }

            if a_inflate != 0 && tan_angle2 != 0.0 {
                radius += a_inflate;
                corner_position += resize_d(incoming, a_inflate as f64 / tan_angle2)
                    + incoming.perpendicular().resize(-a_inflate);
            }

            // Ensure 16+ segments per 360deg and ensure first & last segment are the same size
            let num_segs = 16.max(get_arc_to_segment_count(radius, a_error, FULL_CIRCLE));
            let ang_delta = ANGLE_360 / num_segs;
            let mut last_seg = end_angle;

            if last_seg > ANGLE_0 {
                while last_seg > ang_delta {
                    last_seg -= ang_delta;
                }
            } else {
                while last_seg < -ang_delta {
                    last_seg += ang_delta;
                }
            }

            let mut ang_pos = if last_seg.is_zero() {
                ang_delta
            } else {
                (ang_delta + last_seg) / 2
            };

            let arc_transition_distance = if tan_angle2 > 0.0 {
                radius as f64 / tan_angle2
            } else {
                0.0
            };
            let mut arc_start = corner_position - resize_d(incoming, arc_transition_distance);
            let arc_center = arc_start + incoming.perpendicular().resize(radius);
            let mut arc_end: Vector2I;
            let arc_start_origin: Vector2I;

            if a_error_loc == ERROR_INSIDE {
                arc_end = Seg::new(corner_position, arc_center).reflect_point(arc_start);
                arc_start_origin = arc_start - arc_center;
                outline.append_point(arc_start, -1, -1);
            } else {
                // The outer radius should be radius+aError, recalculate because numSegs is clamped
                let actual_delta_radius = circle_to_end_segment_delta_radius(radius, num_segs);
                let radius_extend = get_circle_to_poly_correction(actual_delta_radius);
                arc_start += incoming.perpendicular().resize(-radius_extend);
                arc_start_origin = arc_start - arc_center;

                // To avoid "ears", we only add segments crossing/within the non-rounded outline
                // Note: outlineIn is short and must be treated as defining an infinite line
                let outline_in = Seg::new(corner_position - incoming, corner_position);
                let mut prev_pt = arc_start;
                arc_end = corner_position; // default if no points within the outline are found

                while ang_pos < end_angle {
                    let mut pt = arc_start_origin;
                    rotate_point(&mut pt, -ang_pos);
                    pt += arc_center;
                    ang_pos += ang_delta;

                    if outline_in.side(pt) > 0 {
                        let intersect = outline_in.intersect_lines(&Seg::new(prev_pt, pt));

                        // wxCHECK_RET( intersect, wxT( "No solutions exist!" ) );
                        let Some(intersect) = intersect else {
                            return;
                        };

                        outline.append_point(intersect, -1, -1);
                        outline.append_point(pt, -1, -1);
                        arc_end = Seg::new(corner_position, arc_center).reflect_point(intersect);
                        break;
                    }

                    end_angle -= ang_delta; // if skipping first, also skip last
                    prev_pt = pt;
                }
            }

            while ang_pos < end_angle {
                let mut pt = arc_start_origin;
                rotate_point(&mut pt, -ang_pos);
                outline.append_point(pt + arc_center, -1, -1);
                ang_pos += ang_delta;
            }

            outline.append_point(arc_end, -1, -1);
        }

        incoming = outgoing;
    }
}

/// Port of `CornerListRemoveDuplicates()`: remove corners equal to their successor (cyclically,
/// scanning backwards from the last one).
pub fn corner_list_remove_duplicates(a_corners: &mut Vec<RoundedCorner>) {
    if a_corners.is_empty() {
        return;
    }

    let mut prev = a_corners[0].position;

    for pos in (0..a_corners.len()).rev() {
        if a_corners[pos].position == prev {
            a_corners.remove(pos);
        } else {
            prev = a_corners[pos].position;
        }
    }
}

/// Port of `TransformTrapezoidToPolygon()`: convert a rectangle or trapezoid to a polygon
/// (appended to `a_buffer`).  This will generate at least 16 segments per circle (when using
/// inflate).
#[allow(clippy::too_many_arguments)]
pub fn transform_trapezoid_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_position: Vector2I,
    a_size: Vector2I,
    a_rotation: EdaAngle,
    a_delta_x: i32,
    a_delta_y: i32,
    a_inflate: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
) {
    let mut a_delta_x = a_delta_x;
    let mut a_delta_y = a_delta_y;
    let mut a_inflate = a_inflate;
    let mut outline = ShapePolySet::new();
    let mut size = a_size / 2;
    let mut corners: Vec<RoundedCorner> = Vec::new();

    if a_inflate < 0 {
        if a_delta_x == 0 && a_delta_y == 0 {
            // rectangle
            size.x = 1.max(size.x + a_inflate);
            size.y = 1.max(size.y + a_inflate);
        } else if a_delta_x != 0 {
            // horizontal trapezoid
            let slope = a_delta_x as f64 / size.x as f64;
            let y_shrink: i32 = ki_round(
                (cmath::hypot(size.x as f64, a_delta_x as f64) * a_inflate as f64) / size.x as f64,
            );
            size.y = 1.max(size.y + y_shrink);
            size.x = 1.max(size.x + a_inflate);
            a_delta_x = ki_round(size.x as f64 * slope);

            if a_delta_x > size.y {
                // shrinking turned the trapezoid into a triangle
                corners.reserve(3);
                corners.push(RoundedCorner::new(-size.x, -size.y - a_delta_x));
                corners.push(RoundedCorner::new(ki_round(size.y as f64 / slope), 0));
                corners.push(RoundedCorner::new(-size.x, size.y + a_delta_x));
            }
        } else {
            // vertical trapezoid
            let slope = a_delta_y as f64 / size.y as f64;
            let x_shrink: i32 = ki_round(
                (cmath::hypot(size.y as f64, a_delta_y as f64) * a_inflate as f64) / size.y as f64,
            );
            size.x = 1.max(size.x + x_shrink);
            size.y = 1.max(size.y + a_inflate);
            a_delta_y = ki_round(size.y as f64 * slope);

            if a_delta_y > size.x {
                corners.reserve(3);
                corners.push(RoundedCorner::new(0, -ki_round(size.x as f64 / slope)));
                corners.push(RoundedCorner::new(size.x + a_delta_y, size.y));
                corners.push(RoundedCorner::new(-size.x - a_delta_y, size.y));
            }
        }

        a_inflate = 0;
    }

    if corners.is_empty() {
        corners.reserve(4);
        corners.push(RoundedCorner::new(-size.x + a_delta_y, -size.y - a_delta_x));
        corners.push(RoundedCorner::new(size.x - a_delta_y, -size.y + a_delta_x));
        corners.push(RoundedCorner::new(size.x + a_delta_y, size.y - a_delta_x));
        corners.push(RoundedCorner::new(-size.x - a_delta_y, size.y + a_delta_x));

        if a_delta_y.wrapping_abs() == size.x.wrapping_abs()
            || a_delta_x.wrapping_abs() == size.y.wrapping_abs()
        {
            corner_list_remove_duplicates(&mut corners);
        }
    }

    corner_list_to_polygon(&mut outline, &corners, a_inflate, a_error, a_error_loc);

    if !a_rotation.is_zero() {
        outline.rotate(a_rotation, Vector2I::new(0, 0));
    }

    outline.move_(a_position);
    a_buffer.append_set(&outline);
}

/// Port of `TransformRoundChamferedRectToPolygon()`: convert a rectangle with rounded and / or
/// chamfered corners to a polygon (appended to `a_buffer`).  Rounded corners get at least 16
/// segments per circle.
///
/// `a_chamfer_corners` is an OR of the `RECT_CHAMFER_*` flags, `a_chamfer_ratio` the ratio
/// between the smaller side and the chamfer size, `a_inflate` the shape inflation (a negative
/// value shrinks the shape).
#[allow(clippy::too_many_arguments)]
pub fn transform_round_chamfered_rect_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_position: Vector2I,
    a_size: Vector2I,
    a_rotation: EdaAngle,
    a_corner_radius: i32,
    a_chamfer_ratio: f64,
    a_chamfer_corners: i32,
    a_inflate: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
) {
    let mut a_corner_radius = a_corner_radius;
    let mut a_inflate = a_inflate;
    let mut outline = ShapePolySet::new();
    let mut size = a_size / 2;
    let chamfer_cnt = (a_chamfer_corners & 0xFF).count_ones() as usize;
    let mut chamfer_deduct = 0.0;

    if a_inflate < 0 {
        size.x = 1.max(size.x + a_inflate);
        size.y = 1.max(size.y + a_inflate);
        chamfer_deduct = a_inflate as f64 * (2.0 - std::f64::consts::SQRT_2);
        a_corner_radius = 0.max(a_corner_radius + a_inflate);
        a_inflate = 0;
    }

    let mut corners: Vec<RoundedCorner> = Vec::with_capacity(4 + chamfer_cnt);
    corners.push(RoundedCorner::with_radius(
        -size.x,
        -size.y,
        a_corner_radius,
    ));
    corners.push(RoundedCorner::with_radius(size.x, -size.y, a_corner_radius));
    corners.push(RoundedCorner::with_radius(size.x, size.y, a_corner_radius));
    corners.push(RoundedCorner::with_radius(-size.x, size.y, a_corner_radius));

    if a_chamfer_corners != 0 {
        let shorter_side = a_size.x.min(a_size.y);
        let chamfer = 0.max(ki_round(
            a_chamfer_ratio * shorter_side as f64 + chamfer_deduct,
        ));
        let cham_id: [i32; 4] = [
            RECT_CHAMFER_TOP_LEFT,
            RECT_CHAMFER_TOP_RIGHT,
            RECT_CHAMFER_BOTTOM_RIGHT,
            RECT_CHAMFER_BOTTOM_LEFT,
        ];
        let sign: [i32; 8] = [0, 1, -1, 0, 0, -1, 1, 0];

        let mut pos: usize = 0;

        for cc in 0..4i32 {
            if a_chamfer_corners & cham_id[cc as usize] != 0 {
                corners[pos].radius = 0;

                if chamfer != 0 {
                    let copy = corners[pos];
                    corners.insert(pos + 1, copy);
                    corners[pos].position.x += sign[((2 * cc) & 7) as usize] * chamfer;
                    corners[pos].position.y += sign[((2 * cc - 2) & 7) as usize] * chamfer;
                    corners[pos + 1].position.x += sign[((2 * cc + 1) & 7) as usize] * chamfer;
                    corners[pos + 1].position.y += sign[((2 * cc - 1) & 7) as usize] * chamfer;
                    pos += 1;
                }
            }

            pos += 1;
        }

        if chamfer_cnt > 1 && 2 * chamfer >= shorter_side {
            corner_list_remove_duplicates(&mut corners);
        }
    }

    corner_list_to_polygon(&mut outline, &corners, a_inflate, a_error, a_error_loc);

    if !a_rotation.is_zero() {
        outline.rotate(a_rotation, Vector2I::new(0, 0));
    }

    outline.move_(a_position);
    a_buffer.append_set(&outline);
}

/// The `append_point` lambda of the three-point `ConvertArcToPolyline`.
fn append_arc_point(
    a_polyline: &mut ShapeLineChain,
    params: &ArcChordParams,
    a_start: Vector2I,
    a_end: Vector2I,
    a_alpha: f64,
    a_effective_radius: f64,
) -> bool {
    const MAX_COORD: f64 = i32::MAX as f64;

    let u_offset = a_effective_radius * cmath::sin(a_alpha);
    let n_offset = params.center_offset() - a_effective_radius * cmath::cos(a_alpha);

    let x = params.mid_x() + params.ux() * u_offset + params.nx() * n_offset;
    let y = params.mid_y() + params.uy() * u_offset + params.ny() * n_offset;

    if !x.is_finite() || !y.is_finite() || x.abs() > MAX_COORD || y.abs() > MAX_COORD {
        a_polyline.clear();
        a_polyline.append_point(a_start, false);
        a_polyline.append_point(a_end, false);
        return false;
    }

    a_polyline.append_xy(ki_round(x), ki_round(y), false);
    true
}

/// Port of `ConvertArcToPolyline( SHAPE_LINE_CHAIN& aPolyline, const VECTOR2I& aStart,
/// const VECTOR2I& aMid, const VECTOR2I& aEnd, double aAccuracy, ERROR_LOC aErrorLoc,
/// double aRadialOffset )`: append a polyline approximating an arc defined by three points,
/// offset radially by `a_radial_offset` (C++ default 0.0).  Returns the segment count (0 for
/// a degenerate arc, which appends its end points).
pub fn convert_arc_to_polyline_3pt(
    a_polyline: &mut ShapeLineChain,
    a_start: Vector2I,
    a_mid: Vector2I,
    a_end: Vector2I,
    a_accuracy: f64,
    a_error_loc: ErrorLoc,
    a_radial_offset: f64,
) -> i32 {
    let mut params = ArcChordParams::new();

    if !params.compute(a_start, a_mid, a_end) {
        a_polyline.append_point(a_start, false);

        if a_end != a_start {
            a_polyline.append_point(a_end, false);
        }

        return 0;
    }

    let arc_angle = params.arc_angle();

    if arc_angle <= 0.0 {
        a_polyline.append_point(a_start, false);
        a_polyline.append_point(a_end, false);
        return 0;
    }

    const MAX_COORD: f64 = i32::MAX as f64;

    let effective_radius = params.radius() + a_radial_offset;

    let arc_angle_eda = EdaAngle::new(arc_angle, RADIANS_T);
    let mut n: i32 = 2;
    let radius_for_seg: i32 = ki_round(crate::math::util::std_min(
        effective_radius.abs(),
        MAX_COORD,
    ));

    if radius_for_seg as f64 >= a_accuracy {
        n = get_arc_to_segment_count(
            radius_for_seg,
            cmath::f64_to_i32_x86(a_accuracy),
            arc_angle_eda,
        ) + 1;
    }

    let half_angle = arc_angle / 2.0;
    let delta = arc_angle / n as f64;

    let ap = |poly: &mut ShapeLineChain, alpha: f64, radius: f64| {
        append_arc_point(poly, &params, a_start, a_end, alpha, radius)
    };

    if a_error_loc == ERROR_INSIDE {
        for i in 0..=n {
            if !ap(a_polyline, -half_angle + delta * i as f64, effective_radius) {
                return 0;
            }
        }
    } else {
        let seg360 = ki_round(n as f64 * 360.0 / arc_angle_eda.as_degrees()).wrapping_abs();

        if seg360 <= 0 {
            for i in 0..=n {
                if !ap(a_polyline, -half_angle + delta * i as f64, effective_radius) {
                    return 0;
                }
            }

            return n;
        }

        let delta_radius = circle_to_end_segment_delta_radius(radius_for_seg, seg360);
        let error_radius = effective_radius + delta_radius as f64;

        if !ap(a_polyline, -half_angle, effective_radius) {
            return 0;
        }

        for i in 0..n {
            if !ap(
                a_polyline,
                -half_angle + delta * (i as f64 + 0.5),
                error_radius,
            ) {
                return 0;
            }
        }

        if !ap(a_polyline, half_angle, effective_radius) {
            return 0;
        }
    }

    n
}

/// Port of `ConvertArcToPolyline( SHAPE_LINE_CHAIN& aPolyline, VECTOR2I aCenter, int aRadius,
/// const EDA_ANGLE& aStartAngle, const EDA_ANGLE& aArcAngle, double aAccuracy,
/// ERROR_LOC aErrorLoc )`: append a polyline approximating an arc given by centre, radius,
/// start angle and arc angle.  Returns the segment count.
pub fn convert_arc_to_polyline(
    a_polyline: &mut ShapeLineChain,
    a_center: Vector2I,
    a_radius: i32,
    a_start_angle: EdaAngle,
    a_arc_angle: EdaAngle,
    a_accuracy: f64,
    a_error_loc: ErrorLoc,
) -> i32 {
    let mut n: i32 = 2;

    if a_radius as f64 >= a_accuracy {
        n = get_arc_to_segment_count(a_radius, cmath::f64_to_i32_x86(a_accuracy), a_arc_angle) + 1;
    }

    let delta = a_arc_angle / n;

    if a_error_loc == ERROR_INSIDE {
        // This is the easy case: with the error on the inside the endpoints of each segment
        // are error-free.

        let mut rot = a_start_angle;

        for _ in 0..=n {
            let x = a_center.x as f64 + a_radius as f64 * rot.cos();
            let y = a_center.y as f64 + a_radius as f64 * rot.sin();

            a_polyline.append_xy(ki_round(x), ki_round(y), false);
            rot += delta;
        }
    } else {
        // This is the hard case: with the error on the outside it's the segment midpoints
        // that are error-free.  So we need to add a half-segment at each end of the arc to get
        // them correct.

        let seg360 = ki_round(n as f64 * 360.0 / a_arc_angle.as_degrees()).wrapping_abs();
        let actual_delta_radius = circle_to_end_segment_delta_radius(a_radius, seg360);
        let error_radius = a_radius + actual_delta_radius;

        let mut x = a_center.x as f64 + a_radius as f64 * a_start_angle.cos();
        let mut y = a_center.y as f64 + a_radius as f64 * a_start_angle.sin();

        a_polyline.append_xy(ki_round(x), ki_round(y), false);

        let mut rot = a_start_angle + delta / 2;

        for _ in 0..n {
            x = a_center.x as f64 + error_radius as f64 * rot.cos();
            y = a_center.y as f64 + error_radius as f64 * rot.sin();

            a_polyline.append_xy(ki_round(x), ki_round(y), false);
            rot += delta;
        }

        x = a_center.x as f64 + a_radius as f64 * (a_start_angle + a_arc_angle).cos();
        y = a_center.y as f64 + a_radius as f64 * (a_start_angle + a_arc_angle).sin();

        a_polyline.append_xy(ki_round(x), ki_round(y), false);
    }

    n
}

/// Port of `TransformArcToPolygon()`: convert a thick arc (start, mid, end, width) to a polygon
/// appended to `a_buffer`.
pub fn transform_arc_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_start: Vector2I,
    a_mid: Vector2I,
    a_end: Vector2I,
    a_width: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
) {
    let start_to_end = Seg::new(a_start, a_end);
    let distance_to_mid = start_to_end.distance(a_mid);

    if distance_to_mid <= 1 {
        // Not an arc but essentially a straight line with a small error
        transform_oval_to_polygon(
            a_buffer,
            a_start,
            a_end,
            a_width + distance_to_mid,
            a_error,
            a_error_loc,
            0,
        );
        return;
    }

    // Determine arc direction using cross product. For consistent polygon winding,
    // ensure we always process a CCW arc by swapping endpoints if needed.
    let start_to_mid = a_mid - a_start;
    let start_to_end_vec = a_end - a_start;
    let cross: i64 = start_to_mid.x as i64 * start_to_end_vec.y as i64
        - start_to_mid.y as i64 * start_to_end_vec.x as i64;

    let mut p0 = a_start;
    let mut p1 = a_end;

    if cross < 0 {
        std::mem::swap(&mut p0, &mut p1);
    }

    let mut params = ArcChordParams::new();

    if !params.compute(p0, a_mid, p1) {
        transform_oval_to_polygon(a_buffer, a_start, a_end, a_width, a_error, a_error_loc, 0);
        return;
    }

    let start_angle = params.start_angle();
    let end_angle = params.end_angle();

    let radial_offset = a_width / 2;
    let arc_inner_radius = params.radius() - radial_offset as f64;
    let error_loc_inner = if a_error_loc == ERROR_INSIDE {
        ERROR_OUTSIDE
    } else {
        ERROR_INSIDE
    };
    let error_loc_outer = if a_error_loc == ERROR_INSIDE {
        ERROR_INSIDE
    } else {
        ERROR_OUTSIDE
    };

    let mut polyshape = ShapePolySet::new();
    polyshape.new_outline();

    {
        let outline = polyshape.outline(0);

        // Starting end cap (semicircle at p0)
        convert_arc_to_polyline(
            outline,
            p0,
            radial_offset,
            start_angle - ANGLE_180,
            ANGLE_180,
            a_error as f64,
            a_error_loc,
        );

        // Outside edge
        convert_arc_to_polyline_3pt(
            outline,
            p0,
            a_mid,
            p1,
            a_error as f64,
            error_loc_outer,
            radial_offset as f64,
        );

        // Ending end cap (semicircle at p1)
        convert_arc_to_polyline(
            outline,
            p1,
            radial_offset,
            end_angle,
            ANGLE_180,
            a_error as f64,
            a_error_loc,
        );

        // Inside edge (reversed direction)
        if arc_inner_radius > 0.0 {
            convert_arc_to_polyline_3pt(
                outline,
                p1,
                a_mid,
                p0,
                a_error as f64,
                error_loc_inner,
                -radial_offset as f64,
            );
        }
    }

    a_buffer.append_set(&polyshape);
}

/// Port of `TransformRingToPolygon()`: convert a ring (circle with a width) to a polygon with a
/// hole, fractured, appended to `a_buffer`.
pub fn transform_ring_to_polygon(
    a_buffer: &mut ShapePolySet,
    a_centre: Vector2I,
    a_radius: i32,
    a_width: i32,
    a_error: i32,
    a_error_loc: ErrorLoc,
) {
    let inner_radius = a_radius - (a_width / 2);
    let outer_radius = inner_radius + a_width;

    if inner_radius <= 0 {
        //In this case, the ring is just a circle (no hole inside)
        transform_circle_to_polygon(
            a_buffer,
            a_centre,
            a_radius + (a_width / 2),
            a_error,
            a_error_loc,
            0,
        );
        return;
    }

    let mut buffer = ShapePolySet::new();

    transform_circle_to_polygon(&mut buffer, a_centre, outer_radius, a_error, a_error_loc, 0);

    // Build the hole:
    buffer.new_hole(-1);
    // The circle is the hole, so the approximation error location is the opposite of aErrorLoc
    let inner_err_loc = if a_error_loc == ERROR_OUTSIDE {
        ERROR_INSIDE
    } else {
        ERROR_OUTSIDE
    };
    transform_circle_to_polygon_chain(
        buffer.hole(0, 0),
        a_centre,
        inner_radius,
        a_error,
        inner_err_loc,
        0,
    );

    buffer.fracture(true);
    a_buffer.append_set(&buffer);
}
