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

//! Port of the iterative refill of `pcbnew/zone_filler.cpp`: the refill loop of
//! `ZONE_FILLER::Fill` (L1381-1659) and `ZONE_FILLER::refillZoneFromCache` (L4587-4820).
//!
//! Each wave reads a snapshot of the fills taken before it, so the candidates of one wave are
//! independent and the sequential port refills them in candidate order. The refill result
//! cache (`m_refillResultCache`, keyed by a hash of the knockouts) is a pure optimisation and
//! is not ported (connectivity-zones.md R-13).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_connectivity::algo::IsolatedIslandsMap;
use kicad_connectivity::data::ConnectivityData;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::hash_128::Hash128;
use kicad_pcb::board::Board;
use kicad_pcb::ctx::{BoardCtx, RuleResolver};
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::zone::{IsolatedIslands, Zone};
use kicad_pcb::zone_settings::ZoneFillMode;

use crate::copper_fill::drop_sub_resolution_outlines;
use crate::filler::{
    EXTRA_CLEARANCE_MM, FillKey, MAX_REFILL_ITERATIONS, ZoneFiller, fill_outline_count,
    remove_islands,
};
use crate::index::{board_and_footprint_zones, query_index};

/// Port of `ZONE_FILLER::FillSnapshot`: zone fills captured before an iterative refill wave.
pub type FillSnapshot = BTreeMap<FillKey, ShapePolySet>;

/// The anonymous `appendZoneOutlineWithoutArcs( aZone, aPolys )`.
fn append_zone_outline_without_arcs(ctx: &BoardCtx<'_>, zone: &Zone, polys: &mut ShapePolySet) {
    let mut outline = zone.board_outline(ctx);

    if outline.arc_count() != 0 {
        outline.clear_arcs();
    }

    polys.append_set(&outline);
}

