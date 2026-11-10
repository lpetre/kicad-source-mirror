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

//! JavaScript bindings of the live router loop (package LIVE-1): transactions
//! (`BOARD_COMMIT::Push` with incremental connectivity), checkpoints, `finalize`, and the flat
//! typed-array forms of the hot calls (`kicad_session::board_api::commit` / `bulk`).

use js_sys::{Float64Array, Int32Array, Object, Reflect};
use kicad_session::SessionError;
use kicad_session::board_api::commit::{CommitReport, Transaction as Txn};
use kicad_session::board_api::check::CheckItemsOptions;
use kicad_session::board_api::edit::{EditBatch, ItemChange, NewItem, RemoveRouting};
use kicad_session::board_api::refill::ChangeDamage;
use wasm_bindgen::prelude::*;

use crate::board_api::{ids, required};
use crate::{Board, from_js, session_error, to_js};

/// Handles as a `Float64Array` (exact: handles are below 2^53).
fn handles_array(ids: &[u64]) -> Float64Array {
    let v: Vec<f64> = ids.iter().map(|&id| id as f64).collect();
    Float64Array::from(v.as_slice())
}

/// The handles of a `Float64Array` (integral, non-negative, below 2^53).
fn handles_of(a: &Float64Array) -> Result<Vec<u64>, JsValue> {
    a.to_vec()
        .into_iter()
        .map(|id| {
            if id >= 0.0 && id.fract() == 0.0 && id < 9_007_199_254_740_992.0 {
                Ok(id as u64)
            } else {
                Err(session_error(SessionError::StaleId { id: 0 }))
            }
        })
        .collect()
}

fn set(obj: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj, &JsValue::from_str(key), value).map(|_| ())
}

/// A staged commit: removals, changes and additions, applied by `Board.commit`.
#[wasm_bindgen]
#[derive(Default)]
pub struct Transaction {
    txn: Txn,
}

#[wasm_bindgen]
impl Transaction {
    /// An empty transaction.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Transaction {
        Transaction::default()
    }

    /// The number of staged entries.
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.txn.entries.len()
    }

    /// `SKIP_CONNECTIVITY`: no re-indexing of changes, ratsnest cleared instead of rebuilt.
    #[wasm_bindgen(setter, js_name = skipConnectivity)]
    pub fn set_skip_connectivity(&mut self, skip: bool) {
        self.txn.skip_connectivity = skip;
    }

    /// Add new items at the end of the board's track list (`ADD_MODE::APPEND`) instead of the
    /// front (`BULK_INSERT`, as `BOARD_COMMIT::Push`).
    #[wasm_bindgen(setter)]
    pub fn set_append(&mut self, append: bool) {
        self.txn.append = append;
    }

    /// Stages removals by handle (a `Float64Array` or a number array).
    pub fn remove(&mut self, handles: JsValue) -> Result<(), JsValue> {
        let ids = match handles.dyn_ref::<Float64Array>() {
            Some(a) => handles_of(a)?,
            None => ids(handles)?,
        };
        self.txn.stage_removals(&ids);
        Ok(())
    }

    /// Stages the removal of the routing of some nets (or of the whole board).
    #[wasm_bindgen(js_name = removeRouting)]
    pub fn remove_routing(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "RemoveRouting | undefined")] options: JsValue,
    ) -> Result<(), JsValue> {
        let r: RemoveRouting = from_js(options)?;
        self.txn.stage_remove_routing(r);
        Ok(())
    }

    /// Stages a change of a track / arc / via.
    pub fn change(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "ItemChange")] change: JsValue,
    ) -> Result<(), JsValue> {
        let c: ItemChange = required(change)?;
        self.txn.stage_change(c);
        Ok(())
    }

    /// Stages a new item (`{ kind: "track" | "arc" | "via", ... }`).
    pub fn add(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "NewItem")] item: JsValue,
    ) -> Result<(), JsValue> {
        let i: NewItem = required(item)?;
        self.txn.stage_add(i);
        Ok(())
    }

    /// Stages a whole `EditBatch` (removals, routing removal, changes, additions).
    pub fn stage(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "EditBatch")] batch: JsValue,
    ) -> Result<(), JsValue> {
        let b: EditBatch = required(batch)?;
        self.txn.stage_batch(&b);
        Ok(())
    }

    /// Stages track segments from flat records `x1, y1, x2, y2, width, layer, net`.
    #[wasm_bindgen(js_name = addTracks)]
    pub fn add_tracks(&mut self, records: &Int32Array) -> Result<usize, JsValue> {
        self.txn
            .stage_track_records(&records.to_vec())
            .map_err(session_error)
    }

    /// Stages arcs from flat records `x1, y1, x2, y2, width, layer, net, midX, midY`.
    #[wasm_bindgen(js_name = addArcs)]
    pub fn add_arcs(&mut self, records: &Int32Array) -> Result<usize, JsValue> {
        self.txn
            .stage_arc_records(&records.to_vec())
            .map_err(session_error)
    }

    /// Stages vias from flat records `x, y, diameter, drill, top, bottom, net, type`.
    #[wasm_bindgen(js_name = addVias)]
    pub fn add_vias(&mut self, records: &Int32Array) -> Result<usize, JsValue> {
        self.txn
            .stage_via_records(&records.to_vec())
            .map_err(session_error)
    }
}

