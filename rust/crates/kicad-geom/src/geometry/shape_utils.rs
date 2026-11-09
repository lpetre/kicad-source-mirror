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

//! Port of `libs/kimath/include/geometry/shape_utils.h` and
//! `libs/kimath/src/geometry/shape_utils.cpp` (namespace `KIGEOM`).
//!
//! `DIRECTION_45::Directions` is `crate::geometry::direction45::Directions`.  Where the C++
//! asserts on an invalid direction (`wxFAIL_MSG`, release builds go on) the port returns the
//! same default value.

use crate::geometry::circle::Circle;
use crate::geometry::direction45::Directions;
use crate::geometry::eda_angle::{ANGLE_90, ANGLE_180, EdaAngle, FULL_CIRCLE};
use crate::geometry::half_line::HalfLine;
use crate::geometry::line::Line;
use crate::geometry::point_types::{PT_CENTER, PT_QUADRANT, TypedPoint2I};
use crate::geometry::seg::Seg;
use crate::geometry::shape::Shape;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::geometry::shape_rect::ShapeRect;
use crate::math::box2::Box2I;
use crate::math::util::ki_round;
use crate::math::vector2::{Vector2D, Vector2I, lexicographical_compare};
use crate::trigo::get_rotated;

/// Port of `KIGEOM::ParameterAlong()`: position of a point along a segment (0 is `a`, 1 is `b`).
///
/// Normalises `SEG::TCoef()`, so it inherits that overflow: a segment longer than the
/// coordinate range wraps.  Zero for a degenerate segment.
pub fn parameter_along(a_seg: &Seg, a_point: Vector2I) -> f64 {
    let length = a_seg.squared_length();

    if length == 0 {
        return 0.0;
    }

    a_seg.t_coef(a_point) as f64 / length as f64
}

/// Port of `KIGEOM::NormalisedSeg()`: a segment whose start point is lexicographically not
/// greater than its end point.
pub fn normalised_seg(a_seg: &Seg) -> Seg {
    if lexicographical_compare(&a_seg.a, &a_seg.b) <= 0 {
        return *a_seg;
    }

    a_seg.reversed()
}

/// Port of `KIGEOM::GetOtherEnd()`: the end point of the segment that is _not_ the given
/// point.
pub fn get_other_end(a_seg: &Seg, a_point: Vector2I) -> Vector2I {
    if a_seg.a == a_point { a_seg.b } else { a_seg.a }
}

/// Port of `KIGEOM::GetSharedEndpoint()`: the shared end point of two segments, or `None` if
/// they are not connected end to end.
pub fn get_shared_endpoint(a_seg_a: &Seg, a_seg_b: &Seg) -> Option<Vector2I> {
    if a_seg_a.a == a_seg_b.a || a_seg_a.a == a_seg_b.b {
        return Some(a_seg_a.a);
    } else if a_seg_a.b == a_seg_b.a || a_seg_a.b == a_seg_b.b {
        return Some(a_seg_a.b);
    }

    None
}

/// Port of `KIGEOM::BoxToSegs()`: the four edges of a box, clockwise from the top left
/// (in KiCad's y-down coordinates).
pub fn box_to_segs(a_box: &Box2I) -> [Seg; 4] {
    let corners: [Vector2I; 4] = [
        Vector2I::new(a_box.left(), a_box.top()),
        Vector2I::new(a_box.right(), a_box.top()),
        Vector2I::new(a_box.right(), a_box.bottom()),
        Vector2I::new(a_box.left(), a_box.bottom()),
    ];

    [
        Seg::new(corners[0], corners[1]),
        Seg::new(corners[1], corners[2]),
        Seg::new(corners[2], corners[3]),
        Seg::new(corners[3], corners[0]),
    ]
}

