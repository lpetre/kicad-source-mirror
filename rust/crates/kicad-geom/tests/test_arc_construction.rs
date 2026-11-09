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

//! Port of `qa/tests/libs/kimath/geometry/test_arc_construction.cpp`.

use std::f64::consts::PI;

use kicad_geom::geometry::arc_construction::{
    ArcSolution, arc_from_start_end_center_drag, arc_from_start_end_mid_drag,
    arc_from_start_tangent_end, arc_through_points, project_to_chord_bisector,
};
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::{Vector2D, Vector2I};
use kicad_geom::test_utils::{is_close_pct, is_small};

const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn dist(a_a: impl Into<Vector2D>, a_b: impl Into<Vector2D>) -> f64 {
    (a_a.into() - a_b.into()).euclidean_norm()
}

/// Sweep of the solution in degrees, using the midpoint to pick between the two arcs
fn sweep_degrees(a_sol: &ArcSolution) -> f64 {
    let s: Vector2D = Vector2D::from(a_sol.start) - a_sol.center;
    let m: Vector2D = Vector2D::from(a_sol.mid) - a_sol.center;
    2.0 * s.cross(m).atan2(s.dot(m)).abs() * 180.0 / PI
}

/// +1 when the sweep from start through mid increases the raw angle, else -1
fn orientation(a_sol: &ArcSolution) -> i32 {
    let s: Vector2D = Vector2D::from(a_sol.start) - a_sol.center;
    let m: Vector2D = Vector2D::from(a_sol.mid) - a_sol.center;
    if s.cross(m) > 0.0 { 1 } else { -1 }
}

/// Every point of the solution lies on its circle and the midpoint bisects the chord
fn check_consistent(a_sol: &ArcSolution, a_start: Vector2I, a_end: Vector2I, a_tol: f64) {
    assert!(a_sol.valid);
    assert_eq!(a_sol.start, a_start);
    assert_eq!(a_sol.end, a_end);
    assert!(is_small(
        dist(a_sol.start, a_sol.center) - a_sol.radius,
        a_tol
    ));
    assert!(is_small(
        dist(a_sol.end, a_sol.center) - a_sol.radius,
        a_tol
    ));
    assert!(is_small(
        dist(a_sol.mid, a_sol.center) - a_sol.radius,
        a_tol
    ));
    assert!(is_small(
        dist(a_sol.mid, a_sol.start) - dist(a_sol.mid, a_sol.end),
        a_tol
    ));
}

#[test]
fn project_lands_on_bisector() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);

    assert_eq!(
        project_to_chord_bisector(start, end, pt(700000, 300000)),
        pt(500000, 300000)
    );
    assert_eq!(
        project_to_chord_bisector(start, end, pt(-40, -900000)),
        pt(500000, -900000)
    );

    // A diagonal chord, so the projection has to rotate
    let p = project_to_chord_bisector(pt(0, 0), pt(1000000, 1000000), pt(900000, 100000));
    assert!(is_small(
        dist(p, Vector2D::new(0.0, 0.0)) - dist(p, Vector2D::new(1000000.0, 1000000.0)),
        1.0
    ));

    assert_eq!(
        project_to_chord_bisector(start, start, pt(12, 34)),
        pt(12, 34)
    );
}

#[test]
fn through_points_quarter_circle() {
    let start = pt(1000000, 0);
    let end = pt(0, 1000000);
    let sol = arc_through_points(start, pt(707107, 707107), end);

    check_consistent(&sol, start, end, 1.5);
    assert!(is_small(sol.center.x, 1.5));
    assert!(is_small(sol.center.y, 1.5));
    assert!(is_small(sol.radius - 1000000.0, 1.5));
    assert!(dist(sol.mid, Vector2D::new(707107.0, 707107.0)) <= 1.5);
    assert!(is_small(sweep_degrees(&sol) - 90.0, 1e-2));
}

#[test]
fn through_points_major_arc_mid_is_halfway() {
    let start = pt(1000000, 0);
    let end = pt(0, 1000000);

    // The on-arc point on the far side of the chord selects the 270 degree arc
    let sol = arc_through_points(start, pt(-707107, -707107), end);

    check_consistent(&sol, start, end, 1.5);
    assert!(is_small(sweep_degrees(&sol) - 270.0, 1e-2));
    assert!(dist(sol.mid, Vector2D::new(-707107.0, -707107.0)) <= 1.5);
}

