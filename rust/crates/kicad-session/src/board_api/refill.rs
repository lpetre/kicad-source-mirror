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

//! Affected-only zone refill for the router loop (package LIVE-2).
//!
//! * [`ChangeDamage`]: the serialisable form of `kicad_zone::affected::ZoneDamage`, carried
//!   by every [`super::commit::CommitReport`] (the areas, layers and nets of the items the
//!   commit removed, changed, added or re-netted, as `BOARD_COMMIT::propagateDamage` sees
//!   them). Merge the reports of all commits since the last refill.
//! * [`FillTracking`]: the session's own record of the damage since the last refill, kept by
//!   the operations that report their damage and checked against the board time stamp and
//!   the rule text, so that the precondition of the identity (the fills before the damage are
//!   those of a refill) does not rest on the caller.
//! * [`Session::refill_zones_affected`]: `FillAllZones` with `ZONE_FILLER::Fill` on the zones
//!   the recorded damage can affect (`kicad_zone::affected::fill_affected_zones`), identical
//!   to [`Session::refill_zones`]; a full refill whenever the record is unknown (after a load
//!   or any untracked change) or a change has effects no damage area bounds (removal of a pad
//!   or zone, a changed worst clearance, `fromTo()` rules).

use kicad_core::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::{Box2I, Vector2I};
use kicad_pcb::ItemId;
use kicad_pcb::ctx::BoardCtx;
use kicad_zone::affected::{DamageRect, ZoneDamage, fill_affected_zones};
use serde::{Deserialize, Serialize};

use super::handle;
use crate::error::SessionError;
use crate::session::Session;

/// A changed area: `[x, y, w, h]` and the layer ids (`PCB_LAYER_ID` numbers).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageArea {
    /// The item's `GetBoundingBox()` as `[x, y, w, h]`.
    pub bbox: [i32; 4],
    /// The item's `GetLayerSet()` as layer ids.
    pub layers: Vec<i32>,
}

/// What changed since the fills were last those of a full refill.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChangeDamage {
    /// Unknown or global changes: the next refill is a full one.
    pub full: bool,
    /// The changed areas.
    pub areas: Vec<DamageArea>,
    /// The nets of the changed items, before and after the change, ascending.
    pub nets: Vec<i32>,
}

impl ChangeDamage {
    /// Forces a full refill (the start state of a loaded board).
    pub fn full() -> Self {
        ChangeDamage {
            full: true,
            ..ChangeDamage::default()
        }
    }

    /// Adds the damage of a later commit.
    pub fn merge(&mut self, other: &ChangeDamage) {
        self.full |= other.full;

        if self.full {
            self.areas.clear();
            self.nets.clear();
            return;
        }

        self.areas.extend(other.areas.iter().cloned());

        for &n in &other.nets {
            if let Err(pos) = self.nets.binary_search(&n) {
                self.nets.insert(pos, n);
            }
        }
    }

    /// Nothing to refill for.
    pub fn is_empty(&self) -> bool {
        !self.full && self.areas.is_empty() && self.nets.is_empty()
    }

    /// The filler's form.
    pub fn to_zone_damage(&self) -> ZoneDamage {
        let mut d = ZoneDamage {
            full: self.full,
            ..ZoneDamage::default()
        };

        for a in &self.areas {
            let layers: Vec<PcbLayerId> = a.layers.iter().map(|&l| PcbLayerId::from(l)).collect();
            d.rects.push(DamageRect {
                bbox: Box2I::new(
                    Vector2I::new(a.bbox[0], a.bbox[1]),
                    Vector2I::new(a.bbox[2], a.bbox[3]),
                ),
                layers: Lset::from_layers(&layers),
            });
        }

        d.nets.extend(self.nets.iter().copied());
        d
    }