/// Port of `KIGEOM::ClipHalfLineToBox()`: the part of the ray inside the box, if any.
pub fn clip_half_line_to_box(a_ray: &HalfLine, a_box: &Box2I) -> Option<Seg> {
    // Do the naive implementation - if this really is done in a tight loop,
    // the Cohen-Sutherland implementation in ClipLine could be faster, but
    // needs to be adapted to work with half-lines.
    let box_segs = box_to_segs(a_box);

    let mut pt_a: Option<Vector2I> = None;
    let mut pt_b: Option<Vector2I> = None;

    for box_seg in &box_segs {
        let Some(intersection) = a_ray.intersect_seg(box_seg) else {
            continue;
        };

        // Init the first point or eat it if it's the same
        if pt_a.is_none() || Some(intersection) == pt_a {
            pt_a = Some(intersection);
        } else {
            pt_b = Some(intersection);
        }
    }

    // If we have exactly two intersections, the ray crossed twice
    // so take the segment between the two points
    if let (Some(a), Some(b)) = (pt_a, pt_b) {
        return Some(Seg::new(a, b));
    }

    // It only crosses once, so the start is in the box. Take the segment from
    // the start point to the intersection
    if let Some(a) = pt_a {
        if a != a_ray.start() {
            return Some(Seg::new(a_ray.start(), a));
        }
    }

    // It didn't cross at all
    None
}

/// Port of `KIGEOM::ClipLineToBox()`: the part of the infinite line inside the box, if any.
pub fn clip_line_to_box(a_line: &Line, a_box: &Box2I) -> Option<Seg> {
    // As above, maybe can be optimised?
    let box_segs = box_to_segs(a_box);

    let mut pt_a: Option<Vector2I> = None;
    let mut pt_b: Option<Vector2I> = None;

    for box_seg in &box_segs {
        let intersection = a_line.intersect_seg(box_seg);

        // Reject intersections that are not on the actual box boundary
        if let Some(intersection) = intersection {
            if box_seg.contains(intersection) {
                // Init the first point or eat it if it's the same
                if pt_a.is_none() || Some(intersection) == pt_a {
                    pt_a = Some(intersection);
                } else {
                    pt_b = Some(intersection);
                }
            }
        }
    }

    // If we have exactly two intersections, we have a segment
    // (zero is no intersection, and one is a just crossing a corner exactly)
    if let (Some(a), Some(b)) = (pt_a, pt_b) {
        return Some(Seg::new(a, b));
    }

    None
}

/// Port of `KIGEOM::CollectBoxCorners()`: append the 4 corners of a box (TL, TR, BR, BL).
pub fn collect_box_corners(a_box: &Box2I, a_corners: &mut Vec<Vector2I>) {
    a_corners.push(Vector2I::new(a_box.left(), a_box.top()));
    a_corners.push(Vector2I::new(a_box.right(), a_box.top()));
    a_corners.push(Vector2I::new(a_box.right(), a_box.bottom()));
    a_corners.push(Vector2I::new(a_box.left(), a_box.bottom()));
}

/// Port of `KIGEOM::BoxToLineChain()`: a closed line chain around a box.
pub fn box_to_line_chain(a_box: &Box2I) -> ShapeLineChain {
    let mut result = ShapeLineChain::new();

    result.append_point(Vector2I::new(a_box.left(), a_box.top()), false);
    result.append_point(Vector2I::new(a_box.right(), a_box.top()), false);
    result.append_point(Vector2I::new(a_box.right(), a_box.bottom()), false);
    result.append_point(Vector2I::new(a_box.left(), a_box.bottom()), false);
    result.set_closed(true);

    result
}

/// Port of `KIGEOM::GetSegsInDirection()`: the sides of a box facing a direction (two for the
/// diagonal directions, in the order of the direction's letters).  Empty for `Last` /
/// `Undefined` (C++: `wxASSERT( false )`).
pub fn get_segs_in_direction(a_box: &Box2I, a_dir: Directions) -> Vec<Seg> {
    let (l, t, r, b) = (a_box.left(), a_box.top(), a_box.right(), a_box.bottom());
    let seg = |x0: i32, y0: i32, x1: i32, y1: i32| Seg::new(Vector2I::new(x0, y0), Vector2I::new(x1, y1));

    match a_dir {
        Directions::N => vec![seg(l, t, r, t)],
        Directions::E => vec![seg(r, t, r, b)],
        Directions::S => vec![seg(l, b, r, b)],
        Directions::W => vec![seg(l, t, l, b)],
        Directions::NE => vec![seg(l, t, r, t), seg(r, t, r, b)],
        Directions::SE => vec![seg(l, b, r, b), seg(r, t, r, b)],
        Directions::SW => vec![seg(l, b, r, b), seg(l, t, l, b)],
        Directions::NW => vec![seg(l, t, r, t), seg(l, t, l, b)],
        Directions::Last | Directions::Undefined => Vec::new(),
    }
}

