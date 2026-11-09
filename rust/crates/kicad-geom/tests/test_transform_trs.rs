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

//! Port of `qa/tests/libs/kimath/geometry/test_transform_trs.cpp` (suite `TransformTrs`).

use kicad_geom::geometry::eda_angle::{
    ANGLE_0, ANGLE_45, ANGLE_90, ANGLE_180, ANGLE_270, DEGREES_T, EdaAngle,
};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::is_close_pct;

/// 1 unit slack for float-to-int rounding.
const IUNIT_TOL: i32 = 1;

#[track_caller]
fn check_vec_near(a_actual: Vector2I, a_expected: Vector2I, a_tol: i32) {
    assert!(
        (a_actual.x - a_expected.x).abs() <= a_tol && (a_actual.y - a_expected.y).abs() <= a_tol,
        "actual ( {}, {} ) expected ( {}, {} ) tol {}",
        a_actual.x,
        a_actual.y,
        a_expected.x,
        a_expected.y,
        a_tol
    );
}

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
}

#[test]
fn identity_apply() {
    let t = TransformTrs::default();
    assert!(t.is_identity());
    assert!(t.is_uniform_scale());

    let p = v(12345, -6789);
    check_vec_near(t.apply(p), p, 0);
    check_vec_near(t.inverse_apply(p), p, 0);
}

#[test]
fn translate_only() {
    let t = TransformTrs::new(v(100, 200), ANGLE_0, 1.0, 1.0);
    assert!(!t.is_identity());

    check_vec_near(t.apply(v(0, 0)), v(100, 200), 0);
    check_vec_near(t.apply(v(50, 50)), v(150, 250), 0);
    check_vec_near(t.inverse_apply(v(100, 200)), v(0, 0), 0);
    check_vec_near(t.inverse_apply(v(150, 250)), v(50, 50), 0);
}

#[test]
fn rotate_only() {
    // Expected values match trigo.h::RotatePoint conventions.
    let cases = [
        (ANGLE_0, v(100, 0), v(100, 0)),
        (ANGLE_90, v(100, 0), v(0, -100)),
        (ANGLE_180, v(100, 0), v(-100, 0)),
        (ANGLE_270, v(100, 0), v(0, 100)),
    ];

    for (angle, p, expected) in cases {
        let t = TransformTrs::new(v(0, 0), angle, 1.0, 1.0);
        check_vec_near(t.apply(p), expected, IUNIT_TOL);
        check_vec_near(t.inverse_apply(expected), p, IUNIT_TOL);
    }
}

#[test]
fn scale_only() {
    let uniform = TransformTrs::new(v(0, 0), ANGLE_0, 2.0, 2.0);
    assert!(uniform.is_uniform_scale());
    check_vec_near(uniform.apply(v(100, 50)), v(200, 100), 0);
    check_vec_near(uniform.inverse_apply(v(200, 100)), v(100, 50), 0);

    let half = TransformTrs::new(v(0, 0), ANGLE_0, 0.5, 0.5);
    check_vec_near(half.apply(v(100, 50)), v(50, 25), 0);

    let nonuniform = TransformTrs::new(v(0, 0), ANGLE_0, 2.0, 0.5);
    assert!(!nonuniform.is_uniform_scale());
    check_vec_near(nonuniform.apply(v(100, 100)), v(200, 50), 0);
    check_vec_near(nonuniform.inverse_apply(v(200, 50)), v(100, 100), 0);
}

#[test]
fn compound_srt_oracle() {
    // p (10, 0): scale 2x to (20, 0); rotate 90 to (0, -20); translate to (1000, 480).
    let t = TransformTrs::new(v(1000, 500), ANGLE_90, 2.0, 2.0);
    check_vec_near(t.apply(v(10, 0)), v(1000, 480), IUNIT_TOL);
    check_vec_near(t.inverse_apply(t.apply(v(10, 0))), v(10, 0), IUNIT_TOL);
}

#[test]
fn inverse_apply_round_trip() {
    let ts = [
        TransformTrs::new(v(100, 200), deg(37.5), 1.0, 1.0),
        TransformTrs::new(v(-50, 75), deg(123.4), 1.5, 1.5),
        TransformTrs::new(v(0, 0), deg(60.0), 2.0, 0.5),
        TransformTrs::new(v(1234, -567), deg(89.0), 0.75, 1.25),
    ];

    let probes = [v(0, 0), v(100, 0), v(0, 100), v(250, -125), v(-300, 400)];

    for t in &ts {
        for &p in &probes {
            let round = t.inverse_apply(t.apply(p));
            check_vec_near(round, p, IUNIT_TOL);
        }
    }
}