    /// Records an item of the board as changed (before or after the change): track, arc,
    /// via, pad and zone areas; any other item (footprint, graphic, board outline) makes
    /// the damage full.
    pub(crate) fn record_item(&mut self, ctx: &BoardCtx<'_>, id: ItemId) {
        if self.full {
            return;
        }

        let Some(item) = ctx.item(id) else {
            self.full = true;
            return;
        };

        match item.type_() {
            KicadT::PcbTraceT
            | KicadT::PcbArcT
            | KicadT::PcbViaT
            | KicadT::PcbPadT
            | KicadT::PcbZoneT => {}
            _ => {
                self.full = true;
                return;
            }
        }

        let bbox = item.bounding_box();
        let layers = item.layer_set();

        if layers.contains(PcbLayerId::EdgeCuts) || layers.contains(PcbLayerId::Margin) {
            self.full = true;
            return;
        }

        self.areas.push(DamageArea {
            bbox: [
                bbox.x(),
                bbox.y(),
                i32::try_from(bbox.width()).unwrap_or(i32::MAX),
                i32::try_from(bbox.height()).unwrap_or(i32::MAX),
            ],
            layers: layers.seq().iter().map(|l| l.0).collect(),
        });

        if let Some(net) = item.net_code() {
            self.record_net(net.0);
        }
    }

    /// Records an item a commit removes: as [`ChangeDamage::record_item`], except that the
    /// removal of a pad or a zone (rule areas and teardrops included) makes the damage full.
    /// Those items set inputs of every fill that the damage area does not bound: the board's
    /// worst clearance (`GetMaxClearanceValue`: zone and pad local clearances), which is the
    /// reach of every knockout query of `ZONE_FILLER::Fill`; the copper layers a pad hole
    /// knocks out (an NPTH pad on the mask layers only knocks its hole out of every copper
    /// layer); the clearances of items anywhere that a custom rule gives by area membership
    /// (`intersectsArea` / `insideArea` / `enclosedByArea`).
    pub(crate) fn record_removed(&mut self, ctx: &BoardCtx<'_>, id: ItemId) {
        if let Some(item) = ctx.item(id) {
            if matches!(item.type_(), KicadT::PcbPadT | KicadT::PcbZoneT) {
                self.full = true;
                self.areas.clear();
                self.nets.clear();
                return;
            }
        }

        self.record_item(ctx, id);
    }

    /// Records a net whose connectivity changed.
    pub(crate) fn record_net(&mut self, net: i32) {
        if let Err(pos) = self.nets.binary_search(&net) {
            self.nets.insert(pos, net);
        }
    }
}

/// What [`Session::refill_zones_affected`] returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefillReport {
    /// The damage was full (or could not be bounded): every zone was refilled.
    pub full: bool,
    /// Handles of the zones that were refilled, in board order.
    pub refilled: Vec<u64>,
    /// Handles of the re-created teardrops that took over the fill of their predecessor.
    pub reused: Vec<u64>,
    /// The number of board zones whose fill was kept.
    pub kept: u32,
    /// Why zones were refilled: `[not filled, new or changed teardrop, net of a change, near
    /// a change, re-created teardrop tied with a coupled zone, closure]`.
    pub reasons: [u32; 6],
    /// Nets the refill's connectivity builds gave to items (`PropagateNets`): damage for
    /// the next affected refill.
    #[serde(default, skip_serializing_if = "ChangeDamage::is_empty")]
    pub damage: ChangeDamage,
}

/// The nets of the board's tracks, arcs, vias and zones (the items whose net a connectivity
/// build can change), for [`net_damage`].
pub(crate) fn net_snapshot(board: &kicad_pcb::Board) -> Vec<(ItemId, i32)> {
    let mut v: Vec<(ItemId, i32)> = board
        .tracks()
        .map(|(id, t)| (ItemId::from(id), t.as_track().base.net.0))
        .collect();
    v.extend(
        board
            .zones()
            .map(|(id, z)| (ItemId::from(id), z.base.net.0)),
    );
    v
}