#[test]
fn through_points_direction_follows_on_arc_point() {
    let a = pt(1000000, 0);
    let b = pt(0, 1000000);
    let on = pt(707107, 707107);

    let fwd = arc_through_points(a, on, b);
    let rev = arc_through_points(b, on, a);

    assert!(fwd.valid);
    assert!(rev.valid);
    assert_eq!(orientation(&fwd), -orientation(&rev));

    // Same circle and same halfway point either way round
    assert!(dist(fwd.center, rev.center) <= 1e-6);
    assert_eq!(fwd.mid, rev.mid);
    assert_eq!(rev.start, b);
    assert_eq!(rev.end, a);
}

#[test]
fn through_points_mirrored_on_arc_point_mirrors_mid() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);

    let up = arc_through_points(start, pt(300000, 200000), end);
    let down = arc_through_points(start, pt(300000, -200000), end);

    check_consistent(&up, start, end, 1.5);
    check_consistent(&down, start, end, 1.5);
    assert!(up.mid.y > 0);
    assert!(down.mid.y < 0);
    assert_eq!(up.mid.x, down.mid.x);
    assert_eq!(up.mid.y, -down.mid.y);
}

#[test]
fn through_points_degenerate_is_invalid() {
    assert!(!arc_through_points(pt(0, 0), pt(500, 0), pt(1000, 0)).valid);
    assert!(!arc_through_points(pt(0, 0), pt(500, 500), pt(1000, 1000)).valid);
    assert!(!arc_through_points(pt(0, 0), pt(0, 0), pt(1000, 1000)).valid);
    assert!(!arc_through_points(pt(5, 5), pt(700, 900), pt(5, 5)).valid);
    assert!(!arc_through_points(pt(5, 5), pt(5, 5), pt(5, 5)).valid);
}

#[test]
fn through_points_very_shallow_arc() {
    // A 2 m chord with a 1 um sagitta, so the center is about 5e14 nm away
    let start = pt(0, 0);
    let end = pt(2000000000, 0);
    let sol = arc_through_points(start, pt(1000000000, 1000), end);

    check_consistent(&sol, start, end, 1.5);
    assert_eq!(sol.mid, pt(1000000000, 1000));
    assert!(is_close_pct(sol.radius, 5.0e14, 1e-3));
}

#[test]
fn through_points_one_nanometer_sagitta() {
    // The center is 2e18 nm away, beyond what a double resolves to the nanometer
    let start = pt(-2000000000, 0);
    let end = pt(2000000000, 0);
    let sol = arc_through_points(start, pt(0, 1), end);

    assert!(sol.valid);
    assert_eq!(sol.mid, pt(0, 1));
    assert!(is_close_pct(sol.radius, 2.0e18, 1e-6));
}

#[test]
fn through_points_near_integer_limits() {
    let start = pt(-2000000000, 0);
    let end = pt(2000000000, 0);
    let sol = arc_through_points(start, pt(0, 1500000000), end);

    check_consistent(&sol, start, end, 4.0);
    assert_eq!(sol.mid, pt(0, 1500000000));
    assert!(is_close_pct(sol.radius, (4.0e18 + 2.25e18) / 3.0e9, 1e-6));
}

#[test]
fn opposite_corners_near_billion_units() {
    // Squared lengths here exceed the range of 32 and 64 bit integers alike when computed carelessly
    let a = pt(-1000000000, -1000000000);
    let b = pt(1000000000, 1000000000);

    let through = arc_through_points(a, pt(1000000000, -1000000000), b);
    check_consistent(&through, a, b, 4.0);
    assert!(is_small(sweep_degrees(&through) - 180.0, 1e-2));

    let mid = arc_from_start_end_mid_drag(a, b, pt(300000000, -700000000));
    check_consistent(&mid, a, b, 4.0);

    let center = arc_from_start_end_center_drag(a, b, pt(0, 0), false);
    check_consistent(&center, a, b, 4.0);
    assert!(is_small(sweep_degrees(&center) - 180.0, 1e-2));

    let tangent = arc_from_start_tangent_end(a, Vector2D::new(1.0, 0.0), b);
    check_consistent(&tangent, a, b, 4.0);
    assert!(tangent.center.x.is_finite() && tangent.center.y.is_finite());

    let p = project_to_chord_bisector(a, b, pt(1000000000, -1000000000));
    assert!((dist(p, a) - dist(p, b)).abs() <= 1.0);
}

