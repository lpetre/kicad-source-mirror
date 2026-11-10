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

//! WASM-API-1: edit calls, teardrops and the structured / quick DRC.
//!
//! * Teardrops: on boards saved by KiCad with teardrops, a full `update_teardrops` rebuilds
//!   them; the saved board must equal the re-save of the loaded board (the reference
//!   re-save, `upgrade_corpus.rs`), i.e. the port creates the reference's teardrops.
//! * Quick DRC with the clearance and connectivity providers equals the full DRC's
//!   violations of those providers.

mod common;

use kicad_session::board_api::check::QuickDrcOptions;
use kicad_session::board_api::edit::{EditBatch, ItemChange, NewItem, NewTrack, NewVia};
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat};
use kicad_session::{HostClock, LoadInputs, Reporter, Session, run_drc};

fn load(rel: &str) -> Session {
    let path = kicad_qa::kicad_source_root().join(rel);
    let text = std::fs::read_to_string(&path).expect("board");
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
    let mut s = common::load_staged(kicad_pcb::project::LoadPath::Cli, &inputs)
        .expect("load")
        .0;
    s.apply_host_inputs(&HostClock::parse_iso("2026-01-01T00:00:00").unwrap(), 1);
    s
}

/// The teardrop zones of a session: (layers, net, outline) sorted. The UUIDs are left out:
/// the cached reference boards carry random teardrop UUIDs (made before
/// `TEARDROP_MANAGER::teardropUuid` derived them from the track and pad).
fn teardrops(s: &Session) -> Vec<(Vec<String>, i32, String)> {
    let mut v: Vec<_> = s
        .zones_info(false)
        .into_iter()
        .filter(|z| z.teardrop)
        .map(|z| (z.layers, z.net, format!("{:?}", z.outline)))
        .collect();
    v.sort();
    v
}

/// `update_teardrops` against the reference: the teardrop zones of
/// `kicad-cli pcb drc --refill-zones --save-board` (`FillAllZones` runs the full
/// `UpdateTeardrops` first) cached by `rust/tools/zone-corpus/run.sh`. Skips silently
/// without the cache.
#[test]
fn teardrop_update_matches_the_reference() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zone-corpus");
    let Ok(dir) = std::fs::read_dir(&root) else {
        return;
    };
    let mut cases: Vec<_> = dir
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains("teardrop") && p.join("ref").is_dir())
        .collect();
    cases.sort();
    let mut compared = 0;
    let mut failures = Vec::new();

    let load_file = |p: &std::path::Path| -> Option<Session> {
        let text = std::fs::read_to_string(p).ok()?;
        let name = p.to_string_lossy().into_owned();
        let inputs = LoadInputs {
            text: &text,
            source_name: &name,
            ..LoadInputs::default()
        };
        common::load_staged(kicad_pcb::project::LoadPath::Cli, &inputs)
            .ok()
            .map(|(s, _)| s)
    };

    for case in cases {
        let Some(board) = std::fs::read_dir(case.join("in"))
            .ok()
            .and_then(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                p.extension().is_some_and(|e| e == "kicad_pcb")
            }))
        else {
            continue;
        };
        let reference = case.join("ref").join(board.file_name().unwrap());
        let (Some(mut s), Some(r)) = (load_file(&board), load_file(&reference)) else {
            continue;
        };
        let outcome = s.update_teardrops().unwrap();
        // Compared as saved (and re-read): the teardrop zones the update created.
        let saved = s.save().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "board_api_td_{}_{}.kicad_pcb",
            std::process::id(),
            compared
        ));
        std::fs::write(&tmp, &saved).unwrap();
        let reread = load_file(&tmp);
        let _ = std::fs::remove_file(&tmp);
        let mine = teardrops(&reread.expect("re-read"));
        let theirs = teardrops(&r);
        compared += 1;

        if outcome.teardrops as usize != theirs.len() || mine != theirs {
            let diff = mine
                .iter()
                .zip(&theirs)
                .find(|(a, b)| a != b)
                .map(|(a, b)| format!("\n  mine {a:?}\n  ref  {b:?}"))
                .unwrap_or_default();
            failures.push(format!(
                "{}: {} teardrops, reference {}{}",
                case.display(),
                mine.len(),
                theirs.len(),
                &diff[..diff.len().min(1200)]
            ));
        }
    }

    eprintln!("compared the teardrops of {compared} boards");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn teardrop_settings_round_trip_and_item_switch() {
    let mut s = load("qa/data/pcbnew/teardrop_spike.kicad_pcb");
    let mut settings = s.teardrop_settings();
    settings.round.max_len += 1;
    s.set_teardrop_settings(&settings);
    assert_eq!(s.teardrop_settings(), settings);

    let vias: Vec<u64> = s
        .route_items()
        .iter()
        .filter(|r| r.kind == "via")
        .map(|r| r.id)
        .collect();
    let changed = s.set_item_teardrops(&vias, false).unwrap();
    assert_eq!(changed, vias);
    assert!(
        s.route_items()
            .iter()
            .filter(|r| r.kind == "via")
            .all(|r| !r.via.as_ref().unwrap().teardrops.enabled)
    );
}

