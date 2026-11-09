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

//! Port of the parts of `qa/tests/libs/kimath/math/test_wide_int.cpp` (Boost test suite
//! `WideInt`) that test `math/wide_int.h`.
//!
//! `OrientationSignBeyondDoublePrecision` and `InCircleDelaunayLegalExact` test
//! `geometry/geometry_predicates.h`, which is not part of this work package.

use kicad_geom::math::vector2::Vector2L;
use kicad_geom::math::wide_int::{KiInt128, cross_wide, dot_wide, to_double, words_to_double};
use kicad_geom::test_utils::Mt19937_64;

/// Port of the anonymous-namespace helper `sign`.
fn sign(a_value: KiInt128) -> i32 {
    (a_value > 0) as i32 - (a_value < 0) as i32
}

/// `std::ldexp( 1.0, exp )` for the exponents the test uses.
fn ldexp1(exp: i32) -> f64 {
    2.0f64.powi(exp)
}

/// Port of `BOOST_AUTO_TEST_CASE( CrossWideKnownValues )`.
#[test]
fn cross_wide_known_values() {
    assert_eq!(to_double(cross_wide(Vector2L::new(3, 4), Vector2L::new(5, 6))), -2.0);
    assert_eq!(to_double(dot_wide(Vector2L::new(3, 4), Vector2L::new(5, 6))), 39.0);

    // The (+-1.1e9) diagonals wrap an int64 cross product
    let d1 = Vector2L::new(2200000000, 2200000000);
    let d2 = Vector2L::new(2200000000, -2200000000);

    assert_eq!(sign(cross_wide(d1, d2)), -1);
    assert_eq!(sign(cross_wide(d2, d1)), 1);
    assert_eq!(sign(cross_wide(d1, d1)), 0);
    assert_eq!(to_double(cross_wide(d1, d2)), -2.0 * 2200000000.0 * 2200000000.0);

    // Products differing by 1, invisible to a double product
    let k: i64 = 3000000000;

    assert_eq!(to_double(cross_wide(Vector2L::new(k + 1, k), Vector2L::new(k + 2, k + 1))), 1.0);
    assert_eq!(to_double(cross_wide(Vector2L::new(k + 2, k + 1), Vector2L::new(k + 1, k))), -1.0);
}

/// Port of `BOOST_AUTO_TEST_CASE( Int64Extremes )`.
#[test]
fn int64_extremes() {
    const LO: i64 = i64::MIN;
    const HI: i64 = i64::MAX;

    // (-2^63)^2 - 0 = 2^126
    assert_eq!(to_double(cross_wide(Vector2L::new(LO, 0), Vector2L::new(0, LO))), ldexp1(126));
    assert_eq!(to_double(cross_wide(Vector2L::new(LO, 0), Vector2L::new(0, HI))), -ldexp1(126));
    assert_eq!(to_double(dot_wide(Vector2L::new(LO, 0), Vector2L::new(LO, 0))), ldexp1(126));
}

/// Port of `BOOST_AUTO_TEST_CASE( WordsToDoubleBorrowAndRounding )`.
#[test]
fn words_to_double_borrow_and_rounding() {
    assert_eq!(words_to_double(0, 0), 0.0);
    assert_eq!(words_to_double(0, u64::MAX), 18446744073709551616.0);
    assert_eq!(words_to_double(1, 0), ldexp1(64));
    assert_eq!(words_to_double(-1, 0), -ldexp1(64));
    assert_eq!(words_to_double(-1, u64::MAX), -1.0);

    // 2^64 + 2^11 + 1 is above the halfway point of a 53-bit mantissa, only the sticky bit rounds it up
    let up: f64 = ldexp1(64) + ldexp1(12);

    assert_eq!(words_to_double(1, (1u64 << 11) + 1), up);
    assert_eq!(words_to_double(-2, (!((1u64 << 11) + 1)).wrapping_add(1)), -up);

    // Exactly halfway rounds to even
    assert_eq!(words_to_double(1, 1u64 << 11), ldexp1(64));
}

/// `BOOST_REQUIRE_CLOSE_FRACTION( a, b, tol )`.
fn close_fraction(a: f64, b: f64, tol: f64) -> bool {
    let diff = (a - b).abs();

    diff <= tol * a.abs() && diff <= tol * b.abs()
}

/// Port of `BOOST_AUTO_TEST_CASE( RandomAgainstLongDouble )`.
///
/// The C++ reference is an x87 `long double` computation, which Rust does not have.  The
/// same operands (drawn from `std::mt19937_64( 4242 )`, whose output is fixed by the C++
/// standard) are checked against the exactly rounded value of the 128-bit result instead,
/// within the tolerance of the original test.  As in C++ the test relies on the sequence
/// never producing the one overflowing operand combination.
#[test]
fn random_against_long_double() {
    let mut rng = Mt19937_64::new(4242);

    let mut operand = || -> i64 {
        let v: i64 = rng.next_u64() as i64;

        v >> (rng.next_u64() % 64)
    };

    for _ in 0..100000 {
        // GCC evaluates constructor arguments right to left: y is drawn before x
        let ay = operand();
        let ax = operand();
        let a = Vector2L::new(ax, ay);
        let by = operand();
        let bx = operand();
        let b = Vector2L::new(bx, by);

        let cross: i128 = a.x as i128 * b.y as i128 - a.y as i128 * b.x as i128;
        let dot: i128 = a.x as i128 * b.x as i128 + a.y as i128 * b.y as i128;

        assert!(close_fraction(to_double(cross_wide(a, b)), cross as f64, 1e-9));
        assert!(close_fraction(to_double(dot_wide(a, b)), dot as f64, 1e-9));
        assert_eq!(cross_wide(a, b), cross);
        assert_eq!(dot_wide(a, b), dot);
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( WordsToDoubleAgainstInt128 )`.
#[test]
fn words_to_double_against_int128() {
    let mut rng = Mt19937_64::new(987);

    for i in 0..300000 {
        // ( static_cast<unsigned __int128>( rng() ) << 64 ) | rng(): GCC evaluates the right
        // operand of | first here; the order does not matter for the property tested.
        let lo: u128 = rng.next_u64() as u128;
        let hi: u128 = rng.next_u64() as u128;
        let mut mag: u128 = (hi << 64) | lo;
        mag >>= rng.next_u64() % 128;

        // Sticky and tie patterns need many trailing zeros
        if i % 3 == 0 {
            mag &= !((1u128 << (rng.next_u64() % 100)) - 1);
        }

        let v: i128 = mag as i128;

        assert_eq!(words_to_double((v >> 64) as i64, v as u64), v as f64);
    }
}
