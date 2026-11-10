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

//! DRC-26: the silk clearance provider (`drc_test_provider_silk_clearance.cpp`) and the
//! physical clearance provider (`drc_test_provider_physical_clearance.cpp`) against the
//! reference DRC reports: the synthetic boards of `tests/oracle/gen_drc26_boards.py` (golden
//! `tests/golden/drc26/`) and the shared corpus, plus the ported C++ test
//! `qa/tests/pcbnew/drc/test_drc_silk_rule_area_issue24177.cpp`
//! (`RuleAreaDoesNotCauseSilkOrMaskViolationsIssue24177`).
//!
//! The physical clearance provider needs the per-run board caches, so the run builds the parts
//! of `DRC_CACHE_GENERATOR::Run()` it reads (clearances, zone lists, zone trees) right before
//! the providers, as `DRC_ENGINE::RunTests` orders them.

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, cached_entries, cli_report_entries, golden, load_board,
    panic_text, quiet, run_providers,
};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::cache_generator::{
    build_zone_caches, build_zone_lists, compute_max_clearances, prepare_board,
};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::physical_clearance::DrcTestProviderPhysicalClearance;
use kicad_drc::providers::silk_clearance::DrcTestProviderSilkClearance;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{
    DRCE_LAST, DRCE_SILK_CLEARANCE, DRCE_SILK_EDGE_CLEARANCE, DRCE_SILK_MASK_CLEARANCE,
    DRCE_SOLDERMASK_BRIDGE,
};

/// The error types of the two providers.
const TYPES: &[&str] = &[
    "silk_overlap",
    "silk_over_copper",
    "clearance",
    "hole_clearance",
];

/// The physical clearance provider then the silk clearance provider (registry order), with the
/// parts of `DRC_CACHE_GENERATOR::Run()` the physical provider reads built from the run's
/// context right before it. The board-mutating part of the cache generator
/// ([`prepare_board`]) runs before the run.
struct PhysicalAndSilk {
    physical: bool,
    silk: bool,
}

impl DrcTestProviderRun for PhysicalAndSilk {
    fn name(&self) -> &str {
        "physical_clearance+silk_clearance"
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        if self.physical {
            let mut caches = DrcBoardCaches::new();
            let (max_clearance, max_physical_clearance) = compute_max_clearances(ctx, engine);
            caches.max_clearance = max_clearance;
            caches.max_physical_clearance = max_physical_clearance;
            build_zone_lists(ctx, &mut caches);
            build_zone_caches(ctx, &mut caches);

            let mut provider = DrcTestProviderPhysicalClearance::new(&caches);

            if !provider.run_tests(engine, run, ctx, units) {
                return false;
            }
        }

        if self.silk {
            let mut provider = DrcTestProviderSilkClearance::default();

            if !provider.run_tests(engine, run, ctx, units) {
                return false;
            }
        }

        true
    }
}

/// The report entries (`kicad-cli pcb drc --format json`) of a run of the two providers.
fn report(loaded: &mut Loaded) -> Vec<Entry> {
    prepare_board(&mut loaded.board);

    let mut provider = PhysicalAndSilk {
        physical: true,
        silk: true,
    };
    cli_report_entries(loaded, false, &mut [&mut provider])
}

