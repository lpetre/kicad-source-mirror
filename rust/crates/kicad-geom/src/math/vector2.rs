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
// Copyright (C) 2010 Virtenio GmbH, Torsten Hueter, torsten.hueter <at> virtenio.de
// Copyright (C) 2012 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
// Copyright (C) 2013 CERN
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>

//! Port of `libs/kimath/include/math/vector2d.h`.
//!
//! `VECTOR2<T>` is a class template; the port is the generic [`Vector2<T>`] over the
//! [`Coord`] trait, which is implemented for the coordinate types the C++ instantiates:
//! `i32` (`VECTOR2I`), `f64` (`VECTOR2D`), `i64` (`VECTOR2L`) and, for the unit tests,
//! `f32` and `u32`.  The trait carries the per-type behaviour that the header selects with
//! `if constexpr` (integral vs. floating point code paths, `VECTOR2_TRAITS<T>::extended_type`).
//!
//! Arithmetic follows the C++ exactly:
//!
//! * `i32`/`i64` arithmetic uses the plain operators.  Signed overflow is undefined behaviour
//!   in C++; in the port it panics in builds with overflow checks (dev/test) and wraps
//!   otherwise.
//! * `u32` arithmetic is modular, as unsigned arithmetic is in C++.
//! * The implicit conversions of the C++ operators are spelled out per operand type pair,
//!   see the operator table at the end of this file.

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};
use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

use crate::cmath;
use crate::math::util::{
    KiRound, ct_sqrt_u64, equals_eps, ki_round, rescale, rounded_hypot, sign, std_clamp,
    unsigned_abs,
};
use crate::ostream::format_double;

/// A coordinate value tagged with its type, used to implement the conversions between
/// `VECTOR2` specializations (`static_cast` between arbitrary coordinate types).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    /// `int`
    I32(i32),
    /// `int64_t`
    I64(i64),
    /// `unsigned`
    U32(u32),
    /// `float`
    F32(f32),
    /// `double`
    F64(f64),
}

/// A coordinate type `VECTOR2<T>` can be instantiated with.
///
/// This plays the role of `VECTOR2_TRAITS<T>` plus the `std::is_integral` /
/// `std::is_floating_point` dispatch in the header.  The `coord_*` functions are the
/// primitive operations of the type "as C++ evaluates them"; use the methods and operators
/// of [`Vector2`] rather than calling them directly.
pub trait Coord: Copy + Default + PartialEq + PartialOrd + fmt::Debug + 'static {
    /// `VECTOR2_TRAITS<T>::extended_type`: extended range/precision type used by operations
    /// involving multiple multiplications to prevent overflow (`int64_t` for `int`, `T` for
    /// everything else).
    type Extended: Coord;

    /// `std::is_integral<T>::value`.
    const IS_INTEGRAL: bool;

    /// `T( 0 )`.
    const ZERO: Self;

    /// `std::numeric_limits<T>::min()` (the smallest positive normal value for floating
    /// point types, as in C++).
    const NUMERIC_MIN: Self;

    /// `std::numeric_limits<T>::max()`.
    const NUMERIC_MAX: Self;

    /// `std::numeric_limits<T>::lowest()`.
    const NUMERIC_LOWEST: Self;

    /// Tag the value with its type.
    fn to_num(self) -> Num;

    /// Conversion of one coordinate as done by the converting constructor
    /// `VECTOR2<T>::VECTOR2( const VECTOR2<CastingType>& )`, see [`Vector2::cast`].
    fn cast_from_num(n: Num) -> Self;

    /// `(extended_type) x`.
    fn to_extended(self) -> Self::Extended;

    /// `a + b` evaluated in `T`.
    fn coord_add(self, rhs: Self) -> Self;

    /// `a - b` evaluated in `T`.
    fn coord_sub(self, rhs: Self) -> Self;

    /// `a * b` evaluated in `T`.
    fn coord_mul(self, rhs: Self) -> Self;

    /// `-a` evaluated in `T`.
    fn coord_neg(self) -> Self;

    /// One coordinate of `VECTOR2<T>::operator/( double aFactor )`.
    fn coord_div_f64(self, a_factor: f64) -> Self;

    /// `VECTOR2<T>::SquaredEuclideanNorm()` of the vector `( x, y )`.
    fn coord_squared_euclidean_norm(x: Self, y: Self) -> Self::Extended;

    /// The value as `std::ostream << T` prints it.
    fn coord_format(self) -> String;
}

/// A coordinate type with a sign, for which the C++ can instantiate the members that use
/// `std::abs` (`EuclideanNorm()` and `Resize()`): all [`Coord`] types except `u32`.
pub trait SignedCoord: Coord {
    /// `VECTOR2<T>::EuclideanNorm()` of the vector `( x, y )`.
    fn coord_euclidean_norm(x: Self, y: Self) -> Self;

    /// `VECTOR2<T>::Resize( aNewLength )` of the vector `( x, y )`.
    fn coord_resize(x: Self, y: Self, a_new_length: Self) -> (Self, Self);
}

/// The integral-target branches of the `VECTOR2` converting constructor.
macro_rules! impl_cast_from_num_integral {
    ($t:ty) => {
        #[inline]
        #[allow(clippy::unnecessary_cast)]
        fn cast_from_num(n: Num) -> $t {
            match n {
                // else if constexpr( std::is_floating_point<CastingType>() )
                Num::F64(v) => std_clamp(v, <$t>::MIN as f64, <$t>::MAX as f64) as $t,
                Num::F32(v) => std_clamp(v, <$t>::MIN as f32, <$t>::MAX as f32) as $t,
                // else if constexpr( std::is_integral<T>() && std::is_integral<CastingType>() )
                Num::I32(v) => std_clamp(v as i64, <$t>::MIN as i64, <$t>::MAX as i64) as $t,
                Num::I64(v) => std_clamp(v, <$t>::MIN as i64, <$t>::MAX as i64) as $t,
                Num::U32(v) => std_clamp(v as i64, <$t>::MIN as i64, <$t>::MAX as i64) as $t,
            }
        }
    };
}

/// The floating-point-target branch of the `VECTOR2` converting constructor: a plain
/// `static_cast`.
macro_rules! impl_cast_from_num_floating {
    ($t:ty) => {
        #[inline]
        #[allow(clippy::unnecessary_cast)]
        fn cast_from_num(n: Num) -> $t {
            match n {
                Num::F64(v) => v as $t,
                Num::F32(v) => v as $t,
                Num::I32(v) => v as $t,
                Num::I64(v) => v as $t,
                Num::U32(v) => v as $t,
            }
        }
    };
}

