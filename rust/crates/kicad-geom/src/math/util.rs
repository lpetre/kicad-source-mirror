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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (c) 2005 Michael Niedermayer <michaelni@gmx.at>
// Copyright (C) CERN
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>
//
// The equals() method to compare two floating point values adapted from
// AlmostEqualRelativeAndAbs() on
// https://randomascii.wordpress.com/2012/02/25/comparing-floating-point-numbers-2012-edition/
// (C) Bruce Dawson subject to the Apache 2.0 license.

//! Port of `libs/kimath/include/math/util.h` and `libs/kimath/src/math/util.cpp`.
//!
//! The C++ templates are mapped as follows:
//!
//! | C++ | Rust |
//! |---|---|
//! | `KiROUND( v )` (`ret_type = int`) | [`ki_round`] |
//! | `KiROUND<fp_type, ret_type>( v )` | [`ki_round_to`] (`ki_round_to::<i64, _>( v )`), [`ki_round_i64`] |
//! | `KiCheckedCast<in_type, ret_type>( v )` | [`ki_checked_cast`] |
//! | `rescale<T>( n, v, d )` | [`rescale`] |
//! | `sign( v )` | [`sign`] |
//! | `ct_sqrt<T>( x )` | [`ct_sqrt_u64`], [`ct_sqrt_i64`], ... |
//! | `isqrt<T>( x )` | [`isqrt`] |
//! | `equals( a, b, eps )` | [`equals`], [`equals_eps`] |
//!
//! `std::clamp`, `std::min` and `std::max` have precisely defined behaviour for unordered
//! (NaN) operands which differs from Rust's `f64::clamp`/`min`/`max`; [`std_clamp`],
//! [`std_min`] and [`std_max`] reproduce the C++ definitions and are used wherever the C++
//! applies those functions to floating point values.

/// Port of `kimathLogDebug`: helper to avoid directly including wx/log.h for the templated
/// functions in kimath.
///
/// The C++ function forwards to `wxVLogWarning` when debug logging is enabled.  The port has
/// no logging backend (library crates must not touch the environment), so this is a no-op.
#[inline]
pub fn kimath_log_debug(_message: &str) {}

/// Port of `kimathLogOverflow`.
///
/// In C++ this raises a `wxFAIL_MSG` ("Overflow converting value %f to %s."), which only has
/// an effect in debug builds.  The port implements the release-build behaviour: nothing
/// happens here and the caller returns the clamped value.
#[inline]
pub fn kimath_log_overflow(_v: f64, _type_name: &str) {}

/// Port of `std::clamp( v, lo, hi )`: `( v < lo ) ? lo : ( hi < v ) ? hi : v`.
///
/// Unlike `f64::clamp` this never panics and passes NaN values through unchanged.
#[inline]
pub fn std_clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// Port of `std::min( a, b )`: `( b < a ) ? b : a`.
#[inline]
pub fn std_min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a { b } else { a }
}

/// Port of `std::max( a, b )`: `( a < b ) ? b : a`.
#[inline]
pub fn std_max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// Port of the `in_type`/`ret_type` pairs of `KiCheckedCast` that kimath instantiates.
pub trait KiCheckedCast<Ret> {
    /// Perform the cast; see [`ki_checked_cast`].
    fn ki_checked_cast(self) -> Ret;
}

impl KiCheckedCast<i32> for i64 {
    /// The `long long int` -> `int` branch of `KiCheckedCast`: clamps to the `int` range.
    ///
    /// Note: on LP64 Linux `int64_t` is `long`, not `long long`, so the C++ instantiation
    /// `KiCheckedCast<int64_t, int>` used by `BOX2I` does *not* take this branch there and
    /// silently wraps instead, while Windows and macOS builds clamp.  The port follows the
    /// documented ("Will clamp the return value to numerical type limits") behaviour on all
    /// platforms.  See `PORT_STATUS.md`.
    #[inline]
    fn ki_checked_cast(self) -> i32 {
        if self > i32::MAX as i64 {
            kimath_log_overflow(self as f64, "int");

            i32::MAX
        } else if self < i32::MIN as i64 {
            kimath_log_overflow(self as f64, "int");

            i32::MIN
        } else {
            self as i32
        }
    }
}

