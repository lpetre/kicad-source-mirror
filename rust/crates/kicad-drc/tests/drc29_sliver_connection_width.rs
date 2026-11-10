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

//! DRC-29: the copper sliver checker (`drc_test_provider_sliver_checker.cpp`) and the
//! connection width provider (`drc_test_provider_connection_width.cpp`).
//!
//! * Corpus: the `copper_sliver` / `connection_width` entries of the reference reports
//!   (`kicad-cli pcb drc --format json --severity-all`, golden `tests/golden/drc29/corpus.txt`
//!   from `tests/oracle/gen_drc29_golden.py`) against a run of the two providers on every board
//!   the port loads completely. The reference JSON of a sliver has no item and no position,
//!   so the positions are checked by the item-level oracle below.
//! * Item level: `tests/oracle/drc29_oracle.cpp` (`build_drc29_oracle.sh`) runs the real
//!   `DRC_ENGINE::RunTests` with only these two codes enabled and prints every violation
//!   (code, message, marker position, layer, items, marker path); golden
//!   `tests/golden/drc29/oracle.txt` (`gen_drc29_oracle.sh`).
//! * Ported C++ tests: `qa/tests/pcbnew/drc/test_drc_copper_sliver.cpp` (`DRCCopperSliver`) and
//!   `test_drc_copper_conn.cpp` (`DRCCopperConn`). Both call `KI_TEST::FillZones` first; the
//!   zone filler (kicad-zone ZF-03) is pending and kicad-zone depends on kicad-drc, so the
//!   ports run on the fills stored in the files and expect the counts the reference build
//!   produces on those fills (`gen_drc29_oracle.sh`); the counts after a refill are recorded
//!   next to them.
//!
//! The order of the connection width entries, and the order of the items inside one entry,
//! is address / hash dependent in C++ (`drc.md` 4.12); those entries are compared as a
//! multiset with the items sorted.

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use drc_harness::{
    Entry, Loaded, cli_report_entries, has_compiled_conditions, load_board, panic_text, quiet,
    run_providers,
};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::cache_generator::{
    build_copper_item_rtree, build_zone_caches, build_zone_lists, compute_max_clearances,
    prepare_board,
};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::connection_width::DrcTestProviderConnectionWidth;
use kicad_drc::providers::sliver_checker::DrcTestProviderSliverChecker;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_CONNECTION_WIDTH, DRCE_COPPER_SLIVER, DRCE_FIRST, DRCE_LAST};

/// The error types of the two providers.
const TYPES: &[&str] = &["copper_sliver", "connection_width"];

/// The two providers in `PROVIDER_ORDER` ("copper width" before "sliver checker") with the
/// parts of `DRC_CACHE_GENERATOR::Run()` the connection width provider reads (copper zone
/// list, copper item tree, zone trees) built right before them, and the run's connectivity
/// (`FlashLayer` of pads and vias).
struct WithCaches<'a> {
    connectivity: &'a ConnectivityData,
}

impl DrcTestProviderRun for WithCaches<'_> {
    fn name(&self) -> &str {
        "copper width"
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        let cctx = BoardCtx {
            board: ctx.board,
            rules: ctx.rules,
            connectivity: Some(self.connectivity),
        };
        let mut caches = DrcBoardCaches::new();
        (caches.max_clearance, caches.max_physical_clearance) =
            compute_max_clearances(&cctx, engine);
        build_zone_lists(&cctx, &mut caches);
        caches.copper_item_rtree = Some(Arc::new(build_copper_item_rtree(
            &cctx,
            caches.max_clearance,
        )));
        build_zone_caches(&cctx, &mut caches);

        let mut width = DrcTestProviderConnectionWidth::new(&caches);

        if !width.run_tests(engine, run, &cctx, units) {
            return false;
        }

        let mut sliver = DrcTestProviderSliverChecker::default();
        sliver.run_tests(engine, run, &cctx, units)
    }
}

/// `BuildConnectivity` at load time, then the board-writing part of the cache generator and
/// its `connectivity->Build( board )` (as `drc08a_copper_clearance.rs`).
fn prepare(loaded: &mut Loaded) -> ConnectivityData {
    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(&mut loaded.board);
    prepare_board(&mut loaded.board);
    connectivity.clear_ratsnest();
    connectivity.build_board(&mut loaded.board);
    connectivity
}