/// The typed-array form of a [`CommitReport`].
fn bulk_report(r: &CommitReport) -> Result<JsValue, JsValue> {
    let o = Object::new();
    set(&o, "added", &handles_array(&r.edit.added))?;
    set(&o, "removed", &handles_array(&r.edit.removed))?;
    set(&o, "changed", &handles_array(&r.edit.changed))?;
    set(&o, "dirtyNets", &Int32Array::from(r.dirty_nets.as_slice()))?;
    let pairs: Vec<i32> = r.net_unconnected.iter().flatten().copied().collect();
    set(&o, "netUnconnected", &Int32Array::from(pairs.as_slice()))?;
    let changes: Vec<f64> = r
        .net_changes
        .iter()
        .flat_map(|c| [c.id as f64, c.from as f64, c.to as f64])
        .collect();
    set(&o, "netChanges", &Float64Array::from(changes.as_slice()))?;
    set(&o, "unconnected", &JsValue::from(r.unconnected))?;
    set(
        &o,
        "unconnectedVisible",
        &JsValue::from(r.unconnected_visible),
    )?;
    Ok(o.into())
}

#[wasm_bindgen]
impl Board {
    /// Applies a transaction as `BOARD_COMMIT::Push` (incremental connectivity, ratsnest of the
    /// dirty nets only); returns the change report.
    #[wasm_bindgen(unchecked_return_type = "CommitReport")]
    pub fn commit(&mut self, txn: &Transaction) -> Result<JsValue, JsValue> {
        let report = self.tracked_commit(txn)?;
        to_js(&report)
    }

    /// `commit` returning typed arrays (handles as `Float64Array`, nets as `Int32Array`,
    /// `netChanges` as `[id, from, to]` triples) and no UUID strings.
    #[wasm_bindgen(js_name = commitBulk, unchecked_return_type = "CommitReportBulk")]
    pub fn commit_bulk(&mut self, txn: &Transaction) -> Result<JsValue, JsValue> {
        let report = self.tracked_commit(txn)?;
        bulk_report(&report)
    }

    /// Saves board and connectivity; returns a token for `rollback`.
    pub fn checkpoint(&mut self) -> u32 {
        let token = self.next_checkpoint;
        self.next_checkpoint += 1;
        self.checkpoints.insert(
            token,
            (self.session.checkpoint(), self.zone_damage.clone()),
        );
        token
    }

