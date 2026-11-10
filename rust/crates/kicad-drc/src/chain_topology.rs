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

//! Port of `pcbnew/drc/drc_chain_topology.h` / `drc_chain_topology.cpp`: a topological view of
//! one named net chain's routed copper (trunk between the two terminal pads, stubs).
//!
//! Order notes: the C++ walks the chain items (`std::set<BOARD_CONNECTED_ITEM*>`) and the pad
//! anchors (`std::map<PAD*, ...>`) in address order; the port uses `ItemId` order. That order
//! decides the node and edge numbering, hence the DFS order, the order of the stubs and of
//! their items, and the summation order of the trunk / stub lengths (last-bit differences of a
//! `double` are possible). Trunk and stubs as sets do not depend on it for a tree. The node
//! index (`std::unordered_map<NodeKey, int>`) is only looked up, never iterated.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kicad_core::layer_ids::{Flashing, PcbLayerId};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::math::vector2::{Vector2D, Vector2I};
use kicad_pcb::{BoardCtx, ItemId, ItemRef};

use crate::constraint_types::DrcConstraintT;
use crate::length_delay_calculation::{
    LdcItemType, LengthDelayCalculation, LengthDelayDomainOpt, LengthDelayLayerOpt,
    PathOptimisations,
};
use crate::net_chain_bridging::{chain_bridging_delay_per_mm, enumerate_chain_bridges};

/// Coalesce coincident graph nodes by quantizing coordinates to this many IU
/// (roughly 1 micron at typical pcbnew IU scale).  Track endpoints set by the
/// router are pixel-exact so 1 IU is the right tolerance.
const NODE_COALESCE_EPSILON: i32 = 1;

/// `CHAIN_TOPOLOGY::STUB`: an off-trunk branch.
#[derive(Clone, Debug, PartialEq)]
pub struct Stub {
    /// `branchPoint`
    pub branch_point: Vector2I,
    /// `branchLayer`
    pub branch_layer: PcbLayerId,
    /// `items`
    pub items: Vec<ItemId>,
    /// `length`
    pub length: f64,
    /// `delay`
    pub delay: f64,
}

/// `CHAIN_TOPOLOGY::STATUS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainTopologyStatus {
    /// `OK`
    Ok,
    /// `NO_ITEMS`
    NoItems,
    /// `NO_TERMINAL_PADS`
    NoTerminalPads,
    /// `DISCONNECTED`
    Disconnected,
    /// `CYCLE_DETECTED`
    CycleDetected,
}

/// `NodeKey`
type NodeKey = (i32, i32, i32);

/// `Edge`
#[derive(Clone, Copy, Debug)]
struct Edge {
    node_a: usize,
    node_b: usize,
    length: f64,
    delay: f64,
    /// `sourceItem` (null for synthetic split halves / ties / bridges)
    source_item: Option<ItemId>,
    /// signal layer for stub markers
    layer: PcbLayerId,
}

/// `Graph`
#[derive(Default)]
struct Graph {
    node_pos: Vec<Vector2I>,
    node_layer: Vec<PcbLayerId>,
    edges: Vec<Edge>,
    /// node -> list of edge indices
    adj: Vec<Vec<usize>>,
    /// Pad anchors: nodes that originated from terminal pads (`std::map<PAD*, ...>`).
    pad_nodes: BTreeMap<ItemId, Vec<usize>>,
    /// `std::unordered_map<NodeKey, int, NodeKeyHash>` (lookup only).
    index: BTreeMap<NodeKey, usize>,
}

impl Graph {
    /// `addNode`
    fn add_node(&mut self, pos: Vector2I, layer: PcbLayerId) -> usize {
        let rx = (pos.x / NODE_COALESCE_EPSILON) * NODE_COALESCE_EPSILON;
        let ry = (pos.y / NODE_COALESCE_EPSILON) * NODE_COALESCE_EPSILON;
        let key = (rx, ry, layer.0);

        if let Some(&idx) = self.index.get(&key) {
            return idx;
        }

        let idx = self.node_pos.len();
        self.node_pos.push(pos);
        self.node_layer.push(layer);
        self.adj.push(Vec::new());
        self.index.insert(key, idx);
        idx
    }

