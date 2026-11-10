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
//! the delegation shims (package M0d); the behaviour of `netinfo_item.cpp` /
//! `netinfo_list.cpp` (package M3). GUI parts (`GetMsgPanelInfo`, `RebuildDisplayNetnames`)
//! and the editor-only `RenameNets` are not ported.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use kicad_core::string_utils::unescape_string;
use kicad_core::{KicadT, Kiid, KiidGenerator, NILUUID, PcbLayerId};
use kicad_geom::{Box2I, Vector2I};

use crate::ctx::BoardCtx;
use crate::ids::{ItemId, NetCode, PadId};
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

    /// Port of `NETINFO_ITEM::SetNetname`: also refreshes the short and display names.
    pub fn set_netname(&mut self, name: &str) {
        self.netname = name.to_string();

        if name.contains('/') {
            self.short_netname = name.rsplit_once('/').map_or(name, |(_, s)| s).to_string();
        } else {
            self.short_netname = name.to_string();
        }

        self.display_netname = unescape_string(&self.short_netname);
    }

    /// `NETINFO_ITEM::SetNetClass` without the board: `Some` is stored; `None` (C++: "the
    /// board's default class") is stored as `None`, which every reader resolves to the board's
    /// default class (`board-model.md` 10). Board code uses `set_netclass_or_default`.
    pub fn set_netclass(&mut self, netclass: Option<Arc<Netclass>>) {
        self.netclass = netclass;
    }

    /// Port of `NETINFO_ITEM::SetNetClass` with the parent board's default class
    /// (`m_parent->GetDesignSettings().m_NetSettings->GetDefaultNetclass()`).
    pub fn set_netclass_or_default(
        &mut self,
        netclass: Option<Arc<Netclass>>,
        default: &Arc<Netclass>,
    ) {
        match netclass {
            Some(netclass) => self.netclass = Some(netclass),
            None => self.netclass = Some(Arc::clone(default)),
        }
    }

    /// `NETINFO_ITEM::Clear` without the board: the class becomes `None` (= the board's
    /// default, see `set_netclass`), chain and terminal pads are cleared. `m_isCurrent` is not
    /// touched (as in C++). Board code uses `clear_with_default`.
    pub fn clear(&mut self) {
        self.netclass = None;
        self.clear_chain_and_terminals();
    }

    /// Port of `NETINFO_ITEM::Clear` with the parent board's default class.
    pub fn clear_with_default(&mut self, default: &Arc<Netclass>) {
        self.netclass = Some(Arc::clone(default));
        self.clear_chain_and_terminals();
    }

    fn clear_chain_and_terminals(&mut self) {
        self.net_chain.clear();
        self.terminal_pads = [None, None];
        self.terminal_pad_uuids = [NILUUID, NILUUID];
    }

    /// `NETINFO_ITEM::SetNetChain`.
    pub fn set_net_chain(&mut self, net_chain: &str) {
        self.net_chain = net_chain.to_string();
    }

    /// `NETINFO_ITEM::GetTerminalPad`.
    pub fn terminal_pad(&self, index: usize) -> Option<PadId> {
        self.terminal_pads[index]
    }

    /// `NETINFO_ITEM::GetTerminalPadUuid`.
    pub fn terminal_pad_uuid(&self, index: usize) -> &Kiid {
        &self.terminal_pad_uuids[index]
    }

    /// `NETINFO_ITEM::SetTerminalPadUuid`.
    pub fn set_terminal_pad_uuid(&mut self, index: usize, uuid: Kiid) {
        self.terminal_pad_uuids[index] = uuid;
    }

    /// `NETINFO_ITEM::ClearTerminalPad`: pointer and UUID together.
    pub fn clear_terminal_pad(&mut self, index: usize) {
        self.terminal_pads[index] = None;
        self.terminal_pad_uuids[index] = NILUUID;
    }

    /// Port of `NETINFO_ITEM::SetTerminal`: pointer and UUID from one pad (`None`: nil UUID).
    pub fn set_terminal(&mut self, index: usize, pad: Option<(PadId, Kiid)>) {
        self.terminal_pads[index] = pad.map(|(id, _)| id);
        self.terminal_pad_uuids[index] = pad.map_or(NILUUID, |(_, uuid)| uuid);
    }

    /// `NETINFO_ITEM::SetIsCurrent`.
    pub fn set_is_current(&mut self, is_current: bool) {
        self.is_current = is_current;
    }

    /// Port of `NETINFO_ITEM::HasAutoGeneratedNetname`: the net was not labelled by the user.
    pub fn has_auto_generated_netname(&self) -> bool {
        self.short_netname.starts_with("Net-(") || self.short_netname.starts_with("unconnected-(")
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
    /// board's tracks, arcs, vias, zones, pads and shapes in this net
    /// (`CONNECTIVITY_DATA::GetNetItems`). Without a connectivity query the board's
    /// `CONNECTIVITY_DATA` is empty (`board-model.md` 6.1): the default box.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        const NET_ITEM_TYPES: [KicadT; 6] = [
            KicadT::PcbTraceT,
            KicadT::PcbArcT,
            KicadT::PcbViaT,
            KicadT::PcbZoneT,
            KicadT::PcbPadT,
            KicadT::PcbShapeT,
        ];

        let mut bbox = Box2I::default();

        if let Some(conn) = ctx.connectivity {
            for id in conn.net_items(self.net_code, &NET_ITEM_TYPES) {
                if let Some(item) = ctx.item(id) {
                    bbox.merge(&item.bounding_box());
                }
            }
        }

        bbox
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
    pub(crate) new_net_code: i32,                // m_newNetCode
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
            display_netnames_dirty: false,
        };

        // Make sure that the unconnected net has number 0
        list.append_net(NetinfoItem::with_name("", NetCode::UNCONNECTED));
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
        self.names
            .values()
            .filter_map(move |code| self.nets.get(code))
    }

    /// `NETINFO_LIST::NetsByNetcode()` iteration order.
    pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem> + '_ {
        self.nets.values()
    }

    /// `NETINFO_LIST::OrphanedItem()`: the static net (empty name, code 0, no board) that
    /// `BOARD::FindNet( 0 )` returns for an empty list.
    pub fn orphaned_item() -> &'static NetinfoItem {
        static ORPHANED_ITEM: OnceLock<NetinfoItem> = OnceLock::new();
        ORPHANED_ITEM.get_or_init(|| NetinfoItem::with_name("", NetCode::UNCONNECTED))
    }

    /// Mutable `GetNetItem( int )`.
    pub(crate) fn get_net_item_mut(&mut self, code: NetCode) -> Option<&mut NetinfoItem> {
        self.nets.get_mut(&code)
    }

    /// Port of `NETINFO_LIST::AppendNet`: `None` (and the net dropped, as C++ deletes it) for a
    /// name already present; a code that is not the list size (or negative) gets
    /// `getFreeNetCode()`. Returns the code the net is stored under; stamps `base.id` with the
    /// net handle (`ItemId::net`). (The C++ "same object" case cannot occur with owned values.)
    pub fn append_net(&mut self, mut net: NetinfoItem) -> Option<NetCode> {
        if self.names.contains_key(&net.netname) {
            return None;
        }

        // be sure that net codes are consecutive
        // negative net code means that it has to be auto assigned
        if i64::from(net.net_code.0) != self.nets.len() as i64 || net.net_code.0 < 0 {
            net.net_code = NetCode(self.get_free_net_code());
        }

        // net names & codes are supposed to be unique
        debug_assert!(!self.nets.contains_key(&net.net_code));

        let code = net.net_code;
        net.base.id = ItemId::net(code);

        // add an entry for fast look up by a net name using a map
        self.names.insert(net.netname.clone(), code);
        self.nets.insert(code, net);

        self.display_netnames_dirty = true;
        Some(code)
    }

    /// Port of `NETINFO_LIST::RemoveNet` (the net is dropped; C++ leaves deleting it to the
    /// caller). A code that is not in the list changes nothing.
    pub fn remove_net(&mut self, code: NetCode) {
        self.take_net(code);
    }

    /// `remove_net` returning the removed net.
    pub(crate) fn take_net(&mut self, code: NetCode) -> Option<NetinfoItem> {
        let removed = self.nets.remove(&code)?;

        let name = self
            .names
            .iter()
            .find(|&(_, &c)| c == code)
            .map(|(name, _)| name.clone());

        if let Some(name) = name {
            self.names.remove(&name);
        }

        self.new_net_code = self.new_net_code.min(removed.net_code.0 - 1);
        self.display_netnames_dirty = true;
        Some(removed)
    }

    /// Port of `NETINFO_LIST::RemoveUnusedNets( nullptr )`: drops every net whose
    /// `is_current` is false (the codes of the others stay).
    pub fn remove_unused_nets(&mut self) {
        let existing = std::mem::take(&mut self.nets);
        self.names.clear();

        for (code, net) in existing {
            if net.is_current {
                // std::map::insert keeps the first entry of a duplicate name
                self.names.entry(net.netname.clone()).or_insert(code);
                self.nets.insert(code, net);
            } else {
                self.display_netnames_dirty = true;
            }
        }
    }

    /// Port of `NETINFO_LIST::clear` (+ `detachAll`): no nets at all, `m_newNetCode = 0`.
    pub fn clear(&mut self) {
        self.names.clear();
        self.nets.clear();
        self.new_net_code = 0;
    }

    /// The list part of `NETINFO_LIST::buildListOfNets` without the board: every net is
    /// `clear()`ed (class `None` = board default) keeping its chain name and terminal pad
    /// UUIDs. `Board::build_list_of_nets` is the complete port (board default class,
    /// `SynchronizeNetsAndNetClasses( false )`, `SetAreasNetCodesFromNetNames()`).
    pub fn build_list_of_nets(&mut self) {
        self.reset_nets(None);
    }

    /// The loop of `NETINFO_LIST::buildListOfNets`: preserve the net-chain names and terminal
    /// pad UUIDs, `Clear()` every net, restore them (the terminal pad handles stay cleared
    /// until resolved again).
    pub(crate) fn reset_nets(&mut self, default: Option<&Arc<Netclass>>) {
        // Preserve any parsed net-chain names and terminal pad UUIDs before clearing. The GUI
        // load path calls BuildListOfNets() after the board file is parsed (see files.cpp)
        // which was unintentionally wiping the m_netChain field set by the (net_chains ...)
        // section.  Cache and restore them so persisted chains survive the rebuild.
        // (Keyed by the unique net code in C++, so a per-net save / restore is the same.)
        for net in self.nets.values_mut() {
            let chain = std::mem::take(&mut net.net_chain);
            let pads = net.terminal_pad_uuids;

            match default {
                Some(default) => net.clear_with_default(default),
                None => net.clear(),
            }

            net.net_chain = chain;
            net.terminal_pad_uuids = pads;
        }
    }

    /// Port of `NETINFO_LIST::getFreeNetCode`.
    pub fn get_free_net_code(&mut self) -> i32 {
        loop {
            if self.new_net_code < 0 {
                self.new_net_code = 0;
            }

            self.new_net_code += 1;

            if !self.nets.contains_key(&NetCode(self.new_net_code)) {
                break;
            }
        }

        self.new_net_code
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
        assert_eq!(
            net.netclass.as_ref().map(|c| c.name.as_str()),
            Some("Default")
        );
        assert_eq!(net.base.uuid, NILUUID);
        assert_eq!(
            NetinfoItem::with_name("GND", NetCode(1)).short_netname,
            "GND"
        );

        let mut generator = KiidGenerator::default();
        assert_ne!(
            NetinfoItem::new(&mut generator, "A", NetCode(1)).base.uuid,
            NILUUID
        );

        let list = NetinfoList::default();
        assert_eq!(list.net_count(), 1);
        assert_eq!(list.new_net_code, 0);
        assert!(list.display_netnames_dirty);
        assert_eq!(
            list.get_net_item_by_name("").map(|n| n.net_code),
            Some(NetCode::UNCONNECTED)
        );
    }

    /// `NETINFO_ITEM::SetNetname` (`pcbnew/netinfo_item.cpp:227`). Expected values from
    /// `pcb-dump board --sections board qa/data/pcbnew/issue11814.kicad_pcb`:
    /// `net.30.name="/OUT1+"`, `net.30.short_name="OUT1+"`.
    #[test]
    fn set_netname_and_auto_names() {
        let mut net = NetinfoItem::with_name("A", NetCode(1));
        net.set_netname("/OUT1+");
        assert_eq!(net.netname(), "/OUT1+");
        assert_eq!(net.short_netname(), "OUT1+");
        assert_eq!(net.display_netname(), "OUT1+");
        net.set_netname("/a/b/{slash}x");
        assert_eq!(net.short_netname(), "{slash}x");
        assert_eq!(net.display_netname(), "/x");
        net.set_netname("GND");
        assert_eq!(net.short_netname(), "GND");
        assert!(!net.has_auto_generated_netname());
        net.set_netname("/sub/Net-(R1-Pad2)");
        assert!(net.has_auto_generated_netname());
        net.set_netname("unconnected-(U1-NC-Pad3)");
        assert!(net.has_auto_generated_netname());
    }

    /// `NETINFO_LIST::AppendNet` / `getFreeNetCode` (`pcbnew/netinfo_list.cpp:154`, `:308`):
    /// a code equal to the list size is kept, any other (or -1) gets the next free code, a
    /// duplicate name is dropped (`settings.md` 4.10).
    #[test]
    fn append_net_code_assignment() {
        let mut list = NetinfoList::default();
        assert_eq!(
            list.get_net_item(NetCode(0)).and_then(|n| n.base.id),
            ItemId::net(NetCode(0))
        );

        assert_eq!(
            list.append_net(NetinfoItem::with_name("A", NetCode(1))),
            Some(NetCode(1))
        );
        assert_eq!(
            list.append_net(NetinfoItem::with_name("B", NetCode(2))),
            Some(NetCode(2))
        );
        // m_newNetCode is still 0: the free-code search starts at 1
        assert_eq!(
            list.append_net(NetinfoItem::with_name("C", NetCode(7))),
            Some(NetCode(3))
        );
        assert_eq!(list.new_net_code, 3);
        assert_eq!(
            list.append_net(NetinfoItem::with_name("D", NetCode::ORPHANED)),
            Some(NetCode(4))
        );
        assert_eq!(
            list.append_net(NetinfoItem::with_name("A", NetCode::ORPHANED)),
            None
        );
        assert_eq!(list.net_count(), 5);

        let by_name: Vec<&str> = list.nets_by_name().map(|n| n.netname()).collect();
        assert_eq!(by_name, ["", "A", "B", "C", "D"]);
        let n = list.get_net_item_by_name("C").unwrap();
        assert_eq!(n.net_code(), NetCode(3));
        assert_eq!(n.base.id, ItemId::net(NetCode(3)));

        // RemoveNet: m_newNetCode = min( m_newNetCode, code - 1 ), so the code is reused
        list.remove_net(NetCode(2));
        assert_eq!(list.new_net_code, 1);
        assert!(list.get_net_item_by_name("B").is_none());
        assert_eq!(
            list.append_net(NetinfoItem::with_name("E", NetCode::ORPHANED)),
            Some(NetCode(2))
        );
        assert_eq!(
            list.append_net(NetinfoItem::with_name("F", NetCode(5))),
            Some(NetCode(5))
        );
        list.remove_net(NetCode(42));
        assert_eq!(list.net_count(), 6);
    }

    /// `NETINFO_LIST::RemoveUnusedNets`, `clear`, `buildListOfNets` list part
    /// (`pcbnew/netinfo_list.cpp:56`, `:128`, `:187`).
    #[test]
    fn remove_unused_clear_and_rebuild() {
        let mut list = NetinfoList::default();
        list.append_net(NetinfoItem::with_name("A", NetCode(1)));
        list.append_net(NetinfoItem::with_name("B", NetCode(2)));
        list.append_net(NetinfoItem::with_name("C", NetCode(3)));
        list.get_net_item_mut(NetCode(2))
            .unwrap()
            .set_is_current(false);
        list.display_netnames_dirty = false;
        list.remove_unused_nets();
        assert!(list.display_netnames_dirty);
        let codes: Vec<i32> = list.nets_by_netcode().map(|n| n.net_code().0).collect();
        assert_eq!(codes, [0, 1, 3]);
        assert!(list.get_net_item_by_name("B").is_none());

        let net = list.get_net_item_mut(NetCode(3)).unwrap();
        net.set_net_chain("CHAIN");
        let uuid = Kiid::new(&mut KiidGenerator::default());
        net.set_terminal(0, Some((PadId::from_parts(4, 0), uuid)));
        net.set_netclass(Some(Arc::new(Netclass::new("HV", true))));
        list.build_list_of_nets();
        let net = list.get_net_item(NetCode(3)).unwrap();
        assert_eq!(net.net_chain(), "CHAIN");
        assert_eq!(net.terminal_pad(0), None);
        assert_eq!(*net.terminal_pad_uuid(0), uuid);
        assert!(net.netclass().is_none());

        let default = Arc::new(Netclass::new("Default", true));
        list.reset_nets(Some(&default));
        assert!(Arc::ptr_eq(
            list.get_net_item(NetCode(3)).unwrap().netclass().unwrap(),
            &default
        ));

        let mut net = NetinfoItem::with_name("X", NetCode(1));
        net.set_netclass_or_default(None, &default);
        assert!(Arc::ptr_eq(net.netclass().unwrap(), &default));
        net.set_net_chain("C");
        net.clear_with_default(&default);
        assert_eq!(net.net_chain(), "");

        list.clear();
        assert_eq!(list.net_count(), 0);
        assert_eq!(list.new_net_code, 0);
        assert_eq!(
            NetinfoList::orphaned_item().net_code(),
            NetCode::UNCONNECTED
        );
    }
}
