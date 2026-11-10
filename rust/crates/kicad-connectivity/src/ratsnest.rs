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

//! Port of `pcbnew/ratsnest/ratsnest_data.h` and `ratsnest_data.cpp` (`RN_NET`: clusters to
//! nodes, triangulation, Kruskal MST, zone anchor optimisation).
//!
//! `RN_NET` keeps `shared_ptr<CN_ANCHOR>`s of the item list; here the nodes are
//! [`CnAnchorRef`]s and every method that reads or writes anchors (tags, clusters) takes the
//! [`CnList`] they live in. Anchors created by `OptimizeRNEdges` live in the edges
//! ([`CnEdgeNode::Optimized`]).

pub mod filter_kruskal;

use std::collections::BTreeMap;

use kicad_geom::geometry::shape::Shape;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::stdsort::std_sort;
use kicad_geom::thirdparty::delaunator::{Delaunator, DelaunatorError, INVALID_INDEX};
use kicad_pcb::ctx::BoardCtx;

use crate::algo::KiUnionFind;
use crate::edge::{CnEdge, CnEdgeNode, CnEdgeTie, anchor_of, anchor_of_mut};
use crate::items::{CnAnchor, CnAnchorRef, CnClusterId, CnItemId, CnList};

/// Key of `m_nodes`: `CN_PTR_CMP` orders by (x, y); libstdc++ `std::multiset::insert` puts an
/// element after the equal ones (upper bound), so the third component is the insertion
/// sequence number.
type NodeKey = (i32, i32, u64);

/// Port of `RN_NET`: the ratsnest of a single net.
#[derive(Clone, Debug)]
pub struct RnNet {
    /// `m_nodes`: `std::multiset<shared_ptr<CN_ANCHOR>, CN_PTR_CMP>`.
    nodes: BTreeMap<NodeKey, CnAnchorRef>,
    /// Insertion counter for [`NodeKey`].
    next_seq: u64,
    /// `m_boardEdges`: edges that make pre-defined connections.
    board_edges: Vec<CnEdge>,
    /// `m_rnEdges`: edges that make the ratsnest for this net.
    rn_edges: Vec<CnEdge>,
    /// `m_dirty`: the ratsnest of this net is invalid and requires an update.
    dirty: bool,
}

impl Default for RnNet {
    fn default() -> Self {
        RnNet::new()
    }
}

/// `RN_NET::kruskalMST` working copy of a candidate edge: the edge, its `orderKey` and its
/// `endpoints` (tags). Positions and tags do not change during the MST, so the key computed
/// once compares exactly like the C++ lambda evaluated on every comparison.
struct MstEdge {
    edge: CnEdge,
    key: (u32, (i32, i32, i32), (i32, i32, i32)),
    ends: (usize, usize),
}

impl RnNet {
    /// Port of `RN_NET()`.
    pub fn new() -> Self {
        RnNet {
            nodes: BTreeMap::new(),
            next_seq: 0,
            board_edges: Vec::new(),
            rn_edges: Vec::new(),
            dirty: true,
        }
    }

    /// `IsDirty()`: the ratsnest of this net is invalid and requires an update.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// `GetNodeCount()`.
    pub fn node_count(&self) -> u32 {
        self.nodes.len() as u32
    }

