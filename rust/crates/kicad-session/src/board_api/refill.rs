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
//! * [`Session::refill_zones_affected`]: `FillAllZones` with `ZONE_FILLER::Fill` on the zones
//!   the damage can affect (`kicad_zone::affected::fill_affected_zones`), identical to
//!   [`Session::refill_zones`] provided the fills before the damage were those of a full
//!   refill (the board after a full or affected refill; not a freshly loaded board, whose
//!   fills come from the file: start with [`ChangeDamage::full`]).

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
                bbox.width(),
                bbox.height(),
            ],
            layers: layers.seq().iter().map(|l| l.0).collect(),
        });

        if let Some(net) = item.net_code() {
            self.record_net(net.0);
        }
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
}

impl Session {
    /// `FillAllZones` refilling only the zones `damage` (the merged
    /// [`super::commit::CommitReport::damage`] of every commit since the last refill) can
    /// affect; the fills, flags, teardrops and connectivity end up as [`Session::refill_zones`]
    /// leaves them (see the module comment for the precondition).
    pub fn refill_zones_affected(
        &mut self,
        damage: &ChangeDamage,
    ) -> Result<RefillReport, SessionError> {
        self.ensure_drc_engine();

        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported("DRC engine"));
        };

        let outcome = fill_affected_zones(
            &mut self.board,
            &mut self.connectivity,
            engine,
            &damage.to_zone_damage(),
        );
        self.log_warnings.extend(outcome.warnings);

        Ok(RefillReport {
            full: outcome.full,
            refilled: outcome.refilled.into_iter().map(handle).collect(),
            reused: outcome.reused.into_iter().map(handle).collect(),
            kept: outcome.kept,
        })
    }
}
