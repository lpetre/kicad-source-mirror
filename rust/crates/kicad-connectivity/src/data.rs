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

//! Port of `pcbnew/connectivity/connectivity_data.h` and `connectivity_data.cpp`
//! (`CONNECTIVITY_DATA`, the facade and its queries; implements
//! `kicad_pcb::ctx::ConnectivityQuery`), plus `BOARD::BuildConnectivity`,
//! `BOARD::CacheTriangulation` (fills) and `BOARD::UpdateRatsnestExclusions` (`board.cpp`),
//! which only drive this facade.
//!
//! Board items are named by [`ItemId`] (C++ `BOARD_CONNECTED_ITEM*`). Where the C++ orders
//! board items by heap address (`std::set<T*>`, `std::sort` of pointers) the port orders by
//! `ItemId` (board-model.md 3.1 rule 2: file order within one kind); see `PORT_STATUS.md`.
//!
//! The facade never mutates the board itself: net propagation (`SetNetCode`) is recorded by
//! the algorithm and applied by [`ConnectivityData::build_board`] (or by the caller of
//! [`ConnectivityData::build`] through [`ConnectivityData::take_net_changes`]),
//! board-model.md 13.
//!
//! Not ported (tier 3, CN-06): the local / dynamic ratsnest (`Build( global, localItems )`,
//! `ComputeLocalRatsnest`, `ClearLocalRatsnest`, `HideLocalRatsnest`, `GetLocalRatsnest`,
//! `BlockRatsnestItems`, `Move`), the progress reporter and the spin lock, the net settings
//! weak pointer (ratsnest drawing only) and the from-to cache (CN-05, `from_to`).

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::layer_ids::{UNDEFINED_LAYER, copper_layer_to_ordinal, to_layer_id};
use kicad_core::{Flashing, KicadT, Kiid, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::math::util::ki_round;
use kicad_geom::{Seg, Shape, ShapeCircle, Vector2I};
use kicad_pcb::Board;
use kicad_pcb::ctx::{BoardCtx, ConnectivityQuery};
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::ids::{ItemId, NetCode, ZoneId};
use kicad_pcb::item::ItemFlags;
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::padstack::UnconnectedLayerMode;
use kicad_pcb::pcb_marker::MarkerT;

use crate::algo::{
    ClusterSearchMode, CnConnectivityAlgo, IsolatedIslandsMap, items_touch_on_layer,
};
use crate::edge::CnEdge;
use crate::items::CnList;
use crate::ratsnest::RnNet;

/// `IGNORE_NETS` flag of [`ConnectivityData::connected_items`].
pub const IGNORE_NETS: i32 = 0x0001;

/// `EXCLUDE_ZONES` flag of [`ConnectivityData::connected_items`].
pub const EXCLUDE_ZONES: i32 = 0x0002;

/// The `board->GetCopperZoneRTree( zone )` lookup of `TestTrackEndpointDangling`
/// (`BOARD::m_CopperZoneRTreeCache`, filled by the DRC cache generator in `kicad-drc`, which
/// depends on this crate).
pub trait CopperZoneRtrees {
    /// `rtree->QueryColliding( aShape.BBox(), &aShape, aLayer )` on the copper zone tree of
    /// `zone`; `None` when the board has no tree for that zone (C++ `nullptr`).
    fn query_colliding(&self, zone: ZoneId, shape: &ShapeCircle, layer: PcbLayerId)
    -> Option<bool>;
}

/// One unconnected edge of the ratsnest with the items it joins: the data an external client
/// (autorouter, DRC unconnected-items test) reads through `RunOnUnconnectedEdges`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnconnectedEdge {
    /// Net code (index of the `RN_NET`).
    pub net: i32,
    /// `GetSourceNode()->Pos()`
    pub source: Vector2I,
    /// `GetTargetNode()->Pos()`
    pub target: Vector2I,
    /// `GetSourceNode()->Parent()`
    pub source_item: ItemId,
    /// `GetTargetNode()->Parent()`
    pub target_item: ItemId,
    /// `GetWeight()`
    pub weight: u32,
    /// `IsVisible()` (false when excluded by a ratsnest marker)
    pub visible: bool,
}

/// Port of `CONNECTIVITY_DATA`: the connectivity graph of a board (`CN_CONNECTIVITY_ALGO`)
/// and the ratsnest of every net (`RN_NET`).
#[derive(Default)]
pub struct ConnectivityData {
    /// `m_connAlgo`
    conn_algo: CnConnectivityAlgo,
    /// `m_nets` (index = net code; never null)
    nets: Vec<RnNet>,
    /// `m_skipRatsnestUpdate`
    skip_ratsnest_update: bool,
    /// `m_netcodeMap`
    netcode_map: BTreeMap<i32, String>,
    /// `board.time_stamp()` after the last build (board-model.md 13); `None` before.
    time_stamp: Option<i32>,
}

impl ConnectivityData {
    /// Port of `CONNECTIVITY_DATA()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `m_skipRatsnestUpdate` (the C++ constructor argument of the local connectivity).
    pub fn set_skip_ratsnest_update(&mut self, skip: bool) {
        self.skip_ratsnest_update = skip;
    }

    // ---------------------------------------------------------------------------------------
    // Build
    // ---------------------------------------------------------------------------------------

