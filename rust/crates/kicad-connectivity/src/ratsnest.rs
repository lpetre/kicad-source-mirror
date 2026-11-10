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
use crate::edge::{CnEdge, CnEdgeNode, anchor_of, anchor_of_mut};
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
                mst_edges.push(CnEdge::between(prev_node, cur_node, weight));
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
                if let Some(optimized) = optimize_zone_anchor(ctx, list, &source, &target) {
                    edge.set_target_node(CnEdgeNode::Optimized(optimized));
                }
            } else if target.connected_items_count(ctx, list) == 0 {
                if let Some(optimized) = optimize_zone_anchor(ctx, list, &target, &source) {
                    edge.set_source_node(CnEdgeNode::Optimized(optimized));
                }
            } else {
                for (a, b) in optimize_zone_to_zone_anchors(list, &source, &target) {
                    edge.set_source_node(CnEdgeNode::Optimized(a));
                    edge.set_target_node(CnEdgeNode::Optimized(b));
                }
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
fn optimize_zone_anchor(
    ctx: &BoardCtx<'_>,
    list: &CnList,
    pos_anchor: &CnAnchor,
    anchor: &CnAnchor,
) -> Option<CnAnchor> {
    let pos = pos_anchor.pos();
    let layer_set = ctx
        .item(pos_anchor.parent(list))
        .expect("CN_ANCHOR::Parent: item not on the board")
        .layer_set();

    let mut closest_dist_sq: i64 = (anchor.pos() - pos).squared_euclidean_norm();
    let mut closest_pt = Vector2I::default();
    let mut closest_item: Option<CnItemId> = None;

    let anchor_item = list.item(anchor.item().expect("CN_ANCHOR::Item: no item"));

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

            for &pt in pts {
                let dist_sq: i64 = (pt - pos).squared_euclidean_norm();

                if dist_sq < closest_dist_sq {
                    closest_pt = pt;
                    closest_item = Some(item_id);
                    closest_dist_sq = dist_sq;
                }
            }
        }
    }

    closest_item.map(|item| CnAnchor::new(closest_pt, Some(item)))
}

/// Port of the `optimizeZoneToZoneAnchors` lambda of `RN_NET::OptimizeRNEdges`: for the 3
/// closest pairs (by bounding box centre) of zone layers connected to `a`'s and `b`'s items,
/// the closest points of their outlines. Returns the `( setOptimizedATo, setOptimizedBTo )`
/// calls in order (later ones overwrite earlier ones).
fn optimize_zone_to_zone_anchors(
    list: &CnList,
    a: &CnAnchor,
    b: &CnAnchor,
) -> Vec<(CnAnchor, CnAnchor)> {
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

    let mut result = Vec::new();

    for pair in pairs_to_test.iter().take(C_POLY_PAIRS_LIMIT) {
        let zone_layer_a = connected_items_a[pair.id_a];
        let zone_layer_b = connected_items_b[pair.id_b];

        if zone_layer_a == zone_layer_b {
            continue;
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

        if let Some((pt_a, pt_b)) = shape_a.closest_segments_fast(shape_b) {
            result.push((
                CnAnchor::new(pt_a, Some(zone_layer_a)),
                CnAnchor::new(pt_b, Some(zone_layer_b)),
            ));
        }
    }

    result
}
