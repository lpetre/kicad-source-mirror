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

//! Port of the file-local fracture helpers of `libs/kimath/src/geometry/shape_poly_set.cpp`:
//! `FractureEdge`, `FRACTURE_EDGE_INDEX`, `fractureIntersectX`, `processHole`,
//! `fractureSingleCacheFriendly` and `splitAtBridges` (the static helper shared by the
//! boolean bridge removal and `unfractureSingle`).

use std::collections::HashMap;

use crate::geometry::fracture_edge_index_utils as fi;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::Polygon;
use crate::math::util::rescale;
use crate::math::vector2::Vector2I;
use crate::stdsort::std_sort;

/// `FractureEdge::Index`
type Index = i32;

/// `sizeof( FractureEdge )` on the reference platform (two `VECTOR2I` and an `int`).
const FRACTURE_EDGE_SIZE: u64 = 20;
/// `sizeof( FRACTURE_EDGE_INDEX::NODE )`
const NODE_SIZE: u64 = 8;
/// `FRACTURE_EDGE_INDEX::INVALID`
const INVALID: u32 = u32::MAX;

/// Port of `FractureEdge`.  The default value (used by `std::vector::resize`, which
/// value-initialises) is all zeros.
#[derive(Clone, Copy, Debug, Default)]
struct FractureEdge {
    p1: Vector2I,
    p2: Vector2I,
    next: Index,
}

impl FractureEdge {
    fn new(p1: Vector2I, p2: Vector2I, next: Index) -> Self {
        FractureEdge { p1, p2, next }
    }

    /// Port of `FractureEdge::matches()`.
    fn matches(&self, y: i32) -> bool {
        (y >= self.p1.y || y >= self.p2.y) && (y <= self.p1.y || y <= self.p2.y)
    }
}

/// Port of `FRACTURE_EDGE_INDEX::NODE`.
#[derive(Clone, Copy, Debug)]
struct Node {
    edge: u32,
    next: u32,
}

/// Port of `FRACTURE_EDGE_INDEX`: a Y-stripe index of the fracture edges.  It only speeds up
/// `processHole()`: the candidate set is a superset of the edges a linear scan would accept
/// and ties are broken by the lowest edge index, so the result equals the linear scan.
///
/// The C++ keeps a reference to the edge set; the port passes it to the methods instead.
struct FractureEdgeIndex {
    min_y: i32,
    max_y: i32,
    stripe_count: u32,
    max_nodes: usize,
    valid: bool,
    offsets: Vec<u32>,
    bucket_ids: Vec<u32>,
    long_ids: Vec<u32>,
    heads: Vec<u32>,
    nodes: Vec<Node>,
}

