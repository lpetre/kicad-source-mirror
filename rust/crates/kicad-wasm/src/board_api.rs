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

//! JavaScript bindings of the router API (`kicad_session::board_api`, package WASM-API-1).
//! New code without a C++ counterpart: one call per bulk query or edit batch, every value
//! one JSON object (see `rust/docs/WASM_API.md` and `types/board_api.d.ts`).

use kicad_session::board_api::check::{QuickDrcOptions, severity_mask};
use kicad_session::board_api::edit::{
    EditBatch, ItemChange, NewArc, NewTrack, NewVia, RemoveRouting, TeardropSettings,
};
use kicad_session::board_api::rules::ConstraintQuery;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::{Board, from_js, session_error, to_js};

/// Options of the bulk read calls.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReadOptions {
    /// Include polygons (pad effective polygons, zone fills).
    pub polygons: bool,
}

/// Options of `drcViolations`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ViolationOptions {
    /// Severity names kept (`error`, `warning`, `exclusions`); empty = error and warning.
    pub severities: Vec<String>,
}

fn required<T: for<'de> Deserialize<'de>>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| {
        session_error(kicad_session::SessionError::invalid_argument(e.to_string()))
    })
}

fn ids(value: JsValue) -> Result<Vec<u64>, JsValue> {
    let raw: Vec<f64> = required(value)?;
    raw.into_iter()
        .map(|id| {
            if id >= 0.0 && id.fract() == 0.0 && id < 9_007_199_254_740_992.0 {
                Ok(id as u64)
            } else {
                Err(session_error(kicad_session::SessionError::StaleId { id: 0 }))
            }
        })
        .collect()
}

