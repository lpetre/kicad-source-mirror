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

//! The edit half of the router API. Every edit goes through the model's mutation API (the
//! item constructors, `BOARD::Add( item, ADD_MODE::APPEND )`, `Board::modify_*` with the
//! setters `SetStart` / `SetEnd` / `SetMid` / `SetPosition` / `SetWidth` / `SetLayer` /
//! `SetNetCode` / `SetLocked`, `BOARD::Remove`); the connectivity is updated once per call
//! through the transaction layer ([`super::commit`], the port of
//! `BOARD_COMMIT::Push`: `CONNECTIVITY_DATA::Add / Remove / Update`, then
//! `RecalculateRatsnest` of the dirty nets). `apply_edits` runs any number of edits as one
//! transaction.

use std::collections::BTreeSet;

use kicad_core::{Kiid, PcbLayerId};
use kicad_pcb::padstack::Padstack;
use kicad_pcb::pcb_track::{PcbArc, PcbTrack, PcbVia, Track, Viatype};
use kicad_pcb::teardrop_parameters::{TargetTd, TeardropParameters};
use kicad_pcb::traits::BoardItemMut;
use kicad_pcb::{AddMode, AnyItemMut, ItemId, NetCode, ParentRef};
use serde::{Deserialize, Serialize};

use super::{Pt, handle, item_id, parse_layer, vec2};
use crate::error::SessionError;
use crate::session::Session;

/// A new track segment. `width` defaults to the router's width for the net and layer
/// ([`Session::router_sizes`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewTrack {
    /// `SetStart`.
    pub start: Pt,
    /// `SetEnd`.
    pub end: Pt,
    /// `SetWidth`.
    pub width: Option<i32>,
    /// `SetLayer` (a copper layer).
    pub layer: String,
    /// `SetNetCode`.
    pub net: i32,
    /// The UUID to give the item (a fresh one when absent).
    pub uuid: Option<String>,
    /// `SetLocked`.
    pub locked: bool,
    /// `SetHasSolderMask`.
    pub solder_mask: bool,
    /// `SetLocalSolderMaskMargin`.
    pub solder_mask_margin: Option<i32>,
}

/// A new arc track (`PCB_ARC`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewArc {
    /// `SetStart`.
    pub start: Pt,
    /// `SetMid`.
    pub mid: Pt,
    /// `SetEnd`.
    pub end: Pt,
    /// `SetWidth`.
    pub width: Option<i32>,
    /// `SetLayer`.
    pub layer: String,
    /// `SetNetCode`.
    pub net: i32,
    /// The UUID to give the item.
    pub uuid: Option<String>,
    /// `SetLocked`.
    pub locked: bool,
    /// `SetHasSolderMask`.
    pub solder_mask: bool,
    /// `SetLocalSolderMaskMargin`.
    pub solder_mask_margin: Option<i32>,
}

/// A new via. `diameter` / `drill` default to the router's sizes (micro via sizes for
/// `micro`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewVia {
    /// `SetPosition`.
    pub at: Pt,
    /// `through` (default), `blind`, `buried`, `micro`.
    pub via_type: Option<String>,
    /// Top layer of the span (default `F.Cu`).
    pub top: Option<String>,
    /// Bottom layer of the span (default `B.Cu`).
    pub bottom: Option<String>,
    /// `SetWidth( d, PADSTACK::ALL_LAYERS )`.
    pub diameter: Option<i32>,
    /// `SetDrill`.
    pub drill: Option<i32>,
    /// `SetNetCode`.
    pub net: i32,
    /// `SetIsFree`.
    pub free: bool,
    /// The UUID to give the item.
    pub uuid: Option<String>,
    /// `SetLocked`.
    pub locked: bool,
    /// The mask / hole treatment modes (default: all `from_board`, as `new PCB_VIA`).
    pub finish: Option<super::read::ViaFinish>,
    /// `Padstack().SetUnconnectedLayerMode` (`KEEP_ALL` default, `START_END_ONLY`,
    /// `REMOVE_ALL`, `REMOVE_EXCEPT_START_AND_END`).
    pub unconnected_layers: Option<String>,
    /// `SetZoneLayerOverride( layer, ZLO_FORCE_FLASHED )` for these layers (zone filler
    /// state; a refill recomputes it).
    pub zone_layer_connections: Vec<String>,
    /// `GetTeardropParams() = ...` (default: the `TEARDROP_PARAMETERS` defaults).
    pub teardrops: Option<TeardropParams>,
}

