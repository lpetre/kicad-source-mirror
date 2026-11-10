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

//! Port of `pcbnew/drc/drc_creepage_engine.h` / `drc_creepage_engine.cpp`: the reusable
//! creepage solver shared by the batch DRC provider (the `RealtimeCreepage` path) and the
//! realtime drag overlay.
//!
//! The C++ engine keeps a `BOARD&` and, in interactive mode, a graph across frames while the
//! dragged items move on the board. The port cannot hold a borrow of a board that changes
//! between frames: every method takes the board context of the moment, and the interactive
//! graph is parked between calls as a [`DetachedGraph`] (the graph without its context), which
//! is the same object as the C++ `m_graph`.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::{Box2I, ShapePolySet, Vector2I};
use kicad_pcb::ids::{ItemId, NetCode};
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{AnyItem, BoardCtx};

use crate::creepage_utils::graph::{
    ConnId, CreepageGraph, GraphConnection, GraphNode, GraphNodeType, NodeId, ShapeId,
};
use crate::creepage_utils::shapes::vi;
use crate::creepage_utils::{CreepItem, CreepShape, build_creepage_board_edges};

/// Port of `CREEPAGE_RESULT`: the result of a single creepage query between two nets on one
/// layer.
///
/// `path` holds the rendered creepage polyline, identical to the geometry the batch DRC
/// marker uses, so the realtime overlay and the eventual DRC marker draw the same shape.
#[derive(Clone, Debug)]
pub struct CreepageResult {
    /// `m_netA`
    pub net_a: i32,
    /// `m_netB`
    pub net_b: i32,
    /// `m_distance`
    pub distance: f64,
    /// `m_constraint`
    pub constraint: f64,
    /// `m_violation`
    pub violation: bool,
    /// `m_start`
    pub start: Vector2I,
    /// `m_end`
    pub end: Vector2I,
    /// `m_itemA`
    pub item_a: Option<CreepItem>,
    /// `m_itemB`
    pub item_b: Option<CreepItem>,
    /// `m_path`
    pub path: Vec<PcbShape>,
}

impl Default for CreepageResult {
    fn default() -> Self {
        CreepageResult {
            net_a: -1,
            net_b: -1,
            distance: 0.0,
            constraint: 0.0,
            violation: false,
            start: Vector2I::default(),
            end: Vector2I::default(),
            item_a: None,
            item_b: None,
            path: Vec::new(),
        }
    }
}

/// The node lookup set of a graph (`CreepageGraph::nodeset`).
type NodeSet = BTreeMap<(GraphNodeType, Option<ShapeId>, i32, i32), NodeId>;

/// A [`CreepageGraph`] without its board context: the interactive `m_graph` (with the
/// `m_outline` it points to) between two calls.
#[derive(Clone, Debug, Default)]
pub struct DetachedGraph {
    board_edge: Vec<CreepItem>,
    owned_board_edges: Vec<PcbShape>,
    board_outline: Option<ShapePolySet>,
    has_overlapping_cutouts: bool,
    nodes: Vec<GraphNode>,
    connections: Vec<GraphConnection>,
    shapes: Vec<CreepShape>,
    shape_collection: Vec<ShapeId>,
    nodeset: NodeSet,
    min_groove_width: i32,
    target: f64,
}

impl DetachedGraph {
    /// Takes the graph's state, dropping its context.
    pub fn detach(g: CreepageGraph<'_>) -> Self {
        DetachedGraph {
            target: g.target(),
            board_edge: g.board_edge,
            owned_board_edges: g.owned_board_edges,
            board_outline: g.board_outline,
            has_overlapping_cutouts: g.has_overlapping_cutouts,
            nodes: g.nodes,
            connections: g.connections,
            shapes: g.shapes,
            shape_collection: g.shape_collection,
            nodeset: g.nodeset,
            min_groove_width: g.min_groove_width,
        }
    }

