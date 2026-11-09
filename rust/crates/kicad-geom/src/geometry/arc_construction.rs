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

//! Port of `libs/kimath/include/geometry/arc_construction.h` and
//! `libs/kimath/src/geometry/arc_construction.cpp` (namespace `KIGEOM`): construction helpers
//! for the interactive arc drawing modes.
//!
//! Every solver returns the arc as start/mid/end, the same form the file formats store. The
//! midpoint fixes the sweep direction, so no separate clockwise flag is needed. Start and end
//! are always returned exactly as given.

use crate::math::util::ki_round;
use crate::math::vector2::{Vector2D, Vector2I};

/// Port of `KIGEOM::ARC_SOLUTION`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArcSolution {
    /// False for degenerate input (collinear points, zero chord, etc)
    pub valid: bool,
    /// Arc start
    pub start: Vector2I,
    /// The point on the arc halfway along the sweep
    pub mid: Vector2I,
    /// Arc end
    pub end: Vector2I,
    /// Arc center
    pub center: Vector2D,
    /// Arc radius
    pub radius: f64,
}

/// Cross product z with the rounding error compensated, for near-collinear tests on large
/// coordinates.  (`std::fma` in C++, hence `mul_add` here.)
fn cross_z(a_x: f64, a_y: f64, b_x: f64, b_y: f64) -> f64 {
    let p: f64 = a_y * b_x;
    let e: f64 = a_y.mul_add(b_x, -p);
    a_x.mul_add(b_y, -p) - e
}

/// Round to internal units, failing when the value does not fit
fn to_iu(a_pt: Vector2D) -> Option<Vector2I> {
    const LIMIT: f64 = (i32::MAX - 1) as f64;

    if !a_pt.x.is_finite() || !a_pt.y.is_finite() || a_pt.x.abs() > LIMIT || a_pt.y.abs() > LIMIT {
        return None;
    }

    Some(Vector2I::new(ki_round(a_pt.x), ki_round(a_pt.y)))
}

// aSide is +1 for an arc on the (-dy, dx) side of start to end, and aCenterT is the center
// offset along that normal
fn build_on_chord(
    a_start: Vector2I,
    a_end: Vector2I,
    a_side: f64,
    a_center_t: f64,
    a_radius: f64,
) -> ArcSolution {
    let mut sol = ArcSolution {
        start: a_start,
        end: a_end,
        ..ArcSolution::default()
    };

    let chord = Vector2D::new(
        a_end.x as f64 - a_start.x as f64,
        a_end.y as f64 - a_start.y as f64,
    );
    let len: f64 = chord.euclidean_norm();

    if len <= 0.0 || !a_radius.is_finite() || a_radius <= 0.0 {
        return sol;
    }

    let normal = Vector2D::new(-chord.y / len, chord.x / len);
    let chord_mid = Vector2D::new(
        (a_start.x as f64 + a_end.x as f64) * 0.5,
        (a_start.y as f64 + a_end.y as f64) * 0.5,
    );
    let half: f64 = len * 0.5;
    let offset: f64 = a_center_t.abs();

    // radius - offset cancels for shallow arcs, so use the equivalent half^2 / ( radius + offset )
    let height: f64 = if a_side * a_center_t <= 0.0 {
        half * half / (a_radius + offset)
    } else {
        a_radius + offset
    };

    match to_iu(chord_mid + normal * (a_side * height)) {
        Some(mid) => sol.mid = mid,
        None => return sol,
    }

    // An apex within rounding of the chord cannot be stored as an arc
    if cross_z(
        chord.x,
        chord.y,
        sol.mid.x as f64 - a_start.x as f64,
        sol.mid.y as f64 - a_start.y as f64,
    ) == 0.0
    {
        return sol;
    }

    sol.center = chord_mid + normal * a_center_t;
    sol.radius = a_radius;
    sol.valid = true;
    sol
}

/// Port of `KIGEOM::ProjectToChordBisector()`: project `a_cursor` onto the perpendicular
/// bisector of the chord `a_start` - `a_end`.
pub fn project_to_chord_bisector(
    a_start: Vector2I,
    a_end: Vector2I,
    a_cursor: Vector2I,
) -> Vector2I {
    let chord = Vector2D::new(
        a_end.x as f64 - a_start.x as f64,
        a_end.y as f64 - a_start.y as f64,
    );
    let len: f64 = chord.euclidean_norm();

    if len <= 0.0 {
        return a_cursor;
    }

    let normal = Vector2D::new(-chord.y / len, chord.x / len);
    let chord_mid = Vector2D::new(
        (a_start.x as f64 + a_end.x as f64) * 0.5,
        (a_start.y as f64 + a_end.y as f64) * 0.5,
    );
    let rel = Vector2D::new(
        a_cursor.x as f64 - chord_mid.x,
        a_cursor.y as f64 - chord_mid.y,
    );

    match to_iu(chord_mid + normal * rel.dot(normal)) {
        Some(projected) => projected,
        None => a_cursor,
    }
}