/// Port of `KIGEOM::MakeArcCw90()`: a clockwise 90 degree arc whose midpoint lies in the given
/// diagonal direction from the centre.  Other directions give a default arc (C++:
/// `wxFAIL_MSG`).
pub fn make_arc_cw90(a_center: Vector2I, a_radius: i32, a_dir: Directions) -> ShapeArc {
    let start = match a_dir {
        Directions::NW => Vector2I::new(-a_radius, 0),
        Directions::NE => Vector2I::new(0, -a_radius),
        Directions::SW => Vector2I::new(0, a_radius),
        Directions::SE => Vector2I::new(a_radius, 0),
        _ => return ShapeArc::new(),
    };

    ShapeArc::from_center_start_angle(a_center, a_center + start, ANGLE_90, 0)
}

/// Port of `KIGEOM::MakeArcCw180()`: a clockwise 180 degree arc whose midpoint lies in the given
/// cardinal direction from the centre.  Other directions give a default arc (C++:
/// `wxFAIL_MSG`).
pub fn make_arc_cw180(a_center: Vector2I, a_radius: i32, a_dir: Directions) -> ShapeArc {
    let start = match a_dir {
        Directions::N => Vector2I::new(-a_radius, 0),
        Directions::E => Vector2I::new(0, -a_radius),
        Directions::S => Vector2I::new(a_radius, 0),
        Directions::W => Vector2I::new(0, a_radius),
        _ => return ShapeArc::new(),
    };

    ShapeArc::from_center_start_angle(a_center, a_center + start, ANGLE_180, 0)
}

/// Port of `KIGEOM::GetPoint()`: the point of a rectangle in a direction (side centres for
/// N/E/S/W, corners for the diagonals), moved outwards by `a_outset`.  `(0, 0)` for an invalid
/// direction (C++: `wxFAIL_MSG`).
pub fn get_point(a_rect: &ShapeRect, a_dir: Directions, a_outset: i32) -> Vector2I {
    let nw = a_rect.get_position();
    let w = a_rect.get_width();
    let h = a_rect.get_height();

    match a_dir {
        Directions::N => nw + Vector2I::new(w / 2, -a_outset),
        Directions::E => nw + Vector2I::new(w + a_outset, h / 2),
        Directions::S => nw + Vector2I::new(w / 2, h + a_outset),
        Directions::W => nw + Vector2I::new(-a_outset, h / 2),
        Directions::NW => nw + Vector2I::new(-a_outset, -a_outset),
        Directions::NE => nw + Vector2I::new(w + a_outset, -a_outset),
        Directions::SW => nw + Vector2I::new(-a_outset, h + a_outset),
        Directions::SE => nw + Vector2I::new(w + a_outset, h + a_outset),
        Directions::Last | Directions::Undefined => Vector2I::new(0, 0),
    }
}

/// Port of `KIGEOM::GetCircleKeyPoints()`: the four cardinal points of a circle and optionally
/// its centre.
pub fn get_circle_key_points(a_circle: &Circle, a_include_center: bool) -> Vec<TypedPoint2I> {
    let mut pts = Vec::new();

    if a_include_center {
        pts.push(TypedPoint2I::new(Vector2I::new(0, 0), PT_CENTER as i32));
    }

    pts.push(TypedPoint2I::new(Vector2I::new(0, a_circle.radius), PT_QUADRANT as i32));
    pts.push(TypedPoint2I::new(Vector2I::new(a_circle.radius, 0), PT_QUADRANT as i32));
    pts.push(TypedPoint2I::new(Vector2I::new(0, -a_circle.radius), PT_QUADRANT as i32));
    pts.push(TypedPoint2I::new(Vector2I::new(-a_circle.radius, 0), PT_QUADRANT as i32));

    // Shift the points to the circle center
    for pt in &mut pts {
        pt.point += a_circle.center;
    }

    pts
}

