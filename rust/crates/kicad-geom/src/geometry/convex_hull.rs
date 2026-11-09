// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2016 Jean-Pierre Charras, jp.charras at wanadoo.fr
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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/geometry/convex_hull.h` and
//! `libs/kimath/src/geometry/convex_hull.cpp`: Andrew's monotone chain 2D convex hull
//! algorithm (asymptotic complexity: O(n log n), see
//! http://www.algorithmist.com/index.php/Monotone_Chain_Convex_Hull).
//!
//! Pseudo-code:
//!
//! ```text
//! Input: a list P of points in the plane.
//!
//! Sort the points of P by x-coordinate (in case of a tie, sort by y-coordinate).
//!
//! Initialize U and L as empty lists.
//! The lists will hold the vertices of upper and lower hulls respectively.
//!
//! for i = 1, 2, ..., n:
//!  while L contains at least two points and the sequence of last two points
//!          of L and the point P[i] does not make a counter-clockwise turn:
//!      remove the last point from L
//!  append P[i] to L
//!
//! for i = n, n-1, ..., 1:
//!  while U contains at least two points and the sequence of last two points
//!          of U and the point P[i] does not make a counter-clockwise turn:
//!      remove the last point from U
//!  append P[i] to U
//!
//! Remove the last point of each list (it's the same as the first point of the other list).
//! Concatenate L and U to obtain the convex hull of P.
//! Points in the result will be listed in counter-clockwise order.
//! ```

use crate::geometry::eda_angle::{ANGLE_0, EdaAngle};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::vector2::Vector2I;
use crate::trigo::rotate_point;

/// `coord2_t`: must be big enough to hold 2*max(|coordinate|)^2
type Coord2T = i64;

/// `cross_product`: 2D cross product of OA and OB vectors, i.e. z-component of their 3D cross
/// product.  Returns a positive value, if OAB makes a counter-clockwise turn, negative for
/// clockwise turn, and zero if the points are collinear.  (The coordinate differences are
/// `int` arithmetic in C++ and wrap like the reference build.)
fn cross_product(o: Vector2I, a: Vector2I, b: Vector2I) -> Coord2T {
    ((a.x.wrapping_sub(o.x)) as Coord2T * (b.y.wrapping_sub(o.y)) as Coord2T)
        .wrapping_sub((a.y.wrapping_sub(o.y)) as Coord2T * (b.x.wrapping_sub(o.x)) as Coord2T)
}

/// Port of `BuildConvexHull( std::vector<VECTOR2I>& aResult, const std::vector<VECTOR2I>& aPoly )`.
///
/// Fills `a_result` with a list of points on the convex hull in counter-clockwise order.  As in
/// C++, `a_result` is left untouched when `a_poly` has fewer than 2 points.
pub fn build_convex_hull(a_result: &mut Vec<Vector2I>, a_poly: &[Vector2I]) {
    let mut poly: Vec<Vector2I> = a_poly.to_vec();
    let point_count = poly.len();

    if point_count < 2 {
        // Should not happen, but who know
        return;
    }

    // Sort points lexicographically
    // Andrew's monotone chain 2D convex hull algorithm needs that.
    // (`compare_point` only ties on identical points, so the sort algorithm does not matter.)
    poly.sort_unstable_by(|r, p| (r.x, r.y).cmp(&(p.x, p.y)));

    let mut k: usize = 0;

    // Store room (2 * n points) for result:
    // The actual convex hull use less points. the room will be adjusted later
    a_result.resize(2 * point_count, Vector2I::default());

    // Build lower hull
    for &p in poly.iter() {
        while k >= 2 && cross_product(a_result[k - 2], a_result[k - 1], p) <= 0 {
            k -= 1;
        }

        a_result[k] = p;
        k += 1;
    }

    // Build upper hull
    let t = k + 1;

    for ii in (0..point_count - 1).rev() {
        while k >= t && cross_product(a_result[k - 2], a_result[k - 1], poly[ii]) <= 0 {
            k -= 1;
        }

        a_result[k] = poly[ii];
        k += 1;
    }

    // The last point in the list is the same as the first one.
    // This is not needed, and sometimes create issues ( 0 length polygon segment:
    // remove it:
    if k > 1 && a_result[0] == a_result[k - 1] {
        k -= 1;
    }

    a_result.truncate(k);
}

/// Convenience form of [`build_convex_hull`] returning a new vector (empty for fewer than 2
/// points).
pub fn convex_hull(a_poly: &[Vector2I]) -> Vec<Vector2I> {
    let mut result = Vec::new();
    build_convex_hull(&mut result, a_poly);
    result
}

/// Port of `BuildConvexHull( std::vector<VECTOR2I>& aResult, const SHAPE_POLY_SET& aPolygons )`.
pub fn build_convex_hull_poly_set(a_result: &mut Vec<Vector2I>, a_polygons: &ShapePolySet) {
    build_convex_hull_poly_set_moved(a_result, a_polygons, Vector2I::new(0, 0), ANGLE_0);
}

/// Port of `BuildConvexHull( std::vector<VECTOR2I>& aResult, const SHAPE_POLY_SET& aPolygons,
/// const VECTOR2I& aPosition, const EDA_ANGLE& aRotation )`.
///
/// Calculate the convex hull of the outlines (holes are ignored), then rotate it by
/// `a_rotation` and move it by `a_position`.
pub fn build_convex_hull_poly_set_moved(
    a_result: &mut Vec<Vector2I>,
    a_polygons: &ShapePolySet,
    a_position: Vector2I,
    a_rotation: EdaAngle,
) {
    // Build the convex hull of the SHAPE_POLY_SET
    let mut buf: Vec<Vector2I> = Vec::new();

    for cnt in 0..a_polygons.outline_count() {
        let poly = a_polygons.c_outline(cnt);

        for ii in 0..poly.point_count() {
            buf.push(poly.c_point(ii));
        }
    }

    build_convex_hull(a_result, &buf);

    // Move and rotate the points according to aPosition and aRotation
    for pt in a_result.iter_mut() {
        rotate_point(pt, a_rotation);
        *pt = Vector2I::new(
            pt.x.wrapping_add(a_position.x),
            pt.y.wrapping_add(a_position.y),
        );
    }
}
