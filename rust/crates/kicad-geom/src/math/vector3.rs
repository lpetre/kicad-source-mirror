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

//! Port of `libs/kimath/include/math/vector3.h` (`VECTOR3<T>`, `VECTOR3I`, `VECTOR3D`,
//! `VECTOR3U`).
//!
//! The template is instantiated for `int`, `unsigned int` and `double` (the typedefs).  Integer
//! arithmetic wraps on overflow like the reference build (C++ signed overflow is undefined,
//! g++ on x86-64 wraps).

use core::fmt;
use core::ops::{DivAssign, MulAssign};

use crate::ostream::format_double;

/// Port of `VECTOR3_TRAITS` and the arithmetic `VECTOR3<T>` needs of `T`.
pub trait Vector3Coord: Copy + PartialEq + PartialOrd + Default + fmt::Debug {
    /// `VECTOR3_TRAITS<T>::extended_type`
    type Extended: Copy + PartialEq + fmt::Debug;

    /// `T * T`
    fn c_mul(self, b: Self) -> Self;
    /// `T - T`
    fn c_sub(self, b: Self) -> Self;
    /// `T / T` (`None` for an integer division by zero, which is a trap in C++)
    fn c_div(self, b: Self) -> Option<Self>;
    /// `Dot()`: `extended{x} * extended{ax} + ...`
    fn dot3(a: [Self; 3], b: [Self; 3]) -> Self::Extended;
    /// `EuclideanNorm()`: `sqrt( (extended) x * x + ... )` converted back to `T`
    fn norm3(a: [Self; 3]) -> Self;
    /// `T( 0 )`
    fn zero() -> Self;
    /// `operator<<` text of one component
    fn stream(self) -> String;
}

impl Vector3Coord for i32 {
    type Extended = i64;

    fn c_mul(self, b: Self) -> Self {
        self.wrapping_mul(b)
    }

    fn c_sub(self, b: Self) -> Self {
        self.wrapping_sub(b)
    }

    fn c_div(self, b: Self) -> Option<Self> {
        if b == 0 {
            None
        } else {
            Some(self.wrapping_div(b))
        }
    }

    fn dot3(a: [Self; 3], b: [Self; 3]) -> i64 {
        (a[0] as i64 * b[0] as i64)
            .wrapping_add(a[1] as i64 * b[1] as i64)
            .wrapping_add(a[2] as i64 * b[2] as i64)
    }

    fn norm3(a: [Self; 3]) -> Self {
        let s = (a[0] as i64 * a[0] as i64)
            .wrapping_add(a[1] as i64 * a[1] as i64)
            .wrapping_add(a[2] as i64 * a[2] as i64);
        crate::cmath::f64_to_i32_x86((s as f64).sqrt())
    }

    fn zero() -> Self {
        0
    }

    fn stream(self) -> String {
        self.to_string()
    }
}

impl Vector3Coord for u32 {
    type Extended = u32;

    fn c_mul(self, b: Self) -> Self {
        self.wrapping_mul(b)
    }

    fn c_sub(self, b: Self) -> Self {
        self.wrapping_sub(b)
    }

    fn c_div(self, b: Self) -> Option<Self> {
        if b == 0 { None } else { Some(self / b) }
    }

    fn dot3(a: [Self; 3], b: [Self; 3]) -> u32 {
        a[0].wrapping_mul(b[0])
            .wrapping_add(a[1].wrapping_mul(b[1]))
            .wrapping_add(a[2].wrapping_mul(b[2]))
    }

    fn norm3(a: [Self; 3]) -> Self {
        let s = a[0]
            .wrapping_mul(a[0])
            .wrapping_add(a[1].wrapping_mul(a[1]))
            .wrapping_add(a[2].wrapping_mul(a[2]));
        // `sqrt( unsigned )` is `sqrt( double )`, always in range
        (s as f64).sqrt() as u32
    }

    fn zero() -> Self {
        0
    }

    fn stream(self) -> String {
        self.to_string()
    }
}

impl Vector3Coord for f64 {
    type Extended = f64;

    fn c_mul(self, b: Self) -> Self {
        self * b
    }

    fn c_sub(self, b: Self) -> Self {
        self - b
    }

    fn c_div(self, b: Self) -> Option<Self> {
        Some(self / b)
    }

