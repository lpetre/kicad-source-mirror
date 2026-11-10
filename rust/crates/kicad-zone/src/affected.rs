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

//! Affected-only zone refill (package LIVE-2): `ZONE_FILLER_TOOL::FillAllZones` with
//! `ZONE_FILLER::Fill` run on the zones whose fill can change only.
//!
//! New code without one C++ counterpart. The nearest C++ is the interactive auto-refill
//! (`BOARD_COMMIT::propagateDamage` collects the zones whose bounding box meets a changed
//! item on a shared copper layer, `ZONE_FILLER_TOOL::ZoneFillDirty` refills them with
//! `ZONE_FILLER::Fill( toFill )`), which is a heuristic. This module refills a superset that
//! makes the result identical to [`crate::fill_all_zones`] on the same board, given that the
//! fills before the changes are those of a full refill of the board before the changes:
//!
//! 1. The teardrop update and time stamp increment of `FillAllZones` run unchanged. Teardrop
//!    zones are re-created by it with new handles and UUIDs; a new teardrop equal in every
//!    setting and in its outline to a removed, filled one ("re-created") takes over the old
//!    fill unless it has a coupled zone of equal priority (the `HigherPriority()` tie is
//!    broken by the UUID, which changed). Any other added or removed teardrop is damage.
//! 2. Seeds: every fillable zone that shares a layer with a damage rectangle (any layer:
//!    non-copper zones knock out the items on their layer) and whose
//!    bounding box, inflated by the reach of [`seed_margin`], meets it; every zone that a
//!    connectivity cluster of its net touching a changed area meets (island removal reads
//!    the clusters, [`cluster_nets_near`]); every zone that is not filled (`ZoneFillDirty`'s `!IsFilled()`). Vias
//!    and pads whose bounding box meets a damage rectangle are damage themselves (their
//!    conditional flashing reads the connectivity at their position).
//! 3. Closure: a zone is added while it is *coupled* to a zone of the set (a shared layer and
//!    `zoneKnockoutMayInteract` in either direction: the gate of every read of another zone's
//!    fill, of the fill DAG and of the iterative refill) or classifies islands and has the
//!    net of a zone of the set
//!    (the islands of a net's zones read each other's fills through the connectivity).
//! 4. `ZONE_FILLER::Fill( set )` in board order, then `BuildConnectivity` as `FillAllZones`.
//!
//! Why the zones outside the set keep the fill a full refill would give them: each depends
//! (fill DAG reads, iterative refill, islands) only on zones it is coupled to or shares its
//! net with, all outside the set by step 3, and on board items within its reach, none of
//! which changed by step 2; its previous fill was computed from the same inputs. Inside the
//! set, every zone a member reads is a member too, so the reads see this run's results as in
//! the full run. Everything global in `Fill()` (connectivity rebuild, item indexes, the two
//! flashing passes over every via and pad) runs as in the full refill.

use std::collections::BTreeSet;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::PcbLayerId;
use kicad_core::kiid::Kiid;
use kicad_core::lset::Lset;
use kicad_geom::Box2I;
use kicad_pcb::Board;
use kicad_pcb::ctx::{BoardCtx, RuleResolver};
use kicad_pcb::ids::ZoneId;
use kicad_pcb::zone::{Zone, ZoneFill};

use crate::filler::ZoneFiller;
use crate::tool::update_teardrops;

/// A changed area: the bounding box of an item before or after a change with the item's
/// layers.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageRect {
    /// `GetBoundingBox()` of the item.
    pub bbox: Box2I,
    /// `GetLayerSet()` of the item.
    pub layers: Lset,
}

/// What changed since the fills were last those of a full refill.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneDamage {
    /// Unknown changes: refill everything.
    pub full: bool,
    /// The changed areas.
    pub rects: Vec<DamageRect>,
    /// The nets of the changed items (before and after the change).
    pub nets: BTreeSet<i32>,
}

