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

//! Port of `qa/tests/libs/kimath/geometry/test_arc_chord_params.cpp`.

use std::f64::consts::PI;

use kicad_geom::geometry::arc_chord_params::ArcChordParams;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{is_close_pct, is_small};

const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

macro_rules! check_close {
    ($a:expr, $b:expr, $tol:expr) => {{
        let (a, b): (f64, f64) = ($a, $b);
        assert!(is_close_pct(a, b, $tol), "{} = {a} is not within {}% of {b}", stringify!($a), $tol);
    }};
}

macro_rules! check_small {
    ($a:expr, $tol:expr) => {{
        let a: f64 = $a;
        assert!(is_small(a, $tol), "{} = {a} is not smaller than {}", stringify!($a), $tol);
    }};
}

#[test]
fn degenerate_cases() {
    let mut params = ArcChordParams::new();

    // Collinear points (mid on the line between start and end)
    assert!(!params.compute(pt(0, 0), pt(500000, 0), pt(1000000, 0)));
    assert!(!params.is_valid());

    // Same start and end point
    assert!(!params.compute(pt(0, 0), pt(500000, 500000), pt(0, 0)));
    assert!(!params.is_valid());

    // All three points the same
    assert!(!params.compute(pt(100, 100), pt(100, 100), pt(100, 100)));
    assert!(!params.is_valid());
}

#[test]
fn quarter_circle() {
    let mut params = ArcChordParams::new();

    // Quarter circle with radius 1000000, center at origin
    // Start at (1000000, 0), mid at approximately (707107, 707107), end at (0, 1000000)
    let radius = 1000000;
    let mid_coord = ki_round(radius as f64 * 2.0f64.sqrt() / 2.0);

    let start = pt(radius, 0);
    let mid = pt(mid_coord, mid_coord);
    let end = pt(0, radius);

    assert!(params.compute(start, mid, end));
    assert!(params.is_valid());

    // Check radius (should be close to 1000000)
    check_close!(params.radius(), radius as f64, 0.1);

    // Check arc angle (should be close to 90 degrees = PI/2 radians)
    check_close!(params.arc_angle(), PI / 2.0, 0.1);

    // Check center point (should be close to origin)
    let center = params.center_point();
    check_small!(center.x, 1.0);
    check_small!(center.y, 1.0);

    // Check chord length (should be sqrt(2) * radius)
    check_close!(params.chord_length(), radius as f64 * 2.0f64.sqrt(), 0.1);

    // Check sagitta (perpendicular distance from chord midpoint to arc)
    // For 90-degree arc: sagitta = radius * (1 - cos(45)) = radius * (1 - sqrt(2)/2)
    let expected_sagitta = radius as f64 * (1.0 - 2.0f64.sqrt() / 2.0);
    check_close!(params.sagitta(), expected_sagitta, 0.1);
}

#[test]
fn semicircle() {
    let mut params = ArcChordParams::new();

    let radius = 500000;
    let start = pt(-radius, 0);
    let mid = pt(0, radius);
    let end = pt(radius, 0);

    assert!(params.compute(start, mid, end));
    assert!(params.is_valid());

    // Check radius
    check_close!(params.radius(), radius as f64, 0.1);

    // Check arc angle (should be PI radians = 180 degrees)
    check_close!(params.arc_angle(), PI, 0.1);

    // Check center point (should be at origin)
    let center = params.center_point();
    check_small!(center.x, 1.0);
    check_small!(center.y, 1.0);

    // Check chord length (should be 2 * radius = diameter)
    check_close!(params.chord_length(), 2.0 * radius as f64, 0.1);

    // Check sagitta (should equal radius for semicircle)
    check_close!(params.sagitta(), radius as f64, 0.1);

    // Check center offset (should be 0 for semicircle since center is on the chord)
    check_small!(params.center_offset(), 1.0);
}

#[test]
fn shallow_arc() {
    let mut params = ArcChordParams::new();

    // Create a shallow arc: 10-degree arc with radius 10mm
    // This tests numerical stability for cases where center is far from arc
    let radius = 10000000; // 10mm in nm
    let arc_angle_rad = 10.0 * PI / 180.0; // 10 degrees
    let half_angle = arc_angle_rad / 2.0;

    // Start at angle -5 degrees, end at +5 degrees (from positive x-axis)
    let start = pt(
        ki_round(radius as f64 * (-half_angle).cos()),
        ki_round(radius as f64 * (-half_angle).sin()),
    );
    let mid = pt(radius, 0); // Midpoint at 0 degrees
    let end = pt(
        ki_round(radius as f64 * half_angle.cos()),
        ki_round(radius as f64 * half_angle.sin()),
    );

    assert!(params.compute(start, mid, end));
    assert!(params.is_valid());

    // Check radius (allow 0.5% tolerance for shallow arc)
    check_close!(params.radius(), radius as f64, 0.5);

    // Check arc angle
    check_close!(params.arc_angle(), arc_angle_rad, 1.0);

    // Center should be at origin
    let center = params.center_point();
    check_small!(center.x, 100.0); // Allow more tolerance for shallow arc
    check_small!(center.y, 100.0);
}