#[wasm_bindgen]
impl Board {
    /// Layers, outline, bounding boxes, rule minimums, stackup.
    #[wasm_bindgen(js_name = boardInfo)]
    pub fn board_info(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.board_info())
    }

    /// Nets with their effective net class and its constituents.
    #[wasm_bindgen(js_name = netDetails)]
    pub fn net_details(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.net_details())
    }

    /// The net classes of the design settings.
    pub fn netclasses(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.netclasses())
    }

    /// Every pad; `{ polygons: true }` adds the effective copper polygon per layer.
    pub fn pads(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: ReadOptions = from_js(options)?;
        to_js(&self.session.pads(o.polygons))
    }

    /// Every footprint with courtyards.
    pub fn footprints(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.footprints_info())
    }

    /// Tracks, arcs and vias with every property `applyEdits` takes back.
    #[wasm_bindgen(js_name = routeItems)]
    pub fn route_items(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.route_items())
    }

    /// Zones and rule areas; `{ polygons: true }` adds the fills.
    pub fn zones(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: ReadOptions = from_js(options)?;
        to_js(&self.session.zones_info(o.polygons))
    }

    /// Graphic shapes and texts on copper layers as polygons.
    #[wasm_bindgen(js_name = copperObstacles)]
    pub fn copper_obstacles(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.copper_obstacles())
    }

    /// The Edge.Cuts shapes as polygons.
    #[wasm_bindgen(js_name = edgeShapes)]
    pub fn edge_shapes(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.edge_shapes())
    }

    /// Ratsnest edges, per-net counts and clusters.
    pub fn connectivity(&mut self) -> Result<JsValue, JsValue> {
        to_js(&self.session.connectivity_info())
    }

    /// `DRC_ENGINE::EvalRules` for `{ kind, a, b, layer }`.
    pub fn constraint(&mut self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: ConstraintQuery = required(query)?;
        to_js(&self.session.constraint(&q).map_err(session_error)?)
    }

    /// The KiCad router's default sizes for a net on a layer.
    #[wasm_bindgen(js_name = routerSizes)]
    pub fn router_sizes(&mut self, net: i32, layer: &str) -> Result<JsValue, JsValue> {
        to_js(&self.session.router_sizes(net, layer).map_err(session_error)?)
    }

    /// Removals, changes and additions with one connectivity rebuild.
    #[wasm_bindgen(js_name = applyEdits)]
    pub fn apply_edits(&mut self, batch: JsValue) -> Result<JsValue, JsValue> {
        let b: EditBatch = required(batch)?;
        to_js(&self.session.apply_edits(&b).map_err(session_error)?)
    }

    /// One track segment.
    #[wasm_bindgen(js_name = addTrackSegment)]
    pub fn add_track_segment(&mut self, spec: JsValue) -> Result<JsValue, JsValue> {
        let s: NewTrack = required(spec)?;
        to_js(&self.session.add_track_segment(&s).map_err(session_error)?)
    }

    /// One arc.
    #[wasm_bindgen(js_name = addArc)]
    pub fn add_arc(&mut self, spec: JsValue) -> Result<JsValue, JsValue> {
        let s: NewArc = required(spec)?;
        to_js(&self.session.add_arc(&s).map_err(session_error)?)
    }

    /// One via of any type.
    #[wasm_bindgen(js_name = placeVia)]
    pub fn place_via(&mut self, spec: JsValue) -> Result<JsValue, JsValue> {
        let s: NewVia = required(spec)?;
        to_js(&self.session.add_via_with(&s).map_err(session_error)?)
    }

    /// Move / resize / re-layer / re-net / lock one item.
    #[wasm_bindgen(js_name = changeItem)]
    pub fn change_item(&mut self, change: JsValue) -> Result<JsValue, JsValue> {
        let c: ItemChange = required(change)?;
        to_js(&self.session.change(&c).map_err(session_error)?)
    }

    /// Remove items by handle.
    #[wasm_bindgen(js_name = removeItems)]
    pub fn remove_items(&mut self, handles: JsValue) -> Result<JsValue, JsValue> {
        let ids = ids(handles)?;
        to_js(&self.session.remove_items(&ids).map_err(session_error)?)
    }

    /// Remove the routing of some nets or of the board.
    #[wasm_bindgen(js_name = removeRouting)]
    pub fn remove_routing(&mut self, options: JsValue) -> Result<JsValue, JsValue> {
        let r: RemoveRouting = from_js(options)?;
        to_js(&self.session.remove_routing(&r).map_err(session_error)?)
    }

    /// `BOARD::BuildConnectivity()`.
    #[wasm_bindgen(js_name = rebuildConnectivity)]
    pub fn rebuild_connectivity(&mut self) {
        self.session.rebuild_connectivity();
    }

    /// The board teardrop settings.
    #[wasm_bindgen(js_name = teardropSettings)]
    pub fn teardrop_settings(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.teardrop_settings())
    }

    /// Replace the board teardrop settings.
    #[wasm_bindgen(js_name = setTeardropSettings)]
    pub fn set_teardrop_settings(&mut self, settings: JsValue) -> Result<(), JsValue> {
        let s: TeardropSettings = required(settings)?;
        self.session.set_teardrop_settings(&s);
        Ok(())
    }

    /// Enable / disable teardrops on pads and vias; returns the handles changed.
    #[wasm_bindgen(js_name = setItemTeardrops)]
    pub fn set_item_teardrops(&mut self, handles: JsValue, enabled: bool) -> Result<JsValue, JsValue> {
        let ids = ids(handles)?;
        to_js(
            &self
                .session
                .set_item_teardrops(&ids, enabled)
                .map_err(session_error)?,
        )
    }

    /// `TEARDROP_MANAGER::UpdateTeardrops` (full update).
    #[wasm_bindgen(js_name = updateTeardrops)]
    pub fn update_teardrops(&mut self) -> Result<JsValue, JsValue> {
        to_js(&self.session.update_teardrops().map_err(session_error)?)
    }

    /// DRC with only the chosen providers, structured violations, markers untouched.
    #[wasm_bindgen(js_name = quickDrc)]
    pub fn quick_drc(&mut self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: QuickDrcOptions = from_js(options)?;
        to_js(&self.session.quick_drc(&o).map_err(session_error)?)
    }

    /// The structured violations of the last `runDrc` (its markers).
    #[wasm_bindgen(js_name = drcViolations)]
    pub fn drc_violations(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: ViolationOptions = from_js(options)?;
        let mask = severity_mask(
            &o.severities,
            kicad_core::Severity::Error.as_i32() | kicad_core::Severity::Warning.as_i32(),
        );
        to_js(&self.session.drc_violations(mask))
    }
}