/// Port of `KIGEOM::RectifyPolygon()`: a polygon with only horizontal / vertical sides that
/// contains the original one and passes through each original corner.
pub fn rectify_polygon(a_poly: &ShapeLineChain) -> ShapeLineChain {
    let mut ra_outline = ShapeLineChain::new();

    let mut handle_segment = |a_seg: Seg| {
        let p0 = Vector2I::new(a_seg.a.x, a_seg.b.y);
        let p1 = Vector2I::new(a_seg.b.x, a_seg.a.y);

        ra_outline.append_point(a_seg.a, false);

        if !a_poly.point_inside(p0, 0, false) {
            ra_outline.append_point(p0, false);
        } else {
            ra_outline.append_point(p1, false);
        }
    };

    for i in 0..a_poly.segment_count() {
        handle_segment(a_poly.c_segment(i));
    }

    // Manually handle the last segment if not closed
    if !a_poly.is_closed() && a_poly.point_count() >= 2 {
        handle_segment(Seg::new(a_poly.c_last_point(), a_poly.c_point(0)));
    }

    ra_outline.set_closed(true);
    ra_outline.simplify(0);

    ra_outline
}

/// Port of `KIGEOM::AddHoleIfValid()`: add a hole to the last outline if it has at least 3
/// points and a non-zero area; returns whether it was added.
pub fn add_hole_if_valid(a_outline: &mut ShapePolySet, a_hole: ShapeLineChain) -> bool {
    if a_hole.point_count() < 3 || a_hole.area(true) == 0.0 {
        return false;
    }

    a_outline.add_hole(&a_hole, -1);
    true
}

/// Port of `KIGEOM::MakeRegularPolygonPoints( aCenter, aN, aPt0 )`: the corners of a regular
/// polygon from its centre, one corner and the number of sides.
pub fn make_regular_polygon_points(a_center: Vector2I, a_n: usize, a_pt0: Vector2I) -> Vec<Vector2I> {
    let pt0_from_c: Vector2D = (a_pt0 - a_center).cast::<f64>();
    let mut pts = Vec::new();

    for i in 0..a_n {
        let pt: Vector2D = get_rotated(pt0_from_c, (FULL_CIRCLE / a_n as f64) * i as f64);
        pts.push(ki_round(pt + a_center.cast::<f64>()));
    }

    pts
}

/// Port of `KIGEOM::MakeRegularPolygonPoints( aCenter, aN, aRadius, aAcrossCorners, aAngle )`:
/// a regular polygon of the given size across the corners (or flats), with one corner at
/// `a_angle` from the centre.
pub fn make_regular_polygon_points_radius(
    a_center: Vector2I,
    a_n: usize,
    a_radius: i32,
    a_across_corners: bool,
    a_angle: EdaAngle,
) -> Vec<Vector2I> {
    let mut a_radius = a_radius;

    if !a_across_corners {
        // if across flats, increase the radius
        a_radius = crate::cmath::f64_to_i32_x86(
            a_radius as f64 / (FULL_CIRCLE / (a_n * 2) as f64).cos(),
        );
    }

    let pt0 = a_center + get_rotated(Vector2I::new(a_radius, 0), a_angle);
    make_regular_polygon_points(a_center, a_n, pt0)
}

/// Port of `KIGEOM::MakeCrossSegments()`: the two segments of a cross of the given size,
/// rotated by `a_angle` (positive = counter-clockwise).
pub fn make_cross_segments(a_center: Vector2I, a_size: Vector2I, a_angle: EdaAngle) -> Vec<Seg> {
    let mut segs = Vec::new();

    let mut pt0 = a_center - get_rotated(Vector2I::new(a_size.x / 2, 0), a_angle);
    segs.push(Seg::new(pt0, a_center - (pt0 - a_center)));

    pt0 = a_center - get_rotated(Vector2I::new(0, a_size.y / 2), a_angle);
    segs.push(Seg::new(pt0, a_center - (pt0 - a_center)));

    segs
}