    /// The graph with the context `ctx`.
    pub fn attach<'a>(self, ctx: &'a BoardCtx<'a>) -> CreepageGraph<'a> {
        let mut g = CreepageGraph::new(ctx);
        g.board_edge = self.board_edge;
        g.owned_board_edges = self.owned_board_edges;
        g.board_outline = self.board_outline;
        g.has_overlapping_cutouts = self.has_overlapping_cutouts;
        g.nodes = self.nodes;
        g.connections = self.connections;
        g.shapes = self.shapes;
        g.shape_collection = self.shape_collection;
        g.nodeset = self.nodeset;
        g.min_groove_width = self.min_groove_width;
        g.set_target(self.target);
        g
    }
}

/// The constraint function of an interactive session (`std::function<double( int, int )>`).
pub type ConstraintFn = Box<dyn Fn(i32, i32) -> f64>;

/// Port of `CREEPAGE_ENGINE`: the reusable creepage solver shared by the batch DRC provider and
/// the realtime drag overlay.
///
/// In whole-board mode ([`CreepageEngine::solve_net_pair_whole_board`]) the engine rebuilds a
/// graph for one net pair and is the reference used to prove exact parity with the legacy
/// inline path.
///
/// In interactive mode the engine builds the static board-edge plus other-net sub-graph once
/// inside a constraint-bounded ROI ([`CreepageEngine::begin_interactive`]), then per frame
/// truncates back to that static prefix, re-adds only the dragged item's geometry at its
/// current board position, links just the moving geometry, and solves the dragged nets against
/// their neighbours ([`CreepageEngine::update`]).
#[derive(Default)]
pub struct CreepageEngine {
    /// `m_minGrooveWidth`
    min_groove_width: i32,
    /// `m_interactive`
    interactive: bool,
    /// `m_dynamicEdges`
    dynamic_edges: bool,
    /// `m_layer`
    layer: Option<PcbLayerId>,
    /// `m_margin`
    margin: i32,
    /// `m_affectedNets`
    affected_nets: BTreeSet<i32>,
    /// `m_movingItems`
    moving_items: BTreeSet<ItemId>,
    /// `m_constraintFn`
    constraint_fn: Option<ConstraintFn>,
    /// Bounding box of every non-affected net, cached once at drag start. Non-affected
    /// geometry is static during a drag, so this avoids recomputing
    /// `NETINFO_ITEM::GetBoundingBox` (a full connectivity sweep) for every net on every
    /// frame. (`m_staticNetBBoxes`)
    static_net_bboxes: BTreeMap<i32, Box2I>,
    /// `m_graph` (with `m_outline`)
    graph: Option<DetachedGraph>,
    /// The static board-edge sub-graph prefix: nodes `[0, static_node_count)` and connections
    /// `[0, static_conn_count)` are pure board geometry, reused every frame.
    static_node_count: usize,
    /// See `static_node_count`.
    static_conn_count: usize,
}

impl CreepageEngine {
    /// Port of `CREEPAGE_ENGINE( BOARD& aBoard )`.
    pub fn new() -> Self {
        CreepageEngine::default()
    }

    /// Port of `SetMinGrooveWidth()`.
    pub fn set_min_groove_width(&mut self, a_width: i32) {
        self.min_groove_width = a_width;
    }

    /// Port of `IsInteractive()`.
    pub fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Port of `populateBoardEdgeGraph()`: build the board-edge geometry into `a_graph`:
    /// groove width, outline (NPTH-subtracted), edge shapes, and the node decomposition.
    /// Shared by the whole-board solve and the interactive prefix so they stay in lockstep.
    fn populate_board_edge_graph(&self, a_graph: &mut CreepageGraph<'_>) {
        a_graph.min_groove_width = self.min_groove_width;
        let ctx = a_graph.ctx;

        // Subtract NPTH holes from the outline so paths through slot interiors are rejected
        // (#24286)
        let (outline, has_outline) = ctx
            .board
            .get_board_polygon_outlines(ctx, false, None, false, true);

        build_creepage_board_edges(
            ctx,
            &mut a_graph.board_edge,
            &mut a_graph.owned_board_edges,
            None,
        );
        a_graph.board_outline = has_outline.then_some(outline);

        a_graph.transform_edge_to_creep_shapes();
        a_graph.remove_duplicated_shapes();
        a_graph.transform_creep_shapes_to_nodes();
    }

