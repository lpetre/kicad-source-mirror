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

//! Port of the net functions of `pcbnew/board.cpp` (`FindNet`, `GetNetInfo`, `NetsByNetcode`,
//! `SynchronizeNetsAndNetClasses`, ...). Package M3; M0a / M0d wrote the signatures and the
//! plain lookups.

use std::collections::BTreeSet;

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::NetCode;
use crate::netinfo::{NetinfoItem, NetinfoList};

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

    /// `BOARD::FindNet( aNetcode )`.
    pub fn find_net(&self, code: NetCode) -> Option<&NetinfoItem> {
        crate::port_pending!("BOARD::FindNet", code)
    }

    /// `BOARD::FindNet( aNetname )`.
    pub fn find_net_by_name(&self, name: &str) -> Option<&NetinfoItem> {
        crate::port_pending!("BOARD::FindNet (by name)", name)
    }

    /// `NETINFO_LIST::AppendNet` through `BOARD::Add( NETINFO_ITEM )`: `None` when a net of
    /// that name exists (the new one is dropped, as C++ deletes it).
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<NetCode> {
        crate::port_pending!("BOARD::Add (net)", net)
    }

    /// The parser's net creation: the existing net of that name, else a new one with `code`
    /// (or the next free code), in first-use order.
    pub fn find_or_add_net(&mut self, name: &str, code: Option<NetCode>) -> NetCode {
        crate::port_pending!("BOARD::FindOrAddNet (parser)", name, code)
    }

    /// `BOARD::Remove( NETINFO_ITEM )`: items of the net fall back to net 0.
    pub fn remove_net(&mut self, code: NetCode) {
        crate::port_pending!("BOARD::Remove (net)", code)
    }

    /// `BOARD::RemoveUnusedNets`.
    pub fn remove_unused_nets(&mut self) {
        crate::port_pending!("BOARD::RemoveUnusedNets")
    }

    /// `BOARD::BuildListOfNets` (`NETINFO_LIST::buildListOfNets`).
    pub fn build_list_of_nets(&mut self) {
        crate::port_pending!("BOARD::BuildListOfNets")
    }

    /// `BOARD::SanitizeNetcodes`.
    pub fn sanitize_netcodes(&mut self) {
        crate::port_pending!("BOARD::SanitizeNetcodes")
    }

    /// `BOARD::MapNets`.
    pub fn map_nets(&mut self, dest: &mut Board) {
        crate::port_pending!("BOARD::MapNets", dest.time_stamp())
    }

    /// `BOARD::MatchDpSuffix`: (+1 / -1 / 0, the complementary net name).
    pub fn match_dp_suffix(net_name: &str) -> (i32, String) {
        crate::port_pending!("BOARD::MatchDpSuffix", net_name)
    }

    /// `BOARD::DpCoupledNet`.
    pub fn dp_coupled_net(&self, net: NetCode) -> Option<NetCode> {
        crate::port_pending!("BOARD::DpCoupledNet", net)
    }

    /// `BOARD::GetNodesCount( aNet )` (asks `ctx.connectivity`).
    pub fn get_nodes_count(&self, ctx: &BoardCtx<'_>, net: Option<NetCode>) -> u32 {
        crate::port_pending!("BOARD::GetNodesCount", ctx.board.time_stamp(), net)
    }

    /// `BOARD::SetAreasNetCodesFromNetNames`.
    pub fn set_areas_net_codes_from_net_names(&mut self) -> i32 {
        crate::port_pending!("BOARD::SetAreasNetCodesFromNetNames")
    }

    /// `BOARD::SynchronizeNetsAndNetClasses( aResetTrackAndViaSizes )`; a no-op without a
    /// project.
    pub fn synchronize_nets_and_netclasses(&mut self, reset_track_and_via_sizes: bool) {
        crate::port_pending!(
            "BOARD::SynchronizeNetsAndNetClasses",
            reset_track_and_via_sizes
        )
    }

    /// `BOARD::ApplyNetChainNetclasses`.
    pub fn apply_net_chain_netclasses(&mut self) {
        crate::port_pending!("BOARD::ApplyNetChainNetclasses")
    }

    /// `BOARD::ResolveNetChainTerminalPads`.
    pub fn resolve_net_chain_terminal_pads(&mut self) {
        crate::port_pending!("BOARD::ResolveNetChainTerminalPads")
    }

    /// `BOARD::GetNetClassAssignmentCandidates`.
    pub fn get_net_class_assignment_candidates(&self) -> BTreeSet<String> {
        crate::port_pending!("BOARD::GetNetClassAssignmentCandidates")
    }

    /// `BOARD::TracksInNet`.
    pub fn tracks_in_net(&self, net: NetCode) -> Vec<crate::ids::TrackId> {
        crate::port_pending!("BOARD::TracksInNet", net)
    }
}
