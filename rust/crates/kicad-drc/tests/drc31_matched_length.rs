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

//! DRC-31: the matched length provider (`drc_test_provider_matched_length.cpp`), the length
//! calculation (`length_delay_calculation.cpp`), net chain topology and bridging.
//!
//! * Reference: `tests/golden/drc31/reference.txt` (`tests/oracle/gen_drc31_golden.py`): the
//!   `length_out_of_range`, `skew_out_of_range`, `too_many_vias`, `net_chain_stub_length` and
//!   `net_chain_return_path` entries of `kicad-cli pcb drc --format json --severity-all
//!   --units mm` for the cases of `tests/data/drc31/cases.tsv` (QA boards of
//!   `qa/data/pcbnew`, with their own rules, rule variants appended or replacing them, and net
//!   chains inserted as the C++ QA tests assign them in code).
//! * Rust: the same board and rules through the real run path (`run_flow`) with the `length`
//!   provider only.
//!
//! Comparison: the C++ groups items in `std::set<BOARD_CONNECTED_ITEM*>` and rules in
//! `std::map<DRC_RULE*, ...>` (address order). So the entries are compared as a multiset; the
//! report shows only the first two items of an entry (address order), so the items are
//! compared by count, and the single item of a skew / via count entry (the last of the net's
//! items in address order) by its net.

mod drc_harness;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use drc_harness::{Entry, cli_report_entries_flow, load_board};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/drc31")
}

/// The board with the case's chains and rules in `dir` (as the generator writes it).
fn materialize(case: &str, rel: &str, mode: &str, chains: &str, dir: &Path) -> PathBuf {
    let src = kicad_qa::kicad_source_root()
        .join("qa/data/pcbnew")
        .join(format!("{rel}.kicad_pcb"));
    let stem = src.file_stem().unwrap().to_string_lossy().to_string();
    let work = dir.join(case.replace(['/', '#', ':', '.'], "_"));
    std::fs::create_dir_all(&work).unwrap();

    let mut text = std::fs::read_to_string(&src).unwrap();

    if chains != "-" {
        let block = std::fs::read_to_string(data_dir().join(chains)).unwrap();
        let end = text.trim_end().rfind(')').unwrap();
        text.insert_str(end, &block);
    }

    std::fs::write(work.join(format!("{stem}.kicad_pcb")), text).unwrap();

    let pro = src.with_extension("kicad_pro");

    if pro.exists() {
        std::fs::copy(&pro, work.join(format!("{stem}.kicad_pro"))).unwrap();
    }

    let own_dru = src.with_extension("kicad_dru");
    let dru = work.join(format!("{stem}.kicad_dru"));

    let rules = if mode == "base" {
        std::fs::read_to_string(&own_dru).ok()
    } else {
        let (kind, variant) = mode.split_once(':').unwrap();
        let extra = std::fs::read_to_string(data_dir().join(format!("{variant}.dru"))).unwrap();

        if kind == "append" {
            let base = std::fs::read_to_string(&own_dru).unwrap_or_else(|_| "(version 1)\n".into());
            Some(format!("{base}\n{extra}"))
        } else {
            Some(format!("(version 1)\n{extra}"))
        }
    };

    if let Some(rules) = rules {
        std::fs::write(&dru, rules).unwrap();
    }

    work.join(format!("{stem}.kicad_pcb"))
}

fn golden() -> BTreeMap<String, Vec<Entry>> {
    let mut out: BTreeMap<String, Vec<Entry>> = BTreeMap::new();

    for line in include_str!("golden/drc31/reference.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let entries = out.entry(f[0].to_string()).or_default();

        if f[1] == "none" {
            continue;
        }

        entries.push(Entry {
            severity: f[1].trim_end_matches("/excluded").to_string(),
            excluded: f[1].ends_with("/excluded"),
            type_: f[2].to_string(),
            description: f[3].to_string(),
            items: f[4..]
                .chunks(4)
                .map(|it| {
                    (
                        it[0].to_string(),
                        it[1].to_string(),
                        it[2].parse().unwrap(),
                        it[3].parse().unwrap(),
                    )
                })
                .collect(),
        });
    }

    out
}

fn net_of(description: &str) -> &str {
    let start = description.find('[').map_or(0, |i| i + 1);
    let end = description[start..]
        .find(']')
        .map_or(description.len(), |i| start + i);
    &description[start..end]
}

