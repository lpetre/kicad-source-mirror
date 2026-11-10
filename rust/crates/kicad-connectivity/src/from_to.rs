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

//! Port of `pcbnew/connectivity/from_to_cache.h` and `from_to_cache.cpp` (`FROM_TO_CACHE`):
//! the paths between two pads (named `REF-PAD`, or `REF` alone, with `*` / `?` wildcards)
//! over the connectivity graph, cached per `(from, to)` wildcard pair, behind the rule
//! function `fromTo('U1-1', 'U2-3')`.
//!
//! Differences in structure (`PORT_STATUS.md`, CN-05):
//!
//! - The cache lives in [`crate::ConnectivityData`] behind a `Mutex` (C++: a
//!   `std::recursive_mutex` member) so that `IsOnFromToPath` can be reached through
//!   `ConnectivityQuery` (`&self`).
//! - `cacheFromToPaths` calls `CONNECTIVITY_DATA::GetConnectedItems( pad, EXCLUDE_ZONES )`,
//!   whose cluster search needs `&mut` in the port. [`FromToCache::rebuild`] (which has
//!   `&mut` access, like every C++ caller of `Rebuild`) therefore runs that cluster search once
//!   for all nets and keeps the clusters; the query then reads the cluster of the pad. This is
//!   the same result as the per-pad call: with `aSingleNet = -1` every item of a net > 0 takes
//!   part and edges only join items of the same net, so the cluster of a pad has exactly the
//!   members (in the same list order) the search restricted to the pad's net gives. The
//!   connectivity must not change between `rebuild` and the queries (it does not during a DRC
//!   run, which is where C++ rebuilds the cache).
//! - `CN_ITEM::ConnectedItems()` is ordered by heap address in C++ and by list order in the
//!   port (connectivity-zones.md R-1). The breadth-first search of `uniquePathBetweenNodes`
//!   visits neighbours in that order; see [`unique_path_between_nodes`] for where this can
//!   change a result.

use std::collections::{BTreeSet, HashMap, VecDeque};

use kicad_core::KicadT;
use kicad_core::string_utils::wild_compare_string;
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::ItemId;

use crate::algo::{ClusterSearchMode, CnConnectivityAlgo};
use crate::items::{CnItemId, CnList};

/// Port of `FROM_TO_CACHE::FT_ENDPOINT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FtEndpoint {
    /// `name`: `REF-PAD` or `REF`.
    pub name: String,
    /// `parent`: the pad.
    pub parent: ItemId,
}

/// Port of `FROM_TO_CACHE::FT_PATH`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FtPath {
    /// `net`: the net code of the `from` pad.
    pub net: i32,
    /// `from`
    pub from: Option<ItemId>,
    /// `to` (`nullptr` when no or more than one pad matched).
    pub to: Option<ItemId>,
    /// `fromName`
    pub from_name: String,
    /// `toName`
    pub to_name: String,
    /// `fromWildcard`
    pub from_wildcard: String,
    /// `toWildcard`
    pub to_wildcard: String,
    /// `isUnique`
    pub is_unique: bool,
    /// `pathItems` (C++ `std::set` of pointers; only membership and equality are used).
    pub path_items: BTreeSet<ItemId>,
}

/// `PATH_STATUS` of `from_to_cache.cpp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStatus {
    /// `PS_OK`
    Ok,
    /// `PS_MULTIPLE_PATHS`
    MultiplePaths,
    /// `PS_NO_PATH`
    NoPath,
}

/// Port of `FROM_TO_CACHE`.
#[derive(Clone, Debug, Default)]
pub struct FromToCache {
    /// `m_board != nullptr`: false until the first [`FromToCache::rebuild`] (C++ constructs
    /// the cache without a board; `IsOnFromToPath` then answers false).
    has_board: bool,
    /// `m_ftEndpoints`
    ft_endpoints: Vec<FtEndpoint>,
    /// The indices into `ft_endpoints` of each pad, in push order (lookup only; replaces the
    /// linear `pad == endpoint.parent` scan without changing the visit order).
    endpoints_of_pad: HashMap<ItemId, Vec<usize>>,
    /// `m_ftPaths`
    ft_paths: Vec<FtPath>,
    /// `GetConnectedItems( pad, EXCLUDE_ZONES )` for every pad: the clusters of the
    /// connectivity check search without zones (valid members' parents, cluster order).
    clusters: Vec<Vec<ItemId>>,
    /// The cluster of each item in `clusters` (lookup only).
    cluster_of: HashMap<ItemId, usize>,
}

