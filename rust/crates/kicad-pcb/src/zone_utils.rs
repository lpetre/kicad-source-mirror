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

//! Port of `pcbnew/zone_utils.h` and `pcbnew/zone_utils.cpp`.
//!
//! Pointer-keyed containers of the C++ (`std::unordered_map<ZONE*, ..>`) are only looked up,
//! never iterated, so they become maps keyed by the zone's position in the eligible-zone list.
//! The constraint computations, run on the thread pool in C++, run sequentially in pair order
//! (their results are collected in submission order in C++ too).

use std::collections::{BTreeMap, HashMap};

use kicad_core::{Lset, PcbLayerId};
use kicad_geom::stdsort::std_sort;
use kicad_geom::{Shape, ShapePolySet};

use crate::board::Board;
use crate::ctx::{BoardCtx, MutEnv};
use crate::ids::ZoneId;
use crate::pcb_track::Track;
use crate::traits::BoardConnectedItem;
use crate::zone::Zone;

/// Port of `RuleAreasHaveSameProps()`.
fn rule_areas_have_same_props(a: &Zone, b: &Zone) -> bool {
    // This function is only used to compare rule areas, so we can assume that both a and b are rule areas
    debug_assert!(a.is_rule_area() && b.is_rule_area());

    a.do_not_allow_zone_fills == b.do_not_allow_zone_fills
        && a.do_not_allow_footprints == b.do_not_allow_footprints
        && a.do_not_allow_tracks == b.do_not_allow_tracks
        && a.do_not_allow_vias == b.do_not_allow_vias
        && a.do_not_allow_pads == b.do_not_allow_pads
}

/// The `polygonsAreMergeable` lambda of `MergeZonesWithSameOutline`.
fn polygons_are_mergeable(
    a: &[kicad_geom::ShapeLineChain],
    b: &[kicad_geom::ShapeLineChain],
) -> bool {
    if a.len() != b.len() {
        return false;
    }

    // NOTE: this assumes the polygons have their line chains in the same order
    // But that is not actually required for same geometry (i.e. mergeability)
    for (chain_a, chain_b) in a.iter().zip(b.iter()) {
        // Note: this assumes the polygons are either already simplified or that it's
        // OK to not merge even if they would be the same after simplification.
        if chain_a.point_count() != chain_b.point_count()
            || chain_a.bbox(0) != chain_b.bbox(0)
            || !chain_a.compare_geometry(chain_b, false, 0)
        {
            // Different geometry, can't merge
            return false;
        }
    }

    true
}

/// The `zonesAreMergeable` lambda of `MergeZonesWithSameOutline`.
fn zones_are_mergeable(a: &Zone, b: &Zone) -> bool {
    // Can't merge rule areas with zone fills
    if a.is_rule_area() != b.is_rule_area() {
        return false;
    }

    if a.is_rule_area() {
        if !rule_areas_have_same_props(a, b) {
            return false;
        }
    } else {
        // We could also check clearances and so on
        if a.base.net != b.base.net {
            return false;
        }
    }

    let poly_set_a = a.outline();
    let poly_set_b = b.outline();

    if poly_set_a.outline_count() != poly_set_b.outline_count() {
        return false;
    }

    if poly_set_a.outline_count() == 0 {
        // both have no outline, so they are the same, but we must not
        // derefence them, as they are empty
        return true;
    }

    // REVIEW: this assumes the zones only have a single polygon in the
    polygons_are_mergeable(poly_set_a.c_polygon(0), poly_set_b.c_polygon(0))
}