#[test]
fn mid_drag_projects_onto_bisector() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);

    let mut sol = arc_from_start_end_mid_drag(start, end, pt(700000, 300000));
    check_consistent(&sol, start, end, 1.5);
    assert_eq!(sol.mid, pt(500000, 300000));
    assert!(is_close_pct(
        sol.radius,
        (500000.0 * 500000.0 + 300000.0 * 300000.0) / 600000.0,
        1e-6
    ));

    // Off the bisector on the other side of the chord
    sol = arc_from_start_end_mid_drag(start, end, pt(100, -250000));
    check_consistent(&sol, start, end, 1.5);
    assert_eq!(sol.mid, pt(500000, -250000));
}

#[test]
fn mid_drag_diagonal_chord_is_equidistant() {
    let start = pt(123457, -98765);
    let end = pt(1000003, 777777);
    let sol = arc_from_start_end_mid_drag(start, end, pt(100000, 900000));

    check_consistent(&sol, start, end, 1.5);
}

#[test]
fn mid_drag_on_chord_is_invalid() {
    assert!(!arc_from_start_end_mid_drag(pt(0, 0), pt(1000000, 0), pt(300000, 0)).valid);
    assert!(!arc_from_start_end_mid_drag(pt(0, 0), pt(1000000, 1000000), pt(900000, 900000)).valid);
    assert!(!arc_from_start_end_mid_drag(pt(7, 7), pt(7, 7), pt(900000, 900000)).valid);
}

#[test]
fn center_drag_minor_is_default() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);
    let sol = arc_from_start_end_center_drag(start, end, pt(500000, 500000), false);

    check_consistent(&sol, start, end, 1.5);
    assert!(dist(sol.center, Vector2D::new(500000.0, 500000.0)) <= 1e-6);
    assert!(is_small(sweep_degrees(&sol) - 90.0, 1e-2));
    assert!(dist(sol.mid, Vector2D::new(500000.0, (500000 - 707107) as f64)) <= 1.5);
}

#[test]
fn center_drag_major_sweeps_past_half_turn() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);
    let sol = arc_from_start_end_center_drag(start, end, pt(500000, 500000), true);

    check_consistent(&sol, start, end, 1.5);
    assert!(is_small(sweep_degrees(&sol) - 270.0, 1e-2));
    assert!(dist(sol.mid, Vector2D::new(500000.0, (500000 + 707107) as f64)) <= 1.5);

    // A center behind the chord swaps which side each posture bulges toward
    let below = arc_from_start_end_center_drag(start, end, pt(500000, -500000), false);
    check_consistent(&below, start, end, 1.5);
    assert!(is_small(sweep_degrees(&below) - 90.0, 1e-2));
    assert!(below.mid.y > 0);
}

#[test]
fn center_drag_on_chord_is_semicircle() {
    let start = pt(0, 0);
    let end = pt(1000000, 0);

    let minor = arc_from_start_end_center_drag(start, end, pt(500000, 0), false);
    let major = arc_from_start_end_center_drag(start, end, pt(500000, 0), true);

    check_consistent(&minor, start, end, 1.5);
    check_consistent(&major, start, end, 1.5);
    assert!(is_small(sweep_degrees(&minor) - 180.0, 1e-2));
    assert!(is_small(sweep_degrees(&major) - 180.0, 1e-2));
    assert_eq!(minor.mid.x, 500000);
    assert_eq!(major.mid.x, 500000);
    assert_eq!(minor.mid.y, -major.mid.y);
    assert_eq!(minor.mid.y.abs(), 500000);
}