impl KiCheckedCast<i64> for i64 {
    #[inline]
    fn ki_checked_cast(self) -> i64 {
        self
    }
}

impl KiCheckedCast<f64> for f64 {
    #[inline]
    fn ki_checked_cast(self) -> f64 {
        self
    }
}

/// Port of `KiCheckedCast<in_type, ret_type>`.
///
/// Perform a cast between numerical types. Will clamp the return value to numerical type
/// limits (only implemented by the C++ for `long long` -> `int`; all other instantiations are
/// plain conversions).
///
/// In C++ Debug builds an assert fires if the value will not fit into the return type; the
/// port never asserts (see [`kimath_log_overflow`]).
#[inline]
pub fn ki_checked_cast<In: KiCheckedCast<Ret>, Ret>(v: In) -> Ret {
    v.ki_checked_cast()
}

/// The floating point branch of `KiROUND<fp_type, ret_type>`.
macro_rules! ki_round_float_impl {
    ($v:expr, $fp:ty, $ret:ty, $name:literal) => {{
        let v: $fp = $v;

        // llround is unspecified for NaN and for results beyond long long, so saturate before
        // calling it.  v != v stands in for std::isnan, which is not constexpr until C++23.
        #[allow(clippy::eq_op, clippy::unnecessary_cast)]
        let result: $ret = if v != v {
            kimath_log_overflow(v as f64, $name);

            0
        } else if (v as f64) >= ((<$ret>::MAX as $fp) as f64) + 0.5 {
            kimath_log_overflow(v as f64, $name);

            <$ret>::MAX
        } else if ((v - (<$ret>::MIN as $fp)) as f64) <= -0.5 {
            // lowest - 0.5 can round to lowest itself, which llround still handles exactly
            kimath_log_overflow(v as f64, $name);

            <$ret>::MIN
        } else {
            // std::llround: round halfway cases away from zero
            let rounded: i64 = v.round() as i64;
            let clamped: i64 = rounded.clamp(<$ret>::MIN as i64, <$ret>::MAX as i64);

            if clamped != rounded {
                kimath_log_overflow(v as f64, $name);
            }

            clamped as $ret
        };

        result
    }};
}

/// The integral branch of `KiROUND<fp_type, ret_type>` (`std::cmp_greater`/`std::cmp_less`).
macro_rules! ki_round_int_impl {
    ($v:expr, $ret:ty, $name:literal) => {{
        let v = $v;

        // llround would convert through double and lose precision past 2^53
        #[allow(clippy::unnecessary_cast)]
        let result: $ret = if (v as i128) > (<$ret>::MAX as i128) {
            kimath_log_overflow(v as f64, $name);

            <$ret>::MAX
        } else if (v as i128) < (<$ret>::MIN as i128) {
            kimath_log_overflow(v as f64, $name);

            <$ret>::MIN
        } else {
            v as $ret
        };

        result
    }};
}

/// Argument types of `KiROUND` (the `fp_type` template parameter).
///
/// Implemented for `f64` and `f32` (round half away from zero, saturating, NaN -> 0) and for
/// the integer types `i32`, `i64`, `u32` and `u64` (saturating conversion).
pub trait KiRoundArg: Copy {
    /// `KiROUND<Self, int>( v )`.
    fn ki_round_i32(self) -> i32;
    /// `KiROUND<Self, int64_t>( v )` (also `long long`).
    fn ki_round_i64(self) -> i64;
    /// `KiROUND<Self, unsigned>( v )`.
    fn ki_round_u32(self) -> u32;
}

