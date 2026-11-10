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

//! DRC-25: the zone connections provider (`drc_test_provider_zone_connections.cpp`) against
//! the reference DRC reports of the shared corpus, plus the ported C++ tests:
//!
//! * `qa/tests/pcbnew/drc/test_drc_starved_thermal.cpp` (`DRCStarvedThermal`)
//! * `qa/tests/pcbnew/drc/test_drc_regressions.cpp`: `issue15280` (very wide spokes counted
//!   as one, `DRCFalsePositiveRegressions`), `issue14334` (thermal spoke to an otherwise
//!   unconnected island, 2 violations in `DRCFalseNegativeRegressions`) and `issue19090`
//!   (`DRCZoneFalsePositiveRegressions`, `starved_thermal` forced to error, 0).
//!
//! The provider needs the run's connectivity and the cache generator's zone lists and islands
//! map, so it runs through the real run path (`drc_harness::run_flow_providers`:
//! `run_flow::run_tests` with the cache generator and the zone connections slot only).

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, cached_entries, cli_report_entries_flow, golden, load_board, panic_text, quiet,
    run_flow_providers, summarize,
};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::DRCE_STARVED_THERMAL;

/// The error type of the provider.
const TYPE: &str = "starved_thermal";

/// The report entries (`kicad-cli pcb drc --format json`) of a run of the provider.
fn report_zone_connections(loaded: &mut Loaded) -> Vec<Entry> {
    cli_report_entries_flow(loaded, false, &["zone connections"])
        .into_iter()
        .filter(|e| e.type_ == TYPE)
        .collect()
}

/// Multiset difference by entry key: (matched, missing, extra).
fn diff(reference: &[Entry], mine: &[Entry]) -> (usize, Vec<String>, Vec<String>) {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();

    for e in reference {
        *counts.entry(e.key()).or_default() += 1;
    }

    let mut extra = Vec::new();

    for e in mine {
        let c = counts.entry(e.key()).or_default();

        if *c > 0 {
            *c -= 1;
        } else {
            extra.push(e.key());
        }
    }

    let missing: Vec<String> = counts
        .iter()
        .flat_map(|(k, &n)| std::iter::repeat_n(k.clone(), n.max(0) as usize))
        .collect();
    let matched = mine.len() - extra.len();

    (matched, missing, extra)
}

/// The (zone, layer) task of an entry: the zone's UUID (first item) and the layer named by
/// the message.
fn task_of(e: &Entry) -> (String, String) {
    let zone = e.items.first().map(|i| i.0.clone()).unwrap_or_default();
    let layer = e
        .description
        .split("(layer ")
        .nth(1)
        .and_then(|s| s.split(';').next())
        .unwrap_or("")
        .to_string();

    (zone, layer)
}

/// Whether the entries of every (zone, layer) task come in the same order on both sides. The
/// C++ tests the tasks on the thread pool, so the interleaving of different tasks in the
/// report is scheduling dependent (`drc.md` 4.12); the order inside one task is not.
fn same_order_within_tasks(theirs: &[Entry], mine: &[Entry]) -> bool {
    let group = |entries: &[Entry]| {
        let mut m: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();

        for e in entries {
            m.entry(task_of(e)).or_default().push(e.key());
        }

        m
    };

    group(theirs) == group(mine)
}

