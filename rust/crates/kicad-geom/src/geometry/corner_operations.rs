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

//! Port of `libs/kimath/include/geometry/corner_operations.h` and the free functions of
//! `libs/kimath/src/geometry/corner_operations.cpp` (`ComputeChamferPoints`, `ComputeDogbone`).
//! `SHAPE_POLY_SET::chamferFilletPolygon` from the same file is in `shape_poly_set.rs`.

use crate::geometry::circle::Circle;
use crate::geometry::eda_angle::{ANGLE_90, ANGLE_180, DEGREES_T, EdaAngle};
use crate::geometry::half_line::HalfLine;
use crate::geometry::seg::Seg;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_utils::{get_other_end, get_shared_endpoint};
use crate::geometry::vector_utils::points_are_in_same_direction;
use crate::math::vector2::Vector2I;
use crate::trigo::get_rotated_around;

/// Port of `CHAMFER_PARAMS`: parameters that define a simple chamfer operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChamferParams {
    /// `m_chamfer_setback_a`: chamfer set-back distance along the first line
    pub chamfer_setback_a: i32,
    /// `m_chamfer_setback_b`: chamfer set-back distance along the second line
    pub chamfer_setback_b: i32,
}

/// Port of `CHAMFER_RESULT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChamferResult {
    /// `m_chamfer`: the chamfer segment
    pub chamfer: Seg,
    /// `m_updated_seg_a`: the updated first segment (`None` if the chamfer "consumed" it)
    pub updated_seg_a: Option<Seg>,
    /// `m_updated_seg_b`: the updated second segment (`None` if the chamfer "consumed" it)
    pub updated_seg_b: Option<Seg>,
}

/// Port of `DOGBONE_RESULT`.
#[derive(Clone, Debug)]
pub struct DogboneResult {
    /// `m_arc`
    pub arc: ShapeArc,
    /// `m_updated_seg_a`: the updated first segment (`None` if the dogbone "consumed" it)
    pub updated_seg_a: Option<Seg>,
    /// `m_updated_seg_b`: the updated second segment (`None` if the dogbone "consumed" it)
    pub updated_seg_b: Option<Seg>,
    /// `m_small_arc_mouth`: if the arc mouth is smaller than the radius
    pub small_arc_mouth: bool,
}

/// Port of the file-local `GetBisectorOfCornerSegments()`: the bisector of two segments that
/// join at a corner, of length `a_length`, starting at the corner.
fn get_bisector_of_corner_segments(a_seg_a: &Seg, a_seg_b: &Seg, a_length: i32) -> Seg {
    // Use the "parallelogram" method to find the bisector

    // The intersection point of the two lines is the one that is shared by both segments
    // (the callers ensure there is one)
    let corner = get_shared_endpoint(a_seg_a, a_seg_b).unwrap_or_default();

    // Get the vector of a segment pointing away from a point
    let get_seg_vector_pointing_away_from = |a_seg: &Seg, a_point: Vector2I| -> Vector2I {
        let dist_a = (a_seg.a - a_point).euclidean_norm();
        let dist_b = (a_seg.b - a_point).euclidean_norm();

        // If A is closer the segment is already pointing away from the corner
        // If B is closer, we need to reverse the segment
        if dist_a < dist_b { a_seg.b - a_seg.a } else { a_seg.a - a_seg.b }
    };

    // The vectors have to be the same length
    let max_len = a_seg_a.length().max(a_seg_b.length());
    let a_vec_outward = get_seg_vector_pointing_away_from(a_seg_a, corner).resize(max_len);
    let b_vec_outward = get_seg_vector_pointing_away_from(a_seg_b, corner).resize(max_len);
    let bisector_outward = a_vec_outward + b_vec_outward;

    Seg::new(corner, corner + bisector_outward.resize(a_length))
}

/// Port of `ComputeChamferPoints()`: the chamfer of two segments that share an end point,
/// or `None` (no chamfer, chamfer too big, segments not coterminous).
pub fn compute_chamfer_points(a_seg_a: &Seg, a_seg_b: &Seg, a_chamfer_params: &ChamferParams) -> Option<ChamferResult> {
    let line_a_setback = a_chamfer_params.chamfer_setback_a;
    let line_b_setback = a_chamfer_params.chamfer_setback_b;

    if line_a_setback == 0 && line_b_setback == 0 {
        // No chamfer to do
        // In theory a chamfer of 0 on one side is kind-of valid (adds a collinear point)
        // so allow it (using an and above, not an or)
        return None;
    }

    if a_seg_a.length() < line_a_setback || a_seg_b.length() < line_b_setback {
        // Chamfer is too big for the line segments
        return None;
    }

    // We only support the case where the lines intersect at the end points
    // otherwise we would need to decide which inside corner to chamfer

    // Figure out which end points are the ones at the intersection
    // (None: the lines are not coterminous)
    let corner = get_shared_endpoint(a_seg_a, a_seg_b)?;

    // These are the other existing line points (the ones that are not the intersection)
    let a_end_pt = get_other_end(a_seg_a, corner);
    let b_end_pt = get_other_end(a_seg_b, corner);

    // Now, construct segment of the set-back lengths, that begins
    // at the intersection point and is parallel to each line segments
    let setback_a = Seg::new(corner, corner + (a_end_pt - corner).resize(line_a_setback));
    let setback_b = Seg::new(corner, corner + (b_end_pt - corner).resize(line_b_setback));

    // The chamfer segment then goes between the end points of the set-back segments
    let chamfer = Seg::new(setback_a.b, setback_b.b);

    // The adjusted segments go from the old end points to the chamfer ends
    let new_a = if a_end_pt != chamfer.a { Some(Seg::new(a_end_pt, chamfer.a)) } else { None };
    let new_b = if b_end_pt != chamfer.b { Some(Seg::new(b_end_pt, chamfer.b)) } else { None };

    Some(ChamferResult { chamfer, updated_seg_a: new_a, updated_seg_b: new_b })
}

