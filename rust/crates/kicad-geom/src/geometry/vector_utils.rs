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

//! Port of `libs/kimath/include/geometry/vector_utils.h` and
//! `libs/kimath/src/geometry/vector_utils.cpp` (namespace `KIGEOM`).
//!
//! Supplemental functions for working with vectors and simple objects that interact with
//! vectors.

use crate::geometry::seg::Seg;
use crate::math::vector2::{Coord, Vector2, Vector2I};

/// Port of `KIGEOM::PointIsInDirection`.
///
/// Determine if a point is in a given direction from another point.
///
/// This returns true if the vector from aFrom to aPoint is within
/// 90 degrees of aDirection.
///
/// ```text
///     ------> aDirection
///
///     /-- aFrom
///    O                /-- aPoint
///                    O
/// ```
///
/// If the point is perpendicular to the direction, it is considered
/// to NOT be in the direction (e.g. both the direction and the
/// reversed direction would return false).
///
/// * `a_point` - The point to test.
/// * `a_direction` - The direction vector.
/// * `a_from` - The point to test from.
///
/// Returns true if the point is in the direction.
pub fn point_is_in_direction<T: Coord>(a_point: Vector2<T>, a_direction: Vector2<T>, a_from: Vector2<T>) -> bool {
    (a_point - a_from).dot(a_direction) > <T::Extended as Coord>::ZERO
}

/// Port of `KIGEOM::PointsAreInSameDirection`.
///
/// Check that the two given points are in the same direction from some other point.
///
/// I.e. the vectors from aFrom to aPointA and aFrom to aPointB are within 90 degrees.
pub fn points_are_in_same_direction<T: Coord>(
    a_point_a: Vector2<T>,
    a_point_b: Vector2<T>,
    a_from: Vector2<T>,
) -> bool {
    point_is_in_direction(a_point_b, a_point_a - a_from, a_from)
}

/// Port of `KIGEOM::SegIsInDirection`.
///
/// Determine if a segment's vector is within 90 degrees of a given direction.
pub fn seg_is_in_direction(a_seg: &Seg, a_direction: Vector2I) -> bool {
    point_is_in_direction(a_seg.b, a_direction, a_seg.a)
}

/// Port of `KIGEOM::PointProjectsOntoSegment`.
///
/// Determine if a point projects onto a segment.
///
/// ```text
///       /--- projects      /--- does not project
///      o                  o
///      |                  |
/// |<------------>|        x
///      aSeg
/// ```
pub fn point_projects_onto_segment(a_point: Vector2I, a_seg: &Seg) -> bool {
    // SEG::NearestPoint returns the end points if the projection is
    // outside the segment
    let projected: Vector2I = a_seg.nearest_point(a_point);

    projected != a_seg.a && projected != a_seg.b
}

/// Port of `KIGEOM::GetLengthRatioFromStart`.
///
/// Get the ratio of the vector to a point from the segment's start,
/// compared to the segment's length.
///
/// ```text
///        /--- aPoint
///  A<---+-------->B  <-- Length L
///  |    |
/// >|----|<-- Length R
/// ```
///
/// The point doesn't have to lie on the segment.
pub fn get_length_ratio_from_start(a_point: Vector2I, a_seg: &Seg) -> f64 {
    let length: f64 = a_seg.length() as f64;
    let projected_length: f64 = (a_point - a_seg.a).euclidean_norm() as f64;

    projected_length / length
}

/// Port of `KIGEOM::GetProjectedPointLengthRatio`.
///
/// Get the ratio of the vector to a point projected onto a segment
/// from the start, relative to the segment's length.
///
/// ```text
///       /--- projects
///      o
///      |
///  A<---+-------->B  <-- Length L
///  |    |
/// >|----|<-- Length R
/// ```
///
/// The ratio is R / L. IF 0, the point is at A. If 1, the point is at B.
/// It assumes the point projects onto the segment.
pub fn get_projected_point_length_ratio(a_point: Vector2I, a_seg: &Seg) -> f64 {
    let projected: Vector2I = a_seg.nearest_point(a_point);

    if projected == a_seg.a {
        return 0.0;
    }

    if projected == a_seg.b {
        return 1.0;
    }

    get_length_ratio_from_start(projected, a_seg)
}