/// Port of `MergeZonesWithSameOutline( aZones )`: merge zones that share an outline (same
/// net, or rule areas with the same keepout flags) into one multi-layer zone; the first zone
/// of each group is kept with the union of the layers and the fills of the merged zones
/// (later zones' fills win per layer).
pub fn merge_zones_with_same_outline(zones: Vec<Zone>) -> Vec<Zone> {
    let mut zones = zones;
    let mut deduplicated_zones: Vec<Zone> = Vec::new();

    // Map of zone indexes that we have already merged into a prior zone
    let mut merged = vec![false; zones.len()];

    for i in 0..zones.len() {
        // This one has already been subsumed into a prior zone, so skip it
        // and it will be dropped at the end.
        if merged[i] {
            continue;
        }

        let mut layers = zones[i].layer_set.clone();
        // std::unordered_map in C++; only the final per-layer value matters (layers are
        // distinct keys, installed one by one), so a sorted map gives the same zone.
        let mut merged_fills: BTreeMap<PcbLayerId, ShapePolySet> = BTreeMap::new();

        for j in (i + 1)..zones.len() {
            // This zone has already been subsumed by a prior zone, so it
            // cannot be merged into another primary
            if merged[j] {
                continue;
            }

            let can_merge = zones_are_mergeable(&zones[i], &zones[j]);

            if can_merge {
                let candidate = &zones[j];

                for layer in candidate.layer_set.seq().iter().copied() {
                    if let Some(fill) = candidate.get_fill(layer) {
                        merged_fills.insert(layer, fill.clone());
                    }
                }

                layers |= &candidate.layer_set;
                merged[j] = true;
            }
        }

        let primary = &mut zones[i];

        if layers != primary.layer_set {
            for layer in primary.layer_set.seq().iter().copied() {
                if let Some(fill) = primary.get_fill(layer) {
                    merged_fills.insert(layer, fill.clone());
                }
            }

            primary.set_layer_set(&MutEnv::detached(), layers);

            for (layer, fill) in &merged_fills {
                primary.fill.set_filled_polys_list(*layer, fill);
            }

            primary.fill.set_need_refill(false);
            primary.fill.set_is_filled(true);
        }
    }

    // Keep the primaries - they may or may not have had other zones merged into them
    for (zone, was_merged) in zones.into_iter().zip(merged) {
        if !was_merged {
            deduplicated_zones.push(zone);
        }
    }

    deduplicated_zones
}

/// `ZONE_OVERLAP_PAIR` (indices into the board zone list).
#[derive(Clone)]
struct ZoneOverlapPair {
    zone_a: usize,
    zone_b: usize,
    shared_layers: Lset,
}

/// `ZONE_PRIORITY_EDGE` (indices into the board zone list).
#[derive(Clone, Copy)]
struct ZonePriorityEdge {
    higher: usize,
    lower: usize,
    count_diff: i32,
    from_area: bool,
}

/// The outline used for collision tests: `GetBoardOutline()` for a footprint zone, else
/// `Outline()` (the stored outline is the board-frame outline then).
fn collision_outline(zone: &Zone, ctx: &BoardCtx<'_>) -> ShapePolySet {
    if zone.parent_transform(ctx).is_some() {
        zone.board_outline(ctx)
    } else {
        zone.outline().clone()
    }
}

/// `GetParentFootprint() ? GetBoardOutline().Area() : Outline()->Area()`.
fn outline_area(zone: &Zone, ctx: &BoardCtx<'_>) -> f64 {
    if zone.parent_transform(ctx).is_some() {
        zone.board_outline(ctx).area()
    } else {
        zone.outline().area()
    }
}

/// Port of `findOverlappingPairs( aBoard )`: copper zone pairs (no rule or teardrop areas)
/// sharing a copper layer whose outlines collide or contain one another's first vertex.
fn find_overlapping_pairs(ctx: &BoardCtx<'_>, zones: &[(ZoneId, &Zone)]) -> Vec<ZoneOverlapPair> {
    let mut pairs = Vec::new();

    for i in 0..zones.len() {
        let a = zones[i].1;

        if a.is_rule_area() || a.is_teardrop_area() || !a.is_on_copper_layer(ctx) {
            continue;
        }

        let bbox_a = a.bounding_box(ctx);

        for (j, &(_, b)) in zones.iter().enumerate().skip(i + 1) {
            if b.is_rule_area() || b.is_teardrop_area() || !b.is_on_copper_layer(ctx) {
                continue;
            }

            let mut shared = a.layer_set.clone() & &b.layer_set;
            shared &= Lset::all_cu_mask();

            if shared.none() {
                continue;
            }

            if !b.bounding_box(ctx).intersects(&bbox_a) {
                continue;
            }

            let a_outline = collision_outline(a, ctx);
            let b_outline = collision_outline(b, ctx);

            let overlaps = a_outline.collide_shape(&b_outline, 0, None, None)
                || (b_outline.total_vertices() > 0
                    && a_outline.contains(b_outline.c_vertex_global(0), -1, 0, false))
                || (a_outline.total_vertices() > 0
                    && b_outline.contains(a_outline.c_vertex_global(0), -1, 0, false));

            if overlaps {
                pairs.push(ZoneOverlapPair {
                    zone_a: i,
                    zone_b: j,
                    shared_layers: shared,
                });
            }
        }
    }

    pairs
}

