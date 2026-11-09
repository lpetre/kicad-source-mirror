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

//! Partial port of `libs/kimath/include/geometry/direction45.h`: only the `DIRECTION_45::Directions`
//! enum, which the `KIGEOM` shape utilities (`shape_utils.h`) and `ROUNDRECT` take as a
//! parameter.  The `DIRECTION_45` class itself is used by the interactive router only and is not
//! ported (see `PORT_STATUS.md`).

/// Port of `DIRECTION_45::Directions`: the eight compass directions (y grows downwards, so `N`
/// is towards negative y).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum Directions {
    /// `N`
    N = 0,
    /// `NE`
    NE = 1,
    /// `E`
    E = 2,
    /// `SE`
    SE = 3,
    /// `S`
    S = 4,
    /// `SW`
    SW = 5,
    /// `W`
    W = 6,
    /// `NW`
    NW = 7,
    /// `LAST`
    Last = 8,
    /// `UNDEFINED`
    Undefined = -1,
}
