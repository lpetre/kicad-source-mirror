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

//! Port of `libs/kimath/include/geometry/half_line.h` and `libs/kimath/src/geometry/half_line.cpp`.
//!
//! `ClipHalfLineToBox()`, declared in `half_line.h`, is implemented in `shape_utils.cpp` and
//! ported in [`crate::geometry::shape_utils`].

use crate::geometry::seg::{Ecoord, OptVector2I, Seg};
use crate::math::util::rescale;
use crate::math::vector2::{Vector2, Vector2I, Vector2L};

/// Port of `HALF_LINE`: a geometric half-line of infinite length (a ray), starting at a given
/// point and extending infinitely.
///
/// In terms of geometric ops, a SEG would probably do in most cases, as it
/// has the same definition, but a separate class is more explicit and also
/// allows compile-time reasoning about the meaning of the object through
/// the type system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalfLine {
    /// Internally, we can represent a just a segment that the ray passes through
    seg: Seg,
}

/// Port of `VectorsInSameQuadrant()`: check if two vectors point into the same quadrant.
pub fn vectors_in_same_quadrant(a_a: Vector2I, a_b: Vector2I) -> bool {
    // The sign of the x and y components of the vectors must be the same
    ((a_a.x >= 0) == (a_b.x >= 0)) && ((a_a.y >= 0) == (a_b.y >= 0))
}

impl HalfLine {
    /// Port of `HALF_LINE( const SEG& )`: the ray starts at the segment's A point and extends
    /// infinitely in the direction of the segment, passing through its B point.
    pub fn from_seg(a_seg: Seg) -> Self {
        HalfLine { seg: a_seg }
    }

    /// Port of `HALF_LINE( const VECTOR2I& aStart, const VECTOR2I& aOtherContainedPoint )`.
    pub fn new(a_start: Vector2I, a_other_contained_point: Vector2I) -> Self {
        HalfLine {
            seg: Seg::new(a_start, a_other_contained_point),
        }
    }

    /// Port of `GetStart()`: the start point of the ray.
    pub fn start(&self) -> Vector2I {
        self.seg.a
    }

    /// Port of `GetContainedPoint()`: one (of the infinite number) of points that the ray
    /// passes through.
    pub fn contained_point(&self) -> Vector2I {
        self.seg.b
    }

    /// Port of `GetContainedSeg()`.
    pub fn contained_seg(&self) -> &Seg {
        &self.seg
    }

    /// Port of `HALF_LINE::Contains()`.
    pub fn contains(&self, a_point: Vector2I) -> bool {
        // Check that the point is on the right side of the ray from
        // the start point
        // This is quick, so we can do it first
        if !vectors_in_same_quadrant(self.seg.b - self.seg.a, a_point - self.seg.a) {
            return false;
        }

        // Check that the point is within a distance of 1 from the
        // infinite line of the ray
        self.seg.line_distance(a_point) <= 1
    }

    /// Port of `HALF_LINE::Intersect( const SEG& )`.
    pub fn intersect_seg(&self, a_seg: &Seg) -> OptVector2I {
        // Intsersection of two infinite lines
        let seg = *self.contained_seg();
        let intersection = a_seg.intersect_with(&seg, false, true)?;

        // Check that the intersection is on the right side of the
        // ray's start point (i.e. equal quadrants)
        if !vectors_in_same_quadrant(self.seg.b - self.seg.a, intersection - self.seg.a) {
            return None;
        }

        // Check that the intersection is not somewhere past the end
        // of the segment
        if !a_seg.contains(intersection) {
            return None;
        }

        Some(intersection)
    }

    /// Port of `HALF_LINE::Intersect( const HALF_LINE& )`.
    pub fn intersect(&self, a_other: &HalfLine) -> OptVector2I {
        // Intsersection of two infinite lines
        let other_seg = *a_other.contained_seg();
        let intersection = self.seg.intersect_with(&other_seg, false, true)?;

        // Check that the intersection is on the right side of both
        // rays' start points (i.e. equal quadrants)
        if !vectors_in_same_quadrant(self.seg.b - self.seg.a, intersection - self.seg.a)
            || !vectors_in_same_quadrant(a_other.seg.b - a_other.seg.a, intersection - a_other.seg.a)
        {
            return None;
        }

        Some(intersection)
    }

    /// Port of `HALF_LINE::NearestPoint()`: the nearest point on the ray to the given point
    /// (the start point of the ray for half the 2D plane).
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

        if t < 0 {
            return self.seg.a;
        }

        let xp: Ecoord = rescale(t, d.x, l_squared);
        let yp: Ecoord = rescale(t, d.y, l_squared);

        Vector2::<i32>::from_vec(Vector2L::new(self.seg.a.x as i64 + xp, self.seg.a.y as i64 + yp))
    }
}