/// Port of `ComputeDogbone()`: turn a sharp internal corner into a "dogbone" shape, which allows
/// it to be routed with a cutter that has a radius, and still allows a sharp external corner
/// to fit into the space.
///
/// ```text
///        _----_
/// ------/      |
///  ---------+  /
///           | |
///           | |
/// ```
pub fn compute_dogbone(a_seg_a: &Seg, a_seg_b: &Seg, a_dogbone_radius: i32, a_add_slots: bool) -> Option<DogboneResult> {
    let corner = get_shared_endpoint(a_seg_a, a_seg_b);

    // Cannot handle parallel lines
    let corner = match corner {
        Some(c) if !a_seg_a.angle(a_seg_b).is_horizontal() => c,
        _ => return None,
    };

    let bisector = get_bisector_of_corner_segments(a_seg_a, a_seg_b, a_dogbone_radius);

    // The dogbone center is the end of the bisector
    let dogbone_center = bisector.b;

    // We can find the ends of the arc by considering the corner angle,
    // but it's easier to just intersect the circle with the original segments.

    let circle = Circle::new(dogbone_center, a_dogbone_radius);

    let seg_a_intersections = circle.intersect_seg(a_seg_a);
    let seg_b_intersections = circle.intersect_seg(a_seg_b);

    // There can be a little bit of error in the intersection calculation
    const S_EPSILON: i32 = 8;

    let get_point_not_on_corner = |a_intersections: &[Vector2I], a_corner: Vector2I| -> Option<Vector2I> {
        a_intersections
            .iter()
            .copied()
            .find(|pt| a_corner.distance(*pt) > S_EPSILON as f64)
    };

    let pt_on_seg_a = get_point_not_on_corner(&seg_a_intersections, corner);
    let pt_on_seg_b = get_point_not_on_corner(&seg_b_intersections, corner);

    // The arc doesn't cross one or both of the segments
    let (Some(pt_on_seg_a), Some(pt_on_seg_b)) = (pt_on_seg_a, pt_on_seg_b) else {
        return None;
    };

    let arc = ShapeArc::from_start_mid_end(pt_on_seg_a, corner, pt_on_seg_b, 0);

    let a_other_pt_a = get_other_end(a_seg_a, corner);
    let a_other_pt_b = get_other_end(a_seg_b, corner);

    let angle_epsilon = EdaAngle::new(1e-3, DEGREES_T);
    let small_arc_mouth = arc.get_central_angle().abs() > (ANGLE_180 + angle_epsilon);

    {
        // See if we need to update the original segments
        // or if the dogbone consumed them
        let new_a = if a_other_pt_a != pt_on_seg_a { Some(Seg::new(a_other_pt_a, pt_on_seg_a)) } else { None };
        let new_b = if a_other_pt_b != pt_on_seg_b { Some(Seg::new(a_other_pt_b, pt_on_seg_b)) } else { None };

        // Nice and easy
        if !small_arc_mouth || !a_add_slots {
            return Some(DogboneResult { arc, updated_seg_a: new_a, updated_seg_b: new_b, small_arc_mouth });
        }
    }

    // If it's a small mouth, we can try to work out the minimal slot to allow

    // First the arc will be pulled back to 180 degrees
    let mut slot_arc = ShapeArc::from_start_mid_end(
        get_rotated_around(corner, dogbone_center, ANGLE_90),
        corner,
        get_rotated_around(corner, dogbone_center, -ANGLE_90),
        0,
    );

    // Make sure P0 is still the 'A' end
    if !points_are_in_same_direction(slot_arc.get_p0(), arc.get_p0(), dogbone_center) {
        slot_arc.reverse();
    }

    // Take the bisector and glue it to the arc ends
    let arc_extension_a = HalfLine::new(slot_arc.get_p0(), slot_arc.get_p0() + (dogbone_center - corner));
    let arc_extension_b = HalfLine::new(slot_arc.get_p1(), slot_arc.get_p1() + (dogbone_center - corner));

    let ext_a_intersect = arc_extension_a.intersect_seg(a_seg_a);
    let ext_b_intersect = arc_extension_b.intersect_seg(a_seg_b);

    // The arc extensions don't intersect the original segments
    let (Some(ext_a_intersect), Some(ext_b_intersect)) = (ext_a_intersect, ext_b_intersect) else {
        return None;
    };

    // See if we need to update the original segments
    // or if the dogbone consumed them
    let new_a = if a_other_pt_a != ext_a_intersect { Some(Seg::new(a_other_pt_a, ext_a_intersect)) } else { None };
    let new_b = if a_other_pt_b != ext_b_intersect { Some(Seg::new(a_other_pt_b, ext_b_intersect)) } else { None };

    Some(DogboneResult { arc: slot_arc, updated_seg_a: new_a, updated_seg_b: new_b, small_arc_mouth })
}
