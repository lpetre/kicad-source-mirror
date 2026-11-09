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
// Copyright (C) 2024 Alex Shvartzkop <dudesuchamazing@gmail.com>

//! Port of `libs/kimath/include/math/box2_minmax.h`.
//!
//! The constructor from a `SHAPE_ARC` (`BOX2I_MINMAX( const SHAPE_ARC& aArc )`, which is
//! `BOX2I_MINMAX( aArc.BBox() )`) is left to the shapes work package; use
//! [`Box2IMinMax::from_box`] with the arc's bounding box.

use crate::geometry::seg::Seg;
use crate::math::box2::{Box2I, box2i_safe_from_int};
use crate::math::vector2::{Vector2I, Vector2L};

/// Port of `BOX2I_MINMAX`: a min-max version of BOX2 for fast intersection checking.
///
/// The C++ members `m_Left`, `m_Top`, `m_Right` and `m_Bottom` are public; so are the fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Box2IMinMax {
    /// `m_Left`
    pub left: i32,
    /// `m_Top`
    pub top: i32,
    /// `m_Right`
    pub right: i32,
    /// `m_Bottom`
    pub bottom: i32,
}

impl Box2IMinMax {
    /// Port of `BOX2I_MINMAX( int aLeft, int aTop, int aRight, int aBottom )`.
    /// (`BOX2I_MINMAX()`, all zero, is `Box2IMinMax::default()`.)
    #[inline]
    pub const fn new(a_left: i32, a_top: i32, a_right: i32, a_bottom: i32) -> Self {
        Box2IMinMax {
            left: a_left,
            top: a_top,
            right: a_right,
            bottom: a_bottom,
        }
    }

    /// Port of `BOX2I_MINMAX( const VECTOR2I& aPt )`.
    #[inline]
    pub const fn from_point(a_pt: Vector2I) -> Self {
        Self::from_xy(a_pt.x, a_pt.y)
    }

    /// Port of `BOX2I_MINMAX( int aX, int aY )`.
    #[inline]
    pub const fn from_xy(a_x: i32, a_y: i32) -> Self {
        Box2IMinMax {
            left: a_x,
            top: a_y,
            right: a_x,
            bottom: a_y,
        }
    }

    /// Port of `BOX2I_MINMAX( const BOX2I& aBox )`.
    #[inline]
    pub fn from_box(a_box: &Box2I) -> Self {
        Box2IMinMax {
            left: a_box.left(),
            top: a_box.top(),
            right: a_box.right(),
            bottom: a_box.bottom(),
        }
    }

    /// Port of `BOX2I_MINMAX( const VECTOR2I& aA, const VECTOR2I& aB )`.
    #[inline]
    pub fn from_corners(a_a: Vector2I, a_b: Vector2I) -> Self {
        Box2IMinMax {
            left: a_a.x.min(a_b.x),
            right: a_a.x.max(a_b.x),
            top: a_a.y.min(a_b.y),
            bottom: a_a.y.max(a_b.y),
        }
    }

    /// Port of `BOX2I_MINMAX( const SEG& aSeg )`.
    #[inline]
    pub fn from_seg(a_seg: &Seg) -> Self {
        Self::from_corners(a_seg.a, a_seg.b)
    }

    /// Port of `BOX2I_MINMAX::operator BOX2I()`.
    #[inline]
    pub fn to_box2i(&self) -> Box2I {
        box2i_safe_from_int(
            Vector2I::new(self.left, self.top),
            Vector2L::new(
                self.right as i64 - self.left as i64,
                self.bottom as i64 - self.top as i64,
            ),
        )
    }

    /// Port of `BOX2I_MINMAX::Intersects()`.
    #[inline]
    pub fn intersects(&self, a_other: &Box2IMinMax) -> bool {
        // calculate the left common area coordinate:
        let left: i32 = self.left.max(a_other.left);
        // calculate the right common area coordinate:
        let right: i32 = self.right.min(a_other.right);
        // calculate the upper common area coordinate:
        let top: i32 = self.top.max(a_other.top);
        // calculate the lower common area coordinate:
        let bottom: i32 = self.bottom.min(a_other.bottom);

        // if a common area exists, it must have a positive (null accepted) size
        left <= right && top <= bottom
    }

