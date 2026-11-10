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

//! Port of `pcbnew/connectivity/connectivity_algo.h` and `connectivity_algo.cpp`
//! (`CN_CONNECTIVITY_ALGO`: build, contact search, `CN_VISITOR`, clusters, net propagation,
//! jumper pads, isolated islands) and of `libs/core/include/core/union_find.h`
//! (`KI_UNION_FIND`).
//!
//! The C++ thread-pool loops (`BuildIndexes`, the contact search) run sequentially in item
//! order; progress reporting and cancellation are not ported (connectivity-zones.md 8).
//!
//! Board mutations: C++ calls `Parent()->SetNetCode()` from the deferred via-net rule and the
//! net propagation. The port updates the item's cached net ([`CnItem::set_net`]) so that the
//! algorithm sees its own changes immediately, and records `(parent, net)` in
//! [`CnConnectivityAlgo::take_net_changes`]; the caller applies them to the board with
//! `Board::set_item_net` (board-model.md 13).

use std::collections::{BTreeMap, HashMap};

use kicad_core::layer_ids::is_copper_layer;
use kicad_core::lset::Lset;
use kicad_core::{Flashing, KicadT, PcbLayerId};
use kicad_geom::stdsort::std_sort;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::footprint::footprint_attr::FP_JUST_ADDED;
use kicad_pcb::ids::{ItemId, ItemKind, NetCode, ZoneId};
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::pcb_track::ZoneLayerOverride;
use kicad_pcb::zone::IsolatedIslands;

use crate::items::{CnCluster, CnClusterId, CnItem, CnItemId, CnList};

// =================================================================================================
// KI_UNION_FIND
// =================================================================================================

/// Port of `KI_UNION_FIND` (`libs/core/include/core/union_find.h`): a disjoint-set forest
/// using Rem's algorithm, in which every tree is rooted at the smallest index it holds.
///
/// The C++ class is lock-free (atomic parents, compare-and-swap); the port is the
/// single-threaded version of the same statements (every exchange succeeds), which yields the
/// same parents and roots as a serial C++ run.
#[derive(Clone, Debug, Default)]
pub struct KiUnionFind {
    /// `m_parent`
    parent: Vec<u32>,
    /// `m_components`
    components: usize,
}

impl KiUnionFind {
    /// Port of `KI_UNION_FIND( aCount )`.
    pub fn new(count: usize) -> Self {
        let mut uf = KiUnionFind::default();
        uf.reset(count);
        uf
    }

    /// Port of `Reset()`: discard all unions and resize the set to `count` single-element
    /// components.
    ///
    /// Parents are `u32` to keep the array dense; C++ throws `std::length_error` for a larger
    /// set (here a programmer error: the connectivity item list never gets near 2^32).
    pub fn reset(&mut self, count: usize) {
        assert!(
            count <= u32::MAX as usize,
            "KI_UNION_FIND exceeds its 2^32 element limit"
        );

        self.parent = (0..count as u32).collect();
        self.components = count;
    }

    /// `Size()`.
    pub fn size(&self) -> usize {
        self.parent.len()
    }

    /// `ComponentCount()`: the number of components that remain.
    pub fn component_count(&self) -> usize {
        self.components
    }

    /// Port of `Unite()`: merge the components that hold `a` and `b`. Returns `true` if this
    /// call made the link.
    pub fn unite(&mut self, a: usize, b: usize) -> bool {
        let mut u = a as u32;
        let mut v = b as u32;
        let mut pu = self.parent[u as usize];
        let mut pv = self.parent[v as usize];

        while pu != pv {
            // Work on the side with the larger parent so that a root always links downwards
            if pu < pv {
                std::mem::swap(&mut u, &mut v);
                std::mem::swap(&mut pu, &mut pv);
            }

            if u == pu {
                // u is a root and pv is below it, so this link is legal
                self.parent[u as usize] = pv;
                self.components -= 1;
                return true;
            }

            // Lift u one level and climb.
            let grandparent = self.parent[pu as usize];
            self.parent[u as usize] = grandparent;
            u = pu;
            pu = self.parent[u as usize];
        }

        false
    }

    /// Port of `Find()`: the representative of the component that holds `x`.
    pub fn find(&self, x: usize) -> usize {
        let mut x = x as u32;
        let mut p = self.parent[x as usize];

        while p != x {
            x = p;
            p = self.parent[x as usize];
        }

        x as usize
    }

    /// Port of `FindCompress()`: like [`KiUnionFind::find`], shortening the path to the root.
    pub fn find_compress(&mut self, x: usize) -> usize {
        let mut x = x as u32;
        let mut p = self.parent[x as usize];

        while p != x {
            let grandparent = self.parent[p as usize];
            self.parent[x as usize] = grandparent;
            x = p;
            p = grandparent;
        }

        x as usize
    }

    /// `Connected()`.
    pub fn connected(&self, a: usize, b: usize) -> bool {
        self.find(a) == self.find(b)
    }
}

// =================================================================================================
// CN_CONNECTIVITY_ALGO
// =================================================================================================

/// `CN_CONNECTIVITY_ALGO::CLUSTER_SEARCH_MODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterSearchMode {
    /// `CSM_PROPAGATE`: clusters across nets, zones excluded by default.
    Propagate,
    /// `CSM_CONNECTIVITY_CHECK`
    ConnectivityCheck,
    /// `CSM_RATSNEST`
    Ratsnest,
}

/// Port of `CN_CONNECTIVITY_ALGO::ITEM_MAP_ENTRY`: the connectivity items of one board item
/// (one for pads, tracks, ...; one per outline per layer for zones).
#[derive(Clone, Debug, Default)]
pub struct ItemMapEntry {
    /// `m_items`
    pub items: Vec<CnItemId>,
}