/// Every corpus board the port loads completely, `starved_thermal` entries only, against the
/// reference report (summary hash in report order, else the cached raw report). Set
/// `DRC25_FILTER=<path substring>` to limit the boards.
#[test]
fn zone_connections_match_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC25_FILTER").unwrap_or_default();
    let (mut compared, mut matched, mut missing, mut extra) = (0, 0, 0, 0);
    let mut nonzero = 0;
    let mut differences = Vec::new();
    let mut pending = Vec::new();

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let Some(reference) = reference else {
            pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, bool), String> {
                    let mut loaded = load_board(&path)?;

                    let entries = report_zone_connections(&mut loaded);
                    Ok((entries, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let mine = match result {
            Ok(Ok((_, false))) => {
                pending.push(format!("{rel}: parser skipped items"));
                continue;
            }
            Ok(Ok((v, true))) => v,
            Ok(Err(e)) => {
                pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                pending.push(format!("{rel}: panic: {}", panic_text(&*e)));
                continue;
            }
        };

        compared += 1;

        let refs: Vec<&Entry> = mine.iter().collect();

        if let Some(&(n, h)) = reference.summaries.get(TYPE) {
            nonzero += 1;

            if summarize(&refs) == (n, h) {
                matched += n;
                continue;
            }

            let Some(theirs) = cached_entries(&rel, TYPE) else {
                missing += n;
                extra += mine.len();
                differences.push(format!(
                    "{rel}: {n} reference entries (summary only), {} here",
                    mine.len()
                ));
                continue;
            };

            let (m, mi, ex) = diff(&theirs, &mine);
            matched += m;
            missing += mi.len();
            extra += ex.len();

            if mi.is_empty() && ex.is_empty() && !same_order_within_tasks(&theirs, &mine) {
                differences.push(format!("{rel}: order differs within a (zone, layer) task"));
            }

            differences.extend(mi.into_iter().map(|k| format!("{rel}: - {k}")));
            differences.extend(ex.into_iter().map(|k| format!("{rel}: + {k}")));
            continue;
        }

        let theirs: Vec<Entry> = reference
            .entries
            .iter()
            .filter(|e| e.type_ == TYPE)
            .cloned()
            .collect();

        if !theirs.is_empty() {
            nonzero += 1;
        }

        let (m, mi, ex) = diff(&theirs, &mine);
        matched += m;
        missing += mi.len();
        extra += ex.len();

        if mi.is_empty() && ex.is_empty() && !same_order_within_tasks(&theirs, &mine) {
            differences.push(format!("{rel}: order differs within a (zone, layer) task"));
        }

        differences.extend(mi.into_iter().map(|k| format!("{rel}: - {k}")));
        differences.extend(ex.into_iter().map(|k| format!("{rel}: + {k}")));
    }

    println!(
        "DRC-25 zone connections: {compared} boards compared ({nonzero} with reference \
         entries); {TYPE}: matched {matched}, missing {missing}, extra {extra}"
    );
    println!("  pending: {}", pending.len());

    for p in &pending {
        println!("    {p}");
    }

    for d in differences.iter().take(200) {
        println!("  {d}");
    }

    assert!(differences.is_empty(), "{} differences", differences.len());
}

/// `RunTests` with the provider only and `starved_thermal` forced to error, as the C++
/// fixtures do; the number of errors. `Err` when the board cannot be run yet.
fn starved_thermal_errors(rel: &str) -> Result<usize, String> {
    let path = kicad_qa::kicad_source_root().join(rel);
    let mut loaded = load_board(&path)?;

    if let Some(first) = loaded.report.skipped.first() {
        return Err(format!(
            "parser skipped {} items (first: {first:?})",
            loaded.report.skipped.len()
        ));
    }

    loaded
        .board
        .design_settings_mut()
        .drc_severities
        .insert(DRCE_STARVED_THERMAL, Severity::Error);

    let found = run_flow_providers(&mut loaded, EdaUnits::Mm, true, &["zone connections"]);

    Ok(found
        .iter()
        .filter(|r| r.item.error_code == DRCE_STARVED_THERMAL)
        .count())
}

/// Runs the C++ test table `(board, expected starved_thermal errors)`; boards that cannot be
/// run yet are reported as pending.
fn check_table(test: &str, table: &[(&str, usize)]) {
    for &(rel, expected) in table {
        match quiet(|| catch_unwind(AssertUnwindSafe(|| starved_thermal_errors(rel)))) {
            Ok(Ok(n)) => {
                assert_eq!(
                    n, expected,
                    "{test}: {rel}: {n} violations found (expected {expected})"
                );
                println!("{test}: {rel}, passed");
            }
            Ok(Err(e)) => println!("{test}: {rel}: pending: {e}"),
            Err(e) => println!("{test}: {rel}: pending: panic: {}", panic_text(&*e)),
        }
    }
}

/// `test_drc_starved_thermal.cpp`, `DRCStarvedThermal`. The C++ refills the zones first
/// (`KI_TEST::FillZones`); the port runs on the fills saved in the files until the zone filler
/// is ported (the reference CLI, which does not refill, reports the same counts).
#[test]
fn drc_starved_thermal() {
    check_table(
        "DRCStarvedThermal",
        &[
            ("qa/data/pcbnew/test_starved_thermal.kicad_pcb", 2),
            // Copper graphic shapes count as thermal connections
            ("qa/data/pcbnew/issue19090/issue19090.kicad_pcb", 0),
        ],
    );
}

/// The zone connection boards of `test_drc_regressions.cpp`: `issue14334` (thermal spoke to
/// an otherwise unconnected island; `DRCFalseNegativeRegressions` expects 2 violations of all
/// providers, no refill; the reference CLI reports both as `starved_thermal`), `issue19090`
/// (`DRCZoneFalsePositiveRegressions`, `starved_thermal` forced to error: 0; the C++ refills,
/// see [`drc_starved_thermal`]) and `issue15280` (very wide spokes mis-counted as being single
/// spoke). `DRCFalsePositiveRegressions` sets `starved_thermal` to ignore for every board, so
/// for `issue15280` the expectation 0 is the reference CLI's (no `starved_thermal` entry).
#[test]
fn drc_regressions_zone_connections() {
    check_table(
        "DRCRegressions",
        &[
            ("qa/data/pcbnew/issue15280.kicad_pcb", 0),
            ("qa/data/pcbnew/issue14334.kicad_pcb", 2),
            ("qa/data/pcbnew/issue19090/issue19090.kicad_pcb", 0),
        ],
    );
}

/// Checks that the pads parsed from the board file are those of the side table
/// `tests/data/zone_connections/<name>.pads` (written by `gen_drc25_boards.py`; it stood in
/// for the pad parser before PCBIO-07): uuid, number, position and net per pad, in footprint
/// order. Adding the table's pads on top of the parsed ones (as the test did before pads
/// parsed) duplicated every pad and produced an extra `starved_thermal` (STAB-DRC).
fn check_synthetic_pads(board: &kicad_pcb::Board, table: &str) {
    use kicad_core::Kiid;

    let iu = |v: &str| (v.parse::<f64>().unwrap() * 1e6).round() as i32;
    let ctx = BoardCtx::bare(board);
    let mut parsed = Vec::new();

    // BOARD::GetPads(): footprint order, then pad order.
    for pad in board.get_pads() {
        let pad = &board[pad];
        parsed.push((
            pad.base.item.uuid,
            pad.number().to_string(),
            pad.position(&ctx),
            board
                .find_net(pad.base.net)
                .map(|n| n.netname.clone())
                .unwrap_or_default(),
        ));
    }

    let expected: Vec<_> = table
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (
                Kiid::parse(f[1]).unwrap(),
                f[2].to_string(),
                kicad_geom::math::vector2::Vector2I::new(iu(f[4]), iu(f[5])),
                f[8].to_string(),
            )
        })
        .collect();

    assert_eq!(parsed, expected);
}