macro_rules! impl_ki_round_arg_float {
    ($($fp:ty),*) => {$(
        impl KiRoundArg for $fp {
            #[inline]
            fn ki_round_i32(self) -> i32 {
                ki_round_float_impl!(self, $fp, i32, "int")
            }

            #[inline]
            fn ki_round_i64(self) -> i64 {
                ki_round_float_impl!(self, $fp, i64, "long")
            }

            #[inline]
            fn ki_round_u32(self) -> u32 {
                ki_round_float_impl!(self, $fp, u32, "unsigned int")
            }
        }
    )*};
}

macro_rules! impl_ki_round_arg_int {
    ($($int:ty),*) => {$(
        impl KiRoundArg for $int {
            #[inline]
            fn ki_round_i32(self) -> i32 {
                ki_round_int_impl!(self, i32, "int")
            }

            #[inline]
            fn ki_round_i64(self) -> i64 {
                ki_round_int_impl!(self, i64, "long")
            }

            #[inline]
            fn ki_round_u32(self) -> u32 {
                ki_round_int_impl!(self, u32, "unsigned int")
            }
        }
    )*};
}

impl_ki_round_arg_float!(f64, f32);
impl_ki_round_arg_int!(i32, i64, u32, u64);

/// Return types of `KiROUND` (the `ret_type` template parameter): `i32`, `i64` and `u32`.
pub trait KiRoundRet: Copy {
    /// `KiROUND<A, Self>( v )`.
    fn ki_round_from<A: KiRoundArg>(v: A) -> Self;
}

impl KiRoundRet for i32 {
    #[inline]
    fn ki_round_from<A: KiRoundArg>(v: A) -> i32 {
        v.ki_round_i32()
    }
}

impl KiRoundRet for i64 {
    #[inline]
    fn ki_round_from<A: KiRoundArg>(v: A) -> i64 {
        v.ki_round_i64()
    }
}

impl KiRoundRet for u32 {
    #[inline]
    fn ki_round_from<A: KiRoundArg>(v: A) -> u32 {
        v.ki_round_u32()
    }
}

/// Port of `KiROUND<fp_type, ret_type>( v )` with an explicit return type, e.g.
/// `ki_round_to::<i64, _>( v )` for `KiROUND<double, int64_t>( v )`.
///
/// Round a numeric value to an integer using "round halfway cases away from zero" and clamp
/// the result to the limits of the return type.  NaN gives 0.
///
/// In C++ Debug builds an assert fires if the value will not fit into the return type (unless
/// `aQuiet` is set); the port implements the release behaviour, so there is no `aQuiet`
/// parameter (see [`kimath_log_overflow`]).
#[inline]
pub fn ki_round_to<R: KiRoundRet, A: KiRoundArg>(v: A) -> R {
    R::ki_round_from(v)
}

/// Port of `KiROUND<fp_type, int64_t>( v )` / `KiROUND<fp_type, long long>( v )`.
#[inline]
pub fn ki_round_i64<A: KiRoundArg>(v: A) -> i64 {
    v.ki_round_i64()
}

/// The overload set of the C++ `KiROUND( x )` call with the default return type:
/// scalars round to `i32`, a `Vector2D` rounds to a `Vector2I` and a `Box2D` to a `Box2I`.
pub trait KiRound {
    /// The result type of the overload.
    type Output;

    /// See [`ki_round`].
    fn ki_round(self) -> Self::Output;
}

macro_rules! impl_ki_round_scalar {
    ($($t:ty),*) => {$(
        impl KiRound for $t {
            type Output = i32;

            #[inline]
            fn ki_round(self) -> i32 {
                self.ki_round_i32()
            }
        }
    )*};
}

impl_ki_round_scalar!(f64, f32, i32, i64, u32, u64);

/// Port of `KiROUND( v )` with the default return type `int`, and of the `KiROUND` overloads
/// for `VECTOR2D` (-> `VECTOR2I`) and `BOX2D` (-> `BOX2I`).
///
/// Round a numeric value to an integer using "round halfway cases away from zero" and clamp
/// the result to the limits of `int`.  NaN gives 0.  Integer arguments are clamped.
///
/// The two-argument form `KiROUND( x, y )` is [`crate::math::vector2::ki_round_xy`].
#[inline]
pub fn ki_round<V: KiRound>(v: V) -> V::Output {
    v.ki_round()
}