/// Port of `CN_CONNECTIVITY_ALGO`.
///
/// `m_itemMap` (an `unordered_map` keyed by `BOARD_ITEM*`, lookups only) is a
/// `HashMap<ItemId, _>`. Clusters (`shared_ptr<CN_CLUSTER>`) live in an arena owned by the
/// algorithm and are named by [`CnClusterId`], handed out in creation order (the replacement
/// of the cluster address order the ratsnest uses, connectivity-zones.md R-2).
///
/// Not ported (tier 3, CN-06): `LocalBuild` / `m_isLocal` / `m_globalConnectivityData`
/// (dynamic ratsnest of the interactive tools) and the progress reporter.
#[derive(Clone, Default)]
pub struct CnConnectivityAlgo {
    /// `m_itemList`
    item_list: CnList,
    /// `m_itemMap`
    item_map: HashMap<ItemId, ItemMapEntry>,
    /// Every cluster created by `SearchClusters`, indexed by `CnClusterId`.
    clusters: Vec<CnCluster>,
    /// `m_connClusters`
    conn_clusters: Vec<CnClusterId>,
    /// `m_ratsnestClusters`
    ratsnest_clusters: Vec<CnClusterId>,
    /// `m_dirtyNets`
    dirty_nets: Vec<bool>,
    /// `Parent()->SetNetCode()` calls not yet applied to the board, in call order.
    net_changes: Vec<(ItemId, NetCode)>,
}

/// `CN_CONNECTIVITY_ALGO::CLUSTERS` (`std::vector<std::shared_ptr<CN_CLUSTER>>`).
pub type Clusters = Vec<CnClusterId>;

/// The `std::map<ZONE*, std::map<PCB_LAYER_ID, ISOLATED_ISLANDS>>` of
/// `FillIsolatedIslandsMap`. Keyed by `ZoneId` (board order) instead of the zone address
/// (board-model.md 3.1 rule 2).
pub type IsolatedIslandsMap = BTreeMap<ZoneId, BTreeMap<PcbLayerId, IsolatedIslands>>;

impl CnConnectivityAlgo {
    /// Port of `CN_CONNECTIVITY_ALGO( aParentConnectivityData )`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `ItemExists()`.
    pub fn item_exists(&self, item: ItemId) -> bool {
        self.item_map.contains_key(&item)
    }

    /// `ItemEntry()` (lookup only: C++ `operator[]` would insert an empty entry).
    pub fn item_entry(&self, item: ItemId) -> Option<&ItemMapEntry> {
        self.item_map.get(&item)
    }

    /// Port of `IsNetDirty()`.
    pub fn is_net_dirty(&self, net: i32) -> bool {
        if net < 0 {
            return false;
        }

        // (C++ indexes the vector unchecked; a net beyond the table is not dirty)
        self.dirty_nets.get(net as usize).copied().unwrap_or(false)
    }

    /// Port of `ClearDirtyFlags()` (of the nets).
    pub fn clear_dirty_flags(&mut self) {
        for d in &mut self.dirty_nets {
            *d = false;
        }
    }

    /// Port of `GetDirtyClusters()`: the ratsnest clusters whose origin net is dirty.
    pub fn dirty_clusters(&self) -> Clusters {
        let mut clusters = Vec::new();

        for &cl in &self.ratsnest_clusters {
            let net = self.cluster(cl).origin_net();

            if self.is_net_dirty(net) {
                clusters.push(cl);
            }
        }

        clusters
    }

    /// Port of `NetCount()`.
    pub fn net_count(&self) -> i32 {
        self.dirty_nets.len() as i32
    }

    /// `ItemList()`.
    pub fn item_list(&self) -> &CnList {
        &self.item_list
    }

    /// Mutable item list (the ratsnest sets anchor tags and clusters).
    pub fn item_list_mut(&mut self) -> &mut CnList {
        &mut self.item_list
    }

    /// The cluster behind a handle (C++ dereferences the `shared_ptr`).
    pub fn cluster(&self, id: CnClusterId) -> &CnCluster {
        &self.clusters[id.0 as usize]
    }

    /// `m_connClusters`: the clusters of the last `PropagateNets` / `FillIsolatedIslandsMap`.
    pub fn conn_clusters(&self) -> &[CnClusterId] {
        &self.conn_clusters
    }

    /// Takes the `Parent()->SetNetCode( net )` calls recorded since the last call, in call
    /// order; the caller applies them with `Board::set_item_net( id, net, false )`.
    pub fn take_net_changes(&mut self) -> Vec<(ItemId, NetCode)> {
        std::mem::take(&mut self.net_changes)
    }

    /// Port of the `ForEachItem()` template: every item of the list, in order.
    pub fn for_each_item(&self, mut func: impl FnMut(&CnItem)) {
        for item in self.item_list.iter() {
            func(item);
        }
    }

    /// Port of the `ForEachAnchor()` template: every stored anchor of every item, in order.
    pub fn for_each_anchor(&self, mut func: impl FnMut(&crate::items::CnAnchor)) {
        for item in self.item_list.iter() {
            for anchor in item.anchors() {
                func(anchor);
            }
        }
    }

    /// Port of `Clear()`.
    pub fn clear(&mut self) {
        self.ratsnest_clusters.clear();
        self.conn_clusters.clear();
        self.clusters.clear();
        self.item_map.clear();
        self.item_list.clear();
    }

    /// Port of `Remove( BOARD_ITEM* )`: marks the connectivity items of a pad, track, arc,
    /// via, zone or copper shape (or of every pad of a footprint) invalid. `false` for any
    /// other item kind (or a stale handle).
    pub fn remove(&mut self, ctx: &BoardCtx<'_>, item: ItemId) -> bool {
        let Some(item_ref) = ctx.item(item) else {
            return false;
        };

        let mut anything_deleted = false;
        self.mark_item_net_as_dirty(ctx, &item_ref);

        match item_ref.type_() {
            KicadT::PcbFootprintT => {
                let Some(fp) = item.typed() else {
                    return false;
                };

                for (pad, _) in ctx.board.footprint_pads(fp) {
                    // prevent double deletion
                    if let Some(entry) = self.item_map.remove(&pad.into()) {
                        self.mark_items_as_invalid(&entry);
                        anything_deleted = true;
                    }
                }

                self.item_list.set_dirty(true);
            }
            KicadT::PcbPadT
            | KicadT::PcbTraceT
            | KicadT::PcbArcT
            | KicadT::PcbViaT
            | KicadT::PcbZoneT
            | KicadT::PcbShapeT => {
                // prevent double deletion
                if let Some(entry) = self.item_map.remove(&item) {
                    self.mark_items_as_invalid(&entry);
                    self.item_list.set_dirty(true);
                    anything_deleted = true;
                }
            }
            _ => return false,
        }

        // Once we delete an item, it may connect between lists, so mark both as potentially
        // invalid
        if anything_deleted {
            self.item_list.set_has_invalid(true);
        }

        true
    }