/// The comparison key. The report shows only the first two items of an entry, which are the
/// first in address order in C++: the items of a length / stub / return path entry are
/// compared by count (their description carries the measured values and, for stubs, the
/// net), the single item of a skew / via count entry by its net.
fn key(e: &Entry) -> String {
    let mut e = e.clone();

    if e.type_ == "skew_out_of_range" || e.type_ == "too_many_vias" {
        for it in e.items.iter_mut() {
            *it = (
                String::new(),
                format!("item of net {}", net_of(&it.1)),
                0.0,
                0.0,
            );
        }
    } else {
        let n = e.items.len();
        e.items = vec![(String::new(), format!("{n} items"), 0.0, 0.0)];
    }

    e.key()
}

fn diff(reference: &[Entry], mine: &[Entry]) -> (usize, Vec<String>, Vec<String>) {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();

    for e in reference {
        *counts.entry(key(e)).or_default() += 1;
    }

    let mut extra = Vec::new();
    let mut matched = 0;

    for e in mine {
        let c = counts.entry(key(e)).or_default();

        if *c > 0 {
            *c -= 1;
            matched += 1;
        } else {
            extra.push(key(e));
        }
    }

    let missing = counts
        .into_iter()
        .flat_map(|(k, n)| std::iter::repeat_n(k, n.max(0) as usize))
        .collect();

    (matched, missing, extra)
}

const TYPES: &[&str] = &[
    "length_out_of_range",
    "skew_out_of_range",
    "too_many_vias",
    "net_chain_stub_length",
    "net_chain_return_path",
];

#[test]
fn drc31_reference_cases() {
    let dir = std::env::temp_dir().join(format!("kicad-drc31-{}", std::process::id()));
    let golden = golden();
    let mut failures = Vec::new();
    let mut per_type: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut cases = 0;

    for line in include_str!("data/drc31/cases.tsv").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (case, rel, mode, chains) = (f[0], f[1], f[2], f[3]);
        let reference = golden.get(case).cloned().unwrap_or_default();
        let path = materialize(case, rel, mode, chains, &dir);

        let mut loaded = match load_board(&path) {
            Ok(l) => l,
            Err(e) => {
                failures.push(format!("{case}: load failed: {e}"));
                continue;
            }
        };

        let mine: Vec<Entry> = cli_report_entries_flow(&mut loaded, false, &["length"])
            .into_iter()
            .filter(|e| TYPES.contains(&e.type_.as_str()))
            .collect();

        cases += 1;
        let (_, missing, extra) = diff(&reference, &mine);

        for t in TYPES {
            let r: Vec<Entry> = reference
                .iter()
                .filter(|e| e.type_ == *t)
                .cloned()
                .collect();
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, mi, ex) = diff(&r, &m);
            let s = per_type.entry(t.to_string()).or_default();
            s.0 += matched;
            s.1 += mi.len();
            s.2 += ex.len();
        }

        for k in missing.iter().take(4) {
            failures.push(format!("{case}: - {k}"));
        }

        for k in extra.iter().take(4) {
            failures.push(format!("{case}: + {k}"));
        }
    }

    println!("DRC-31 reference cases: {cases}");

    for (t, (m, mi, ex)) in &per_type {
        println!("  {t}: matched {m}, missing {mi}, extra {ex}");
    }

    let _ = std::fs::remove_dir_all(&dir);

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Regression (perf gate, found on `vme-wren`): the provider held the from-to cache's lock
/// while evaluating the length rules, and a `fromTo()` condition locks it again, so the run
/// deadlocked. A `fromTo` length rule on a small board must finish.
#[test]
fn from_to_rule_condition_does_not_deadlock() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("drc31_fromto_lock");
    let path = materialize(
        "via_stack_track_width#fromto_lock",
        "via_stack_track_width",
        "replace:fromto_lock",
        "-",
        &dir,
    );
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut loaded = load_board(&path).expect("board loads");
        let entries = cli_report_entries_flow(&mut loaded, false, &["length"]);
        let _ = tx.send(entries.len());
    });

    rx.recv_timeout(std::time::Duration::from_secs(120))
        .expect("the length provider finished (no deadlock on the from-to cache)");
}