/// `SquaredEuclideanNorm()` for `int` and `int64_t`: saturates at the extended type maximum.
macro_rules! impl_squared_norm_saturating {
    ($t:ty) => {
        #[inline]
        #[allow(clippy::unnecessary_cast)]
        fn coord_squared_euclidean_norm(x: $t, y: $t) -> i64 {
            // Past this a single square exceeds ECOORD_MAX; below it the sum still fits in
            // uint64_t
            const NARROW_MAX: u64 = ct_sqrt_u64(i64::MAX as u64);

            let ax: u64 = unsigned_abs(x as i64);
            let ay: u64 = unsigned_abs(y as i64);

            if ax > NARROW_MAX || ay > NARROW_MAX {
                return i64::MAX;
            }

            (ax * ax + ay * ay).min(i64::MAX as u64) as i64
        }
    };
}

/// `EuclideanNorm()` and `Resize()` for the signed integral types.
macro_rules! impl_signed_coord_integral {
    ($t:ty) => {
        impl SignedCoord for $t {
            /// For signed integer types the result is exactly rounded to nearest and
            /// saturates at the type maximum.
            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn coord_euclidean_norm(x: $t, y: $t) -> $t {
                rounded_hypot(x as i64, y as i64).min(<$t>::MAX as u64) as $t
            }

            #[allow(clippy::unnecessary_cast)]
            fn coord_resize(x: $t, y: $t, a_new_length: $t) -> ($t, $t) {
                type Ext = <$t as Coord>::Extended;

                if x == 0 && y == 0 {
                    return (0, 0);
                }

                let new_x: f64;
                let new_y: f64;

                if x.abs() == y.abs() {
                    new_x = a_new_length.abs() as f64 * FRAC_1_SQRT_2;
                    new_y = new_x;
                } else {
                    let x_sq: Ext = x as Ext * x as Ext;
                    let y_sq: Ext = y as Ext * y as Ext;
                    let l_sq: Ext = x_sq + y_sq;
                    let new_length_sq: Ext = a_new_length as Ext * a_new_length as Ext;
                    new_x = (rescale(new_length_sq, x_sq, l_sq) as f64).sqrt();
                    new_y = (rescale(new_length_sq, y_sq, l_sq) as f64).sqrt();
                }

                // KiROUND returns int here, whatever T is
                let rx: $t = (if x < 0 {
                    -ki_round(new_x)
                } else {
                    ki_round(new_x)
                }) as $t;
                let ry: $t = (if y < 0 {
                    -ki_round(new_y)
                } else {
                    ki_round(new_y)
                }) as $t;
                let s: $t = sign(a_new_length) as $t;

                (rx * s, ry * s)
            }
        }
    };
}

impl Coord for i32 {
    type Extended = i64;

    const IS_INTEGRAL: bool = true;
    const ZERO: i32 = 0;
    const NUMERIC_MIN: i32 = i32::MIN;
    const NUMERIC_MAX: i32 = i32::MAX;
    const NUMERIC_LOWEST: i32 = i32::MIN;

    #[inline]
    fn to_num(self) -> Num {
        Num::I32(self)
    }

    impl_cast_from_num_integral!(i32);

    #[inline]
    fn to_extended(self) -> i64 {
        self as i64
    }

    #[inline]
    fn coord_add(self, rhs: i32) -> i32 {
        self + rhs
    }

    #[inline]
    fn coord_sub(self, rhs: i32) -> i32 {
        self - rhs
    }

    #[inline]
    fn coord_mul(self, rhs: i32) -> i32 {
        self * rhs
    }

    #[inline]
    fn coord_neg(self) -> i32 {
        -self
    }

    #[inline]
    fn coord_div_f64(self, a_factor: f64) -> i32 {
        ki_round(self as f64 / a_factor)
    }

    impl_squared_norm_saturating!(i32);

    fn coord_format(self) -> String {
        self.to_string()
    }
}

impl Coord for i64 {
    type Extended = i64;

    const IS_INTEGRAL: bool = true;
    const ZERO: i64 = 0;
    const NUMERIC_MIN: i64 = i64::MIN;
    const NUMERIC_MAX: i64 = i64::MAX;
    const NUMERIC_LOWEST: i64 = i64::MIN;

    #[inline]
    fn to_num(self) -> Num {
        Num::I64(self)
    }

    impl_cast_from_num_integral!(i64);

    #[inline]
    fn to_extended(self) -> i64 {
        self
    }

    #[inline]
    fn coord_add(self, rhs: i64) -> i64 {
        self + rhs
    }

    #[inline]
    fn coord_sub(self, rhs: i64) -> i64 {
        self - rhs
    }

    #[inline]
    fn coord_mul(self, rhs: i64) -> i64 {
        self * rhs
    }

    #[inline]
    fn coord_neg(self) -> i64 {
        -self
    }

    /// `KiROUND( x / aFactor )` returns `int`, so the quotient is clamped to the `int`
    /// range before it is widened back to `int64_t`, exactly as in C++.
    #[inline]
    fn coord_div_f64(self, a_factor: f64) -> i64 {
        ki_round(self as f64 / a_factor) as i64
    }

    impl_squared_norm_saturating!(i64);

    fn coord_format(self) -> String {
        self.to_string()
    }
}

impl Coord for u32 {
    type Extended = u32;

    const IS_INTEGRAL: bool = true;
    const ZERO: u32 = 0;
    const NUMERIC_MIN: u32 = u32::MIN;
    const NUMERIC_MAX: u32 = u32::MAX;
    const NUMERIC_LOWEST: u32 = u32::MIN;

    #[inline]
    fn to_num(self) -> Num {
        Num::U32(self)
    }

    impl_cast_from_num_integral!(u32);

    #[inline]
    fn to_extended(self) -> u32 {
        self
    }

    #[inline]
    fn coord_add(self, rhs: u32) -> u32 {
        self.wrapping_add(rhs)
    }

    #[inline]
    fn coord_sub(self, rhs: u32) -> u32 {
        self.wrapping_sub(rhs)
    }

    #[inline]
    fn coord_mul(self, rhs: u32) -> u32 {
        self.wrapping_mul(rhs)
    }

    #[inline]
    fn coord_neg(self) -> u32 {
        self.wrapping_neg()
    }

    /// `KiROUND( x / aFactor )` returns `int`, which is then converted to `unsigned`.
    #[inline]
    fn coord_div_f64(self, a_factor: f64) -> u32 {
        ki_round(self as f64 / a_factor) as u32
    }

    #[inline]
    fn coord_squared_euclidean_norm(x: u32, y: u32) -> u32 {
        x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y))
    }

    fn coord_format(self) -> String {
        self.to_string()
    }
}

impl Coord for f64 {
    type Extended = f64;

    const IS_INTEGRAL: bool = false;
    const ZERO: f64 = 0.0;
    const NUMERIC_MIN: f64 = f64::MIN_POSITIVE;
    const NUMERIC_MAX: f64 = f64::MAX;
    const NUMERIC_LOWEST: f64 = f64::MIN;

    #[inline]
    fn to_num(self) -> Num {
        Num::F64(self)
    }

    impl_cast_from_num_floating!(f64);