/// A change of one existing track / arc / via; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemChange {
    /// The item handle.
    pub id: u64,
    /// `SetStart` (a via: `SetPosition`).
    pub start: Option<Pt>,
    /// `SetEnd`.
    pub end: Option<Pt>,
    /// `SetMid` (arcs).
    pub mid: Option<Pt>,
    /// `SetWidth` (a via: `SetWidth( w, ALL_LAYERS )`).
    pub width: Option<i32>,
    /// `SetDrill` (vias).
    pub drill: Option<i32>,
    /// `SetLayer` (a via: `SetLayerPair( layer, bottom )`).
    pub layer: Option<String>,
    /// Via bottom layer (`SetLayerPair( top, bottom )`).
    pub bottom: Option<String>,
    /// `SetNetCode`.
    pub net: Option<i32>,
    /// `SetLocked`.
    pub locked: Option<bool>,
    /// `SetIsFree` (vias).
    pub free: Option<bool>,
}

/// Remove the routing (tracks, arcs and vias) of some nets, or of the whole board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RemoveRouting {
    /// The nets; empty = every net.
    pub nets: Vec<i32>,
    /// Keep locked items.
    pub keep_locked: bool,
    /// Also remove vias (default true).
    pub vias: Option<bool>,
}

/// A batch of edits, applied in the order removals, changes, additions,
/// with one connectivity rebuild.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EditBatch {
    /// Handles to remove.
    pub remove: Vec<u64>,
    /// Routing to remove.
    pub remove_routing: Option<RemoveRouting>,
    /// Changes of existing items.
    pub change: Vec<ItemChange>,
    /// New items, added in this order.
    pub add: Vec<NewItem>,
}

/// A new item of an [`EditBatch`]: `{ "kind": "track" | "arc" | "via", ... }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NewItem {
    /// A track segment.
    Track(NewTrack),
    /// An arc.
    Arc(NewArc),
    /// A via.
    Via(NewVia),
}

/// What an edit returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditResult {
    /// Handles of the new items, in the order of the request.
    pub added: Vec<u64>,
    /// UUIDs of the new items, same order.
    pub added_uuids: Vec<String>,
    /// Handles of the removed items.
    pub removed: Vec<u64>,
    /// Handles of the changed items.
    pub changed: Vec<u64>,
}

/// Board teardrop settings (`BOARD_DESIGN_SETTINGS::m_TeardropParamsList`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeardropSettings {
    /// `m_TargetVias`.
    pub target_vias: bool,
    /// `m_TargetPTHPads`.
    pub target_pth_pads: bool,
    /// `m_TargetSMDPads`.
    pub target_smd_pads: bool,
    /// `m_UseRoundShapesOnly`.
    pub use_round_shapes_only: bool,
    /// The parameters for round shapes (`TARGET_ROUND`).
    pub round: TeardropParams,
    /// The parameters for rectangular shapes (`TARGET_RECT`).
    pub rect: TeardropParams,
    /// The parameters for track to track (`TARGET_TRACK`).
    pub track: TeardropParams,
}

impl Default for TeardropSettings {
    fn default() -> Self {
        let p = TeardropParams::of(&TeardropParameters::default());
        TeardropSettings {
            target_vias: true,
            target_pth_pads: true,
            target_smd_pads: true,
            use_round_shapes_only: false,
            round: p.clone(),
            rect: p.clone(),
            track: p,
        }
    }
}

/// `TEARDROP_PARAMETERS`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeardropParams {
    /// `m_Enabled`.
    pub enabled: bool,
    /// `m_TdMaxLen`.
    pub max_len: i32,
    /// `m_TdMaxWidth`.
    pub max_width: i32,
    /// `m_BestLengthRatio`.
    pub best_length_ratio: f64,
    /// `m_BestWidthRatio`.
    pub best_width_ratio: f64,
    /// `m_WidthtoSizeFilterRatio`.
    pub width_to_size_filter_ratio: f64,
    /// `m_CurvedEdges`.
    pub curved_edges: bool,
    /// `m_AllowUseTwoTracks`.
    pub allow_two_tracks: bool,
    /// `m_TdOnPadsInZones`.
    pub on_pads_in_zones: bool,
}

