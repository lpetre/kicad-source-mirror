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

//! Package LIVE-1, verification a and c (no C++ test applies):
//!
//! * a: incremental == full. On the boards of `tests/oracle/live1_boards.txt`, seeded random
//!   batches through `Session::commit`; after every commit the live connectivity is compared
//!   with a fresh `BuildConnectivity` on a clone of the board: nets of every item, cluster
//!   partition, ratsnest nodes and edges with weights (and the edge order of nets without a
//!   tie), unconnected counts, dangling ends, `IsConnectedOnLayer` of every via and pad layer,
//!   the isolated-island map. Default run: 20 batches per board; `LIVE1_BATCHES=200` for the
//!   full campaign.
//! * c: checkpoints. After a sequence of commits, `rollback` gives a board whose save equals
//!   the base save, whose connectivity facts equal the base ones, and a second run of the same
//!   batches gives the same reports, facts and saves (UUIDs included).

mod live1_common;

use live1_common::*;

fn batches() -> usize {
    std::env::var("LIVE1_BATCHES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
}

/// Cases where the reference itself gives different connectivity incrementally and after a
/// full rebuild (`BOARD::BuildConnectivity` depends on the build history: list order of the
/// net propagation / via net deferral), with the port equal to the reference on both sides.
/// Classified with the reference driver (see [`classify`]); the default run (20 batches)
/// meets these.
const PROVEN_HISTORY: &[(&str, usize)] = &[
    ("qa/data/pcbnew/odbpp/two_board_islands.kicad_pcb", 11),
    ("qa/data/pcbnew/issue8407.kicad_pcb", 11),
];

/// The reference driver, if built (`tests/oracle/gen_live1_golden.sh` with
/// `KEEP=<rust>/target/live1`, or `LIVE1_ORACLE_BIN`).
fn oracle_bin() -> Option<std::path::PathBuf> {
    let p = std::env::var("LIVE1_ORACLE_BIN").map(Into::into).unwrap_or_else(|_| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/live1/live1_oracle")
    });
    p.exists().then_some(p)
}