    /// Port of `ITEM_MAP_ENTRY::MarkItemsAsInvalid()`.
    fn mark_items_as_invalid(&mut self, entry: &ItemMapEntry) {
        for &id in &entry.items {
            self.item_list.item_mut(id).set_valid(false);
        }
    }

    /// Port of `markItemNetAsDirty()`.
    fn mark_item_net_as_dirty(&mut self, ctx: &BoardCtx<'_>, item: &ItemRef<'_>) {
        if item.is_connected() {
            self.mark_net_as_dirty(item.net_code().map_or(0, |n| n.0));
        } else if item.type_() == KicadT::PcbFootprintT {
            if let Some(fp) = item.id().and_then(|id| id.typed()) {
                let nets: Vec<i32> = ctx
                    .board
                    .footprint_pads(fp)
                    .filter_map(|(pad, _)| ctx.item(pad.into()))
                    .map(|p| p.net_code().map_or(0, |n| n.0))
                    .collect();

                for net in nets {
                    self.mark_net_as_dirty(net);
                }
            }
        }
    }

    /// Port of `Add( BOARD_ITEM* )`: adds the connectivity items of a pad, footprint (its
    /// copper pads), track, arc, via, copper shape or zone (one item per fill outline per
    /// enabled layer, with every outline vertex as anchor), or marks a net dirty
    /// (`PCB_NETINFO_T`). `false` if nothing was added.
    pub fn add(&mut self, ctx: &BoardCtx<'_>, item: ItemId) -> bool {
        if item.kind() == ItemKind::Net {
            self.mark_net_as_dirty(item.index() as i32);
            return true;
        }

        let Some(item_ref) = ctx.item(item) else {
            return false;
        };

        // Don't be fooled by an empty ITEM_MAP_ENTRY auto-created by operator[].
        let already_added = |map: &HashMap<ItemId, ItemMapEntry>, id: ItemId| {
            map.get(&id).is_some_and(|e| !e.items.is_empty())
        };

        match item_ref.type_() {
            KicadT::PcbFootprintT => {
                let Some(fp) = item_ref.as_footprint() else {
                    return false;
                };

                if fp.attributes() & FP_JUST_ADDED != 0 {
                    return false;
                }

                let Some(fp_id) = item.typed() else {
                    return false;
                };

                for (pad_id, pad) in ctx.board.footprint_pads(fp_id) {
                    if !pad.is_on_copper_layer(ctx) {
                        continue;
                    }

                    if already_added(&self.item_map, pad_id.into()) {
                        return false;
                    }

                    let cn = self.item_list.add_pad(ctx, pad_id);
                    self.link_new(pad_id.into(), cn);
                }
            }
            KicadT::PcbPadT => {
                if !item_ref.as_dyn().is_on_copper_layer(ctx) {
                    return false;
                }

                if let Some(fp) = item_ref.parent_footprint().and_then(|f| f.as_footprint()) {
                    if fp.attributes() & FP_JUST_ADDED != 0 {
                        return false;
                    }
                }

                if already_added(&self.item_map, item) {
                    return false;
                }

                let Some(pad_id) = item.typed() else {
                    return false;
                };

                let cn = self.item_list.add_pad(ctx, pad_id);
                self.link_new(item, cn);
            }
            KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT => {
                if already_added(&self.item_map, item) {
                    return false;
                }

                let Some(track_id) = item.typed() else {
                    return false;
                };

                let cn = self.item_list.add_track(ctx, track_id);
                self.link_new(item, cn);
            }
            KicadT::PcbShapeT => {
                if !item_ref.as_dyn().is_on_copper_layer(ctx) {
                    return false;
                }

                if already_added(&self.item_map, item) {
                    return false;
                }

                if !is_copper_layer(item_ref.layer()) {
                    return false;
                }

                let cn = self.item_list.add_shape(ctx, item);
                self.link_new(item, cn);
            }
            KicadT::PcbZoneT => {
                if !item_ref.as_dyn().is_on_copper_layer(ctx) {
                    return false;
                }

                if already_added(&self.item_map, item) {
                    return false;
                }

                let Some(zone_id) = item.typed::<kicad_pcb::zone::Zone>() else {
                    return false;
                };

                self.item_map.insert(item, ItemMapEntry::default());

                // Don't check for connections on layers that only exist in the zone but
                // were disabled in the board
                let mut layerset = ctx.board.enabled_layers();
                layerset &= &item_ref.layer_set();

                for layer in layers_of(&layerset) {
                    for zitem in self.item_list.add_zone(ctx, zone_id, layer) {
                        self.item_map.entry(item).or_default().items.push(zitem);
                    }
                }
            }
            _ => return false,
        }

        self.mark_item_net_as_dirty(ctx, &item_ref);

        true
    }

    /// The `add()` template: `m_itemMap[ brditem ] = ITEM_MAP_ENTRY( item )` (an empty entry
    /// for a null item).
    fn link_new(&mut self, parent: ItemId, cn: Option<CnItemId>) {
        let mut entry = ItemMapEntry::default();
        entry.items.extend(cn);
        self.item_map.insert(parent, entry);
    }

    /// Port of `RemoveInvalidRefs()`: every item drops its connections to invalid items.
    pub fn remove_invalid_refs(&mut self) {
        for ii in 0..self.item_list.ids().len() {
            let id = self.item_list.at(ii);
            let list = &self.item_list;
            let invalid: Vec<CnItemId> = list
                .item(id)
                .connected_items()
                .iter()
                .copied()
                .filter(|&c| !list.item(c).valid())
                .collect();

            if !invalid.is_empty() {
                self.item_list
                    .item_mut(id)
                    .remove_invalid_refs(|c| !invalid.contains(&c));
            }
        }
    }