impl ZoneDamage {
    /// Damage that forces a full refill.
    pub fn full() -> Self {
        ZoneDamage {
            full: true,
            ..ZoneDamage::default()
        }
    }

    /// Nothing changed.
    pub fn is_empty(&self) -> bool {
        !self.full && self.rects.is_empty() && self.nets.is_empty()
    }

    /// Adds `other` (the changes of a later commit).
    pub fn merge(&mut self, other: &ZoneDamage) {
        self.full |= other.full;

        if self.full {
            self.rects.clear();
            self.nets.clear();
            return;
        }

        self.rects.extend(other.rects.iter().cloned());
        self.nets.extend(other.nets.iter().copied());
    }

    /// Adds an item's area and net. Items on `Edge.Cuts` or `Margin` (board outline) force a
    /// full refill, as they change the outline every zone is clipped to.
    pub fn add_area(&mut self, bbox: Box2I, layers: Lset, net: Option<i32>) {
        if layers.contains(PcbLayerId::EdgeCuts) || layers.contains(PcbLayerId::Margin) {
            self.full = true;
        }

        if let Some(net) = net {
            self.nets.insert(net);
        }

        self.rects.push(DamageRect { bbox, layers });
    }
}

/// What [`fill_affected_zones`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AffectedFillOutcome {
    /// The return value of `FillAllZones`.
    pub filled: bool,
    /// The filler's `wxLogWarning` texts.
    pub warnings: Vec<String>,
    /// The damage forced a full refill.
    pub full: bool,
    /// The zones `ZONE_FILLER::Fill` ran on, in board order.
    pub refilled: Vec<ZoneId>,
    /// The re-created teardrops that took over the fill of their predecessor.
    pub reused: Vec<ZoneId>,
    /// Board zones that keep their fill (not refilled, not re-created).
    pub kept: u32,
    /// Why zones are in the refill set: not filled, new or changed teardrop, net of a change,
    /// near a change, re-created teardrop with an equal-priority coupled zone, added by the
    /// closure.
    pub reasons: [u32; 6],
}

/// The settings of a teardrop zone that its fill depends on: every field of `ZONE` except
/// the UUID, the handle, the priority (re-assigned by `setTeardropPriorities` in an order
/// that breaks area ties by UUID; it only decides the relation to other teardrops, see
/// [`affected_set`]), the fill results and the derived display caches.
fn teardrop_signature(z: &Zone) -> Option<String> {
    let outline = z.library_outline();

    // Outlines with arcs are never matched (their vertices alone do not give the geometry).
    for i in 0..outline.outline_count() {
        if outline.c_outline(i).arc_count() > 0
            || (0..outline.hole_count(i)).any(|h| outline.c_hole(i, h).arc_count() > 0)
        {
            return None;
        }
    }

    Some(format!(
        "{:?}",
        (
            (
                outline_points(z.library_outline()),
                &z.layer_set,
                z.base.net,
                &z.base.teardrop_params,
                z.base.item.layer,
                z.base.item.is_locked,
                z.base.item.is_knockout,
                z.teardrop_type,
                &z.layer_properties,
                &z.zone_name,
            ),
            (
                z.corner_smoothing_type,
                z.corner_radius,
                z.is_rule_area,
                z.pad_connection,
                z.zone_clearance,
                z.zone_min_thickness,
                z.fill_version,
                z.island_removal_mode,
                z.min_island_area,
                z.thermal_relief_gap,
                z.thermal_relief_spoke_width,
                z.fill_mode,
            ),
            (
                z.hatch_thickness,
                z.hatch_gap,
                z.hatch_orientation,
                z.hatch_smoothing_level,
                z.hatch_smoothing_value.to_bits(),
                z.hatch_hole_min_area.to_bits(),
                z.hatch_border_algorithm,
                &z.thieving_settings,
                z.local_flgs,
                z.border_style,
                z.border_hatch_pitch,
                z.do_not_allow_zone_fills,
            ),
        )
    ))
}