impl FromToCache {
    /// Port of `FROM_TO_CACHE( nullptr )`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `FROM_TO_CACHE::Rebuild( aBoard )`, plus the cluster snapshot that stands for
    /// the `GetConnectedItems` calls of `cacheFromToPaths` (module comment). `algo` is the
    /// board's connectivity (`m_board->GetConnectivity()->GetConnectivityAlgo()`).
    pub fn rebuild(&mut self, ctx: &BoardCtx<'_>, algo: &mut CnConnectivityAlgo) {
        self.has_board = true;
        self.build_endpoint_list(ctx);
        self.ft_paths.clear();

        self.clusters.clear();
        self.cluster_of.clear();

        let clusters = algo.search_clusters_ex(ctx, ClusterSearchMode::ConnectivityCheck, true, -1);
        let list = algo.item_list();

        for cl in clusters {
            let index = self.clusters.len();
            let mut members = Vec::new();

            for &citem in algo.cluster(cl).items() {
                let citem = list.item(citem);

                if citem.valid() {
                    members.push(citem.parent());
                    self.cluster_of.entry(citem.parent()).or_insert(index);
                }
            }

            self.clusters.push(members);
        }
    }

    /// Port of `FROM_TO_CACHE::buildEndpointList`.
    fn build_endpoint_list(&mut self, ctx: &BoardCtx<'_>) {
        let board: &Board = ctx.board;

        self.ft_endpoints.clear();
        self.endpoints_of_pad.clear();

        for (fp_id, footprint) in board.footprints() {
            let reference = footprint.reference(ctx);

            for (pad_id, pad) in board.footprint_pads(fp_id) {
                let parent: ItemId = pad_id.into();
                let slots = self.endpoints_of_pad.entry(parent).or_default();

                slots.push(self.ft_endpoints.len());
                self.ft_endpoints.push(FtEndpoint {
                    name: format!("{reference}-{}", pad.number()),
                    parent,
                });

                slots.push(self.ft_endpoints.len());
                self.ft_endpoints.push(FtEndpoint {
                    name: reference.clone(),
                    parent,
                });
            }
        }
    }

    /// `m_ftEndpoints`.
    pub fn endpoints(&self) -> &[FtEndpoint] {
        &self.ft_endpoints
    }

    /// `m_ftPaths`, in the order they were cached.
    pub fn paths(&self) -> &[FtPath] {
        &self.ft_paths
    }

    /// `GetConnectedItems( aItem, EXCLUDE_ZONES )` from the snapshot of [`Self::rebuild`].
    fn connected_items(&self, item: ItemId) -> &[ItemId] {
        match self.cluster_of.get(&item) {
            Some(&index) => &self.clusters[index],
            None => &[],
        }
    }

    /// `GetReference() + "-" + GetNumber()` of a pad (`GetParentFootprint()` is never null
    /// for the endpoints, which all come from footprints).
    fn pad_name(ctx: &BoardCtx<'_>, pad: ItemId) -> String {
        let Some(item) = ctx.item(pad) else {
            return String::new();
        };

        let reference = item
            .parent_footprint()
            .and_then(|fp| fp.as_footprint().map(|f| f.reference(ctx)))
            .unwrap_or_default();
        let number = item.as_pad().map(|p| p.number().to_string()).unwrap_or_default();

        format!("{reference}-{number}")
    }

    /// Port of `FROM_TO_CACHE::cacheFromToPaths`: caches the paths of every pad matching
    /// `from` to the single pad of its cluster matching `to`; returns the number of new paths.
    fn cache_from_to_paths(
        &mut self,
        ctx: &BoardCtx<'_>,
        algo: &CnConnectivityAlgo,
        from: &str,
        to: &str,
    ) -> i32 {
        let mut paths: Vec<FtPath> = Vec::new();

        for endpoint in &self.ft_endpoints {
            if wild_compare_string(from, &endpoint.name, false) {
                paths.push(FtPath {
                    net: ctx
                        .item(endpoint.parent)
                        .and_then(|i| i.net_code())
                        .map_or(0, |n| n.0),
                    from: Some(endpoint.parent),
                    to: None,
                    ..FtPath::default()
                });
            }
        }

        for path in &mut paths {
            let mut count = 0;
            let path_from = path.from.expect("from is set above");

            let from_name = Self::pad_name(ctx, path_from);

            let pad_candidates = self.connected_items(path_from);

            for &pitem in pad_candidates {
                if pitem == path_from {
                    continue;
                }

                if ctx.item(pitem).map(|i| i.type_()) != Some(KicadT::PcbPadT) {
                    continue;
                }

                let pad = pitem;

                let to_name = Self::pad_name(ctx, pad);

                // for( const FT_ENDPOINT& endpoint : m_ftEndpoints ) if( pad == endpoint.parent )
                let Some(slots) = self.endpoints_of_pad.get(&pad) else {
                    continue;
                };

                for &slot in slots {
                    let endpoint = &self.ft_endpoints[slot];

                    if wild_compare_string(to, &endpoint.name, false) {
                        count += 1;
                        let to_pad = endpoint.parent;

                        path.to = Some(to_pad);
                        path.from_name = from_name.clone();
                        path.to_name = to_name.clone();
                        path.from_wildcard = from.to_string();
                        path.to_wildcard = to.to_string();

                        if count >= 2 {
                            // fixme: report this somewhere?
                            path.to = None;
                        }
                    }
                }
            }
        }

        let mut new_paths = 0;
        let list = algo.item_list();

        for mut path in paths {
            let (Some(from_pad), Some(to_pad)) = (path.from, path.to) else {
                continue;
            };

            // cnAlgo->ItemEntry( pad ).GetItems().front(): both pads are in a cluster here,
            // so both have connectivity items
            let first = |pad: ItemId| {
                algo.item_entry(pad)
                    .and_then(|entry| entry.items.first().copied())
            };

            let (Some(cn_from), Some(cn_to)) = (first(from_pad), first(to_pad)) else {
                continue;
            };

            let (result, upath) = unique_path_between_nodes(list, cn_from, cn_to);

            path.is_unique = result == PathStatus::Ok;

            if result == PathStatus::NoPath {
                continue;
            }

            for &item in &upath {
                path.path_items.insert(list.item(item).parent());
            }

            self.ft_paths.push(path);
            new_paths += 1;
        }

        new_paths
    }