    /// Port of `CONNECTIVITY_DATA::Build( BOARD* )` without its first statement
    /// (`aBoard->CacheTriangulation()`, which mutates the board: see
    /// [`ConnectivityData::build_board`]). The fills must already be triangulated.
    ///
    /// `ctx.connectivity` must be `None` (board-model.md 6.1: the C++ algorithm never calls
    /// `FlashLayer`). Net propagation is recorded, not applied: the caller applies
    /// [`ConnectivityData::take_net_changes`] to the board. Always returns true (the C++
    /// spin lock cannot be contended in the sequential port).
    pub fn build(&mut self, ctx: &BoardCtx<'_>) -> bool {
        self.nets.clear();

        self.conn_algo = CnConnectivityAlgo::new();
        self.conn_algo.build(ctx);

        // m_netSettings = aBoard->GetDesignSettings().m_NetSettings: ratsnest drawing only.

        self.refresh_netcode_map(ctx.board);

        self.internal_recalculate_ratsnest(ctx);

        self.time_stamp = Some(ctx.board.time_stamp());
        true
    }

    /// `CONNECTIVITY_DATA::Build( aBoard )` as the C++ runs it on a board it owns:
    /// `BOARD::CacheTriangulation` (fills), [`ConnectivityData::build`], then the recorded
    /// `SetNetCode` calls applied to the board in call order, and the board time stamp
    /// re-recorded (propagation does not change the topology, board-model.md 13).
    pub fn build_board(&mut self, board: &mut Board) -> bool {
        cache_board_triangulation(board);

        let ok = self.build(&BoardCtx::bare(board));
        self.apply_net_changes(board);
        ok
    }

    /// Port of `BOARD::BuildConnectivity()`: [`ConnectivityData::build_board`], then
    /// `UpdateRatsnestExclusions()`.
    pub fn build_connectivity(&mut self, board: &mut Board) -> bool {
        if !self.build_board(board) {
            return false;
        }

        self.update_ratsnest_exclusions(board);
        true
    }

    /// Applies the recorded `Parent()->SetNetCode( net )` calls to the board
    /// (`Board::set_item_net( id, net, false )`) and re-records the time stamp.
    pub fn apply_net_changes(&mut self, board: &mut Board) {
        for (id, net) in self.conn_algo.take_net_changes() {
            board.set_item_net(id, net, false);
        }

        if self.time_stamp.is_some() {
            self.time_stamp = Some(board.time_stamp());
        }
    }

    /// The `SetNetCode` calls of the last build / propagation not yet applied, in call order
    /// (see [`CnConnectivityAlgo::take_net_changes`]).
    pub fn take_net_changes(&mut self) -> Vec<(ItemId, NetCode)> {
        self.conn_algo.take_net_changes()
    }

    /// The board time stamp recorded by the last build; `None` before the first build.
    pub fn time_stamp(&self) -> Option<i32> {
        self.time_stamp
    }

    /// True when the board changed since the last build (or nothing was built yet): tier 1/2
    /// callers rebuild in full then (board-model.md 8.5).
    pub fn is_stale(&self, board: &Board) -> bool {
        self.time_stamp != Some(board.time_stamp())
    }

    /// Port of `CONNECTIVITY_DATA::RefreshNetcodeMap( aBoard )`.
    pub fn refresh_netcode_map(&mut self, board: &Board) {
        self.netcode_map.clear();

        for net in board.nets() {
            self.netcode_map
                .insert(net.net_code().0, net.netname().to_string());
        }
    }

    /// Port of `HasNetNameForNetCode()`.
    pub fn has_net_name_for_net_code(&self, nc: i32) -> bool {
        self.netcode_map.contains_key(&nc)
    }

    /// Port of `GetNetNameForNetCode()` (`std::map::at`: `None` for an unknown code instead
    /// of throwing).
    pub fn net_name_for_net_code(&self, nc: i32) -> Option<&str> {
        self.netcode_map.get(&nc).map(String::as_str)
    }

    /// Port of `Add( BOARD_ITEM* )` (incremental; the tier 1/2 flows rebuild in full).
    pub fn add(&mut self, ctx: &BoardCtx<'_>, item: ItemId) -> bool {
        self.conn_algo.add(ctx, item);
        true
    }

    /// Port of `Remove( BOARD_ITEM* )`.
    pub fn remove(&mut self, ctx: &BoardCtx<'_>, item: ItemId) -> bool {
        self.conn_algo.remove(ctx, item);
        true
    }

    /// Port of `Update( BOARD_ITEM* )`: remove, then add.
    pub fn update(&mut self, ctx: &BoardCtx<'_>, item: ItemId) -> bool {
        self.conn_algo.remove(ctx, item);
        self.conn_algo.add(ctx, item);
        true
    }

    /// Port of `GetConnectivityAlgo()`.
    pub fn algo(&self) -> &CnConnectivityAlgo {
        &self.conn_algo
    }

    /// Mutable `GetConnectivityAlgo()`.
    pub fn algo_mut(&mut self) -> &mut CnConnectivityAlgo {
        &mut self.conn_algo
    }

    /// The connectivity item list (`GetConnectivityAlgo()->ItemList()`).
    pub fn item_list(&self) -> &CnList {
        self.conn_algo.item_list()
    }

    // ---------------------------------------------------------------------------------------
    // Ratsnest
    // ---------------------------------------------------------------------------------------

    /// Port of `updateRatsnest()`: `UpdateNet()` then (second pass) `OptimizeRNEdges()` for
    /// every net from 1 that is dirty and has nodes (sequential; each net is independent).
    fn update_ratsnest(&mut self, ctx: &BoardCtx<'_>) {
        // Start with net 1 as net 0 is reserved for not-connected
        // Nets without nodes are also ignored
        let dirty_nets: Vec<usize> = (1..self.nets.len())
            .filter(|&i| self.nets[i].is_dirty() && self.nets[i].node_count() > 0)
            .collect();

        for &i in &dirty_nets {
            // A delaunator failure is a std::runtime_error swallowed by the thread pool's
            // wait() in C++: the net stays as it is (see RnNet::update_net).
            let _ = self.nets[i].update_net(ctx, self.conn_algo.item_list_mut());
        }

        for &i in &dirty_nets {
            self.nets[i].optimize_rn_edges(ctx, self.conn_algo.item_list());
        }
    }