    #[inline]
    fn to_extended(self) -> f64 {
        self
    }

    #[inline]
    fn coord_add(self, rhs: f64) -> f64 {
        self + rhs
    }

    #[inline]
    fn coord_sub(self, rhs: f64) -> f64 {
        self - rhs
    }

    #[inline]
    fn coord_mul(self, rhs: f64) -> f64 {
        self * rhs
    }

    #[inline]
    fn coord_neg(self) -> f64 {
        -self
    }

    #[inline]
    fn coord_div_f64(self, a_factor: f64) -> f64 {
        self / a_factor
    }

    #[inline]
    fn coord_squared_euclidean_norm(x: f64, y: f64) -> f64 {
        x * x + y * y
    }

    fn coord_format(self) -> String {
        format_double(self)
    }
}

impl Coord for f32 {
    type Extended = f32;

    const IS_INTEGRAL: bool = false;
    const ZERO: f32 = 0.0;
    const NUMERIC_MIN: f32 = f32::MIN_POSITIVE;
    const NUMERIC_MAX: f32 = f32::MAX;
    const NUMERIC_LOWEST: f32 = f32::MIN;

    #[inline]
    fn to_num(self) -> Num {
        Num::F32(self)
    }

    impl_cast_from_num_floating!(f32);

    #[inline]
    fn to_extended(self) -> f32 {
        self
    }

    #[inline]
    fn coord_add(self, rhs: f32) -> f32 {
        self + rhs
    }

    #[inline]
    fn coord_sub(self, rhs: f32) -> f32 {
        self - rhs
    }

    #[inline]
    fn coord_mul(self, rhs: f32) -> f32 {
        self * rhs
    }

    #[inline]
    fn coord_neg(self) -> f32 {
        -self
    }

    /// `static_cast<float>( x / aFactor )`, the division being done in `double`.
    #[inline]
    fn coord_div_f64(self, a_factor: f64) -> f32 {
        (self as f64 / a_factor) as f32
    }

    #[inline]
    fn coord_squared_euclidean_norm(x: f32, y: f32) -> f32 {
        x * x + y * y
    }

    fn coord_format(self) -> String {
        format_double(self as f64)
    }
}

impl_signed_coord_integral!(i32);
impl_signed_coord_integral!(i64);

impl SignedCoord for f64 {
    #[inline]
    fn coord_euclidean_norm(x: f64, y: f64) -> f64 {
        // 45° are common in KiCad, so we can optimize the calculation
        if x.abs() == y.abs() {
            return x.abs() * SQRT_2;
        }

        if x == 0.0 {
            return y.abs();
        }

        if y == 0.0 {
            return x.abs();
        }

        cmath::hypot(x, y)
    }

    fn coord_resize(x: f64, y: f64, a_new_length: f64) -> (f64, f64) {
        if x == 0.0 && y == 0.0 {
            return (0.0, 0.0);
        }

        let new_x: f64;
        let new_y: f64;

        if x.abs() == y.abs() {
            new_x = a_new_length.abs() * FRAC_1_SQRT_2;
            new_y = new_x;
        } else {
            let x_sq: f64 = x * x;
            let y_sq: f64 = y * y;
            let l_sq: f64 = x_sq + y_sq;
            let new_length_sq: f64 = a_new_length * a_new_length;
            new_x = rescale(new_length_sq, x_sq, l_sq).sqrt();
            new_y = rescale(new_length_sq, y_sq, l_sq).sqrt();
        }

        let rx: f64 = if x < 0.0 { -new_x } else { new_x };
        let ry: f64 = if y < 0.0 { -new_y } else { new_y };
        let s: f64 = sign(a_new_length) as f64;

        (rx * s, ry * s)
    }
}

impl SignedCoord for f32 {
    #[inline]
    fn coord_euclidean_norm(x: f32, y: f32) -> f32 {
        // 45° are common in KiCad, so we can optimize the calculation
        if x.abs() == y.abs() {
            return (x.abs() as f64 * SQRT_2) as f32;
        }

        if x == 0.0 {
            return y.abs();
        }

        if y == 0.0 {
            return x.abs();
        }

        cmath::hypotf(x, y)
    }

    fn coord_resize(x: f32, y: f32, a_new_length: f32) -> (f32, f32) {
        if x == 0.0 && y == 0.0 {
            return (0.0, 0.0);
        }

        let new_x: f64;
        let new_y: f64;

        if x.abs() == y.abs() {
            new_x = a_new_length.abs() as f64 * FRAC_1_SQRT_2;
            new_y = new_x;
        } else {
            let x_sq: f32 = x * x;
            let y_sq: f32 = y * y;
            let l_sq: f32 = x_sq + y_sq;
            let new_length_sq: f32 = a_new_length * a_new_length;
            // std::sqrt( float ) is evaluated in single precision
            new_x = rescale(new_length_sq, x_sq, l_sq).sqrt() as f64;
            new_y = rescale(new_length_sq, y_sq, l_sq).sqrt() as f64;
        }

        let rx: f32 = (if x < 0.0 { -new_x } else { new_x }) as f32;
        let ry: f32 = (if y < 0.0 { -new_y } else { new_y }) as f32;
        let s: f32 = sign(a_new_length) as f32;

        (rx * s, ry * s)
    }
}

/// Port of `VECTOR2<T>`: define a general 2D-vector/point.
///
/// Several operators are provided to help easy implementing of linear algebra equations.
///
/// The derived ordering (`PartialOrd`, and `Ord` for the integer types) compares `x` first
/// and then `y`, which is exactly `VECTOR2<T>::operator<` and friends, i.e. the same order as
/// [`lexicographical_compare`].
///
/// `Display` prints the vector like `VECTOR2<T>::Format()`; see also
/// [`Vector2::to_stream_string`].
///
/// Note on hashing: the C++ header deliberately deletes `std::hash<VECTOR2I>` ("DO NOT USE
/// hash tables with VECTOR2 elements ... Use the std::map/std::set trees instead").  `Hash`
/// is derived here because it is harmless in Rust, but ported algorithms should keep using
/// ordered containers (`BTreeMap`/`BTreeSet`) wherever the C++ does, to preserve iteration
/// order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Vector2<T> {
    /// The x coordinate.
    pub x: T,
    /// The y coordinate.
    pub y: T,
}

/// Port of `VECTOR2I` (`VECTOR2<int32_t>`).
pub type Vector2I = Vector2<i32>;

/// Port of `VECTOR2D` (`VECTOR2<double>`).
pub type Vector2D = Vector2<f64>;

/// Port of `VECTOR2L` (`VECTOR2<int64_t>`).
pub type Vector2L = Vector2<i64>;

impl<T> Vector2<T> {
    /// Port of `VECTOR2<T>::VECTOR2( T x, T y )`: construct a vector with given components
    /// x, y.  (The default constructor, x = y = 0, is `Vector2::default()`.)
    #[inline]
    pub const fn new(x: T, y: T) -> Self {
        Vector2 { x, y }
    }
}

