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

//! Port of `pcbnew/netinfo.h` (`NETINFO_ITEM`, `NETINFO_LIST`): data, constructor defaults and
//! the delegation shims (package M0d). The behaviour of `netinfo_item.cpp` /
//! `netinfo_list.cpp` is package M3.

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::string_utils::unescape_string;
use kicad_core::{KicadT, Kiid, KiidGenerator, NILUUID, PcbLayerId};
use kicad_geom::{Box2I, Vector2I};

use crate::ctx::BoardCtx;
use crate::ids::{NetCode, PadId};
use crate::item::BoardItemBase;
use crate::netclass::{NETCLASS_DEFAULT_NAME, Netclass};
use crate::traits::{BoardItem, Invalidate};

/// `NETINFO_ITEM`.
#[derive(Clone, Debug)]
pub struct NetinfoItem {
    /// It is a BOARD_ITEM in C++: uuid, parent = Board; `base.id` is the `ItemKind::Net` handle.
    /// `BoardItem` is implemented for it (so `ItemRef::as_dyn()` is total).
    pub base: BoardItemBase,
    pub net_code: NetCode,                 // m_netCode
    pub netname: String,                   // m_netname
    pub short_netname: String,             // m_shortNetname
    pub display_netname: String,           // m_displayNetname
    pub net_chain: String,                 // m_netChain
    pub terminal_pads: [Option<PadId>; 2], // m_terminalPads
    pub terminal_pad_uuids: [Kiid; 2],     // m_terminalPadUuids
    /// m_netClass: the effective class, set by `synchronize_nets_and_netclasses`.
    pub netclass: Option<Arc<Netclass>>,
    pub is_current: bool, // m_isCurrent
}

impl Default for NetinfoItem {
    /// `NETINFO_ITEM( nullptr, wxEmptyString, -1 )` with a nil UUID (the argument defaults of
    /// the constructor; without a board the class is a fresh `NETCLASS( "Default" )`).
    fn default() -> Self {
        NetinfoItem::with_name("", NetCode::ORPHANED)
    }
}

impl NetinfoItem {
    /// `NETINFO_ITEM::NETINFO_ITEM( aParent, aNetName, aNetCode )` with a nil UUID:
    /// `m_shortNetname = aNetName.AfterLast( '/' )`, `m_displayNetname =
    /// UnescapeString( m_shortNetname )`, `m_isCurrent = true`, no terminal pads, and (no
    /// parent board) a fresh `NETCLASS( "Default" )`. A board-owned net takes the board's
    /// default class instead (`Board::add_net`).
    pub fn with_name(name: &str, code: NetCode) -> Self {
        // wxString::AfterLast returns the whole string when the character is absent.
        let short = name.rsplit_once('/').map_or(name, |(_, s)| s).to_string();

        NetinfoItem {
            base: BoardItemBase::default(),
            net_code: code,
            netname: name.to_string(),
            display_netname: unescape_string(&short),
            short_netname: short,
            net_chain: String::new(),
            terminal_pads: [None, None],
            terminal_pad_uuids: [NILUUID, NILUUID],
            netclass: Some(Arc::new(Netclass::new(NETCLASS_DEFAULT_NAME, true))),
            is_current: true,
        }
    }

    /// `new NETINFO_ITEM( aParent, aNetName, aNetCode )`: `with_name` plus the fresh UUID the
    /// `EDA_ITEM` constructor draws.
    pub fn new(kiid_generator: &mut KiidGenerator, name: &str, code: NetCode) -> Self {
        let mut net = NetinfoItem::with_name(name, code);
        net.base.uuid = Kiid::new(kiid_generator);
        net
    }

    /// `NETINFO_ITEM::GetNetCode`.
    pub fn net_code(&self) -> NetCode {
        self.net_code
    }

    /// `NETINFO_ITEM::GetNetname`.
    pub fn netname(&self) -> &str {
        &self.netname
    }

    /// `NETINFO_ITEM::GetShortNetname`.
    pub fn short_netname(&self) -> &str {
        &self.short_netname
    }

    /// `NETINFO_ITEM::GetDisplayNetname`.
    pub fn display_netname(&self) -> &str {
        &self.display_netname
    }

    /// `NETINFO_ITEM::GetNetChain`.
    pub fn net_chain(&self) -> &str {
        &self.net_chain
    }

    /// `NETINFO_ITEM::IsCurrent`.
    pub fn is_current(&self) -> bool {
        self.is_current
    }

    /// `NETINFO_ITEM::GetNetClass` (the effective class; `None` before synchronisation of a
    /// net that was built without one).
    pub fn netclass(&self) -> Option<&Arc<Netclass>> {
        self.netclass.as_ref()
    }

    /// `NETINFO_ITEM::SetNetname`: also refreshes the short and display names.
    pub fn set_netname(&mut self, name: &str) {
        crate::port_pending!("NETINFO_ITEM::SetNetname", name)
    }

    /// `NETINFO_ITEM::SetNetClass`.
    pub fn set_netclass(&mut self, netclass: Option<Arc<Netclass>>) {
        crate::port_pending!("NETINFO_ITEM::SetNetClass", netclass)
    }

    /// `NETINFO_ITEM::Clear` (`m_isCurrent = true`, netclass reset).
    pub fn clear(&mut self) {
        crate::port_pending!("NETINFO_ITEM::Clear")
    }

    /// `NETINFO_ITEM::GetLayer` override: `UNDEFINED_LAYER` (C++ also asserts: "doesn't have
    /// meaning").
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbLayerId::UndefinedLayer
    }

    /// `NETINFO_ITEM::GetPosition` override: always (0, 0).
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        Vector2I::new(0, 0)
    }

    /// `NETINFO_ITEM::GetBoundingBox` override: the union of the bounding boxes of the
    /// board's tracks, vias, zones and pads in this net.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("NETINFO_ITEM::GetBoundingBox", ctx)
    }
}