    /// The nodes (`m_nodes`) in multiset order.
    pub fn nodes(&self) -> impl Iterator<Item = CnAnchorRef> + '_ {
        self.nodes.values().copied()
    }

    /// `m_boardEdges`.
    pub fn board_edges(&self) -> &[CnEdge] {
        &self.board_edges
    }

    /// Port of `GetEdges()`: `std::stable_sort`s the ratsnest edges with
    /// `CN_EDGE::StableSortCompare` (on every call, like the C++) and returns them.
    pub fn edges(&mut self, list: &CnList) -> &[CnEdge] {
        self.sort_edges(list);
        &self.rn_edges
    }

    /// Port of the non-const `GetEdges()`: sorted like [`RnNet::edges`], mutable.
    pub fn edges_mut(&mut self, list: &CnList) -> &mut [CnEdge] {
        self.sort_edges(list);
        &mut self.rn_edges
    }

    fn sort_edges(&mut self, list: &CnList) {
        // std::stable_sort == slice::sort_by (both stable)
        self.rn_edges
            .sort_by(|a, b| a.stable_sort_ordering(b, list));
    }

    /// Port of `GetUnsortedEdges()`: the ratsnest edges in no particular order.
    pub fn unsorted_edges(&self) -> &[CnEdge] {
        &self.rn_edges
    }

    /// Port of `GetTotalAirlineLength()`: the total length of the ratsnest airlines
    /// (unrouted connections) for this net.
    pub fn total_airline_length(&self, list: &CnList) -> i64 {
        let mut total: i64 = 0;

        for edge in &self.rn_edges {
            total += edge.length(list) as i64;
        }

        total
    }

    /// Port of `RN_NET::kruskalMST`: computes the minimum spanning tree using Filter-Kruskal.
    fn kruskal_mst(&mut self, list: &mut CnList, edges: Vec<CnEdge>) {
        self.rn_edges.clear();

        // node->SetTag( tag++ )
        for (tag, &node) in self.nodes.values().enumerate() {
            anchor_of_mut(list, node).set_tag(tag as i32);
        }

        let mut forest = KiUnionFind::new(self.nodes.len());

        // A zero-weight edge is a connection the board already makes and can never be a
        // ratsnest line, so uniting it here keeps it out of the sort where on a routed net it
        // would dominate
        let mut candidates: Vec<MstEdge> = Vec::with_capacity(edges.len());

        for edge in edges {
            let (Some(source), Some(target)) = (edge.source_anchor(list), edge.target_anchor(list))
            else {
                // wxCHECK2( source && !source->Dirty() && target && !target->Dirty(), continue )
                continue;
            };

            if source.dirty(list) || target.dirty(list) {
                continue;
            }

            let s_tag = source.tag();
            let t_tag = target.tag();

            if edge.weight() > 0 {
                // orderKey: canonical because an edge is undirected and the triangulator may
                // hand it to us either way round; the tag then separates distinct anchors that
                // share a position
                let mut first = (source.pos().x, source.pos().y, s_tag);
                let mut second = (target.pos().x, target.pos().y, t_tag);

                if second < first {
                    std::mem::swap(&mut first, &mut second);
                }

                candidates.push(MstEdge {
                    key: (edge.weight(), first, second),
                    ends: (s_tag as usize, t_tag as usize),
                    edge,
                });
            } else {
                forest.unite(s_tag as usize, t_tag as usize);
            }
        }

        self.rn_edges.reserve(self.nodes.len());

        let rn_edges = &mut self.rn_edges;

        filter_kruskal::filter_kruskal(
            &mut candidates,
            &mut forest,
            |lhs: &MstEdge, rhs: &MstEdge| lhs.key < rhs.key,
            |e: &MstEdge| e.ends,
            |e: &MstEdge| rn_edges.push(e.edge.clone()),
        );
    }

    /// Port of `RN_NET::TRIANGULATOR_STATE::Triangulate` (the triangulator's node set is a
    /// copy of `m_nodes` in the same order): appends the candidate edges to `mst_edges`.
    ///
    /// An error is the `std::runtime_error` the bundled delaunator throws; see
    /// [`RnNet::update_net`].
    fn triangulate(
        &self,
        ctx: &BoardCtx<'_>,
        list: &CnList,
        mst_edges: &mut Vec<CnEdge>,
    ) -> Result<(), DelaunatorError> {
        let all_nodes: Vec<CnAnchorRef> = self.nodes.values().copied().collect();
        let mut node_pts: Vec<f64> = Vec::with_capacity(2 * all_nodes.len());
        let mut anchors: Vec<CnAnchorRef> = Vec::with_capacity(all_nodes.len());
        let mut anchor_chains: Vec<Vec<CnAnchorRef>> = vec![Vec::new(); all_nodes.len()];

        let pos = |r: CnAnchorRef| anchor_of(list, r).pos();
        let add_edge = |mst_edges: &mut Vec<CnEdge>, src: CnAnchorRef, dst: CnAnchorRef| {
            let weight = anchor_of(list, src).dist(anchor_of(list, dst));
            mst_edges.push(CnEdge::between(src, dst, weight));
        };

        let mut prev: Option<CnAnchorRef> = None;

        for &n in &all_nodes {
            if prev.is_none_or(|p| pos(p) != pos(n)) {
                node_pts.push(pos(n).x as f64);
                node_pts.push(pos(n).y as f64);
                anchors.push(n);
                prev = Some(n);
            }

            anchor_chains[anchors.len() - 1].push(n);
        }

        if anchors.is_empty() {
            return Ok(());
        } else if anchors.len() == 1 {
            // The anchors all have the same position, but may not have overlapping layers.
            let mut prev: Option<CnAnchorRef> = None;

            for &n in &all_nodes {
                if let Some(p) = prev {
                    let layers_of = |r: CnAnchorRef| {
                        let parent = anchor_of(list, r).parent(list);
                        ctx.item(parent)
                            .expect("CN_ANCHOR::Parent: item not on the board")
                            .layer_set()
                    };

                    let mut common = layers_of(p);
                    common &= &layers_of(n);

                    if !common.any() {
                        // Use a minimal but non-zero distance or the edge will be ignored
                        mst_edges.push(CnEdge::between(p, n, 1));
                    }
                }

                prev = Some(n);
            }

            return Ok(());
        } else if are_nodes_colinear(list, &anchors) {
            // special case: all nodes are on the same line - there's no
            // triangulation for such set. In this case, we sort along any coordinate
            // and chain the nodes together.
            for i in 0..anchors.len() - 1 {
                add_edge(mst_edges, anchors[i], anchors[i + 1]);
            }
        } else {
            let delaunator = Delaunator::new(&node_pts)?;
            let triangles = &delaunator.triangles;

            // Half-edge e runs from triangles[e] to triangles[next(e)], so keeping the
            // lower half of each opposite pair plus the hull edges visits each edge once
            for e in 0..triangles.len() {
                let opposite = delaunator.halfedges[e];

                if opposite != INVALID_INDEX && opposite < e {
                    continue;
                }

                let next = if e % 3 == 2 { e - 2 } else { e + 1 };

                add_edge(mst_edges, anchors[triangles[e]], anchors[triangles[next]]);
            }
        }

        for chain in &mut anchor_chains {
            if chain.len() < 2 {
                continue;
            }

            // C++ sorts by `GetCluster().get()`, the heap address of the CN_CLUSTER; the
            // cluster id (creation order) stands in for it (connectivity-zones.md R-2).
            let cluster = |r: &CnAnchorRef| anchor_of(list, *r).cluster();
            std_sort(chain, |a, b| cluster(a) < cluster(b));

            for j in 1..chain.len() {
                let prev_node = chain[j - 1];
                let cur_node = chain[j];
                let weight = if cluster(&prev_node) != cluster(&cur_node) {
                    1
                } else {
                    0
                };
                let mut edge = CnEdge::between(prev_node, cur_node, weight);

                // FIX-CN-1: which coincident anchors of different clusters are paired, and
                // which way round, follows the CN_CLUSTER addresses in C++ (site P7).
                if weight == 1 {
                    *edge.tie_mut() = CnEdgeTie {
                        coincident_clusters: true,
                        unordered: true,
                        source_candidates: chain.iter().map(|&r| CnEdgeNode::Anchor(r)).collect(),
                        target_candidates: chain.iter().map(|&r| CnEdgeNode::Anchor(r)).collect(),
                        ..CnEdgeTie::default()
                    };
                }

                mst_edges.push(edge);
            }
        }

        Ok(())
    }

    /// Port of `RN_NET::compute()`: recomputes the ratsnest from scratch.
    fn compute(&mut self, ctx: &BoardCtx<'_>, list: &mut CnList) -> Result<(), DelaunatorError> {
        // Special cases do not need complicated algorithms (actually, it does not work well
        // with the Delaunay triangulator)
        if self.nodes.len() <= 2 {
            self.rn_edges.clear();

            // Check if the only possible connection exists
            if self.board_edges.is_empty() && self.nodes.len() == 2 {
                // There can be only one possible connection, but it is missing
                let mut it = self.nodes.values().copied();
                let source = it.next().expect("two nodes");
                let target = it.next().expect("two nodes");

                anchor_of_mut(list, source).set_tag(0);
                anchor_of_mut(list, target).set_tag(1);
                self.rn_edges.push(CnEdge::between(source, target, 0));
            } else {
                // Set tags to m_nodes as connected
                for &node in self.nodes.values() {
                    anchor_of_mut(list, node).set_tag(0);
                }
            }

            return Ok(());
        }

        // A Delaunay triangulation of n points has at most 3n-6 edges
        let mut triang_edges: Vec<CnEdge> =
            Vec::with_capacity(3 * self.nodes.len() + self.board_edges.len());

        self.triangulate(ctx, list, &mut triang_edges)?;

        triang_edges.extend(self.board_edges.iter().cloned());

        // Get the minimal spanning tree
        self.kruskal_mst(list, triang_edges);

        Ok(())
    }

    /// Port of `RN_NET::OptimizeRNEdges()`: finds optimal ends of the ratsnest edges. The MST
    /// will have found the closest anchors, but when zones are involved we might have points
    /// closer than the anchors.
    ///
    /// Normally called after [`RnNet::update_net`].
    pub fn optimize_rn_edges(&mut self, ctx: &BoardCtx<'_>, list: &CnList) {
        for edge in &mut self.rn_edges {
            let (Some(source), Some(target)) = (
                edge.source_anchor(list).cloned(),
                edge.target_anchor(list).cloned(),
            ) else {
                continue;
            };

            // wxCHECK2( source && !source->Dirty() && target && !target->Dirty(), continue )
            if source.dirty(list) || target.dirty(list) {
                continue;
            }

            if source.connected_items_count(ctx, list) == 0 {
                if let Some((optimized, tied)) = optimize_zone_anchor(ctx, list, &source, &target) {
                    edge.set_target_node(CnEdgeNode::Optimized(optimized));
                    record_zone_anchor_tie(edge, tied, false);
                }
            } else if target.connected_items_count(ctx, list) == 0 {
                if let Some((optimized, tied)) = optimize_zone_anchor(ctx, list, &target, &source) {
                    edge.set_source_node(CnEdgeNode::Optimized(optimized));
                    record_zone_anchor_tie(edge, tied, true);
                }
            } else {
                let original = (edge.source_node().cloned(), edge.target_node().cloned());
                let (calls, outcomes) = optimize_zone_to_zone_anchors(list, &source, &target);

                for (a, b) in calls {
                    edge.set_source_node(CnEdgeNode::Optimized(a));
                    edge.set_target_node(CnEdgeNode::Optimized(b));
                }

                record_zone_to_zone_tie(edge, original, outcomes);
            }
        }
    }

    /// Port of `RN_NET::UpdateNet()`: recomputes the ratsnest for the net.
    ///
    /// The C++ delaunator throws `std::runtime_error` on a point set it cannot triangulate;
    /// `CONNECTIVITY_DATA::updateRatsnest` runs `UpdateNet` in a thread pool whose
    /// `wait()` drops the exception, so the net keeps its previous (after `Clear()`: empty)
    /// edges, its tags and its dirty flag. That is what happens here; the error is returned
    /// for information only.
    pub fn update_net(
        &mut self,
        ctx: &BoardCtx<'_>,
        list: &mut CnList,
    ) -> Result<(), DelaunatorError> {
        self.compute(ctx, list)?;

        self.dirty = false;
        Ok(())
    }

    /// Port of `RN_NET::RemoveInvalidRefs()`.
    pub fn remove_invalid_refs(&mut self, list: &CnList) {
        for edge in &mut self.rn_edges {
            edge.remove_invalid_refs(list);
        }

        for edge in &mut self.board_edges {
            edge.remove_invalid_refs(list);
        }

        let is_invalid =
            |edge: &CnEdge| edge.source_node().is_none() || edge.target_node().is_none();

        self.rn_edges.retain(|e| !is_invalid(e));
        self.board_edges.retain(|e| !is_invalid(e));
    }

    /// Port of `RN_NET::Clear()`.
    pub fn clear(&mut self) {
        self.rn_edges.clear();
        self.board_edges.clear();
        self.nodes.clear();

        self.dirty = true;
    }

    /// Port of `RN_NET::AddCluster()`: adds the anchors of the cluster `cluster` (members
    /// `items`, in cluster order) as nodes, joined by weight-0 board edges.
    ///
    /// The members are passed separately because the cluster and the list both belong to the
    /// algorithm: `let items = algo.cluster( c ).items().to_vec();`.
    pub fn add_cluster(&mut self, list: &mut CnList, cluster: CnClusterId, items: &[CnItemId]) {
        let mut first_anchor: Option<CnAnchorRef> = None;

        for &item_id in items {
            let item = list.item(item_id);
            let anchor_count = item.anchors().len();
            let mut n_anchors = if item.zone_layer().is_some() {
                1
            } else {
                anchor_count
            };

            if n_anchors > anchor_count {
                n_anchors = anchor_count;
            }

            for i in 0..n_anchors {
                let r = CnAnchorRef {
                    item: item_id,
                    index: i as u32,
                };
                let anchor = anchor_of_mut(list, r);
                anchor.set_cluster(Some(cluster));
                let pos = anchor.pos();

                self.nodes.insert((pos.x, pos.y, self.next_seq), r);
                self.next_seq += 1;

                match first_anchor {
                    Some(first) => {
                        if first != r {
                            self.board_edges.push(CnEdge::between(first, r, 0));
                        }
                    }
                    None => first_anchor = Some(r),
                }
            }
        }
    }

    /// Port of `RN_NET::NearestBicoloredPair()`: the closest pair of nodes, one of
    /// `other_net` and one of this net, ignoring `no_line` nodes. Returns `( aPos1, aPos2 )`
    /// (`other_net` node first) or `None` (C++ `false`).
    pub fn nearest_bicolored_pair(
        &self,
        other_net: &RnNet,
        list: &CnList,
    ) -> Option<(Vector2I, Vector2I)> {
        let mut rv: Option<(Vector2I, Vector2I)> = None;

        let mut dist_max_sq: i64 = i64::MAX;

        let mut verify = |test_node1: &CnAnchor, test_node2: &CnAnchor, dist_max_sq: &mut i64| {
            let diff = test_node1.pos() - test_node2.pos();
            let dist_sq = diff.squared_euclidean_norm();

            if dist_sq < *dist_max_sq {
                rv = Some((test_node1.pos(), test_node2.pos()));
                *dist_max_sq = dist_sq;
            }
        };

        // Sweep-line algorithm to cut the number of comparisons to find the closest point
        //
        // Step 1: The outer loop needs to be the subset (selected nodes) as it is a linear
        // search
        for &a in other_net.nodes.values() {
            let node_a = anchor_of(list, a);

            if node_a.no_line() {
                continue;
            }

            // Step 2: O( log n ) search to identify a close element ordered by x
            // The forward iterator will move forward through the elements while
            // the reverse iterator will move backward through the same set
            let lower: NodeKey = (node_a.pos().x, node_a.pos().y, 0);

            for &b in self.nodes.range(lower..).map(|(_, b)| b) {
                let node_b = anchor_of(list, b);

                if node_b.no_line() {
                    continue;
                }

                let dx = node_a.pos().x as i64 - node_b.pos().x as i64;
                let dist_x_sq = dx * dx;

                // As soon as the x distance (primary sort) is larger than the smallest
                // distance, stop checking further elements
                if dist_x_sq > dist_max_sq {
                    break;
                }

                verify(node_a, node_b, &mut dist_max_sq);
            }

            // Step 3: using the same starting point, check points backwards for closer points
            for &b in self.nodes.range(..lower).rev().map(|(_, b)| b) {
                let node_b = anchor_of(list, b);

                if node_b.no_line() {
                    continue;
                }

                let dx = node_a.pos().x as i64 - node_b.pos().x as i64;
                let dist_x_sq = dx * dx;

                if dist_x_sq > dist_max_sq {
                    break;
                }

                verify(node_a, node_b, &mut dist_max_sq);
            }
        }

        rv
    }
}