impl<T: Coord> Vector2<T> {
    /// Port of `VECTOR2<T>::ECOORD_MAX`.
    pub const ECOORD_MAX: T::Extended = <T::Extended as Coord>::NUMERIC_MAX;

    /// Port of `VECTOR2<T>::ECOORD_MIN` (`std::numeric_limits<extended_type>::min()`).
    pub const ECOORD_MIN: T::Extended = <T::Extended as Coord>::NUMERIC_MIN;

    /// Port of the converting constructor `VECTOR2<T>::VECTOR2( const VECTOR2<CastingType>& )`
    /// and of the cast operator `VECTOR2<T>::operator()<U>()`: cast a vector to another
    /// specialization.  Beware of rounding issues.
    ///
    /// * To a floating point type: `static_cast`.
    /// * From a floating point type to an integral type: the value is clamped to the range of
    ///   the target type and then truncated towards zero (it is *not* rounded; use
    ///   [`ki_round`] for that).
    /// * Between integral types: the value is clamped to the range of the target type.
    ///
    /// Where the C++ result is undefined (NaN, or a clamp limit that is not representable in
    /// the target type such as `2^63`), the port follows Rust's `as` conversion: NaN becomes 0
    /// and out-of-range values saturate.
    #[inline]
    pub fn cast<U: Coord>(self) -> Vector2<U> {
        Vector2::new(
            U::cast_from_num(self.x.to_num()),
            U::cast_from_num(self.y.to_num()),
        )
    }

    /// Same conversion as [`Vector2::cast`], spelled like the C++ converting constructor
    /// (`VECTOR2I( aVecD )` becomes `Vector2I::from_vec( a_vec_d )`).
    #[inline]
    pub fn from_vec<U: Coord>(a_vec: Vector2<U>) -> Self {
        a_vec.cast()
    }

    /// Port of `VECTOR2<T>::SquaredEuclideanNorm()`.
    ///
    /// Compute the squared euclidean norm of the vector, which is defined as
    /// (x ** 2 + y ** 2).
    ///
    /// It is used to calculate the length of the vector.  For int and int64_t it saturates at
    /// the extended type maximum.
    #[inline]
    pub fn squared_euclidean_norm(self) -> T::Extended {
        T::coord_squared_euclidean_norm(self.x, self.y)
    }

    /// Port of `VECTOR2<T>::Perpendicular()`: compute the perpendicular vector `( -y, x )`.
    #[inline]
    pub fn perpendicular(self) -> Self {
        Vector2::new(self.y.coord_neg(), self.x)
    }

    /// Port of `VECTOR2<T>::Format()`: return the vector formatted as a string,
    /// `( xy <x> <y> )`.
    pub fn format(self) -> String {
        format!("( xy {} {} )", self.x.coord_format(), self.y.coord_format())
    }

    /// Port of `operator<<( std::ostream&, const VECTOR2<T>& )`: `[ <x> | <y> ]`.
    pub fn to_stream_string(self) -> String {
        format!("[ {} | {} ]", self.x.coord_format(), self.y.coord_format())
    }

    /// Port of `VECTOR2<T>::Cross()`: compute cross product of self with `a_vector`.
    #[inline]
    pub fn cross(self, a_vector: Self) -> T::Extended {
        self.x
            .to_extended()
            .coord_mul(a_vector.y.to_extended())
            .coord_sub(self.y.to_extended().coord_mul(a_vector.x.to_extended()))
    }

    /// Port of `VECTOR2<T>::Dot()`: compute dot product of self with `a_vector`.
    #[inline]
    pub fn dot(self, a_vector: Self) -> T::Extended {
        self.x
            .to_extended()
            .coord_mul(a_vector.x.to_extended())
            .coord_add(self.y.to_extended().coord_mul(a_vector.y.to_extended()))
    }

    /// Port of `VECTOR2<T>::Distance()`.
    ///
    /// Compute the distance between two vectors.  This is a double precision value because
    /// the distance is frequently non-integer.
    ///
    /// The C++ parameter type is `VECTOR2<extended_type>`; a `Vector2I` argument converts
    /// implicitly there and via `Into` here.
    #[inline]
    pub fn distance(self, a_vector: impl Into<Vector2<T::Extended>>) -> f64 {
        let a_vector: Vector2<T::Extended> = a_vector.into();
        let dx: T::Extended = a_vector.x.coord_sub(self.x.to_extended());
        let dy: T::Extended = a_vector.y.coord_sub(self.y.to_extended());
        let diff: Vector2<f64> = Vector2::new(
            f64::cast_from_num(dx.to_num()),
            f64::cast_from_num(dy.to_num()),
        );

        diff.euclidean_norm()
    }

    /// Port of `VECTOR2<T>::SquaredDistance()`: compute the squared distance between two
    /// vectors.
    #[inline]
    pub fn squared_distance(self, a_vector: Self) -> T::Extended {
        let dx: T::Extended = self.x.to_extended().coord_sub(a_vector.x.to_extended());
        let dy: T::Extended = self.y.to_extended().coord_sub(a_vector.y.to_extended());

        dx.coord_mul(dx).coord_add(dy.coord_mul(dy))
    }
}

impl<T: SignedCoord> Vector2<T> {
    /// Port of `VECTOR2<T>::EuclideanNorm()`.
    ///
    /// Compute the Euclidean norm of the vector, which is defined as sqrt(x ** 2 + y ** 2).
    ///
    /// It is used to calculate the length of the vector.
    ///
    /// For signed integer types the result is exactly rounded to nearest and saturates at the
    /// type maximum.
    #[inline]
    pub fn euclidean_norm(self) -> T {
        T::coord_euclidean_norm(self.x, self.y)
    }

    /// Port of `VECTOR2<T>::Resize()`.
    ///
    /// Return a vector of the same direction, but length specified in `a_new_length`.
    ///
    /// Note that for `Vector2L` the components of the result are limited to the `int` range,
    /// because the C++ rounds them with `KiROUND` (which returns `int`).
    #[inline]
    pub fn resize(self, a_new_length: T) -> Self {
        let (x, y) = T::coord_resize(self.x, self.y, a_new_length);

        Vector2::new(x, y)
    }
}

impl<T: Coord> fmt::Display for Vector2<T> {
    /// Same text as [`Vector2::format`] (`VECTOR2<T>::Format()`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "( xy {} {} )",
            self.x.coord_format(),
            self.y.coord_format()
        )
    }
}

impl<T> From<(T, T)> for Vector2<T> {
    /// Brace initialisation `VECTOR2<T>{ x, y }`.
    #[inline]
    fn from((x, y): (T, T)) -> Self {
        Vector2::new(x, y)
    }
}

impl From<Vector2<i32>> for Vector2<i64> {
    /// The (lossless) implicit conversion `VECTOR2I` -> `VECTOR2L`.
    #[inline]
    fn from(v: Vector2<i32>) -> Self {
        Vector2::new(v.x as i64, v.y as i64)
    }
}