/// The names (`DRC_CONSTRAINT::GetName()`) of the physical clearance constraints of the engine:
/// a `clearance` / `hole_clearance` entry naming one of them comes from the physical clearance
/// provider (the copper and hole clearance providers report the same types).
fn physical_names(engine: &DrcEngine) -> Vec<String> {
    let mut names = Vec::new();

    for t in [
        DrcConstraintT::PhysicalClearanceConstraint,
        DrcConstraintT::PhysicalHoleClearanceConstraint,
    ] {
        if let Some(list) = engine.constraint_map().get(&t) {
            for c in list {
                let name = c.constraint.name();

                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }

    names
}

/// Whether an entry was produced by one of the two providers (`gen_drc26_boards.py`
/// `is_ours`, with the physical rule names of the board).
fn is_ours(e: &Entry, names: &[String]) -> bool {
    match e.type_.as_str() {
        "silk_overlap" => true,
        "silk_over_copper" => e.items.len() == 2,
        "clearance" | "hole_clearance" => {
            e.description.starts_with("Internal clearance violation")
                || names
                    .iter()
                    .any(|n| e.description.contains(&format!("({n} clearance ")))
        }
        _ => false,
    }
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

/// The golden entries of a synthetic board (`tests/oracle/gen_drc26_boards.py`).
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

/// The synthetic boards (no footprints: silk graphics and texts against every target layer,
/// tented / untented vias, mask openings, the board edge, a silk rule area; tracks, vias, copper
/// polygons, arcs, beziers, zones with several fill islands, texts and edge cuts under
/// unconditional physical rules) against the reference: every entry of the two providers, in
/// report order.
#[test]
fn synthetic_boards_match_reference() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/silk_physical");
    let cases: &[(&str, &str)] = &[
        (
            "silk_default",
            include_str!("golden/drc26/silk_default.txt"),
        ),
        ("silk_setup", include_str!("golden/drc26/silk_setup.txt")),
        (
            "silk_rule_web",
            include_str!("golden/drc26/silk_rule_web.txt"),
        ),
        ("physical", include_str!("golden/drc26/physical.txt")),
        (
            "physical_small",
            include_str!("golden/drc26/physical_small.txt"),
        ),
    ];
    let mut failures = Vec::new();

    for &(name, golden_text) in cases {
        let mut loaded = load_board(&data.join(format!("{name}.kicad_pcb"))).unwrap();
        assert!(
            loaded.report.skipped.is_empty(),
            "{name}: parser skipped items: {:?}",
            loaded.report.skipped
        );

        let names = physical_names(&loaded.engine);
        let mine: Vec<Entry> = report(&mut loaded)
            .into_iter()
            .filter(|e| is_ours(e, &names))
            .collect();
        let theirs = synthetic_golden(golden_text);

        println!("{name}: {} entries, reference {}", mine.len(), theirs.len());

        if mine != theirs {
            let (_, missing, extra) = diff(&theirs, &mine);

            for k in &missing {
                println!("  - {k}");
            }

            for k in &extra {
                println!("  + {k}");
            }

            if missing.is_empty() && extra.is_empty() {
                println!("  same entries in a different order");
            }

            failures.push(name);
        }
    }

    assert!(failures.is_empty(), "mismatching boards: {failures:?}");
}

#[derive(Default)]
struct Score {
    matched: BTreeMap<String, usize>,
    missing: BTreeMap<String, usize>,
    extra: BTreeMap<String, usize>,
    compared: usize,
    with_physical_rules: usize,
    order_differs: Vec<String>,
    differences: Vec<String>,
    pending: Vec<String>,
}

/// Every corpus board that loads completely: the entries of the two providers against the
/// reference report (set and order per type).
#[test]
fn silk_and_physical_match_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC26_FILTER").unwrap_or_default();
    let mut score = Score::default();

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        // Types stored as a summary need the cached raw report (the entries of the other
        // providers of the same type have to be filtered out).
        let summarized: Vec<&str> = TYPES
            .iter()
            .copied()
            .filter(|t| reference.summaries.contains_key(*t))
            .collect();

        if summarized.iter().any(|t| cached_entries(&rel, t).is_none()) {
            score
                .pending
                .push(format!("{rel}: summary only (run gen_drc_corpus.py)"));
            continue;
        }

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, Vec<String>, bool), String> {
                    let mut loaded = load_board(&path)?;


                    let names = physical_names(&loaded.engine);
                    let entries = report(&mut loaded)
                        .into_iter()
                        .filter(|e| is_ours(e, &names))
                        .collect();

                    Ok((entries, names, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let (mine, names, complete) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                score.pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                score
                    .pending
                    .push(format!("{rel}: panic: {}", panic_text(&*e)));
                continue;
            }
        };

        if !complete {
            score.pending.push(format!("{rel}: parser skipped items"));
            continue;
        }

        let mut theirs = Vec::new();

        for t in TYPES {
            match cached_entries(&rel, t) {
                Some(v) => theirs.extend(v.into_iter().filter(|e| is_ours(e, &names))),
                None => theirs.extend(
                    reference
                        .entries
                        .iter()
                        .filter(|e| e.type_ == *t && is_ours(e, &names))
                        .cloned(),
                ),
            }
        }

        score.compared += 1;

        if !names.is_empty() {
            score.with_physical_rules += 1;
        }

        for t in TYPES {
            let r: Vec<Entry> = theirs.iter().filter(|e| e.type_ == *t).cloned().collect();
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, missing, extra) = diff(&r, &m);

            *score.matched.entry(t.to_string()).or_default() += matched;
            *score.missing.entry(t.to_string()).or_default() += missing.len();
            *score.extra.entry(t.to_string()).or_default() += extra.len();

            for k in &missing {
                score.differences.push(format!("{rel}: - {k}"));
            }

            for k in &extra {
                score.differences.push(format!("{rel}: + {k}"));
            }

            if r != m && missing.is_empty() && extra.is_empty() {
                score.order_differs.push(format!("{rel}: {t}"));
            }
        }
    }

    println!(
        "DRC-26 silk / physical clearance: {} boards compared ({} with physical clearance constraints, incl. the implicit barcode rule)",
        score.compared, score.with_physical_rules
    );

    for t in TYPES {
        println!(
            "  {t}: matched {}, missing {}, extra {}",
            score.matched.get(*t).unwrap_or(&0),
            score.missing.get(*t).unwrap_or(&0),
            score.extra.get(*t).unwrap_or(&0)
        );
    }

    println!("  order differs: {}", score.order_differs.len());

    for d in &score.order_differs {
        println!("    {d}");
    }

    println!("  pending: {}", score.pending.len());

    for p in &score.pending {
        println!("    {p}");
    }

    for d in score.differences.iter().take(200) {
        println!("  {d}");
    }

    assert!(
        score.differences.is_empty() && score.order_differs.is_empty(),
        "{} differences, {} order differences",
        score.differences.len(),
        score.order_differs.len()
    );
}