    /// Port of `searchConnections()`: garbage collection, the contact search of every dirty
    /// item against its r-tree neighbours (`CN_VISITOR`), then the deferred via-net rule.
    ///
    /// `m_parentConnectivityData->RemoveInvalidRefs()` also cleans the ratsnest of every net;
    /// that part belongs to the `CONNECTIVITY_DATA` port (CN-04), which must call it before
    /// it reads ratsnest anchors after an incremental update.
    fn search_connections(&mut self, ctx: &BoardCtx<'_>) {
        self.remove_invalid_refs();
        // The garbage stays in the arena (the C++ deletes it); nothing refers to it any more.
        let _garbage = self.item_list.remove_invalid_items();

        let dirty_items: Vec<CnItemId> = self
            .item_list
            .iter()
            .filter(|i| i.dirty())
            .map(CnItem::id)
            .collect();

        if self.item_list.is_dirty() {
            // Collect deferred net code changes. Vias connected to zones have their net codes
            // updated after the search completes, but only if the via has no higher-priority
            // connections (tracks, pads).
            let mut deferred_net_codes: Vec<(CnItemId, i32)> = Vec::new();

            for &item in &dirty_items {
                // The C++ visits the r-tree candidates with the visitor directly; the visit
                // order does not matter (connections are kept sorted, the deferred net
                // codes are grouped), so the candidates are collected first.
                let mut candidates = Vec::new();
                self.item_list.find_nearby(item, |&c| {
                    candidates.push(c);
                    true
                });

                for candidate in candidates {
                    cn_visit(
                        ctx,
                        &mut self.item_list,
                        item,
                        candidate,
                        &mut deferred_net_codes,
                    );
                }
            }

            // Apply deferred zone net changes, but only for vias that have no non-zone
            // connections.  Tracks and pads take priority over zones for net assignment;
            // cluster-based propagation will handle those vias.
            //
            // A single via can touch zones of several different nets (e.g. a through via
            // crossing a GND plane and a power plane).  The order in which those candidate
            // nets are collected depends on the parallel search and is not stable across
            // connectivity rebuilds, so we must not simply pick the first one: doing so makes
            // the via's net flip arbitrarily on every rebuild (i.e. on every undo/redo).
            // Instead, if the via's existing net matches any zone it touches, we keep it.
            // This preserves a deliberately-assigned net and only falls back to a
            // deterministic choice (lowest net code) when the current net no longer touches
            // any zone.
            //
            // (C++ sorts by item pointer and walks the groups; the groups are independent, so
            // grouping by item id gives the same result.)
            let mut groups: BTreeMap<CnItemId, Vec<i32>> = BTreeMap::new();

            for (item, net) in deferred_net_codes {
                groups.entry(item).or_default().push(net);
            }

            for (cn_item, nets) in groups {
                let item = self.item_list.item(cn_item);

                if item
                    .connected_items()
                    .iter()
                    .any(|&c| self.item_list.item(c).parent_type() != KicadT::PcbZoneT)
                {
                    // Connected to a track or pad, so cluster propagation owns the net.
                    continue;
                }

                let existing_net = item.net();
                let mut keep_existing = false;
                let mut best_net = i32::MAX;

                for &entry in &nets {
                    if entry == existing_net {
                        keep_existing = true;
                        break;
                    }

                    best_net = best_net.min(entry);
                }

                if !keep_existing {
                    self.set_parent_net_code(ctx, cn_item, best_net);
                }
            }
        }

        self.item_list.clear_dirty_flags();
    }

    /// `item->Parent()->SetNetCode( aNetCode )` (`BOARD_CONNECTED_ITEM::SetNetCode` with the
    /// board lookup: 0 for an item off copper, -1 for an unknown net code): updates the cached
    /// net of every connectivity item of the parent and records the change for the caller.
    fn set_parent_net_code(&mut self, ctx: &BoardCtx<'_>, cn_item: CnItemId, net: i32) {
        let parent = self.item_list.item(cn_item).parent();
        let mut code = net;

        if let Some(item) = ctx.item(parent) {
            if !item.as_dyn().is_on_copper_layer(ctx) {
                code = 0;
            }

            // if aNetCode < 0 (typically NETINFO_LIST::FORCE_ORPHANED) or no parent board,
            // set the m_netinfo to the dummy NETINFO_LIST::ORPHANED
            if code >= 0 && item.has_board() {
                if ctx.board.find_net(NetCode(code)).is_none() {
                    code = NetCode::ORPHANED.0;
                }
            } else {
                code = NetCode::UNCONNECTED.0;
            }
        }

        let items = match self.item_map.get(&parent) {
            Some(e) if !e.items.is_empty() => e.items.clone(),
            _ => vec![cn_item],
        };

        for id in items {
            self.item_list.item_mut(id).set_net(code);
        }

        self.net_changes.push((parent, NetCode(net)));
    }

    /// Port of `SearchClusters( aMode )`: zones are excluded in `CSM_PROPAGATE` mode.
    pub fn search_clusters(&mut self, ctx: &BoardCtx<'_>, mode: ClusterSearchMode) -> Clusters {
        self.search_clusters_ex(ctx, mode, mode == ClusterSearchMode::Propagate, -1)
    }