impl TeardropParams {
    /// The values of a `TEARDROP_PARAMETERS`.
    pub fn of(p: &TeardropParameters) -> TeardropParams {
        TeardropParams {
            enabled: p.enabled,
            max_len: p.td_max_len,
            max_width: p.td_max_width,
            best_length_ratio: p.best_length_ratio,
            best_width_ratio: p.best_width_ratio,
            width_to_size_filter_ratio: p.width_to_size_filter_ratio,
            curved_edges: p.curved_edges,
            allow_two_tracks: p.allow_use_two_tracks,
            on_pads_in_zones: p.td_on_pads_in_zones,
        }
    }

    /// Store into a `TEARDROP_PARAMETERS`.
    pub fn store(&self, p: &mut TeardropParameters) {
        p.enabled = self.enabled;
        p.td_max_len = self.max_len;
        p.td_max_width = self.max_width;
        p.best_length_ratio = self.best_length_ratio;
        p.best_width_ratio = self.best_width_ratio;
        p.width_to_size_filter_ratio = self.width_to_size_filter_ratio;
        p.curved_edges = self.curved_edges;
        p.allow_use_two_tracks = self.allow_two_tracks;
        p.td_on_pads_in_zones = self.on_pads_in_zones;
    }
}

/// What [`Session::update_teardrops`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardropOutcome {
    /// The number of teardrop zones on the board after the update.
    pub teardrops: u32,
    /// Pads and vias with teardrops enabled (and of a targeted kind) that have connected
    /// tracks: the candidates.
    pub candidates: u32,
    /// The candidates for which no teardrop was created (no teardrop zone with the UUID
    /// `TEARDROP_MANAGER::teardropUuid( track, item, 0 )` for any connected track).
    pub without: Vec<u64>,
}

fn new_uuid(session: &Session, uuid: &Option<String>) -> Result<Option<Kiid>, SessionError> {
    let Some(text) = uuid else { return Ok(None) };
    let Some(kiid) = Kiid::parse(text) else {
        return Err(SessionError::invalid_argument(format!(
            "invalid UUID '{text}'"
        )));
    };

    if session.board.resolve_item(&kiid).is_some() || kiid == session.board.uuid() {
        return Err(SessionError::invalid_argument(format!(
            "UUID {text} is already in use"
        )));
    }

    Ok(Some(kiid))
}

pub(super) fn copper_layer(name: &str) -> Result<PcbLayerId, SessionError> {
    let layer = parse_layer(name)?;

    if !kicad_core::layer_ids::is_copper_layer(layer.as_i32()) {
        // BOARD::Add refuses it with the same message.
        return Err(SessionError::invalid_argument(
            "Cannot place Track on non-copper layer",
        ));
    }

    Ok(layer)
}

fn via_type(name: Option<&str>) -> Result<Viatype, SessionError> {
    Ok(match name.unwrap_or("through") {
        "through" => Viatype::Through,
        "blind" => Viatype::Blind,
        "buried" => Viatype::Buried,
        "micro" => Viatype::Microvia,
        other => {
            return Err(SessionError::invalid_argument(format!(
                "unknown via type '{other}'"
            )));
        }
    })
}

fn mode_value(name: &str) -> Result<i32, SessionError> {
    match name {
        "from_board" => Ok(0),
        "yes" => Ok(1),
        "no" => Ok(2),
        other => Err(SessionError::invalid_argument(format!(
            "invalid via mode '{other}' (from_board, yes, no)"
        ))),
    }
}

/// `SetFrontTentingMode` ... `SetFillingMode` from a [`super::read::ViaFinish`].
fn apply_finish(v: &mut PcbVia, f: &super::read::ViaFinish) -> Result<(), SessionError> {
    use kicad_pcb::pcb_track::{CappingMode, CoveringMode, FillingMode, PluggingMode, TentingMode};

    macro_rules! mode {
        ($ty:ident, $name:expr, $a:ident, $b:ident) => {
            match mode_value($name)? {
                1 => $ty::$a,
                2 => $ty::$b,
                _ => $ty::FromBoard,
            }
        };
    }

    v.set_front_tenting_mode(mode!(TentingMode, &f.tenting[0], Tented, NotTented));
    v.set_back_tenting_mode(mode!(TentingMode, &f.tenting[1], Tented, NotTented));
    v.set_front_covering_mode(mode!(CoveringMode, &f.covering[0], Covered, NotCovered));
    v.set_back_covering_mode(mode!(CoveringMode, &f.covering[1], Covered, NotCovered));
    v.set_front_plugging_mode(mode!(PluggingMode, &f.plugging[0], Plugged, NotPlugged));
    v.set_back_plugging_mode(mode!(PluggingMode, &f.plugging[1], Plugged, NotPlugged));
    v.set_capping_mode(mode!(CappingMode, &f.capping, Capped, NotCapped));
    v.set_filling_mode(mode!(FillingMode, &f.filling, Filled, NotFilled));
    Ok(())
}