impl FractureEdgeIndex {
    /// Port of the `FRACTURE_EDGE_INDEX` constructor; `a_originals` lists the original edges
    /// with their stripe span, in the order the C++ `aOriginals` visitor produces them.
    fn new(
        a_edge_count: usize,
        a_originals: &[(u32, u32, u32)],
        a_min_y: i32,
        a_max_y: i32,
        a_stripe_count: u32,
        a_hole_count: usize,
    ) -> Self {
        let mut index = FractureEdgeIndex {
            min_y: a_min_y,
            max_y: a_max_y,
            stripe_count: a_stripe_count,
            max_nodes: (fi::MAX_BUCKET_SPAN as usize + 2) * a_hole_count,
            valid: false,
            offsets: Vec::new(),
            bucket_ids: Vec::new(),
            long_ids: Vec::new(),
            heads: Vec::new(),
            nodes: Vec::new(),
        };
        let budget = fi::capacity_budget(a_edge_count as u64, FRACTURE_EDGE_SIZE);

        let mut counts: Vec<u32> = vec![0; a_stripe_count as usize];
        let mut bucket_ids: usize = 0;
        let mut long_ids: usize = 0;

        for &(_, first, last) in a_originals {
            if last - first + 1 > fi::MAX_BUCKET_SPAN {
                long_ids += 1;
            } else {
                bucket_ids += (last - first + 1) as usize;

                for stripe in first..=last {
                    counts[stripe as usize] += 1;
                }
            }
        }

        if !fi::capacity_fits(
            bucket_ids as u64,
            long_ids as u64,
            a_stripe_count as u64,
            a_hole_count as u64,
            NODE_SIZE,
            budget,
        ) {
            return index;
        }

        index.offsets = vec![0; a_stripe_count as usize + 1];

        for stripe in 0..a_stripe_count as usize {
            index.offsets[stripe + 1] = index.offsets[stripe] + counts[stripe];
        }

        index.bucket_ids = vec![0; bucket_ids];
        index.long_ids.reserve(long_ids);

        counts.copy_from_slice(&index.offsets[..a_stripe_count as usize]);

        for &(edge, first, last) in a_originals {
            if last - first + 1 > fi::MAX_BUCKET_SPAN {
                index.long_ids.push(edge);
            } else {
                for stripe in first..=last {
                    index.bucket_ids[counts[stripe as usize] as usize] = edge;
                    counts[stripe as usize] += 1;
                }
            }
        }

        index.heads = vec![INVALID; a_stripe_count as usize + 1];
        index.nodes.reserve(index.max_nodes);

        // The estimate prevents an over-budget allocation; this check catches allocator
        // over-capacity.  (libstdc++ allocates exactly the requested sizes here, so the
        // capacities are the sizes.)
        let (fits, _allocated_bytes) = fi::actual_capacity_fits(
            index.bucket_ids.len() as u64,
            long_ids as u64,
            index.offsets.len() as u64,
            counts.len() as u64,
            index.heads.len() as u64,
            index.max_nodes as u64,
            NODE_SIZE,
            budget,
        );

        if !fits {
            return index;
        }

        index.valid = true;
        index
    }

    /// Port of `FRACTURE_EDGE_INDEX::Query()`.
    fn query(&self, a_y: i32, a_provoking_index: Index, a_visitor: &mut dyn FnMut(Index)) {
        let stripe = self.map(a_y) as usize;
        let provoking = a_provoking_index as u32;

        // Buckets and the long list are ascending, so the first out-of-range id ends the scan
        for pos in self.offsets[stripe]..self.offsets[stripe + 1] {
            let edge = self.bucket_ids[pos as usize];

            if edge >= provoking {
                break;
            }

            a_visitor(edge as Index);
        }

        for &edge in &self.long_ids {
            if edge >= provoking {
                break;
            }

            a_visitor(edge as Index);
        }

        self.visit_overflow(self.heads[stripe], provoking, a_visitor);
        self.visit_overflow(self.heads[self.stripe_count as usize], provoking, a_visitor);
    }

    /// Port of `FRACTURE_EDGE_INDEX::InsertBridges()`.
    fn insert_bridges(&mut self, a_edges: &[FractureEdge], a_first: Index) {
        for edge in a_first..a_first + 3 {
            let e = &a_edges[edge as usize];
            let (first, last) =
                fi::stripe_span(e.p1.y, e.p2.y, self.min_y, self.max_y, self.stripe_count);

            if last - first + 1 > fi::MAX_BUCKET_SPAN {
                self.insert(edge as u32, self.stripe_count);
            } else {
                for stripe in first..=last {
                    self.insert(edge as u32, stripe);
                }
            }
        }
    }

    fn map(&self, a_y: i32) -> u32 {
        fi::map_y_to_stripe(a_y, self.min_y, self.max_y, self.stripe_count)
    }

    /// Port of `FRACTURE_EDGE_INDEX::visitOverflow()`.
    fn visit_overflow(&self, a_node: u32, a_provoking: u32, a_visitor: &mut dyn FnMut(Index)) {
        // Bridges are only ever inserted for already processed holes, so the assert should
        // hold, but the filter keeps the release build honest if that ordering ever changes
        let mut node_idx = a_node;

        while node_idx != INVALID {
            let node = self.nodes[node_idx as usize];

            if node.edge < a_provoking {
                a_visitor(node.edge as Index);
            }

            node_idx = node.next;
        }
    }