/// Port of `computeConstraint( aPair, aBoard )`: which zone of a different-net pair should
/// win, by the pads and vias of each net inside the overlap (or the smaller area when the
/// counts are similar or zero).
fn compute_constraint(
    ctx: &BoardCtx<'_>,
    zones: &[(ZoneId, &Zone)],
    pair: &ZoneOverlapPair,
) -> Option<ZonePriorityEdge> {
    let zone_a = zones[pair.zone_a].1;
    let zone_b = zones[pair.zone_b].1;

    let mut poly_a = zone_a.board_outline(ctx);
    let mut poly_b = zone_b.board_outline(ctx);
    poly_a.clear_arcs();
    poly_b.clear_arcs();

    let mut intersection = ShapePolySet::new();
    intersection.boolean_intersection_sets(&poly_a, &poly_b);

    if intersection.is_empty() {
        return None;
    }

    intersection.build_bbox_caches();

    let net_code_a = zone_a.base.net;
    let net_code_b = zone_b.base.net;

    // Same-net overlapping zones are cooperative, not competitive. Priority
    // between them is meaningless to the fill engine. Return no constraint
    // here; AutoAssignZonePriorities() groups them to the same priority level.
    if net_code_a == net_code_b {
        return None;
    }

    let mut count_a = 0i32;
    let mut count_b = 0i32;

    let mut count_if_in_overlap = |pos, net_code, layer: PcbLayerId| {
        if !pair.shared_layers.test(layer) {
            return;
        }

        if intersection.contains(pos, -1, 0, false) {
            if net_code == net_code_a {
                count_a += 1;
            } else if net_code == net_code_b {
                count_b += 1;
            }
        }
    };

    let board = ctx.board;
    let shared_seq = pair.shared_layers.seq();

    for (fp, _) in board.footprints() {
        for (_, pad) in board.footprint_pads(fp) {
            for layer in shared_seq.iter().copied() {
                if pad.is_on_layer(ctx, layer) {
                    count_if_in_overlap(pad.position(ctx), pad.net_code(), layer);
                    break;
                }
            }
        }
    }

    for (_, track) in board.tracks() {
        let Track::Via(via) = track else {
            continue;
        };

        for layer in shared_seq.iter().copied() {
            if via.is_on_layer(ctx, layer) {
                count_if_in_overlap(via.position(ctx), via.net_code(), layer);
                break;
            }
        }
    }

    let area_edge = |count_diff: i32| -> Option<ZonePriorityEdge> {
        let area_a = outline_area(zone_a, ctx);
        let area_b = outline_area(zone_b, ctx);

        if area_a == area_b {
            return None;
        }

        let higher = if area_a < area_b {
            pair.zone_a
        } else {
            pair.zone_b
        };
        let lower = if higher == pair.zone_a {
            pair.zone_b
        } else {
            pair.zone_a
        };

        Some(ZonePriorityEdge {
            higher,
            lower,
            count_diff,
            from_area: true,
        })
    };

    if count_a == 0 && count_b == 0 {
        return area_edge(0);
    }

    let max_count = count_a.max(count_b);
    let diff = (count_a - count_b).abs();
    let ratio = f64::from(diff) / f64::from(max_count);

    const SIMILARITY_THRESHOLD: f64 = 0.20;

    if ratio < SIMILARITY_THRESHOLD {
        return area_edge(diff);
    }

    let higher = if count_a > count_b {
        pair.zone_a
    } else {
        pair.zone_b
    };
    let lower = if higher == pair.zone_a {
        pair.zone_b
    } else {
        pair.zone_a
    };

    Some(ZonePriorityEdge {
        higher,
        lower,
        count_diff: diff,
        from_area: false,
    })
}