    /// Port of `SearchClusters( aMode, aExcludeZones, aSingleNet )`: the connected components
    /// of the participating items (valid; with a net > 0 and, if `single_net >= 0`, of that
    /// net unless `mode` is `CSM_PROPAGATE`; no zones if `exclude_zones`), sorted by origin net
    /// with the libstdc++ `std::sort` (unstable, connectivity-zones.md R-3).
    pub fn search_clusters_ex(
        &mut self,
        ctx: &BoardCtx<'_>,
        mode: ClusterSearchMode,
        exclude_zones: bool,
        single_net: i32,
    ) -> Clusters {
        let within_any_net = mode != ClusterSearchMode::Propagate;

        let mut clusters: Clusters = Vec::new();

        if self.item_list.is_dirty() {
            self.search_connections(ctx);
        }

        let list = &self.item_list;
        let size = list.size() as usize;

        // Numbering stays local rather than stamped on the items because several DRC threads
        // can be inside this function at once, each with a different view of who takes part
        let mut members: Vec<CnItemId> = Vec::with_capacity(size);
        let mut member_of: Vec<i32> = vec![-1; size];

        // aSingleNet restricts which items may seed a cluster, not which may be reached, since
        // propagation mode crosses nets once started.  Hold the net test back for the emit
        let mut selected: Vec<bool> = Vec::with_capacity(size);

        for item in list.iter() {
            let participates = item.valid()
                && !(within_any_net && item.net() <= 0)
                && !(within_any_net && single_net >= 0 && item.net() != single_net)
                && !(exclude_zones && item.parent_type() == KicadT::PcbZoneT);

            if participates {
                member_of[item.list_index() as usize] = members.len() as i32;
                members.push(item.id());
                selected.push(single_net < 0 || item.net() == single_net);
            }
        }

        // Every member of a cluster carries the cluster's net, so gating a neighbour on the near
        // end's net selects the same edges as gating it on the component root's
        let mut forest = KiUnionFind::new(members.len());

        for (ii, &member) in members.iter().enumerate() {
            let item = list.item(member);

            for &neighbour_id in item.connected_items() {
                let neighbour = list.item(neighbour_id);
                let list_index = neighbour.list_index();

                // An adjacency that outlived its item would otherwise index the map out of
                // range
                if list_index < 0 || list_index >= member_of.len() as i32 {
                    continue;
                }

                let index = member_of[list_index as usize];

                if index < 0 {
                    continue;
                }

                if within_any_net && neighbour.net() != item.net() {
                    continue;
                }

                forest.unite(ii, index as usize);
            }
        }

        // Index order makes cluster contents a function of the item list alone, so the origin
        // pad elected on a cluster spanning several nets no longer follows the heap layout
        let mut cluster_of: Vec<i32> = vec![-1; members.len()];
        let mut cluster_selected: Vec<bool> = Vec::new();

        for ii in 0..members.len() {
            let root = forest.find_compress(ii);

            if cluster_of[root] < 0 {
                cluster_of[root] = clusters.len() as i32;
                clusters.push(CnClusterId(self.clusters.len() as u32));
                self.clusters.push(CnCluster::new());
                cluster_selected.push(false);
            }

            let cl = clusters[cluster_of[root] as usize];
            self.clusters[cl.0 as usize].add(ctx, &self.item_list, members[ii]);

            if selected[ii] {
                cluster_selected[cluster_of[root] as usize] = true;
            }
        }

        // A component reached only from items outside aSingleNet was never a result
        if single_net >= 0 {
            clusters = clusters
                .into_iter()
                .zip(cluster_selected)
                .filter_map(|(c, sel)| sel.then_some(c))
                .collect();
        }

        let arena = &self.clusters;
        std_sort(&mut clusters, |a, b| {
            arena[a.0 as usize].origin_net() < arena[b.0 as usize].origin_net()
        });

        clusters
    }

    /// Port of `Build( BOARD* )`: the connectivity items of the whole board in the C++ order:
    /// one `CN_ZONE_LAYER` per fill outline of every copper zone (enabled copper layers, LSET
    /// order; built without anchors, connectivity-zones.md R-5), then every track / arc / via,
    /// every pad of every footprint, every copper `PCB_SHAPE` of the board drawings.
    ///
    /// The fills must have been triangulated (`BOARD::CacheTriangulation`) by the caller.
    pub fn build(&mut self, ctx: &BoardCtx<'_>) {
        let board = ctx.board;

        // Nothing queries the index until searchConnections(), so index the board in one
        // packed load rather than run the R*-tree insertion heuristic once per item
        self.item_list.start_bulk_add();

        // Generate CN_ZONE_LAYERs for each island on each layer of each zone
        let mut zitems: Vec<CnItem> = Vec::new();

        for (zone_id, zone) in board.zones() {
            if !zone.is_on_copper_layer(ctx) {
                continue;
            }

            let zone_item: ItemId = zone_id.into();
            self.item_map.insert(zone_item, ItemMapEntry::default());

            if let Some(zone_ref) = ctx.item(zone_item) {
                self.mark_item_net_as_dirty(ctx, &zone_ref);
            }

            // Don't check for connections on layers that only exist in the zone but
            // were disabled in the board
            let mut layerset = board.enabled_layers();
            layerset &= &zone.layer_set(ctx);
            layerset &= Lset::all_cu_mask();

            for layer in layers_of(&layerset) {
                // GetFilledPolysList( layer ) is std::map::at (throws for a missing layer)
                let Some(polys) = zone.fill().filled_polys_list.get(&layer) else {
                    continue;
                };

                for j in 0..polys.outline_count() {
                    if let Some(zitem) = CnItem::new_zone_layer(ctx, zone_id, layer, j) {
                        zitems.push(zitem);
                    }
                }
            }
        }

        // Generate RTrees for CN_ZONE_LAYER items (in parallel in C++)
        for zitem in &mut zitems {
            if let Some(zl) = zitem.zone_layer_mut() {
                zl.build_indexes(ctx);
            }
        }

        // Add CN_ZONE_LAYERS, tracks, and pads to connectivity
        for zitem in zitems {
            let parent = zitem.parent();
            let id = self.item_list.add_zone_layer(zitem);
            self.item_map.entry(parent).or_default().items.push(id);
        }

        for (track_id, _) in board.tracks() {
            self.add(ctx, track_id.into());
        }

        for (fp_id, _) in board.footprints() {
            for (pad_id, _) in board.footprint_pads(fp_id) {
                self.add(ctx, pad_id.into());
            }
        }

        for (id, drawing) in board.drawings() {
            // dynamic_cast<PCB_SHAPE*> also accepts text boxes (and the tables' cells are not
            // drawings), which Add() then rejects by type; only PCB_SHAPE_T is ever added.
            let _ = drawing;

            if let Some(item) = ctx.item(id) {
                if item.type_() == KicadT::PcbShapeT && item.as_dyn().is_on_copper_layer(ctx) {
                    self.add(ctx, id);
                }
            }
        }

        self.item_list.finish_bulk_add();
    }