impl Session {
    pub(super) fn net_code(&self, net: i32) -> Result<NetCode, SessionError> {
        let code = NetCode(net);

        if self.board.find_net(code).is_none() {
            return Err(SessionError::invalid_argument(format!(
                "no net with code {net}"
            )));
        }

        Ok(code)
    }

    fn default_sizes(&mut self, net: i32, layer: &str) -> Option<super::rules::RouterSizes> {
        self.router_sizes(net, layer).ok()
    }

    fn add_route_item(
        &mut self,
        track: Track,
        locked: bool,
        mode: AddMode,
    ) -> Result<ItemId, SessionError> {
        let id: ItemId = self
            .board
            .add_track(track, mode)
            .map_err(|_| SessionError::invalid_argument("Cannot place Track on non-copper layer"))?
            .into();

        if locked {
            self.board.set_item_locked(id, true);
        }

        Ok(id)
    }

    pub(super) fn insert_track(
        &mut self,
        spec: &NewTrack,
        mode: AddMode,
    ) -> Result<ItemId, SessionError> {
        let layer = copper_layer(&spec.layer)?;
        let net = self.net_code(spec.net)?;
        let uuid = new_uuid(self, &spec.uuid)?;
        let width = match spec.width {
            Some(w) => w,
            None => self
                .default_sizes(spec.net, &spec.layer)
                .map_or(self.board.design_settings().track_min_width, |s| {
                    s.track_width
                }),
        };

        let mut t = PcbTrack::new(&mut self.board.kiid_generator);
        if let Some(uuid) = uuid {
            t.base.item.uuid = uuid;
        }
        t.set_start(vec2(spec.start));
        t.set_end(vec2(spec.end));
        t.set_width(width);
        t.base.item.layer = layer;
        t.base.net = net;
        t.set_has_solder_mask(spec.solder_mask);
        t.set_local_solder_mask_margin(spec.solder_mask_margin);
        self.add_route_item(Track::Trace(t), spec.locked, mode)
    }

    pub(super) fn insert_arc(
        &mut self,
        spec: &NewArc,
        mode: AddMode,
    ) -> Result<ItemId, SessionError> {
        let layer = copper_layer(&spec.layer)?;
        let net = self.net_code(spec.net)?;
        let uuid = new_uuid(self, &spec.uuid)?;
        let width = match spec.width {
            Some(w) => w,
            None => self
                .default_sizes(spec.net, &spec.layer)
                .map_or(self.board.design_settings().track_min_width, |s| {
                    s.track_width
                }),
        };

        let mut a = PcbArc::new(&mut self.board.kiid_generator);
        if let Some(uuid) = uuid {
            a.track.base.item.uuid = uuid;
        }
        a.track.set_start(vec2(spec.start));
        a.set_mid(vec2(spec.mid));
        a.track.set_end(vec2(spec.end));
        a.track.set_width(width);
        a.track.base.item.layer = layer;
        a.track.base.net = net;
        a.track.set_has_solder_mask(spec.solder_mask);
        a.track
            .set_local_solder_mask_margin(spec.solder_mask_margin);
        self.add_route_item(Track::Arc(a), spec.locked, mode)
    }