/// Runs the reference driver on `script` (the batches so far, then `F`) and checks that the
/// reference's incremental state equals the port's (`live`) and its full rebuild the port's
/// (`full`): then the difference is the reference's own history dependence. `None` without
/// the driver.
fn classify(rel: &str, script: &str, k: usize, live: &[String], full: &[String]) -> Option<bool> {
    let bin = oracle_bin()?;
    let dir = std::env::temp_dir().join(format!("live1-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("home")).ok()?;
    let board = dir.join(format!("{}.kicad_pcb", stem(rel)));
    std::fs::copy(board_path(rel), &board).ok()?;
    let script_path = dir.join("script.txt");
    std::fs::write(&script_path, format!("{script}F\n")).ok()?;
    let out = std::process::Command::new(bin)
        .arg(&board)
        .arg(&script_path)
        .env("LIVE1_LINES", "1")
        .env("HOME", dir.join("home"))
        .output()
        .ok()?;
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout);
    let commit = |c: usize| -> Vec<String> {
        let p = format!("{c}|");
        let mut v: Vec<String> =
            text.lines().filter_map(|l| l.strip_prefix(&p)).map(String::from).collect();
        v.sort();
        v
    };
    let same = |cpp: Vec<String>, rust: &[String]| {
        let (ce, cr) = split_edges(&cpp);
        let (re, rr) = split_edges(rust);
        cr == rr && edge_differences(&ce, &re).is_empty()
    };
    let (ci, cf) = (commit(k + 1), commit(k + 2));
    let reference_differs = ci != cf;
    Some(reference_differs && same(ci, live) && same(cf, full))
}

fn full_facts(s: &kicad_session::Session) -> (Vec<String>, Vec<String>) {
    let mut board = s.board.clone();
    let mut conn = kicad_connectivity::data::ConnectivityData::new();
    conn.build_connectivity(&mut board);
    let a = fact_lines(&board, &mut conn.clone());
    let b = port_only_lines(&board, &mut conn);
    (a, b)
}

fn live_facts(s: &kicad_session::Session) -> (Vec<String>, Vec<String>) {
    let a = fact_lines(&s.board, &mut s.connectivity.clone());
    let b = port_only_lines(&s.board, &mut s.connectivity.clone());
    (a, b)
}

#[test]
fn live1_incremental_equals_full_build() {
    let n = batches();
    let mut commits = 0;
    let mut history = 0;
    let mut failures = Vec::new();

    let only = std::env::var("LIVE1_BOARD").ok();

    for (bi, rel) in boards().iter().enumerate() {
        if only.as_ref().is_some_and(|o| !rel.contains(o.as_str())) {
            continue;
        }

        let Some(mut s) = load(rel) else {
            failures.push(format!("{rel}: load failed"));
            continue;
        };
        let mut handles = uuid_handles(&s);
        let mut generator = Gen::new(&s, 0x11fe_0001 + bi as u64);
        let mut script = String::new();

        for k in 0..n {
            let ops = generator.batch();
            for op in &ops {
                script.push_str(&op.line());
                script.push('\n');
            }
            script.push_str("C\n");
            let txn = transaction(&ops, &handles);
            let report = match s.commit(&txn) {
                Ok(r) => r,
                Err(e) => {
                    failures.push(format!("{rel} batch {k}: commit failed: {e}"));
                    break;
                }
            };
            for (u, &h) in report.edit.added_uuids.iter().zip(&report.edit.added) {
                handles.insert(u.clone(), h);
            }
            commits += 1;

            let (la, lb) = live_facts(&s);
            let (fa, fb) = full_facts(&s);
            let (le, lr) = split_edges(&la);
            let (fe, fr) = split_edges(&fa);
            let bad_nets = edge_differences(&le, &fe);

            if lr != fr || lb != fb || !bad_nets.is_empty() {
                if PROVEN_HISTORY.contains(&(rel.as_str(), k)) {
                    history += 1;
                    break;
                }

                match classify(rel, &script, k, &la, &fa) {
                    Some(true) => {
                        history += 1;
                        eprintln!("{rel} batch {k}: reference history dependence (driver)");
                        break;
                    }
                    Some(false) => eprintln!("{rel} batch {k}: the driver does not explain it"),
                    None => {}
                }

                // LIVE1_FAIL_DIR: the batches so far as a script for the reference driver
                // (ending with a full rebuild, `F`) and the port's lines of the last commit
                // (`k+1|...`) and of the full rebuild (`k+2|...`), for `diff`.
                if let Ok(dir) = std::env::var("LIVE1_FAIL_DIR") {
                    let st = stem(rel);
                    let dir = std::path::Path::new(&dir);
                    std::fs::write(dir.join(format!("{st}.script")), format!("{script}F\n")).unwrap();
                    let mut text = String::new();
                    for (c, lines) in [(k + 1, &la), (k + 2, &fa)] {
                        for l in lines.iter() {
                            text.push_str(&format!("{c}|{l}\n"));
                        }
                    }
                    std::fs::write(dir.join(format!("{st}.rust")), text).unwrap();
                }
                let max = if only.is_some() { 60 } else { 6 };
                let mut d = diff(&lr, &fr, max);
                d.extend(diff(&lb, &fb, max));
                for n in &bad_nets {
                    d.push(format!("edges of net {n}: {:?} vs {:?}", le.get(n), fe.get(n)));
                }
                let ops: Vec<String> = ops.iter().map(Op::line).collect();
                failures.push(format!(
                    "{rel} batch {k}: incremental != full\n  ops: {}\n  {}",
                    ops.join("; "),
                    d.join("\n  ")
                ));
                break;
            }

            if report.unconnected != s.connectivity.unconnected_count(false) {
                failures.push(format!("{rel} batch {k}: report unconnected count"));
                break;
            }
        }
    }

    eprintln!(
        "live1 incremental: {commits} commits compared, {history} boards stopped at a proven \
         reference history dependence, {} failures",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn live1_rollback_restores_the_base_state_exactly() {

    for (bi, rel) in boards().iter().take(12).enumerate() {
        let mut s = load(rel).expect("load");
        let base_save = s.save().expect("save");
        let base_facts = live_facts(&s);
        let base_uuids = uuid_handles(&s);
        let cp = s.checkpoint();

        let run = |s: &mut kicad_session::Session| {
            let mut handles = uuid_handles(s);
            let mut generator = Gen::new(s, 0x22fe_0001 + bi as u64);
            let mut trace = Vec::new();
            for _ in 0..8 {
                let ops = generator.batch();
                let r = s.commit(&transaction(&ops, &handles)).expect("commit");
                for (u, &h) in r.edit.added_uuids.iter().zip(&r.edit.added) {
                    handles.insert(u.clone(), h);
                }
                trace.push(format!("{r:?}"));
                trace.extend(live_facts(s).0);
            }
            // items without an explicit UUID draw from the board's generator
            let r = s
                .apply_edits(&kicad_session::board_api::edit::EditBatch {
                    add: vec![kicad_session::board_api::edit::NewItem::Track(
                        kicad_session::board_api::edit::NewTrack {
                            start: [0, 0],
                            end: [1_000_000, 0],
                            width: Some(200_000),
                            layer: "F.Cu".into(),
                            ..Default::default()
                        },
                    )],
                    ..Default::default()
                })
                .expect("generated uuid");
            trace.push(format!("{r:?}"));
            trace.push(s.save().expect("save"));
            trace
        };

        let first = run(&mut s);
        s.rollback(&cp);
        assert_eq!(s.save().expect("save"), base_save, "{rel}: save after rollback");
        assert_eq!(live_facts(&s), base_facts, "{rel}: connectivity after rollback");
        assert_eq!(uuid_handles(&s), base_uuids, "{rel}: handles after rollback");
        assert!(!s.connectivity.is_stale(&s.board), "{rel}: connectivity stale");

        let second = run(&mut s);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert!(a == b, "{rel}: second run differs");
        }

        s.restore(cp);
        assert_eq!(s.save().expect("save"), base_save, "{rel}: save after restore");
    }
}