/// Numeric types `rescale` is instantiated for.
pub trait Rescale: Copy {
    /// See [`rescale`].
    fn rescale(numerator: Self, value: Self, denominator: Self) -> Self;
}

impl Rescale for i32 {
    /// Port of the explicit specialization `rescale<int>`, taking care of overflow.
    #[inline]
    fn rescale(a_numerator: i32, a_value: i32, a_denominator: i32) -> i32 {
        let numerator: i64 = a_numerator as i64 * a_value as i64;

        // round to nearest
        if (numerator < 0) ^ (a_denominator < 0) {
            ((numerator - (a_denominator / 2) as i64) / a_denominator as i64) as i32
        } else {
            ((numerator + (a_denominator / 2) as i64) / a_denominator as i64) as i32
        }
    }
}

impl Rescale for i64 {
    /// Port of the explicit specialization `rescale<int64_t>` (the `__SIZEOF_INT128__` code
    /// path, which is the one GCC and Clang builds use).
    #[inline]
    fn rescale(a_numerator: i64, a_value: i64, a_denominator: i64) -> i64 {
        let numerator: i128 = a_numerator as i128 * a_value as i128;

        if (numerator < 0) ^ (a_denominator < 0) {
            ((numerator - (a_denominator / 2) as i128) / a_denominator as i128) as i64
        } else {
            ((numerator + (a_denominator / 2) as i128) / a_denominator as i128) as i64
        }
    }
}

impl Rescale for u32 {
    /// The generic template `aNumerator * aValue / aDenominator` in modular unsigned
    /// arithmetic.
    #[inline]
    fn rescale(a_numerator: u32, a_value: u32, a_denominator: u32) -> u32 {
        a_numerator.wrapping_mul(a_value) / a_denominator
    }
}

impl Rescale for f64 {
    /// The generic template `aNumerator * aValue / aDenominator`.
    #[inline]
    fn rescale(a_numerator: f64, a_value: f64, a_denominator: f64) -> f64 {
        a_numerator * a_value / a_denominator
    }
}

impl Rescale for f32 {
    /// The generic template `aNumerator * aValue / aDenominator`.
    #[inline]
    fn rescale(a_numerator: f32, a_value: f32, a_denominator: f32) -> f32 {
        a_numerator * a_value / a_denominator
    }
}

/// Port of `rescale<T>( aNumerator, aValue, aDenominator )`.
///
/// Scale a number (value) by rational (numerator/denominator). Numerator must be <=
/// denominator.
///
/// The integer versions round to nearest (halves away from zero) and compute the product in
/// twice the width (`i64` for `i32`, `i128` for `i64`).  As in C++, a zero denominator is a
/// caller error (integer division by zero).
#[inline]
pub fn rescale<T: Rescale>(a_numerator: T, a_value: T, a_denominator: T) -> T {
    T::rescale(a_numerator, a_value, a_denominator)
}

/// Port of `sign( val )`: `( T( 0 ) < val ) - ( val < T( 0 ) )`.
#[inline]
pub fn sign<T: PartialOrd + Default>(val: T) -> i32 {
    let zero = T::default();

    (zero < val) as i32 - (val < zero) as i32
}

macro_rules! impl_ct_sqrt {
    ($helper:ident, $name:ident, $t:ty) => {
        /// Port of `ct_sqrt_helper<T>`.
        pub const fn $helper(a_x: $t, a_lo: $t, a_hi: $t) -> $t {
            if a_lo == a_hi {
                return a_lo;
            }

            let mid: $t = (a_lo + a_hi + 1) / 2;

            if a_x / mid < mid {
                return $helper(a_x, a_lo, mid - 1);
            }

            $helper(a_x, mid, a_hi)
        }

        /// Port of `ct_sqrt<T>`: floor of the square root of an integer, evaluated at
        /// compile time.
        pub const fn $name(a_x: $t) -> $t {
            $helper(a_x, 0, a_x / 2 + 1)
        }
    };
}