impl From<Vector2<i32>> for Vector2<f64> {
    /// The (lossless) implicit conversion `VECTOR2I` -> `VECTOR2D`.
    #[inline]
    fn from(v: Vector2<i32>) -> Self {
        Vector2::new(v.x as f64, v.y as f64)
    }
}

// ---------------------------------------------------------------------------------------
// Operators
//
// C++                                                   Rust
// VECTOR2<T> + VECTOR2<U> -> VECTOR2<common_type>       Add (same type; i32/i64/f64 mixes)
// VECTOR2<T> - VECTOR2<U> -> VECTOR2<common_type>       Sub (same type; i32/i64/f64 mixes)
// VECTOR2<T> * VECTOR2<U> -> extended_type (dot)        Mul (same type; i32/i64/f64 mixes)
// VECTOR2<T> + U, VECTOR2<T> - U -> VECTOR2<T>          Add/Sub with a scalar
// VECTOR2<T> * U -> VECTOR2<common_type>                Mul with a scalar
// T * VECTOR2<U> -> VECTOR2<common_type>                Mul for i32/i64/f64 scalars
// VECTOR2<T> / double -> VECTOR2<T>                     Div<f64> (and Div<i32>, see below)
// -VECTOR2<T>                                           Neg
// +=, -= (vector and scalar), *= (vector and scalar)    AddAssign, SubAssign, MulAssign
// ---------------------------------------------------------------------------------------

impl<T: Coord> Add for Vector2<T> {
    type Output = Vector2<T>;

    #[inline]
    fn add(self, rhs: Vector2<T>) -> Vector2<T> {
        Vector2::new(self.x.coord_add(rhs.x), self.y.coord_add(rhs.y))
    }
}

impl<T: Coord> Sub for Vector2<T> {
    type Output = Vector2<T>;

    #[inline]
    fn sub(self, rhs: Vector2<T>) -> Vector2<T> {
        Vector2::new(self.x.coord_sub(rhs.x), self.y.coord_sub(rhs.y))
    }
}

impl<T: Coord> Mul<Vector2<T>> for Vector2<T> {
    type Output = T::Extended;

    /// Port of `operator*( const VECTOR2<T>&, const VECTOR2<U>& )`: the scalar (dot) product
    /// in the extended type.
    #[inline]
    fn mul(self, rhs: Vector2<T>) -> T::Extended {
        self.dot(rhs)
    }
}

impl<T: Coord> Add<T> for Vector2<T> {
    type Output = Vector2<T>;

    /// Port of `operator+( const VECTOR2<T>&, const U& aScalar )` for `U == T`.
    #[inline]
    fn add(self, a_scalar: T) -> Vector2<T> {
        Vector2::new(self.x.coord_add(a_scalar), self.y.coord_add(a_scalar))
    }
}

impl<T: Coord> Sub<T> for Vector2<T> {
    type Output = Vector2<T>;

    /// Port of `operator-( const VECTOR2<T>&, U aScalar )` for `U == T`.
    #[inline]
    fn sub(self, a_scalar: T) -> Vector2<T> {
        Vector2::new(self.x.coord_sub(a_scalar), self.y.coord_sub(a_scalar))
    }
}

impl<T: Coord> Mul<T> for Vector2<T> {
    type Output = Vector2<T>;

    /// Port of `operator*( const VECTOR2<T>&, const U& aScalar )` for `U == T`.
    #[inline]
    fn mul(self, a_scalar: T) -> Vector2<T> {
        Vector2::new(self.x.coord_mul(a_scalar), self.y.coord_mul(a_scalar))
    }
}

impl<T: Coord> Neg for Vector2<T> {
    type Output = Vector2<T>;

    /// Port of `VECTOR2<T>::operator-()`: negate vector operator.
    #[inline]
    fn neg(self) -> Vector2<T> {
        Vector2::new(self.x.coord_neg(), self.y.coord_neg())
    }
}

impl<T: Coord> Div<f64> for Vector2<T> {
    type Output = Vector2<T>;

    /// Port of `VECTOR2<T>::operator/( double aFactor )`: division with a factor.
    ///
    /// For integral `T` the quotients are rounded with `KiROUND` (and therefore limited to
    /// the `int` range, also for `Vector2L`).
    #[inline]
    fn div(self, a_factor: f64) -> Vector2<T> {
        Vector2::new(
            self.x.coord_div_f64(a_factor),
            self.y.coord_div_f64(a_factor),
        )
    }
}

impl<T: Coord> Div<i32> for Vector2<T> {
    type Output = Vector2<T>;

    /// `VECTOR2<T>::operator/( double aFactor )` called with an `int` argument (as in
    /// `( A + B ) / 2`), which C++ converts to `double` implicitly.
    #[inline]
    fn div(self, a_factor: i32) -> Vector2<T> {
        self / a_factor as f64
    }
}

impl<T: Coord> AddAssign for Vector2<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Vector2<T>) {
        self.x = self.x.coord_add(rhs.x);
        self.y = self.y.coord_add(rhs.y);
    }
}

impl<T: Coord> AddAssign<T> for Vector2<T> {
    #[inline]
    fn add_assign(&mut self, a_scalar: T) {
        self.x = self.x.coord_add(a_scalar);
        self.y = self.y.coord_add(a_scalar);
    }
}

impl<T: Coord> SubAssign for Vector2<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Vector2<T>) {
        self.x = self.x.coord_sub(rhs.x);
        self.y = self.y.coord_sub(rhs.y);
    }
}

impl<T: Coord> SubAssign<T> for Vector2<T> {
    #[inline]
    fn sub_assign(&mut self, a_scalar: T) {
        self.x = self.x.coord_sub(a_scalar);
        self.y = self.y.coord_sub(a_scalar);
    }
}

impl<T: Coord> MulAssign for Vector2<T> {
    /// Port of `VECTOR2<T>::operator*=( const VECTOR2<T>& )`: component-wise product.
    #[inline]
    fn mul_assign(&mut self, rhs: Vector2<T>) {
        self.x = self.x.coord_mul(rhs.x);
        self.y = self.y.coord_mul(rhs.y);
    }
}

impl<T: Coord> MulAssign<T> for Vector2<T> {
    #[inline]
    fn mul_assign(&mut self, a_scalar: T) {
        self.x = self.x.coord_mul(a_scalar);
        self.y = self.y.coord_mul(a_scalar);
    }
}