/// Port of `KIGEOM::ArcThroughPoints()`: arc from `a_start` to `a_end` that passes through
/// `a_on_arc`.
pub fn arc_through_points(a_start: Vector2I, a_on_arc: Vector2I, a_end: Vector2I) -> ArcSolution {
    let b = Vector2D::new(
        a_end.x as f64 - a_start.x as f64,
        a_end.y as f64 - a_start.y as f64,
    );
    let c = Vector2D::new(
        a_on_arc.x as f64 - a_start.x as f64,
        a_on_arc.y as f64 - a_start.y as f64,
    );
    let d: f64 = cross_z(b.x, b.y, c.x, c.y);

    if d == 0.0 {
        return ArcSolution::default();
    }

    let bb: f64 = b.squared_euclidean_norm();
    let cc: f64 = c.squared_euclidean_norm();

    // Circumcenter relative to the start point
    let rel = Vector2D::new(
        (c.y * bb - b.y * cc) / (2.0 * d),
        (b.x * cc - c.x * bb) / (2.0 * d),
    );
    let len: f64 = b.euclidean_norm();

    // The signed distance of the center along the chord normal, measured from the chord midpoint
    let center_t: f64 = (-rel.x * b.y + rel.y * b.x) / len;

    build_on_chord(
        a_start,
        a_end,
        if d > 0.0 { 1.0 } else { -1.0 },
        center_t,
        rel.euclidean_norm(),
    )
}

/// Port of `KIGEOM::ArcFromStartEndMidDrag()`: arc from `a_start` to `a_end` whose midpoint is
/// `a_cursor` projected onto the chord bisector.  Invalid when the projection lands on the
/// chord.
pub fn arc_from_start_end_mid_drag(
    a_start: Vector2I,
    a_end: Vector2I,
    a_cursor: Vector2I,
) -> ArcSolution {
    let projected = project_to_chord_bisector(a_start, a_end, a_cursor);
    let mut sol = arc_through_points(a_start, projected, a_end);

    // The circle passes through the projected point exactly, so keep it as the midpoint
    if sol.valid {
        sol.mid = projected;
    }

    sol
}

/// Port of `KIGEOM::ArcFromStartEndCenterDrag()`: arc from `a_start` to `a_end` centered on
/// `a_cursor` projected onto the chord bisector.
///
/// `a_major` is false for the minor arc (sweep up to 180 degrees), true for the complementary
/// major arc.  A center on the chord gives a semicircle in either case, and the flag then picks
/// the side.
pub fn arc_from_start_end_center_drag(
    a_start: Vector2I,
    a_end: Vector2I,
    a_cursor: Vector2I,
    a_major: bool,
) -> ArcSolution {
    let chord = Vector2D::new(
        a_end.x as f64 - a_start.x as f64,
        a_end.y as f64 - a_start.y as f64,
    );
    let len: f64 = chord.euclidean_norm();

    if len <= 0.0 {
        return ArcSolution::default();
    }

    let normal = Vector2D::new(-chord.y / len, chord.x / len);
    let chord_mid = Vector2D::new(
        (a_start.x as f64 + a_end.x as f64) * 0.5,
        (a_start.y as f64 + a_end.y as f64) * 0.5,
    );
    let center_t: f64 = Vector2D::new(
        a_cursor.x as f64 - chord_mid.x,
        a_cursor.y as f64 - chord_mid.y,
    )
    .dot(normal);

    // The minor arc bulges away from the center, the major arc toward it
    let mut side: f64 = if center_t < 0.0 { -1.0 } else { 1.0 };

    if !a_major {
        side = -side;
    }

    build_on_chord(
        a_start,
        a_end,
        side,
        center_t,
        crate::cmath::hypot(len * 0.5, center_t),
    )
}

/// Port of `KIGEOM::ArcFromStartTangentEnd()`: arc leaving `a_start` along `a_tangent` and
/// ending at `a_end`.  Invalid when `a_end` lies on the tangent line or `a_tangent` is zero.
pub fn arc_from_start_tangent_end(
    a_start: Vector2I,
    a_tangent: Vector2D,
    a_end: Vector2I,
) -> ArcSolution {
    let tangent_len: f64 = a_tangent.euclidean_norm();

    if tangent_len <= 0.0 || !tangent_len.is_finite() {
        return ArcSolution::default();
    }

    let t = Vector2D::new(a_tangent.x / tangent_len, a_tangent.y / tangent_len);
    let d = Vector2D::new(
        a_end.x as f64 - a_start.x as f64,
        a_end.y as f64 - a_start.y as f64,
    );
    let dd: f64 = d.squared_euclidean_norm();
    let cross: f64 = cross_z(d.x, d.y, t.x, t.y);

    if dd <= 0.0 || cross == 0.0 {
        return ArcSolution::default();
    }

    // The center sits on the normal to the tangent at the start, equidistant from start and end
    let k: f64 = -dd / (2.0 * cross);
    let n = Vector2D::new(-t.y, t.x);
    let len: f64 = dd.sqrt();
    let chord_normal = Vector2D::new(-d.y / len, d.x / len);

    build_on_chord(
        a_start,
        a_end,
        if cross > 0.0 { 1.0 } else { -1.0 },
        k * n.dot(chord_normal),
        k.abs(),
    )
}
