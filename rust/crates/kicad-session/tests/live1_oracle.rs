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

//! Package LIVE-1, verification b: the transaction layer against the reference's incremental
//! connectivity. `tests/oracle/live1_oracle.cpp` (built and run by
//! `tests/oracle/gen_live1_golden.sh`) replays the scripts `tests/golden/live1/<board>.script`
//! the way `BOARD_COMMIT::Push` applies a commit and prints one hash per fact section after
//! the load and after every commit (`<board>.golden`); this test replays the same scripts
//! through `Session::commit` and compares the hashes. `LIVE1_WRITE_SCRIPTS=1` regenerates the
//! scripts (10 boards x 20 seeded batches).

mod live1_common;

use std::collections::BTreeMap;

use live1_common::*;

const BOARDS: usize = 10;
const BATCHES: usize = 20;

#[test]
fn live1_write_scripts() {
    if std::env::var("LIVE1_WRITE_SCRIPTS").is_err() {
        return;
    }

    std::fs::create_dir_all(golden_dir()).unwrap();

    for (bi, rel) in boards().iter().take(BOARDS).enumerate() {
        let s = load(rel).expect("load");
        let text = script(&s, 0x33fe_0001 + bi as u64, BATCHES);
        std::fs::write(golden_dir().join(format!("{}.script", stem(rel))), text).unwrap();
    }
}

/// `<commit>|<section>|<hash>` lines of the replay of one script (without `rn_edges`), and
/// the ratsnest edges per net after every commit.
fn replay_full(rel: &str, script: &str) -> (Vec<String>, Vec<BTreeMap<i32, Vec<EdgeRec>>>) {
    let mut s = load(rel).expect("load");
    let mut handles = uuid_handles(&s);
    let mut hashes = Vec::new();
    let mut edges = Vec::new();
    let emit = |k: usize, s: &kicad_session::Session, hashes: &mut Vec<String>, edges: &mut Vec<_>| {
        let (e, facts) = split_edges(&fact_lines(&s.board, &mut s.connectivity.clone()));
        for (sec, h) in section_hashes(&facts) {
            hashes.push(format!("{k}|{sec}|{h}"));
        }
        edges.push(e);
    };

    let mut commit = 0;
    emit(commit, &s, &mut hashes, &mut edges);
    let mut ops = Vec::new();

    for line in script.lines() {
        match parse_op(line) {
            Some(op) => ops.push(op),
            None => {
                let report = s.commit(&transaction(&ops, &handles)).expect("commit");
                for (u, &h) in report.edit.added_uuids.iter().zip(&report.edit.added) {
                    handles.insert(u.clone(), h);
                }
                ops.clear();
                commit += 1;
                emit(commit, &s, &mut hashes, &mut edges);
            }
        }
    }

    (hashes, edges)
}

/// `<commit>|<fact line>` of the replay of one script (debugging, `LIVE1_LINES`).
fn replay(rel: &str, script: &str, lines: bool) -> Vec<String> {
    let mut s = load(rel).expect("load");
    let mut handles = uuid_handles(&s);
    let mut out = Vec::new();
    let emit = |k: usize, s: &kicad_session::Session, out: &mut Vec<String>| {
        let facts = fact_lines(&s.board, &mut s.connectivity.clone());
        if lines {
            out.extend(facts.iter().map(|l| format!("{k}|{l}")));
        } else {
            for (sec, h) in section_hashes(&facts) {
                out.push(format!("{k}|{sec}|{h}"));
            }
        }
    };

    let mut commit = 0;
    emit(commit, &s, &mut out);
    let mut ops = Vec::new();

    for line in script.lines() {
        match parse_op(line) {
            Some(op) => ops.push(op),
            None => {
                let report = s.commit(&transaction(&ops, &handles)).expect("commit");
                for (u, &h) in report.edit.added_uuids.iter().zip(&report.edit.added) {
                    handles.insert(u.clone(), h);
                }
                ops.clear();
                commit += 1;
                emit(commit, &s, &mut out);
            }
        }
    }

    out
}