    /// `addEdge`: `None` (C++ -1) for a degenerate edge.
    fn add_edge(
        &mut self,
        node_a: usize,
        node_b: usize,
        length: f64,
        delay: f64,
        src: Option<ItemId>,
        layer: PcbLayerId,
    ) -> Option<usize> {
        if node_a == node_b {
            return None;
        }

        let idx = self.edges.len();
        self.edges.push(Edge {
            node_a,
            node_b,
            length,
            delay,
            source_item: src,
            layer,
        });
        self.adj[node_a].push(idx);
        self.adj[node_b].push(idx);
        Some(idx)
    }
}

/// `euclidean`
fn euclidean(a: Vector2I, b: Vector2I) -> f64 {
    (Vector2D::from(a) - Vector2D::from(b)).euclidean_norm()
}

/// Port of `trackDelay`: the delay of one track from the per-item length calculation.
fn track_delay(track: ItemRef<'_>, length: f64, calc: &LengthDelayCalculation<'_>, fallback_delay_per_mm: f64) -> f64 {
    if length <= 0.0 {
        return 0.0;
    }

    let length_item = calc.length_calculation_item(track);

    if length_item.type_() == LdcItemType::Unknown {
        return fallback_delay_per_mm * length / kicad_core::base_units::PCB_IU_SCALE.iu_per_mm;
    }

    let opts = PathOptimisations::default();
    let mut items = vec![length_item];
    let stats = calc.calculate_length_details(
        &mut items,
        opts,
        None,
        None,
        LengthDelayLayerOpt::NoLayerDetail,
        LengthDelayDomainOpt::WithDelayDetail,
        None,
    );

    stats.track_delay as f64
}

/// Port of `addViaEdges`: one edge per adjacent copper layer pair of the via, with the via's
/// length and delay apportioned by stackup height.
fn add_via_edges(graph: &mut Graph, via: ItemRef<'_>, calc: &LengthDelayCalculation<'_>) -> i32 {
    let stack = via.layer_set().cu_stack().0;

    if stack.len() < 2 {
        return 0;
    }

    // Compute total via length and delay once via the per-item query so per-via
    // overrides from GetPropagationDelays() are honored.
    let mut total_delay = 0.0;
    let mut total_length = 0.0;

    let length_item = calc.length_calculation_item(via);

    if length_item.type_() != LdcItemType::Unknown {
        let opts = PathOptimisations::default();
        let mut items = vec![length_item];
        let stats = calc.calculate_length_details(
            &mut items,
            opts,
            None,
            None,
            LengthDelayLayerOpt::NoLayerDetail,
            LengthDelayDomainOpt::WithDelayDetail,
            None,
        );

        total_delay = stats.via_delay as f64;
        total_length = f64::from(stats.via_length);
    }

    // If the per-item query reports no via length, fall back to summing
    // StackupHeight across the adjacent-layer pairs and apportion delay
    // equally.
    let mut heights: Vec<i32> = Vec::new();
    let mut height_sum: i32 = 0;

    for i in 0..stack.len() - 1 {
        let h = calc.stackup_height(stack[i], stack[i + 1]);
        heights.push(h);
        height_sum += h;
    }

    if total_length <= 0.0 {
        total_length = f64::from(height_sum);
    }

    let mut edges = 0;
    let via_pos = via.position();
    let via_id = via.id();

    for i in 0..stack.len() - 1 {
        let n_a = graph.add_node(via_pos, stack[i]);
        let n_b = graph.add_node(via_pos, stack[i + 1]);

        let seg_len = f64::from(heights[i]);
        let mut seg_delay = 0.0;

        if total_length > 0.0 {
            seg_delay = total_delay * (seg_len / total_length);
        } else if !heights.is_empty() {
            seg_delay = total_delay / heights.len() as f64; // equal split fallback
        }

        if graph
            .add_edge(n_a, n_b, seg_len, seg_delay, via_id, stack[i])
            .is_some()
        {
            edges += 1;
        }
    }

    edges
}

/// The copper layers of a pad in stack order (`( GetLayerSet() & AllCuMask() ).CuStack()`).
fn pad_cu_stack(pad: ItemRef<'_>) -> Vec<PcbLayerId> {
    (&pad.layer_set() & Lset::all_cu_mask()).cu_stack().0
}

