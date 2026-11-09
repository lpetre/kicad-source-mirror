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

//! Port of the comparison part of `pcbnew/footprint.cpp`: `FOOTPRINT::cmp_drawings`,
//! `cmp_pads`, `cmp_zones` (save-order comparators, `board-model-types.md` 18) and
//! `FOOTPRINT::operator==` (package M20).

use std::cmp::Ordering;

use crate::ctx::BoardCtx;
use crate::footprint::Footprint;
use crate::ids::ItemId;

/// `FOOTPRINT::cmp_pads` as an ordering (`Less` where the C++ functor returns true).
pub fn footprint_cmp_pads(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("FOOTPRINT::cmp_pads", ctx, a, b)
}

/// `FOOTPRINT::cmp_drawings` as an ordering.
pub fn footprint_cmp_drawings(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("FOOTPRINT::cmp_drawings", ctx, a, b)
}

/// `FOOTPRINT::cmp_zones` as an ordering.
pub fn footprint_cmp_zones(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("FOOTPRINT::cmp_zones", ctx, a, b)
}

impl Footprint {
    /// `FOOTPRINT::operator==( const FOOTPRINT& )` (children compared through `ctx`).
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Footprint) -> bool {
        crate::port_pending!("FOOTPRINT::operator==", ctx, other)
    }
}