/// The items whose net differs from `before` ([`net_snapshot`]) as damage: the item's area
/// with its old and new nets.
pub(crate) fn net_damage(board: &kicad_pcb::Board, before: &[(ItemId, i32)]) -> ChangeDamage {
    let after: std::collections::HashMap<ItemId, i32> = net_snapshot(board).into_iter().collect();
    let ctx = BoardCtx::bare(board);
    let mut d = ChangeDamage::default();

    for &(id, net) in before {
        if after.get(&id).is_some_and(|&n| n != net) {
            d.record_net(net);
            d.record_item(&ctx, id);
        }
    }

    d
}

/// The session's record of what changed since the fills were last those of a refill
/// (package LIVE-2), the precondition of [`Session::refill_zones_affected`]. It is kept by the
/// operations that report their damage ([`Session::commit`], [`Session::quick_drc`] /
/// [`Session::check_items`], [`Session::finalize`], the refills) and saved and restored with
/// a [`super::commit::Checkpoint`]. It is valid only while the board time stamp and the rule
/// text are the ones the last of those operations left: any other change of the board (every
/// `Board` mutator bumps the time stamp: `apply_edits`, `add_track`, `update_teardrops`,
/// `run_drc`, design settings, a direct edit of `Session::board`, ...) or of
/// `Session::rules_text` makes it unknown, and the next affected refill is a full one. A
/// loaded board starts unknown (its fills come from the file).
#[derive(Clone, Debug, Default)]
pub struct FillTracking {
    known: Option<KnownFills>,
}

#[derive(Clone, Debug)]
struct KnownFills {
    /// `BOARD::GetTimeStamp()` after the last tracked operation.
    stamp: i32,
    /// Fingerprint of `Session::rules_text` at the last refill.
    rules: u64,
    /// `GetMaxClearanceValue()` at the end of the last refill.
    worst_clearance: i32,
    /// The merged damage since the last refill.
    damage: ChangeDamage,
}

/// A fingerprint of the custom rule text (the rules the engine compiles).
fn rules_fingerprint(rules: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut h = std::collections::hash_map::DefaultHasher::new();
    rules.hash(&mut h);
    h.finish()
}

impl Session {
    /// Whether [`FillTracking`] describes the board as it is now.
    pub(crate) fn fill_tracking_valid(&self) -> bool {
        self.fill_tracking.known.as_ref().is_some_and(|k| {
            k.stamp == self.board.time_stamp()
                && k.rules == rules_fingerprint(self.rules_text.as_deref())
        })
    }

    /// Adds the damage of a tracked operation that started with the tracking `was_valid`
    /// ([`Session::fill_tracking_valid`] before it); otherwise the tracking becomes unknown.
    pub(crate) fn fill_tracking_advance(&mut self, was_valid: bool, damage: &ChangeDamage) {
        let stamp = self.board.time_stamp();

        match (&mut self.fill_tracking.known, was_valid) {
            (Some(k), true) => {
                k.damage.merge(damage);
                k.stamp = stamp;
            }
            _ => self.fill_tracking.known = None,
        }
    }

    /// Records the fills as those of a refill; `damage` is what the refill's own
    /// connectivity builds changed after it filled.
    fn fill_tracking_reset(&mut self, damage: ChangeDamage) {
        let worst_clearance = match self.drc_engine.as_ref() {
            Some(engine) => self
                .board
                .get_max_clearance_value(&BoardCtx::bare(&self.board).with_rules(engine)),
            None => {
                self.fill_tracking.known = None;
                return;
            }
        };

        self.fill_tracking.known = Some(KnownFills {
            stamp: self.board.time_stamp(),
            rules: rules_fingerprint(self.rules_text.as_deref()),
            worst_clearance,
            damage,
        });
    }

    /// The damage the next [`Session::refill_zones_affected`] refills for: the merged damage
    /// of the tracked operations since the last refill, or [`ChangeDamage::full`] when the
    /// fills are not known to be those of a refill of the board as it is.
    pub fn fill_damage(&self) -> ChangeDamage {
        match &self.fill_tracking.known {
            Some(k) if self.fill_tracking_valid() => k.damage.clone(),
            _ => ChangeDamage::full(),
        }
    }