    /// Port of `BOX2I_MINMAX::Merge()`.
    #[inline]
    pub fn merge(&mut self, a_pt: Vector2I) {
        self.left = self.left.min(a_pt.x);
        self.right = self.right.max(a_pt.x);
        self.top = self.top.min(a_pt.y);
        self.bottom = self.bottom.max(a_pt.y);
    }

    /// Port of `BOX2I_MINMAX::GetCenter()`.
    #[inline]
    pub fn center(&self) -> Vector2I {
        let cx: i32 = ((self.left as i64 + self.right as i64) / 2) as i32;
        let cy: i32 = ((self.top as i64 + self.bottom as i64) / 2) as i32;

        Vector2I::new(cx, cy)
    }

    /// Port of `BOX2I_MINMAX::GetDiameter()`.
    #[inline]
    pub fn diameter(&self) -> f64 {
        let start = Vector2L::new(self.left as i64, self.top as i64);
        let end = Vector2L::new(self.right as i64, self.bottom as i64);
        let d: Vector2L = end - start;

        // std::hypot( d.x, d.y )
        crate::cmath::hypot(d.x as f64, d.y as f64)
    }
}

impl From<Box2IMinMax> for Box2I {
    /// Port of `BOX2I_MINMAX::operator BOX2I()`.
    #[inline]
    fn from(b: Box2IMinMax) -> Box2I {
        b.to_box2i()
    }
}

impl From<&Box2I> for Box2IMinMax {
    #[inline]
    fn from(b: &Box2I) -> Box2IMinMax {
        Box2IMinMax::from_box(b)
    }
}

impl From<&Seg> for Box2IMinMax {
    #[inline]
    fn from(s: &Seg) -> Box2IMinMax {
        Box2IMinMax::from_seg(s)
    }
}

impl From<Vector2I> for Box2IMinMax {
    #[inline]
    fn from(p: Vector2I) -> Box2IMinMax {
        Box2IMinMax::from_point(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_queries() {
        let a = Box2IMinMax::from_corners(Vector2I::new(10, -5), Vector2I::new(-10, 5));
        assert_eq!(a, Box2IMinMax::new(-10, -5, 10, 5));
        assert_eq!(a.center(), Vector2I::new(0, 0));
        assert_eq!(a.diameter(), 500.0f64.sqrt());
        assert_eq!(Box2IMinMax::from_seg(&Seg::from_coords(10, -5, -10, 5)), a);
        assert_eq!(
            Box2IMinMax::from_point(Vector2I::new(3, 4)),
            Box2IMinMax::new(3, 4, 3, 4)
        );
        assert_eq!(Box2IMinMax::default(), Box2IMinMax::new(0, 0, 0, 0));

        let b = Box2I::new(Vector2I::new(1, 2), Vector2I::new(3, 4));
        let mm = Box2IMinMax::from_box(&b);
        assert_eq!(mm, Box2IMinMax::new(1, 2, 4, 6));
        assert_eq!(mm.to_box2i(), b);
        assert_eq!(Box2I::from(mm), b);

        assert!(a.intersects(&Box2IMinMax::new(10, 5, 20, 20)));
        assert!(!a.intersects(&Box2IMinMax::new(11, 5, 20, 20)));

        let mut m = Box2IMinMax::from_xy(0, 0);
        m.merge(Vector2I::new(-7, 9));
        assert_eq!(m, Box2IMinMax::new(-7, 0, 0, 9));

        // The full int range does not overflow
        let full = Box2IMinMax::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(full.center(), Vector2I::new(0, 0));
        assert_eq!(full.to_box2i().origin(), Vector2I::new(i32::MIN, i32::MIN));
        assert_eq!(full.to_box2i().end(), Vector2I::new(i32::MAX, i32::MAX));
    }
}