/// Port of `RN_NET::TRIANGULATOR_STATE::areNodesColinear`: checks if all nodes lie on a
/// single line. Requires the nodes to have unique coordinates!
fn are_nodes_colinear(list: &CnList, nodes: &[CnAnchorRef]) -> bool {
    if nodes.len() <= 2 {
        return true;
    }

    let p0 = anchor_of(list, nodes[0]).pos();
    let v0 = anchor_of(list, nodes[1]).pos() - p0;

    for &n in &nodes[2..] {
        let v1 = anchor_of(list, n).pos() - p0;

        if v0.cross(v1) != 0 {
            return false;
        }
    }

    true
}

/// Port of the `optimizeZoneAnchor` lambda of `RN_NET::OptimizeRNEdges`: among the zone
/// layers of the same net connected to `anchor`'s item on one of the layers of `pos_anchor`'s
/// parent, the outline vertex closest to `pos_anchor` (if closer than `anchor`) becomes a new
/// anchor on that zone layer.
///
/// Also returns (FIX-CN-1) the anchors every other walk order of `ConnectedItems()` could have
/// produced: one per zone layer whose closest vertex is at the winning distance (its first
/// such vertex, as the strict `<` takes it); more than one means the C++ result follows the
/// heap order of `m_connected` (site P8). The port's own result is among them.
fn optimize_zone_anchor(
    ctx: &BoardCtx<'_>,
    list: &CnList,
    pos_anchor: &CnAnchor,
    anchor: &CnAnchor,
) -> Option<(CnAnchor, Vec<CnAnchor>)> {
    let pos = pos_anchor.pos();
    let layer_set = ctx
        .item(pos_anchor.parent(list))
        .expect("CN_ANCHOR::Parent: item not on the board")
        .layer_set();

    let mut closest_dist_sq: i64 = (anchor.pos() - pos).squared_euclidean_norm();
    let mut closest_pt = Vector2I::default();
    let mut closest_item: Option<CnItemId> = None;

    let anchor_item = list.item(anchor.item().expect("CN_ANCHOR::Item: no item"));

    // FIX-CN-1: per zone layer its closest vertex (the first one at that distance).
    let mut per_item: Vec<(CnItemId, i64, Vector2I)> = Vec::new();

    for &item_id in anchor_item.connected_items() {
        let item = list.item(item_id);

        // Don't consider shorted items
        if anchor_item.net() != item.net() {
            continue;
        }

        let Some(zone_layer) = item.zone_layer() else {
            continue;
        };

        if layer_set.contains(item.board_layer()) {
            let pts = zone_layer.outline().c_points();
            let mut item_best: Option<(i64, Vector2I)> = None;

            for &pt in pts {
                let dist_sq: i64 = (pt - pos).squared_euclidean_norm();

                if item_best.is_none_or(|(d, _)| dist_sq < d) {
                    item_best = Some((dist_sq, pt));
                }

                if dist_sq < closest_dist_sq {
                    closest_pt = pt;
                    closest_item = Some(item_id);
                    closest_dist_sq = dist_sq;
                }
            }

            if let Some((d, pt)) = item_best
                && !per_item.iter().any(|&(id, _, _)| id == item_id)
            {
                per_item.push((item_id, d, pt));
            }
        }
    }

    closest_item.map(|item| {
        let tied: Vec<CnAnchor> = per_item
            .iter()
            .filter(|&&(_, d, _)| d == closest_dist_sq)
            .map(|&(id, _, pt)| CnAnchor::new(pt, Some(id)))
            .collect();

        (CnAnchor::new(closest_pt, Some(item)), tied)
    })
}