    /// Port of `FRACTURE_EDGE_INDEX::insert()`.
    fn insert(&mut self, a_edge: u32, a_head: u32) {
        debug_assert!(self.nodes.len() < self.max_nodes);
        self.nodes.push(Node {
            edge: a_edge,
            next: self.heads[a_head as usize],
        });
        self.heads[a_head as usize] = (self.nodes.len() - 1) as u32;
    }
}

/// Port of `fractureIntersectX()`: where a horizontal ray at `a_y` crosses `a_edge`, valid only
/// when `a_edge.matches( a_y )`.
fn fracture_intersect_x(a_edge: &FractureEdge, a_y: i32) -> i32 {
    if a_edge.p1.y == a_edge.p2.y {
        return a_edge.p1.x.max(a_edge.p2.x);
    }

    a_edge.p1.x
        + rescale(
            a_edge.p2.x - a_edge.p1.x,
            a_y - a_edge.p1.y,
            a_edge.p2.y - a_edge.p1.y,
        )
}

/// Port of `processHole()`; returns whether a nearest edge was found (C++: non-null).
fn process_hole(
    edges: &mut [FractureEdge],
    provoking_index: Index,
    edge_index: Index,
    bridge_index: Index,
    a_index: Option<&mut FractureEdgeIndex>,
) -> bool {
    let edge = edges[edge_index as usize];
    let x = edge.p1.x;
    let y = edge.p1.y;
    let mut min_dist = i32::MAX;
    let mut x_nearest = 0;

    let mut e_nearest: Option<usize> = None;

    // Since this function is run for all holes left to right, no need to
    // check for any edge beyond the provoking one because they will always be
    // further to the right, and unconnected to the outline anyway.
    match &a_index {
        Some(index) => {
            let mut nearest_index: Index = -1;

            let mut consider = |a_candidate_index: Index| {
                let candidate = &edges[a_candidate_index as usize];

                if !candidate.matches(y) {
                    return;
                }

                let x_intersect = fracture_intersect_x(candidate, y);
                let dist = x - x_intersect;

                if dist >= 0
                    && (dist < min_dist
                        || (nearest_index >= 0
                            && dist == min_dist
                            && a_candidate_index < nearest_index))
                {
                    min_dist = dist;
                    x_nearest = x_intersect;
                    nearest_index = a_candidate_index;
                }
            };

            index.query(y, provoking_index, &mut consider);

            if nearest_index >= 0 {
                e_nearest = Some(nearest_index as usize);
            }
        }
        None => {
            for i in 0..provoking_index as usize {
                let candidate = &edges[i];

                if !candidate.matches(y) {
                    continue;
                }

                let x_intersect = fracture_intersect_x(candidate, y);
                let dist = x - x_intersect;

                if dist >= 0 && dist < min_dist {
                    min_dist = dist;
                    x_nearest = x_intersect;
                    e_nearest = Some(i);
                }
            }
        }
    }

    let Some(nearest) = e_nearest else {
        return false;
    };

    let outline2hole_index = bridge_index;
    let hole2outline_index = bridge_index + 1;
    let split_index = bridge_index + 2;
    let split_pt = Vector2I::new(x_nearest, y);

    // Make an edge between the split outline edge and the hole...
    edges[outline2hole_index as usize] = FractureEdge::new(split_pt, edge.p1, edge_index);
    // ...between the hole and the edge...
    edges[hole2outline_index as usize] = FractureEdge::new(edge.p1, split_pt, split_index);
    // ...and between the split outline edge and the rest.
    edges[split_index as usize] =
        FractureEdge::new(split_pt, edges[nearest].p2, edges[nearest].next);

    // Perform the actual outline edge split
    // Existing stripe membership remains conservative because the selected y is inside the
    // edge's current interval, while newly initialized bridge edges are inserted below.
    edges[nearest].p2 = split_pt;
    edges[nearest].next = outline2hole_index;

    let mut last = edge_index as usize;

    while edges[last].next != edge_index {
        last = edges[last].next as usize;
    }

    edges[last].next = hole2outline_index;

    if let Some(index) = a_index {
        index.insert_bridges(edges, bridge_index);
    }

    true
}

