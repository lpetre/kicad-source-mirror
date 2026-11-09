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

//! Port of `libs/kimath/include/math/wide_int.h`.
//!
//! 128-bit integers for exact products of 64-bit coordinate deltas.
//!
//! Coordinates span 2^32 units, so deltas need 33 bits and their cross and dot products need
//! 66.  `KI_INT128` holds those products exactly; in the port it is Rust's native `i128`
//! (`KI_UINT128` is `u128`).

use crate::math::vector2::Vector2L;

/// Port of `KI_INT128`.
pub type KiInt128 = i128;

/// Port of `KI_UINT128`.
pub type KiUint128 = u128;

/// Port of `KIGEOM_WIDE::WordsToDouble`: nearest double to hi * 2^64 + lo, ties to even.
///
/// The C++ only needs this for the MSVC 128-bit integer class; it is ported because it is
/// part of the header (and has unit tests).  [`to_double`] is what the rest of the port uses.
pub fn words_to_double(a_hi: i64, a_lo: u64) -> f64 {
    let negative: bool = a_hi < 0;
    let mut hi: u64 = a_hi as u64;
    let mut lo: u64 = a_lo;

    if negative {
        lo = (!lo).wrapping_add(1);
        hi = (!hi).wrapping_add(if lo == 0 { 1 } else { 0 });
    }

    let magnitude: f64 = if hi == 0 {
        lo as f64
    } else {
        // Fold the discarded low bits into a sticky bit so the single uint64 to double rounding
        // matches rounding the full 128-bit value
        let shift: u32 = hi.leading_zeros();
        let mut top: u64 = if shift == 0 {
            hi
        } else {
            (hi << shift) | (lo >> (64 - shift))
        };

        if (lo << shift) != 0 {
            top |= 1;
        }

        // std::ldexp( static_cast<double>( top ), 64 - shift ): the scaling by a power of two
        // is exact (the result is far from the overflow and subnormal ranges).  The factor
        // 2^(64 - shift) is built directly from its bit pattern.
        let scale: f64 = f64::from_bits((1023 + (64 - shift) as u64) << 52);

        top as f64 * scale
    };

    if negative { -magnitude } else { magnitude }
}

/// Port of `ToDouble( KI_INT128 )`: `static_cast<double>( aValue )`, i.e. the nearest double,
/// ties to even.
#[inline]
pub fn to_double(a_value: KiInt128) -> f64 {
    a_value as f64
}

/// Port of `CrossWide`.
///
/// Exact aA.x * aB.y - aA.y * aB.x.  Never overflows for any int64 components.
#[inline]
pub fn cross_wide(a_a: Vector2L, a_b: Vector2L) -> KiInt128 {
    a_a.x as i128 * a_b.y as i128 - a_a.y as i128 * a_b.x as i128
}

/// Port of `DotWide`.
///
/// Exact aA.x * aB.x + aA.y * aB.y.  Overflows only when all four components are INT64_MIN.
#[inline]
pub fn dot_wide(a_a: Vector2L, a_b: Vector2L) -> KiInt128 {
    a_a.x as i128 * a_b.x as i128 + a_a.y as i128 * a_b.y as i128
}