fn pad_center(ctx: &BoardCtx<'_>, pad: ItemRef<'_>) -> Vector2I {
    pad.as_pad().expect("a pad").center(ctx)
}

/// Port of `addPadNodes`: one node per copper layer of the pad.
fn add_pad_nodes(graph: &mut Graph, ctx: &BoardCtx<'_>, pad: ItemRef<'_>) -> Vec<(PcbLayerId, usize)> {
    let mut nodes = Vec::new();
    let center = pad_center(ctx, pad);

    for layer in pad_cu_stack(pad) {
        let n = graph.add_node(center, layer);
        nodes.push((layer, n));
    }

    nodes
}

/// `isInteriorParametric`: true if the parametric position t (0..1) is strictly inside the
/// open interval (epsilon, 1-epsilon), i.e. not at an endpoint.
fn is_interior_parametric(t: f64) -> bool {
    const K_EDGE_FRACTION: f64 = 1e-4;
    t > K_EDGE_FRACTION && t < 1.0 - K_EDGE_FRACTION
}

fn is_trace(ctx: &BoardCtx<'_>, item: Option<ItemId>) -> bool {
    item.and_then(|id| ctx.item(id))
        .is_some_and(|i| i.type_() == KicadT::PcbTraceT)
}

/// Port of `splitTrackTJunctions`: for each track edge endpoint, check whether it lies on the
/// interior of another track edge on the same layer; if so, split that edge.
fn split_track_t_junctions(graph: &mut Graph, ctx: &BoardCtx<'_>) {
    // Copy the current edge index list because we may mutate edges as we go.
    // We only consider track edges (sourceItem is a PCB_TRACK and not an arc)
    // because mid-segment T-joins on arcs are exotic and outside v1 scope.
    let track_edges: Vec<usize> = (0..graph.edges.len())
        .filter(|&i| is_trace(ctx, graph.edges[i].source_item))
        .collect();

    for &endpoint_edge in &track_edges {
        for endpoint_node_sel in 0..2 {
            let src_edge = graph.edges[endpoint_edge];
            let endpoint_node = if endpoint_node_sel == 0 {
                src_edge.node_a
            } else {
                src_edge.node_b
            };
            let endpoint_pos = graph.node_pos[endpoint_node];
            let endpoint_layer = graph.node_layer[endpoint_node];

            // Check this endpoint against every other track edge on the same
            // layer.  Iterate by index so any newly inserted edges from a
            // previous split are re-considered.
            let mut other_idx = 0;

            while other_idx < graph.edges.len() {
                let this_idx = other_idx;
                other_idx += 1;

                if this_idx == endpoint_edge {
                    continue;
                }

                let other = graph.edges[this_idx];

                if !is_trace(ctx, other.source_item) {
                    continue;
                }

                if other.layer != endpoint_layer {
                    continue;
                }

                let a_pos = graph.node_pos[other.node_a];
                let b_pos = graph.node_pos[other.node_b];

                // Parametric projection of endpoint onto the other segment.
                let ab = Vector2D::from(b_pos) - Vector2D::from(a_pos);
                let len2 = ab.x * ab.x + ab.y * ab.y;

                if len2 <= 0.0 {
                    continue;
                }

                let ap = Vector2D::from(endpoint_pos) - Vector2D::from(a_pos);
                let t = (ap.x * ab.x + ap.y * ab.y) / len2;

                if !is_interior_parametric(t) {
                    continue;
                }

                let proj_d = Vector2D::from(a_pos) + ab * t;
                let proj = Vector2I::new(proj_d.x.round() as i32, proj_d.y.round() as i32);

                // Use the track's effective shape (which is width-aware) for
                // the actual contact test — KiCad's connectivity engine does
                // the same (connectivity_algo.cpp's
                // GetEffectiveShape->Collide pattern), so two tracks that
                // touch only by their copper width but not exactly on the
                // centerline are still recognized as a T-junction.  The
                // centerline projection above gave us the candidate split
                // point; Collide gates whether to actually split.
                let other_track = other
                    .source_item
                    .and_then(|id| ctx.item(id))
                    .filter(|i| i.as_track().is_some());

                if let Some(other_track) = other_track {
                    let shape = other_track.effective_shape(
                        endpoint_layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );

                    if !shape.collide_point(endpoint_pos, 0, None, None) {
                        // Fall back to the centerline epsilon test for cases
                        // where GetEffectiveShape isn't available (synthetic
                        // edges from earlier splits with no source item).
                        if euclidean(proj, endpoint_pos) > f64::from(NODE_COALESCE_EPSILON) {
                            continue;
                        }
                    }
                } else if euclidean(proj, endpoint_pos) > f64::from(NODE_COALESCE_EPSILON) {
                    continue;
                }

                // Split the other edge at the projected point.  Replace the
                // single Edge with two halves that share a new junction node,
                // each carrying half the length / delay scaled by t.
                let full_len = other.length;
                let full_delay = other.delay;
                let old_a = other.node_a;
                let old_b = other.node_b;
                let old_src = other.source_item;
                let old_layer = other.layer;

                let junction_node = graph.add_node(proj, endpoint_layer);

                if junction_node == old_a || junction_node == old_b {
                    continue; // would have collapsed back; skip
                }

                // Remove the old edge from adjacency lists; we'll rewrite it.
                graph.adj[old_a].retain(|&e| e != this_idx);
                graph.adj[old_b].retain(|&e| e != this_idx);

                // Re-purpose `other` as the first half (oldA -> junction).
                graph.edges[this_idx].node_a = old_a;
                graph.edges[this_idx].node_b = junction_node;
                graph.edges[this_idx].length = full_len * t;
                graph.edges[this_idx].delay = full_delay * t;
                graph.adj[old_a].push(this_idx);
                graph.adj[junction_node].push(this_idx);

                // Add the second half as a new edge (junction -> oldB).
                graph.add_edge(
                    junction_node,
                    old_b,
                    full_len * (1.0 - t),
                    full_delay * (1.0 - t),
                    old_src,
                    old_layer,
                );

                // Connect the original endpoint to the junction explicitly.
                // (The endpoint's edge already terminates at endpointNode and
                // endpointNode == junctionNode after coalescing because they
                // share (x, y, layer); the addNode lookup above should have
                // returned the same index.  If not, we add a zero-length tie
                // to avoid a graph break.)
                if endpoint_node != junction_node {
                    graph.add_edge(endpoint_node, junction_node, 0.0, 0.0, None, endpoint_layer);
                }
            }
        }
    }
}