    /// Port of `RecalculateRatsnest()`.
    pub fn recalculate_ratsnest(&mut self, ctx: &BoardCtx<'_>) {
        self.internal_recalculate_ratsnest(ctx);
    }

    /// Port of `internalRecalculateRatsnest()`.
    fn internal_recalculate_ratsnest(&mut self, ctx: &BoardCtx<'_>) {
        self.conn_algo.propagate_nets(ctx);

        let last_net = self.conn_algo.net_count();

        if last_net >= self.nets.len() as i32 {
            self.nets.resize_with(last_net as usize + 1, RnNet::new);
        } else {
            for ii in last_net as usize..self.nets.len() {
                self.nets[ii].clear();
            }
        }

        let clusters = self.conn_algo.get_clusters(ctx).to_vec();

        for net in 0..last_net {
            if self.conn_algo.is_net_dirty(net) {
                self.nets[net as usize].clear();
            }
        }

        for c in clusters {
            let cluster = self.conn_algo.cluster(c);
            let net = cluster.origin_net();

            // Don't add intentionally-kept zone islands to the ratsnest
            if cluster.is_orphaned()
                && cluster.size() == 1
                && self
                    .conn_algo
                    .item_list()
                    .item(cluster.items()[0])
                    .zone_layer()
                    .is_some()
            {
                continue;
            }

            if self.conn_algo.is_net_dirty(net) {
                // addRatsnestCluster( c ): m_nets[ OriginNet ]->AddCluster( c ). The origin
                // net of a ratsnest cluster is > 0 (CSM_RATSNEST seeds only items with a net).
                let items = cluster.items().to_vec();
                self.nets[net as usize].add_cluster(self.conn_algo.item_list_mut(), c, &items);
            }
        }

        self.conn_algo.clear_dirty_flags();

        if !self.skip_ratsnest_update {
            self.update_ratsnest(ctx);
        }
    }

    /// Port of `GetNetCount()`.
    pub fn net_count(&self) -> i32 {
        self.conn_algo.net_count()
    }

    /// Port of `GetRatsnestForNet( aNet )`.
    pub fn ratsnest_for_net(&self, net: i32) -> Option<&RnNet> {
        if net < 0 || net >= self.nets.len() as i32 {
            return None;
        }

        Some(&self.nets[net as usize])
    }

    /// Mutable `GetRatsnestForNet( aNet )` (`RN_NET::GetEdges()` sorts in place).
    pub fn ratsnest_for_net_mut(&mut self, net: i32) -> Option<&mut RnNet> {
        if net < 0 || net >= self.nets.len() as i32 {
            return None;
        }

        Some(&mut self.nets[net as usize])
    }

    /// The ratsnest edges of `net` in `GetEdges()` order (stable-sorted, as every C++ call
    /// sorts); empty for an unknown net.
    pub fn net_edges(&mut self, net: i32) -> &[CnEdge] {
        let list = self.conn_algo.item_list();

        match usize::try_from(net).ok().and_then(|n| self.nets.get_mut(n)) {
            Some(rn) => rn.edges(list),
            None => &[],
        }
    }

    /// Port of `ClearRatsnest()`.
    pub fn clear_ratsnest(&mut self) {
        for net in &mut self.nets {
            net.clear();
        }
    }

    /// Port of `GetUnconnectedCount( aVisibleOnly )`. Counting does not depend on the edge
    /// order, so the unsorted edges are counted (C++ `GetEdges()` sorts them first).
    pub fn unconnected_count(&self, visible_only: bool) -> u32 {
        let mut unconnected = 0;

        for net in &self.nets {
            for edge in net.unsorted_edges() {
                if edge.is_visible() || !visible_only {
                    unconnected += 1;
                }
            }
        }

        unconnected
    }

    /// Port of `RunOnUnconnectedEdges( aFunc )`: every edge of every net in net order and
    /// `GetEdges()` order, until `func` returns false. `func` receives the item list to
    /// resolve the edge's anchors.
    pub fn run_on_unconnected_edges(&mut self, mut func: impl FnMut(&mut CnEdge, &CnList) -> bool) {
        let list = self.conn_algo.item_list();

        for rn_net in &mut self.nets {
            for edge in rn_net.edges_mut(list).iter_mut() {
                if !func(edge, list) {
                    return;
                }
            }
        }
    }

    /// The unconnected edges of every net (net order, `GetEdges()` order) as point pairs with
    /// the items they join; edges with a missing or dirty node are left out (as every C++
    /// consumer of `RunOnUnconnectedEdges` does). The client API of an autorouter / the DRC
    /// unconnected-items test.
    pub fn unconnected_edges(&mut self) -> Vec<UnconnectedEdge> {
        let mut out = Vec::new();
        let list = self.conn_algo.item_list();

        for (net, rn_net) in self.nets.iter_mut().enumerate() {
            for edge in rn_net.edges(list) {
                let (Some(src), Some(dst)) = (edge.source_anchor(list), edge.target_anchor(list))
                else {
                    continue;
                };

                if src.dirty(list) || dst.dirty(list) {
                    continue;
                }

                out.push(UnconnectedEdge {
                    net: net as i32,
                    source: src.pos(),
                    target: dst.pos(),
                    source_item: src.parent(list),
                    target_item: dst.parent(list),
                    weight: edge.weight(),
                    visible: edge.is_visible(),
                });
            }
        }

        out
    }