    fn dot3(a: [Self; 3], b: [Self; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn norm3(a: [Self; 3]) -> Self {
        (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
    }

    fn zero() -> Self {
        0.0
    }

    fn stream(self) -> String {
        format_double(self)
    }
}

/// Port of `VECTOR3<T>`: a general 3D vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3<T> {
    /// `x`
    pub x: T,
    /// `y`
    pub y: T,
    /// `z`
    pub z: T,
}

/// Port of `VECTOR3D`.
pub type Vector3D = Vector3<f64>;
/// Port of `VECTOR3I`.
pub type Vector3I = Vector3<i32>;
/// Port of `VECTOR3U`.
pub type Vector3U = Vector3<u32>;

impl<T: Vector3Coord> Vector3<T> {
    /// `VECTOR3( T x, T y, T z )`.
    pub const fn new(x: T, y: T, z: T) -> Self {
        Vector3 { x, y, z }
    }

    /// Port of `VECTOR3<T>::Cross`.
    pub fn cross(&self, a_vector: &Vector3<T>) -> Vector3<T> {
        Vector3::new(
            self.y.c_mul(a_vector.z).c_sub(self.z.c_mul(a_vector.y)),
            self.z.c_mul(a_vector.x).c_sub(self.x.c_mul(a_vector.z)),
            self.x.c_mul(a_vector.y).c_sub(self.y.c_mul(a_vector.x)),
        )
    }

    /// Port of `VECTOR3<T>::Dot`.
    pub fn dot(&self, a_vector: &Vector3<T>) -> T::Extended {
        T::dot3(
            [self.x, self.y, self.z],
            [a_vector.x, a_vector.y, a_vector.z],
        )
    }

    /// Port of `VECTOR3<T>::EuclideanNorm`.
    pub fn euclidean_norm(&self) -> T {
        T::norm3([self.x, self.y, self.z])
    }

    /// Port of `VECTOR3<T>::Normalize`: normalizes in place and returns the result.
    ///
    /// `wxCHECK_MSG( norm > T( 0 ), *this, ... )`: a zero (or NaN) length leaves the vector
    /// unchanged.
    pub fn normalize(&mut self) -> Vector3<T> {
        let norm = self.euclidean_norm();

        if !(norm > T::zero()) {
            return *self;
        }

        // norm > 0, so the integer divisions cannot trap
        self.x = self.x.c_div(norm).unwrap_or(self.x);
        self.y = self.y.c_div(norm).unwrap_or(self.y);
        self.z = self.z.c_div(norm).unwrap_or(self.z);
        *self
    }

    /// Port of `VECTOR3<T>::SetAll`.
    pub fn set_all(&mut self, val: T) -> Vector3<T> {
        self.x = val;
        self.y = val;
        self.z = val;
        *self
    }
}

impl Vector3<f64> {
    /// The converting constructor `VECTOR3<double>( const VECTOR3<int>& )`.
    pub fn from_i(a_vec: Vector3I) -> Self {
        Vector3::new(a_vec.x as f64, a_vec.y as f64, a_vec.z as f64)
    }
}

impl<T: Vector3Coord> MulAssign<T> for Vector3<T> {
    /// Port of `VECTOR3<T>::operator*=( T )`.
    fn mul_assign(&mut self, a_scalar: T) {
        self.x = self.x.c_mul(a_scalar);
        self.y = self.y.c_mul(a_scalar);
        self.z = self.z.c_mul(a_scalar);
    }
}

impl<T: Vector3Coord> DivAssign<T> for Vector3<T> {
    /// Port of `VECTOR3<T>::operator/=( T )`.
    ///
    /// An integer division by zero (a hardware trap in C++) leaves the components unchanged.
    fn div_assign(&mut self, a_scalar: T) {
        self.x = self.x.c_div(a_scalar).unwrap_or(self.x);
        self.y = self.y.c_div(a_scalar).unwrap_or(self.y);
        self.z = self.z.c_div(a_scalar).unwrap_or(self.z);
    }
}

impl<T: Vector3Coord> fmt::Display for Vector3<T> {
    /// Port of `operator<<( std::ostream&, const VECTOR3<T>& )`: `[ x | y | z ]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[ {} | {} | {} ]",
            self.x.stream(),
            self.y.stream(),
            self.z.stream()
        )
    }
}