/// Port of `KIGEOM::GetNearestEndpoint`.
///
/// Get the nearest end of a segment to a point.
///
/// If equidistant, the start point is returned.
pub fn get_nearest_endpoint(a_seg: &Seg, a_point: Vector2I) -> Vector2I {
    let dist_to_cb_start: f64 = a_seg.a.distance(a_point);
    let dist_to_cb_end: f64 = a_seg.b.distance(a_point);

    if dist_to_cb_start <= dist_to_cb_end { a_seg.a } else { a_seg.b }
}

/// Port of the file-local `RoundNearest` template.
#[inline]
const fn round_nearest(x: i32, g: i32) -> i32 {
    (x + (if x < 0 { -g / 2 } else { g / 2 })) / g * g
}

/// Port of the file-local `RoundDown` template.
#[inline]
const fn round_down(x: i32, g: i32) -> i32 {
    ((if x < 0 { x - g + 1 } else { x }) / g) * g
}

/// Port of the file-local `RoundUp` template.
#[inline]
const fn round_up(x: i32, g: i32) -> i32 {
    ((if x < 0 { x } else { x + g - 1 }) / g) * g
}

/// Port of `KIGEOM::RoundGrid`.
///
/// Round a vector to the nearest grid point in any direction.
pub fn round_grid(a_vec: Vector2I, a_grid_size: i32) -> Vector2I {
    Vector2I::new(round_nearest(a_vec.x, a_grid_size), round_nearest(a_vec.y, a_grid_size))
}

/// Port of `KIGEOM::RoundNW`.
///
/// Round a vector to the nearest grid point in the NW direction.
///
/// This means x and y are both rounded downwards (regardless of sign).
pub fn round_nw(a_vec: Vector2I, a_grid_size: i32) -> Vector2I {
    Vector2I::new(round_down(a_vec.x, a_grid_size), round_down(a_vec.y, a_grid_size))
}

/// Port of `KIGEOM::RoundSE`.
///
/// Round a vector to the nearest grid point in the SE direction.
///
/// This means x and y are both rounded upwards (regardless of sign).
pub fn round_se(a_vec: Vector2I, a_grid_size: i32) -> Vector2I {
    Vector2I::new(round_up(a_vec.x, a_grid_size), round_up(a_vec.y, a_grid_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_rounding() {
        assert_eq!(round_grid(Vector2I::new(14, -14), 10), Vector2I::new(10, -10));
        assert_eq!(round_grid(Vector2I::new(15, -15), 10), Vector2I::new(20, -20));
        assert_eq!(round_grid(Vector2I::new(0, 4), 10), Vector2I::new(0, 0));
        assert_eq!(round_nw(Vector2I::new(19, -11), 10), Vector2I::new(10, -20));
        assert_eq!(round_nw(Vector2I::new(20, -20), 10), Vector2I::new(20, -20));
        assert_eq!(round_se(Vector2I::new(11, -19), 10), Vector2I::new(20, -10));
        assert_eq!(round_se(Vector2I::new(20, -20), 10), Vector2I::new(20, -20));
    }

    #[test]
    fn direction_helpers() {
        let seg = Seg::new(Vector2I::new(0, 0), Vector2I::new(100, 0));

        assert!(seg_is_in_direction(&seg, Vector2I::new(1, 1)));
        assert!(!seg_is_in_direction(&seg, Vector2I::new(-1, 1)));
        assert!(!seg_is_in_direction(&seg, Vector2I::new(0, 1)));
        assert!(points_are_in_same_direction(
            Vector2I::new(10, 1),
            Vector2I::new(10, -1),
            Vector2I::new(0, 0)
        ));
        assert!(!points_are_in_same_direction(
            Vector2I::new(10, 1),
            Vector2I::new(-10, -1),
            Vector2I::new(0, 0)
        ));
        assert_eq!(get_projected_point_length_ratio(Vector2I::new(-5, 7), &seg), 0.0);
        assert_eq!(get_projected_point_length_ratio(Vector2I::new(500, 7), &seg), 1.0);
        assert_eq!(get_projected_point_length_ratio(Vector2I::new(25, 7), &seg), 0.25);
    }
}