    pub(super) fn insert_via(
        &mut self,
        spec: &NewVia,
        mode: AddMode,
    ) -> Result<ItemId, SessionError> {
        let net = self.net_code(spec.net)?;
        let uuid = new_uuid(self, &spec.uuid)?;
        let vt = via_type(spec.via_type.as_deref())?;
        let top = copper_layer(spec.top.as_deref().unwrap_or("F.Cu"))?;
        let bottom = copper_layer(spec.bottom.as_deref().unwrap_or("B.Cu"))?;

        let (mut diameter, mut drill) = (spec.diameter, spec.drill);

        if diameter.is_none() || drill.is_none() {
            let sizes = self.default_sizes(spec.net, &super::layer_name(top));
            let bds = self.board.design_settings();
            let (d, h) = match (&sizes, vt) {
                (Some(s), Viatype::Microvia) => (s.microvia_diameter, s.microvia_drill),
                (Some(s), _) => (s.via_diameter, s.via_drill),
                (None, _) => (bds.vias_min_size, bds.min_through_drill),
            };
            diameter = diameter.or(Some(d));
            drill = drill.or(Some(h));
        }

        let env = self.board.child_env(ParentRef::Board);
        let mut v = PcbVia::new(&mut self.board.kiid_generator);
        if let Some(uuid) = uuid {
            v.track.base.item.uuid = uuid;
        }
        BoardItemMut::set_position(&mut v, &env, vec2(spec.at));
        v.set_via_type(&env, vt);
        v.set_layer_pair(&env, top, bottom);
        v.set_width(Padstack::ALL_LAYERS, diameter.unwrap_or_default());
        v.set_drill(drill.unwrap_or_default());
        v.track.base.net = net;
        v.set_is_free(spec.free);
        if let Some(mode) = &spec.unconnected_layers {
            use kicad_pcb::padstack::UnconnectedLayerMode as M;
            let m = [
                M::KeepAll,
                M::StartEndOnly,
                M::RemoveAll,
                M::RemoveExceptStartAndEnd,
            ]
            .into_iter()
            .find(|m| super::enum_name(*m) == *mode)
            .ok_or_else(|| {
                SessionError::invalid_argument(format!("invalid unconnected layer mode '{mode}'"))
            })?;
            v.padstack.set_unconnected_layer_mode(m);
        }

        for l in &spec.zone_layer_connections {
            v.set_zone_layer_override(
                copper_layer(l)?,
                kicad_pcb::pcb_track::ZoneLayerOverride::ForceFlashed,
            );
        }

        if let Some(f) = &spec.finish {
            apply_finish(&mut v, f)?;
        }

        if let Some(td) = &spec.teardrops {
            td.store(&mut v.track.base.teardrop_params);
        }

        self.add_route_item(Track::Via(Box::new(v)), spec.locked, mode)
    }

    pub(super) fn change_item(&mut self, c: &ItemChange) -> Result<ItemId, SessionError> {
        let id = item_id(c.id)?;
        let Some(track_id) = id.typed::<Track>() else {
            return Err(SessionError::invalid_argument(
                "only tracks, arcs and vias can be changed",
            ));
        };

        if self.board.track(track_id).is_none() {
            return Err(SessionError::StaleId { id: c.id });
        }

        let layer = c.layer.as_deref().map(copper_layer).transpose()?;
        let bottom = c.bottom.as_deref().map(copper_layer).transpose()?;

        if let Some(net) = c.net {
            self.net_code(net)?;
        }

        self.board.modify_track(track_id, |track, env| match track {
            Track::Via(v) => {
                if let Some(p) = c.start {
                    BoardItemMut::set_position(&mut **v, env, vec2(p));
                }

                if let Some(w) = c.width {
                    v.set_width(Padstack::ALL_LAYERS, w);
                }

                if let Some(d) = c.drill {
                    v.set_drill(d);
                }

                if layer.is_some() || bottom.is_some() {
                    let top = layer.unwrap_or_else(|| v.top_layer());
                    let bot = bottom.unwrap_or_else(|| v.bottom_layer());
                    v.set_layer_pair(env, top, bot);
                }

                if let Some(free) = c.free {
                    v.set_is_free(free);
                }
            }
            Track::Trace(_) | Track::Arc(_) => {
                if let Some(p) = c.start {
                    track.as_track_mut().set_start(vec2(p));
                }

                if let Some(p) = c.end {
                    track.as_track_mut().set_end(vec2(p));
                }

                if let (Some(p), Track::Arc(a)) = (c.mid, &mut *track) {
                    a.set_mid(vec2(p));
                }

                if let Some(w) = c.width {
                    track.set_width(w);
                }

                if let Some(l) = layer {
                    BoardItemMut::set_layer(track, env, l);
                }
            }
        });

        if let Some(net) = c.net {
            self.board.set_item_net(id, NetCode(net), false);
        }

        if let Some(locked) = c.locked {
            self.board.set_item_locked(id, locked);
        }

        Ok(id)
    }