/// The vertices (and arc flags) of every outline and hole of a polygon set: the geometry
/// without the cached triangulation and bounding boxes that `Debug` would print.
fn outline_points(set: &kicad_geom::ShapePolySet) -> Vec<Vec<Vec<kicad_geom::Vector2I>>> {
    (0..set.outline_count())
        .map(|i| {
            let mut chains = vec![set.c_outline(i)];
            chains.extend((0..set.hole_count(i)).map(|h| set.c_hole(i, h)));
            chains.into_iter().map(|c| c.c_points().to_vec()).collect()
        })
        .collect()
}

/// Whether `Fill()` classifies the zone's islands (`isolatedIslandsMap`): not for
/// copper-thieving stamps and teardrops, whose fill then never reads the connectivity.
fn tracks_islands(z: &Zone) -> bool {
    !z.is_copper_thieving() && !z.is_teardrop_area()
}

/// A zone `ZONE_FILLER::Fill` fills (`Fill()` skips rule areas and zones of two corners or
/// less).
fn fillable(z: &Zone) -> bool {
    !z.is_rule_area() && z.num_corners() > 2
}

/// The inflation of a zone's bounding box within which a changed item can change its fill:
/// the knockout reach of `zoneKnockoutMayInteract` (worst clearance, knockout slack, minimum
/// width, corner radii) plus the worst thermal relief gap and spoke width (pad knockouts and
/// spokes extend that far past a pad), doubled, plus 1 mm. Deliberately generous: a larger
/// margin only refills more zones.
fn seed_margin(board: &Board, rules: &dyn RuleResolver, filler: &ZoneFiller, zone: &Zone) -> i32 {
    use kicad_pcb::drc_api::DrcConstraintT as C;

    let mut worst = filler.worst_clearance;

    for c in [
        C::ClearanceConstraint,
        C::HoleClearanceConstraint,
        C::EdgeClearanceConstraint,
        C::PhysicalClearanceConstraint,
        C::PhysicalHoleClearanceConstraint,
        C::ThermalReliefGapConstraint,
        C::ThermalSpokeWidthConstraint,
    ] {
        if let Some(con) = rules.query_worst_constraint(c) {
            let v = con.value();

            for (has, x) in [
                (v.has_min(), v.min()),
                (v.has_opt(), v.opt()),
                (v.has_max(), v.max()),
            ] {
                if has {
                    worst = worst.max(x);
                }
            }
        }
    }

    let mut thermal = 0;

    for (_, z) in board.zones() {
        thermal = thermal.max(z.thermal_relief_gap + z.thermal_relief_spoke_width);
    }

    let reach = (worst as i64) * 2
        + thermal as i64 * 2
        + filler.zone_knockout_slack as i64
        + zone.min_thickness() as i64 * 2
        + filler.max_zone_corner_radius as i64
        + zone.corner_radius as i64
        + 1_000_000;

    reach.min(i32::MAX as i64 / 4) as i32
}

