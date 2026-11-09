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

//! Port of `pcbnew/board_connected_item.cpp` (`BOARD_CONNECTED_ITEM`): the base-class bodies
//! the `BoardConnectedItem` trait defaults forward to (`board-model.md` 6.2), generic over the
//! item. Package M2 ports the bodies; M0a wrote the signatures.

use kicad_core::PcbLayerId;

use crate::ctx::BoardCtx;
use crate::netclass::Netclass;
use crate::traits::BoardConnectedItem;

/// Port of `BOARD_CONNECTED_ITEM::GetEffectiveNetClass` (`board-model.md` 10: net code ->
/// `NetinfoItem::netclass`, else the default netclass, else `Netclass::fallback()`).
pub fn effective_netclass<'a, B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'a>,
) -> &'a Netclass {
    crate::port_pending!("BOARD_CONNECTED_ITEM::GetEffectiveNetClass", item, ctx)
}

/// Port of `BOARD_CONNECTED_ITEM::GetOwnClearance( aLayer, aSource )`.
pub fn own_clearance<B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    want_source: bool,
) -> (i32, Option<String>) {
    crate::port_pending!(
        "BOARD_CONNECTED_ITEM::GetOwnClearance",
        item,
        ctx,
        layer,
        want_source
    )
}
