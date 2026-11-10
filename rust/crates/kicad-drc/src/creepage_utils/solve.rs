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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of `GetPaths()`, `CREEPAGE_GRAPH::Solve()`, `CREEPAGE_GRAPH::Addshape()`,
//! `CREEPAGE_GRAPH::GeneratePaths()` and `CREEPAGE_GRAPH::AddNetElements()` of
//! `pcbnew/drc/drc_creepage_utils.cpp` (lines 2252-3015 and 3121-3185).
//!
//! Order. The C++ orders several containers by heap address: `GeneratePaths` sorts the nodes by
//! their `CREEP_SHAPE*` (here [`ShapeId`], the creation order), compares `BOARD_ITEM*` parents
//! (here the `Ord` of [`CreepItem`]: `ItemId` order, the port's replacement for pointer order,
//! owned board edges after board items) and walks pointer-keyed `unordered_map`s (here first
//! insertion order); `Solve` breaks distance ties by `GRAPH_NODE*` (here [`NodeId`]) and walks
//! `m_node_conns` (here [`ConnId`] order). These orders only decide the creation order of nodes
//! and connections (the graph is the same set) and, between equally short paths, which one is
//! returned: the reference is not reproducible there, compare tie-aware.
//!
//! Scheduling. `GeneratePaths` hands the parent search and the path generation to the thread
//! pool only when `tp.get_tasks_total() < tp.get_thread_count() - 4` (and more than 100 parents
//! for the search): on a machine with at most 4 hardware threads (the reference machine) the
//! unsigned right side is 0 or wraps, so both run inline. The port is sequential everywhere.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use kicad_core::layer_ids::{Flashing, PcbLayerId};
use kicad_geom::geometry::rtree::PackedRTree;
use kicad_geom::{Box2I, Seg, ShapeRef, ShapeSegment};
use kicad_geom::geometry::shape::Shape;
use kicad_pcb::eda_shape::{EdaShape, FillT, ShapeT};
use kicad_pcb::pcb_track::Track;

use super::board_edges::board_edge_bounding_box;
use super::graph::{ConnId, CreepageGraph, GraphNodeType, NodeId};
use super::paths::{CreepageTrackEntry, PathConnection, TrackRtree};
use super::shapes::{
    CreepItem, CreepShape, CreepShapeType, CuShapeArc, CuShapeCircle, CuShapeSegment, vd, vi,
};
use crate::constraint_types::DrcConstraintT;

/// Port of `GetPaths()`: the candidate paths between two shapes. The C++ `dynamic_cast` chain
/// selects the overload of the most derived classes of both shapes, which is what
/// [`CreepShape::paths`] dispatches on.
pub fn get_paths(a_s1: &CreepShape, a_s2: &CreepShape, a_max_weight: f64) -> Vec<PathConnection> {
    let max_weight = a_max_weight;
    let max_weight_squared = max_weight * max_weight;

    a_s1.paths(a_s2, max_weight, max_weight_squared)
}

/// A `std::pair<double, GRAPH_NODE*>` of the Dijkstra queue with the order of its comparator
/// (`cmp( l, r )` is true when `l` comes out after `r`): the smallest distance first, between
/// equal distances the smallest node. `BinaryHeap` pops the greatest, so "greater" here means
/// "popped first".
#[derive(Clone, Copy, Debug)]
struct QueueItem(f64, NodeId);

impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for QueueItem {}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // cmp( aLeft, aRight ): aLeft.first == aRight.first ? aLeft.second > aRight.second
        //                                                   : aLeft.first > aRight.first
        // "cmp( a, b )" means a has the lower priority (comes out later).
        let a_after_b = |a: &QueueItem, b: &QueueItem| {
            if a.0 == b.0 { a.1 > b.1 } else { a.0 > b.0 }
        };

        if a_after_b(self, other) {
            Ordering::Less
        } else if a_after_b(other, self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

/// A parent entry of the parent r-tree of `GeneratePaths` (`ParentEntry`).
#[derive(Clone, Copy, Debug)]
struct ParentEntry {
    parent: CreepItem,
    bbox: Box2I,
}

/// The nodes of one parent, grouped by net (`std::unordered_map<int, std::vector<...>>`, in
/// first insertion order).
type NetGroups = Vec<(i32, Vec<NodeId>)>;

impl<'a> CreepageGraph<'a> {
    /// Port of `CREEPAGE_GRAPH::Solve()`: Dijkstra's shortest path from `a_from` to `a_to`.
    /// The connections of the path are appended to `a_result` from `a_to` back to `a_from`.
    /// Returns the path weight, infinity when `a_to` is unreachable, 0 for a missing or
    /// identical end.
    pub fn solve(
        &self,
        a_from: Option<NodeId>,
        a_to: Option<NodeId>,
        a_result: &mut Vec<ConnId>,
    ) -> f64 {
        let (Some(a_from), Some(a_to)) = (a_from, a_to) else {
            return 0.0;
        };

        if a_from == a_to {
            return 0.0;
        }

        // Dijkstra's algorithm for shortest path
        // (`distances[node]` default-inserts 0.0 for a node outside m_nodes; every node is in
        // it here.)
        let mut distances: Vec<f64> = vec![f64::INFINITY; self.nodes.len()];
        let mut previous: Vec<Option<NodeId>> = vec![None; self.nodes.len()];

        // Each heap entry carries the tentative distance captured at push time. A comparator
        // that read the live distances map instead would let a decrease-key reinsertion
        // silently corrupt the heap ordering, so aTo could be popped on a non-shortest path
        // and the early break would return it.
        let mut pq: BinaryHeap<QueueItem> = BinaryHeap::new();

        // Initialize distances to infinity for all nodes except the starting node
        distances[a_from.0] = 0.0;
        distances[a_to.0] = f64::INFINITY;
        pq.push(QueueItem(0.0, a_from));

        // Dijkstra's main loop
        while let Some(QueueItem(dist, current)) = pq.pop() {
            // A stale entry left behind by a decrease-key reinsertion; its shorter copy was
            // already processed
            if dist > distances[current.0] {
                continue;
            }

            if current == a_to {
                break; // Shortest path found
            }

            // Traverse neighbors
            for &connection in &self.nodes[current.0].node_conns {
                let gc = &self.connections[connection.0];
                let neighbor = if gc.n1 == current { gc.n2 } else { gc.n1 };

                // Ignore connections with negative weights as Dijkstra doesn't support them.
                if gc.path.weight < 0.0 {
                    continue;
                }

                // Calculate alternative path cost
                let alt = distances[current.0] + gc.path.weight;

                if alt < distances[neighbor.0] {
                    distances[neighbor.0] = alt;
                    previous[neighbor.0] = Some(current);
                    pq.push(QueueItem(alt, neighbor));
                }
            }
        }

        let path_weight = distances[a_to.0];

        // If aTo is unreachable, return infinity
        if path_weight == f64::INFINITY {
            return f64::INFINITY;
        }

        // Trace back the path from aTo to aFrom
        let mut step = a_to;

        while step != a_from {
            let Some(prev_node) = previous[step.0] else {
                // previous[step] is a null pointer: the C++ would dereference it. Unreachable
                // for a finite distance.
                break;
            };

            for &node_conn in &self.nodes[step.0].node_conns {
                let gc = &self.connections[node_conn.0];

                if (gc.n1 == prev_node && gc.n2 == step) || (gc.n1 == step && gc.n2 == prev_node) {
                    a_result.push(node_conn);
                    break;
                }
            }

            step = prev_node;
        }

        path_weight
    }

    /// Port of `CREEPAGE_GRAPH::Addshape()`: add the copper shape `a_shape` of the board item
    /// `a_parent` to the graph, each primitive as a node with a hidden zero-weight connection
    /// to `a_connect_to` (the net's virtual node), recursing into compound shapes.
    pub fn add_shape(
        &mut self,
        a_shape: &dyn Shape,
        a_connect_to: Option<NodeId>,
        a_parent: Option<CreepItem>,
    ) {
        let Some(connect_to) = a_connect_to else {
            return;
        };

        let mut newshape: Option<CreepShape> = None;
        let mut node_type: Option<GraphNodeType> = None;

        match a_shape.as_shape_ref() {
            ShapeRef::Segment(segment) => {
                newshape = Some(CreepShape::CuSegment(CuShapeSegment::new(
                    segment.get_seg().a,
                    segment.get_seg().b,
                    segment.get_width() as f64,
                )));
                node_type = Some(GraphNodeType::Segment);
            }

            ShapeRef::Circle(circle) => {
                newshape = Some(CreepShape::CuCircle(CuShapeCircle::new(
                    circle.get_center(),
                    circle.get_radius() as f64,
                )));
                node_type = Some(GraphNodeType::Circle);
            }

            ShapeRef::Arc(arc) => {
                // EDA_SHAPE edaArc( arc )
                let mut eda_arc = EdaShape::new(ShapeT::Arc, 0, FillT::NoFill);

                if arc.is_clockwise() {
                    eda_arc.set_arc_geometry(arc.get_p0(), arc.get_arc_mid(), arc.get_p1());
                } else {
                    eda_arc.set_arc_geometry(arc.get_p1(), arc.get_arc_mid(), arc.get_p0());
                }

                let (alpha, beta) = eda_arc.calc_arc_angles();

                let mut cu_arc = CuShapeArc::new(
                    eda_arc.center(),
                    eda_arc.radius() as f64,
                    alpha,
                    beta,
                    vd(arc.get_p0()),
                    vd(arc.get_p1()),
                );
                cu_arc.set_width(arc.get_width() as f64);
                newshape = Some(CreepShape::CuArc(cu_arc));
                node_type = Some(GraphNodeType::Arc);
            }

            ShapeRef::Compound(compound) => {
                let nb_shapes = compound.shapes().len() as i32;

                for subshape in compound.shapes() {
                    // We don't want to add shape for the inner rectangle of rounded rectangles
                    if !(matches!(subshape.as_shape_ref(), ShapeRef::Rect(_)) && nb_shapes == 5) {
                        self.add_shape(&**subshape, a_connect_to, a_parent);
                    }
                }
            }

            ShapeRef::PolySet(poly_set) => {
                for object in poly_set.c_iterate_segments_with_holes() {
                    let segment = ShapeSegment::new(object.a, object.b, 0);
                    self.add_shape(&segment, a_connect_to, a_parent);
                }
            }

            ShapeRef::LineChain(line_chain) => {
                let mut prev_point = line_chain.c_last_point();

                for &point in line_chain.c_points() {
                    let segment = ShapeSegment::new(point, prev_point, 0);
                    prev_point = point;
                    self.add_shape(&segment, a_connect_to, a_parent);
                }
            }

            ShapeRef::Simple(simple) => {
                // SHAPE_SIMPLE is the arbitrary-polygon form used for rectangular, trapezoidal
                // and chamfered pads when they are not axis-aligned (orthogonal rotations
                // collapse to SH_RECT instead). Decompose its closed outline into segments so
                // the copper edge is added to the graph, otherwise the pad contributes no
                // creepage anchor and the path snaps to the pad hole instead of the copper
                // (issue #24543).
                let vertices = simple.vertices();

                if vertices.point_count() >= 3 {
                    let mut prev_point = vertices.c_last_point();

                    for &point in vertices.c_points() {
                        if point != prev_point {
                            self.add_shape(
                                &ShapeSegment::new(prev_point, point, 0),
                                a_connect_to,
                                a_parent,
                            );
                        }

                        prev_point = point;
                    }
                }
            }

            ShapeRef::Rect(rect) => {
                let point0 = rect.get_position();
                let point1 = rect.get_position() + kicad_geom::Vector2I::new(rect.get_size().x, 0);
                let point2 = rect.get_position() + rect.get_size();
                let point3 = rect.get_position() + kicad_geom::Vector2I::new(0, rect.get_size().y);

                self.add_shape(&ShapeSegment::new(point0, point1, 0), a_connect_to, a_parent);
                self.add_shape(&ShapeSegment::new(point1, point2, 0), a_connect_to, a_parent);
                self.add_shape(&ShapeSegment::new(point2, point3, 0), a_connect_to, a_parent);
                self.add_shape(&ShapeSegment::new(point3, point0, 0), a_connect_to, a_parent);
            }

            _ => {}
        }

        let (Some(mut newshape), Some(node_type)) = (newshape, node_type) else {
            return;
        };

        newshape.set_parent(a_parent);
        let pos = newshape.pos();

        // m_shapeCollection.push_back( std::move( newshape ) ) happens after AddNode in C++;
        // AddNode never finds an existing node for a new shape, so the order is the same.
        let shape_id = self.push_shape(newshape);
        let gn_shape = self.add_node(node_type, Some(shape_id), pos);

        self.nodes[gn_shape.0].net = self.nodes[connect_to.0].net;
        let gc = self.add_connection_default(gn_shape, connect_to);
        self.connections[gc.0].path.show = false;
    }

    /// Port of `CREEPAGE_GRAPH::GeneratePaths()`: connect the directly connectable nodes of
    /// different shapes by every valid candidate path no longer than `a_max_weight`, on
    /// `a_layer` (`Edge_Cuts` for the board-edge graph). With `a_relevant_nets`, pairs of
    /// conductive nodes of two other nets are skipped.
    pub fn generate_paths(
        &mut self,
        a_max_weight: f64,
        a_layer: PcbLayerId,
        a_relevant_nets: Option<&BTreeSet<i32>>,
    ) {
        let ctx = self.ctx;

        let irrelevant_pair = |g: &CreepageGraph<'_>, gn1: NodeId, gn2: NodeId| -> bool {
            let n1 = &g.nodes[gn1.0];
            let n2 = &g.nodes[gn2.0];

            let Some(relevant) = a_relevant_nets else {
                return false;
            };

            match (n1.parent, n2.parent) {
                (Some(p1), Some(p2)) => {
                    g.shape(p1).is_conductive()
                        && g.shape(p2).is_conductive()
                        && !relevant.contains(&n1.net)
                        && !relevant.contains(&n2.net)
                }
                _ => false,
            }
        };

        let mut track_builder = TrackRtree::builder();

        if a_layer != PcbLayerId::EdgeCuts {
            for (track_id, track) in ctx.board.tracks() {
                let Track::Trace(trace) = track else {
                    continue;
                };

                let item = ctx.view(kicad_pcb::AnyItem::Track(track));

                if !item.is_on_layer(a_layer) {
                    continue;
                }

                let sh = item.effective_shape(
                    PcbLayerId::UndefinedLayer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );

                if matches!(sh.as_shape_ref(), ShapeRef::Segment(_)) {
                    let entry = CreepageTrackEntry {
                        segment: Seg::new(trace.start, trace.end),
                        layer: a_layer,
                        half_width: trace.width / 2,
                        track: CreepItem::Item(track_id.into()),
                    };

                    let bbox = item.bounding_box();
                    let min_coords = [bbox.x(), bbox.y()];
                    let max_coords = [bbox.right(), bbox.bottom()];
                    track_builder.add(&min_coords, &max_coords, entry);
                }
            }
        }

        let track_index: TrackRtree = track_builder.build();

        let mut nodes: Vec<NodeId> = (0..self.nodes.len())
            .map(NodeId)
            .filter(|&gn| {
                let n = &self.nodes[gn.0];
                n.parent.is_some() && n.connect_directly && n.type_ != GraphNodeType::Virtual
            })
            .collect();

        {
            let all = &self.nodes;
            kicad_geom::stdsort::std_sort(&mut nodes, |gn1, gn2| {
                let (n1, n2) = (&all[gn1.0], &all[gn2.0]);
                n1.parent < n2.parent || (n1.parent == n2.parent && n1.net < n2.net)
            });
        }

        // Build parent -> net -> nodes mapping for efficient filtering
        // Also cache bounding boxes for early spatial filtering
        let mut parent_net_groups: Vec<(Option<CreepItem>, NetGroups)> = Vec::new();
        let mut parent_index: BTreeMap<Option<CreepItem>, usize> = BTreeMap::new();
        let mut parent_bboxes: BTreeMap<CreepItem, Box2I> = BTreeMap::new();
        let mut parent_keys: Vec<Option<CreepItem>> = Vec::new();

        for &gn in &nodes {
            let shape = self.nodes[gn.0].parent.expect("filtered");
            let parent = self.shape(shape).parent();
            let net = self.nodes[gn.0].net;

            let idx = *parent_index.entry(parent).or_insert_with(|| {
                parent_net_groups.push((parent, Vec::new()));
                parent_net_groups.len() - 1
            });

            if parent_net_groups[idx].1.is_empty() {
                parent_keys.push(parent);

                if let Some(p) = parent {
                    let bbox = board_edge_bounding_box(ctx, &self.owned_board_edges, p)
                        .unwrap_or_default();
                    parent_bboxes.insert(p, bbox);
                }
            }

            let groups = &mut parent_net_groups[idx].1;

            match groups.iter_mut().find(|(n, _)| *n == net) {
                Some((_, list)) => list.push(gn),
                None => groups.push((net, vec![gn])),
            }
        }

        // Generate work items using parent-level spatial indexing
        let mut work_items: Vec<(NodeId, NodeId)> = Vec::new();

        // Use RTree for spatial indexing of parent bounding boxes
        // Expand each bbox by maxWeight to find potentially overlapping parents

        // static_cast<int64_t>( aMaxWeight )
        let max_dist: i64 = kicad_geom::cmath::f64_to_i64_x86(a_max_weight);

        let mut parent_entries: Vec<ParentEntry> = Vec::new();

        for &parent in &parent_keys {
            if let Some(parent) = parent {
                parent_entries.push(ParentEntry {
                    parent,
                    bbox: parent_bboxes[&parent],
                });
            }
        }

        let mut parent_builder: kicad_geom::geometry::rtree::PackedRTreeBuilder<usize, 2> =
            PackedRTree::builder();

        for (i, entry) in parent_entries.iter().enumerate() {
            let min_coords = [entry.bbox.left(), entry.bbox.top()];
            let max_coords = [entry.bbox.right(), entry.bbox.bottom()];
            parent_builder.add(&min_coords, &max_coords, i);
        }

        let parent_rtree = parent_builder.build();

        let group_of = |parent: Option<CreepItem>| -> Option<&NetGroups> {
            parent_index.get(&parent).map(|&i| &parent_net_groups[i].1)
        };

        // The parent pair search (sequential: see the module comment)
        for entry1 in &parent_entries {
            let parent1 = entry1.parent;
            let bbox1 = entry1.bbox;

            let mut local_work_items: Vec<(NodeId, NodeId)> = Vec::new();

            // Search for parents within maxDist of bbox1 ((int) maxDist: a 64 -> 32 bit
            // truncation, as x86-64 executes it)
            let md = max_dist as i32;
            let search_min = [bbox1.left().wrapping_sub(md), bbox1.top().wrapping_sub(md)];
            let search_max = [
                bbox1.right().wrapping_add(md),
                bbox1.bottom().wrapping_add(md),
            ];

            parent_rtree.search(&search_min, &search_max, |&i2| {
                let entry2 = &parent_entries[i2];
                let parent2 = entry2.parent;

                // Only process if parent1 < parent2 to avoid duplicates
                if parent1 >= parent2 {
                    return true;
                }

                // Precise bbox distance check
                let bbox2 = entry2.bbox;

                let mut bbox_dist_x: i64 = 0;

                if bbox2.left() > bbox1.right() {
                    bbox_dist_x = bbox2.left().wrapping_sub(bbox1.right()) as i64;
                } else if bbox1.left() > bbox2.right() {
                    bbox_dist_x = bbox1.left().wrapping_sub(bbox2.right()) as i64;
                }

                let mut bbox_dist_y: i64 = 0;

                if bbox2.top() > bbox1.bottom() {
                    bbox_dist_y = bbox2.top().wrapping_sub(bbox1.bottom()) as i64;
                } else if bbox1.top() > bbox2.bottom() {
                    bbox_dist_y = bbox1.top().wrapping_sub(bbox2.bottom()) as i64;
                }

                let bbox_dist_sq = bbox_dist_x * bbox_dist_x + bbox_dist_y * bbox_dist_y;

                if bbox_dist_sq > max_dist.wrapping_mul(max_dist) {
                    return true;
                }

                // Get nodes for both parents
                let (Some(groups1), Some(groups2)) = (group_of(Some(parent1)), group_of(Some(parent2)))
                else {
                    return true;
                };

                for (net1, nodes1) in groups1 {
                    for (net2, nodes2) in groups2 {
                        // Skip same net if both are conductive
                        if net1 == net2 && !nodes1.is_empty() && !nodes2.is_empty() {
                            let s1 = self.nodes[nodes1[0].0].parent.expect("filtered");
                            let s2 = self.nodes[nodes2[0].0].parent.expect("filtered");

                            if self.shape(s1).is_conductive() && self.shape(s2).is_conductive() {
                                continue;
                            }
                        }

                        for &gn1 in nodes1 {
                            for &gn2 in nodes2 {
                                if !self.pair_in_range(gn1, gn2, a_max_weight) {
                                    continue;
                                }

                                if irrelevant_pair(self, gn1, gn2) {
                                    continue;
                                }

                                local_work_items.push((gn1, gn2));
                            }
                        }
                    }
                }

                true
            });

            // Merge local results into global
            work_items.extend(local_work_items);
        }

        // Generate work items for same-parent node pairs. The cross-parent search above
        // skips pairs where parent1 == parent2, but creepage paths between different edge
        // segments of the same slot (which share a footprint grandparent) are needed for
        // the path to navigate around the slot geometry. Also handles null-parent nodes
        // (e.g. NPTH pad shapes) which were excluded from the RTree search entirely.
        // (The C++ walks the pointer-keyed unordered_map: here the first insertion order.)
        for (_, net_groups) in &parent_net_groups {
            let mut same_parent_nodes: Vec<NodeId> = Vec::new();

            for (_, node_list) in net_groups {
                same_parent_nodes.extend(node_list.iter().copied());
            }

            for i in 0..same_parent_nodes.len() {
                for j in i + 1..same_parent_nodes.len() {
                    let gn1 = same_parent_nodes[i];
                    let gn2 = same_parent_nodes[j];
                    let (n1, n2) = (&self.nodes[gn1.0], &self.nodes[gn2.0]);
                    let (s1, s2) = (n1.parent.expect("filtered"), n2.parent.expect("filtered"));

                    // ConnectChildren already handles nodes on the same CREEP_SHAPE
                    if s1 == s2 {
                        continue;
                    }

                    // Skip same-net conductive pairs
                    if self.shape(s1).is_conductive()
                        && self.shape(s2).is_conductive()
                        && n1.net == n2.net
                    {
                        continue;
                    }

                    if !self.pair_in_range(gn1, gn2, a_max_weight) {
                        continue;
                    }

                    if irrelevant_pair(self, gn1, gn2) {
                        continue;
                    }

                    work_items.push((gn1, gn2));
                }
            }
        }

        // processWorkItems, inline (see the module comment)
        for (gn1, gn2) in work_items {
            // Distance filtering already done during work item creation
            let shape1 = self.nodes[gn1.0].parent.expect("filtered");
            let shape2 = self.nodes[gn2.0].parent.expect("filtered");

            let paths = get_paths(self.shape(shape1), self.shape(shape2), a_max_weight);

            for pc in &paths {
                let mut ignore_for_test: Vec<CreepItem> = Vec::new();

                // Don't ignore the whole parent board item for arc/circle ends. The
                // tangent touch is already handled by the endpoint exclusion in
                // segmentIntersectsArc/Circle (issue #24286). A rounded slot is a single
                // PCB_SHAPE, so ignoring the parent would exempt every other edge of the
                // same slot and let a path cut across it.

                // Ignore each CU shape's own parent for the endpoint-inside-track
                // test so we don't reject paths that touch the track's own edge.
                // (A null parent pushed by the C++ never matches an edge or a track.)
                if self.shape(shape1).is_conductive()
                    && let Some(p) = self.shape(shape1).parent()
                {
                    ignore_for_test.push(p);
                }

                if self.shape(shape2).is_conductive()
                    && let Some(p) = self.shape(shape2).parent()
                {
                    ignore_for_test.push(p);
                }

                // A path with a NaN end (overlapping circles: the tangent construction takes
                // the square root of a negative number) has NaN weight, so Dijkstra can never
                // relax it, and its INT_MIN end nodes are dead ends. The C++ validates it
                // with INT_MIN coordinates (signed overflow in the outline tests, which panics
                // in a checked build): the port rejects it up front. Same shortest paths;
                // only unusable nodes and connections are missing (PORT_STATUS.md).
                let finite = pc.a1.x.is_finite()
                    && pc.a1.y.is_finite()
                    && pc.a2.x.is_finite()
                    && pc.a2.y.is_finite()
                    && !pc.weight.is_nan();

                if !finite {
                    continue;
                }

                let valid = pc.is_valid(
                    ctx,
                    a_layer,
                    &self.board_edge,
                    &self.owned_board_edges,
                    &ignore_for_test,
                    self.board_outline.as_ref(),
                    (false, true),
                    self.min_groove_width,
                    Some(&track_index),
                );

                if !valid {
                    continue;
                }

                let mut connect1 = gn1;
                let mut connect2 = gn2;

                // Handle non-point node1
                if self.shape(shape1).type_() != CreepShapeType::PointType {
                    let gnt1 = self.add_node(GraphNodeType::Point, Some(shape1), vi(pc.a1));
                    self.nodes[gnt1.0].connect_directly = false;
                    connect1 = gnt1;

                    if self.shape(shape1).is_conductive() {
                        let gc = self.add_connection_default(gn1, gnt1);
                        self.connections[gc.0].path.show = false;
                    }
                }

                // Handle non-point node2
                if self.shape(shape2).type_() != CreepShapeType::PointType {
                    let gnt2 = self.add_node(GraphNodeType::Point, Some(shape2), vi(pc.a2));
                    self.nodes[gnt2.0].connect_directly = false;
                    connect2 = gnt2;

                    if self.shape(shape2).is_conductive() {
                        let gc = self.add_connection_default(gn2, gnt2);
                        self.connections[gc.0].path.show = false;
                    }
                }

                self.add_connection(connect1, connect2, pc);
            }
        }
    }

    /// The centre distance filter of the work item generation of `GeneratePaths`: whether
    /// the shapes of `gn1` and `gn2` can be within `a_max_weight` of each other.
    fn pair_in_range(&self, gn1: NodeId, gn2: NodeId, a_max_weight: f64) -> bool {
        let s1 = self.shape(self.nodes[gn1.0].parent.expect("filtered"));
        let s2 = self.shape(self.nodes[gn2.0].parent.expect("filtered"));
        let pos1 = s1.pos();
        let pos2 = s2.pos();
        let r1 = s1.radius();
        let r2 = s2.radius();

        let center_dist_sq: i64 = (pos1 - pos2).squared_euclidean_norm();
        let threshold: f64 = a_max_weight + r1 as f64 + r2 as f64;
        let threshold_sq = threshold * threshold;

        // if( (double) centerDistSq > thresholdSq ) continue;
        !((center_dist_sq as f64) > threshold_sq)
    }

    /// Port of `CREEPAGE_GRAPH::AddNetElements()`: add every copper item of net `a_net_code`
    /// on `a_layer` (pads, tracks, zones and connected board drawings) to the graph, all
    /// connected to a new virtual node of the net, which is returned.
    pub fn add_net_elements(
        &mut self,
        a_net_code: i32,
        a_layer: PcbLayerId,
        a_max_creepage: i32,
    ) -> NodeId {
        let _ = a_max_creepage; // unused in the C++ as well
        let ctx = self.ctx;
        let board = ctx.board;

        let virtual_node = self.add_node_virtual();
        self.nodes[virtual_node.0].net = a_net_code;

        for (fp_id, _) in board.footprints() {
            for (pad_id, pad) in board.footprint_pads(fp_id) {
                let item = ctx.view(kicad_pcb::AnyItem::Pad(pad));

                if item.net_code().map_or(-1, |n| n.0) != a_net_code || !item.is_on_layer(a_layer)
                {
                    continue;
                }

                if pad.has_copper_removed(ctx, a_layer) {
                    continue;
                }

                let pad_shape =
                    item.effective_shape(a_layer, Flashing::Default, DrcConstraintT::NullConstraint);
                self.add_shape(
                    &*pad_shape,
                    Some(virtual_node),
                    Some(CreepItem::Item(pad_id.into())),
                );
            }
        }

        for (track_id, track) in board.tracks() {
            let item = ctx.view(kicad_pcb::AnyItem::Track(track));

            if item.net_code().map_or(-1, |n| n.0) != a_net_code || !item.is_on_layer(a_layer) {
                continue;
            }

            if let Track::Via(via) = track
                && via.has_copper_removed(ctx, a_layer)
            {
                continue;
            }

            let shape = item.effective_shape(
                PcbLayerId::UndefinedLayer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            self.add_shape(
                &*shape,
                Some(virtual_node),
                Some(CreepItem::Item(track_id.into())),
            );
        }

        for (zone_id, zone) in board.zones() {
            if zone.is_rule_area() {
                continue;
            }

            let item = ctx.view(kicad_pcb::AnyItem::Zone(zone));

            if item.net_code().map_or(-1, |n| n.0) != a_net_code || !item.is_on_layer(a_layer) {
                continue;
            }

            let shape =
                item.effective_shape(a_layer, Flashing::Default, DrcConstraintT::NullConstraint);
            self.add_shape(
                &*shape,
                Some(virtual_node),
                Some(CreepItem::Item(zone_id.into())),
            );
        }

        for (drawing_id, drawing) in board.drawings() {
            let item = ctx.view(drawing);

            if item.is_connected() {
                if item.net_code().map_or(-1, |n| n.0) != a_net_code
                    || !item.is_on_layer(a_layer)
                {
                    continue;
                }

                let shape = item.effective_shape(
                    PcbLayerId::UndefinedLayer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );
                self.add_shape(&*shape, Some(virtual_node), Some(CreepItem::Item(drawing_id)));
            }
        }

        virtual_node
    }
}