/// `ZONE_FILLER_TOOL::FillAllZones( nullptr, aReporter, true )` (see [`crate::fill_all_zones`])
/// with `ZONE_FILLER::Fill` restricted to the zones `damage` can affect (module comment).
/// `damage.full` runs the full refill.
pub fn fill_affected_zones(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    rules: &dyn RuleResolver,
    damage: &ZoneDamage,
) -> AffectedFillOutcome {
    if damage.full {
        return full_refill(board, connectivity, rules);
    }

    let worst_before = board.get_max_clearance_value(&BoardCtx::bare(board).with_rules(rules));

    // The teardrops before the update (they are all removed and re-created by it).
    #[allow(clippy::type_complexity)]
    let old_teardrops: Vec<(Option<String>, ZoneFill, Box2I, Lset, i32, (u32, Kiid))> = {
        let ctx = BoardCtx::bare(board).with_rules(rules);
        board
            .zones()
            .filter(|(_, z)| z.is_teardrop_area())
            .map(|(_, z)| {
                (
                    teardrop_signature(z),
                    z.fill().clone(),
                    z.bounding_box(&ctx),
                    z.layer_set.clone(),
                    z.base.net.0,
                    (z.priority, z.base.item.uuid),
                )
            })
            .collect()
    };
    let old_keys: Vec<(u32, Kiid)> = old_teardrops.iter().map(|t| t.5).collect();

    // TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, true )
    update_teardrops(board, connectivity, rules, true);

    board.increment_time_stamp(); // Clear caches

    let worst_after = board.get_max_clearance_value(&BoardCtx::bare(board).with_rules(rules));

    if worst_before != worst_after {
        // The reach of every knockout gate changed: no zone can be proven unaffected.
        return fill_after_teardrops(board, connectivity, rules, None);
    }

    let mut damage = damage.clone();
    let mut matched = vec![false; old_teardrops.len()];
    let mut recreated: Vec<(ZoneId, usize)> = Vec::new();

    {
        let ctx = BoardCtx::bare(board).with_rules(rules);

        for (id, z) in board.zones().filter(|(_, z)| z.is_teardrop_area()) {
            let sig = teardrop_signature(z);
            let found = old_teardrops.iter().enumerate().position(|(k, old)| {
                !matched[k] && old.1.is_filled && sig.is_some() && old.0 == sig
            });

            match found {
                Some(k) => {
                    matched[k] = true;
                    recreated.push((id, k));
                }
                None => damage.add_area(
                    z.bounding_box(&ctx),
                    z.layer_set.clone(),
                    Some(z.base.net.0),
                ),
            }
        }
    }

    for (k, old) in old_teardrops.iter().enumerate() {
        if !matched[k] {
            damage.add_area(old.2, old.3.clone(), Some(old.4));
        }
    }

    if damage.full {
        return fill_after_teardrops(board, connectivity, rules, None);
    }

    // Re-created teardrops take over their predecessor's fill, so that the connectivity
    // sees them; the ones the set below contains get their new (empty) fill back before
    // `Fill()`, which hashes the fill it replaces.
    let mut fresh: Vec<(ZoneId, ZoneFill)> = Vec::new();

    for (id, k) in &recreated {
        let fill = board.zone_fill_mut(*id);
        fresh.push((*id, fill.clone()));
        *fill = old_teardrops[*k].1.clone();
    }

    let mut filler = ZoneFiller::new(board);
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    // The connectivity `Fill()` starts from (it rebuilds it the same way first).
    connectivity.clear_ratsnest();
    connectivity.build_board(board);

    let set = {
        let ctx = BoardCtx::bare(board).with_rules(rules);
        filler.worst_clearance = worst_after;

        for zone in board.get_zone_list(true) {
            if let Some(z) = board.zone(zone) {
                // (int) zone->GetCornerRadius()
                filler.max_zone_corner_radius =
                    filler.max_zone_corner_radius.max(z.corner_radius as i32);
            }
        }

        // Vias and pads at a changed area: their flashing reads the connectivity there.
        let rects = damage.rects.clone();
        let hit = |bbox: &Box2I| rects.iter().any(|r| r.bbox.intersects(bbox));

        for (id, t) in board.tracks() {
            if matches!(t, kicad_pcb::pcb_track::Track::Via(_)) {
                if let Some(item) = ctx.item(id.into()) {
                    let bbox = item.bounding_box();

                    if hit(&bbox) {
                        damage.add_area(bbox, item.layer_set(), None);
                    }
                }
            }
        }

        for (fp, _) in board.footprints() {
            for (pad, _) in board.footprint_pads(fp) {
                if let Some(item) = ctx.item(pad.into()) {
                    let bbox = item.bounding_box();

                    if hit(&bbox) {
                        damage.add_area(bbox, item.layer_set(), None);
                    }
                }
            }
        }

        let near_clusters = clusters_near(&ctx, connectivity, &damage);

        affected_set(
            board,
            &ctx,
            rules,
            &filler,
            &damage,
            &zones,
            &recreated,
            &old_keys,
            &near_clusters,
        )
    };

    let Some((set, reused_from, reasons)) = set else {
        return fill_after_teardrops(board, connectivity, rules, None);
    };

    for (id, mut fill) in fresh {
        let layers: Vec<PcbLayerId> = board
            .zone(id)
            .map(|z| z.layer_set.iter().collect())
            .unwrap_or_default();

        if set.contains(&id) {
            *board.zone_fill_mut(id) = fill;
        } else {
            // `Fill()` hashes the fill it is about to replace: for a re-created teardrop the
            // new, empty one.
            for &layer in &layers {
                fill.build_hash_value(layer);
            }

            board.zone_fill_mut(id).filled_polys_hash = fill.filled_polys_hash;
        }
    }

    // ... and for every other zone it fills its current fill, which the kept zones keep.
    let recreated_ids: BTreeSet<ZoneId> = recreated.iter().map(|(id, _)| *id).collect();

    for &id in &zones {
        if set.contains(&id) || recreated_ids.contains(&id) {
            continue;
        }

        let Some(z) = board.zone(id) else {
            continue;
        };

        if !fillable(z) {
            continue;
        }

        let layers: Vec<PcbLayerId> = z.layer_set.iter().collect();
        let fill = board.zone_fill_mut(id);

        for layer in layers {
            fill.build_hash_value(layer);
        }
    }

    let reused: Vec<ZoneId> = recreated
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !set.contains(id))
        .collect();

    debug_assert_eq!(reused.len(), reused_from);

    let refill: Vec<ZoneId> = zones.iter().copied().filter(|z| set.contains(z)).collect();
    let mut outcome = fill_after_teardrops(board, connectivity, rules, Some(&refill));
    outcome.reused = reused;
    outcome.reasons = reasons;
    outcome.kept = (zones.len() - outcome.refilled.len() - outcome.reused.len()) as u32;
    outcome
}