/// FIX-CN-1: records on `edge` the candidates of the end `optimizeZoneAnchor` just set (the
/// source end if `source_end`), flagging the edge when several zone layers tie (site P8).
fn record_zone_anchor_tie(edge: &mut CnEdge, tied: Vec<CnAnchor>, source_end: bool) {
    if tied.len() <= 1 && !edge.is_tied() {
        return;
    }

    let tie = edge.tie_mut();

    if tied.len() > 1 {
        tie.zone_anchor = true;
    }

    let candidates = tied.into_iter().map(CnEdgeNode::Optimized).collect();

    if source_end {
        tie.source_candidates = candidates;
    } else {
        tie.target_candidates = candidates;
    }
}

/// FIX-CN-1: flags `edge` when `optimizeZoneToZoneAnchors` could have produced more than one
/// result under another `ConnectedItems()` order (site P9). `outcomes` are the possible
/// results (`None`: the ends are left as they were, `original`).
fn record_zone_to_zone_tie(
    edge: &mut CnEdge,
    original: (Option<CnEdgeNode>, Option<CnEdgeNode>),
    outcomes: Vec<Option<(CnAnchor, CnAnchor)>>,
) {
    if outcomes.len() <= 1 {
        return;
    }

    let mut sources: Vec<CnEdgeNode> = Vec::new();
    let mut targets: Vec<CnEdgeNode> = Vec::new();

    let push = |v: &mut Vec<CnEdgeNode>, n: Option<CnEdgeNode>| {
        if let Some(n) = n
            && !v.contains(&n)
        {
            v.push(n);
        }
    };

    for outcome in outcomes {
        match outcome {
            Some((a, b)) => {
                push(&mut sources, Some(CnEdgeNode::Optimized(a)));
                push(&mut targets, Some(CnEdgeNode::Optimized(b)));
            }
            None => {
                push(&mut sources, original.0.clone());
                push(&mut targets, original.1.clone());
            }
        }
    }

    let tie = edge.tie_mut();
    tie.zone_to_zone = true;
    tie.source_candidates = sources;
    tie.target_candidates = targets;
}