    /// Port of `BOARD::UpdateRatsnestExclusions()`: an edge is hidden when an excluded
    /// ratsnest marker names its two items (either way round).
    pub fn update_ratsnest_exclusions(&mut self, board: &Board) {
        let mut ratsnest_exclusions: BTreeSet<(Kiid, Kiid)> = BTreeSet::new();

        for (_, marker) in board.markers() {
            if marker.marker_type() == MarkerT::Ratsnest && marker.is_excluded() {
                let rc_item = marker.rc_item();
                ratsnest_exclusions.insert((rc_item.main_item_id(), rc_item.aux_item_id()));
                ratsnest_exclusions.insert((rc_item.aux_item_id(), rc_item.main_item_id()));
            }
        }

        let ctx = BoardCtx::bare(board);
        let uuid_of = |id: ItemId| ctx.item(id).map(|i| i.uuid()).unwrap_or_default();

        self.run_on_unconnected_edges(|edge, list| {
            if let (Some(src), Some(dst)) = (edge.source_anchor(list), edge.target_anchor(list)) {
                if !src.dirty(list) && !dst.dirty(list) {
                    let ids = (uuid_of(src.parent(list)), uuid_of(dst.parent(list)));
                    let visible = !ratsnest_exclusions.contains(&ids);
                    edge.set_visible(visible);
                }
            }

            true
        });
    }

    /// Port of `GetRatsnestForItems( aItems )`: the edges of the items' nets whose two ends
    /// are both on items of the set (footprints stand for their pads).
    pub fn ratsnest_for_items(&mut self, ctx: &BoardCtx<'_>, items: &[ItemId]) -> Vec<CnEdge> {
        let mut nets: BTreeSet<i32> = BTreeSet::new();
        let mut edges: Vec<CnEdge> = Vec::new();
        let mut item_set: BTreeSet<ItemId> = BTreeSet::new();

        for &id in items {
            let Some(item) = ctx.item(id) else {
                continue;
            };

            if let Some(fp) = item.as_footprint().and(id.typed()) {
                for (pad_id, _) in ctx.board.footprint_pads(fp) {
                    let pad: ItemId = pad_id.into();
                    nets.insert(net_code_of(ctx, pad));
                    item_set.insert(pad);
                }
            } else if item.is_connected() {
                item_set.insert(id);
                nets.insert(item.net_code().map_or(0, |n| n.0));
            }
        }

        let list = self.conn_algo.item_list();

        for netcode in nets {
            let Some(net) = usize::try_from(netcode)
                .ok()
                .and_then(|n| self.nets.get_mut(n))
            else {
                continue;
            };

            for edge in net.edges(list) {
                let (Some(src), Some(dst)) = (edge.source_anchor(list), edge.target_anchor(list))
                else {
                    continue;
                };

                if src.dirty(list) || dst.dirty(list) {
                    continue;
                }

                let src_found = item_set.contains(&src.parent(list));
                let dst_found = item_set.contains(&dst.parent(list));

                if src_found && dst_found {
                    edges.push(edge.clone());
                }
            }
        }

        edges
    }

    /// Port of `GetRatsnestForPad( aPad )`.
    pub fn ratsnest_for_pad(&mut self, ctx: &BoardCtx<'_>, pad: ItemId) -> Vec<CnEdge> {
        let mut edges = Vec::new();
        let netcode = net_code_of(ctx, pad);
        let list = self.conn_algo.item_list();

        let Some(net) = usize::try_from(netcode)
            .ok()
            .and_then(|n| self.nets.get_mut(n))
        else {
            return edges;
        };

        for edge in net.edges(list) {
            let Some(src) = edge.source_anchor(list).filter(|a| !a.dirty(list)) else {
                continue;
            };

            let Some(dst) = edge.target_anchor(list).filter(|a| !a.dirty(list)) else {
                continue;
            };

            if src.parent(list) == pad || dst.parent(list) == pad {
                edges.push(edge.clone());
            }
        }

        edges
    }

    /// Port of `GetRatsnestForComponent( aComponent, aSkipInternalConnections )`.
    ///
    /// As written in C++, an edge between two pads of the footprint is kept whatever
    /// `skip_internal_connections` says (the `else if` takes it).
    pub fn ratsnest_for_component(
        &mut self,
        ctx: &BoardCtx<'_>,
        component: ItemId,
        skip_internal_connections: bool,
    ) -> Vec<CnEdge> {
        let mut nets: BTreeSet<i32> = BTreeSet::new();
        let mut pads: BTreeSet<ItemId> = BTreeSet::new();
        let mut edges: Vec<CnEdge> = Vec::new();

        if let Some(fp) = component.typed() {
            for (pad_id, _) in ctx.board.footprint_pads(fp) {
                let pad: ItemId = pad_id.into();
                nets.insert(net_code_of(ctx, pad));
                pads.insert(pad);
            }
        }

        let list = self.conn_algo.item_list();

        for netcode in nets {
            let Some(net) = usize::try_from(netcode)
                .ok()
                .and_then(|n| self.nets.get_mut(n))
            else {
                continue;
            };

            for edge in net.edges(list) {
                let (Some(src), Some(dst)) = (edge.source_anchor(list), edge.target_anchor(list))
                else {
                    continue;
                };

                if src.dirty(list) || dst.dirty(list) {
                    continue;
                }

                let src_found = pads.contains(&src.parent(list));
                let dst_found = pads.contains(&dst.parent(list));

                if (src_found && dst_found && !skip_internal_connections) || src_found || dst_found
                {
                    edges.push(edge.clone());
                }
            }
        }

        edges
    }

    // ---------------------------------------------------------------------------------------
    // Nets, islands
    // ---------------------------------------------------------------------------------------