/// `VECTOR2<T> op VECTOR2<U>` for `T != U`: the operands are converted to
/// `std::common_type_t<T, U>` and the result is a vector (or, for `*`, the dot product in the
/// extended type) of that type.
macro_rules! impl_mixed_vector_ops {
    ($lhs:ty, $rhs:ty => $out:ty) => {
        impl Add<Vector2<$rhs>> for Vector2<$lhs> {
            type Output = Vector2<$out>;

            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn add(self, rhs: Vector2<$rhs>) -> Vector2<$out> {
                Vector2::new(
                    self.x as $out + rhs.x as $out,
                    self.y as $out + rhs.y as $out,
                )
            }
        }

        impl Sub<Vector2<$rhs>> for Vector2<$lhs> {
            type Output = Vector2<$out>;

            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn sub(self, rhs: Vector2<$rhs>) -> Vector2<$out> {
                Vector2::new(
                    self.x as $out - rhs.x as $out,
                    self.y as $out - rhs.y as $out,
                )
            }
        }

        impl Mul<Vector2<$rhs>> for Vector2<$lhs> {
            type Output = $out;

            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn mul(self, rhs: Vector2<$rhs>) -> $out {
                self.x as $out * rhs.x as $out + self.y as $out * rhs.y as $out
            }
        }
    };
}

impl_mixed_vector_ops!(i32, i64 => i64);
impl_mixed_vector_ops!(i64, i32 => i64);
impl_mixed_vector_ops!(i32, f64 => f64);
impl_mixed_vector_ops!(f64, i32 => f64);
impl_mixed_vector_ops!(i64, f64 => f64);
impl_mixed_vector_ops!(f64, i64 => f64);

/// `VECTOR2<T> * U` for `T != U`: a vector of `std::common_type_t<T, U>`.
macro_rules! impl_mixed_scalar_mul {
    ($vec:ty, $scalar:ty => $out:ty) => {
        impl Mul<$scalar> for Vector2<$vec> {
            type Output = Vector2<$out>;

            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn mul(self, a_scalar: $scalar) -> Vector2<$out> {
                Vector2::new(
                    self.x as $out * a_scalar as $out,
                    self.y as $out * a_scalar as $out,
                )
            }
        }
    };
}

impl_mixed_scalar_mul!(i32, i64 => i64);
impl_mixed_scalar_mul!(i64, i32 => i64);
impl_mixed_scalar_mul!(i32, f64 => f64);
impl_mixed_scalar_mul!(f64, i32 => f64);
impl_mixed_scalar_mul!(i64, f64 => f64);
impl_mixed_scalar_mul!(f64, i64 => f64);

/// `T * VECTOR2<U>`: a vector of `std::common_type_t<T, U>`.
macro_rules! impl_scalar_times_vector {
    ($scalar:ty, $vec:ty => $out:ty) => {
        impl Mul<Vector2<$vec>> for $scalar {
            type Output = Vector2<$out>;

            #[inline]
            #[allow(clippy::unnecessary_cast)]
            fn mul(self, a_vector: Vector2<$vec>) -> Vector2<$out> {
                Vector2::new(
                    self as $out * a_vector.x as $out,
                    self as $out * a_vector.y as $out,
                )
            }
        }
    };
}

impl_scalar_times_vector!(i32, i32 => i32);
impl_scalar_times_vector!(i64, i64 => i64);
impl_scalar_times_vector!(f64, f64 => f64);
impl_scalar_times_vector!(i32, i64 => i64);
impl_scalar_times_vector!(i64, i32 => i64);
impl_scalar_times_vector!(i32, f64 => f64);
impl_scalar_times_vector!(f64, i32 => f64);
impl_scalar_times_vector!(i64, f64 => f64);
impl_scalar_times_vector!(f64, i64 => f64);

/// `VECTOR2<T> + U` and `VECTOR2<T> - U` for `T != U`.  The result stays a `VECTOR2<T>`:
///
/// * floating point `T`: the sum is computed in the common type and converted back to `T`;
/// * integral `T`, integral `U`: likewise (modular conversion back to `T`);
/// * integral `T`, floating point `U`: the sum is rounded with `KiROUND` (to `int`!) and
///   converted to `T`.
macro_rules! impl_mixed_scalar_add_sub {
    ($vec:ty, $scalar:ty, |$x:ident, $s:ident| add: $add:expr, sub: $sub:expr) => {
        impl Add<$scalar> for Vector2<$vec> {
            type Output = Vector2<$vec>;

            #[inline]
            fn add(self, a_scalar: $scalar) -> Vector2<$vec> {
                let op = |$x: $vec, $s: $scalar| -> $vec { $add };

                Vector2::new(op(self.x, a_scalar), op(self.y, a_scalar))
            }
        }

        impl Sub<$scalar> for Vector2<$vec> {
            type Output = Vector2<$vec>;

            #[inline]
            fn sub(self, a_scalar: $scalar) -> Vector2<$vec> {
                let op = |$x: $vec, $s: $scalar| -> $vec { $sub };

                Vector2::new(op(self.x, a_scalar), op(self.y, a_scalar))
            }
        }
    };
}

impl_mixed_scalar_add_sub!(i32, f64, |x, s| add: ki_round(x as f64 + s), sub: ki_round(x as f64 - s));
impl_mixed_scalar_add_sub!(i32, i64, |x, s| add: (x as i64 + s) as i32, sub: (x as i64 - s) as i32);
impl_mixed_scalar_add_sub!(i32, u32, |x, s|
    add: (x as u32).wrapping_add(s) as i32, sub: (x as u32).wrapping_sub(s) as i32);
impl_mixed_scalar_add_sub!(i64, f64, |x, s|
    add: ki_round(x as f64 + s) as i64, sub: ki_round(x as f64 - s) as i64);
impl_mixed_scalar_add_sub!(i64, i32, |x, s| add: x + s as i64, sub: x - s as i64);
impl_mixed_scalar_add_sub!(i64, u32, |x, s| add: x + s as i64, sub: x - s as i64);
impl_mixed_scalar_add_sub!(u32, f64, |x, s|
    add: ki_round(x as f64 + s) as u32, sub: ki_round(x as f64 - s) as u32);
impl_mixed_scalar_add_sub!(u32, i32, |x, s| add: x.wrapping_add(s as u32), sub: x.wrapping_sub(s as u32));
impl_mixed_scalar_add_sub!(u32, i64, |x, s| add: (x as i64 + s) as u32, sub: (x as i64 - s) as u32);
impl_mixed_scalar_add_sub!(f64, i32, |x, s| add: x + s as f64, sub: x - s as f64);
impl_mixed_scalar_add_sub!(f64, i64, |x, s| add: x + s as f64, sub: x - s as f64);
impl_mixed_scalar_add_sub!(f64, u32, |x, s| add: x + s as f64, sub: x - s as f64);
impl_mixed_scalar_add_sub!(f32, i32, |x, s| add: x + s as f32, sub: x - s as f32);
impl_mixed_scalar_add_sub!(f32, u32, |x, s| add: x + s as f32, sub: x - s as f32);
impl_mixed_scalar_add_sub!(f32, f64, |x, s| add: (x as f64 + s) as f32, sub: (x as f64 - s) as f32);