/// Port of `assignPrioritiesFromGraph( aEdges, aAllZones )`: Kahn's topological order of
/// the "higher than" graph (sources first, ties by current priority via libstdc++
/// `std::sort`), zones stuck in cycles appended by current priority; the order gets
/// descending priorities. `priorities[i]` is the priority of board zone `i`.
fn assign_priorities_from_graph(
    edges: &[ZonePriorityEdge],
    all_zones: &[usize],
    priorities: &mut [u32],
) {
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut in_degree: HashMap<usize, i32> = HashMap::new();

    for &z in all_zones {
        in_degree.insert(z, 0);
    }

    // Sort edges so area-based (weakest) come first, then by ascending countDiff
    let mut sorted_edges = edges.to_vec();

    std_sort(&mut sorted_edges, |a, b| {
        if a.from_area != b.from_area {
            return a.from_area;
        }

        a.count_diff < b.count_diff
    });

    for edge in &sorted_edges {
        adj.entry(edge.higher).or_default().push(edge.lower);
        *in_degree.entry(edge.lower).or_default() += 1;
    }

    // Kahn's algorithm: sources (in-degree 0) have nothing constraining them to be lower,
    // so they are the highest-priority zones. Process them first.
    let mut queue: Vec<usize> = Vec::new();

    for &z in all_zones {
        if in_degree.get(&z).copied().unwrap_or(0) == 0 {
            queue.push(z);
        }
    }

    {
        let pr: &[u32] = priorities;
        std_sort(&mut queue, |a, b| pr[*a] < pr[*b]);
    }

    let mut topo_order: Vec<usize> = Vec::with_capacity(all_zones.len());

    while !queue.is_empty() {
        let current = queue.remove(0);
        topo_order.push(current);

        let neighbors = adj.entry(current).or_default();

        {
            let pr: &[u32] = priorities;
            std_sort(neighbors, |a, b| pr[*a] < pr[*b]);
        }

        for &neighbor in neighbors.iter() {
            let degree = in_degree.entry(neighbor).or_default();
            *degree -= 1;

            if *degree == 0 {
                queue.push(neighbor);
            }
        }

        let pr: &[u32] = priorities;
        std_sort(&mut queue, |a, b| pr[*a] < pr[*b]);
    }

    // Zones stuck in cycles get appended sorted by their current priority
    if topo_order.len() < all_zones.len() {
        let mut remaining: Vec<usize> = all_zones
            .iter()
            .copied()
            .filter(|z| !topo_order.contains(z))
            .collect();

        let pr: &[u32] = priorities;
        std_sort(&mut remaining, |a, b| pr[*a] < pr[*b]);

        topo_order.extend(remaining);
    }

    // topoOrder[0] is the highest-priority zone (source node). Assign descending values.
    let n = topo_order.len();

    for (i, &z) in topo_order.iter().enumerate() {
        priorities[z] = (n - 1 - i) as u32;
    }
}

/// Port of `ufFind()` (path compression).
fn uf_find(parent: &mut HashMap<usize, usize>, zone: usize) -> usize {
    let p = *parent.entry(zone).or_insert(0);

    if p != zone {
        let root = uf_find(parent, p);
        parent.insert(zone, root);
        return root;
    }

    p
}

