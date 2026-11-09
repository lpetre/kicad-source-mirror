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

//! Port of the save-order comparators of `pcbnew/board.cpp` (`BOARD::cmp_items`,
//! `BOARD::cmp_drawings`). Package M22; M0d wrote the signatures. Free functions next to the
//! C++ comparator's type (`board-model.md` 10).

use std::cmp::Ordering;

use crate::ctx::BoardCtx;
use crate::ids::ItemId;

/// `BOARD::cmp_items::operator()`.
pub fn board_cmp_items(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("BOARD::cmp_items", ctx.board.time_stamp(), a, b)
}

/// `BOARD::cmp_drawings::operator()`.
pub fn board_cmp_drawings(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("BOARD::cmp_drawings", ctx.board.time_stamp(), a, b)
}