#[test]
fn start_end_angles() {
    let mut params = ArcChordParams::new();

    // Create arc from (1000, 0) through (0, 1000) to (-1000, 0)
    // This is a semicircle with center at origin
    let radius = 1000000;
    let start = pt(radius, 0);
    let mid = pt(0, radius);
    let end = pt(-radius, 0);

    assert!(params.compute(start, mid, end));

    // Start angle should be 0 degrees (pointing right from center)
    let start_angle = params.start_angle();
    check_close!(start_angle.as_degrees(), 0.0, 1.0);

    // End angle should be 180 degrees (pointing left from center)
    let end_angle = params.end_angle();
    check_close!(end_angle.as_degrees().abs(), 180.0, 1.0);
}

#[test]
fn offset_center() {
    let mut params = ArcChordParams::new();

    // Create quarter circle with center at (1000000, 1000000)
    let cx = 1000000;
    let cy = 1000000;
    let radius = 500000;

    let start = pt(cx + radius, cy);
    let mid_coord = ki_round(radius as f64 * 2.0f64.sqrt() / 2.0);
    let mid = pt(cx + mid_coord, cy + mid_coord);
    let end = pt(cx, cy + radius);

    assert!(params.compute(start, mid, end));
    assert!(params.is_valid());

    // Check radius
    check_close!(params.radius(), radius as f64, 0.1);

    // Check center point
    let center = params.center_point();
    check_close!(center.x, cx as f64, 0.1);
    check_close!(center.y, cy as f64, 0.1);
}

#[test]
fn arc_direction() {
    let mut params_ccw = ArcChordParams::new();
    let mut params_cw = ArcChordParams::new();

    let radius = 1000000;

    // CCW arc: start at right, mid at top, end at left
    let start = pt(radius, 0);
    let mid_ccw = pt(0, radius);
    let end = pt(-radius, 0);

    assert!(params_ccw.compute(start, mid_ccw, end));

    // CW arc: same start and end, but mid at bottom
    let mid_cw = pt(0, -radius);

    assert!(params_cw.compute(start, mid_cw, end));

    // Both should have same radius
    check_close!(params_ccw.radius(), radius as f64, 0.1);
    check_close!(params_cw.radius(), radius as f64, 0.1);

    // Both should have same arc angle magnitude (PI)
    check_close!(params_ccw.arc_angle(), PI, 0.1);
    check_close!(params_cw.arc_angle(), PI, 0.1);

    // Centers should be on opposite sides of the chord
    let _center_ccw = params_ccw.center_point();
    let _center_cw = params_cw.center_point();

    // For semicircles, centers are on the chord, so we check the normal direction
    let normal_ccw = params_ccw.normal_unit_vec();
    let normal_cw = params_cw.normal_unit_vec();

    // Normals should point in opposite directions (toward their respective centers)
    check_close!(normal_ccw.y, -normal_cw.y, 0.1);
}

#[test]
fn chord_midpoint() {
    let mut params = ArcChordParams::new();

    let start = pt(0, 0);
    let mid = pt(500000, 500000);
    let end = pt(1000000, 0);

    assert!(params.compute(start, mid, end));

    let chord_mid = params.chord_midpoint();

    // Chord midpoint should be average of start and end
    check_close!(chord_mid.x, 500000.0, 0.001);
    check_close!(chord_mid.y, 0.0, 0.001);
}

#[test]
fn unit_vectors_normalized() {
    let mut params = ArcChordParams::new();

    let start = pt(123456, 789012);
    let mid = pt(500000, 900000);
    let end = pt(876543, 210987);

    assert!(params.compute(start, mid, end));

    // Check chord unit vector is normalized
    let u = params.chord_unit_vec();
    let u_len = (u.x * u.x + u.y * u.y).sqrt();
    check_close!(u_len, 1.0, 0.0001);

    // Check normal unit vector is normalized
    let n = params.normal_unit_vec();
    let n_len = (n.x * n.x + n.y * n.y).sqrt();
    check_close!(n_len, 1.0, 0.0001);

    // Check u and n are perpendicular (dot product = 0)
    let dot = u.x * n.x + u.y * n.y;
    check_small!(dot, 0.0001);
}

#[test]
fn major_arc() {
    let mut params = ArcChordParams::new();

    // Create a 270-degree arc (major arc)
    // Start at (1000, 0), go CCW through (0, -1000) to (-1000, 0)
    // The "mid" point is at 135 degrees from start, which is at (-707, -707)
    let radius = 1000000;
    let mid_coord = ki_round(radius as f64 * 2.0f64.sqrt() / 2.0);

    let start = pt(radius, 0);
    let mid = pt(-mid_coord, -mid_coord); // At 225 degrees (halfway through 270-degree arc)
    let end = pt(0, radius);

    assert!(params.compute(start, mid, end));
    assert!(params.is_valid());

    // Check radius
    check_close!(params.radius(), radius as f64, 0.5);

    // Check arc angle (should be 270 degrees = 3*PI/2 radians)
    check_close!(params.arc_angle(), 3.0 * PI / 2.0, 1.0);
}