/// Port of `ufUnion()` (union by rank).
fn uf_union(
    parent: &mut HashMap<usize, usize>,
    rank: &mut HashMap<usize, i32>,
    a: usize,
    b: usize,
) {
    let mut root_a = uf_find(parent, a);
    let mut root_b = uf_find(parent, b);

    if root_a == root_b {
        return;
    }

    if rank.get(&root_a).copied().unwrap_or(0) < rank.get(&root_b).copied().unwrap_or(0) {
        std::mem::swap(&mut root_a, &mut root_b);
    }

    parent.insert(root_b, root_a);

    if rank.get(&root_a).copied().unwrap_or(0) == rank.get(&root_b).copied().unwrap_or(0) {
        *rank.entry(root_a).or_default() += 1;
    }
}

/// Port of `AutoAssignZonePriorities( aBoard, aReporter )` (no progress reporter): orders
/// overlapping copper zones of different nets by the pads / vias of their nets inside the
/// overlap (or by area), gives same-net overlapping zones one shared priority, and writes
/// the priorities (`SetAssignedPriority`, through `modify_zone`). True if any priority
/// changed.
pub fn auto_assign_zone_priorities(board: &mut Board) -> bool {
    let (eligible_zones, new_priorities, zone_ids) = {
        let ctx = BoardCtx::bare(board);
        let zones: Vec<(ZoneId, &Zone)> = board.zones().collect();

        // Indices into `zones` (the board zone list) of the eligible zones.
        let eligible_zones: Vec<usize> = zones
            .iter()
            .enumerate()
            .filter(|(_, (_, zone))| {
                !zone.is_rule_area() && !zone.is_teardrop_area() && zone.is_on_copper_layer(&ctx)
            })
            .map(|(i, _)| i)
            .collect();

        if eligible_zones.len() < 2 {
            return false;
        }

        let pairs = find_overlapping_pairs(&ctx, &zones);

        if pairs.is_empty() {
            return false;
        }

        let mut priorities: Vec<u32> = zones.iter().map(|(_, z)| z.assigned_priority()).collect();

        // Build equivalence classes for same-net overlapping zones. These zones
        // are cooperative and must share the same priority after assignment.
        let mut uf_parent: HashMap<usize, usize> = HashMap::new();
        let mut uf_rank: HashMap<usize, i32> = HashMap::new();

        for &z in &eligible_zones {
            uf_parent.insert(z, z);
            uf_rank.insert(z, 0);
        }

        for pair in &pairs {
            if zones[pair.zone_a].1.base.net == zones[pair.zone_b].1.base.net {
                uf_union(&mut uf_parent, &mut uf_rank, pair.zone_a, pair.zone_b);
            }
        }

        let mut edges: Vec<ZonePriorityEdge> = Vec::new();

        for pair in &pairs {
            if zones[pair.zone_a].1.base.net == zones[pair.zone_b].1.base.net {
                continue;
            }

            if let Some(edge) = compute_constraint(&ctx, &zones, pair) {
                edges.push(edge);
            }
        }

        if !edges.is_empty() {
            assign_priorities_from_graph(&edges, &eligible_zones, &mut priorities);
        }

        // Equalize priorities within each same-net equivalence class. Each group
        // gets the maximum priority of any member so ordering constraints from
        // different-net edges propagate to the whole group.
        let mut group_max: HashMap<usize, u32> = HashMap::new();

        for &z in &eligible_zones {
            let root = uf_find(&mut uf_parent, z);
            let pri = priorities[z];
            let max_pri = group_max.entry(root).or_insert(0);

            if pri > *max_pri {
                *max_pri = pri;
            }
        }

        for &z in &eligible_zones {
            let root = uf_find(&mut uf_parent, z);
            priorities[z] = group_max[&root];
        }

        let zone_ids: Vec<ZoneId> = zones.iter().map(|(id, _)| *id).collect();
        (eligible_zones, priorities, zone_ids)
    };

    let mut changed = false;

    for &z in &eligible_zones {
        let id = zone_ids[z];
        let new_priority = new_priorities[z];

        if board[id].assigned_priority() != new_priority {
            board.modify_zone(id, |zone, _| zone.set_assigned_priority(new_priority));
            changed = true;
        }
    }

    changed
}