impl_ct_sqrt!(ct_sqrt_helper_u64, ct_sqrt_u64, u64);
impl_ct_sqrt!(ct_sqrt_helper_i64, ct_sqrt_i64, i64);
impl_ct_sqrt!(ct_sqrt_helper_u32, ct_sqrt_u32, u32);
impl_ct_sqrt!(ct_sqrt_helper_i32, ct_sqrt_i32, i32);

/// Integer types `isqrt` is instantiated for.
pub trait Isqrt: Copy {
    /// See [`isqrt`].
    fn isqrt(self) -> Self;
}

macro_rules! impl_isqrt {
    ($t:ty, $ct_sqrt:ident, signed) => {
        impl Isqrt for $t {
            fn isqrt(self) -> $t {
                const SQRT_MAX: $t = $ct_sqrt(<$t>::MAX);

                if self < 0 {
                    return SQRT_MAX;
                }

                impl_isqrt!(@body self, $t, SQRT_MAX)
            }
        }
    };
    ($t:ty, $ct_sqrt:ident, unsigned) => {
        impl Isqrt for $t {
            fn isqrt(self) -> $t {
                const SQRT_MAX: $t = $ct_sqrt(<$t>::MAX);

                impl_isqrt!(@body self, $t, SQRT_MAX)
            }
        }
    };
    (@body $x:expr, $t:ty, $sqrt_max:ident) => {{
        let a_x: $t = $x;
        let mut r: $t = (a_x as f64).sqrt() as $t;

        // The double conversion loses precision above 2^53
        while r < $sqrt_max && r * r < a_x {
            r += 1;
        }

        while r > $sqrt_max || r * r > a_x {
            r -= 1;
        }

        r
    }};
}

impl_isqrt!(i64, ct_sqrt_i64, signed);
impl_isqrt!(u64, ct_sqrt_u64, unsigned);
impl_isqrt!(i32, ct_sqrt_i32, signed);
impl_isqrt!(u32, ct_sqrt_u32, unsigned);

/// Port of `isqrt<T>`.
///
/// Exact floor of the square root of an integer.  Negative input returns the largest root
/// representable in T.
#[inline]
pub fn isqrt<T: Isqrt>(a_x: T) -> T {
    a_x.isqrt()
}

/// Port of `UnsignedAbs`: magnitude of a signed integer, well defined for INT64_MIN.
#[inline]
pub const fn unsigned_abs(a_x: i64) -> u64 {
    if a_x < 0 { 0u64.wrapping_sub(a_x as u64) } else { a_x as u64 }
}

/// Port of `RoundedHypotWide`.
///
/// Nearest integer to sqrt( aX^2 + aY^2 ) for magnitudes of at most 2^63, using 128-bit
/// arithmetic.
pub fn rounded_hypot_wide(a_x: u64, a_y: u64) -> u64 {
    let n: u128 = a_x as u128 * a_x as u128 + a_y as u128 * a_y as u128;
    let mut f: u64 = 0;

    // n <= 2^127 keeps the floor root below 2^64
    let mut bit: i32 = 63;

    while bit >= 0 {
        let candidate: u64 = f | (1u64 << bit);

        if candidate as u128 * candidate as u128 <= n {
            f = candidate;
        }

        bit -= 1;
    }

    // Round up when n - f^2 > f, the same test as the narrow path
    f + (n - f as u128 * f as u128 > f as u128) as u64
}