/// `PathInfo` of `fractureSingleCacheFriendly()`.
#[derive(Clone, Copy, Debug)]
struct PathInfo {
    path_or_provoking_index: i32,
    leftmost: Index,
    x: i32,
    y_or_bridge: i32,
}

/// Port of `fractureSingleCacheFriendly()` (the default `fractureSingle()`,
/// `ADVANCED_CFG::m_EnableCacheFriendlyFracture` and `m_EnableFractureEdgeIndex` are true).
pub(crate) fn fracture_single_cache_friendly(paths: &mut Polygon) {
    let mut edges: Vec<FractureEdge> = Vec::new();
    let mut outline = true;

    if paths.len() == 1 {
        return;
    }

    let mut total_point_count: usize = 0;

    for path in paths.iter() {
        total_point_count += path.point_count() as usize;

        // A contour without points makes the C++ read before its point array (undefined
        // behaviour); the port leaves such a polygon unchanged.
        if path.point_count() == 0 {
            return;
        }
    }

    if total_point_count > i32::MAX as usize {
        // wxLogWarning( "Polygon has more points than int limit" )
        return;
    }

    // Reserve space in the edge set so pointers don't get invalidated during
    // the whole fracture process; one for each original edge, plus 3 per
    // path to join it to the outline.
    edges.reserve(total_point_count + paths.len() * 3);

    // Sort the paths by their lowest X bound before processing them.
    // This ensures the processing order for processEdge() is correct.
    let paths_count = paths.len() as i32;
    let mut sorted_paths: Vec<PathInfo> = Vec::with_capacity(paths_count as usize);

    for path_index in 0..paths_count {
        let points = paths[path_index as usize].c_points();
        let mut x_min = i32::MAX;
        let mut y_min = i32::MAX;
        let mut leftmost = -1;

        for (point_index, point) in points.iter().enumerate() {
            if point.x < x_min {
                x_min = point.x;
                leftmost = point_index as i32;
            }

            if point.y < y_min {
                y_min = point.y;
            }
        }

        sorted_paths.push(PathInfo {
            path_or_provoking_index: path_index,
            leftmost,
            x: x_min,
            y_or_bridge: y_min,
        });
    }

    std_sort(&mut sorted_paths[1..], |a, b| {
        if a.x == b.x {
            return a.y_or_bridge < b.y_or_bridge;
        }

        a.x < b.x
    });

    let mut edge_index: Index = 0;

    for path_info in sorted_paths.iter_mut() {
        let points = paths[path_info.path_or_provoking_index as usize].c_points();
        let point_count = points.len();

        // Index of the provoking (first) edge for this path
        let provoking_edge = edge_index;

        for i in 0..point_count - 1 {
            edges.push(FractureEdge::new(points[i], points[i + 1], edge_index + 1));
            edge_index += 1;
        }

        // Create last edge looping back to the provoking one.
        edges.push(FractureEdge::new(
            points[point_count - 1],
            points[0],
            provoking_edge,
        ));
        edge_index += 1;

        if !outline {
            // Repurpose the path sorting data structure to schedule the leftmost edge
            // for merging to the outline, which will in turn merge the rest of the path.
            path_info.path_or_provoking_index = provoking_edge;
            path_info.y_or_bridge = edge_index;

            // Reserve 3 additional edges to bridge with the outline.
            edge_index += 3;
            edges.resize(edge_index as usize, FractureEdge::default());
        }

        outline = false; // first path is always the outline
    }

    debug_assert!(edges.len() == total_point_count + 3 * (paths.len() - 1));

    let mut estimated_visits: u64 = 0;

    for it in &sorted_paths[1..] {
        estimated_visits += it.path_or_provoking_index as u64;
    }

    let mut index: Option<FractureEdgeIndex> = None;

    if fi::should_index(estimated_visits, paths.len() - 1) {
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        let stripe_count = fi::stripe_count_for(edges.len());

        for path in paths.iter() {
            for point in path.c_points() {
                min_y = min_y.min(point.y);
                max_y = max_y.max(point.y);
            }
        }

        let mut originals: Vec<(u32, u32, u32)> = Vec::new();
        let mut visit_range = |a_begin: Index, a_end: Index| {
            for edge in a_begin..a_end {
                let original = &edges[edge as usize];
                let (first, last) =
                    fi::stripe_span(original.p1.y, original.p2.y, min_y, max_y, stripe_count);
                originals.push((edge as u32, first, last));
            }
        };

        visit_range(0, paths[0].point_count());

        for it in &sorted_paths[1..] {
            visit_range(it.path_or_provoking_index, it.y_or_bridge);
        }

        let idx = FractureEdgeIndex::new(
            edges.len(),
            &originals,
            min_y,
            max_y,
            stripe_count,
            paths.len() - 1,
        );

        if idx.valid {
            index = Some(idx);
        }
    }

    for it in &sorted_paths[1..] {
        let found = process_hole(
            &mut edges,
            it.path_or_provoking_index,
            it.path_or_provoking_index + it.leftmost,
            it.y_or_bridge,
            index.as_mut(),
        );

        // If we can't handle the hole, the zone is broken (maybe)
        if !found {
            // wxLogWarning( "Broken polygon, dropping path" )
            return;
        }
    }

    paths.truncate(1);
    let new_path = &mut paths[0];

    new_path.clear();
    new_path.set_closed(true);

    // Root edge is always at index 0
    let mut e = 0usize;

    while edges[e].next != 0 {
        new_path.append_point(edges[e].p1, false);
        e = edges[e].next as usize;
    }

    new_path.append_point(edges[e].p1, false);
}

