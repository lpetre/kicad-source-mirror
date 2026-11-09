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

//! Port of `qa/tests/libs/kimath/math/test_util.cpp` (Boost test suite `MathUtil`),
//! plus extra unit tests for the numeric edge cases of `KiROUND`.

use kicad_geom::math::util::{KiRoundArg, ki_round, ki_round_i64, ki_round_to, rescale};

struct TestRescaleI64Case {
    numerator: i64,
    value: i64,
    denominator: i64,
    result: i64,
}

const fn c(numerator: i64, value: i64, denominator: i64, result: i64) -> TestRescaleI64Case {
    TestRescaleI64Case {
        numerator,
        value,
        denominator,
        result,
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( test_rescale_int64 )`.
#[test]
fn test_rescale_int64() {
    // Order: numerator, value, denominator, result.
    #[rustfmt::skip]
    let rescale_i64_cases: Vec<TestRescaleI64Case> = vec![
        c( 10, 10, 1, 100 ),
        c( 10, 10, -1, -100 ),
        c( 10, -10, 1, -100 ),
        c( 10, -10, -1, 100 ),

        c( 1, 9, 1, 9 ),
        c( 1, 9, -1, -9 ),
        c( 1, -9, 1, -9 ),
        c( 1, -9, -1, 9 ),

        c( 10, 10, 2, 50 ),
        c( 10, 10, -2, -50 ),
        c( 10, -10, 2, -50 ),
        c( 10, -10, -2, 50 ),

        c( 1, 9, 2, 5 ),
        c( 1, 9, -2, -5 ),
        c( 1, -9, 2, -5 ),
        c( 1, -9, -2, 5 ),

        c( 1, 17, 4, 4 ),
        c( 1, 17, -4, -4 ),
        c( 1, -17, 4, -4 ),
        c( 1, -17, -4, 4 ),

        c( 1, 19, 4, 5 ),
        c( 1, 19, -4, -5 ),
        c( 1, -19, 4, -5 ),
        c( 1, -19, -4, 5 ),

        c( 1, 0, 4, 0 ),
        c( 1, 0, -4, 0 ),
        c( -1, 0, 4, 0 ),
        c( -1, 0, -4, 0 ),

        // sqrt(2^63) = 3037000499.98..
        c( 3037000499, 3037000499, 1, 9223372030926249001 ),
        c( 3037000499, 3037000499, -1, -9223372030926249001 ),
        c( 3037000499, -3037000499, 1, -9223372030926249001 ),
        c( 3037000499, -3037000499, -1, 9223372030926249001 ),

        // sqrt(2^63 * 10) = 9603838834.99..
        c( 9603838834, 9603838834, 10, 9223372034944647956 ),
        c( 9603838834, 9603838834, -10, -9223372034944647956 ),
        c( 9603838834, -9603838834, 10, -9223372034944647956 ),
        c( 9603838834, -9603838834, -10, 9223372034944647956 ),

        // INT64_MAX = 9223372036854775807
        c( i64::MAX, 10, 10, i64::MAX ),
        c( i64::MAX, 10, -10, -i64::MAX ),
        c( i64::MAX, -10, 10, -i64::MAX ),
        c( i64::MAX, -10, -10, i64::MAX ),

        c( i64::MAX, 10, i64::MAX, 10 ),
        c( i64::MAX, 10, -i64::MAX, -10 ),
        c( i64::MAX, -10, i64::MAX, -10 ),
        c( i64::MAX, -10, -i64::MAX, 10 ),

        c( i64::MAX, i64::MAX, i64::MAX, i64::MAX ),
        c( i64::MAX, i64::MAX, -i64::MAX, -i64::MAX ),
        c( i64::MAX, -i64::MAX, i64::MAX, -i64::MAX ),
        c( i64::MAX, -i64::MAX, -i64::MAX, i64::MAX ),
    ];

    for entry in &rescale_i64_cases {
        let calculated: i64 = rescale(entry.numerator, entry.value, entry.denominator);

        assert!(
            calculated == entry.result,
            "rescale<int64_t>( {}, {}, {} ) failed. \nExpected: {}\nGot: {}",
            entry.numerator,
            entry.value,
            entry.denominator,
            entry.result,
            calculated
        );
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( KiROUNDSaturates )`.
///
/// The C++ passes `aQuiet = true` to suppress the debug-build overflow assertion; the port
/// never asserts, so there is no such parameter.
#[test]
fn ki_round_saturates() {
    let inf = f64::INFINITY;
    let nan = f64::NAN;

    assert_eq!(ki_round(1e30), i32::MAX);
    assert_eq!(ki_round(-1e30), i32::MIN);
    assert_eq!(ki_round(inf), i32::MAX);
    assert_eq!(ki_round(-inf), i32::MIN);
    assert_eq!(ki_round(nan), 0);
    assert_eq!(ki_round(2147483647.4), i32::MAX);
    assert_eq!(ki_round(2147483647.5), i32::MAX);
    assert_eq!(ki_round(-2147483648.5), i32::MIN);

    assert_eq!(ki_round_to::<i64, f64>(1e19), i64::MAX);
    assert_eq!(ki_round_to::<i64, f64>(9223372036854775808.0), i64::MAX); // 0x1p63

    assert_eq!(ki_round(1e30f32), i32::MAX);
    assert_eq!(ki_round(f32::NAN), 0);

    assert_eq!(ki_round(i64::MAX), i32::MAX); // LLONG_MAX
    assert_eq!(ki_round_to::<i64, i64>((1i64 << 53) + 1), (1i64 << 53) + 1);
}

// ---------------------------------------------------------------------------------------
// Additional tests (not in the C++ suite) for the numeric behaviour of the port.
// ---------------------------------------------------------------------------------------

#[test]
fn ki_round_rounds_half_away_from_zero() {
    assert_eq!(ki_round(0.0), 0);
    assert_eq!(ki_round(-0.0), 0);
    assert_eq!(ki_round(0.4999999999999999), 0);
    assert_eq!(ki_round(0.5), 1);
    assert_eq!(ki_round(-0.5), -1);
    assert_eq!(ki_round(1.5), 2);
    assert_eq!(ki_round(-1.5), -2);
    assert_eq!(ki_round(2.5), 3);
    assert_eq!(ki_round(-2.5), -3);
    assert_eq!(ki_round(2.4999999999999996), 2);
    assert_eq!(ki_round(1234567.49), 1234567);
    assert_eq!(ki_round(-1234567.51), -1234568);

    // The largest double below 0.5 must not round up (as a naive floor( v + 0.5 ) would)
    assert_eq!(ki_round(0.49999999999999994), 0);
    assert_eq!(ki_round(-0.49999999999999994), 0);

    // Above 2^52 every double is an integer; a naive v + 0.5 would change the value
    assert_eq!(ki_round_i64(4503599627370497.0), 4503599627370497);
    assert_eq!(ki_round_i64(-4503599627370497.0), -4503599627370497);
    assert_eq!(ki_round_i64(9007199254740993.0), 9007199254740992);
}

#[test]
fn ki_round_int_limits() {
    // Values that round to exactly INT_MAX / INT_MIN are in range
    assert_eq!(ki_round(2147483646.5), i32::MAX);
    assert_eq!(ki_round(2147483646.4999995), 2147483646);
    assert_eq!(ki_round(2147483647.0), i32::MAX);
    assert_eq!(ki_round(2147483647.4999995), i32::MAX);
    assert_eq!(ki_round(-2147483647.5), i32::MIN);
    assert_eq!(ki_round(-2147483648.0), i32::MIN);
    assert_eq!(ki_round(-2147483648.4999995), i32::MIN);
    assert_eq!(ki_round(-2147483647.4999995), -2147483647);
    assert_eq!(ki_round(2147483648.0), i32::MAX);
    assert_eq!(ki_round(-2147483649.0), i32::MIN);
    assert_eq!(ki_round(f64::MAX), i32::MAX);
    assert_eq!(ki_round(f64::MIN), i32::MIN);
    assert_eq!(ki_round(f64::MIN_POSITIVE), 0);
    assert_eq!(ki_round(-f64::NAN), 0);
}

#[test]
fn ki_round_int64_limits() {
    assert_eq!(ki_round_i64(9223372036854774784.0), 9223372036854774784); // largest double below 2^63
    assert_eq!(ki_round_i64(9223372036854775808.0), i64::MAX);
    assert_eq!(ki_round_i64(-9223372036854775808.0), i64::MIN);
    assert_eq!(ki_round_i64(-9223372036854777856.0), i64::MIN); // next double below -2^63
    assert_eq!(ki_round_i64(f64::INFINITY), i64::MAX);
    assert_eq!(ki_round_i64(f64::NEG_INFINITY), i64::MIN);
    assert_eq!(ki_round_i64(f64::NAN), 0);
    assert_eq!(ki_round_i64(0.5), 1);
    assert_eq!(ki_round_i64(-0.5), -1);
    assert_eq!(ki_round_i64(3000000000.5), 3000000001);
}

#[test]
fn ki_round_float_argument() {
    assert_eq!(ki_round(0.5f32), 1);
    assert_eq!(ki_round(-0.5f32), -1);
    assert_eq!(ki_round(2.5f32), 3);
    assert_eq!(ki_round(16777217.0f32), 16777216);
    assert_eq!(ki_round(2147483648.0f32), i32::MAX);
    assert_eq!(ki_round(2147483520.0f32), 2147483520); // largest float below 2^31
    assert_eq!(ki_round(-2147483648.0f32), i32::MIN);
    assert_eq!(ki_round(-2147483904.0f32), i32::MIN); // next float below -2^31
    assert_eq!(ki_round(f32::INFINITY), i32::MAX);
    assert_eq!(ki_round(f32::NEG_INFINITY), i32::MIN);
    assert_eq!(ki_round_i64(1e30f32), i64::MAX);
    assert_eq!(ki_round_i64(-1e30f32), i64::MIN);
}

#[test]
fn ki_round_integral_arguments_clamp() {
    assert_eq!(ki_round(5i32), 5);
    assert_eq!(ki_round(-5i64), -5);
    assert_eq!(ki_round(i64::MIN), i32::MIN);
    assert_eq!(ki_round(3_000_000_000u32), i32::MAX);
    assert_eq!(ki_round(u64::MAX), i32::MAX);
    assert_eq!(ki_round_i64(u64::MAX), i64::MAX);
    assert_eq!(ki_round_i64(i64::MIN), i64::MIN);
    assert_eq!(ki_round_to::<u32, i32>(-1), 0);
    assert_eq!(ki_round_to::<u32, i64>(1 << 40), u32::MAX);
    assert_eq!(ki_round_to::<u32, u64>(123), 123);
}

#[test]
fn ki_round_unsigned_return_type() {
    assert_eq!(ki_round_to::<u32, f64>(-0.4), 0);
    assert_eq!(ki_round_to::<u32, f64>(-0.5), 0);
    assert_eq!(ki_round_to::<u32, f64>(-1e30), 0);
    assert_eq!(ki_round_to::<u32, f64>(0.5), 1);
    assert_eq!(ki_round_to::<u32, f64>(4294967294.5), u32::MAX);
    assert_eq!(ki_round_to::<u32, f64>(4294967295.4), u32::MAX);
    assert_eq!(ki_round_to::<u32, f64>(1e30), u32::MAX);
    assert_eq!(ki_round_to::<u32, f64>(f64::NAN), 0);
    assert_eq!(3000000000.4f64.ki_round_u32(), 3000000000);
}

#[test]
fn rescale_int_specialization() {
    // rescale<int>: the product is computed in 64 bits and rounded to nearest
    assert_eq!(rescale(1_000_000i32, 1_000_000, 1_000_000), 1_000_000);
    assert_eq!(rescale(i32::MAX, i32::MAX, i32::MAX), i32::MAX);
    assert_eq!(rescale(i32::MAX, -i32::MAX, i32::MAX), -i32::MAX);
    assert_eq!(rescale(7i32, 3, 2), 11); // 10.5 -> 11
    assert_eq!(rescale(-7i32, 3, 2), -11);
    assert_eq!(rescale(7i32, 3, -2), -11);
    assert_eq!(rescale(-7i32, 3, -2), 11);
    assert_eq!(rescale(5i32, 1, 3), 2); // 1.67 -> 2
    assert_eq!(rescale(4i32, 1, 3), 1); // 1.33 -> 1
}