    /// Port of `connectChildren()`: the same-parent `ConnectChildren` pass the legacy solver
    /// runs before Dijkstra.
    pub fn connect_children(a_graph: &mut CreepageGraph<'_>) {
        let temp_nodes: Vec<NodeId> = (0..a_graph.nodes.len())
            .map(NodeId)
            .filter(|&n| {
                let node = &a_graph.nodes[n.0];
                node.parent
                    .is_some_and(|p| !a_graph.shape(p).is_conductive())
                    && !node.connect_directly
                    && node.type_ == GraphNodeType::Point
            })
            .collect();

        // alg::for_all_pairs
        for i in 0..temp_nodes.len() {
            for j in i + 1..temp_nodes.len() {
                let (n1, n2) = (temp_nodes[i], temp_nodes[j]);

                if n1 == n2 {
                    continue;
                }

                let (p1, p2) = (a_graph.nodes[n1.0].parent, a_graph.nodes[n2.0].parent);

                let (Some(p1), Some(p2)) = (p1, p2) else {
                    continue;
                };

                if p1 != p2 {
                    continue;
                }

                a_graph.connect_children(p1, n1, n2);
            }
        }
    }

    /// Port of `extractResult()`: a [`CreepageResult`] from a solved shortest path, or `None`
    /// if not a violation.
    pub fn extract_result(
        a_graph: &CreepageGraph<'_>,
        a_path: &[ConnId],
        a_distance: f64,
        a_net_a: i32,
        a_net_b: i32,
        a_constraint: f64,
        a_near_margin: i32,
    ) -> Option<CreepageResult> {
        if a_path.is_empty() || a_path.len() < 4 {
            return None;
        }

        // aNearMargin == 0 reproduces the legacy "distance < constraint" test exactly; the
        // overlay passes a positive margin to also surface near misses
        if !(a_distance - a_constraint < a_near_margin as f64) {
            return None;
        }

        let gc1 = &a_graph.connections[a_path[1].0];
        let gc2 = &a_graph.connections[a_path[a_path.len() - 2].0];

        let mut result = CreepageResult {
            net_a: a_net_a,
            net_b: a_net_b,
            distance: a_distance,
            constraint: a_constraint,
            violation: a_distance < a_constraint,
            start: vi(gc1.path.a2),
            end: vi(gc2.path.a2),
            ..CreepageResult::default()
        };

        // Connections [1] and [size-2] sit on real shape nodes; the path ends are zero-weight
        // links to the per-net virtual nodes
        if let Some(p) = a_graph.nodes[gc1.n1.0].parent {
            result.item_a = a_graph.shape(p).parent();
        }

        if let Some(p) = a_graph.nodes[gc2.n2.0].parent {
            result.item_b = a_graph.shape(p).parent();
        }

        for &gc in a_path {
            a_graph.connection_shapes(gc, &mut result.path);
        }

        Some(result)
    }

