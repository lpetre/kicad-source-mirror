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

//! The transcendental functions of the C math library (`<cmath>`), as the ported C++ code
//! calls them.  This module has no C++ source file of its own.
//!
//! IEEE 754 defines the result of `+ - * /`, `sqrt`, `fma`, `fmod`, `floor`, `round` and the
//! conversions exactly, so those behave identically everywhere and the port uses the Rust
//! operators and `f64` methods for them directly.  The functions below are different: their
//! results are only "accurate to about one ulp", and the last bit depends on the libm
//! implementation.  KiCad's results depend on that last bit whenever such a value is rounded
//! to an integer coordinate, so that the reference for the port is the libm of the reference
//! build of KiCad: glibc on x86-64 Linux.
//!
//! All transcendental function calls of the port go through this module, so that the choice
//! of the implementation is made in exactly one place.  Today the functions forward to
//! Rust's `std`, which
//!
//! * on Linux/glibc calls the very same libm as the C++ reference build (bit-identical
//!   results, verified by `tests/oracle_diff.rs`),
//! * on other targets - notably `wasm32-unknown-unknown`, where `std` uses the pure-Rust libm
//!   bundled with the compiler - can differ from the reference by one ulp.  See "Known gaps"
//!   in `PORT_STATUS.md`.
//!
//! Code built on this crate should call these functions (rather than the `f64` methods) for
//! any transcendental function that the C++ original evaluates, so that a future
//! glibc-compatible implementation makes all of it bit-exact on every target at once.

/// `std::sin( double )`.
#[inline]
pub fn sin(x: f64) -> f64 {
    x.sin()
}

/// `std::cos( double )`.
#[inline]
pub fn cos(x: f64) -> f64 {
    x.cos()
}

/// `std::tan( double )`.
#[inline]
pub fn tan(x: f64) -> f64 {
    x.tan()
}

/// `std::asin( double )`.
#[inline]
pub fn asin(x: f64) -> f64 {
    x.asin()
}

/// `std::acos( double )`.
#[inline]
pub fn acos(x: f64) -> f64 {
    x.acos()
}

/// `std::atan( double )`.
#[inline]
pub fn atan(x: f64) -> f64 {
    x.atan()
}

/// `std::atan2( double y, double x )`.
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// `std::hypot( double, double )`.
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}

/// `std::hypot( float, float )`.
#[inline]
pub fn hypotf(x: f32, y: f32) -> f32 {
    x.hypot(y)
}

/// `std::pow( double, double )`.
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

/// `std::exp( double )`.
#[inline]
pub fn exp(x: f64) -> f64 {
    x.exp()
}

/// `std::log( double )` (natural logarithm).
#[inline]
pub fn log(x: f64) -> f64 {
    x.ln()
}

/// `std::log10( double )`.
#[inline]
pub fn log10(x: f64) -> f64 {
    x.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_values() {
        // Values every reasonable libm gets exactly right
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(tan(0.0), 0.0);
        assert_eq!(asin(0.0), 0.0);
        assert_eq!(acos(1.0), 0.0);
        assert_eq!(atan(0.0), 0.0);
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(hypotf(3.0, 4.0), 5.0);
        assert_eq!(pow(10.0, 3.0), 1000.0);
        assert_eq!(pow(2.0, -2.0), 0.25);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(log(1.0), 0.0);
        assert_eq!(log10(1000.0), 3.0);
    }
}
