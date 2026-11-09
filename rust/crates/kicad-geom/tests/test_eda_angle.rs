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

//! Port of `qa/tests/libs/kimath/geometry/test_eda_angle.cpp` (Boost test suite `EdaAngle`).

use kicad_geom::geometry::eda_angle::{
    ANGLE_0, ANGLE_45, ANGLE_90, ANGLE_135, ANGLE_180, ANGLE_270, ANGLE_360, ANGLE_HORIZONTAL, ANGLE_VERTICAL,
    DEGREES_T, EdaAngle, EdaOrientation, FULL_CIRCLE,
};

/// Port of `EDA_ANGLE_NORMALISE_CASE`.
struct EdaAngleNormaliseCase {
    angle: f64,

    // Expected:
    exp_normalized: f64,
    exp_normalized_negative: f64,
    exp_normalized90: f64,
    exp_normalized180: f64,
    exp_normalized720: f64,
}

const fn nc(
    angle: f64,
    exp_normalized: f64,
    exp_normalized_negative: f64,
    exp_normalized90: f64,
    exp_normalized180: f64,
    exp_normalized720: f64,
) -> EdaAngleNormaliseCase {
    EdaAngleNormaliseCase {
        angle,
        exp_normalized,
        exp_normalized_negative,
        exp_normalized90,
        exp_normalized180,
        exp_normalized720,
    }
}

/// Port of `normalize_cases`.
#[rustfmt::skip]
const NORMALIZE_CASES: &[EdaAngleNormaliseCase] = &[
    //@todo: should we unify the ranges of Normalize180, Normalize720 to be the same
    // as Normalize90 (i.e. inclusive of both sides of the range)?

    //                 [0,360)   (-360,0]    [-90,90]   (-180,180]   [-360,360)
    // Original     Normalized    NormNeg      Norm90      Norm180      Norm720
    nc(       90.0,        90.0,    -270.0,       90.0,        90.0,        90.0 ),
    nc(      -90.0,       270.0,     -90.0,      -90.0,       -90.0,       -90.0 ),
    nc(      135.0,       135.0,    -225.0,      -45.0,       135.0,       135.0 ),
    nc(     -135.0,       225.0,    -135.0,       45.0,      -135.0,      -135.0 ),
    nc(      180.0,       180.0,    -180.0,        0.0,       180.0,       180.0 ),
    nc(     -180.0,       180.0,    -180.0,        0.0,       180.0,      -180.0 ),
    nc(      360.0,         0.0,       0.0,        0.0,         0.0,         0.0 ),
    nc(     -360.0,         0.0,       0.0,        0.0,         0.0,      -360.0 ),
    nc(      390.0,        30.0,    -330.0,       30.0,        30.0,        30.0 ),
    nc(     -390.0,       330.0,     -30.0,      -30.0,       -30.0,       -30.0 ),
    nc(      720.0,         0.0,       0.0,        0.0,         0.0,         0.0 ),
    nc(     -720.0,         0.0,       0.0,        0.0,         0.0,      -360.0 ),
];