    /// Port of `SolveNetPairWholeBoard()`: solve a single net pair against the whole board on
    /// one layer, building a fresh graph.
    ///
    /// Returns the creepage result when a path shorter than the constraint exists (a
    /// violation), otherwise `None`. This is the parity reference for the batch provider.
    pub fn solve_net_pair_whole_board<'a>(
        &mut self,
        ctx: &'a BoardCtx<'a>,
        a_net_a: i32,
        a_net_b: i32,
        a_layer: PcbLayerId,
        a_constraint: f64,
    ) -> Option<CreepageResult> {
        if a_constraint <= 0.0 {
            return None;
        }

        let net_a = ctx.board.find_net(NetCode(a_net_a))?;
        let net_b = ctx.board.find_net(NetCode(a_net_b))?;

        if net_a
            .bounding_box(ctx)
            .distance_box(&net_b.bounding_box(ctx)) as f64
            > a_constraint
        {
            return None;
        }

        let mut graph = CreepageGraph::new(ctx);
        self.populate_board_edge_graph(&mut graph);

        graph.generate_paths(a_constraint, PcbLayerId::EdgeCuts, None);
        graph.set_target(a_constraint);

        // AddNetElements( int, PCB_LAYER_ID, int aMaxCreepage ): the double converts to int
        let max_creepage = kicad_geom::cmath::f64_to_i32_x86(a_constraint);
        let node_a = graph.add_net_elements(a_net_a, a_layer, max_creepage);
        let node_b = graph.add_net_elements(a_net_b, a_layer, max_creepage);

        graph.generate_paths(a_constraint, a_layer, None);

        Self::connect_children(&mut graph);

        let mut shortest_path: Vec<ConnId> = Vec::new();
        let distance = graph.solve(Some(node_a), Some(node_b), &mut shortest_path);

        Self::extract_result(
            &graph,
            &shortest_path,
            distance,
            a_net_a,
            a_net_b,
            a_constraint,
            0,
        )
    }

    /// Port of `addNetsInRegion()`: add net elements for the affected nets plus every other
    /// net whose bbox intersects `a_region`. Returns the per-net virtual node keyed by net
    /// code.
    fn add_nets_in_region(
        &self,
        graph: &mut CreepageGraph<'_>,
        a_region: &Box2I,
    ) -> BTreeMap<i32, NodeId> {
        let mut added = BTreeMap::new();
        let layer = self.layer.unwrap_or(PcbLayerId::UndefinedLayer);

        // Iterate in net-code order so node creation order matches the whole-board solve
        let codes: Vec<i32> = graph
            .ctx
            .board
            .nets_by_netcode()
            .map(|n| n.net_code.0)
            .collect();

        for net_code in codes {
            if net_code <= 0 {
                continue;
            }

            if !self.affected_nets.contains(&net_code) {
                match self.static_net_bboxes.get(&net_code) {
                    Some(bbox) if bbox.intersects(a_region) => {}
                    _ => continue,
                }
            }

            added.insert(
                net_code,
                graph.add_net_elements(net_code, layer, self.margin),
            );
        }

        added
    }

    /// Port of `BeginInteractive()`: begin an interactive drag session.
    ///
    /// Builds the static board-edge sub-graph once and records it as the immutable graph
    /// prefix. `a_affected_nets` are the nets carried by the dragged items; `a_margin` bounds
    /// the search radius (the largest creepage constraint plus a near-violation band). Other
    /// nets' geometry and the copper paths are rebuilt each frame within that margin of the
    /// dragged items.
    pub fn begin_interactive<'a>(
        &mut self,
        ctx: &'a BoardCtx<'a>,
        a_layer: PcbLayerId,
        a_affected_nets: &BTreeSet<i32>,
        a_moving_items: &BTreeSet<ItemId>,
        a_margin: i32,
        a_constraint_fn: Option<ConstraintFn>,
    ) {
        self.interactive = true;
        self.layer = Some(a_layer);
        self.affected_nets = a_affected_nets.clone();
        self.moving_items = a_moving_items.clone();
        self.constraint_fn = a_constraint_fn;
        self.margin = a_margin;

        self.static_net_bboxes.clear();

        for net in ctx.board.nets_by_netcode() {
            let net_code = net.net_code.0;

            if net_code > 0 && !self.affected_nets.contains(&net_code) {
                self.static_net_bboxes
                    .entry(net_code)
                    .or_insert_with(|| net.bounding_box(ctx));
            }
        }

        self.dynamic_edges = self.moving_items_have_npth(ctx);

        self.build_board_edge_prefix(ctx);
    }

    /// Port of `buildBoardEdgePrefix()`: (re)build the board-edge visibility sub-graph from
    /// the board at its current state and record it as the prefix. Called once at drag start,
    /// or every frame when a dragged item carries NPTH holes (which are moving board-edge
    /// obstacles).
    fn build_board_edge_prefix<'a>(&mut self, ctx: &'a BoardCtx<'a>) {
        let mut graph = CreepageGraph::new(ctx);
        self.populate_board_edge_graph(&mut graph);
        graph.set_target(self.margin as f64);

        // The board-edge sub-graph is pure board geometry, independent of any track, so it is
        // the cacheable prefix.  Copper-layer paths depend on every track through the
        // collision test and must be rebuilt each frame.
        graph.generate_paths(self.margin as f64, PcbLayerId::EdgeCuts, None);
        Self::connect_children(&mut graph);

        self.static_node_count = graph.nodes.len();
        self.static_conn_count = graph.connections.len();
        self.graph = Some(DetachedGraph::detach(graph));
    }

    /// Port of `movingItemsHaveNPTH()`: true when any dragged item carries an NPTH hole,
    /// making the board-edge graph position dependent and therefore not cacheable across
    /// frames.
    fn moving_items_have_npth(&self, ctx: &BoardCtx<'_>) -> bool {
        let board = ctx.board;

        for &id in &self.moving_items {
            let Some(item) = board.get(id) else {
                continue;
            };

            match item {
                AnyItem::Pad(pad) => {
                    if pad.attribute() == PadAttrib::Npth {
                        return true;
                    }
                }
                AnyItem::Footprint(_) => {
                    let fp: Option<kicad_pcb::ids::FootprintId> = id.typed();

                    if let Some(fp) = fp {
                        for (_, pad) in board.footprint_pads(fp) {
                            if pad.attribute() == PadAttrib::Npth {
                                return true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        false
    }

    /// Port of `Update()`: recompute creepage for the dragged nets at the items' current board
    /// positions.
    ///
    /// Returns the nearest creepage path per affected net pair whose distance is below
    /// constraint + `a_near_margin`. Reuses the static board-edge prefix; rebuilds the copper
    /// paths.
    pub fn update<'a>(&mut self, ctx: &'a BoardCtx<'a>, a_near_margin: i32) -> Vec<CreepageResult> {
        let mut results = Vec::new();

        if !self.interactive || self.graph.is_none() {
            return results;
        }

        if self.dynamic_edges {
            self.build_board_edge_prefix(ctx);
        }

        let mut graph = self.graph.take().expect("checked above").attach(ctx);

        if !self.dynamic_edges {
            graph.truncate_to_prefix(self.static_node_count, self.static_conn_count);
        }

        let mut roi = Box2I::default();

        for &id in &self.moving_items {
            if let Some(item) = ctx.item(id) {
                roi.merge(&item.bounding_box());
            }
        }

        roi.inflate(self.margin);

        let net_nodes = self.add_nets_in_region(&mut graph, &roi);

        graph.generate_paths(
            self.margin as f64,
            self.layer.unwrap_or(PcbLayerId::UndefinedLayer),
            Some(&self.affected_nets),
        );
        Self::connect_children(&mut graph);

        let mut solve = |a_net_a: i32, a_node_a: NodeId, a_net_b: i32, a_node_b: NodeId| {
            let constraint = self
                .constraint_fn
                .as_ref()
                .map_or(0.0, |f| f(a_net_a, a_net_b));

            if constraint <= 0.0 {
                return;
            }

            let mut path: Vec<ConnId> = Vec::new();
            let distance = graph.solve(Some(a_node_a), Some(a_node_b), &mut path);

            if let Some(r) = Self::extract_result(
                &graph,
                &path,
                distance,
                a_net_a,
                a_net_b,
                constraint,
                a_near_margin,
            ) {
                results.push(r);
            }
        };

        for &affected in &self.affected_nets {
            let Some(&node_a) = net_nodes.get(&affected) else {
                continue;
            };

            for (&net_b, &node_b) in &net_nodes {
                if net_b == affected {
                    continue;
                }

                // Avoid solving an affected-affected pair twice
                if self.affected_nets.contains(&net_b) && net_b < affected {
                    continue;
                }

                solve(affected, node_a, net_b, node_b);
            }
        }

        self.graph = Some(DetachedGraph::detach(graph));

        results
    }

    /// Port of `EndInteractive()`.
    pub fn end_interactive(&mut self) {
        self.interactive = false;
        self.dynamic_edges = false;
        self.graph = None;
        self.affected_nets.clear();
        self.moving_items.clear();
        self.static_net_bboxes.clear();
        self.constraint_fn = None;
        self.static_node_count = 0;
        self.static_conn_count = 0;
    }
}