/// Port of `qa/tests/pcbnew/drc/test_drc_silk_rule_area_issue24177.cpp`
/// (`RuleAreaDoesNotCauseSilkOrMaskViolationsIssue24177`): a rule area on the silk and mask
/// layers never collides with silk. The solder mask bridge and silk edge counts belong to the
/// solder mask and edge clearance providers; this test runs the silk clearance provider (the
/// solder mask provider's part is `drc27_solder_mask.rs`).
#[test]
fn rule_area_does_not_cause_silk_or_mask_violations_issue24177() {
    let path = kicad_qa::pcbnew_data_dir().join("issue24177/silk_rule_area.kicad_pcb");
    let mut loaded = load_board(&path).unwrap();

    // Zero violations is also what an empty board reports, so pin down the geometry that has
    // to be present for the counts below to mean anything.
    let zones: Vec<_> = loaded.board.zones().collect();
    assert_eq!(zones.len(), 1);
    assert!(zones[0].1.is_rule_area());

    let ctx = BoardCtx::bare(&loaded.board);
    assert!(loaded.board.drawings().any(|(id, _)| {
        let item = ctx.item(id).unwrap();
        item.type_() == kicad_core::typeinfo::KicadT::PcbShapeT
            && item.layer() == kicad_core::layer_ids::PcbLayerId::FSilkS
    }));

    // Focus the run on the providers we changed.
    let bds = loaded.board.design_settings_mut();

    for code in 0..=DRCE_LAST {
        bds.drc_severities.insert(code, Severity::Ignore);
    }

    for code in [
        DRCE_SILK_CLEARANCE,
        DRCE_SILK_MASK_CLEARANCE,
        DRCE_SILK_EDGE_CLEARANCE,
        DRCE_SOLDERMASK_BRIDGE,
    ] {
        bds.drc_severities.insert(code, Severity::Error);
    }

    let mut provider = PhysicalAndSilk {
        physical: false,
        silk: true,
    };
    let found = run_providers(&mut loaded, EdaUnits::Mm, true, &mut [&mut provider]);

    let count = |code: i32| found.iter().filter(|r| r.item.error_code == code).count();

    assert_eq!(count(DRCE_SILK_CLEARANCE), 0);
    assert_eq!(count(DRCE_SILK_MASK_CLEARANCE), 0);
}