#[test]
fn center_drag_degenerate_and_overflow_are_invalid() {
    assert!(!arc_from_start_end_center_drag(pt(3, 3), pt(3, 3), pt(500, 500), false).valid);

    // The major arc apex lands outside the coordinate range, the minor arc still fits
    assert!(
        !arc_from_start_end_center_drag(
            pt(-2000000000, 0),
            pt(2000000000, 0),
            pt(0, 2000000000),
            true
        )
        .valid
    );
    assert!(
        arc_from_start_end_center_drag(
            pt(-2000000000, 0),
            pt(2000000000, 0),
            pt(0, 2000000000),
            false
        )
        .valid
    );
}

#[test]
fn tangent_start_direction_is_parallel_over_end_sweep() {
    let start = pt(250000, -130000);

    for tangent in [
        Vector2D::new(1.0, 0.0),
        Vector2D::new(0.0, -1.0),
        Vector2D::new(3.0, 4.0),
        Vector2D::new(-5.0, 2.0),
    ] {
        let unit = tangent / tangent.euclidean_norm();

        // Ends ahead of, beside and behind the start, at a spread of distances
        let mut degrees = 5;

        while degrees < 360 {
            for radius in [3000.0, 400000.0, 90000000.0] {
                let a = degrees as f64 * PI / 180.0;
                let end = pt(
                    start.x + ki_round(radius * (unit.x * a.cos() - unit.y * a.sin())),
                    start.y + ki_round(radius * (unit.x * a.sin() + unit.y * a.cos())),
                );

                let sol = arc_from_start_tangent_end(start, tangent, end);

                // "tangent << tangent << degrees << degrees << radius << radius"
                check_consistent(&sol, start, end, 2.0);
                let radial = (Vector2D::from(start) - sol.center) / sol.radius;
                let travel = Vector2D::new(-radial.y, radial.x) * orientation(&sol) as f64;
                assert!(
                    is_small(radial.dot(unit) * sol.radius, 1.0),
                    "{tangent:?} {degrees} {radius}"
                );
                assert!(
                    travel.dot(unit) > 0.999999,
                    "{tangent:?} {degrees} {radius}"
                );
            }

            degrees += 10;
        }
    }
}

#[test]
fn tangent_sweep_is_twice_the_chord_angle() {
    let start = pt(0, 0);

    // Ahead and to the side gives a quarter turn's chord at 45 degrees, so 90 degrees of sweep
    assert!(is_small(
        sweep_degrees(&arc_from_start_tangent_end(
            start,
            Vector2D::new(1.0, 0.0),
            pt(100000, 100000)
        )) - 90.0,
        1e-2
    ));

    // Behind the start the arc has to swing around the long way
    assert!(is_small(
        sweep_degrees(&arc_from_start_tangent_end(
            start,
            Vector2D::new(1.0, 0.0),
            pt(-100000, 100000)
        )) - 270.0,
        1e-2
    ));
}

#[test]
fn tangent_side_follows_end_side() {
    let left = arc_from_start_tangent_end(pt(0, 0), Vector2D::new(1.0, 0.0), pt(100000, 100000));
    let right = arc_from_start_tangent_end(pt(0, 0), Vector2D::new(1.0, 0.0), pt(100000, -100000));

    assert!(left.valid);
    assert!(right.valid);
    assert!(left.center.y > 0.0);
    assert!(right.center.y < 0.0);
    assert_eq!(left.mid.x, right.mid.x);
    assert_eq!(left.mid.y, -right.mid.y);
}

#[test]
fn tangent_degenerate_is_invalid() {
    // End on the tangent line, ahead of and behind the start
    assert!(!arc_from_start_tangent_end(pt(0, 0), Vector2D::new(1.0, 0.0), pt(500000, 0)).valid);
    assert!(!arc_from_start_tangent_end(pt(0, 0), Vector2D::new(1.0, 0.0), pt(-500000, 0)).valid);
    assert!(
        !arc_from_start_tangent_end(pt(0, 0), Vector2D::new(0.0, 0.0), pt(500000, 500000)).valid
    );
    assert!(!arc_from_start_tangent_end(pt(10, 10), Vector2D::new(1.0, 1.0), pt(10, 10)).valid);
    assert!(
        !arc_from_start_tangent_end(pt(0, 0), Vector2D::new(1.0, 1.0), pt(500000, 500000)).valid
    );
}
