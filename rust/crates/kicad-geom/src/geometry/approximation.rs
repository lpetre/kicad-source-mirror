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

//! Port of `libs/kimath/include/geometry/approximation.h`.

/// Port of `ERROR_LOC`.
///
/// When approximating an arc or circle, should the error be placed on the outside
/// or inside of the curve?  (Generally speaking filled shape errors go on the inside
/// and knockout errors go on the outside.  This preserves minimum clearances.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum ErrorLoc {
    /// `ERROR_OUTSIDE`
    Outside = 0,
    /// `ERROR_INSIDE`
    Inside,
}

/// `ERROR_OUTSIDE`
pub const ERROR_OUTSIDE: ErrorLoc = ErrorLoc::Outside;
/// `ERROR_INSIDE`
pub const ERROR_INSIDE: ErrorLoc = ErrorLoc::Inside;