    /// Port of `PropagateNets()` (the changes are recorded, see
    /// [`ConnectivityData::take_net_changes`]).
    pub fn propagate_nets(&mut self, ctx: &BoardCtx<'_>) {
        self.conn_algo.propagate_nets(ctx);
    }

    /// Port of `FillIsolatedIslandsMap( aMap, aConnectivityAlreadyRebuilt )`.
    pub fn fill_isolated_islands_map(
        &mut self,
        ctx: &BoardCtx<'_>,
        map: &mut IsolatedIslandsMap,
        connectivity_already_rebuilt: bool,
    ) {
        self.conn_algo
            .fill_isolated_islands_map(ctx, map, connectivity_already_rebuilt);
    }

    /// Port of `MarkItemNetAsDirty( aItem )`: a footprint marks the nets of its pads; a
    /// connected item its own net.
    pub fn mark_item_net_as_dirty(&mut self, ctx: &BoardCtx<'_>, item: ItemId) {
        let Some(item_ref) = ctx.item(item) else {
            return;
        };

        if let Some(fp) = item_ref.as_footprint().and(item.typed()) {
            for (pad_id, _) in ctx.board.footprint_pads(fp) {
                self.conn_algo
                    .mark_net_as_dirty(net_code_of(ctx, pad_id.into()));
            }
        }

        if item_ref.is_connected() {
            self.conn_algo
                .mark_net_as_dirty(item_ref.net_code().map_or(0, |n| n.0));
        }
    }