/// Port of `BOOST_AUTO_TEST_CASE( Normalize )`.
#[test]
fn normalize() {
    for c in NORMALIZE_CASES {
        let ctx = format!("Original angle: {} degrees", c.angle);

        let mut normalized = EdaAngle::new(c.angle, DEGREES_T);
        normalized.normalize();

        let mut normalized_negative = EdaAngle::new(c.angle, DEGREES_T);
        normalized_negative.normalize_negative();

        let mut normalized90 = EdaAngle::new(c.angle, DEGREES_T);
        normalized90.normalize90();

        let mut normalized180 = EdaAngle::new(c.angle, DEGREES_T);
        normalized180.normalize180();

        let mut normalized720 = EdaAngle::new(c.angle, DEGREES_T);
        normalized720.normalize720();

        assert_eq!(normalized.as_degrees(), c.exp_normalized, "{ctx}");
        assert_eq!(normalized_negative.as_degrees(), c.exp_normalized_negative, "{ctx}");
        assert_eq!(normalized90.as_degrees(), c.exp_normalized90, "{ctx}");
        assert_eq!(normalized180.as_degrees(), c.exp_normalized180, "{ctx}");
        assert_eq!(normalized720.as_degrees(), c.exp_normalized720, "{ctx}");
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( NormalizeHugeAndNonFinite )`.
#[test]
fn normalize_huge_and_non_finite() {
    // The old loops leave NaN alone and never finish on inf or 1e19, so fail here before reaching those
    assert_eq!(EdaAngle::new(f64::NAN, DEGREES_T).normalize180().as_degrees(), 0.0);

    // 1e19 is exact and 1e19 mod 360 = 280
    //                 [0,360)   (-360,0]    [-90,90]   (-180,180]   [-360,360)
    #[rustfmt::skip]
    let cases: Vec<EdaAngleNormaliseCase> = vec![
        nc(           1e19,       280.0,     -80.0,      -80.0,       -80.0,       280.0 ),
        nc(          -1e19,        80.0,    -280.0,       80.0,        80.0,      -280.0 ),
        nc(  f64::INFINITY,         0.0,       0.0,        0.0,         0.0,         0.0 ),
        nc( -f64::INFINITY,         0.0,       0.0,        0.0,         0.0,         0.0 ),
        nc(       f64::NAN,         0.0,       0.0,        0.0,         0.0,         0.0 ),
    ];

    for c in &cases {
        let ctx = format!("Original angle: {} degrees", c.angle);

        assert_eq!(EdaAngle::new(c.angle, DEGREES_T).normalize().as_degrees(), c.exp_normalized, "{ctx}");
        assert_eq!(
            EdaAngle::new(c.angle, DEGREES_T).normalize_negative().as_degrees(),
            c.exp_normalized_negative,
            "{ctx}"
        );
        assert_eq!(EdaAngle::new(c.angle, DEGREES_T).normalize90().as_degrees(), c.exp_normalized90, "{ctx}");
        assert_eq!(EdaAngle::new(c.angle, DEGREES_T).normalize180().as_degrees(), c.exp_normalized180, "{ctx}");
        assert_eq!(EdaAngle::new(c.angle, DEGREES_T).normalize720().as_degrees(), c.exp_normalized720, "{ctx}");
    }

    // 9e18 is an exact multiple of 180
    assert!(EdaAngle::new(9e18, DEGREES_T).is_cardinal());
    assert!(!EdaAngle::new(9e18, DEGREES_T).is_cardinal90());

    for bad in [f64::INFINITY, -f64::INFINITY] {
        assert!(!EdaAngle::new(bad, DEGREES_T).is_cardinal());
        assert!(!EdaAngle::new(bad, DEGREES_T).is_cardinal90());
    }
}

/// Port of the static helper `refStep`.
///
/// Reference copy of the stepping loops that normalization used before exact folding.  They
/// are exact at the magnitudes sampled here, so the folded versions must match them bit for
/// bit, signed zero included.
fn ref_step(a_value: f64, a_low: f64, a_low_incl: bool, a_high: f64, a_high_incl: bool, a_period: f64) -> f64 {
    let mut a_value = a_value;

    while if a_low_incl { a_value < a_low } else { a_value <= a_low } {
        a_value += a_period;
    }

    while if a_high_incl { a_value > a_high } else { a_value >= a_high } {
        a_value -= a_period;
    }

    a_value
}

/// Port of the static helper `refIsCardinal`.
fn ref_is_cardinal(a_value: f64) -> bool {
    let mut a_value = a_value;

    while a_value < 0.0 {
        a_value += 90.0;
    }

    while a_value >= 90.0 {
        a_value -= 90.0;
    }

    a_value == 0.0
}

/// Port of the static helper `refIsCardinal90`.
fn ref_is_cardinal90(a_value: f64) -> bool {
    let mut a_value = a_value.abs();

    while a_value >= 180.0 {
        a_value -= 180.0;
    }

    a_value == 90.0
}

/// Port of the static helper `deg`.
fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
}

/// Port of the static helper `orientation`.
fn orientation(d: f64) -> EdaOrientation {
    EdaOrientation::new(deg(d))
}

/// `std::nextafter( x, to )` for finite `x` and an infinite `to` (the only way the test uses
/// it).
fn nextafter(x: f64, to: f64) -> f64 {
    assert!(x.is_finite() && to.is_infinite());

    let up: bool = to > 0.0;

    if x == 0.0 {
        // The smallest subnormal with the sign of the direction
        return if up { f64::from_bits(1) } else { -f64::from_bits(1) };
    }

    let bits: u64 = x.to_bits();

    // Moving away from zero increments the magnitude bits, moving towards zero decrements
    if (x > 0.0) == up { f64::from_bits(bits + 1) } else { f64::from_bits(bits - 1) }
}

/// Port of `BOOST_AUTO_TEST_CASE( NormalizeMatchesSteppingLoops )`.
#[test]
fn normalize_matches_stepping_loops() {
    let mut inputs: Vec<f64> = Vec::new();

    for i in -40..=40 {
        let base: f64 = 45.0 * i as f64;
        inputs.extend_from_slice(&[
            base,
            nextafter(base, -f64::INFINITY),
            nextafter(base, f64::INFINITY),
            base - 1e-20,
            base + 1e-20,
        ]);
    }

    for i in -5000..=5000 {
        inputs.push(i as f64 * 199.87654321);
    }

    inputs.extend_from_slice(&[0.0, -0.0, 1e-300, -1e-300]);

    let same_bits = |a: f64, b: f64| -> bool { a == b && a.is_sign_negative() == b.is_sign_negative() };

    for x in inputs {
        let ctx = format!("Original angle: {x:.17e} degrees");

        assert!(
            same_bits(
                EdaAngle::new(x, DEGREES_T).normalize().as_degrees(),
                ref_step(x, 0.0, true, 360.0, false, 360.0)
            ),
            "{ctx}"
        );
        assert!(
            same_bits(
                EdaAngle::new(x, DEGREES_T).normalize_negative().as_degrees(),
                ref_step(x, -360.0, false, 0.0, true, 360.0)
            ),
            "{ctx}"
        );
        assert!(
            same_bits(
                EdaAngle::new(x, DEGREES_T).normalize90().as_degrees(),
                ref_step(x, -90.0, true, 90.0, true, 180.0)
            ),
            "{ctx}"
        );
        assert!(
            same_bits(
                EdaAngle::new(x, DEGREES_T).normalize180().as_degrees(),
                ref_step(x, -180.0, false, 180.0, true, 360.0)
            ),
            "{ctx}"
        );
        assert!(
            same_bits(
                EdaAngle::new(x, DEGREES_T).normalize720().as_degrees(),
                ref_step(x, -360.0, true, 360.0, false, 360.0)
            ),
            "{ctx}"
        );
        assert_eq!(EdaAngle::new(x, DEGREES_T).is_cardinal(), ref_is_cardinal(x), "{ctx}");
        assert_eq!(EdaAngle::new(x, DEGREES_T).is_cardinal90(), ref_is_cardinal90(x), "{ctx}");
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( ConstantAngles )`.
#[test]
fn constant_angles() {
    assert_eq!(ANGLE_0.as_degrees(), 0.0);
    assert_eq!(ANGLE_45.as_degrees(), 45.0);
    assert_eq!(ANGLE_90.as_degrees(), 90.0);
    assert_eq!(ANGLE_135.as_degrees(), 135.0);
    assert_eq!(ANGLE_180.as_degrees(), 180.0);
    assert_eq!(ANGLE_270.as_degrees(), 270.0);
    assert_eq!(ANGLE_360.as_degrees(), 360.0);

    assert_eq!(ANGLE_HORIZONTAL.as_degrees(), 0.0);
    assert_eq!(ANGLE_VERTICAL.as_degrees(), 90.0);
    assert_eq!(FULL_CIRCLE.as_degrees(), 360.0);
}

/// Port of `BOOST_AUTO_TEST_CASE( Snapped )`.
#[test]
fn snapped() {
    assert_eq!(deg(80.0).snapped(ANGLE_90).as_degrees(), 90.0);
    assert_eq!(deg(30.0).snapped(ANGLE_90).as_degrees(), 0.0);
    assert_eq!(deg(135.0).snapped(ANGLE_90).as_degrees(), 180.0);
    assert_eq!(deg(45.0).snapped(ANGLE_90).as_degrees(), 90.0);
    assert_eq!(deg(-10.0).snapped(ANGLE_90).as_degrees(), 0.0);
    assert_eq!(deg(80.0).snapped(ANGLE_45).as_degrees(), 90.0);
    assert_eq!(deg(5.0).snapped(ANGLE_0).as_degrees(), 5.0); // step <= 0 guard

    // Minimal equivalent rotation used by grid-aware placement: raw - raw.Snapped(period).
    let residual = |raw: f64, period: EdaAngle| -> f64 {
        let a: EdaAngle = deg(raw);
        (a - a.snapped(period)).as_degrees()
    };

    assert_eq!(residual(80.0, ANGLE_90), -10.0);
    assert_eq!(residual(30.0, ANGLE_90), 30.0);
    assert_eq!(residual(135.0, ANGLE_90), -45.0);
    assert_eq!(residual(0.0, ANGLE_90), 0.0);
}

/// Port of `BOOST_AUTO_TEST_CASE( AngleReflect )`.
#[test]
fn angle_reflect() {
    let check_reflect = |a_angle: f64, a_axis: f64, a_expected: f64| {
        let ctx = format!("Testing reflect( {a_angle} about {a_axis} ) -> {a_expected}");

        let angle: EdaAngle = deg(a_angle);
        let axis: EdaAngle = deg(a_axis);

        let reflected: EdaAngle = angle.reflect(axis);
        assert_eq!(reflected.normalized().as_degrees(), a_expected, "{ctx}");

        // And check the reverse returns
        let reversed: EdaAngle = reflected.reflect(axis);

        assert_eq!(reversed.normalized().as_degrees(), angle.normalized().as_degrees(), "{ctx}");

        // And check that and axis reversal has no effect on the reflection result.
        let reversed_axis: EdaAngle = axis + deg(180.0);
        let reflected_with_reversed_axis: EdaAngle = angle.reflect(reversed_axis);
        assert_eq!(reflected_with_reversed_axis.normalized().as_degrees(), a_expected, "{ctx}");
    };

    //   ^
    //   | A = 90
    // ------
    //   | A' = -90
    //   v
    check_reflect(90.0, 0.0, 270.0);

    //      |
    //   /  |  \
    //   A  |  A'
    check_reflect(45.0, 90.0, 135.0);

    //  | A  /
    //  v   /
    //     /
    //    /   <---- A'
    //
    // Note: reflecting in a non-cardinal axis is the only time a sign error manifests, so this
    // one is probably the most critical test.
    check_reflect(-90.0, 45.0, 180.0);

    //   /    /
    //  v    /    /
    //      /    v
    check_reflect(45.0, 45.0, 45.0);
}

/// Port of `BOOST_AUTO_TEST_CASE( Orientation )`.
#[test]
fn orientation_test() {
    // Construction normalizes into [0, 360), and equality compares canonical values.
    assert_eq!(orientation(370.0).angle().as_degrees(), 10.0);
    assert_eq!(orientation(-90.0).angle().as_degrees(), 270.0);
    assert!(orientation(370.0) == orientation(10.0));

    // Addition and subtraction wrap in every form.
    let mut turn: EdaOrientation = orientation(350.0);
    assert_eq!((turn + deg(30.0)).angle().as_degrees(), 20.0);
    assert_eq!((deg(30.0) + turn).angle().as_degrees(), 20.0);
    turn += deg(30.0);
    assert_eq!(turn.angle().as_degrees(), 20.0);
    turn -= deg(30.0);
    assert_eq!(turn.angle().as_degrees(), 350.0);

    // Negation and subtraction
    assert_eq!((-orientation(45.0)).angle().as_degrees(), 315.0);
    // Wrapping subtraction
    assert_eq!((ANGLE_180 - orientation(270.0)).angle().as_degrees(), 270.0);
    // Orientation minus angle, with wrapping
    assert_eq!((orientation(10.0) - deg(350.0)).angle().as_degrees(), 20.0);

    // A difference of orientations -> signed rotation amount (EDA_ANGLE, not EDA_ORIENTATION)
    let diff: EdaAngle = orientation(10.0) - orientation(350.0);
    assert_eq!(diff.as_degrees(), -340.0);

    // -0 becomes 0
    assert!(!(-orientation(0.0)).angle().as_degrees().is_sign_negative());
}

/// Port of `BOOST_AUTO_TEST_CASE( OrientationReflect )`.
#[test]
fn orientation_reflect() {
    // Reflecting an orientation is just reflecting its angle over the given axis.
    assert_eq!(orientation(30.0).reflect(deg(90.0)).angle().as_degrees(), 150.0);
    assert_eq!(orientation(0.0).reflect(deg(45.0)).angle().as_degrees(), 90.0);
}