fn full_drc(s: &mut Session) -> kicad_session::board_api::check::DrcSummary {
    let mut job = JobPcbDrc::new();
    job.rc.format = RcOutputFormat::Json;
    job.report_date = "2026-01-01T00:00:00+0000".to_string();
    let mut reporter = Reporter::new();
    run_drc(s, &mut job, &mut reporter).expect("drc");
    s.drc_violations(job.rc.severity)
}

#[test]
fn quick_drc_equals_full_drc_for_its_providers() {
    for b in [
        "demos/ecc83/ecc83-pp.kicad_pcb",
        "qa/data/pcbnew/severities.kicad_pcb",
        "qa/data/pcbnew/connection_width_rules.kicad_pcb",
    ] {
        let mut s = load(b);
        let full = full_drc(&mut s);
        let quick = s
            .quick_drc(&QuickDrcOptions {
                providers: vec!["clearance".into(), "connectivity".into()],
                ..QuickDrcOptions::default()
            })
            .unwrap();
        // The connectivity provider reports unconnected items, dangling tracks / vias and
        // isolated copper; the clearance provider the copper clearance family.
        let types: std::collections::BTreeSet<&str> = quick
            .violations
            .iter()
            .map(|v| v.violation_type.as_str())
            .collect();
        let expected: Vec<_> = full
            .violations
            .iter()
            .filter(|v| types.contains(v.violation_type.as_str()))
            .collect();
        let got: Vec<_> = quick.violations.iter().collect();
        assert_eq!(got, expected, "{b}");
        assert_eq!(quick.unconnected, full.unconnected, "{b}");
    }
}

#[test]
fn edits_change_the_board() {
    let mut s = load("demos/ecc83/ecc83-pp.kicad_pcb");
    let tracks = s.route_items();
    let t = tracks.iter().find(|t| t.kind == "track").unwrap().clone();
    let other_net = s.net_details().iter().find(|n| n.code > 0 && n.code != t.net).unwrap().code;

    let r = s
        .change(&ItemChange {
            id: t.id,
            width: Some(t.width + 1000),
            layer: Some("F.Cu".into()),
            locked: Some(true),
            end: Some([t.end[0] + 100, t.end[1]]),
            ..ItemChange::default()
        })
        .unwrap();
    assert_eq!(r.changed, vec![t.id]);
    let now = s.route_item(t.id).unwrap();
    assert_eq!(now.width, t.width + 1000);
    assert_eq!(now.layer, "F.Cu");
    assert!(now.locked);
    assert_eq!(now.end, [t.end[0] + 100, t.end[1]]);

    // Additions with router defaults.
    let added = s
        .apply_edits(&EditBatch {
            add: vec![
                NewItem::Track(NewTrack {
                    start: [100_000_000, 100_000_000],
                    end: [101_000_000, 100_000_000],
                    layer: "B.Cu".into(),
                    net: other_net,
                    ..NewTrack::default()
                }),
                NewItem::Via(NewVia {
                    at: [101_000_000, 100_000_000],
                    net: other_net,
                    ..NewVia::default()
                }),
            ],
            ..EditBatch::default()
        })
        .unwrap();
    assert_eq!(added.added.len(), 2);
    let sizes = s.router_sizes(other_net, "B.Cu").unwrap();
    let new_track = s.route_item(added.added[0]).unwrap();
    let new_via = s.route_item(added.added[1]).unwrap();
    assert_eq!(new_track.width, sizes.track_width);
    assert_eq!(new_via.width, sizes.via_diameter);
    assert_eq!(new_via.via.as_ref().unwrap().drill, sizes.via_drill);

    // SetNetCode on an isolated track sticks (a track touching pads gets their net back
    // from the connectivity rebuild's net propagation, as in KiCad).
    s.change(&ItemChange {
        id: added.added[0],
        net: Some(t.net),
        ..ItemChange::default()
    })
    .unwrap();
    assert_eq!(s.route_item(added.added[0]).unwrap().net, t.net);

    // Errors leave the board as it was.
    let before = s.route_items();
    assert!(s.remove_items(&[t.id, 1 << 60]).is_err());
    assert!(
        s.apply_edits(&EditBatch {
            add: vec![NewItem::Track(NewTrack {
                layer: "F.SilkS".into(),
                width: Some(1),
                ..NewTrack::default()
            })],
            ..EditBatch::default()
        })
        .is_err()
    );
    assert_eq!(s.route_items(), before);

    // Duplicate UUID refused.
    assert!(
        s.apply_edits(&EditBatch {
            add: vec![NewItem::Track(NewTrack {
                layer: "F.Cu".into(),
                width: Some(1),
                uuid: Some(t.uuid.clone()),
                ..NewTrack::default()
            })],
            ..EditBatch::default()
        })
        .is_err()
    );

    let removed = s.remove_items(&[t.id]).unwrap();
    assert_eq!(removed.removed, vec![t.id]);
    assert!(s.route_item(t.id).is_none());

    // Connectivity after the edits.
    let c = s.connectivity_info();
    assert_eq!(c.unconnected as usize, c.edges.len());
}