    pub(super) fn routing_to_remove(&self, r: &RemoveRouting) -> Vec<ItemId> {
        let ctx = kicad_pcb::ctx::BoardCtx::bare(&self.board);
        let nets: BTreeSet<i32> = r.nets.iter().copied().collect();
        let vias = r.vias.unwrap_or(true);

        self.board
            .tracks()
            .filter(|(_, t)| nets.is_empty() || nets.contains(&t.as_track().base.net.0))
            .filter(|(_, t)| vias || !matches!(t, Track::Via(_)))
            .map(|(id, _)| ItemId::from(id))
            .filter(|&id| !r.keep_locked || !ctx.item(id).is_some_and(|it| it.is_locked()))
            .collect()
    }

    /// Port of `BOARD::BuildConnectivity()` on the session's connectivity (what every edit
    /// call does once at its end).
    pub fn rebuild_connectivity(&mut self) {
        self.connectivity.build_connectivity(&mut self.board);
    }

    /// Apply a batch of edits: removals (by handle, then `remove_routing`), changes, then
    /// additions, as one transaction with `ADD_MODE::APPEND` ([`Session::commit`], the port of
    /// `BOARD_COMMIT::Push`:
    /// incremental connectivity, ratsnest of the dirty nets only). Fails without changing
    /// anything when a handle is stale or a layer / net is invalid in the removals or
    /// changes; an invalid addition stops the batch after the edits before it (the error
    /// names it; the connectivity is brought up to date for what was applied).
    pub fn apply_edits(&mut self, batch: &EditBatch) -> Result<EditResult, SessionError> {
        let mut txn = super::commit::Transaction::begin();
        txn.stage_batch(batch);
        // WASM-API-1's contract: new items go to the end of BOARD::Tracks() (re-adding a
        // recorded routing restores the loaded order)
        txn.append = true;
        self.commit(&txn).map(|r| r.edit)
    }