/// Port of `dfsSpanningTree`: a DFS that records parent edges and detects cycles by
/// encountering a back-edge to a non-parent visited node. Returns (cycle, parent node,
/// parent edge, visited).
#[allow(clippy::type_complexity)]
fn dfs_spanning_tree(graph: &Graph, root: usize) -> (bool, Vec<Option<usize>>, Vec<Option<usize>>, Vec<bool>) {
    let n = graph.node_pos.len();
    let mut parent_node = vec![None; n];
    let mut parent_edge: Vec<Option<usize>> = vec![None; n];
    let mut visited = vec![false; n];

    // (node, parentEdgeIdx)
    let mut stack: Vec<(usize, Option<usize>)> = vec![(root, None)];

    let mut cycle = false;

    while let Some((node, from_edge)) = stack.pop() {
        if visited[node] {
            continue;
        }

        visited[node] = true;
        parent_edge[node] = from_edge;

        if let Some(from_edge) = from_edge {
            let e = &graph.edges[from_edge];
            parent_node[node] = Some(if e.node_a == node { e.node_b } else { e.node_a });
        }

        for &e_idx in &graph.adj[node] {
            if Some(e_idx) == from_edge {
                continue;
            }

            let e = &graph.edges[e_idx];
            let next = if e.node_a == node { e.node_b } else { e.node_a };

            if visited[next] {
                cycle = true;
                continue;
            }

            stack.push((next, Some(e_idx)));
        }
    }

    (cycle, parent_node, parent_edge, visited)
}