    /// Port of `propagateConnections()`: in every cluster that is not conflicting and has a
    /// valid origin net, every valid item that can change net and is on another net takes the
    /// origin net.
    fn propagate_connections(&mut self, ctx: &BoardCtx<'_>) {
        for ci in 0..self.conn_clusters.len() {
            let cl = self.conn_clusters[ci];
            let cluster = &self.clusters[cl.0 as usize];

            if cluster.is_conflicting() {
                // Conflicting pads in cluster: we don't know the user's intent so best to do
                // nothing.
                continue;
            }

            if !cluster.has_valid_net() {
                // connected to unused net
                continue;
            }

            // Propagate from the origin (will be a pad if there are any, or another item if
            // there are no pads).
            let origin_net = cluster.origin_net();
            let items = cluster.items().to_vec();

            for item_id in items {
                let item = self.item_list.item(item_id);

                if item.valid() && item.can_change_net() && item.net() != origin_net {
                    let old = item.net();
                    self.mark_net_as_dirty(old);
                    self.mark_net_as_dirty(origin_net);
                    self.set_parent_net_code(ctx, item_id, origin_net);
                }
            }
        }
    }

    /// Port of `PropagateNets()`: jumper pads, `SearchClusters( CSM_PROPAGATE )`, then the
    /// propagation of the origin nets (the changes are recorded for the caller, see
    /// [`CnConnectivityAlgo::take_net_changes`]).
    pub fn propagate_nets(&mut self, ctx: &BoardCtx<'_>) {
        self.update_jumper_pads(ctx);
        self.conn_clusters = self.search_clusters(ctx, ClusterSearchMode::Propagate);
        self.propagate_connections(ctx);
    }

    /// Port of `FillIsolatedIslandsMap()`: for every (zone, layer) of `map`, the fill outlines
    /// that belong to an orphaned cluster (no pad) and those with a single connection.
    ///
    /// Unless `connectivity_already_rebuilt`, the zones are first removed and re-added with
    /// their current fills (incremental path: new items with anchors at the end of the list).
    pub fn fill_isolated_islands_map(
        &mut self,
        ctx: &BoardCtx<'_>,
        map: &mut IsolatedIslandsMap,
        connectivity_already_rebuilt: bool,
    ) {
        if !connectivity_already_rebuilt {
            for &zone in map.keys() {
                self.remove(ctx, zone.into());
                self.add(ctx, zone.into());
            }
        }

        self.conn_clusters = self.search_clusters(ctx, ClusterSearchMode::ConnectivityCheck);

        // Bucket the zone items in one pass. Keep the cluster and item order, which sets the
        // recorded outline order. (item, orphaned) per layer per zone.
        let mut zone_items: HashMap<ItemId, BTreeMap<PcbLayerId, Vec<(CnItemId, bool)>>> =
            HashMap::new();

        for &zone in map.keys() {
            zone_items.entry(zone.into()).or_default();
        }

        for &cl in &self.conn_clusters {
            let cluster = &self.clusters[cl.0 as usize];
            let orphaned = cluster.is_orphaned();

            for &item_id in cluster.items() {
                let item = self.item_list.item(item_id);

                if let Some(layers) = zone_items.get_mut(&item.parent()) {
                    layers
                        .entry(item.board_layer())
                        .or_default()
                        .push((item_id, orphaned));
                }
            }
        }

        for (&zone, zone_islands) in map.iter_mut() {
            let layer_items = zone_items.entry(zone.into()).or_default();
            let fills = ctx
                .item(zone.into())
                .and_then(|z| z.as_zone())
                .map(|z| &z.fill().filled_polys_list);

            for (&layer, layer_islands) in zone_islands.iter_mut() {
                // GetFilledPolysList( layer )->IsEmpty() (std::map::at in C++)
                let empty = fills
                    .and_then(|f| f.get(&layer))
                    .is_none_or(|p| p.is_empty());

                if empty {
                    continue;
                }

                let entries = layer_items.get(&layer);
                let not_in_connectivity = entries.is_none();

                if let Some(entries) = entries {
                    for &(item_id, orphaned) in entries {
                        let item = self.item_list.item(item_id);
                        let subpoly = item.zone_layer().map_or(0, |zl| zl.subpoly_index());

                        if orphaned {
                            layer_islands.isolated_outlines.push(subpoly);
                        } else if item.has_single_connection(&self.item_list) {
                            layer_islands.single_connection_outlines.push(subpoly);
                        }
                    }
                }

                // Non-copper zones (silk, mask, etc.) are never added to the connectivity
                // graph, so notInConnectivity is always true for them.  Without the
                // IsCopperLayer guard outline 0 of every non-copper multi-island fill gets
                // dropped on every refill (issue 24089).
                if not_in_connectivity && is_copper_layer(layer) {
                    layer_islands.isolated_outlines.push(0);
                }
            }
        }
    }

    /// Port of `GetClusters()`: `SearchClusters( CSM_RATSNEST )`, kept as the ratsnest
    /// clusters.
    pub fn get_clusters(&mut self, ctx: &BoardCtx<'_>) -> &[CnClusterId] {
        self.ratsnest_clusters = self.search_clusters(ctx, ClusterSearchMode::Ratsnest);
        &self.ratsnest_clusters
    }

    /// The ratsnest clusters of the last [`CnConnectivityAlgo::get_clusters`].
    pub fn ratsnest_clusters(&self) -> &[CnClusterId] {
        &self.ratsnest_clusters
    }

    /// Port of `MarkNetAsDirty()`: grows the dirty-net table to `net + 1` (every new entry
    /// dirty) and marks `net` dirty; negative codes are ignored.
    pub fn mark_net_as_dirty(&mut self, net: i32) {
        if net < 0 {
            return;
        }

        if self.dirty_nets.len() as i32 <= net {
            let mut last_net = self.dirty_nets.len() as i32 - 1;

            if last_net < 0 {
                last_net = 0;
            }

            self.dirty_nets.resize(net as usize + 1, false);

            for i in last_net..net + 1 {
                self.dirty_nets[i as usize] = true;
            }
        }

        self.dirty_nets[net as usize] = true;
    }