    /// One new track segment ([`Session::apply_edits`] with one addition).
    pub fn add_track_segment(&mut self, spec: &NewTrack) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            add: vec![NewItem::Track(spec.clone())],
            ..EditBatch::default()
        })
    }

    /// One new arc.
    pub fn add_arc(&mut self, spec: &NewArc) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            add: vec![NewItem::Arc(spec.clone())],
            ..EditBatch::default()
        })
    }

    /// One new via.
    pub fn add_via_with(&mut self, spec: &NewVia) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            add: vec![NewItem::Via(spec.clone())],
            ..EditBatch::default()
        })
    }

    /// Change one item.
    pub fn change(&mut self, change: &ItemChange) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            change: vec![change.clone()],
            ..EditBatch::default()
        })
    }

    /// Remove items by handle.
    pub fn remove_items(&mut self, ids: &[u64]) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            remove: ids.to_vec(),
            ..EditBatch::default()
        })
    }

    /// Remove the routing of some nets or of the board.
    pub fn remove_routing(&mut self, r: &RemoveRouting) -> Result<EditResult, SessionError> {
        self.apply_edits(&EditBatch {
            remove_routing: Some(r.clone()),
            ..EditBatch::default()
        })
    }

    /// The board teardrop settings.
    pub fn teardrop_settings(&self) -> TeardropSettings {
        let l = self.board.design_settings().teardrop_params_list();
        TeardropSettings {
            target_vias: l.target_vias,
            target_pth_pads: l.target_pth_pads,
            target_smd_pads: l.target_smd_pads,
            use_round_shapes_only: l.use_round_shapes_only,
            round: TeardropParams::of(l.parameters(TargetTd::Round)),
            rect: TeardropParams::of(l.parameters(TargetTd::Rect)),
            track: TeardropParams::of(l.parameters(TargetTd::Track)),
        }
    }

    /// Replace the board teardrop settings.
    pub fn set_teardrop_settings(&mut self, s: &TeardropSettings) {
        let l = self.board.design_settings_mut().teardrop_params_list_mut();
        l.target_vias = s.target_vias;
        l.target_pth_pads = s.target_pth_pads;
        l.target_smd_pads = s.target_smd_pads;
        l.use_round_shapes_only = s.use_round_shapes_only;
        s.round.store(l.parameters_mut(TargetTd::Round));
        s.rect.store(l.parameters_mut(TargetTd::Rect));
        s.track.store(l.parameters_mut(TargetTd::Track));
    }

    /// `GetTeardropParams().m_Enabled = enabled` on pads and vias (the per-item switch the
    /// teardrop manager reads); returns the handles changed.
    pub fn set_item_teardrops(
        &mut self,
        ids: &[u64],
        enabled: bool,
    ) -> Result<Vec<u64>, SessionError> {
        let items: Vec<ItemId> = ids
            .iter()
            .map(|&id| item_id(id))
            .collect::<Result<_, _>>()?;
        let mut changed = Vec::new();

        for (item, &raw) in items.iter().zip(ids) {
            let done = self.board.modify(*item, |any, _| match any {
                AnyItemMut::Pad(p) => {
                    p.base.teardrop_params.enabled = enabled;
                    true
                }
                AnyItemMut::Track(Track::Via(v)) => {
                    v.track.base.teardrop_params.enabled = enabled;
                    true
                }
                _ => false,
            });

            match done {
                None => return Err(SessionError::StaleId { id: raw }),
                Some(true) => changed.push(raw),
                Some(false) => {}
            }
        }

        Ok(changed)
    }

    /// `TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, true )` (full update,
    /// `kicad_zone::tool::update_teardrops`), then `BuildConnectivity`, then the summary of
    /// [`TeardropOutcome`].
    pub fn update_teardrops(&mut self) -> Result<TeardropOutcome, SessionError> {
        self.ensure_drc_engine();
        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported("DRC engine"));
        };

        kicad_zone::tool::update_teardrops(&mut self.board, &mut self.connectivity, engine, true);
        self.rebuild_connectivity();

        let board = &self.board;
        let list = board.design_settings().teardrop_params_list();
        let mut outcome = TeardropOutcome {
            teardrops: board.zones().filter(|(_, z)| z.is_teardrop_area()).count() as u32,
            ..TeardropOutcome::default()
        };

        let mut candidates: Vec<(ItemId, Kiid)> = Vec::new();

        if list.target_vias {
            for (id, t) in board.tracks() {
                if let Track::Via(v) = t {
                    if v.track.base.teardrop_params.enabled {
                        candidates.push((id.into(), v.track.base.item.uuid));
                    }
                }
            }
        }

        for (fp, _) in board.footprints() {
            for (id, pad) in board.footprint_pads(fp) {
                let targeted = match pad.attribute() {
                    kicad_pcb::padstack::PadAttrib::Pth => list.target_pth_pads,
                    kicad_pcb::padstack::PadAttrib::Smd => list.target_smd_pads,
                    _ => false,
                };

                if targeted && pad.base.teardrop_params.enabled {
                    candidates.push((id.into(), pad.base.item.uuid));
                }
            }
        }

        for (item, uuid) in candidates {
            let tracks: Vec<ItemId> = self
                .connectivity
                .connected_tracks(item)
                .into_iter()
                .filter(|t| {
                    !matches!(
                        self.board.track(t.typed().unwrap()),
                        Some(Track::Via(_)) | None
                    )
                })
                .collect();

            if tracks.is_empty() {
                continue;
            }

            outcome.candidates += 1;

            let has = tracks.iter().any(|t| {
                let track_uuid = self
                    .board
                    .track(t.typed().unwrap())
                    .unwrap()
                    .as_track()
                    .base
                    .item
                    .uuid;
                let td =
                    kicad_zone::teardrop::TeardropManager::teardrop_uuid(&track_uuid, &uuid, 0);
                self.board.resolve_item(&td).is_some()
            });

            if !has {
                outcome.without.push(handle(item));
            }
        }

        Ok(outcome)
    }

    /// The DRC engine the zone filler and the teardrop manager need: created from the
    /// session's rule text when the board has none ([`Session::new_empty`]), as
    /// `ZONE_FILLER::Fill` does (see [`Session::fill_all_zones`]).
    pub(crate) fn ensure_drc_engine(&mut self) {
        if self.drc_engine.is_none() {
            let _ = self.fill_engine_only();
        }
    }

    fn fill_engine_only(&mut self) -> Option<()> {
        let mut engine = kicad_drc::engine::DrcEngine::new();
        let rules_path = crate::board_loader::with_extension(&self.source_name, "kicad_dru");
        let tuning_profiles = crate::board_loader::tuning_profiles(&self.board);
        let _ = engine.init_engine(
            &mut self.board,
            self.rules_text.as_deref().map(|t| (t, rules_path.as_str())),
            kicad_drc::rule_parser::default_layer_choices(),
            &tuning_profiles,
            None,
        );
        self.drc_engine = Some(engine);
        Some(())
    }
}