impl ZoneFiller {
    /// The iterative refill loop of `Fill()` (L1381-1659): up to 8 waves of
    /// `refillZoneFromCache` over the zones a changed (zone, layer) can affect, each followed
    /// by island removal on the refilled zones, until no fill hash changes.
    pub(crate) fn iterative_refill_loop(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: &dyn RuleResolver,
        zones: &[ZoneId],
        mut changed_zone_layers: BTreeSet<FillKey>,
        initially_fully_isolated_layers: &BTreeSet<FillKey>,
    ) {
        let mut hit_iteration_limit = false;

        for iteration in 0..MAX_REFILL_ITERATIONS {
            // Candidate selection: only re-refill (zone, layer) pairs where `layer` is the
            // same layer that changed on some seed zone and whose bbox touches it.
            let zones_to_refill = {
                let ctx = BoardCtx::bare(board).with_rules(rules);
                self.refill_candidates(&ctx, zones, &changed_zone_layers)
            };

            if zones_to_refill.is_empty() {
                break;
            }

            // Snapshot hashes before the wave for convergence detection.  Only zones in
            // zonesToRefill can change their fill this wave (refill writes them; subsequent
            // island removal also only touches them), so we only need pre-hashes for those.
            let mut iter_hashes: BTreeMap<FillKey, Hash128> = BTreeMap::new();

            for &(zone_id, layer) in &zones_to_refill {
                board.zone_fill_mut(zone_id).build_hash_value(layer);

                if let Some(zone) = board.zone(zone_id) {
                    iter_hashes.insert((zone_id, layer), zone.hash_value(layer));
                }
            }

            // Snapshot fills before the wave.  Every refill task reads knockouts from this
            // snapshot so all tasks see the same pre-wave state regardless of completion
            // order.  refillZoneFromCache only reads knockouts on the layer being refilled,
            // so we only need to clone fills on layers that appear in zonesToRefill.
            let snapshot = fill_snapshot(board, &zones_to_refill);

            // run_fill_waves( zonesToRefill, cached_refill_fill_lambda,
            // cached_refill_tessellate_lambda, no_dependency, false )
            for &(zone_id, layer) in &zones_to_refill {
                let result = {
                    let ctx = BoardCtx::bare(board)
                        .with_rules(rules)
                        .with_connectivity(&*connectivity);
                    self.refill_zone_from_cache(&ctx, zone_id, layer, Some(&snapshot))
                };

                let Some(fill_polys) = result else {
                    continue;
                };

                let fill = board.zone_fill_mut(zone_id);
                fill.move_filled_polys_list(layer, fill_polys);
                fill.set_fill_flag(layer, false);

                fill.cache_triangulation(layer);
                fill.set_fill_flag(layer, true);
            }

            // Island detection on the refilled zones only.  Zones that grew into freed space
            // can still develop islands if they are simultaneously blocked on one side by a
            // higher-priority zone that grew in a prior wave.
            let mut refill_islands_map: IsolatedIslandsMap = BTreeMap::new();

            for &(zone_id, layer) in &zones_to_refill {
                if self.debug_zone_filler && Lset::internal_cu_mask().contains(layer) {
                    continue;
                }

                let Some(zone) = board.zone(zone_id) else {
                    continue;
                };

                // Mirrors the initial isolatedIslandsMap build above.
                if zone.is_copper_thieving() || zone.is_teardrop_area() {
                    continue;
                }

                refill_islands_map
                    .entry(zone_id)
                    .or_default()
                    .insert(layer, IsolatedIslands::default());
            }

            connectivity.fill_isolated_islands_map(
                &BoardCtx::bare(board),
                &mut refill_islands_map,
                false,
            );

            for (&zone_id, zone_islands) in &refill_islands_map {
                for (&layer, layer_islands) in zone_islands {
                    if self.debug_zone_filler && Lset::internal_cu_mask().contains(layer) {
                        continue;
                    }

                    if layer_islands.isolated_outlines.is_empty() {
                        continue;
                    }

                    // Preserve layers that were initially fully isolated (unconnected pours):
                    // if every outline on this layer is still an island, keep them as-is.
                    if initially_fully_isolated_layers.contains(&(zone_id, layer))
                        && layer_islands.isolated_outlines.len()
                            == fill_outline_count(board, zone_id, layer) as usize
                    {
                        continue;
                    }

                    remove_islands(board, zone_id, layer, &layer_islands.isolated_outlines);
                }
            }

            // Convergence check: collect zone-layer pairs whose fill changed (refill or
            // island removal) compared to the pre-wave hash snapshot.  These seed the next
            // iteration.  Only zonesToRefill entries can have changed, so we only scan those.
            changed_zone_layers.clear();

            for &fill_item in &zones_to_refill {
                let (zone_id, layer) = fill_item;
                board.zone_fill_mut(zone_id).build_hash_value(layer);

                let old_hash = iter_hashes.get(&fill_item).copied().unwrap_or_default();

                if board
                    .zone(zone_id)
                    .is_some_and(|z| z.hash_value(layer) != old_hash)
                {
                    changed_zone_layers.insert(fill_item);
                }
            }

            if changed_zone_layers.is_empty() {
                break; // Stable - converged.
            }

            if iteration + 1 >= MAX_REFILL_ITERATIONS {
                hit_iteration_limit = true;
                break;
            }
        }

        if hit_iteration_limit {
            // wxLogWarning (no parent window in the port)
            self.warnings.push(format!(
                "Zone fills may be incorrect: iterative refill did not converge after {} \
                 passes.\n\nThis can happen with complex overlapping zones.  Consider \
                 simplifying your zones.",
                MAX_REFILL_ITERATIONS
            ));
        }
    }

    /// The candidate selection of one refill wave (L1411-1467), deduplicated in discovery
    /// order.
    fn refill_candidates(
        &self,
        ctx: &BoardCtx<'_>,
        zones: &[ZoneId],
        changed_zone_layers: &BTreeSet<FillKey>,
    ) -> Vec<FillKey> {
        let board = ctx.board;
        let mut zones_to_refill: Vec<FillKey> = Vec::new();
        let mut zones_to_refill_set: BTreeSet<FillKey> = BTreeSet::new();

        for &(changed_id, changed_layer) in changed_zone_layers {
            let Some(changed_zone) = board.zone(changed_id) else {
                continue;
            };

            let mut bbox = changed_zone.bounding_box(ctx);
            bbox.inflate(self.worst_clearance);

            for &zone_id in zones {
                let Some(zone) = board.zone(zone_id) else {
                    continue;
                };

                if zone.is_rule_area() {
                    continue;
                }

                // Nothing that can shrink outranks a teardrop, so no refill frees space for
                // one. A refill would restore the fill it already has.
                if zone.is_teardrop_area() {
                    continue;
                }

                if !zone.layer_set.test(changed_layer) {
                    continue;
                }

                // A candidate only needs re-evaluation when the changed zone can affect it in
                // one of two ways: 1. fill shape (the changed zone is a higher-priority
                // knockout of the candidate), 2. connectivity cluster (same net: refilling
                // from cache restores outlines that were removed as islands; this drives
                // cascading island refills).
                if zone_id != changed_id
                    && !changed_zone.higher_priority(zone)
                    && !changed_zone.same_net(zone)
                {
                    continue;
                }

                // Same gate as the initial fill keeps the refill's knockout set identical;
                // same-net candidates interact through connectivity, not a knockout.
                if zone_id != changed_id && !changed_zone.same_net(zone) {
                    if !self.zone_knockout_may_interact(ctx, zone, changed_zone) {
                        continue;
                    }
                } else if !zone.bounding_box(ctx).intersects(&bbox) {
                    continue;
                }

                let fill_item = (zone_id, changed_layer);

                if zones_to_refill_set.insert(fill_item) {
                    zones_to_refill.push(fill_item);
                }
            }
        }

        zones_to_refill
    }