    /// Port of `updateJumperPads()`: pads of one footprint with the same number are connected
    /// when the footprint's duplicate pad numbers are jumpers, and the pads of every jumper
    /// group are connected.
    ///
    /// The double loops are `i in 0..n, j in 1..n`, so a pad at `i == j >= 1` is connected to
    /// itself (reproduced). Footprints are visited in `FootprintId` order (C++: a map keyed
    /// by address; each footprint is independent).
    fn update_jumper_pads(&mut self, ctx: &BoardCtx<'_>) {
        // Map of footprint -> map of pad number -> list of CN_ITEMs for pads with that number
        let mut pads_by_footprint: BTreeMap<
            Option<kicad_pcb::ids::FootprintId>,
            BTreeMap<String, Vec<CnItemId>>,
        > = BTreeMap::new();

        for item in self.item_list.iter() {
            if !item.valid() || item.parent_type() != KicadT::PcbPadT {
                continue;
            }

            let Some(pad_ref) = ctx.item(item.parent()) else {
                continue;
            };
            let Some(pad) = pad_ref.as_pad() else {
                continue;
            };

            pads_by_footprint
                .entry(pad_ref.parent_footprint_id())
                .or_default()
                .entry(pad.number().to_string())
                .or_default()
                .push(item.id());
        }

        let mut pairs: Vec<(CnItemId, CnItemId)> = Vec::new();

        let connect_all = |pads: &[CnItemId], pairs: &mut Vec<(CnItemId, CnItemId)>| {
            for i in 0..pads.len() {
                for j in 1..pads.len() {
                    pairs.push((pads[i], pads[j]));
                }
            }
        };

        for (footprint, pads_map) in &pads_by_footprint {
            // C++ dereferences the footprint pointer; a board pad always has one.
            let Some(fp) = footprint
                .and_then(|f| ctx.item(f.into()))
                .and_then(|f| f.as_footprint())
            else {
                continue;
            };

            if fp.duplicate_pad_numbers_are_jumpers {
                for pads_list in pads_map.values() {
                    connect_all(pads_list, &mut pairs);
                }
            }

            for group in &fp.jumper_pad_groups.groups {
                let mut to_connect: Vec<CnItemId> = Vec::new();

                for pad_number in &group.names {
                    if let Some(pads) = pads_map.get(pad_number) {
                        to_connect.extend_from_slice(pads);
                    }
                }

                connect_all(&to_connect, &mut pairs);
            }
        }

        for (a, b) in pairs {
            self.item_list.item_mut(a).connect(b);
            self.item_list.item_mut(b).connect(a);
        }
    }
}

/// The layers of an `LSET` in iteration order (layer id order).
fn layers_of(set: &Lset) -> Vec<PcbLayerId> {
    let mut layers = Vec::new();
    set.run_on_layers(|l| layers.push(l));
    layers
}

// =================================================================================================
// CN_VISITOR
// =================================================================================================

/// Port of `CN_VISITOR::operator()( aCandidate )` for the visitor of `item`: tests the pair for
/// contact and connects it; via-zone contacts are queued in `deferred` (`m_deferredNetCodes`).
///
/// The C++ skips a candidate that is still dirty and whose address is below the visiting
/// item's. A dirty item stops being dirty when its own search starts (`FindNearby` calls
/// `BBox()`, which clears the flag), so a pair of dirty items is always tested by the visitor
/// that runs later, and also by the earlier one unless the later item has the lower address.
/// The port visits each pair of dirty items once (from the lower `CnItemId`) and makes up for
/// the second visit where it can change the result: the zone tests are symmetric, but
/// `SHAPE::Collide` of two item shapes is not (see [`items_touch_from_both_visitors`]).
pub fn cn_visit(
    ctx: &BoardCtx<'_>,
    list: &mut CnList,
    item: CnItemId,
    candidate: CnItemId,
    deferred: &mut Vec<(CnItemId, i32)>,
) -> bool {
    let cand = list.item(candidate);
    let me = list.item(item);
    let parent_a = cand.parent();
    let parent_b = me.parent();

    if !cand.valid() || !me.valid() {
        return true;
    }

    if parent_a == parent_b {
        return true;
    }

    // Don't connect items in different nets that can't be changed
    if !cand.can_change_net() && !me.can_change_net() && cand.net() != me.net() {
        return true;
    }

    // If both m_item and aCandidate are marked dirty, they will both be searched
    // Since we are reciprocal in our connection, we arbitrarily pick one of the connections
    // to conduct the expensive search
    if cand.dirty() && candidate < item {
        return true;
    }

    let type_a = cand.parent_type();
    let type_b = me.parent_type();

    // We should handle zone-zone connection separately
    if type_a == KicadT::PcbZoneT && type_b == KicadT::PcbZoneT {
        check_zone_zone_connection(list, item, candidate);
        return true;
    }

    if type_a == KicadT::PcbZoneT {
        check_zone_item_connection(ctx, list, candidate, item, deferred);
        return true;
    }

    if type_b == KicadT::PcbZoneT {
        check_zone_item_connection(ctx, list, item, candidate, deferred);
        return true;
    }

    let (Some(ref_a), Some(ref_b)) = (ctx.item(parent_a), ctx.item(parent_b)) else {
        return true;
    };

    let mut common_layers = ref_a.layer_set();
    common_layers &= &ref_b.layer_set();

    if ref_a.has_board() {
        common_layers &= &ctx.board.enabled_layers();
    }

    // A dirty candidate runs (or has run) its own search and tests the pair with the operands
    // the other way round; a candidate that is not dirty is only ever tested from here.
    let both_visitors = cand.dirty();

    for layer in layers_of(&common_layers) {
        let touch = if both_visitors {
            items_touch_from_both_visitors(&ref_a, &ref_b, layer)
        } else {
            items_touch_on_layer(&ref_a, &ref_b, layer)
        };

        if touch {
            list.connect_pair(item, candidate);
            return true;
        }
    }

    true
}

