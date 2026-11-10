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

//! Placeholders for the functions of `pcbnew/drc/drc_creepage_utils.cpp` that part 1 (DRC-32)
//! calls but that belong to part 2 (work package DRC-33). They panic until DRC-33 lands.

use kicad_geom::Vector2I;
use kicad_pcb::BoardCtx;
use kicad_pcb::pcb_shape::PcbShape;

use super::shapes::CreepItem;

/// Port of `SegmentIntersectsBoard()` (DRC-33): whether the segment `a_p1 a_p2` stays clear of
/// the board edges `a_be` (`m_boardEdge`, the owned shapes in `a_owned`), ignoring
/// `a_dont_test_against`. Returns `false` when the segment crosses an edge.
pub fn segment_intersects_board(
    ctx: &BoardCtx<'_>,
    a_p1: Vector2I,
    a_p2: Vector2I,
    a_be: &[CreepItem],
    a_owned: &[PcbShape],
    a_dont_test_against: &[CreepItem],
    a_min_groove_width: i32,
) -> bool {
    crate::port_pending!(
        "SegmentIntersectsBoard (drc_creepage_utils.cpp, work package DRC-33)",
        ctx,
        a_p1,
        a_p2,
        a_be,
        a_owned,
        a_dont_test_against,
        a_min_groove_width
    )
}