/// Steps 2 and 3 of the module comment: the zones to refill, and the number of re-created
/// teardrops outside it; `None` when a re-created teardrop cannot keep its fill and the
/// set has to be recomputed with it as damage (the caller then refills everything, which
/// is always right).
#[allow(clippy::too_many_arguments)]
fn affected_set(
    board: &Board,
    ctx: &BoardCtx<'_>,
    rules: &dyn RuleResolver,
    filler: &ZoneFiller,
    damage: &ZoneDamage,
    zones: &[ZoneId],
    recreated: &[(ZoneId, usize)],
    old_keys: &[(u32, Kiid)],
    near_clusters: &[(Box2I, i32)],
) -> Option<(BTreeSet<ZoneId>, usize, [u32; 6])> {
    let info: Vec<(ZoneId, &Zone, Box2I)> = zones
        .iter()
        .filter_map(|&id| board.zone(id).map(|z| (id, z, z.bounding_box(ctx))))
        .filter(|(_, z, _)| fillable(z))
        .collect();

    let coupled = |a: &Zone, b: &Zone| {
        let shared = &a.layer_set & &b.layer_set;

        shared.any()
            && (filler.zone_knockout_may_interact(ctx, a, b)
                || filler.zone_knockout_may_interact(ctx, b, a))
    };

    let recreated_ids: BTreeSet<ZoneId> = recreated.iter().map(|(id, _)| *id).collect();
    let mut set: BTreeSet<ZoneId> = BTreeSet::new();
    let mut reasons = [0u32; 6];

    for (id, z, bbox) in &info {
        let seeded = if !z.is_teardrop_area() && !z.fill().is_filled {
            reasons[0] += 1;
            true
        } else if !recreated_ids.contains(id) && z.is_teardrop_area() {
            reasons[1] += 1;
            true // a new or changed teardrop (no predecessor to take over from)
        } else if tracks_islands(z) && cluster_nets_near(near_clusters, z, bbox) {
            reasons[2] += 1;
            true
        } else {
            let mut area = *bbox;
            area.inflate(seed_margin(board, rules, filler, z));
            let layers = &z.layer_set;

            let near = damage
                .rects
                .iter()
                .any(|r| (layers & &r.layers).any() && area.intersects(&r.bbox));
            reasons[3] += near as u32;
            near
        };

        if seeded {
            set.insert(*id);
        }
    }

    // A re-created teardrop has a new priority and UUID. `HigherPriority()` decides a
    // teardrop against a regular zone by the type alone, but against another teardrop by
    // priority, then UUID: a re-created teardrop coupled to another re-created one whose
    // relation to it is not the old one cannot take over the old fill. (A coupled teardrop
    // without predecessor is in the set already and pulls it in through the closure.)
    let pred: std::collections::BTreeMap<ZoneId, usize> = recreated.iter().copied().collect();
    let old_higher = |a: usize, b: usize| {
        let (pa, ua) = old_keys[a];
        let (pb, ub) = old_keys[b];
        if pa != pb { pa > pb } else { ua > ub }
    };

    for (id, z, _) in &info {
        let Some(&k) = pred.get(id) else {
            continue;
        };

        let tie = info.iter().any(|(other_id, other, _)| {
            let Some(&m) = pred.get(other_id) else {
                return false;
            };

            other_id != id
                && (z.higher_priority(other) != old_higher(k, m)
                    || other.higher_priority(z) != old_higher(m, k))
                && coupled(z, other)
        });

        if tie && set.insert(*id) {
            reasons[4] += 1;
        }
    }

    // Closure under coupling and shared nets.
    let mut nets: BTreeSet<i32> = BTreeSet::new();

    loop {
        let before = set.len();

        for (id, z, _) in &info {
            if set.contains(id) {
                nets.insert(z.base.net.0);
            }
        }

        for (id, z, _) in &info {
            if set.contains(id) {
                continue;
            }

            // A teardrop is always higher than a regular zone (`HigherPriority()`): it never
            // reads a regular zone's fill, and its own fill is final as filled (no island
            // removal, no iterative refill, never out of the board), so a refilled regular
            // zone reads the same teardrop fill as in the full refill.
            if (tracks_islands(z) && nets.contains(&z.base.net.0))
                || info.iter().any(|(other_id, other, _)| {
                    set.contains(other_id)
                        && !(z.is_teardrop_area() && !other.is_teardrop_area())
                        && coupled(z, other)
                })
            {
                set.insert(*id);
            }
        }

        if set.len() == before {
            break;
        }
    }

    reasons[5] = set.len() as u32 - reasons[..5].iter().sum::<u32>();
    let reused = recreated.iter().filter(|(id, _)| !set.contains(id)).count();
    Some((set, reused, reasons))
}

