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

/// `std::cbrt( double )`: a port of glibc's `__cbrt` (`sysdeps/ieee754/dbl-64/s_cbrt.c`,
/// Copyright (C) 1997-2024 Free Software Foundation, Inc., LGPL-2.1-or-later), because Rust's
/// `f64::cbrt` does not call the system libm and differs from glibc in the last bit for about
/// half of all inputs.  Verified bit for bit against glibc 2.39 on 200,000 random bit patterns
/// (normal, subnormal, both signs).  Used on every target, so this function is exact on wasm too.
pub fn cbrt(x: f64) -> f64 {
    const CBRT2: f64 = 1.259_921_049_894_873_164_8; // 2^(1/3)
    const SQR_CBRT2: f64 = 1.587_401_051_968_199_474_8; // 2^(2/3)
    const FACTOR: [f64; 5] = [1.0 / SQR_CBRT2, 1.0 / CBRT2, 1.0, CBRT2, SQR_CBRT2];

    // If X is not finite or is null return it (with raising exceptions if necessary).
    if !x.is_finite() || x == 0.0 {
        return x + x;
    }

    // Reduce X.  XM now is an range 1.0 to 0.5.
    let (xm, xe) = frexp(x.abs());

    #[allow(clippy::excessive_precision)]
    let u = 0.354895765043919860
        + ((1.50819193781584896
            + ((-2.11499494167371287
                + ((2.44693122563534430
                    + ((-1.83469277483613086
                        + (0.784932344976639262 - 0.145263899385486377 * xm) * xm)
                        * xm))
                    * xm))
                * xm))
            * xm);

    let t2 = u * u * u;
    let ym = u * (t2 + 2.0 * xm) / (2.0 * t2 + xm) * FACTOR[(2 + xe % 3) as usize];

    // __ldexp( x > 0.0 ? ym : -ym, xe / 3 ): the result is always a normal number
    let k = xe / 3;
    (if x > 0.0 { ym } else { -ym }) * f64::from_bits(((1023 + k) as u64) << 52)
}

/// `frexp` for finite, non-zero `x`: mantissa in [0.5, 1) and exponent.
fn frexp(x: f64) -> (f64, i32) {
    let bits = x.to_bits();
    let e = ((bits >> 52) & 0x7ff) as i32;

    if e == 0 {
        // subnormal: scale by 2^54 first
        let (m, ee) = frexp(x * f64::from_bits(0x4350_0000_0000_0000));
        return (m, ee - 54);
    }

    let m = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (m, e - 1022)
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

/// A plain C++ `(int)` conversion (or implicit conversion) of a `double`, as the reference
/// build (x86-64, `cvttsd2si`) executes it: truncation towards zero, and `INT_MIN` for NaN and
/// for every value whose truncation is outside the `int` range.  The C++ behaviour is
/// undefined in that case; Rust's `as` would saturate instead (PORTING.md section 9).
#[inline]
pub fn f64_to_i32_x86(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// A plain C++ conversion of a `double` to `int64_t` / `long long`, as the reference build
/// (x86-64, 64-bit `cvttsd2si`) executes it: truncation towards zero, and `INT64_MIN` for NaN
/// and for every out-of-range value (see [`f64_to_i32_x86`]).
#[inline]
pub fn f64_to_i64_x86(x: f64) -> i64 {
    if x >= -9223372036854775808.0 && x < 9223372036854775808.0 {
        x as i64
    } else {
        i64::MIN
    }
}

/// Plain C++ conversion of a coordinate type to `int` on the reference build: wrapping
/// narrowing for integers (two's complement, as g++ defines it), [`f64_to_i32_x86`] for
/// doubles.  Used by generic code (the `BOX2` methods) where the coordinate type varies.
pub trait ToI32X86 {
    /// `(int) self` as executed by the reference build.
    fn to_i32_x86(self) -> i32;
}

impl ToI32X86 for i32 {
    #[inline]
    fn to_i32_x86(self) -> i32 {
        self
    }
}

impl ToI32X86 for i64 {
    #[inline]
    fn to_i32_x86(self) -> i32 {
        self as i32
    }
}

impl ToI32X86 for f64 {
    #[inline]
    fn to_i32_x86(self) -> i32 {
        f64_to_i32_x86(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x86_conversions() {
        assert_eq!(f64_to_i32_x86(1.9), 1);
        assert_eq!(f64_to_i32_x86(-1.9), -1);
        assert_eq!(f64_to_i32_x86(2147483647.9), i32::MAX);
        assert_eq!(f64_to_i32_x86(-2147483648.9), i32::MIN);
        assert_eq!(f64_to_i32_x86(2147483648.0), i32::MIN);
        assert_eq!(f64_to_i32_x86(-2147483649.0), i32::MIN);
        assert_eq!(f64_to_i32_x86(f64::NAN), i32::MIN);
        assert_eq!(f64_to_i32_x86(f64::INFINITY), i32::MIN);
        assert_eq!(f64_to_i32_x86((i64::MAX as f64).sqrt()), i32::MIN);
        assert_eq!(f64_to_i64_x86(-9223372036854775808.0), i64::MIN);
        assert_eq!(f64_to_i64_x86(9223372036854775808.0), i64::MIN);
        assert_eq!(f64_to_i64_x86(9223372036854774784.0), 9223372036854774784);
        assert_eq!(f64_to_i64_x86(f64::NAN), i64::MIN);
        assert_eq!(f64_to_i64_x86(-3.5), -3);
    }

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
