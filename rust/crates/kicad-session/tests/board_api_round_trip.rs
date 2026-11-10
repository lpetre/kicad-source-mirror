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

//! WASM-API-1: the router round trip through the board API. For corpus boards: load (the
//! kicad-cli path), save; record the routing with `route_items`, remove all routing
//! (`apply_edits` with `remove_routing`), add it back with the recorded UUIDs in one
//! `apply_edits`, save again: the two saves must be identical (and the first equals the
//! reference re-save, `upgrade_corpus.rs`). DRC (`run_drc`, JSON) on the round-tripped
//! board must equal DRC on the original (ecc83 and the first twenty corpus boards).

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kicad_pcb::project::LoadPath;
use kicad_qa::io_corpus::io_corpus_boards;
use kicad_session::board_api::edit::{EditBatch, NewArc, NewItem, NewTrack, NewVia, RemoveRouting};
use kicad_session::board_api::read::RouteItem;
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat};
use kicad_session::{HostClock, LoadInputs, Reporter, Session, run_drc};

/// The [`NewItem`] that re-creates a recorded item exactly (UUID included).
pub fn new_item(r: &RouteItem) -> NewItem {
    match r.kind.as_str() {
        "track" => NewItem::Track(NewTrack {
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
        "arc" => NewItem::Arc(NewArc {
            start: r.start,
            mid: r.mid.expect("arc mid"),
            end: r.end,
            width: Some(r.width),
            layer: r.layer.clone(),
            net: r.net,
            uuid: Some(r.uuid.clone()),
            locked: r.locked,
            solder_mask: r.solder_mask,
            solder_mask_margin: r.solder_mask_margin,
        }),
        _ => {
            let v = r.via.as_ref().expect("via details");
            NewItem::Via(NewVia {
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
            })
        }
    }
}

fn load(path: &Path) -> Option<Session> {
    let text = std::fs::read_to_string(path).ok()?;
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let dru = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_string_lossy().into_owned();
    let inputs = LoadInputs {
        text: &text,
        source_name: &name,
        project_json: pro.as_deref(),
        rules_text: dru.as_deref(),
        ..LoadInputs::default()
    };
    let (mut s, _) = common::load_staged(LoadPath::Cli, &inputs).ok()?;
    s.apply_host_inputs(&HostClock::parse_iso("2026-01-01T00:00:00").unwrap(), 1);
    Some(s)
}

fn drc_json(s: &mut Session) -> String {
    let mut job = JobPcbDrc::new();
    job.rc.format = RcOutputFormat::Json;
    job.report_date = "2026-01-01T00:00:00+0000".to_string();
    job.marker_uuid_seed = 7;
    let mut reporter = Reporter::new();
    let outcome = run_drc(s, &mut job, &mut reporter).expect("drc");
    outcome.report_text.unwrap_or_default()
}

/// The round trip on one board: `Ok(true)` identical, `Ok(false)` skipped, `Err` mismatch.
fn round_trip(path: &Path, with_drc: bool) -> Result<bool, String> {
    let Some(mut s) = load(path) else {
        return Ok(false);
    };
    // Known limit: group / generator membership of a removed track is not restored.
    if s.board
        .tracks()
        .any(|(_, t)| t.as_track().base.item.group.is_some())
    {
        return Ok(false);
    }

    let original = s.save().map_err(|e| e.to_string())?;
    let recorded = s.route_items();

    let removed = s
        .apply_edits(&EditBatch {
            remove_routing: Some(RemoveRouting::default()),
            ..EditBatch::default()
        })
        .map_err(|e| e.to_string())?;

    if removed.removed.len() != recorded.len() {
        return Err(format!(
            "removed {} of {} items",
            removed.removed.len(),
            recorded.len()
        ));
    }

    if !s.route_items().is_empty() {
        return Err("routing left after remove_routing".into());
    }

    let added = s
        .apply_edits(&EditBatch {
            add: recorded.iter().map(new_item).collect(),
            ..EditBatch::default()
        })
        .map_err(|e| e.to_string())?;

    if added.added_uuids != recorded.iter().map(|r| r.uuid.clone()).collect::<Vec<_>>() {
        return Err("UUIDs not kept".into());
    }

    let again = s.save().map_err(|e| e.to_string())?;

    if again != original {
        let d = kicad_qa::sexpr_text::first_difference(&original, &again, 3)
            .map(|d| format!("{d:?}"))
            .unwrap_or_default();
        return Err(format!("saved board differs: {d}"));
    }

    // The re-read routing is the recorded one (handles are new).
    let strip = |v: Vec<RouteItem>| {
        v.into_iter()
            .map(|mut r| {
                r.id = 0;
                r
            })
            .collect::<Vec<_>>()
    };

    if strip(s.route_items()) != strip(recorded) {
        return Err("route items differ after the round trip".into());
    }

    if with_drc {
        let mut fresh = load(path).ok_or("reload")?;
        let a = drc_json(&mut fresh);
        let b = drc_json(&mut s);

        if a != b {
            return Err("DRC report differs after the round trip".into());
        }
    }

    Ok(true)
}

/// Boards whose routing the API cannot re-create exactly (PORT_STATUS.md, WASM-API-1 known
/// gaps): per-layer via padstacks, backdrill and post-machining are not in `NewVia`.
const KNOWN_GAPS: &[(&str, &str)] = &[
    (
        "qa/data/pcbnew/issue24758/issue24758.kicad_pcb",
        "via padstack per layer",
    ),
    (
        "qa/data/pcbnew/issue25021/backdrill.kicad_pcb",
        "via backdrill",
    ),
    (
        "qa/data/pcbnew/padstacks.kicad_pcb",
        "via post-machining / padstack",
    ),
];

#[test]
fn ecc83_round_trip_with_drc() {
    let path = kicad_qa::demos_dir().join("ecc83/ecc83-pp.kicad_pcb");
    assert_eq!(round_trip(&path, true), Ok(true));
}

#[test]
fn corpus_round_trip() {
    common::quiet_pending_panics();
    let mut ok = 0;
    let mut failures = Vec::new();

    for b in io_corpus_boards() {
        if b.skip.is_some() || b.size > 300_000 {
            continue;
        }

        if KNOWN_GAPS.iter().any(|(n, _)| *n == b.name) {
            continue;
        }

        // DRC (JSON report) compared on the first twenty boards.
        let with_drc = ok < 20;

        match catch_unwind(AssertUnwindSafe(|| round_trip(&b.path, with_drc))) {
            Ok(Ok(true)) => ok += 1,
            Ok(Ok(false)) => {}
            Ok(Err(e)) => failures.push(format!("{}: {e}", b.name)),
            Err(p) => {
                let text = common::panic_text(p.as_ref());

                if !text.contains("port pending") {
                    failures.push(format!("{}: panic {text}", b.name));
                }
            }
        }
    }

    eprintln!("round trip identical on {ok} boards (DRC compared on 20)");
    assert!(ok >= 280, "only {ok} boards");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