/// Port of `LexicographicalMax`.
#[inline]
pub fn lexicographical_max<'a, T: Coord>(
    a_a: &'a Vector2<T>,
    a_b: &'a Vector2<T>,
) -> &'a Vector2<T> {
    if a_a.x > a_b.x || (a_a.x == a_b.x && a_a.y > a_b.y) {
        return a_a;
    }

    a_b
}

/// Port of `LexicographicalMin`.
#[inline]
pub fn lexicographical_min<'a, T: Coord>(
    a_a: &'a Vector2<T>,
    a_b: &'a Vector2<T>,
) -> &'a Vector2<T> {
    if a_a.x < a_b.x || (a_a.x == a_b.x && a_a.y < a_b.y) {
        return a_a;
    }

    a_b
}

/// Port of `LexicographicalCompare`: -1, 0 or 1.
#[inline]
pub fn lexicographical_compare<T: Coord>(a_a: &Vector2<T>, a_b: &Vector2<T>) -> i32 {
    if a_a.x < a_b.x {
        -1
    } else if a_a.x > a_b.x {
        1
    } else {
        // aA.x == aB.x
        if a_a.y < a_b.y {
            -1
        } else if a_a.y > a_b.y {
            1
        } else {
            0
        }
    }
}

/// Port of `equals( VECTOR2<T> const&, VECTOR2<T> const&, T aEpsilon )` for non-integer `T`.
///
/// Compare two vectors for equality within a required epsilon.
///
/// * `a_first` - value to compare.
/// * `a_second` - value to compare.
/// * `a_epsilon` - allowed error.
///
/// Returns true if the values considered equal within the specified epsilon, otherwise false.
#[inline]
pub fn equals_vec_eps<T: crate::math::util::KiFloat>(
    a_first: Vector2<T>,
    a_second: Vector2<T>,
    a_epsilon: T,
) -> bool {
    if !equals_eps(a_first.x, a_second.x, a_epsilon) {
        return false;
    }

    equals_eps(a_first.y, a_second.y, a_epsilon)
}

/// Port of `equals( VECTOR2<T> const&, VECTOR2<T> const& )` with the default epsilon
/// `std::numeric_limits<T>::epsilon()`.
#[inline]
pub fn equals_vec<T: crate::math::util::KiFloat>(
    a_first: Vector2<T>,
    a_second: Vector2<T>,
) -> bool {
    equals_vec_eps(a_first, a_second, T::EPSILON)
}

impl KiRound for Vector2<f64> {
    type Output = Vector2<i32>;

    /// Port of `KiROUND( const VECTOR2D& vec )`.
    #[inline]
    fn ki_round(self) -> Vector2<i32> {
        Vector2::new(ki_round(self.x), ki_round(self.y))
    }
}

