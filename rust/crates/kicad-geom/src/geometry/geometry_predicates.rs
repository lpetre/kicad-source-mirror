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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/geometry/geometry_predicates.h` (namespace `KIGEOM`): exact
//! orientation and in-circle predicates over integer coordinates.

use crate::math::util::std_max;
use crate::math::vector2::{Vector2I, Vector2L};
use crate::math::wide_int::{KiInt128, cross_wide};

/// Port of `KIGEOM::OrientationSign`.
///
/// Orientation of triangle (a, b, c): +1 counter-clockwise, -1 clockwise, 0 collinear.
pub fn orientation_sign(a: Vector2I, b: Vector2I, c: Vector2I) -> i32 {
    // Widen before subtracting; a 32-bit coordinate difference overflows past ~2.1e9 nm.
    let ab_x: i64 = b.x as i64 - a.x as i64;
    let ab_y: i64 = b.y as i64 - a.y as i64;
    let ac_x: i64 = c.x as i64 - a.x as i64;
    let ac_y: i64 = c.y as i64 - a.y as i64;

    let cross: KiInt128 = cross_wide(Vector2L::new(ab_x, ab_y), Vector2L::new(ac_x, ac_y));

    if cross > 0 {
        1
    } else if cross < 0 {
        -1
    } else {
        0
    }
}

/// Port of `KIGEOM::detail::inCircleLegalDouble`.
///
/// Filtered-double legality test; the tie margin treats a near-cocircular quad as legal to
/// keep a flip loop from oscillating.
pub fn in_circle_legal_double(a: Vector2I, b: Vector2I, c: Vector2I, p: Vector2I) -> bool {
    let pa_x: f64 = a.x as f64 - p.x as f64;
    let pa_y: f64 = a.y as f64 - p.y as f64;
    let pb_x: f64 = b.x as f64 - p.x as f64;
    let pb_y: f64 = b.y as f64 - p.y as f64;
    let pc_x: f64 = c.x as f64 - p.x as f64;
    let pc_y: f64 = c.y as f64 - p.y as f64;

    let pa_sq: f64 = pa_x * pa_x + pa_y * pa_y;
    let pb_sq: f64 = pb_x * pb_x + pb_y * pb_y;
    let pc_sq: f64 = pc_x * pc_x + pc_y * pc_y;

    let det: f64 = pa_x * (pb_y * pc_sq - pb_sq * pc_y) - pa_y * (pb_x * pc_sq - pb_sq * pc_x)
        + pa_sq * (pb_x * pc_y - pb_y * pc_x);
    let sum_sq: f64 = pa_sq + pb_sq + pc_sq;

    det <= 1e-13 * sum_sq * sum_sq
}

/// Port of `KIGEOM::InCircleDelaunayLegal`.
///
/// True when p is outside the circumcircle of CCW triangle (a, b, c): the shared edge is
/// already Delaunay and must not be flipped. Exact, so an exact cocircular result never
/// oscillates a flip.
pub fn in_circle_delaunay_legal(a: Vector2I, b: Vector2I, c: Vector2I, p: Vector2I) -> bool {
    // Widen before subtracting; an int32 difference overflows past ~2.1e9 nm.
    let pa_x: i64 = a.x as i64 - p.x as i64;
    let pa_y: i64 = a.y as i64 - p.y as i64;
    let pb_x: i64 = b.x as i64 - p.x as i64;
    let pb_y: i64 = b.y as i64 - p.y as i64;
    let pc_x: i64 = c.x as i64 - p.x as i64;
    let pc_y: i64 = c.y as i64 - p.y as i64;

    // Beyond this the degree-4 determinant (~12*M^4) can exceed signed int128.
    const K_MAX_SAFE_DIFF: i64 = 1_500_000_000;

    if pa_x.abs() > K_MAX_SAFE_DIFF
        || pa_y.abs() > K_MAX_SAFE_DIFF
        || pb_x.abs() > K_MAX_SAFE_DIFF
        || pb_y.abs() > K_MAX_SAFE_DIFF
        || pc_x.abs() > K_MAX_SAFE_DIFF
        || pc_y.abs() > K_MAX_SAFE_DIFF
    {
        return in_circle_legal_double(a, b, c, p);
    }

    let (pa_x, pa_y) = (pa_x as KiInt128, pa_y as KiInt128);
    let (pb_x, pb_y) = (pb_x as KiInt128, pb_y as KiInt128);
    let (pc_x, pc_y) = (pc_x as KiInt128, pc_y as KiInt128);

    let pa_sq: KiInt128 = pa_x * pa_x + pa_y * pa_y;
    let pb_sq: KiInt128 = pb_x * pb_x + pb_y * pb_y;
    let pc_sq: KiInt128 = pc_x * pc_x + pc_y * pc_y;

    let det: KiInt128 = pa_x * (pb_y * pc_sq - pb_sq * pc_y) - pa_y * (pb_x * pc_sq - pb_sq * pc_x)
        + pa_sq * (pb_x * pc_y - pb_y * pc_x);

    det <= 0
}

/// Port of `KIGEOM::IsSliverTriangle`: a triangle is a sliver when its longest edge exceeds
/// ten times its shortest.
pub fn is_sliver_triangle(a: Vector2I, b: Vector2I, c: Vector2I) -> bool {
    let ab_x: f64 = b.x as f64 - a.x as f64;
    let ab_y: f64 = b.y as f64 - a.y as f64;
    let bc_x: f64 = c.x as f64 - b.x as f64;
    let bc_y: f64 = c.y as f64 - b.y as f64;
    let ca_x: f64 = a.x as f64 - c.x as f64;
    let ca_y: f64 = a.y as f64 - c.y as f64;

    let ab_squared: f64 = ab_x * ab_x + ab_y * ab_y;
    let bc_squared: f64 = bc_x * bc_x + bc_y * bc_y;
    let ca_squared: f64 = ca_x * ca_x + ca_y * ca_y;

    // std::max / std::min of an initializer list keep the first of equal elements
    let longest_squared = std_max(std_max(ab_squared, bc_squared), ca_squared);
    let shortest_squared = {
        let m = if bc_squared < ab_squared { bc_squared } else { ab_squared };
        if ca_squared < m { ca_squared } else { m }
    };

    shortest_squared > 0.0 && longest_squared > 100.0 * shortest_squared
}