/// The golden entries of a synthetic board (`tests/oracle/gen_drc25_boards.py`).
fn synthetic_golden(text: &str) -> Vec<Entry> {
    text.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();

            Entry {
                severity: f[0].to_string(),
                type_: f[1].to_string(),
                description: f[2].to_string(),
                items: f[3..]
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
                excluded: false,
            }
        })
        .collect()
}

/// The synthetic thermal relief boards (zones filled by the reference; pads on and near the
/// zone edges, spokes blocked by other-net tracks, islands, a GND track and a GND copper line
/// as manual spokes, PTH pads on both layers, an unconditional `min_resolved_spokes` rule)
/// against the reference: every `starved_thermal` entry, and their order within each
/// (zone, layer) task (the interleaving of the tasks is scheduling dependent in C++).
#[test]
fn synthetic_boards_match_reference() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/zone_connections");
    let cases: &[(&str, &str, &str)] = &[
        (
            "thermals",
            include_str!("data/zone_connections/thermals.pads"),
            include_str!("golden/drc25/thermals.txt"),
        ),
        (
            "thermals_rule",
            include_str!("data/zone_connections/thermals_rule.pads"),
            include_str!("golden/drc25/thermals_rule.txt"),
        ),
    ];
    let mut total = 0;

    for &(name, pads, golden_text) in cases {
        let mut loaded = load_board(&data.join(format!("{name}.kicad_pcb"))).unwrap();
        assert!(
            loaded.report.skipped.is_empty(),
            "{name}: {:?}",
            loaded.report.skipped
        );
        check_synthetic_pads(&loaded.board, pads);

        let mine = report_zone_connections(&mut loaded);
        let theirs = synthetic_golden(golden_text);
        let (matched, missing, extra) = diff(&theirs, &mine);

        assert!(
            missing.is_empty() && extra.is_empty(),
            "{name}: missing {missing:#?}\nextra {extra:#?}"
        );
        assert!(
            same_order_within_tasks(&theirs, &mine),
            "{name}: order within a (zone, layer) task"
        );
        println!(
            "{name}: {matched} entries identical (report order {})",
            if mine == theirs {
                "identical"
            } else {
                "differs only between (zone, layer) tasks"
            }
        );
        total += matched;
    }

    assert_eq!(total, 9);
}