/// Port of the `optimizeZoneToZoneAnchors` lambda of `RN_NET::OptimizeRNEdges`: for the 3
/// closest pairs (by bounding box centre) of zone layers connected to `a`'s and `b`'s items,
/// the closest points of their outlines. Returns the `( setOptimizedATo, setOptimizedBTo )`
/// calls in order (later ones overwrite earlier ones).
///
/// Also returns (FIX-CN-1) every result another order of the input pairs could have given
/// (`None`: no call), deduplicated, the port's own among them; see [`zone_to_zone_outcomes`].
fn optimize_zone_to_zone_anchors(
    list: &CnList,
    a: &CnAnchor,
    b: &CnAnchor,
) -> (Vec<(CnAnchor, CnAnchor)>, Vec<Option<(CnAnchor, CnAnchor)>>) {
    #[derive(Clone, Copy, Default)]
    struct Center {
        pt: Vector2I,
        valid: bool,
    }

    #[derive(Clone, Copy)]
    struct DistPair {
        dist_sq: i64,
        id_a: usize,
        id_b: usize,
    }

    let connected_items_a = list
        .item(a.item().expect("CN_ANCHOR::Item: no item"))
        .connected_items();
    let connected_items_b = list
        .item(b.item().expect("CN_ANCHOR::Item: no item"))
        .connected_items();

    let centers_of = |items: &[CnItemId]| -> Vec<Center> {
        items
            .iter()
            .map(|&id| match list.item(id).zone_layer() {
                Some(zl) => Center {
                    pt: zl.outline().bbox(0).center(),
                    valid: true,
                },
                None => Center::default(),
            })
            .collect()
    };

    let centers_a = centers_of(connected_items_a);
    let centers_b = centers_of(connected_items_b);

    let mut pairs_to_test: Vec<DistPair> = Vec::new();

    for (ia, ca) in centers_a.iter().enumerate() {
        for (ib, cb) in centers_b.iter().enumerate() {
            if !ca.valid || !cb.valid {
                continue;
            }

            // VECTOR2L arithmetic
            let dx = cb.pt.x as i64 - ca.pt.x as i64;
            let dy = cb.pt.y as i64 - ca.pt.y as i64;
            let dist_sq = dx * dx + dy * dy;

            pairs_to_test.push(DistPair {
                dist_sq,
                id_a: ia,
                id_b: ib,
            });
        }
    }

    std_sort(&mut pairs_to_test, |dp_a, dp_b| dp_a.dist_sq < dp_b.dist_sq);

    const C_POLY_PAIRS_LIMIT: usize = 3;

    // The `( setOptimizedATo, setOptimizedBTo )` arguments of one pair, `None` when the pair
    // is skipped or `ClosestSegmentsFast` fails.
    let pair_result = |pair: &DistPair| -> Option<(CnAnchor, CnAnchor)> {
        let zone_layer_a = connected_items_a[pair.id_a];
        let zone_layer_b = connected_items_b[pair.id_b];

        if zone_layer_a == zone_layer_b {
            return None;
        }

        let shape_a = list
            .item(zone_layer_a)
            .zone_layer()
            .expect("zone layer")
            .outline();
        let shape_b = list
            .item(zone_layer_b)
            .zone_layer()
            .expect("zone layer")
            .outline();

        shape_a.closest_segments_fast(shape_b).map(|(pt_a, pt_b)| {
            (
                CnAnchor::new(pt_a, Some(zone_layer_a)),
                CnAnchor::new(pt_b, Some(zone_layer_b)),
            )
        })
    };

    let mut result = Vec::new();

    for pair in pairs_to_test.iter().take(C_POLY_PAIRS_LIMIT) {
        if let Some(r) = pair_result(pair) {
            result.push(r);
        }
    }

    let dists: Vec<i64> = pairs_to_test.iter().map(|p| p.dist_sq).collect();
    let outcomes = zone_to_zone_outcomes(&dists, C_POLY_PAIRS_LIMIT, |i| {
        pair_result(&pairs_to_test[i])
    });

    (result, outcomes)
}