/// Port of the static `splitAtBridges()`: split a closed ring into the rings left once pairs
/// of coincident opposite edges are removed.  `Fracture()` joins holes to their outline
/// through such pairs.
///
/// Returns false, leaving `a_rings` empty, when the ring holds no pair.  (The C++
/// `std::unordered_map` is only used for lookups, so its iteration order does not matter.)
pub(crate) fn split_at_bridges(
    a_chain: &ShapeLineChain,
    a_rings: &mut Vec<ShapeLineChain>,
) -> bool {
    let pts = a_chain.c_points();
    let count = pts.len() as i32;

    // A bucket per directed edge so repeated bridges along one line all find a partner
    let mut unpaired: HashMap<(Vector2I, Vector2I), Vec<i32>> = HashMap::with_capacity(pts.len());
    let mut partner: Vec<i32> = vec![-1; pts.len()];
    let mut has_bridge = false;

    for ii in 0..count {
        let a = pts[ii as usize];
        let b = pts[((ii + 1) % count) as usize];

        if a == b {
            continue;
        }

        if let Some(twin) = unpaired.get_mut(&(b, a)) {
            let jj = twin.pop().expect("buckets are never left empty");

            if twin.is_empty() {
                unpaired.remove(&(b, a));
            }

            partner[ii as usize] = jj;
            partner[jj as usize] = ii;
            has_bridge = true;
        } else {
            unpaired.entry((a, b)).or_default().push(ii);
        }
    }

    if !has_bridge {
        return false;
    }

    // The edge that continues from the end of aEdge once bridge pairs are skipped
    let next_edge = |a_edge: i32| -> i32 {
        let mut next = (a_edge + 1) % count;
        let mut guard = 0;

        while partner[next as usize] >= 0 {
            if guard > count {
                return -1;
            }

            next = (partner[next as usize] + 1) % count;
            guard += 1;
        }

        next
    };

    let mut visited: Vec<bool> = vec![false; pts.len()];

    for start in 0..count {
        if visited[start as usize] || partner[start as usize] >= 0 {
            continue;
        }

        let mut ring = ShapeLineChain::new();
        let mut edge = start;

        loop {
            if edge < 0 || visited[edge as usize] {
                a_rings.clear();
                return false;
            }

            visited[edge as usize] = true;
            ring.append_point(pts[edge as usize], true);
            edge = next_edge(edge);

            if edge == start {
                break;
            }
        }

        if ring.point_count() >= 3 {
            ring.set_closed(true);
            a_rings.push(ring);
        }
    }

    true
}