#[test]
fn live1_incremental_matches_the_reference() {
    let mut compared = 0;
    let mut failures = Vec::new();

    for rel in boards().iter().take(BOARDS) {
        let name = stem(rel);
        let (Ok(script), Ok(golden)) = (
            std::fs::read_to_string(golden_dir().join(format!("{name}.script"))),
            std::fs::read_to_string(golden_dir().join(format!("{name}.golden"))),
        ) else {
            failures.push(format!("{name}: script or golden missing"));
            continue;
        };

        let mut golden_hashes: BTreeMap<(usize, String), String> = BTreeMap::new();
        let mut golden_edges: BTreeMap<usize, BTreeMap<i32, Vec<EdgeRec>>> = BTreeMap::new();

        for l in golden.lines() {
            let f: Vec<&str> = l.splitn(4, '|').collect();
            match f.as_slice() {
                [k, "rn_edges", net, edges] => {
                    golden_edges
                        .entry(k.parse().unwrap())
                        .or_default()
                        .insert(net.parse().unwrap(), parse_edges(edges));
                }
                [k, sec, h] => {
                    golden_hashes.insert((k.parse().unwrap(), sec.to_string()), h.to_string());
                }
                _ => failures.push(format!("{name}: bad golden line {l}")),
            }
        }

        let (hashes, edges) = replay_full(rel, &script);
        let rust: BTreeMap<(usize, String), String> = hashes
            .iter()
            .map(|l| {
                let f: Vec<&str> = l.split('|').collect();
                ((f[0].parse().unwrap(), f[1].to_string()), f[2].to_string())
            })
            .collect();

        for (key, h) in &golden_hashes {
            compared += 1;
            if rust.get(key) != Some(h) {
                failures.push(format!("{name}: commit {} section {} differs", key.0, key.1));
            }
        }

        if rust.len() != golden_hashes.len() {
            failures.push(format!("{name}: {} rust vs {} reference sections", rust.len(), golden_hashes.len()));
        }

        let none = BTreeMap::new();
        for (k, nets) in edges.iter().enumerate() {
            compared += nets.len();
            for n in edge_differences(golden_edges.get(&k).unwrap_or(&none), nets) {
                failures.push(format!("{name}: commit {k} ratsnest edges of net {n} differ"));
            }
        }
    }

    // LIVE1_LINES=<board stem> prints the fact lines of the Rust replay (to diff against
    // `LIVE1_LINES=1 live1_oracle`).
    if let Ok(only) = std::env::var("LIVE1_LINES") {
        for rel in boards().iter().take(BOARDS).filter(|r| stem(r) == only) {
            let script = std::fs::read_to_string(golden_dir().join(format!("{only}.script"))).unwrap();
            for l in replay(rel, &script, true) {
                println!("{l}");
            }
        }
    }

    eprintln!("live1 oracle: {compared} section hashes compared, {} differ", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn live1_debug_edges() {
    let Ok(spec) = std::env::var("LIVE1_EDGES") else {
        return;
    };
    let (name, k, net) = {
        let f: Vec<&str> = spec.split(':').collect();
        (f[0].to_string(), f[1].parse::<usize>().unwrap(), f[2].parse::<i32>().unwrap())
    };
    let rel = boards().into_iter().find(|r| stem(r) == name).unwrap();
    let script = std::fs::read_to_string(golden_dir().join(format!("{name}.script"))).unwrap();
    let (_, edges) = replay_full(&rel, &script);
    println!("{:?}", edges[k].get(&net));
    let golden = std::fs::read_to_string(golden_dir().join(format!("{name}.golden"))).unwrap();
    let prefix = format!("{k}|rn_edges|{net}|");
    for l in golden.lines().filter(|l| l.starts_with(&prefix)) {
        println!("{:?}", parse_edges(&l[prefix.len()..]));
    }
}
