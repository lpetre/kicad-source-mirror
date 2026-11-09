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
use kicad_core::string_utils::unescape_string;

use crate::board_item::has_board;
use crate::ctx::BoardCtx;
use crate::ids::{ItemId, NetCode};
use crate::netclass::Netclass;
use crate::netinfo::NetinfoItem;
use crate::traits::BoardConnectedItem;

/// `m_netinfo` of an item: the board's net of its net code (`None` for a negative code, which
/// is the C++ null `m_netinfo`, and for a code the board does not know).
///
/// Deviation: C++ items that never had `SetNetCode` called point at the static
/// `NETINFO_LIST::OrphanedItem()` (net code 0, empty name, a fresh `NETCLASS( "Default" )`);
/// the Rust item stores only the code, so code 0 resolves to the board's net 0.
pub fn net<'a, B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'a>,
) -> Option<&'a NetinfoItem> {
    let code = item.net_code();

    if code.0 < 0 {
        return None;
    }

    let board: &'a crate::board::Board = ctx.board;
    board.find_net(code)
}

/// Port of `BOARD_CONNECTED_ITEM::GetEffectiveNetClass` (`board-model.md` 10: net code ->
/// `NetinfoItem::netclass`, else the default netclass, else `Netclass::fallback()`).
///
/// An item whose parent chain does not reach the board (`GetBoard() == nullptr`) takes the
/// static fallback: in C++ such an item holds `OrphanedItem()`, whose class is a fresh
/// `NETCLASS( "Default" )`, equal to the fallback.
pub fn effective_netclass<'a, B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'a>,
) -> &'a Netclass {
    // Static fallback netclass for items without a valid board (e.g., during DRC evaluation
    // of dummy items, or items not yet added to a board).
    if !has_board(item, ctx) {
        return Netclass::fallback();
    }

    if let Some(netclass) = net(item, ctx).and_then(|n| n.netclass()) {
        return netclass;
    }

    // `m_NetSettings` always exists on a Rust board.
    let board: &'a crate::board::Board = ctx.board;
    board.design_settings().net_settings.default_netclass()
}

/// Port of `BOARD_CONNECTED_ITEM::GetNetClassName`.
pub fn net_class_name<B: BoardConnectedItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    item.effective_netclass(ctx).name.clone()
}

/// Port of `BOARD_CONNECTED_ITEM::GetOwnClearance( aLayer, aSource )`: the DRC engine's
/// cached own clearance (`ctx.rules`), 0 (source untouched) without a board or rules.
///
/// The item is handed to the engine through its handle (`ctx.item( base.id )`); a value
/// outside the board (`base.id == None`) cannot be wrapped generically, so its class must
/// override `own_clearance` and pass `ctx.view( AnyItem::X( self ) )` itself (PAD does).
pub fn own_clearance<B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    want_source: bool,
) -> (i32, Option<String>) {
    if has_board(item, ctx) {
        if let Some(rules) = ctx.rules {
            let Some(this) = item.base().id().and_then(|id| ctx.item(id)) else {
                crate::port_pending!(
                    "BOARD_CONNECTED_ITEM::GetOwnClearance (detached item: no ItemRef of self)",
                    layer,
                    want_source
                )
            };

            return rules.cached_own_clearance(ctx, this, layer, want_source);
        }
    }

    (0, None)
}

/// Port of `BOARD_CONNECTED_ITEM::GetNetname`: the net's name, empty without a net.
pub fn netname<'a, B: BoardConnectedItem + ?Sized>(item: &B, ctx: &BoardCtx<'a>) -> &'a str {
    net(item, ctx).map_or("", |n| n.netname())
}

/// Port of `BOARD_CONNECTED_ITEM::GetNetnameMsg` (English; `_( "Not Found" )` untranslated).
pub fn netname_msg<B: BoardConnectedItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    if !has_board(item, ctx) {
        return "[** NO BOARD DEFINED **]".to_string();
    }

    let netname = netname(item, ctx);

    if netname.is_empty() {
        "[<no net>]".to_string()
    } else if item.net_code().0 < 0 {
        format!("[{}](Not Found)", unescape_string(netname))
    } else {
        format!("[{}]", unescape_string(netname))
    }
}

/// Port of `BOARD_CONNECTED_ITEM::GetShortNetname`.
pub fn short_netname<'a, B: BoardConnectedItem + ?Sized>(item: &B, ctx: &BoardCtx<'a>) -> &'a str {
    net(item, ctx).map_or("", |n| n.short_netname())
}

/// Port of `BOARD_CONNECTED_ITEM::GetDisplayNetname`. The C++ body first rebuilds the board's
/// display names when they are dirty (a `const` method mutating `mutable` state); here the
/// stored value is returned and the rebuild is the net list owner's job after net changes.
pub fn display_netname<'a, B: BoardConnectedItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'a>,
) -> &'a str {
    net(item, ctx).map_or("", |n| n.display_netname())
}

impl crate::board::Board {
    /// `BOARD_CONNECTED_ITEM::SetNetCode( aNetCode, aNoAssert )` with the board lookup: an
    /// item off copper gets net 0, an unknown code makes the item orphaned (-1) and returns
    /// `false`. `false` also for a stale or non-connected handle.
    ///
    /// A negative code or an item without a board takes `OrphanedItem()` (net code 0) and
    /// returns `true`, as C++. The `wxASSERT( m_netinfo )` is not reproduced (`aNoAssert` has
    /// no effect); `InvalidateClearanceCache` has no counterpart (`board-model.md` 7).
    pub fn set_item_net(&mut self, id: ItemId, net: NetCode, no_assert: bool) -> bool {
        let _ = no_assert;

        let new_net = {
            let ctx = BoardCtx::bare(self);
            let Some(item) = ctx.item(id) else {
                return false;
            };
            let Some(connected) = item.as_connected() else {
                return false;
            };

            let mut code = net;

            if !connected.is_on_copper_layer(&ctx) {
                code = NetCode::UNCONNECTED;
            }

            // if aNetCode < 0 (typically NETINFO_LIST::FORCE_ORPHANED) or no parent board,
            // set the m_netinfo to the dummy NETINFO_LIST::ORPHANED
            if code.0 >= 0 && item.has_board() {
                if self.find_net(code).is_some() {
                    code
                } else {
                    NetCode::ORPHANED
                }
            } else {
                NetCode::UNCONNECTED
            }
        };

        self.modify(id, |mut item, _| {
            if let Some(connected) = item.as_dyn_mut().connected_mut() {
                connected.net = new_net;
            }
        });

        new_net != NetCode::ORPHANED
    }
}