/// Port of `RoundedHypot`.
///
/// Nearest integer to sqrt( aX^2 + aY^2 ), computed exactly.  Ties cannot occur for integer
/// input.
#[inline]
pub fn rounded_hypot(a_x: i64, a_y: i64) -> u64 {
    // Below this both squares are under 2^63, so their sum fits in uint64_t
    const NARROW_MAX: u64 = ct_sqrt_u64(i64::MAX as u64);

    let ax: u64 = unsigned_abs(a_x);
    let ay: u64 = unsigned_abs(a_y);

    if ax > NARROW_MAX || ay > NARROW_MAX {
        return rounded_hypot_wide(ax, ay);
    }

    let n: u64 = ax * ax + ay * ay;
    let f: u64 = isqrt(n);

    // n > f^2 + f is the same as n >= (f + 1/2)^2 for integer n
    f + (n - f * f > f) as u64
}

/// Floating point types the scalar `equals` template is instantiated for.
pub trait KiFloat: Copy + PartialOrd + core::ops::Sub<Output = Self> + core::ops::Mul<Output = Self> {
    /// `std::numeric_limits<T>::epsilon()`.
    const EPSILON: Self;

    /// `std::abs`.
    fn abs(self) -> Self;
}

impl KiFloat for f64 {
    const EPSILON: f64 = f64::EPSILON;

    #[inline]
    fn abs(self) -> f64 {
        f64::abs(self)
    }
}

impl KiFloat for f32 {
    const EPSILON: f32 = f32::EPSILON;

    #[inline]
    fn abs(self) -> f32 {
        f32::abs(self)
    }
}

/// Port of `equals( aFirst, aSecond, aEpsilon )` for floating point values.
///
/// Compare two floating point values for equality within a required epsilon.
///
/// * `a_first` - value to compare.
/// * `a_second` - value to compare.
/// * `a_epsilon` - allowed error.
///
/// Returns true if the values considered equal within the specified epsilon, otherwise false.
#[inline]
pub fn equals_eps<T: KiFloat>(a_first: T, a_second: T, a_epsilon: T) -> bool {
    let diff: T = (a_first - a_second).abs();

    if diff < a_epsilon {
        return true;
    }

    let a_first = a_first.abs();
    let a_second = a_second.abs();
    let largest: T = if a_first > a_second { a_first } else { a_second };

    if diff <= largest * a_epsilon {
        return true;
    }

    false
}

