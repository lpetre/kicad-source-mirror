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

//! Port of `include/core/mirror.h`.
//!
//! The header lives in KiCad's top-level `include/core` directory but only depends on
//! kimath types, and the kimath shapes (`SHAPE_ARC::Mirror`, `SHAPE_LINE_CHAIN::Mirror`,
//! `SHAPE_POLY_SET::Mirror`, ...) take a `FLIP_DIRECTION`, so it is ported here.

use core::ops::{Add, Neg, Sub};

use crate::geometry::eda_angle::{ANGLE_0, ANGLE_90, EdaAngle};
use crate::math::vector2::{Coord, Vector2};

/// Port of `FLIP_DIRECTION`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlipDirection {
    /// Flip left to right (around the Y axis)
    LeftRight,
    /// Flip top to bottom (around the X axis)
    TopBottom,
}

/// Port of `MIRRORVAL`.
///
/// Returns the mirror of aPoint relative to the aMirrorRef.
#[inline]
pub fn mirror_val<T>(a_point: T, a_mirror_ref: T) -> T
where
    T: Copy + Sub<Output = T> + Add<Output = T> + Neg<Output = T>,
{
    -(a_point - a_mirror_ref) + a_mirror_ref
}

/// Port of `MIRROR( T& aPoint, const T& aMirrorRef )`.
///
/// Updates aPoint with the mirror of aPoint relative to the aMirrorRef.
#[inline]
pub fn mirror<T>(a_point: &mut T, a_mirror_ref: T)
where
    T: Copy + Sub<Output = T> + Add<Output = T> + Neg<Output = T>,
{
    *a_point = mirror_val(*a_point, a_mirror_ref);
}

/// Port of `MIRROR( VT& aPoint, const VT& aMirrorRef, FLIP_DIRECTION aFlipDirection )`.
///
/// Updates aPoint with the mirror of aPoint relative to the aMirrorRef,
/// in the specified direction.
#[inline]
pub fn mirror_point<T>(a_point: &mut Vector2<T>, a_mirror_ref: Vector2<T>, a_flip_direction: FlipDirection)
where
    T: Coord + Sub<Output = T> + Add<Output = T> + Neg<Output = T>,
{
    if a_flip_direction == FlipDirection::LeftRight {
        mirror(&mut a_point.x, a_mirror_ref.x);
    } else {
        mirror(&mut a_point.y, a_mirror_ref.y);
    }
}

/// Port of `MIRROR_AXIS( FLIP_DIRECTION aFlipDirection )`.
///
/// Returns the angle of the axis of reflection for the given flip direction.
#[inline]
pub const fn mirror_axis(a_flip_direction: FlipDirection) -> EdaAngle {
    match a_flip_direction {
        FlipDirection::LeftRight => ANGLE_90,
        FlipDirection::TopBottom => ANGLE_0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vector2::{Vector2D, Vector2I};

    #[test]
    fn mirror_helpers() {
        assert_eq!(mirror_val(10, 3), -4);
        assert_eq!(mirror_val(2.5, 1.0), -0.5);

        let mut v = 7;
        mirror(&mut v, 10);
        assert_eq!(v, 13);

        let mut p = Vector2I::new(10, 20);
        mirror_point(&mut p, Vector2I::new(0, 5), FlipDirection::LeftRight);
        assert_eq!(p, Vector2I::new(-10, 20));
        mirror_point(&mut p, Vector2I::new(0, 5), FlipDirection::TopBottom);
        assert_eq!(p, Vector2I::new(-10, -10));

        let mut d = Vector2D::new(1.5, 2.5);
        mirror_point(&mut d, Vector2D::new(1.0, 1.0), FlipDirection::TopBottom);
        assert_eq!(d, Vector2D::new(1.5, -0.5));

        assert_eq!(mirror_axis(FlipDirection::LeftRight), ANGLE_90);
        assert_eq!(mirror_axis(FlipDirection::TopBottom), ANGLE_0);
    }
}
