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

//! Port of the net functions of `pcbnew/board.cpp` (`FindNet`, `FindOrAddNet`, `Add` /
//! `Remove` of a `NETINFO_ITEM`, `BuildListOfNets`, `SynchronizeNetsAndNetClasses`,
//! `ApplyNetChainNetclasses`, `SetAreasNetCodesFromNetNames`, `MapNets`, `SanitizeNetcodes`,
//! `MatchDpSuffix`, `DpCoupledNet`, `GetNodesCount`, `TracksInNet`, ...). Package M3; M0a / M0d
//! wrote the signatures and the plain lookups.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::{ItemId, NetCode};
use crate::item::ParentRef;
use crate::net_settings::NetChainSource;
use crate::netinfo::{NetinfoItem, NetinfoList};
use crate::traits::BoardConnectedItem;

impl Board {
    /// `BOARD::GetNetInfo()`.
    pub fn net_info(&self) -> &NetinfoList {
        &self.net_info
    }

    /// `for( NETINFO_ITEM* net : GetNetInfo() )`: by name.
    pub fn nets(&self) -> impl Iterator<Item = &NetinfoItem> + '_ {
        self.net_info.nets_by_name()
    }

    /// `NETINFO_LIST::NetsByNetcode()`: by code.
    pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem> + '_ {
        self.net_info.nets_by_netcode()
    }

    /// `BOARD::GetNetCount()`.
    pub fn net_count(&self) -> usize {
        self.net_info.net_count()
    }

    /// Port of `BOARD::FindNet( aNetcode )`.
    pub fn find_net(&self, code: NetCode) -> Option<&NetinfoItem> {
        // the first valid netcode is 1 and the last is m_NetInfo.GetCount()-1.
        // zero is reserved for "no connection" and is not actually a net.
        // nullptr is returned for non valid netcodes

        if code == NetCode::UNCONNECTED && self.net_info.net_count() == 0 {
            Some(NetinfoList::orphaned_item())
        } else {
            self.net_info.get_net_item(code)
        }
    }

    /// Port of `BOARD::FindNet( aNetname )`.
    pub fn find_net_by_name(&self, name: &str) -> Option<&NetinfoItem> {
        self.net_info.get_net_item_by_name(name)
    }

    /// Port of `BOARD::Add` for a `NETINFO_ITEM` (`NETINFO_LIST::AppendNet`, then parent,
    /// UUID index, time stamp): `None` when a net of that name exists (the new one is dropped,
    /// as C++ deletes it).
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<NetCode> {
        // A duplicate net is deleted here, so nothing below may touch it
        let code = self.net_info.append_net(net)?;
        let id = ItemId::net(code);

        let uuid = match self.net_info.get_net_item_mut(code) {
            Some(net) => {
                net.base.parent = ParentRef::Board;
                net.base.flags = Default::default(); // ClearEditFlags (a net carries no others)
                net.base.uuid
            }
            None => return Some(code),
        };

        if let Some(id) = id {
            self.bind_uuid(uuid, id);
        }

        self.increment_time_stamp();
        Some(code)
    }

    /// Port of `BOARD::FindOrAddNet( aNetName, aNetCode )`, the parser's net creation: the
    /// existing net of that name, else `new NETINFO_ITEM( this, aNetName, aNetCode )` (fresh
    /// UUID, the board's default class; `None` = -1, the next free code) added to the board.
    pub fn find_or_add_net(&mut self, name: &str, code: Option<NetCode>) -> NetCode {
        if let Some(existing) = self.find_net_by_name(name) {
            return existing.net_code;
        }

        let mut net = NetinfoItem::new(
            &mut self.kiid_generator,
            name,
            code.unwrap_or(NetCode::ORPHANED),
        );
        net.netclass = Some(Arc::clone(
            self.design_settings.net_settings.default_netclass(),
        ));

        // AppendNet cannot refuse it: no net of that name exists.
        self.add_net(net).unwrap_or(NetCode::ORPHANED)
    }

    /// Port of `BOARD::Remove( NETINFO_ITEM )`: the UUID is unbound, items of the net fall
    /// back to net 0 (`SetNet( unconnected )`, no copper check), the net is removed from the
    /// list, time stamp. A code that is not on the board changes nothing.
    pub fn remove_net(&mut self, code: NetCode) {
        let Some(uuid) = self.net_info.get_net_item(code).map(|n| n.base.uuid) else {
            return;
        };

        if let Some(id) = ItemId::net(code) {
            self.unbind_net_uuid(uuid, id);
        }

        let affected: Vec<ItemId> = self
            .all_connected_items()
            .into_iter()
            .filter(|&id| {
                self.get(id)
                    .and_then(|item| item.as_connected())
                    .is_some_and(|c| c.net_code() == code)
            })
            .collect();

        for id in affected {
            self.set_item_net_direct(id, NetCode::UNCONNECTED);
        }

        self.net_info.remove_net(code);
        self.increment_time_stamp();
    }

    /// Port of `BOARD::RemoveUnusedNets( nullptr )` (`NETINFO_LIST::RemoveUnusedNets`). The
    /// UUIDs of the dropped nets are unbound (C++ leaves them in the item index, dangling).
    pub fn remove_unused_nets(&mut self) {
        let unused: Vec<(NetCode, kicad_core::Kiid)> = self
            .net_info
            .nets_by_netcode()
            .filter(|net| !net.is_current)
            .map(|net| (net.net_code, net.base.uuid))
            .collect();

        self.net_info.remove_unused_nets();

        for (code, uuid) in unused {
            if let Some(id) = ItemId::net(code) {
                self.unbind_net_uuid(uuid, id);
            }
        }
    }

    /// Port of `BOARD::BuildListOfNets` (`NETINFO_LIST::buildListOfNets`): every net is
    /// cleared to the board's default class keeping its chain name and terminal pad UUIDs,
    /// then `SynchronizeNetsAndNetClasses( false )` and `SetAreasNetCodesFromNetNames()`.
    pub fn build_list_of_nets(&mut self) {
        let default = Arc::clone(self.design_settings.net_settings.default_netclass());
        self.net_info.reset_nets(Some(&default));

        self.synchronize_nets_and_netclasses(false);
        self.set_areas_net_codes_from_net_names();
    }

    /// Port of `BOARD::SanitizeNetcodes`: items whose net code is not on the board become
    /// orphaned (`SetNetCode( NETINFO_LIST::ORPHANED )`).
    pub fn sanitize_netcodes(&mut self) {
        let orphans: Vec<ItemId> = self
            .all_connected_items()
            .into_iter()
            .filter(|&id| {
                self.get(id)
                    .and_then(|item| item.as_connected())
                    .is_some_and(|c| self.find_net(c.net_code()).is_none())
            })
            .collect();

        for id in orphans {
            self.set_item_net(id, NetCode::ORPHANED, false);
        }
    }

    /// Port of `BOARD::MapNets( aDestBoard )`: every connected item of this board is given
    /// the net of `dest` with its net name (`SetNet`, no copper check), creating missing nets
    /// in `dest` (`new NETINFO_ITEM( aDestBoard, name )`, code -1). As in C++ the items stay
    /// here and afterwards carry `dest`'s net codes.
    pub fn map_nets(&mut self, dest: &mut Board) {
        for id in self.all_connected_items() {
            let netname = {
                let ctx = BoardCtx::bare(self);
                match ctx.item(id) {
                    Some(item) => item.netname().to_string(),
                    None => continue,
                }
            };

            let code = match dest.find_net_by_name(&netname) {
                Some(net_info) => net_info.net_code,
                None => {
                    let mut new_net =
                        NetinfoItem::new(&mut dest.kiid_generator, &netname, NetCode::ORPHANED);
                    new_net.netclass = Some(Arc::clone(
                        dest.design_settings.net_settings.default_netclass(),
                    ));
                    dest.add_net(new_net).unwrap_or(NetCode::ORPHANED)
                }
            };

            self.set_item_net_direct(id, code);
        }
    }

    /// Port of `BOARD::MatchDpSuffix`: (+1 for a `+` / `P` suffix, -1 for `-` / `N`, 0 for
    /// none; trailing digits and `_` are skipped), and the complementary net name (empty when
    /// the result is 0).
    pub fn match_dp_suffix(net_name: &str) -> (i32, String) {
        let chars: Vec<char> = net_name.chars().collect();
        let mut complement = String::new();
        let mut rv = 0;
        let mut count = 0usize;

        for &ch in chars.iter().rev() {
            if rv != 0 {
                break;
            }

            if ch.is_ascii_digit() || ch == '_' {
                // continue
            } else if ch == '+' {
                complement = "-".to_string();
                rv = 1;
            } else if ch == '-' {
                complement = "+".to_string();
                rv = -1;
            } else if ch == 'N' {
                complement = "P".to_string();
                rv = -1;
            } else if ch == 'P' {
                complement = "N".to_string();
                rv = 1;
            } else {
                break;
            }

            count += 1;
        }

        if rv != 0 && count >= 1 {
            let left: String = chars[..chars.len() - count].iter().collect();
            let right: String = chars[chars.len() - (count - 1)..].iter().collect();
            complement = left + &complement + &right;
        }

        (rv, complement)
    }

    /// Port of `BOARD::DpCoupledNet`: the net whose name has the complementary
    /// differential-pair suffix, if it exists.
    pub fn dp_coupled_net(&self, net: NetCode) -> Option<NetCode> {
        let net = self.find_net(net)?;
        let (rv, coupled_net_name) = Board::match_dp_suffix(&net.netname);

        if rv != 0 {
            return self.find_net_by_name(&coupled_net_name).map(|n| n.net_code);
        }

        None
    }

    /// Port of `BOARD::GetNodesCount( aNet )`: the pads in `net`, or (`None`, C++ -1) all
    /// pads with a net code > 0. (The C++ body counts footprint pads; `ctx` is not needed.)
    pub fn get_nodes_count(&self, _ctx: &BoardCtx<'_>, net: Option<NetCode>) -> u32 {
        let a_net = net.unwrap_or(NetCode::ORPHANED);
        let mut retval = 0u32;

        for (fp, _) in self.footprints() {
            for (_, pad) in self.footprint_pads(fp) {
                let code = pad.net_code();

                if (a_net == NetCode::ORPHANED && code.0 > 0) || a_net == code {
                    retval += 1;
                }
            }
        }

        retval
    }

    /// Port of `BOARD::SetAreasNetCodesFromNetNames`: zones off copper get net 0; a zone with
    /// a net code that is not on the board gets -1 and counts as an error. Returns the error
    /// count.
    pub fn set_areas_net_codes_from_net_names(&mut self) -> i32 {
        let mut error_count = 0;
        let mut updates: Vec<(ItemId, NetCode)> = Vec::new();

        {
            let ctx = BoardCtx::bare(self);

            for (zid, zone) in self.zones() {
                if !zone.is_on_copper_layer(&ctx) {
                    updates.push((zid.into(), NetCode::UNCONNECTED));
                    continue;
                }

                // i.e. if this zone is connected to a net
                if zone.net_code() != NetCode::UNCONNECTED {
                    match self.find_net(zone.net_code()) {
                        Some(net) => updates.push((zid.into(), net.net_code)),
                        None => {
                            error_count += 1;

                            // keep Net Name and set m_NetCode to -1 : error flag.
                            updates.push((zid.into(), NetCode::ORPHANED));
                        }
                    }
                }
            }
        }

        for (id, code) in updates {
            self.set_item_net(id, code, false);
        }

        error_count
    }

    /// Port of `BOARD::SynchronizeNetsAndNetClasses( aResetTrackAndViaSizes )`; a no-op
    /// without a project. Stores `GetEffectiveNetClass( name )` into every net (name order);
    /// with `reset_track_and_via_sizes` the custom track / via / diff-pair sizes are reset to
    /// the default class values. Bumps the time stamp (C++ notifies the listeners).
    pub fn synchronize_nets_and_netclasses(&mut self, reset_track_and_via_sizes: bool) {
        if self.project.is_none() {
            return;
        }

        let default_net_class = Arc::clone(self.design_settings.net_settings.default_netclass());

        self.apply_net_chain_netclasses();

        self.design_settings.net_settings.clear_all_caches();

        let nets: Vec<(String, NetCode)> = self
            .net_info
            .names
            .iter()
            .map(|(name, &code)| (name.clone(), code))
            .collect();

        for (name, code) in nets {
            let netclass = self
                .design_settings
                .net_settings
                .get_effective_netclass(&name);

            if let Some(net) = self.net_info.get_net_item_mut(code) {
                net.set_netclass_or_default(Some(netclass), &default_net_class);
            }
        }

        if reset_track_and_via_sizes {
            let bds = &mut self.design_settings;

            // Set initial values for custom track width & via size to match the default
            // netclass settings
            bds.set_use_custom_track_via_size(false);
            bds.custom_track_width = default_net_class.track_width();
            bds.custom_via_size.diameter = default_net_class.via_diameter();
            bds.custom_via_size.drill = default_net_class.via_drill();

            if default_net_class.has_diff_pair_width() {
                bds.custom_diff_pair.width = default_net_class.diff_pair_width();
            } else {
                bds.custom_diff_pair.width = default_net_class.track_width();
            }

            if default_net_class.has_diff_pair_gap() {
                bds.custom_diff_pair.gap = default_net_class.diff_pair_gap();
            } else {
                bds.custom_diff_pair.gap = default_net_class.clearance();
            }

            if default_net_class.has_diff_pair_via_gap() {
                bds.custom_diff_pair.via_gap = default_net_class.diff_pair_via_gap();
            } else {
                bds.custom_diff_pair.via_gap = bds.custom_diff_pair.gap;
            }
        }

        // InvokeListeners( OnBoardNetSettingsChanged ): consumers revalidate by time stamp.
        self.increment_time_stamp();
    }

    /// Port of `BOARD::ApplyNetChainNetclasses`: derives the board's chain pattern
    /// assignments (net name -> class of its chain) from `m_netChainNetClasses`. A no-op
    /// without a project, or without chain classes and earlier board chain assignments.
    pub fn apply_net_chain_netclasses(&mut self) {
        if self.project.is_none() {
            return;
        }

        let net_settings = &self.design_settings.net_settings;
        let chain_netclasses = &net_settings.net_chain_net_classes;

        // Nothing to derive and nothing left from a prior pass, so skip the cache wipe a
        // rebuild would cost on every resync of a board without chain overrides.
        if chain_netclasses.is_empty()
            && !net_settings.has_chain_pattern_assignments(NetChainSource::Board)
        {
            return;
        }

        // The loop over m_NetInfo (name order) only reads the nets and HasNetclass(), which
        // ClearChainPatternAssignments() does not change: collect first, then mutate.
        let assignments: Vec<(String, String)> = self
            .nets()
            .filter_map(|net| {
                let chain_name = &net.net_chain;

                if chain_name.is_empty() {
                    return None;
                }

                let class = chain_netclasses.get(chain_name)?;

                if !net_settings.has_netclass(class) {
                    return None;
                }

                Some((net.netname.clone(), class.clone()))
            })
            .collect();

        let net_settings = &mut self.design_settings.net_settings;
        net_settings.clear_chain_pattern_assignments(NetChainSource::Board);

        for (net_name, class) in &assignments {
            net_settings.set_chain_pattern_assignment(NetChainSource::Board, net_name, class);
        }
    }

    /// The terminal-pad resolution loop at the end of the parser
    /// (`NETINFO_ITEM::ResolveTerminalPads( m_board )` for every net, name order): each
    /// unresolved terminal pad is looked up by its UUID (`BOARD::FindPadByUuid`). The parser's
    /// UUID remapping before it belongs to the parser.
    pub fn resolve_net_chain_terminal_pads(&mut self) {
        let codes: Vec<NetCode> = self.nets().map(|net| net.net_code).collect();

        for code in codes {
            for i in 0..2 {
                let Some(net) = self.net_info.get_net_item(code) else {
                    continue;
                };

                if net.terminal_pads[i].is_some() {
                    continue;
                }

                let uuid = net.terminal_pad_uuids[i];
                let pad = self.find_pad_by_uuid(&uuid);

                if let Some(net) = self.net_info.get_net_item_mut(code) {
                    net.terminal_pads[i] = pad;
                }
            }
        }
    }

    /// Port of `BOARD::GetNetClassAssignmentCandidates`: the non-empty net names.
    pub fn get_net_class_assignment_candidates(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();

        for net in self.nets() {
            if !net.netname.is_empty() {
                names.insert(net.netname.clone());
            }
        }

        names
    }

    /// Port of `BOARD::TracksInNet`: tracks, arcs and vias of `net` in `m_tracks` order (the
    /// `Visit` with `GENERAL_COLLECTOR::Tracks`).
    pub fn tracks_in_net(&self, net: NetCode) -> Vec<crate::ids::TrackId> {
        self.tracks()
            .filter(|(_, track)| track.net_code() == net)
            .map(|(id, _)| id)
            .collect()
    }

    /// `BOARD_CONNECTED_ITEM::SetNet` on a stored item (virtual: the `ZONE` override
    /// applies): the net code is written without the copper check of `SetNetCode`.
    fn set_item_net_direct(&mut self, id: ItemId, code: NetCode) {
        self.set_item_netinfo(id, code);
    }

    /// `m_itemIndex.Unbind( net->m_Uuid )` when the index entry names this net.
    fn unbind_net_uuid(&mut self, uuid: kicad_core::Kiid, id: ItemId) {
        if self.uuid_index.get(&uuid) == Some(&id) {
            self.uuid_index.remove(&uuid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netclass::Netclass;

    /// `BOARD::MatchDpSuffix` (`pcbnew/board.cpp:2835`). The `/OUT1+` / `/OUT1-` pair is
    /// `net.30.name` / `net.31.name` of `pcb-dump board --sections board
    /// qa/data/pcbnew/issue11814.kicad_pcb`.
    #[test]
    fn match_dp_suffix() {
        let m = |s: &str| Board::match_dp_suffix(s);
        assert_eq!(m("/OUT1+"), (1, "/OUT1-".to_string()));
        assert_eq!(m("/OUT1-"), (-1, "/OUT1+".to_string()));
        assert_eq!(m("CLK_N"), (-1, "CLK_P".to_string()));
        assert_eq!(m("USB_DP"), (1, "USB_DN".to_string()));
        assert_eq!(m("D+1"), (1, "D-1".to_string()));
        assert_eq!(m("LANE_P_12"), (1, "LANE_N_12".to_string()));
        assert_eq!(m("P"), (1, "N".to_string()));
        assert_eq!(m("GND"), (0, String::new()));
        assert_eq!(m("12_3"), (0, String::new()));
        assert_eq!(m(""), (0, String::new()));
        assert_eq!(m("Ü+"), (1, "Ü-".to_string()));
    }

    /// `BOARD::FindNet`, `FindOrAddNet`, `Add( NETINFO_ITEM )`, `DpCoupledNet`,
    /// `GetNetClassAssignmentCandidates`, `GetNodesCount`, `BuildListOfNets` without a project
    /// (`pcbnew/board.cpp:2805-2895`, `:2954`, `:2527`, `board.h:1180`).
    #[test]
    fn find_add_and_couple() {
        let mut board = Board::default();
        assert_eq!(board.net_count(), 1);
        assert_eq!(board.find_net(NetCode(0)).map(|n| n.netname()), Some(""));
        assert!(board.find_net(NetCode(1)).is_none());

        assert_eq!(
            board.find_or_add_net("/OUT1+", Some(NetCode(1))),
            NetCode(1)
        );
        assert_eq!(board.find_or_add_net("/OUT1-", None), NetCode(2));
        assert_eq!(
            board.find_or_add_net("/OUT1+", Some(NetCode(9))),
            NetCode(1)
        );
        assert_eq!(board.find_or_add_net("GND", Some(NetCode(9))), NetCode(3));

        let net = board.find_net(NetCode(2)).unwrap();
        assert_eq!(net.short_netname(), "OUT1-");
        assert_eq!(net.base.parent, ParentRef::Board);
        assert_ne!(net.base.uuid, kicad_core::NILUUID);
        assert!(Arc::ptr_eq(
            net.netclass().unwrap(),
            board.design_settings().net_settings.default_netclass()
        ));
        assert_eq!(board.resolve_item(&net.base.uuid), ItemId::net(NetCode(2)));
        assert_eq!(
            board.find_net_by_name("GND").map(|n| n.net_code()),
            Some(NetCode(3))
        );

        assert_eq!(
            board.add_net(NetinfoItem::with_name("GND", NetCode(4))),
            None
        );
        assert_eq!(
            board.add_net(NetinfoItem::with_name("VCC", NetCode(4))),
            Some(NetCode(4))
        );

        assert_eq!(board.dp_coupled_net(NetCode(1)), Some(NetCode(2)));
        assert_eq!(board.dp_coupled_net(NetCode(2)), Some(NetCode(1)));
        assert_eq!(board.dp_coupled_net(NetCode(3)), None);
        assert_eq!(board.dp_coupled_net(NetCode(17)), None);

        let candidates: Vec<String> = board
            .get_net_class_assignment_candidates()
            .into_iter()
            .collect();
        assert_eq!(candidates, ["/OUT1+", "/OUT1-", "GND", "VCC"]);
        assert_eq!(board.get_nodes_count(&BoardCtx::bare(&board), None), 0);
        assert!(board.tracks_in_net(NetCode(1)).is_empty());

        // No project: SynchronizeNetsAndNetClasses is a no-op, the classes become the default.
        board
            .net_info
            .get_net_item_mut(NetCode(3))
            .unwrap()
            .set_netclass(Some(Arc::new(Netclass::new("HV", true))));
        board.build_list_of_nets();
        assert_eq!(
            board
                .find_net(NetCode(3))
                .and_then(|n| n.netclass())
                .map(|c| c.name.as_str()),
            Some("Default")
        );

        board.check_invariants().unwrap();

        // FindNet( 0 ) of an empty list: the orphaned item
        board.net_info.clear();
        assert!(std::ptr::eq(
            board.find_net(NetCode(0)).unwrap(),
            NetinfoList::orphaned_item()
        ));
    }
}