impl Invalidate for NetinfoItem {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for NetinfoItem {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbNetinfoT
    }

    fn class_name(&self) -> &'static str {
        "NETINFO_ITEM"
    }

    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        NetinfoItem::layer(self, ctx)
    }

    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        NetinfoItem::position(self, ctx)
    }

    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        NetinfoItem::bounding_box(self, ctx)
    }
}

/// `NETINFO_LIST`. The owner map is by code; `names` reproduces the `NETNAMES_MAP` iteration
/// (by name).
#[derive(Clone, Debug)]
pub struct NetinfoList {
    /// m_netCodes (+ ownership of m_netNames): the owner map, by code.
    pub(crate) nets: BTreeMap<NetCode, NetinfoItem>,
    pub(crate) names: BTreeMap<String, NetCode>, // m_netNames
    #[allow(dead_code)] // read by getFreeNetCode (package M3)
    pub(crate) new_net_code: i32, // m_newNetCode
    pub display_netnames_dirty: bool,            // m_DisplayNetnamesDirty
}

impl Default for NetinfoList {
    /// `NETINFO_LIST::NETINFO_LIST( nullptr )`: `m_newNetCode = 0` and the unconnected net
    /// (code 0, empty name) appended (`AppendNet` keeps code 0 because it equals the list
    /// size, and sets `m_DisplayNetnamesDirty`).
    fn default() -> Self {
        let mut list = NetinfoList {
            nets: BTreeMap::new(),
            names: BTreeMap::new(),
            new_net_code: 0,
            display_netnames_dirty: true,
        };

        list.names.insert(String::new(), NetCode::UNCONNECTED);
        list.nets.insert(
            NetCode::UNCONNECTED,
            NetinfoItem::with_name("", NetCode::UNCONNECTED),
        );
        list
    }
}

impl NetinfoList {
    /// `NETINFO_LIST::GetNetCount`.
    pub fn net_count(&self) -> usize {
        self.nets.len()
    }

    /// `NETINFO_LIST::GetNetItem( int )`.
    pub fn get_net_item(&self, code: NetCode) -> Option<&NetinfoItem> {
        self.nets.get(&code)
    }

    /// `NETINFO_LIST::GetNetItem( const wxString& )`.
    pub fn get_net_item_by_name(&self, name: &str) -> Option<&NetinfoItem> {
        self.names.get(name).and_then(|code| self.nets.get(code))
    }

    /// `NETINFO_LIST::NetsByName()` iteration order.
    pub fn nets_by_name(&self) -> impl Iterator<Item = &NetinfoItem> + '_ {
        self.names.values().filter_map(move |code| self.nets.get(code))
    }

    /// `NETINFO_LIST::NetsByNetcode()` iteration order.
    pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem> + '_ {
        self.nets.values()
    }

    /// `NETINFO_LIST::AppendNet`: `false` (and the net dropped) for a name already present;
    /// a code that is not the list size (or negative) gets `getFreeNetCode()`.
    pub fn append_net(&mut self, net: NetinfoItem) -> Option<NetCode> {
        crate::port_pending!("NETINFO_LIST::AppendNet", net)
    }

    /// `NETINFO_LIST::RemoveNet`.
    pub fn remove_net(&mut self, code: NetCode) {
        crate::port_pending!("NETINFO_LIST::RemoveNet", code)
    }

    /// `NETINFO_LIST::RemoveUnusedNets` helper part: drops every net whose `is_current` is
    /// false.
    pub fn remove_unused_nets(&mut self) {
        crate::port_pending!("NETINFO_LIST::RemoveUnusedNets")
    }

    /// `NETINFO_LIST::clear`.
    pub fn clear(&mut self) {
        crate::port_pending!("NETINFO_LIST::clear")
    }

    /// `NETINFO_LIST::buildListOfNets`.
    pub fn build_list_of_nets(&mut self) {
        crate::port_pending!("NETINFO_LIST::buildListOfNets")
    }

    /// `NETINFO_LIST::getFreeNetCode`.
    pub fn get_free_net_code(&mut self) -> i32 {
        crate::port_pending!("NETINFO_LIST::getFreeNetCode")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NETINFO_ITEM::NETINFO_ITEM` (`pcbnew/netinfo_item.cpp:43`) and
    /// `NETINFO_LIST::NETINFO_LIST` (`pcbnew/netinfo_list.cpp:39`).
    #[test]
    fn constructor_defaults() {
        let net = NetinfoItem::with_name("/sheet/sub/VOUT", NetCode(3));
        assert_eq!(net.net_code, NetCode(3));
        assert_eq!(net.short_netname, "VOUT");
        assert_eq!(net.display_netname, "VOUT");
        assert!(net.is_current);
        assert_eq!(net.terminal_pads, [None, None]);
        assert_eq!(net.terminal_pad_uuids, [NILUUID, NILUUID]);
        assert_eq!(net.netclass.as_ref().map(|c| c.name.as_str()), Some("Default"));
        assert_eq!(net.base.uuid, NILUUID);
        assert_eq!(NetinfoItem::with_name("GND", NetCode(1)).short_netname, "GND");

        let mut generator = KiidGenerator::default();
        assert_ne!(NetinfoItem::new(&mut generator, "A", NetCode(1)).base.uuid, NILUUID);

        let list = NetinfoList::default();
        assert_eq!(list.net_count(), 1);
        assert_eq!(list.new_net_code, 0);
        assert!(list.display_netnames_dirty);
        assert_eq!(
            list.get_net_item_by_name("").map(|n| n.net_code),
            Some(NetCode::UNCONNECTED)
        );
    }
}