/// Port of `equals( aFirst, aSecond )` with the default epsilon
/// `std::numeric_limits<T>::epsilon()`.
#[inline]
pub fn equals<T: KiFloat>(a_first: T, a_second: T) -> bool {
    equals_eps(a_first, a_second, T::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_sqrt_values() {
        assert_eq!(ct_sqrt_u64(i64::MAX as u64), 3037000499);
        assert_eq!(ct_sqrt_i64(i64::MAX), 3037000499);
        assert_eq!(ct_sqrt_u64(u64::MAX), 4294967295);
        assert_eq!(ct_sqrt_i32(i32::MAX), 46340);
        assert_eq!(ct_sqrt_u32(u32::MAX), 65535);
        assert_eq!(ct_sqrt_u64(0), 0);
        assert_eq!(ct_sqrt_u64(1), 1);
        assert_eq!(ct_sqrt_u64(15), 3);
        assert_eq!(ct_sqrt_u64(16), 4);
    }

    #[test]
    fn isqrt_is_exact_floor() {
        assert_eq!(isqrt(-5i64), 3037000499);
        assert_eq!(isqrt(-5i32), 46340);
        assert_eq!(isqrt(0i64), 0);
        assert_eq!(isqrt(1i64), 1);
        assert_eq!(isqrt(99i64), 9);
        assert_eq!(isqrt(100i64), 10);
        assert_eq!(isqrt(i64::MAX), 3037000499);
        assert_eq!(isqrt(u64::MAX), 4294967295);
        assert_eq!(isqrt(u64::MAX - 1), 4294967295);
        assert_eq!(isqrt(4294967295u64 * 4294967295u64), 4294967295);
        assert_eq!(isqrt(4294967295u64 * 4294967295u64 - 1), 4294967294);

        // Just above and below perfect squares that are not exactly representable as doubles
        for r in [94906265u64, 94906266, 94906267, 3037000498, 3037000499, 4000000000] {
            assert_eq!(isqrt(r * r), r);
            assert_eq!(isqrt(r * r - 1), r - 1);
            assert_eq!(isqrt(r * r + 1), r);
        }
    }

    #[test]
    fn unsigned_abs_values() {
        assert_eq!(unsigned_abs(0), 0);
        assert_eq!(unsigned_abs(-1), 1);
        assert_eq!(unsigned_abs(i64::MAX), i64::MAX as u64);
        assert_eq!(unsigned_abs(i64::MIN), 1u64 << 63);
    }

    #[test]
    fn rounded_hypot_values() {
        assert_eq!(rounded_hypot(3, 4), 5);
        assert_eq!(rounded_hypot(-3, -4), 5);
        assert_eq!(rounded_hypot(1, 1), 1);
        assert_eq!(rounded_hypot(2, 2), 3);
        assert_eq!(rounded_hypot(i64::MIN, 0), 1u64 << 63);
        assert_eq!(rounded_hypot(i64::MIN, i64::MIN), 13043817825332782212);
        assert_eq!(rounded_hypot_wide(3, 4), 5);
        assert_eq!(rounded_hypot_wide(2, 2), 3);
    }

    #[test]
    fn rescale_int_rounds_to_nearest() {
        assert_eq!(rescale(1i32, 9, 2), 5);
        assert_eq!(rescale(1i32, -9, 2), -5);
        assert_eq!(rescale(1i32, 9, -2), -5);
        assert_eq!(rescale(1i32, 17, 4), 4);
        assert_eq!(rescale(1i32, 19, 4), 5);
        assert_eq!(rescale(i32::MAX, i32::MAX, i32::MAX), i32::MAX);
        assert_eq!(rescale(2.0f64, 3.0, 4.0), 1.5);
        assert_eq!(rescale(7u32, 3, 2), 10);
    }

    #[test]
    fn sign_values() {
        assert_eq!(sign(5), 1);
        assert_eq!(sign(-5), -1);
        assert_eq!(sign(0), 0);
        assert_eq!(sign(-0.0f64), 0);
        assert_eq!(sign(0.1f64), 1);
        assert_eq!(sign(f64::NAN), 0);
        assert_eq!(sign(-7i64), -1);
    }

    #[test]
    fn std_clamp_min_max_follow_cpp() {
        assert_eq!(std_clamp(5, 0, 3), 3);
        assert_eq!(std_clamp(-5, 0, 3), 0);
        assert_eq!(std_clamp(2, 0, 3), 2);
        assert!(std_clamp(f64::NAN, 0.0, 3.0).is_nan());
        assert_eq!(std_min(1.0, f64::NAN), 1.0);
        assert!(std_min(f64::NAN, 1.0).is_nan());
        assert_eq!(std_max(1.0, f64::NAN), 1.0);
        assert!(std_max(f64::NAN, 1.0).is_nan());
    }

    #[test]
    fn ki_checked_cast_clamps() {
        assert_eq!(ki_checked_cast::<i64, i32>(3_000_000_000), i32::MAX);
        assert_eq!(ki_checked_cast::<i64, i32>(-3_000_000_000), i32::MIN);
        assert_eq!(ki_checked_cast::<i64, i32>(-42), -42);
        assert_eq!(ki_checked_cast::<i64, i64>(3_000_000_000), 3_000_000_000);
        assert_eq!(ki_checked_cast::<f64, f64>(1.5), 1.5);
    }

    #[test]
    fn equals_values() {
        assert!(equals(1.0f64, 1.0));
        assert!(equals(0.1f64 + 0.2, 0.3));
        assert!(!equals(1.0f64, 1.0001));
        assert!(equals_eps(1.0f64, 1.0001, 0.001));
        assert!(equals(1.0e20f64, 1.0e20 + 1.0));
        assert!(equals(1.0f32, 1.0 + f32::EPSILON / 2.0));
        assert!(!equals(f64::NAN, f64::NAN));
    }
}