/// FIX-CN-1: the results `optimizeZoneToZoneAnchors` can give for any order of its input
/// pairs (the C++ builds them in `ConnectedItems()` order, i.e. heap order, and `std::sort`s
/// them by centre distance, so equal distances come out in an order the port cannot know).
///
/// `dists` are the sorted distances, `limit` the number of pairs tested and `result( i )` the
/// call pair `i` makes (`None`: none). The result is the last call among the first `limit`
/// pairs. Groups of equal distance lying wholly inside the tested prefix can come out in any
/// order: if one member makes a call, the last call is one of the group's. The group that
/// straddles the end of the prefix (`k` of its members tested) can contribute any of its
/// calling members, and when `k` of them make no call, the result falls back to the groups
/// before it. Deduplicated, in discovery order; `None` stands for "no call at all".
fn zone_to_zone_outcomes<T: Clone + PartialEq>(
    dists: &[i64],
    limit: usize,
    result: impl Fn(usize) -> Option<T>,
) -> Vec<Option<T>> {
    let n = dists.len().min(limit);
    let mut outcomes: Vec<Option<T>> = Vec::new();

    if n == 0 {
        return outcomes;
    }

    let push = |outcomes: &mut Vec<Option<T>>, o: Option<T>| {
        if !outcomes.contains(&o) {
            outcomes.push(o);
        }
    };

    // Walk the groups backwards from the one holding the last tested position.
    let mut end = n; // tested positions of the current group are [start, end)
    let mut first = true;

    loop {
        let value = dists[end - 1];
        let mut start = end - 1;

        while start > 0 && dists[start - 1] == value {
            start -= 1;
        }

        // The whole group (beyond the tested prefix too, for the straddling group).
        let mut group_end = end;

        if first {
            while group_end < dists.len() && dists[group_end] == value {
                group_end += 1;
            }
        }

        let slots = end - start;
        let mut calls = 0;
        let mut silent = 0;

        for i in start..group_end {
            match result(i) {
                Some(r) => {
                    calls += 1;
                    push(&mut outcomes, Some(r));
                }
                None => silent += 1,
            }
        }

        // Can every tested slot of this group make no call?
        let fall_back = calls == 0 || silent >= slots;

        if !fall_back {
            break;
        }

        if start == 0 {
            push(&mut outcomes, None);
            break;
        }

        end = start;
        first = false;
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use super::zone_to_zone_outcomes;

    /// FIX-CN-1: the possible results of `optimizeZoneToZoneAnchors` under any order of tied
    /// pairs (`None` in `results`: the pair makes no call).
    fn outcomes(dists: &[i64], results: &[Option<u32>]) -> Vec<Option<u32>> {
        let mut o = zone_to_zone_outcomes(dists, 3, |i| results[i]);
        o.sort();
        o
    }

    #[test]
    fn distinct_distances_have_one_outcome() {
        assert_eq!(
            outcomes(&[1, 2, 3, 4], &[Some(0), Some(1), Some(2), Some(3)]),
            vec![Some(2)]
        );
        assert_eq!(
            outcomes(&[1, 2, 3, 4], &[Some(0), Some(1), None, Some(3)]),
            vec![Some(1)]
        );
        assert_eq!(outcomes(&[1, 2], &[None, None]), vec![None]);
        assert_eq!(outcomes(&[], &[]), Vec::<Option<u32>>::new());
        // a tie beyond the tested prefix does not matter
        assert_eq!(
            outcomes(
                &[1, 2, 3, 4, 4],
                &[Some(0), Some(1), Some(2), Some(3), Some(4)]
            ),
            vec![Some(2)]
        );
    }

    #[test]
    fn tied_groups() {
        // a tie inside the prefix: either member can come last
        assert_eq!(
            outcomes(&[1, 2, 2], &[Some(0), Some(1), Some(2)]),
            vec![Some(1), Some(2)]
        );
        // a tie inside the prefix before the last pair does not matter
        assert_eq!(
            outcomes(&[1, 1, 2], &[Some(0), Some(1), Some(2)]),
            vec![Some(2)]
        );
        // four pairs at one distance (rev_order_probe N3): any of them
        assert_eq!(
            outcomes(&[5, 5, 5, 5], &[Some(0), Some(1), Some(2), Some(3)]),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
        // straddling group whose tested slot can be a silent pair: falls back
        assert_eq!(
            outcomes(&[1, 2, 3, 3], &[Some(0), Some(1), None, Some(3)]),
            vec![Some(1), Some(3)]
        );
        // a silent group inside the prefix falls back to the group before it
        assert_eq!(
            outcomes(&[1, 1, 2], &[Some(0), Some(1), None]),
            vec![Some(0), Some(1)]
        );
        // all silent but for one member of a straddling group
        assert_eq!(
            outcomes(&[1, 1, 1, 1], &[None, None, None, Some(3)]),
            vec![None, Some(3)]
        );
    }
}
