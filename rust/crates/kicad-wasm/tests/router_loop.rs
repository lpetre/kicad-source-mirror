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

//! WASM-API-1: the router loop of `web/test/router_loop.mjs`, natively, through the same
//! session API the bindings call: load (fixed UUID seed and date), record the routing,
//! remove all routing, add it back in one `apply_edits`, update teardrops, refill, DRC (JSON),
//! save. The results are pinned in `tests/golden/router_loop_native.json`, which the node
//! test compares with the wasm build (native == wasm); `KICAD_WASM_UPDATE_GOLDEN=1` rewrites
//! it. The loop must leave the board as `load -> refill -> save` does (the routing comes back
//! unchanged) and DRC must report the same.

use kicad_session::board_api::edit::{EditBatch, NewArc, NewItem, NewTrack, NewVia, RemoveRouting};
use kicad_session::board_api::read::RouteItem;
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat, RcUnits};
use kicad_session::{HostClock, LoadInputs, Reporter, Session, run_drc};
use kicad_wasm::api;
use serde_json::{Value, json};

const FIXED_DATE: &str = "2026-01-02T03:04:05";
const UUID_SEED: u32 = 7;

/// ecc83 (2 layers, the package's demo), a 4-layer board with zones and 100 vias, a teardrop
/// board.
const BOARDS: &[&str] = &[
    "demos/ecc83/ecc83-pp.kicad_pcb",
    "qa/data/pcbnew/issue22010/issue22010.kicad_pcb",
    "qa/data/pcbnew/teardrop_spike.kicad_pcb",
];

fn hashed(text: &str) -> Value {
    json!({ "length": text.len(), "fnv": format!("{:016x}", api::fnv1a64(text.as_bytes())) })
}

fn load(rel: &str) -> Session {
    let path = kicad_qa::kicad_source_root().join(rel);
    let text = std::fs::read_to_string(&path).expect("board");
    let project = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let rules = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = rel.rsplit('/').next().unwrap();
    let inputs = LoadInputs {
        text: &text,
        source_name: name,
        project_json: project.as_deref(),
        rules_text: rules.as_deref(),
        ..LoadInputs::default()
    };
    let mut s = Session::load_board_with(&inputs).expect("loads");
    s.apply_host_inputs(&HostClock::parse_iso(FIXED_DATE).unwrap(), UUID_SEED);
    s
}

/// `runDrc({ format: 'json' })` of `lib.rs`.
fn drc_json(s: &mut Session) -> String {
    let mut job = JobPcbDrc::new();
    job.rc.exit_code_violations = false;
    job.parity = false;
    job.rc.units = RcUnits::Mm;
    job.rc.format = RcOutputFormat::Json;
    job.report_date = HostClock::parse_iso(FIXED_DATE).unwrap().iso8601();
    job.marker_uuid_seed = UUID_SEED;
    let mut reporter = Reporter::new();
    run_drc(s, &mut job, &mut reporter)
        .expect("drc")
        .report_text
        .unwrap_or_default()
}

/// What `router_loop.mjs` does with a recorded item (the same field mapping).
fn new_item(r: &RouteItem) -> NewItem {
    match (r.kind.as_str(), &r.via) {
        ("arc", _) => NewItem::Arc(NewArc {
            start: r.start,
            mid: r.mid.unwrap(),
            end: r.end,
            width: Some(r.width),
            layer: r.layer.clone(),
            net: r.net,
            uuid: Some(r.uuid.clone()),
            locked: r.locked,
            solder_mask: r.solder_mask,
            solder_mask_margin: r.solder_mask_margin,
        }),
        ("via", Some(v)) => NewItem::Via(NewVia {
            at: r.start,
            via_type: Some(v.via_type.clone()),
            top: Some(v.top.clone()),
            bottom: Some(v.bottom.clone()),
            diameter: Some(r.width),
            drill: Some(v.drill),
            net: r.net,
            free: v.free,
            uuid: Some(r.uuid.clone()),
            locked: r.locked,
            finish: Some(v.finish.clone()),
            unconnected_layers: Some(v.unconnected_layers.clone()),
            zone_layer_connections: v.zone_layer_connections.clone(),
            teardrops: Some(v.teardrops.clone()),
        }),
        _ => NewItem::Track(NewTrack {
            start: r.start,
            end: r.end,
            width: Some(r.width),
            layer: r.layer.clone(),
            net: r.net,
            uuid: Some(r.uuid.clone()),
            locked: r.locked,
            solder_mask: r.solder_mask,
            solder_mask_margin: r.solder_mask_margin,
        }),
    }
}

fn run_loop(rel: &str) -> Value {
    let mut s = load(rel);
    let info = s.board_info();
    let pads = s.pads(true);
    let items = s.route_items();
    let before = s.connectivity_info();

    let removed = s
        .apply_edits(&EditBatch {
            remove_routing: Some(RemoveRouting::default()),
            ..EditBatch::default()
        })
        .unwrap();
    let stripped = s.connectivity_info();
    let added = s
        .apply_edits(&EditBatch {
            add: items.iter().map(new_item).collect(),
            ..EditBatch::default()
        })
        .unwrap();
    let after = s.connectivity_info();

    let uses_teardrops = s.zones_info(false).iter().any(|z| z.teardrop);
    let teardrops = if uses_teardrops {
        let o = s.update_teardrops().unwrap();
        json!({ "teardrops": o.teardrops, "candidates": o.candidates, "without": o.without.len() })
    } else {
        Value::Null
    };

    s.refill_zones().unwrap();
    let drc = drc_json(&mut s);
    let violations = s.drc_violations(
        kicad_core::Severity::Error.as_i32() | kicad_core::Severity::Warning.as_i32(),
    );
    let saved = s.save().unwrap();

    // The loop leaves the board as a plain refill does.
    let mut plain = load(rel);
    plain.refill_zones().unwrap();
    let plain_drc = drc_json(&mut plain);
    assert_eq!(saved, plain.save().unwrap(), "{rel}: saved board");
    assert_eq!(drc, plain_drc, "{rel}: DRC report");
    assert_eq!(before.unconnected, after.unconnected, "{rel}: unconnected");

    json!({
        "file": rel,
        "copperLayers": info.copper_layer_count,
        "pads": pads.len(),
        "padPolygons": pads.iter().map(|p| p.polygons.len()).sum::<usize>(),
        "items": items.len(),
        "removed": removed.removed.len(),
        "added": added.added.len(),
        "unconnectedBefore": before.unconnected,
        "unconnectedStripped": stripped.unconnected,
        "unconnectedAfter": after.unconnected,
        "teardrops": teardrops,
        "violations": violations.violations.len(),
        "drc": hashed(&drc),
        "save": hashed(&saved),
    })
}

#[test]
fn router_loop_native_matches_golden() {
    let current = serde_json::to_string_pretty(&json!({
        "fixedDate": FIXED_DATE,
        "uuidSeed": UUID_SEED,
        "boards": BOARDS.iter().map(|b| run_loop(b)).collect::<Vec<_>>(),
    }))
    .unwrap()
        + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/router_loop_native.json");

    if std::env::var_os("KICAD_WASM_UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::write(&path, &current).unwrap();
    }

    assert_eq!(std::fs::read_to_string(&path).unwrap(), current);
}