    /// Port of `ZONE_FILLER::refillZoneFromCache( aZone, aLayer, aFillPolys, aSnapshot )`:
    /// refill a zone from its cached pre-knockout fill, re-applying only the knockouts of
    /// higher-priority zones with their current (snapshot) fills. `None` where the C++
    /// returns false (no cached fill).
    pub fn refill_zone_from_cache(
        &self,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
        snapshot: Option<&FillSnapshot>,
    ) -> Option<ShapePolySet> {
        let cache_key = (zone_id, layer);
        let zone = ctx.board.zone(zone_id)?;

        // Restore the cached pre-knockout fill
        let mut fill_polys = self.pre_knockout_fill_cache.get(&cache_key)?.clone();

        // Subtract the FILLED area of higher-priority zones (with clearance for different
        // nets).  For same-net zones: subtract the filled area directly.  For different-net
        // zones: subtract the filled area with DRC-evaluated clearance plus extra_margin and
        // m_maxError to match the margins used in the initial fill. Without these margins,
        // polygon approximation error can produce fills that violate clearance (issue 23053).
        let extra_margin = PCB_IU_SCALE.mm_to_iu(EXTRA_CLEARANCE_MM);
        let mut zone_bbox = zone.bounding_box(ctx);
        zone_bbox.inflate(self.worst_clearance + extra_margin);

        let eval_rules_for_items =
            |constraint: DrcConstraintT, a: ZoneId, b: ZoneId, eval_layer: PcbLayerId| -> i32 {
                let Some(rules) = ctx.rules else {
                    return -1;
                };

                let c = rules.eval_rules(
                    ctx,
                    constraint,
                    ctx.item(a.into()),
                    ctx.item(b.into()),
                    eval_layer,
                );

                if c.is_null() { -1 } else { c.value().min() }
            };

        let mut knockouts_applied = false;
        let mut diff_net_knockouts = ShapePolySet::new();
        let mut same_net_knockouts = ShapePolySet::new();

        let mut collect_zone_knockout = |other_id: ZoneId| {
            if other_id == zone_id {
                return;
            }

            let Some(other_zone) = ctx.board.zone(other_id) else {
                return;
            };

            if !other_zone.layer_set.test(layer) {
                return;
            }

            // The cached pre-knockout fill already holds the teardrop knockouts.
            if other_zone.is_teardrop_area() {
                return;
            }

            if !other_zone.higher_priority(zone) {
                return;
            }

            // Same gate as the initial fill so the refill's knockout set matches; same-net
            // fills are subtracted un-inflated, so a plain bbox test suffices.
            if other_zone.same_net(zone) {
                if !other_zone.bounding_box(ctx).intersects(&zone_bbox) {
                    return;
                }
            } else if !self.zone_knockout_may_interact(ctx, zone, other_zone) {
                return;
            }

            // Resolve the fill to use: from the snapshot when provided, otherwise the live
            // fill.
            let fill: &ShapePolySet = match snapshot {
                Some(snapshot) => match snapshot.get(&(other_id, layer)) {
                    Some(fill) => fill,
                    None => return, // not filled at snapshot time; skip
                },
                None => match other_zone.get_fill(layer) {
                    Some(fill) => fill,
                    None => return,
                },
            };

            if fill.outline_count() == 0 {
                return;
            }

            if other_zone.same_net(zone) {
                // Equal priorities tie-break on UUID in HigherPriority(). The initial fill
                // only gives strictly-higher zones their outline.
                let owns_outline = other_zone.fill_mode() == ZoneFillMode::HatchPattern
                    && other_zone.assigned_priority() > zone.assigned_priority();

                if owns_outline {
                    append_zone_outline_without_arcs(ctx, other_zone, &mut same_net_knockouts);
                } else {
                    same_net_knockouts.append_set(fill);
                }
            } else {
                let mut gap = eval_rules_for_items(
                    DrcConstraintT::PhysicalClearanceConstraint,
                    zone_id,
                    other_id,
                    layer,
                );
                gap = gap.max(eval_rules_for_items(
                    DrcConstraintT::ClearanceConstraint,
                    zone_id,
                    other_id,
                    layer,
                ));

                if gap < 0 {
                    return;
                }

                let mut inflated_fill = fill.clone();
                inflated_fill.inflate(
                    gap + extra_margin + self.max_error,
                    CornerStrategy::RoundAllCorners,
                    self.max_error,
                    false,
                );
                diff_net_knockouts.append_set(&inflated_fill);
                knockouts_applied = true;
            }
        };

        if let Some(index) = self.zone_index.get(&layer) {
            let hits = query_index(index, &self.zone_knockout_query_box(ctx, zone));

            for hit in hits {
                if let Some(other) = hit.item.typed::<Zone>() {
                    collect_zone_knockout(other);
                }
            }
        }

        // Keepout zones are not collected here because they are already baked into the cached
        // pre-knockout fill.  They were subtracted before the initial deflate/inflate min-width
        // cycle so the cached fill already reflects keepout boundaries (issue 23515).

        // Subtract different-net knockouts first, then re-prune min-width violations BEFORE
        // subtracting same-net knockouts.  The cached fill was already trimmed to the zone
        // outline, so the prune needs the cached apron to stand in for the overlap with
        // abutting same-net zones and keep the deflate/inflate cycle from carving divots at
        // their shared boundaries.
        if diff_net_knockouts.outline_count() > 0 {
            fill_polys.boolean_subtract(&diff_net_knockouts);
        }

        if knockouts_applied {
            let mut same_net_apron = self
                .same_net_apron_cache
                .get(&cache_key)
                .cloned()
                .unwrap_or_default();

            // The apron may only buffer where copper can still go, so it takes the same
            // knockouts
            if same_net_apron.outline_count() > 0 && diff_net_knockouts.outline_count() > 0 {
                same_net_apron.boolean_subtract(&diff_net_knockouts);
            }

            // The test stand-in for the pending copper fill has no min-width cycle.
            if !self.trivial_copper_fill {
                self.post_knockout_min_width_prune(zone, &mut fill_polys, &same_net_apron);
            }
        }

        if same_net_knockouts.outline_count() > 0 {
            fill_polys.boolean_subtract(&same_net_knockouts);
        }

        // The cache was hatched before these knockouts, so restore the border the carve cut
        // through with a min-width ring, bounded by the un-hatched extent to stay
        // clearance-safe (issue 24758).
        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            let solid_extent = self
                .pre_hatch_solid_fill_cache
                .get(&cache_key)
                .cloned()
                .unwrap_or_default();

            let mut knockouts = diff_net_knockouts.clone();
            knockouts.append_set(&same_net_knockouts);

            if solid_extent.outline_count() > 0 && knockouts.outline_count() > 0 {
                let mut border = knockouts.clone();
                border.inflate(
                    zone.min_thickness(),
                    CornerStrategy::RoundAllCorners,
                    self.max_error,
                    false,
                );
                border.boolean_subtract(&knockouts);
                border.boolean_intersection(&solid_extent);

                fill_polys.boolean_add(&border);
            }
        }

