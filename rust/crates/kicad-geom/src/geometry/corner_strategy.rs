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

//! Port of `libs/kimath/include/geometry/corner_strategy.h`.

/// Port of `CORNER_STRATEGY`: define how inflate transform build inflated polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CornerStrategy {
    /// `ALLOW_ACUTE_CORNERS`: just inflate the polygon. Acute angles create spikes
    AllowAcuteCorners,
    /// `CHAMFER_ACUTE_CORNERS`: Acute angles are chamfered
    ChamferAcuteCorners,
    /// `ROUND_ACUTE_CORNERS`: Acute angles are rounded
    RoundAcuteCorners,
    /// `CHAMFER_ALL_CORNERS`: All angles are chamfered.  The distance between new and old
    /// polygon edges is not constant, but do not change a lot
    ChamferAllCorners,
    /// `ROUND_ALL_CORNERS`: All angles are rounded.  The distance between new and old polygon
    /// edges is constant
    RoundAllCorners,
}