    /// The session's tracking state, for [`super::commit::Checkpoint`].
    pub(crate) fn fill_tracking_state(&self) -> FillTracking {
        self.fill_tracking.clone()
    }

    /// Puts the tracking state of a checkpoint back.
    pub(crate) fn set_fill_tracking_state(&mut self, state: FillTracking) {
        self.fill_tracking = state;
    }

    /// [`Session::finalize`] returning the items whose net the full rebuild changed (also
    /// recorded by the session for the next [`Session::refill_zones_affected`]).
    pub fn finalize_tracked(&mut self) -> ChangeDamage {
        let valid = self.fill_tracking_valid();
        let before = net_snapshot(&self.board);
        self.rebuild_connectivity();
        let damage = net_damage(&self.board, &before);
        self.fill_tracking_advance(valid, &damage);
        damage
    }

    /// The full refill [`Session::refill_zones`] with the session's tracking: afterwards the
    /// fills are known to be those of a refill.
    pub(crate) fn refill_zones_tracked(&mut self) -> Result<(), SessionError> {
        let before = net_snapshot(&self.board);
        let outcome = self.fill_all_zones()?;
        self.log_warnings.extend(outcome.warnings);
        let damage = net_damage(&self.board, &before);
        self.fill_tracking_reset(damage);
        Ok(())
    }

    /// `FillAllZones` refilling only the zones the changes since the last refill can affect;
    /// the fills, flags, teardrops and connectivity end up as [`Session::refill_zones`]
    /// leaves them.
    ///
    /// The changes are those the session recorded ([`Session::fill_damage`]: the damage of
    /// every commit, DRC run and `finalize` since the last refill, restored with
    /// checkpoints) plus `extra` (damage the caller knows of beyond that; pass
    /// `ChangeDamage::default()` when there is none, `ChangeDamage::full()` to force a full
    /// refill). The refill is a full one whenever the session cannot vouch for the fills:
    /// after a load (fills from the file), after any change of the board or of the rule text
    /// by another operation, when the board's worst clearance differs from the one of the last
    /// refill, and when the custom rules use `fromTo()` (its from-to cache is rebuilt by
    /// every DRC run and changes the clearance of items no damage area bounds).
    pub fn refill_zones_affected(
        &mut self,
        extra: &ChangeDamage,
    ) -> Result<RefillReport, SessionError> {
        let mut damage = self.fill_damage();
        self.ensure_drc_engine();

        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported("DRC engine"));
        };

        if !damage.full {
            let worst = self
                .board
                .get_max_clearance_value(&BoardCtx::bare(&self.board).with_rules(engine));

            if self
                .fill_tracking
                .known
                .as_ref()
                .is_none_or(|k| k.worst_clearance != worst)
            {
                damage = ChangeDamage::full();
            }
        }

        if self
            .rules_text
            .as_deref()
            // function names are case-insensitive in rule conditions (PCBEXPR lowers them)
            .is_some_and(|t| t.to_ascii_lowercase().contains("fromto"))
        {
            damage = ChangeDamage::full();
        }

        damage.merge(extra);

        let before = net_snapshot(&self.board);
        let outcome = fill_affected_zones(
            &mut self.board,
            &mut self.connectivity,
            engine,
            &damage.to_zone_damage(),
        );
        self.log_warnings.extend(outcome.warnings);
        let after = net_damage(&self.board, &before);
        self.fill_tracking_reset(after.clone());

        Ok(RefillReport {
            full: outcome.full,
            refilled: outcome.refilled.into_iter().map(handle).collect(),
            reused: outcome.reused.into_iter().map(handle).collect(),
            kept: outcome.kept,
            reasons: outcome.reasons,
            damage: after,
        })
    }
}