#[test]
fn rescale_around_fixed_point_invariant() {
    let t = TransformTrs::new(v(1000, 500), ANGLE_0, 1.0, 1.0);
    let lib_point = v(25, 25);
    let fixed_point = t.apply(lib_point);

    let rescaled = t.rescale_around(fixed_point, 3.0, 3.0);
    check_vec_near(rescaled.apply(lib_point), fixed_point, IUNIT_TOL);
}

#[test]
fn rescale_around_composes_scale() {
    let t = TransformTrs::new(v(1000, 500), ANGLE_0, 1.5, 1.5);
    let rescaled = t.rescale_around(v(0, 0), 2.0, 2.0);

    assert!(is_close_pct(rescaled.scale_x(), 3.0, 1e-6));
    assert!(is_close_pct(rescaled.scale_y(), 3.0, 1e-6));
}

#[test]
fn rescale_around_anchor_follows_center() {
    // 2x rescale around (0, 0): anchor (1000, 500) goes to (2000, 1000).
    let t = TransformTrs::new(v(1000, 500), ANGLE_0, 1.0, 1.0);
    let r = t.rescale_around(v(0, 0), 2.0, 2.0);
    check_vec_near(r.translate(), v(2000, 1000), 0);
}

#[test]
fn rescale_around_rotated_non_uniform_scales_local_axes() {
    // The scale factors act in the footprint's own frame, so a non-uniform
    // rescale must apply directly (Sx *= aSx, Sy *= aSy) and never swap axes
    // with the rotation.
    let t = TransformTrs::new(v(1000, 500), deg(90.0), 1.0, 1.0);
    let r = t.rescale_around(v(0, 0), 2.0, 3.0);

    assert!(is_close_pct(r.scale_x(), 2.0, 1e-6));
    assert!(is_close_pct(r.scale_y(), 3.0, 1e-6));
}

#[test]
fn rescale_around_rotated_fixed_point_invariant() {
    // The fixed point stays put even with rotation and a non-uniform scale.
    let t = TransformTrs::new(v(1000, 500), deg(90.0), 1.0, 1.0);
    let lib_point = v(25, 40);
    let fixed_point = t.apply(lib_point);

    let r = t.rescale_around(fixed_point, 2.0, 3.0);
    check_vec_near(r.apply(lib_point), fixed_point, IUNIT_TOL);
}

#[test]
fn equality_and_inequality() {
    let a = TransformTrs::new(v(100, 200), ANGLE_45, 1.5, 0.5);
    let b = TransformTrs::new(v(100, 200), ANGLE_45, 1.5, 0.5);
    let c = TransformTrs::new(v(100, 200), ANGLE_45, 1.5, 0.6);

    assert!(a == b);
    assert!(!(a == c));
    assert!(a != c);
}

#[test]
fn apply_linear_scale_arithmetic_mean() {
    let t = TransformTrs::new(v(0, 0), ANGLE_0, 2.0, 1.0);
    assert!(is_close_pct(t.apply_linear_scale(100.0), 150.0, 1e-9));

    let uniform = TransformTrs::new(v(0, 0), ANGLE_0, 0.5, 0.5);
    assert!(is_close_pct(uniform.apply_linear_scale(100.0), 50.0, 1e-9));
}

// A rotation composed from a matrix can leave one scale a unit in the last place off. That is
// still a uniform, unit scale, or a rotated footprint's circles and arcs turn into ellipses.
#[test]
fn rounding_noise_keeps_scale_uniform() {
    let noisy = f64::from_bits(1.0f64.to_bits() + 1); // std::nextafter( 1.0, 2.0 )
    let t = TransformTrs::new(v(0, 0), deg(2.5), 1.0, noisy);
    assert!(t.is_uniform_scale());
    assert!(t.has_unit_scale());

    let scaled = TransformTrs::new(v(0, 0), ANGLE_0, 1.0, 1.0 + 1e-6);
    assert!(!scaled.is_uniform_scale());
    assert!(!scaled.has_unit_scale());
}