/// The members (bounding box, net) of every connectivity cluster (`CSM_CONNECTIVITY_CHECK`,
/// zones included) of a net of the damage that has a member whose bounding box meets a
/// damage rectangle: the clusters whose pad membership a change can have altered (a removed
/// item's neighbours touch its area, an added or changed item lies in its own).
fn clusters_near(
    ctx: &BoardCtx<'_>,
    connectivity: &mut ConnectivityData,
    damage: &ZoneDamage,
) -> Vec<(Box2I, i32)> {
    use kicad_connectivity::algo::ClusterSearchMode;

    let clusters = connectivity.algo_mut().search_clusters_ex(
        ctx,
        ClusterSearchMode::ConnectivityCheck,
        false,
        -1,
    );
    let algo = connectivity.algo();
    let list = algo.item_list();
    let mut out = Vec::new();

    for cl in clusters {
        let members: Vec<(Box2I, i32)> = algo
            .cluster(cl)
            .items()
            .iter()
            .map(|&i| list.item(i))
            .filter(|it| it.valid())
            .map(|it| (*it.bbox(), it.net()))
            .collect();

        // CSM_CONNECTIVITY_CHECK clusters hold one net (`withinAnyNet`: a neighbour of
        // another net is skipped): only a change of an item of that net (before or after
        // the change) alters one.
        if members
            .first()
            .is_some_and(|(_, n)| damage.nets.contains(n))
            && members
                .iter()
                .any(|(b, _)| damage.rects.iter().any(|r| r.bbox.intersects(b)))
        {
            out.extend(members);
        }
    }

    out
}