/// Port of `KiROUND( double x, double y )`: the rounded point as a `VECTOR2I`.
#[inline]
pub fn ki_round_xy(x: f64, y: f64) -> Vector2I {
    Vector2::new(ki_round(x), ki_round(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecoord_limits() {
        assert_eq!(Vector2I::ECOORD_MAX, i64::MAX);
        assert_eq!(Vector2I::ECOORD_MIN, i64::MIN);
        assert_eq!(Vector2L::ECOORD_MAX, i64::MAX);
        assert_eq!(Vector2D::ECOORD_MAX, f64::MAX);
        assert_eq!(Vector2D::ECOORD_MIN, f64::MIN_POSITIVE);
    }

    #[test]
    fn format_and_display() {
        assert_eq!(Vector2I::new(1, -2).format(), "( xy 1 -2 )");
        assert_eq!(Vector2I::new(1, -2).to_string(), "( xy 1 -2 )");
        assert_eq!(Vector2I::new(1, -2).to_stream_string(), "[ 1 | -2 ]");
        assert_eq!(Vector2D::new(1.5, -2.5e-7).format(), "( xy 1.5 -2.5e-07 )");
        assert_eq!(
            Vector2D::new(1234567.0, 0.1 + 0.2).format(),
            "( xy 1.23457e+06 0.3 )"
        );
        assert_eq!(
            Vector2L::new(i64::MAX, i64::MIN).format(),
            "( xy 9223372036854775807 -9223372036854775808 )"
        );
    }

    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn ordering_is_lexicographical() {
        let a = Vector2I::new(1, 5);
        let b = Vector2I::new(2, 0);
        let c = Vector2I::new(1, 6);

        assert!(a < b);
        assert!(a < c);
        assert!(c < b);
        assert!(a <= a);
        assert!(b > c);
        assert_eq!(a.cmp(&b), core::cmp::Ordering::Less);
        assert_eq!(lexicographical_compare(&a, &b), -1);
        assert_eq!(lexicographical_compare(&b, &a), 1);
        assert_eq!(lexicographical_compare(&a, &a), 0);
        assert_eq!(lexicographical_compare(&a, &c), -1);
        assert_eq!(*lexicographical_max(&a, &c), c);
        assert_eq!(*lexicographical_min(&a, &c), a);

        // NaN compares false in every direction, as in C++
        let n = Vector2D::new(f64::NAN, 0.0);
        let z = Vector2D::new(0.0, 0.0);
        assert!(!(n < z) && !(n > z) && !(n <= z) && !(n >= z));
        let m = Vector2D::new(0.0, f64::NAN);
        assert!(!(m < z) && !(m > z) && !(m <= z) && !(m >= z));
        assert!(Vector2D::new(-1.0, f64::NAN) < z);
        assert!(Vector2D::new(-1.0, f64::NAN) <= z);
    }

    #[test]
    fn cast_follows_converting_constructor() {
        // Floating point to integral truncates (does not round) after clamping
        assert_eq!(Vector2D::new(1.9, -1.9).cast::<i32>(), Vector2I::new(1, -1));
        assert_eq!(
            Vector2D::new(1e30, -1e30).cast::<i32>(),
            Vector2I::new(i32::MAX, i32::MIN)
        );
        assert_eq!(
            Vector2D::new(1e30, -1e30).cast::<i64>(),
            Vector2L::new(i64::MAX, i64::MIN)
        );
        assert_eq!(
            Vector2D::new(1e30, -5.0).cast::<u32>(),
            Vector2::<u32>::new(u32::MAX, 0)
        );
        // Integral to integral clamps
        assert_eq!(
            Vector2L::new(1 << 40, -(1 << 40)).cast::<i32>(),
            Vector2I::new(i32::MAX, i32::MIN)
        );
        assert_eq!(
            Vector2I::new(-4, 3).cast::<u32>(),
            Vector2::<u32>::new(0, 3)
        );
        assert_eq!(
            Vector2::<u32>::new(u32::MAX, 7).cast::<i32>(),
            Vector2I::new(i32::MAX, 7)
        );
        // To floating point is a plain static_cast
        assert_eq!(
            Vector2L::new(i64::MAX, 1).cast::<f64>(),
            Vector2D::new(9223372036854775808.0, 1.0)
        );
        assert_eq!(
            Vector2I::from_vec(Vector2D::new(2.5, 3.5)),
            Vector2I::new(2, 3)
        );
        assert_eq!(Vector2L::from(Vector2I::new(-1, 2)), Vector2L::new(-1, 2));
        assert_eq!(
            Vector2D::from(Vector2I::new(-1, 2)),
            Vector2D::new(-1.0, 2.0)
        );
    }

    #[test]
    fn integer_division_rounds_through_ki_round() {
        assert_eq!(Vector2I::new(5, -5) / 2, Vector2I::new(3, -3));
        assert_eq!(Vector2I::new(5, -5) / 2.0, Vector2I::new(3, -3));
        assert_eq!(Vector2I::new(10, 20) / 4.0, Vector2I::new(3, 5));
        assert_eq!(Vector2D::new(5.0, -5.0) / 2, Vector2D::new(2.5, -2.5));
        // VECTOR2L division is clamped to the int range by KiROUND
        assert_eq!(
            Vector2L::new(1 << 40, 6) / 2.0,
            Vector2L::new(i32::MAX as i64, 3)
        );
    }

    #[test]
    fn mixed_operators() {
        let vi = Vector2I::new(3, -4);
        let vl = Vector2L::new(10_000_000_000, 1);
        let vd = Vector2D::new(0.5, 0.25);

        assert_eq!(vi + vl, Vector2L::new(10_000_000_003, -3));
        assert_eq!(vl - vi, Vector2L::new(9_999_999_997, 5));
        assert_eq!(vi + vd, Vector2D::new(3.5, -3.75));
        assert_eq!(vd - vi, Vector2D::new(-2.5, 4.25));
        assert_eq!(vi * vi, 25i64);
        assert_eq!(vi * vl, 30_000_000_000i64 - 4);
        assert_eq!(vi * vd, 0.5f64);
        assert_eq!(vi * 2, Vector2I::new(6, -8));
        assert_eq!(2 * vi, Vector2I::new(6, -8));
        assert_eq!(vi * 0.5, Vector2D::new(1.5, -2.0));
        assert_eq!(0.5 * vi, Vector2D::new(1.5, -2.0));
        assert_eq!(
            vi * 10_000_000_000i64,
            Vector2L::new(30_000_000_000, -40_000_000_000)
        );
        assert_eq!(-vi, Vector2I::new(-3, 4));
        // KiROUND( 3.5 ) and KiROUND( -3.5 ): halfway cases round away from zero
        assert_eq!(vi + 0.5, Vector2I::new(4, -4));
        assert_eq!(vi - 0.5, Vector2I::new(3, -5));
        assert_eq!(vl + 1, Vector2L::new(10_000_000_001, 2));
        assert_eq!(vd + 1, Vector2D::new(1.5, 1.25));
    }

    #[test]
    fn compound_assignment() {
        let mut v = Vector2I::new(1, 2);
        v += Vector2I::new(10, 20);
        assert_eq!(v, Vector2I::new(11, 22));
        v -= Vector2I::new(1, 2);
        assert_eq!(v, Vector2I::new(10, 20));
        v += 5;
        assert_eq!(v, Vector2I::new(15, 25));
        v -= 10;
        assert_eq!(v, Vector2I::new(5, 15));
        v *= 2;
        assert_eq!(v, Vector2I::new(10, 30));
        v *= Vector2I::new(2, 3);
        assert_eq!(v, Vector2I::new(20, 90));
    }

    #[test]
    fn perpendicular_cross_dot() {
        let v = Vector2I::new(3, 4);
        assert_eq!(v.perpendicular(), Vector2I::new(-4, 3));
        assert_eq!(v.cross(v.perpendicular()), 25);
        assert_eq!(v.dot(v.perpendicular()), 0);
        assert_eq!(v.squared_distance(Vector2I::new(0, 0)), 25);
        assert_eq!(v.distance(Vector2I::new(0, 0)), 5.0);
        assert_eq!(v.distance(Vector2L::new(6, 8)), 5.0);
        // The products are computed in the extended type
        let big = Vector2I::new(i32::MAX, i32::MAX);
        assert_eq!(big.dot(big), 2 * (i32::MAX as i64) * (i32::MAX as i64));
    }

    #[test]
    fn euclidean_norm_double_special_cases() {
        assert_eq!(Vector2D::new(3.0, 4.0).euclidean_norm(), 5.0);
        assert_eq!(Vector2D::new(0.0, -4.0).euclidean_norm(), 4.0);
        assert_eq!(Vector2D::new(-3.0, 0.0).euclidean_norm(), 3.0);
        assert_eq!(Vector2D::new(1.0, -1.0).euclidean_norm(), SQRT_2);
        assert_eq!(Vector2I::new(1, 1).euclidean_norm(), 1);
        assert_eq!(Vector2I::new(2, 2).euclidean_norm(), 3);
        assert_eq!(Vector2I::new(3, 4).euclidean_norm(), 5);
    }

    #[test]
    fn resize_special_cases() {
        assert_eq!(Vector2I::new(0, 0).resize(10), Vector2I::new(0, 0));
        assert_eq!(Vector2I::new(10, 0).resize(5), Vector2I::new(5, 0));
        assert_eq!(Vector2I::new(0, -10).resize(5), Vector2I::new(0, -5));
        assert_eq!(Vector2I::new(10, 0).resize(-5), Vector2I::new(-5, 0));
        assert_eq!(Vector2I::new(10, 0).resize(0), Vector2I::new(0, 0));
        assert_eq!(Vector2I::new(-7, 7).resize(100), Vector2I::new(-71, 71));
        assert_eq!(
            Vector2D::new(3.0, 4.0).resize(10.0),
            Vector2D::new(6.0, 8.0)
        );
        assert_eq!(
            Vector2D::new(0.0, 0.0).resize(10.0),
            Vector2D::new(0.0, 0.0)
        );
        assert_eq!(Vector2L::new(3, 4).resize(10), Vector2L::new(6, 8));
    }

    #[test]
    fn ki_round_vectors() {
        assert_eq!(ki_round(Vector2D::new(0.5, -0.5)), Vector2I::new(1, -1));
        assert_eq!(
            ki_round(Vector2D::new(1e30, f64::NAN)),
            Vector2I::new(i32::MAX, 0)
        );
        assert_eq!(ki_round_xy(2.4, -2.6), Vector2I::new(2, -3));
    }

    #[test]
    fn equals_for_vectors() {
        assert!(equals_vec(
            Vector2D::new(0.1 + 0.2, 1.0),
            Vector2D::new(0.3, 1.0)
        ));
        assert!(!equals_vec(
            Vector2D::new(0.31, 1.0),
            Vector2D::new(0.3, 1.0)
        ));
        assert!(equals_vec_eps(
            Vector2D::new(1.0, 2.0),
            Vector2D::new(3.0, 4.0),
            2.0
        ));
    }
}
