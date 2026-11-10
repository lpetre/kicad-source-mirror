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
//! 2. Seeds: every fillable zone that shares a copper layer with a damage rectangle and whose
//!    bounding box, inflated by the reach of [`seed_margin`], meets it; every zone of a net
//!    that a changed item had before or after the change (island removal reads the net's
//!    connectivity); every zone that is not filled (`ZoneFillDirty`'s `!IsFilled()`). Vias
//!    and pads whose bounding box meets a damage rectangle are damage themselves (their
//!    conditional flashing reads the connectivity at their position).
//! 3. Closure: a zone is added while it is *coupled* to a zone of the set (a shared layer and
//!    `zoneKnockoutMayInteract` in either direction: the gate of every read of another zone's
//!    fill, of the fill DAG and of the iterative refill) or has the net of a zone of the set
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
use kicad_core::lset::Lset;
use kicad_core::PcbLayerId;
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
}

/// The settings of a teardrop zone that its fill and its fill relations depend on: every
/// field of `ZONE` except the UUID, the handle, the fill results and the derived display
/// caches.
fn teardrop_signature(z: &Zone) -> String {
    format!(
        "{:?}",
        (
            (
                z.library_outline(),
                &z.layer_set,
                z.base.net,
                &z.base.teardrop_params,
                z.base.item.layer,
                z.base.item.is_locked,
                z.base.item.is_knockout,
                z.priority,
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
    )
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
            worst = worst.max(con.value().max());
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
    let old_teardrops: Vec<(String, ZoneFill, Box2I, Lset, i32)> = {
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
                )
            })
            .collect()
    };

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
            let found = old_teardrops
                .iter()
                .enumerate()
                .position(|(k, old)| !matched[k] && old.1.is_filled && old.0 == sig);

            match found {
                Some(k) => {
                    matched[k] = true;
                    recreated.push((id, k));
                }
                None => damage.add_area(z.bounding_box(&ctx), z.layer_set.clone(), Some(z.base.net.0)),
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

    let mut filler = ZoneFiller::new(board);
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

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

        affected_set(board, &ctx, rules, &filler, &damage, &zones, &recreated)
    };

    let Some((set, reused_from)) = set else {
        return fill_after_teardrops(board, connectivity, rules, None);
    };

    // Re-created teardrops outside the set take over their predecessor's fill.
    let mut reused = Vec::new();

    for (id, k) in &recreated {
        if !set.contains(id) {
            *board.zone_fill_mut(*id) = old_teardrops[*k].1.clone();
            reused.push(*id);
        }
    }

    debug_assert_eq!(reused.len(), reused_from);

    let refill: Vec<ZoneId> = zones.iter().copied().filter(|z| set.contains(z)).collect();
    let mut outcome = fill_after_teardrops(board, connectivity, rules, Some(&refill));
    outcome.reused = reused;
    outcome.kept = (zones.len() - outcome.refilled.len() - outcome.reused.len()) as u32;
    outcome
}

/// Steps 2 and 3 of the module comment: the zones to refill, and the number of re-created
/// teardrops outside it; `None` when a re-created teardrop cannot keep its fill and the
/// set has to be recomputed with it as damage (the caller then refills everything, which
/// is always right).
fn affected_set(
    board: &Board,
    ctx: &BoardCtx<'_>,
    rules: &dyn RuleResolver,
    filler: &ZoneFiller,
    damage: &ZoneDamage,
    zones: &[ZoneId],
    recreated: &[(ZoneId, usize)],
) -> Option<(BTreeSet<ZoneId>, usize)> {
    let cu = Lset::all_cu_mask();
    let info: Vec<(ZoneId, &Zone, Box2I)> = zones
        .iter()
        .filter_map(|&id| board.zone(id).map(|z| (id, z, z.bounding_box(ctx))))
        .filter(|(_, z, _)| fillable(z))
        .collect();

    let coupled = |a: &Zone, b: &Zone| {
        let shared = &(&a.layer_set & &b.layer_set) & cu;

        shared.any()
            && (filler.zone_knockout_may_interact(ctx, a, b)
                || filler.zone_knockout_may_interact(ctx, b, a))
    };

    let recreated_ids: BTreeSet<ZoneId> = recreated.iter().map(|(id, _)| *id).collect();
    let mut set: BTreeSet<ZoneId> = BTreeSet::new();

    for (id, z, bbox) in &info {
        let seeded = if !z.is_teardrop_area() && !z.fill().is_filled {
            true
        } else if !recreated_ids.contains(id) && z.is_teardrop_area() {
            true // a new or changed teardrop (no predecessor to take over from)
        } else if damage.nets.contains(&z.base.net.0) {
            true
        } else {
            let mut area = *bbox;
            area.inflate(seed_margin(board, rules, filler, z));
            let layers = &z.layer_set & cu;

            damage
                .rects
                .iter()
                .any(|r| (&layers & &r.layers).any() && area.intersects(&r.bbox))
        };

        if seeded {
            set.insert(*id);
        }
    }

    // A re-created teardrop whose relation to an equal-priority coupled zone was decided by
    // its old UUID: it cannot take over the old fill.
    for (id, z, _) in &info {
        if !recreated_ids.contains(id) {
            continue;
        }

        let tie = info.iter().any(|(other_id, other, _)| {
            other_id != id
                && other.priority == z.priority
                && other.is_teardrop_area() == z.is_teardrop_area()
                && coupled(z, other)
        });

        if tie {
            set.insert(*id);
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

            if nets.contains(&z.base.net.0)
                || info
                    .iter()
                    .any(|(other_id, other, _)| set.contains(other_id) && coupled(z, other))
            {
                set.insert(*id);
            }
        }

        if set.len() == before {
            break;
        }
    }

    let reused = recreated.iter().filter(|(id, _)| !set.contains(id)).count();
    Some((set, reused))
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
            Box2I::new(kicad_geom::Vector2I::new(0, 0), kicad_geom::Vector2I::new(10, 10)),
            Lset::from_layers(&[PcbLayerId::FCu]),
            Some(3),
        );
        assert!(!d.full && d.nets.contains(&3) && d.rects.len() == 1);

        let mut e = ZoneDamage::default();
        e.add_area(
            Box2I::new(kicad_geom::Vector2I::new(0, 0), kicad_geom::Vector2I::new(1, 1)),
            Lset::from_layers(&[PcbLayerId::EdgeCuts]),
            None,
        );
        assert!(e.full);
        d.merge(&e);
        assert!(d.full && d.rects.is_empty());
    }
}