    /// Port of `RemoveInvalidRefs()`: the algorithm's items, then the ratsnest of every net.
    pub fn remove_invalid_refs(&mut self) {
        self.conn_algo.remove_invalid_refs();

        let list = self.conn_algo.item_list();

        for rn_net in &mut self.nets {
            rn_net.remove_invalid_refs(list);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Queries
    // ---------------------------------------------------------------------------------------

    /// Port of `IsConnectedOnLayer( aItem, aLayer, aTypes )`: whether a connected item of a
    /// type in `types` (empty = any) and of the item's net touches the item on `layer`.
    /// Used by `PAD::FlashLayer` / `PCB_VIA::FlashLayer` with `{ TRACE, ARC, VIA, PAD }`.
    ///
    /// An item without a handle (a detached copy) has no entry in the item map: false.
    pub fn is_connected_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: i32,
        types: &[KicadT],
    ) -> bool {
        let Some(item_id) = item.id() else {
            return false;
        };

        let Some(entry) = self.conn_algo.item_entry(item_id) else {
            return false;
        };

        let list = self.conn_algo.item_list();
        let parent_footprint = item.parent_footprint_id();
        let item_type = item.type_();
        let item_net = item.net_code().map_or(0, |n| n.0);

        let match_type = |item_type: KicadT| types.is_empty() || types.contains(&item_type);

        for &citem in &entry.items {
            for &connected_id in list.item(citem).connected_items() {
                let connected = list.item(connected_id);
                let zone_layer = connected.zone_layer();

                // StartLayer() and EndLayer() are copper layer ordinals, not PCB_LAYER_IDs
                let ly_idx = copper_layer_to_ordinal(to_layer_id(layer)) as i32;

                if connected.valid()
                    && connected.start_layer() <= ly_idx
                    && connected.end_layer() >= ly_idx
                    && match_type(connected.parent_type())
                    && connected.net() == item_net
                {
                    let connected_item_id = connected.parent();

                    if connected_item_id == item_id {
                        continue;
                    }

                    let Some(connected_item) = ctx.item(connected_item_id) else {
                        continue;
                    };

                    if parent_footprint.is_some()
                        && connected_item.parent_footprint_id() == parent_footprint
                    {
                        continue;
                    }

                    if item_type == KicadT::PcbPadT && connected_item.type_() == KicadT::PcbPadT {
                        let flashes_conditionally = |mode: UnconnectedLayerMode| {
                            mode == UnconnectedLayerMode::RemoveExceptStartAndEnd
                                || mode == UnconnectedLayerMode::RemoveAll
                        };

                        let this_mode = item.as_pad().map(|p| p.unconnected_layer_mode());
                        let other_mode =
                            connected_item.as_pad().map(|p| p.unconnected_layer_mode());

                        if this_mode.is_some_and(flashes_conditionally)
                            && other_mode.is_some_and(flashes_conditionally)
                        {
                            continue;
                        }
                    }

                    // A pad or a via spans several layers, so touching us is not the same as
                    // touching us here
                    if connected_item.type_() == KicadT::PcbPadT
                        || connected_item.type_() == KicadT::PcbViaT
                    {
                        if !items_touch_on_layer(&item, &connected_item, to_layer_id(layer)) {
                            continue;
                        }
                    }

                    if let (Some(pad), Some(zone_layer)) = (item.as_pad(), zone_layer) {
                        let island_idx = zone_layer.subpoly_index();

                        if let Some(zone) = connected_item.as_zone().filter(|z| z.is_filled()) {
                            let pcb_layer = to_layer_id(layer);

                            if let Some(zone_fill) = zone.get_fill(pcb_layer) {
                                let pad_poly =
                                    pad.effective_polygon(ctx, pcb_layer, ErrorLoc::Inside);

                                for &pt in zone_fill.c_outline(island_idx).c_points() {
                                    // If the entire island is inside the pad's flashing then
                                    // the pad won't actually connect to anything else, so only
                                    // return true if part of the island is *outside* the
                                    // pad's flashing.

                                    // (Outline( 0 ) of an empty set is undefined in C++; an
                                    // empty hull contains no point.)
                                    let inside = pad_poly.outline_count() > 0
                                        && pad_poly.c_outline(0).point_inside(pt, 0, false);

                                    if !inside {
                                        return true;
                                    }
                                }
                            }
                        }

                        continue;
                    } else if let (Some(via), Some(zone_layer)) = (item.as_via(), zone_layer) {
                        let island_idx = zone_layer.subpoly_index();

                        if let Some(zone) = connected_item.as_zone().filter(|z| z.is_filled()) {
                            let layer = to_layer_id(layer);

                            if let Some(zone_fill) = zone.get_fill(layer) {
                                // PCB_VIA::GetCenter() is GetPosition()
                                let via_hull =
                                    ShapeCircle::new(item.position(), via.width(layer) / 2);

                                for &pt in zone_fill.c_outline(island_idx).c_points() {
                                    // If the entire island is inside the via's flashing then
                                    // the via won't actually connect to anything else, so only
                                    // return true if part of the island is *outside* the via's
                                    // flashing.

                                    // viaHull.SHAPE::Collide( pt ) = Collide( SEG( pt, pt ) )
                                    if !via_hull.collide_seg(&Seg::new(pt, pt), 0, None, None) {
                                        return true;
                                    }
                                }
                            }
                        }

                        continue;
                    }

                    return true;
                }
            }
        }

        false
    }

    /// Port of `GetNodeCount( aNet )`: the ratsnest nodes of `net`, or of every net for a
    /// negative code.
    pub fn node_count_of(&self, net: i32) -> u32 {
        let mut sum: i32 = 0;

        if net < 0 {
            // Node count for all nets
            for rn_net in &self.nets {
                sum += rn_net.node_count() as i32;
            }
        } else if net < self.nets.len() as i32 {
            sum = self.nets[net as usize].node_count() as i32;
        }

        sum as u32
    }

    /// Port of `GetPadCount( aNet )`: the valid pad items (of `net`, or all for a negative
    /// code; the pad's own net code is compared).
    pub fn pad_count_of(&self, ctx: &BoardCtx<'_>, net: i32) -> u32 {
        let mut n: i32 = 0;

        for pad in self.conn_algo.item_list().iter() {
            if !pad.valid() || pad.parent_type() != KicadT::PcbPadT {
                continue;
            }

            if net < 0 || net == net_code_of(ctx, pad.parent()) {
                n += 1;
            }
        }

        n as u32
    }

    /// Port of `GetConnectedItems( aItem, aFlags )` (`IGNORE_NETS`, `EXCLUDE_ZONES`): the
    /// items of every cluster containing the item, in cluster order. `&mut self` because the
    /// cluster search runs the pending contact search and keeps its clusters in the arena.
    pub fn connected_items(&mut self, ctx: &BoardCtx<'_>, item: ItemId, flags: i32) -> Vec<ItemId> {
        let mut rv = Vec::new();

        let mode = if flags & IGNORE_NETS != 0 {
            ClusterSearchMode::Propagate
        } else {
            ClusterSearchMode::ConnectivityCheck
        };

        let single_net = if flags & IGNORE_NETS != 0 {
            -1
        } else {
            net_code_of(ctx, item)
        };

        let clusters =
            self.conn_algo
                .search_clusters_ex(ctx, mode, flags & EXCLUDE_ZONES != 0, single_net);
        let list = self.conn_algo.item_list();

        for cl in clusters {
            let cluster = self.conn_algo.cluster(cl);

            if cluster.contains_parent(list, item) {
                for &citem in cluster.items() {
                    let citem = list.item(citem);

                    if citem.valid() {
                        rv.push(citem.parent());
                    }
                }
            }
        }

        rv
    }

    /// Port of `GetNetItems( aNetCode, aTypes )`: the valid items of a net whose type is in
    /// `types`, sorted and deduplicated. C++ sorts by `BOARD_CONNECTED_ITEM*` address; the
    /// port sorts by `ItemId` (board-model.md 3.1 rule 2).
    pub fn net_items(&self, net_code: i32, types: &[KicadT]) -> Vec<ItemId> {
        let mut items: Vec<ItemId> = Vec::with_capacity(32);

        self.conn_algo.for_each_item(|item| {
            if item.valid() && item.net() == net_code && types.contains(&item.parent_type()) {
                items.push(item.parent());
            }
        });

        items.sort();
        items.dedup();
        items
    }

    /// Port of `GetConnectedTracks( aItem )`: the tracks, arcs and vias connected to the item.
    /// C++ collects them in a `std::set<PCB_TRACK*>` (address order); the port orders by
    /// `ItemId` (= board track order on a loaded board).
    pub fn connected_tracks(&self, item: ItemId) -> Vec<ItemId> {
        let mut tracks: BTreeSet<ItemId> = BTreeSet::new();
        let list = self.conn_algo.item_list();

        if let Some(entry) = self.conn_algo.item_entry(item) {
            for &citem in &entry.items {
                for &connected in list.item(citem).connected_items() {
                    let connected = list.item(connected);

                    if connected.valid()
                        && matches!(
                            connected.parent_type(),
                            KicadT::PcbTraceT | KicadT::PcbViaT | KicadT::PcbArcT
                        )
                    {
                        tracks.insert(connected.parent());
                    }
                }
            }
        }

        tracks.into_iter().collect()
    }

    /// Port of `GetConnectedPads( aItem, std::set<PAD*>* )`: adds the pads connected to the
    /// item to `pads` (`ItemId` order instead of address order).
    pub fn connected_pads_into(&self, item: ItemId, pads: &mut BTreeSet<ItemId>) {
        let list = self.conn_algo.item_list();

        if let Some(entry) = self.conn_algo.item_entry(item) {
            for &citem in &entry.items {
                for &connected in list.item(citem).connected_items() {
                    let connected = list.item(connected);

                    if connected.valid() && connected.parent_type() == KicadT::PcbPadT {
                        pads.insert(connected.parent());
                    }
                }
            }
        }
    }

    /// Port of `GetConnectedPads( aItem )`: the pads connected to the item, in `ItemId` order
    /// (C++: `std::set<PAD*>` address order).
    pub fn connected_pads(&self, item: ItemId) -> Vec<ItemId> {
        let mut pads = BTreeSet::new();
        self.connected_pads_into(item, &mut pads);
        pads.into_iter().collect()
    }

    /// Port of `GetConnectedPadsAndVias( aItem, pads, vias )`: appends the connected pads and
    /// vias in `ConnectedItems()` order (list order, the CN-01 replacement of the address
    /// order, connectivity-zones.md R-1), duplicates included.
    pub fn connected_pads_and_vias(
        &self,
        item: ItemId,
        pads: &mut Vec<ItemId>,
        vias: &mut Vec<ItemId>,
    ) {
        let list = self.conn_algo.item_list();

        if let Some(entry) = self.conn_algo.item_entry(item) {
            for &citem in &entry.items {
                for &connected in list.item(citem).connected_items() {
                    let connected = list.item(connected);

                    if connected.valid() {
                        match connected.parent_type() {
                            KicadT::PcbPadT => pads.push(connected.parent()),
                            KicadT::PcbViaT => vias.push(connected.parent()),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Port of `GetZoneIslandConnections( aZone, aLayer, aIslands )`: for each valid filled
    /// island of the zone on `layer` (item order), the items it touches. C++ uses
    /// `std::set<const BOARD_ITEM*>`; the port `BTreeSet<ItemId>`.
    pub fn zone_island_connections(
        &self,
        zone: ItemId,
        layer: PcbLayerId,
    ) -> Vec<BTreeSet<ItemId>> {
        let mut islands: Vec<BTreeSet<ItemId>> = Vec::new();
        let list = self.conn_algo.item_list();

        let Some(entry) = self.conn_algo.item_entry(zone) else {
            return islands;
        };

        for &citem in &entry.items {
            let island = list.item(citem);

            let Some(zone_layer) = island.zone_layer() else {
                continue;
            };

            if !island.valid() || zone_layer.layer() != layer {
                continue;
            }

            let mut connected = BTreeSet::new();

            for &other in island.connected_items() {
                let other = list.item(other);

                if other.valid() {
                    connected.insert(other.parent());
                }
            }

            islands.push(connected);
        }

        islands
    }

    /// Port of `GetConnectedItemsAtAnchor( aItem, aAnchor, aTypes, aMaxError )`: the connected
    /// items of a type in `types` with a stored anchor within `max_error` of `anchor`
    /// (`ConnectedItems()` order).
    pub fn connected_items_at_anchor(
        &self,
        item: ItemId,
        anchor: Vector2I,
        types: &[KicadT],
        max_error: i32,
    ) -> Vec<ItemId> {
        let mut rv = Vec::new();
        let max_error_sq: i64 = max_error as i64 * max_error as i64;
        let list = self.conn_algo.item_list();

        let Some(entry) = self.conn_algo.item_entry(item) else {
            return rv;
        };

        for &cn_item in &entry.items {
            for &connected in list.item(cn_item).connected_items() {
                let connected = list.item(connected);

                for a in connected.anchors() {
                    if (a.pos() - anchor).squared_euclidean_norm() <= max_error_sq {
                        for &type_ in types {
                            if connected.valid() && connected.parent_type() == type_ {
                                rv.push(connected.parent());
                                break;
                            }
                        }

                        break;
                    }
                }
            }
        }

        rv
    }

    /// Port of `TestTrackEndpointDangling( aTrack, aIgnoreTracksInPads, aPos )`: whether a
    /// track / arc has an unconnected end, or a via is connected on at most one layer.
    /// Returns (dangling, the `*aPos` written, if any).
    ///
    /// Item shapes are read with this connectivity as the board's (`ctx.connectivity` is
    /// replaced by `self`, as C++ `GetBoard()->GetConnectivity()` is this object).
    /// `zone_rtrees` is the board's copper zone tree cache (`None`: no cache, every zone
    /// tests its effective shape, as on a board DRC has not run on).
    pub fn test_track_endpoint_dangling(
        &self,
        ctx: &BoardCtx<'_>,
        track: ItemId,
        ignore_tracks_in_pads: bool,
        zone_rtrees: Option<&dyn CopperZoneRtrees>,
    ) -> (bool, Option<Vector2I>) {
        let ctx = BoardCtx {
            board: ctx.board,
            rules: ctx.rules,
            connectivity: Some(self as &dyn ConnectivityQuery),
        };

        let list = self.conn_algo.item_list();

        // Not in the connectivity system.  This is a bug!
        let Some(&citem) = self
            .conn_algo
            .item_entry(track)
            .and_then(|entry| entry.items.first())
        else {
            debug_assert!(false, "track not in connectivity system");
            return (false, None);
        };

        let citem = list.item(citem);

        if !citem.valid() {
            return (false, None);
        }

        let Some(track_ref) = ctx.item(track) else {
            return (false, None);
        };

        let Some(t) = track_ref.as_track() else {
            debug_assert!(
                false,
                "CONNECTIVITY_DATA::TestTrackEndpointDangling: unknown track type"
            );
            return (false, None);
        };

        let track_type = track_ref.type_();

        if track_type == KicadT::PcbTraceT || track_type == KicadT::PcbArcT {
            // Test if a segment is connected on each end.
            //
            // NB: be wary of short segments which can be connected to the *same* other item
            // on each end.  If that's their only connection then they're still dangling.

            let layer = track_ref.layer();
            let accuracy: i32 = ki_round(t.width() as f64 / 2.0);
            let start = t.as_track().start();
            let end = t.as_track().end();
            let mut start_count = 0;
            let mut end_count = 0;

            for &connected in citem.connected_items() {
                let parent = list.item(connected).parent();

                let Some(item) = ctx.item(parent) else {
                    continue;
                };

                if item.base().flags.0 & ItemFlags::IS_DELETED != 0 {
                    continue;
                }

                let zone: Option<ZoneId> = item.as_zone().and(parent.typed());
                let rtree_hits = match (zone, zone_rtrees) {
                    (Some(zone), Some(rtrees)) => {
                        let start_shape = ShapeCircle::new(start, accuracy);
                        let end_shape = ShapeCircle::new(end, accuracy);

                        rtrees
                            .query_colliding(zone, &start_shape, layer)
                            .zip(rtrees.query_colliding(zone, &end_shape, layer))
                    }
                    _ => None,
                };

                let (hit_start, hit_end) = match rtree_hits {
                    Some(hits) => hits,
                    None => {
                        let shape = item.effective_shape(
                            layer,
                            Flashing::Default,
                            DrcConstraintT::NullConstraint,
                        );

                        (
                            shape.collide_point(start, accuracy, None, None),
                            shape.collide_point(end, accuracy, None, None),
                        )
                    }
                };

                if hit_start && hit_end {
                    if zone.is_some() {
                        // Both start and end in a zone: track may be redundant, but it's not
                        // dangling
                        return (false, None);
                    } else if item.type_() == KicadT::PcbPadT || item.type_() == KicadT::PcbViaT {
                        // Both start and end are under a pad: see what the caller wants us to
                        // do
                        if ignore_tracks_in_pads {
                            return (false, None);
                        }
                    }

                    if min_dist(&item, start) < min_dist(&item, end) {
                        start_count += 1;
                    } else {
                        end_count += 1;
                    }
                } else if hit_start {
                    start_count += 1;
                } else if hit_end {
                    end_count += 1;
                }

                if start_count > 0 && end_count > 0 {
                    return (false, None);
                }
            }

            let pos = if start_count == 0 { start } else { end };
            (true, Some(pos))
        } else {
            // PCB_VIA_T: test if a via is only connected on one layer

            let connected = citem.connected_items();

            if connected.is_empty() {
                // No connections AND no-net is not an error
                if track_ref.net_code().map_or(0, |n| n.0) <= 0 {
                    return (false, None);
                }

                return (true, Some(track_ref.position()));
            }

            // Here, we check if the via is connected only to items on a single layer
            let mut first_layer: i32 = UNDEFINED_LAYER.0;

            for &item in connected {
                let item = list.item(item);
                let deleted = ctx
                    .item(item.parent())
                    .is_some_and(|p| p.base().flags.0 & ItemFlags::IS_DELETED != 0);

                if deleted {
                    continue;
                }

                if first_layer == UNDEFINED_LAYER.0 {
                    first_layer = item.layer();
                } else if item.layer() != first_layer {
                    return (false, None);
                }
            }

            (true, Some(track_ref.position()))
        }
    }
}

/// Port of the static `getMinDist( aItem, aPoint )` of `connectivity_data.cpp`: the distance
/// from the nearer end of a track / arc, else from the item position (truncated to `int`).
fn min_dist(item: &ItemRef<'_>, point: Vector2I) -> i32 {
    match item.type_() {
        KicadT::PcbTraceT | KicadT::PcbArcT => {
            let track = item.as_track().expect("track type").as_track();

            track
                .start()
                .distance(point)
                .min(track.end().distance(point)) as i32
        }
        _ => item.position().distance(point) as i32,
    }
}

/// `GetNetCode()` of a board item (0 for a stale handle or a non-connected kind).
fn net_code_of(ctx: &BoardCtx<'_>, item: ItemId) -> i32 {
    ctx.item(item).and_then(|i| i.net_code()).map_or(0, |n| n.0)
}

/// Port of the fill part of `BOARD::CacheTriangulation()` (`board.cpp`): every filled layer
/// of every board zone (sequential). The zone outlines (`CacheOutlineTriangulation`) are a
/// DRC cache only: the connectivity never reads them, and they are left to the DRC cache
/// generator.
pub fn cache_board_triangulation(board: &mut Board) {
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    for zone in zones {
        // The format stores each fill's layer independently, so the layer set can miss one
        board
            .zone_fill_mut(zone)
            .cache_triangulation(PcbLayerId::UndefinedLayer);
    }
}

impl ConnectivityQuery for ConnectivityData {
    fn is_connected_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        types: &[KicadT],
    ) -> bool {
        ConnectivityData::is_connected_on_layer(self, ctx, item, layer.0, types)
    }

    fn node_count(&self, net: Option<NetCode>) -> u32 {
        self.node_count_of(net.map_or(-1, |n| n.0))
    }

    fn net_items(&self, net: NetCode, types: &[KicadT]) -> Vec<ItemId> {
        ConnectivityData::net_items(self, net.0, types)
    }

    fn pad_count(&self, net: Option<NetCode>) -> u32 {
        // GetPadCount compares the pads' own net codes; the cached connectivity net is the
        // same value once the propagation has been applied (pads never change net).
        let net = net.map_or(-1, |n| n.0);
        let mut n: u32 = 0;

        for pad in self.conn_algo.item_list().iter() {
            if !pad.valid() || pad.parent_type() != KicadT::PcbPadT {
                continue;
            }

            if net < 0 || net == pad.net() {
                n += 1;
            }
        }

        n
    }
}