    /// Puts board and connectivity back to a checkpoint (which stays available).
    pub fn rollback(&mut self, token: u32) -> Result<(), JsValue> {
        let cp = self.checkpoints.get(&token).ok_or_else(|| {
            session_error(SessionError::invalid_argument(format!(
                "no checkpoint {token}"
            )))
        })?;
        self.session.rollback(&cp.0);
        self.zone_damage = cp.1.clone();
        Ok(())
    }

    /// Frees a checkpoint.
    #[wasm_bindgen(js_name = releaseCheckpoint)]
    pub fn release_checkpoint(&mut self, token: u32) -> bool {
        self.checkpoints.remove(&token).is_some()
    }

    /// Full connectivity rebuild (`BOARD::BuildConnectivity`): what a reload of the saved
    /// board computes. Call it before the final refill, DRC and save.
    pub fn finalize(&mut self) {
        // the full rebuild can give items other nets than the incremental commits did
        let damage = self.session.finalize_tracked();
        self.zone_damage.merge(&damage);
    }

    /// The routing as flat records: `{ trackIds, tracks, arcIds, arcs, viaIds, vias }`.
    #[wasm_bindgen(js_name = routeRecords, unchecked_return_type = "RouteRecords")]
    pub fn route_records(&self) -> Result<JsValue, JsValue> {
        let r = self.session.route_records();
        let o = Object::new();
        set(&o, "trackIds", &handles_array(&r.track_ids))?;
        set(&o, "tracks", &Int32Array::from(r.tracks.as_slice()))?;
        set(&o, "arcIds", &handles_array(&r.arc_ids))?;
        set(&o, "arcs", &Int32Array::from(r.arcs.as_slice()))?;
        set(&o, "viaIds", &handles_array(&r.via_ids))?;
        set(&o, "vias", &Int32Array::from(r.vias.as_slice()))?;
        Ok(o.into())
    }
}

impl Board {
    /// `Session::commit` with its damage merged into the zone damage (package LIVE-2).
    fn tracked_commit(&mut self, txn: &Transaction) -> Result<CommitReport, JsValue> {
        let report = self.session.commit(&txn.txn);

        match &report {
            Ok(r) => self.zone_damage.merge(&r.damage),
            // an addition that failed half way: what was applied is not reported
            Err(_) => self.untracked_edit(),
        }

        report.map_err(session_error)
    }

    /// An edit that does not report its damage: the next affected refill is a full one.
    pub(crate) fn untracked_edit(&mut self) {
        self.zone_damage = ChangeDamage::full();
    }
}

#[wasm_bindgen]
impl Board {
    /// Zone refill of only the zones the changes since the last refill can affect (every
    /// commit, `quickDrc` / `checkItems` and `finalize` reports its changes; any other edit
    /// makes the next refill a full one). The result is identical to `refillZones()`.
    #[wasm_bindgen(js_name = refillZonesAffected, unchecked_return_type = "RefillReport")]
    pub fn refill_zones_affected(&mut self) -> Result<JsValue, JsValue> {
        let report = self
            .session
            .refill_zones_affected(&self.zone_damage)
            .map_err(session_error)?;
        self.zone_damage = report.damage.clone();
        to_js(&report)
    }

    /// The changes the next `refillZonesAffected` refills for.
    #[wasm_bindgen(js_name = zoneDamage, unchecked_return_type = "ChangeDamage")]
    pub fn zone_damage(&self) -> Result<JsValue, JsValue> {
        to_js(&self.zone_damage)
    }

    /// The violations that involve one of the items (the subset of a full all-track-errors
    /// run of the chosen providers whose entries name one of them).
    #[wasm_bindgen(js_name = checkItems, unchecked_return_type = "DrcSummary")]
    pub fn check_items(
        &mut self,
        handles: &Float64Array,
        #[wasm_bindgen(unchecked_param_type = "CheckItemsOptions | undefined")] options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let o: CheckItemsOptions = from_js(options)?;
        let ids = handles_of(handles)?;
        let summary = self.session.check_items(&ids, &o).map_err(session_error)?;
        self.zone_damage.merge(&summary.damage);
        to_js(&summary)
    }
}
