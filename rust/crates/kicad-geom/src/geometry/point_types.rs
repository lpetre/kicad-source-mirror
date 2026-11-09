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

//! Port of `libs/kimath/include/geometry/point_types.h` (`POINT_TYPE`, `TYPED_POINT2I`).

use crate::math::vector2::Vector2I;

/// Port of `POINT_TYPE`: meanings that can be assigned to a point in pure geometric terms.
///
/// For example, a circle has a center point and four quadrant points.  These can be combined
/// using bitwise OR if a point has multiple meanings, so they are plain `i32` flags as in C++.
pub type PointType = i32;

/// No specific point type.
pub const PT_NONE: PointType = 0;
/// The point is the center of something.
pub const PT_CENTER: PointType = 1 << 0;
/// The point is at the end of a segment, arc, etc.
pub const PT_END: PointType = 1 << 1;
/// The point is at the middle of a segment, arc, etc.
pub const PT_MID: PointType = 1 << 2;
/// The point is on a quadrant of a circle (N, E, S, W points).
pub const PT_QUADRANT: PointType = 1 << 3;
/// The point is a corner of a polygon, rectangle, etc (you may want to infer PT_END from this)
pub const PT_CORNER: PointType = 1 << 4;
/// The point is an intersection of two (or more) items.
pub const PT_INTERSECTION: PointType = 1 << 5;
/// The point is somewhere on another element, but not some specific point.
/// (you can infer this from some other point types)
pub const PT_ON_ELEMENT: PointType = 1 << 6;

/// Port of `TYPED_POINT2I`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedPoint2I {
    /// `m_point`
    pub point: Vector2I,
    /// `m_types`: bitwise OR of `POINT_TYPE` values
    pub types: i32,
}

impl TypedPoint2I {
    /// `TYPED_POINT2I( const VECTOR2I& aVec, int aTypes )`.
    pub const fn new(a_vec: Vector2I, a_types: i32) -> Self {
        TypedPoint2I {
            point: a_vec,
            types: a_types,
        }
    }
}