    /// Port of `FROM_TO_CACHE::IsOnFromToPath`: whether `item` lies on a cached path between
    /// pads matching `from` and `to`, caching the paths of that pair on first use. `item` is
    /// `None` for a value without a handle (it is on no path). `algo` must be the connectivity
    /// the cache was rebuilt from.
    pub fn is_on_from_to_path(
        &mut self,
        ctx: &BoardCtx<'_>,
        algo: &CnConnectivityAlgo,
        item: Option<ItemId>,
        from: &str,
        to: &str,
    ) -> bool {
        let mut n_from_tos_found = 0;

        if !self.has_board {
            return false;
        }

        for _attempt in 0..2 {
            // item already belongs to path
            for ft_path in &self.ft_paths {
                if from == ft_path.from_wildcard && to == ft_path.to_wildcard {
                    n_from_tos_found += 1;

                    if item.is_some_and(|i| ft_path.path_items.contains(&i)) {
                        return true;
                    }
                }
            }

            if n_from_tos_found == 0 {
                self.cache_from_to_paths(ctx, algo, from, to);
            } else {
                return false;
            }
        }

        false
    }

    /// Port of `FROM_TO_CACHE::QueryFromToPath`: the first cached path whose items are exactly
    /// `items`.
    pub fn query_from_to_path(&self, items: &BTreeSet<ItemId>) -> Option<&FtPath> {
        self.ft_paths.iter().find(|p| p.path_items == *items)
    }
}

/// Port of the static `isVertexVisited` of `from_to_cache.cpp`.
fn is_vertex_visited(v: CnItemId, path: &[CnItemId]) -> bool {
    path.iter().any(|&u| u == v)
}

/// Port of the static `uniquePathBetweenNodes` of `from_to_cache.cpp`: a breadth-first search
/// over paths from `u` to `v` that does not extend a path by a vertex that is on it or on any
/// queued path. Returns the status and `outPath` (the first path found, or the second one for
/// [`PathStatus::MultiplePaths`]; empty for [`PathStatus::NoPath`]).
///
/// Pointer-order site: the neighbours of a vertex are visited in `ConnectedItems()` order,
/// which is heap-address order in C++ and list order here. When the graph between `u` and `v`
/// has cycles (three tracks meeting in one point are already a triangle), that order decides
/// which of two equally long paths is queued first and whether a vertex is still "queued"
/// when another path reaches it, so the path items (and `isUnique`) can differ. The result in
/// the reference build is then not stable from run to run either; the oracle replays the C++
/// search in list order to compare (`tests/oracle/ft_cache_oracle.cpp`).
pub fn unique_path_between_nodes(
    list: &CnList,
    u: CnItemId,
    v: CnItemId,
) -> (PathStatus, Vec<CnItemId>) {
    let mut q: VecDeque<Vec<CnItemId>> = VecDeque::new();
    let mut out_path: Vec<CnItemId> = Vec::new();

    let mut path_found = false;
    q.push_back(vec![u]);

    while let Some(path) = q.pop_front() {
        let last = *path.last().expect("paths are never empty");

        if last == v {
            out_path = path.clone();

            if path_found {
                return (PathStatus::MultiplePaths, out_path);
            }

            path_found = true;
        }

        for &ci in list.item(last).connected_items() {
            let mut vertex_visited = is_vertex_visited(ci, &path);

            for p in &q {
                if is_vertex_visited(ci, p) {
                    vertex_visited = true;
                    break;
                }
            }

            if !vertex_visited {
                let mut newpath = path.clone();
                newpath.push(ci);
                q.push_back(newpath);
            }
        }
    }

    if path_found {
        (PathStatus::Ok, out_path)
    } else {
        (PathStatus::NoPath, out_path)
    }
}