/// Whether a member of [`clusters_near`] has the zone's net and meets its bounding box: only
/// then can the islands of the zone (`FillIsolatedIslandsMap`: a fill outline in a cluster
/// without a pad) change. A fill outline joins a cluster through an item touching it, which
/// meets the zone's bounding box; removed islands of other zones that touch it make those
/// zones coupled (step 3).
fn cluster_nets_near(near: &[(Box2I, i32)], zone: &Zone, bbox: &Box2I) -> bool {
    let net = zone.base.net.0;
    let mut area = *bbox;
    area.inflate(1);

    near.iter().any(|(b, n)| *n == net && area.intersects(b))
}

/// `FillAllZones` from the teardrop update on: the whole refill.
fn full_refill(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    rules: &dyn RuleResolver,
) -> AffectedFillOutcome {
    let outcome = crate::fill_all_zones(board, connectivity, rules);
    let refilled: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    AffectedFillOutcome {
        filled: outcome.filled,
        warnings: outcome.warnings,
        full: true,
        refilled,
        reused: Vec::new(),
        kept: 0,
        reasons: [0; 6],
    }
}

/// The part of `FillAllZones` after `UpdateTeardrops` and `IncrementTimeStamp`:
/// `ZONE_FILLER::Fill( toFill )` and `PostFillRefresh` (`BuildConnectivity`). `None` fills
/// every board zone (the full refill).
fn fill_after_teardrops(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    rules: &dyn RuleResolver,
    zones: Option<&[ZoneId]>,
) -> AffectedFillOutcome {
    let full = zones.is_none();
    let to_fill: Vec<ZoneId> = match zones {
        Some(z) => z.to_vec(),
        None => board.zones().map(|(id, _)| id).collect(),
    };

    let mut filler = ZoneFiller::new(board);
    let filled = filler.fill(board, connectivity, rules, &to_fill);
    let warnings = filler.warnings().to_vec();

    // PostFillRefresh( aHeadless ) -> board()->BuildConnectivity()
    connectivity.build_connectivity(board);

    let total = board.zones().count();

    AffectedFillOutcome {
        filled,
        warnings,
        full,
        kept: (total - to_fill.len()) as u32,
        refilled: to_fill,
        reused: Vec::new(),
        reasons: [0; 6],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_merge_and_full() {
        let mut d = ZoneDamage::default();
        assert!(d.is_empty());
        d.add_area(
            Box2I::new(
                kicad_geom::Vector2I::new(0, 0),
                kicad_geom::Vector2I::new(10, 10),
            ),
            Lset::from_layers(&[PcbLayerId::FCu]),
            Some(3),
        );
        assert!(!d.full && d.nets.contains(&3) && d.rects.len() == 1);

        let mut e = ZoneDamage::default();
        e.add_area(
            Box2I::new(
                kicad_geom::Vector2I::new(0, 0),
                kicad_geom::Vector2I::new(1, 1),
            ),
            Lset::from_layers(&[PcbLayerId::EdgeCuts]),
            None,
        );
        assert!(e.full);
        d.merge(&e);
        assert!(d.full && d.rects.is_empty());
    }
}