/// Port of `CHAIN_TOPOLOGY`: the topology of a single named net chain's routed copper.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainTopology {
    /// `m_chainName`
    chain_name: String,
    /// `m_status`
    status: ChainTopologyStatus,
    /// `m_trunkLength`
    trunk_length: f64,
    /// `m_trunkDelay`
    trunk_delay: f64,
    /// `m_stubs`
    stubs: Vec<Stub>,
}

impl ChainTopology {
    /// Port of `CHAIN_TOPOLOGY( aBoard, aChainName, aChainItems, aDefaultUnitDelay )`.
    pub fn new(
        calc: &LengthDelayCalculation<'_>,
        chain_name: &str,
        chain_items: &BTreeSet<ItemId>,
        default_unit_delay: f64,
    ) -> Self {
        let mut topo = ChainTopology {
            chain_name: chain_name.to_string(),
            status: ChainTopologyStatus::NoItems,
            trunk_length: 0.0,
            trunk_delay: 0.0,
            stubs: Vec::new(),
        };

        topo.build(calc, chain_items, default_unit_delay);
        topo
    }

    fn build(&mut self, calc: &LengthDelayCalculation<'_>, chain_items: &BTreeSet<ItemId>, default_unit_delay: f64) {
        let ctx = calc.ctx();

        if self.chain_name.is_empty() || chain_items.is_empty() {
            self.status = ChainTopologyStatus::NoItems;
            return;
        }

        // Locate the two terminal pads from the first member-net we encounter
        // that has them set.  Per chain semantics every member-net carries the
        // same terminal-pad pair, so reading from any one is sufficient.
        let mut terminal_a = None;
        let mut terminal_b = None;

        for &item in chain_items {
            let Some(ni) = ctx.item(item).and_then(|i| i.net()) else {
                continue;
            };

            if let Some(p0) = ni.terminal_pad(0) {
                terminal_a = Some(p0.item_id());
            }

            if let Some(p1) = ni.terminal_pad(1) {
                terminal_b = Some(p1.item_id());
            }

            if terminal_a.is_some() && terminal_b.is_some() {
                break;
            }
        }

        let (Some(terminal_a), Some(terminal_b)) = (terminal_a, terminal_b) else {
            self.status = ChainTopologyStatus::NoTerminalPads;
            return;
        };

        // Build the graph.
        let mut graph = Graph::default();
        let fallback_delay_per_mm = chain_bridging_delay_per_mm(ctx, &self.chain_name, default_unit_delay);

        // Collect pads belonging to chain-member nets — including ones not in
        // aChainItems (terminal pads sit on the chain but might not be in the
        // matched-length sweep's per-net set if they own no routing segments
        // attached to a tracked rule).  We need those pad nodes for the graph
        // anchors regardless.
        let mut seen_pads: BTreeSet<ItemId> = BTreeSet::new();

        let register_all_pad_layers = |graph: &mut Graph, seen_pads: &mut BTreeSet<ItemId>, pad: ItemId| {
            let Some(pad_ref) = ctx.item(pad) else {
                return;
            };

            if !seen_pads.insert(pad) {
                return;
            }

            add_pad_nodes(graph, ctx, pad_ref);

            // Track which nodes belong to this pad for later terminal lookup.
            let center = pad_center(ctx, pad_ref);
            let mut list = Vec::new();

            for layer in pad_cu_stack(pad_ref) {
                list.push(graph.add_node(center, layer));
            }

            graph.pad_nodes.insert(pad, list);
        };

        register_all_pad_layers(&mut graph, &mut seen_pads, terminal_a);
        register_all_pad_layers(&mut graph, &mut seen_pads, terminal_b);

        // Edges from tracks, arcs, vias, and pads in the matched item set.
        for &item in chain_items {
            let Some(item_ref) = ctx.item(item) else {
                continue;
            };

            match item_ref.type_() {
                KicadT::PcbTraceT | KicadT::PcbArcT => {
                    let track = item_ref.as_track().expect("a track").as_track();
                    let layer = item_ref.layer();
                    let n_a = graph.add_node(track.start(), layer);
                    let n_b = graph.add_node(track.end(), layer);
                    let len = item_ref.as_track().expect("a track").length();
                    let dly = track_delay(item_ref, len, calc, fallback_delay_per_mm);
                    graph.add_edge(n_a, n_b, len, dly, Some(item), layer);
                }
                KicadT::PcbViaT => {
                    add_via_edges(&mut graph, item_ref, calc);
                }
                KicadT::PcbPadT => {
                    register_all_pad_layers(&mut graph, &mut seen_pads, item);
                }
                _ => {}
            }
        }

        // Bridge edges through series passives.
        let bridges = enumerate_chain_bridges(ctx, &self.chain_name, default_unit_delay);

        for br in &bridges {
            register_all_pad_layers(&mut graph, &mut seen_pads, br.pad_a.item_id());
            register_all_pad_layers(&mut graph, &mut seen_pads, br.pad_b.item_id());

            let pad_a = ctx.item(br.pad_a.item_id()).expect("a pad");
            let pad_b = ctx.item(br.pad_b.item_id()).expect("a pad");

            // Pick the bridge's anchor layer: footprint placement layer for SMD
            // pads, topmost shared copper layer for THT.
            let mut anchor_layer = pad_a
                .parent_footprint()
                .map_or(PcbLayerId::FCu, |fp| fp.layer());

            let common_cu = &(&pad_a.layer_set() & &pad_b.layer_set()) & Lset::all_cu_mask();

            if !common_cu.contains(anchor_layer) {
                let stack = common_cu.cu_stack().0;

                if let Some(&first) = stack.first() {
                    anchor_layer = first;
                } else {
                    continue;
                }
            }

            let n_a = graph.add_node(pad_center(ctx, pad_a), anchor_layer);
            let n_b = graph.add_node(pad_center(ctx, pad_b), anchor_layer);
            graph.add_edge(n_a, n_b, br.length, br.delay, None, anchor_layer);
        }

        // T-junction split pass on the populated graph.
        split_track_t_junctions(&mut graph, ctx);

        // Tag terminal nodes — pick any layer in the pad's LSET (the pad has zero
        // intrinsic length so any anchor layer works for path queries; tracks
        // terminating at the pad on layer L will share the same (x, y, L) node).
        let first_pad_node =
            |graph: &mut Graph, pad: ItemId| graph.pad_nodes.entry(pad).or_default().first().copied();

        let root_a = first_pad_node(&mut graph, terminal_a);
        let root_b = first_pad_node(&mut graph, terminal_b);

        let (Some(root_a), Some(root_b)) = (root_a, root_b) else {
            self.status = ChainTopologyStatus::NoTerminalPads;
            return;
        };

        // Bridge zero-length edges between every pair of pad-anchor layers so
        // that a pad on multiple layers (e.g. THT) is one logical graph node.
        let pad_node_lists: Vec<Vec<usize>> = graph.pad_nodes.values().cloned().collect();

        for nodes in &pad_node_lists {
            for &node in nodes.iter().skip(1) {
                let layer = graph.node_layer[node];
                graph.add_edge(nodes[0], node, 0.0, 0.0, None, layer);
            }
        }

        // DFS spanning tree from terminalA.
        let (cycle, parent_node, parent_edge, visited) = dfs_spanning_tree(&graph, root_a);

        if cycle {
            self.status = ChainTopologyStatus::CycleDetected;
            return;
        }

        if !visited[root_b] {
            self.status = ChainTopologyStatus::Disconnected;
            return;
        }

        // Recover trunk: walk from rootB back to rootA via parent pointers.
        let mut trunk_edges: Vec<usize> = Vec::new();
        let mut trunk_nodes: Vec<usize> = Vec::new();

        {
            let mut cur = Some(root_b);

            while let Some(c) = cur {
                if c == root_a {
                    break;
                }

                trunk_nodes.push(c);

                let Some(e) = parent_edge[c] else {
                    break;
                };

                trunk_edges.push(e);
                cur = parent_node[c];
            }

            if cur != Some(root_a) {
                self.status = ChainTopologyStatus::Disconnected;
                return;
            }

            trunk_nodes.push(root_a);
        }

        // Sum trunk length / delay.
        self.trunk_length = 0.0;
        self.trunk_delay = 0.0;

        for &e_idx in &trunk_edges {
            self.trunk_length += graph.edges[e_idx].length;
            self.trunk_delay += graph.edges[e_idx].delay;
        }

        // Stub identification: BFS from each non-trunk node not yet on a stub
        // component, following non-trunk edges only, and find the unique
        // trunk-touching node.
        let trunk_node_set: BTreeSet<usize> = trunk_nodes.iter().copied().collect();
        let trunk_edge_set: BTreeSet<usize> = trunk_edges.iter().copied().collect();
        let mut node_in_stub_component = vec![false; graph.node_pos.len()];

        for start_node in 0..graph.node_pos.len() {
            if !visited[start_node] {
                continue;
            }

            if trunk_node_set.contains(&start_node) {
                continue;
            }

            if node_in_stub_component[start_node] {
                continue;
            }

            // BFS this stub component (via edges *not* in trunkEdgeSet) and
            // collect all of its nodes + items + length/delay + every trunk node
            // it touches.
            let mut comp_nodes: BTreeSet<usize> = BTreeSet::new();
            let mut comp_edges: BTreeSet<usize> = BTreeSet::new();
            let mut trunk_touches: BTreeSet<usize> = BTreeSet::new();
            let mut items: Vec<ItemId> = Vec::new();
            let mut comp_length = 0.0;
            let mut comp_delay = 0.0;
            let mut any_layer = PcbLayerId::UndefinedLayer;

            let mut q: VecDeque<usize> = VecDeque::new();
            q.push_back(start_node);
            comp_nodes.insert(start_node);

            while let Some(n) = q.pop_front() {
                for &e_idx in &graph.adj[n] {
                    let e = &graph.edges[e_idx];
                    let other = if e.node_a == n { e.node_b } else { e.node_a };

                    if trunk_edge_set.contains(&e_idx) {
                        // Edge into the trunk — only record the touch.  The
                        // edge itself belongs to the trunk and is not part of
                        // the stub component.
                        if trunk_node_set.contains(&other) {
                            trunk_touches.insert(other);
                        }

                        continue;
                    }

                    // Accumulate this stub edge exactly once across the BFS.
                    // Tracking visited edges (not visited nodes) is the right
                    // primitive: nothing about traversal order can correctly be
                    // inferred from node-index comparisons because BFS may
                    // reach a higher-index node from a lower one.
                    if comp_edges.insert(e_idx) {
                        comp_length += e.length;
                        comp_delay += e.delay;

                        if let Some(src) = e.source_item {
                            items.push(src);
                        }

                        if any_layer == PcbLayerId::UndefinedLayer {
                            any_layer = e.layer;
                        }
                    }

                    if trunk_node_set.contains(&other) {
                        trunk_touches.insert(other);
                        continue;
                    }

                    if comp_nodes.insert(other) {
                        q.push_back(other);
                    }
                }
            }

            for &n in &comp_nodes {
                node_in_stub_component[n] = true;
            }

            if trunk_touches.is_empty() {
                // Floating copper — DRCE_UNCONNECTED_ITEMS will catch it.
                continue;
            }

            if trunk_touches.len() >= 2 {
                // Two or more trunk-touching points imply a cycle that the
                // earlier DFS missed.  Defensive bail.
                self.status = ChainTopologyStatus::CycleDetected;
                return;
            }

            let branch_node = *trunk_touches.iter().next().expect("a trunk touch");

            self.stubs.push(Stub {
                branch_point: graph.node_pos[branch_node],
                branch_layer: if any_layer != PcbLayerId::UndefinedLayer {
                    any_layer
                } else {
                    graph.node_layer[branch_node]
                },
                items,
                length: comp_length,
                delay: comp_delay,
            });
        }

        self.status = ChainTopologyStatus::Ok;
    }

    /// `GetStatus()`.
    pub fn status(&self) -> ChainTopologyStatus {
        self.status
    }

    /// `IsValid()`.
    pub fn is_valid(&self) -> bool {
        self.status == ChainTopologyStatus::Ok
    }

    /// `TrunkLength()`.
    pub fn trunk_length(&self) -> f64 {
        self.trunk_length
    }

    /// `TrunkDelay()`.
    pub fn trunk_delay(&self) -> f64 {
        self.trunk_delay
    }

    /// `Stubs()`.
    pub fn stubs(&self) -> &[Stub] {
        &self.stubs
    }

    /// `GetChainName()`.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }
}