/// Port of `CN_VISITOR::checkZoneItemConnection()`: connects a zone layer item and another item
/// when an anchor of the item lies in the zone fill outline or the item's effective shape on
/// that layer (always flashed for pads and vias) collides with it.
fn check_zone_item_connection(
    ctx: &BoardCtx<'_>,
    list: &mut CnList,
    zone_layer: CnItemId,
    item_id: CnItemId,
    deferred: &mut Vec<(CnItemId, i32)>,
) {
    let zitem = list.item(zone_layer);
    let citem = list.item(item_id);
    let Some(zl) = zitem.zone_layer() else {
        return;
    };
    let layer = zl.layer();
    let Some(item) = ctx.item(citem.parent()) else {
        return;
    };

    if !item.is_on_layer(layer) {
        return;
    }

    // Try quick checks first...
    if let Some(pad) = item.as_pad() {
        if pad.conditionally_flashed(layer)
            && pad.zone_layer_override(layer) == ZoneLayerOverride::ForceNoZoneConnection
        {
            return;
        }

        // Don't connect zones to pads on backdrilled or post-machined layers
        if pad.is_backdrilled_or_post_machined(ctx, layer) {
            return;
        }
    } else if let Some(via) = item.as_via() {
        if via.conditionally_flashed(layer)
            && via.zone_layer_override(layer) == ZoneLayerOverride::ForceNoZoneConnection
        {
            return;
        }

        // Don't connect zones to vias on backdrilled or post-machined layers
        if via.is_backdrilled_or_post_machined(ctx, layer) {
            return;
        }
    }

    let item_type = citem.parent_type();
    let mut connected = false;

    for i in 0..citem.anchor_count() {
        if zl.contains_point(citem.anchor(ctx, i)) {
            connected = true;
            break;
        }
    }

    if !connected {
        if item_type == KicadT::PcbViaT || item_type == KicadT::PcbPadT {
            // As long as the pad/via crosses the zone layer, check for the full effective
            // shape. We check for the overlapping layers above
            let shape = item.effective_shape(
                layer,
                Flashing::AlwaysFlashed,
                DrcConstraintT::NullConstraint,
            );
            connected = zl.collide(&*shape);
        } else {
            let shape =
                item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
            connected = zl.collide(&*shape);
        }
    }

    if connected {
        // We don't propagate nets from zones, so via-zone net changes are deferred and applied
        // only if the via has no higher-priority connections (tracks, pads).
        if item_type == KicadT::PcbViaT && citem.can_change_net() {
            deferred.push((item_id, zitem.net()));
        }

        list.connect_pair(zone_layer, item_id);
    }
}

/// Port of `CN_VISITOR::checkZoneZoneConnection()`: two zone layer items on the same layer
/// connect when a vertex of one outline lies in the other's fill.
fn check_zone_zone_connection(list: &mut CnList, a: CnItemId, b: CnItemId) {
    let item_a = list.item(a);
    let item_b = list.item(b);
    let (Some(zla), Some(zlb)) = (item_a.zone_layer(), item_b.zone_layer()) else {
        return;
    };

    // CN_ZONE_LAYER now caches its own copy of the outline, so we just check if it's
    // non-empty.
    if !zla.has_valid_outline() || !zlb.has_valid_outline() {
        return;
    }

    let box_a = *item_a.bbox();
    let box_b = *item_b.bbox();

    let layer = zla.layer();

    if zlb.layer() != layer {
        return;
    }

    if !box_a.intersects(&box_b) {
        return;
    }

    let mut connect = false;

    for &pt in zla.outline().c_points() {
        if !box_b.contains(pt) {
            continue;
        }

        if zlb.contains_point(pt) {
            connect = true;
            break;
        }
    }

    if !connect {
        for &pt in zlb.outline().c_points() {
            if !box_a.contains(pt) {
                continue;
            }

            if zla.contains_point(pt) {
                connect = true;
                break;
            }
        }
    }

    if connect {
        list.connect_pair(a, b);
    }
}

/// Port of the static `flashingFor()`: pads and vias use their pad shape on layers where they
/// are always flashed, only their hole where they are conditionally flashed.
fn flashing_for(item: &ItemRef<'_>, layer: PcbLayerId) -> Flashing {
    let mut always_flashed = false;

    if let Some(pad) = item.as_pad() {
        always_flashed = !pad.conditionally_flashed(layer);
    } else if let Some(via) = item.as_via() {
        always_flashed = !via.conditionally_flashed(layer);
    }

    if always_flashed {
        Flashing::AlwaysFlashed
    } else {
        Flashing::NeverFlashed
    }
}

/// Port of `ItemsTouchOnLayer()`: the effective shapes of two connected items on `layer`
/// collide (clearance 0: touching counts).
pub fn items_touch_on_layer(a: &ItemRef<'_>, b: &ItemRef<'_>, layer: PcbLayerId) -> bool {
    let shape_a = a.effective_shape(
        layer,
        flashing_for(a, layer),
        DrcConstraintT::NullConstraint,
    );
    let shape_b = b.effective_shape(
        layer,
        flashing_for(b, layer),
        DrcConstraintT::NullConstraint,
    );

    shape_a.collide(&*shape_b, 0, None, None)
}

/// `ItemsTouchOnLayer( a, b, layer ) || ItemsTouchOnLayer( b, a, layer )`: what the two
/// `CN_VISITOR`s of a pair of dirty items compute between them in C++ (each visitor passes its
/// candidate first).
///
/// The order matters because `SHAPE::Collide` is not symmetric: for two `SHAPE_SEGMENT`s the
/// minimum distance is `( wA + 1 ) / 2 + wB / 2`, so two tracks whose axes are exactly that far
/// apart touch only with the odd-width one as the first operand (1 nm band, odd widths only).
/// The reference build connects such a pair whichever of the two comes first in the board
/// (`rev_shapes` golden); on the rare pair that only one C++ visitor tests (the later item has
/// the lower heap address) the C++ result depends on the heap, see
/// `rust/docs/design/review-connectivity.md` F-2.
fn items_touch_from_both_visitors(a: &ItemRef<'_>, b: &ItemRef<'_>, layer: PcbLayerId) -> bool {
    let shape_a = a.effective_shape(
        layer,
        flashing_for(a, layer),
        DrcConstraintT::NullConstraint,
    );
    let shape_b = b.effective_shape(
        layer,
        flashing_for(b, layer),
        DrcConstraintT::NullConstraint,
    );

    shape_a.collide(&*shape_b, 0, None, None) || shape_b.collide(&*shape_a, 0, None, None)
}
