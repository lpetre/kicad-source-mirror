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

//! DRC-30: the differential pair coupling provider
//! (`drc_test_provider_diff_pair_coupling.cpp`).
//!
//! * Reference: `tests/golden/drc30/reference.txt` (`tests/oracle/gen_drc30_golden.py`), the
//!   `diff_pair_gap_out_of_range` / `diff_pair_uncoupled_length_too_long` entries of
//!   `kicad-cli pcb drc --format json --severity-all --units mm` for the QA diff pair boards,
//!   each with its own rules (`base`) and with the rule variants of `tests/data/drc30/`
//!   appended to its rule file (`gap`, `unc`, `both`, `layer`).
//! * Rust: the same board and rule file, the provider run with the copper item tree of the
//!   cache generator, the report entries built as the CLI job does.
//!
//! The items of a diff pair are kept in `std::set<BOARD_CONNECTED_ITEM*>` (address order) in
//! C++, so the order of the gap entries of one pair and the order of the items of an uncoupled
//! entry are heap dependent there; the entries are compared as a multiset, the items of an
//! uncoupled entry as a set.
//!
//! The C++ regression tests that use these boards (`test_drc_regressions.cpp`
//! `DRCFalsePositiveRegressions`: `issue7975`, `issue8407`, `issue17967/issue17967`,
//! `diff_pair_uncoupled_tuning_drc`; expected 0 errors) run with every provider; their
//! diff pair part is the `base` case here (0 entries), the whole test is in
//! `kicad-session/tests/drc12_regressions.rs`.

mod drc_harness;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use drc_harness::{Entry, Loaded, cli_report_entries, load_board};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::cache_generator::{
    build_copper_item_rtree, build_zone_caches, build_zone_lists, compute_max_clearances,
    prepare_board,
};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::diff_pair_coupling::DrcTestProviderDiffPairCoupling;
use kicad_pcb::BoardCtx;

/// The error types of the provider.
const TYPES: &[&str] = &[
    "diff_pair_gap_out_of_range",
    "diff_pair_uncoupled_length_too_long",
];

/// The provider with the parts of `DRC_CACHE_GENERATOR::Run()` it reads (the copper item
/// tree) built right before it, and the run's connectivity.
struct WithCaches<'a> {
    connectivity: &'a ConnectivityData,
}

impl DrcTestProviderRun for WithCaches<'_> {
    fn name(&self) -> &str {
        "diff_pair_coupling"
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

        DrcTestProviderDiffPairCoupling::new(&caches).run_tests(engine, run, &cctx, units)
    }
}

/// `BuildConnectivity` at load time, then the board-writing part of the cache generator and
/// its `connectivity->Build( board )`.
fn prepare(loaded: &mut Loaded) -> ConnectivityData {
    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(&mut loaded.board);
    prepare_board(&mut loaded.board);
    connectivity.clear_ratsnest();
    connectivity.build_board(&mut loaded.board);
    connectivity
}

/// The report entries of the provider's types for one board file.
fn report(path: &Path) -> Result<Vec<Entry>, String> {
    let mut loaded = load_board(path)?;
    let connectivity = prepare(&mut loaded);
    let mut provider = WithCaches {
        connectivity: &connectivity,
    };

    Ok(cli_report_entries(&mut loaded, false, &mut [&mut provider])
        .into_iter()
        .filter(|e| TYPES.contains(&e.type_.as_str()))
        .collect())
}

/// Copies the board of `case` (`<board path>#<variant>`) with its project and rule file to a
/// private directory and appends the variant's rules, as `gen_drc30_golden.py` does.
fn materialize(case: &str, dir: &Path) -> PathBuf {
    let (rel, variant) = case.split_once('#').unwrap();
    let src = kicad_qa::kicad_source_root().join(rel);
    let stem = src.file_stem().unwrap().to_string_lossy().to_string();
    let work = dir.join(case.replace(['/', '#', '.'], "_"));
    std::fs::create_dir_all(&work).unwrap();

    for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
        let p = src.with_extension(ext);

        if p.exists() {
            std::fs::copy(&p, work.join(format!("{stem}.{ext}"))).unwrap();
        }
    }

    let dru = work.join(format!("{stem}.kicad_dru"));

    if variant != "base" {
        let mut text = std::fs::read_to_string(&dru).unwrap_or_else(|_| "(version 1)\n".into());
        text += "\n";
        text += &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/drc30")
                .join(format!("{variant}.dru")),
        )
        .unwrap();
        std::fs::write(&dru, text).unwrap();
    }

    work.join(format!("{stem}.kicad_pcb"))
}

/// The golden: per case its entries in report order.
fn golden() -> BTreeMap<String, Vec<Entry>> {
    let mut out: BTreeMap<String, Vec<Entry>> = BTreeMap::new();

    for line in include_str!("golden/drc30/reference.txt").lines() {
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

/// The net part (`[<net>]`) of an item description.
fn net_of(description: &str) -> &str {
    let start = description.find('[').map_or(0, |i| i + 1);
    let end = description[start..]
        .find(']')
        .map_or(description.len(), |i| start + i);
    &description[start..end]
}

/// The comparison key of an entry. The two items a report shows for an uncoupled entry are
/// the first items of the positive and of the negative net in address order in C++ (heap
/// dependent), so they are compared by their nets only (positive net first).
fn key(e: &Entry) -> String {
    let mut e = e.clone();

    if e.type_ == "diff_pair_uncoupled_length_too_long" {
        for it in e.items.iter_mut() {
            *it = (String::new(), format!("item of net {}", net_of(&it.1)), 0.0, 0.0);
        }
    }

    e.key()
}

/// Multiset difference by [`key`]: (matched, missing, extra).
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

#[test]
fn drc30_reference_cases() {
    let dir = std::env::temp_dir().join(format!("kicad-drc30-{}", std::process::id()));
    let mut failures = Vec::new();
    let mut total = (0, 0, 0);
    let mut same_order = 0;
    let mut cases = 0;

    for (case, reference) in golden() {
        let path = materialize(&case, &dir);
        let mine = match report(&path) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{case}: load failed: {e}"));
                continue;
            }
        };

        let (matched, missing, extra) = diff(&reference, &mine);
        total.0 += matched;
        total.1 += missing.len();
        total.2 += extra.len();
        cases += 1;

        if missing.is_empty() && extra.is_empty() {
            let a: Vec<String> = reference.iter().map(key).collect();
            let b: Vec<String> = mine.iter().map(key).collect();

            if a == b {
                same_order += 1;
            } else {
                println!("{case}: same entries, different order");
            }
        }

        for k in missing.iter().take(5) {
            failures.push(format!("{case}: - {k}"));
        }

        for k in extra.iter().take(5) {
            failures.push(format!("{case}: + {k}"));
        }
    }

    let _ = std::fs::remove_dir_all(&dir);

    println!(
        "drc30: {cases} cases, {} matched, {} missing, {} extra; {same_order} cases in the \
         reference order",
        total.0, total.1, total.2
    );

    for f in &failures {
        println!("{f}");
    }

    assert!(failures.is_empty(), "{} differences", failures.len());
}