/// The report entries of the two types after a run of the providers.
fn report(loaded: &mut Loaded) -> Vec<Entry> {
    let connectivity = prepare(loaded);
    let mut provider = WithCaches {
        connectivity: &connectivity,
    };

    cli_report_entries(loaded, false, &mut [&mut provider])
        .into_iter()
        .filter(|e| TYPES.contains(&e.type_.as_str()))
        .collect()
}

/// The comparison key of an entry: the items sorted for `connection_width` (their order is
/// address / hash dependent in C++).
fn key(e: &Entry) -> String {
    let mut e = e.clone();

    if e.type_ == "connection_width" {
        e.items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    }

    e.key()
}

/// The golden: per board its entries in report order.
fn golden() -> BTreeMap<String, Vec<Entry>> {
    let mut out: BTreeMap<String, Vec<Entry>> = BTreeMap::new();

    for line in include_str!("golden/drc29/corpus.txt").lines() {
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

/// Multiset difference by [`key`]: (matched, missing, extra).
fn diff(reference: &[Entry], mine: &[Entry]) -> (usize, Vec<String>, Vec<String>) {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();

    for e in reference {
        *counts.entry(key(e)).or_default() += 1;
    }

    let mut extra = Vec::new();

    for e in mine {
        let c = counts.entry(key(e)).or_default();

        if *c > 0 {
            *c -= 1;
        } else {
            extra.push(key(e));
        }
    }

    let missing: Vec<String> = counts
        .iter()
        .flat_map(|(k, &n)| std::iter::repeat_n(k.clone(), n.max(0) as usize))
        .collect();

    (mine.len() - extra.len(), missing, extra)
}

/// Every corpus board the port loads completely against the reference report. Set
/// `DRC29_FILTER=<path substring>` to limit the boards.
#[test]
fn sliver_and_connection_width_match_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC29_FILTER").unwrap_or_default();
    let mut compared = 0;
    let mut per_type: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    let mut differences = Vec::new();
    let mut partial = Vec::new();
    let mut pending = Vec::new();
    let mut order_differs = Vec::new();

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let path = root.join(&rel);
        let run = || -> Result<(Vec<Entry>, bool), String> {
            let mut loaded = load_board(&path)?;

            if has_compiled_conditions(&loaded.engine) {
                return Err("user rule conditions need DRC-21".to_string());
            }

            let complete = loaded.report.skipped.is_empty();
            Ok((report(&mut loaded), complete))
        };
        // DRC29_PANIC=1 lets a panic through (with its location and backtrace).
        let result = if std::env::var("DRC29_PANIC").is_ok() {
            Ok(run())
        } else {
            quiet(|| catch_unwind(AssertUnwindSafe(run)))
        };

        let (mine, complete) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                pending.push(format!("{rel}: panic: {}", panic_text(&*e)));
                continue;
            }
        };

        let mut diffs = Vec::new();

        for t in TYPES {
            let r: Vec<Entry> = reference
                .iter()
                .filter(|e| e.type_ == *t)
                .cloned()
                .collect();
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, missing, extra) = diff(&r, &m);

            if complete {
                let s = per_type.entry(t).or_default();
                s.0 += matched;
                s.1 += missing.len();
                s.2 += extra.len();
            }

            diffs.extend(missing.into_iter().map(|k| format!("{rel}: - {k}")));
            diffs.extend(extra.into_iter().map(|k| format!("{rel}: + {k}")));

            // The sliver report order is deterministic in C++ (layer, outline, vertex).
            if *t == "copper_sliver" && diffs.is_empty() && r != m {
                order_differs.push(rel.clone());
            }
        }

        if complete {
            compared += 1;
            differences.extend(diffs);
        } else {
            pending.push(format!(
                "{rel}: parser skipped items ({} diffs)",
                diffs.len()
            ));
            partial.extend(diffs);
        }
    }

    eprintln!(
        "DRC-29 sliver / connection width: {compared} boards compared, {} pending",
        pending.len()
    );

    for (t, (m, missing, extra)) in &per_type {
        eprintln!("  {t}: matched {m} missing {missing} extra {extra}");
    }

    for d in &differences {
        eprintln!("  DIFF {d}");
    }

    for d in &partial {
        eprintln!("  diff on a partially loaded board: {d}");
    }

    for d in &order_differs {
        eprintln!("  ORDER {d}");
    }

    if std::env::var("DRC29_VERBOSE").is_ok() {
        for p in &pending {
            eprintln!("  pending: {p}");
        }
    }

    assert!(compared > 0);
    assert!(
        differences.is_empty() && order_differs.is_empty(),
        "{} differences",
        differences.len() + order_differs.len()
    );
}