        drop_sub_resolution_outlines(&mut fill_polys, self.max_error);

        fill_polys.fracture(true);

        Some(fill_polys)
    }
}

/// The fill snapshot of one refill wave: every non-rule board / footprint zone's non-empty
/// fill (`CloneDropTriangulation`) on the copper layers of the wave.
fn fill_snapshot(board: &Board, zones_to_refill: &[FillKey]) -> FillSnapshot {
    let ctx = BoardCtx::bare(board);
    let mut snapshot_layers = Lset::new();

    for &(_, layer) in zones_to_refill {
        snapshot_layers.set(layer);
    }

    let mut snapshot = FillSnapshot::new();

    for zone_id in board_and_footprint_zones(&ctx) {
        let Some(zone) = board.zone(zone_id) else {
            continue;
        };

        if zone.is_rule_area() {
            continue;
        }

        let mut copper_layers = zone.layer_set.clone();
        copper_layers &= &Lset::all_cu_mask_n(board.copper_layer_count());
        copper_layers &= &snapshot_layers;

        for layer in copper_layers.iter() {
            let Some(fill) = zone.fill().filled_polys_list.get(&layer) else {
                continue;
            };

            if fill.outline_count() > 0 {
                snapshot.insert(
                    (zone_id, layer),
                    Arc::clone(fill).clone_drop_triangulation(),
                );
            }
        }
    }

    snapshot
}
