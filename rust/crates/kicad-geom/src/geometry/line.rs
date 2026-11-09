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

//! Port of `libs/kimath/include/geometry/line.h` and `libs/kimath/src/geometry/line.cpp`.

use crate::geometry::seg::{Ecoord, OptVector2I, Seg};
use crate::math::util::rescale;
use crate::math::vector2::{Vector2, Vector2I, Vector2L};

/// Port of `LINE`: a geometric line of infinite length.
///
/// In terms of geometric ops, a SEG would probably do as it has the same definition,
/// but a separate class is more explicit and also allows compile-time
/// reasoning about the meaning of the object through the type system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Internally, we can represent a just a segment that the line passes through
    seg: Seg,
}

impl Line {
    /// Port of `LINE( const SEG& )`.
    pub fn from_seg(a_seg: Seg) -> Self {
        Line { seg: a_seg }
    }

    /// Port of `LINE( const VECTOR2I& aStart, const VECTOR2I& aEnd )`.
    pub fn new(a_start: Vector2I, a_end: Vector2I) -> Self {
        Line {
            seg: Seg::new(a_start, a_end),
        }
    }

    /// Port of `GetContainedSeg()`: one of the infinite number of segments that the line
    /// passes through.
    pub fn contained_seg(&self) -> &Seg {
        &self.seg
    }

    /// Port of `LINE::Intersect( const SEG& )`.
    pub fn intersect_seg(&self, a_seg: &Seg) -> OptVector2I {
        // intersect as two lines
        if let Some(intersection) = a_seg.intersect_with(&self.seg, false, true) {
            // Not parallel.
            // That was two lines, but we need to check if the intersection is on
            // the requested segment
            if a_seg.contains(intersection) {
                return Some(intersection);
            }
        }

        None
    }

    /// Port of `LINE::Intersect( const LINE& )`.
    pub fn intersect(&self, a_other: &Line) -> OptVector2I {
        // Defer to the SEG implementation
        a_other.seg.intersect_with(&self.seg, false, true)
    }

    /// Port of `LINE::Distance()`: the distance from the line to the given point.
    pub fn distance(&self, a_point: Vector2I) -> i32 {
        // Just defer to the SEG implementation
        self.seg.line_distance(a_point)
    }

    /// Port of `LINE::NearestPoint()`: the nearest point on the line to the given point.
    pub fn nearest_point(&self, a_point: Vector2I) -> Vector2I {
        // Same as the SEG implementation, but without the early return
        // if the point isn't on the segment.

        // Inlined for performance reasons
        let d = Vector2L::new(
            (self.seg.b.x - self.seg.a.x) as i64,
            (self.seg.b.y - self.seg.a.y) as i64,
        );
        let l_squared: Ecoord = d.x * d.x + d.y * d.y;

        if l_squared == 0 {
            return self.seg.a;
        }

        let t: Ecoord = d.dot(Vector2L::from(a_point - self.seg.a));

        let xp: Ecoord = rescale(t, d.x, l_squared);
        let yp: Ecoord = rescale(t, d.y, l_squared);

        Vector2::<i32>::from_vec(Vector2L::new(
            self.seg.a.x as i64 + xp,
            self.seg.a.y as i64 + yp,
        ))
    }
}