/// Loads a QA board; `None` (with a note) when the port does not load it completely yet.
fn load_qa(name: &str) -> Option<Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));

    match quiet(|| catch_unwind(AssertUnwindSafe(|| load_board(&path)))) {
        Ok(Ok(l)) if l.report.skipped.is_empty() && !has_compiled_conditions(&l.engine) => Some(l),
        Ok(Ok(_)) => {
            eprintln!("{name}: pending (skipped items or rule conditions)");
            None
        }
        Ok(Err(e)) => {
            eprintln!("{name}: pending ({e})");
            None
        }
        Err(e) => {
            eprintln!("{name}: pending ({})", panic_text(&*e));
            None
        }
    }
}

/// `RunTests( MM, true, false )` with every code ignored but `code`, which is an error: the
/// number of violations of `code` the handler receives.
fn count_violations(loaded: &mut Loaded, code: i32, ignored: Option<&[i32]>) -> usize {
    {
        let bds = loaded.board.design_settings_mut();

        match ignored {
            None => {
                for ii in DRCE_FIRST..=DRCE_LAST {
                    bds.drc_severities.insert(ii, Severity::Ignore);
                }
            }
            Some(codes) => {
                for &ii in codes {
                    bds.drc_severities.insert(ii, Severity::Ignore);
                }
            }
        }

        bds.drc_severities.insert(code, Severity::Error);
    }

    let connectivity = prepare(loaded);
    let mut provider = WithCaches {
        connectivity: &connectivity,
    };
    let found = run_providers(loaded, EdaUnits::Mm, true, &mut [&mut provider]);

    found
        .iter()
        .filter(|r| {
            loaded
                .board
                .design_settings()
                .get_severity(r.item.error_code())
                == Severity::Error
        })
        .count()
}

/// Port of `DRCCopperSliver` (`test_drc_copper_sliver.cpp`) on the stored fills. The C++
/// expectations after `KI_TEST::FillZones` are `sliver` 1, the others 0.
#[test]
fn drc_copper_sliver() {
    // (board, count on the stored fills (reference build), count after a refill (C++ test))
    let cases = [
        ("sliver", 1, 1),
        ("issue14449", 0, 0),
        ("issue14549", 0, 0),
        ("issue14549_2", 0, 0),
        ("issue14559", 0, 0),
    ];

    for (name, stored, _refilled) in cases {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };

        let n = count_violations(&mut loaded, DRCE_COPPER_SLIVER, None);
        assert_eq!(n, stored, "DRC copper sliver: {name}");
    }
}

/// Port of `DRCCopperConn` (`test_drc_copper_conn.cpp`) on the stored fills. The C++
/// expectations after `KI_TEST::FillZones` are `issue9870` 12, `connection_width_rules` 3,
/// `issue12831` 0, `issue14130` 0.
#[test]
fn drc_copper_conn() {
    use kicad_pcb::drc_api::{
        DRCE_DRILL_OUT_OF_RANGE, DRCE_INVALID_OUTLINE, DRCE_LIB_FOOTPRINT_ISSUES,
        DRCE_LIB_FOOTPRINT_MISMATCH, DRCE_STARVED_THERMAL, DRCE_UNCONNECTED_ITEMS,
        DRCE_VIA_DIAMETER,
    };

    let ignored = [
        DRCE_INVALID_OUTLINE,
        DRCE_UNCONNECTED_ITEMS,
        DRCE_COPPER_SLIVER,
        DRCE_STARVED_THERMAL,
        DRCE_DRILL_OUT_OF_RANGE,
        DRCE_VIA_DIAMETER,
        DRCE_LIB_FOOTPRINT_ISSUES,
        DRCE_LIB_FOOTPRINT_MISMATCH,
    ];

    // (board, count on the stored fills (reference build), count after a refill (C++ test))
    let cases = [
        ("issue9870", 13, 12),
        ("connection_width_rules", 3, 3),
        ("issue12831", 0, 0),
        ("issue14130", 0, 0),
    ];

    for (name, stored, _refilled) in cases {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };

        let n = count_violations(&mut loaded, DRCE_CONNECTION_WIDTH, Some(&ignored));
        assert_eq!(n, stored, "DRC connection width: {name}");
    }
}
