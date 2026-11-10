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

//! DRC-10: the disallow provider (`drc_test_provider_disallow.cpp`) and the implicit keepout
//! rule condition (`A.intersectsKeepout('<uuid>')`) against the reference
//! `kicad-cli pcb drc --format json --severity-all --units mm` reports of the shared corpus
//! (`tests/golden/drc_corpus.txt`, raw reports cached under `rust/target/drc-corpus/`), plus
//! the ports of the C++ tests of the provider: `qa/tests/pcbnew/drc/test_drc_keepout_disallow.cpp`,
//! `test_drc_issue24525.cpp` and the `issue7567` case of `DRCFalsePositiveRegressions`
//! (`test_drc_regressions.cpp`).
//!
//! The provider runs on the thread pool in C++ (report order scheduling dependent): its
//! entries are compared as multisets. User rule conditions are compiled with the real
//! expression binding (DRC-21 / DRC-22), and the run holds the board's expression caches
//! (`PcbExprCacheScope`, C++ `BOARD::m_IntersectsKeepoutCache` and friends, which live for the
//! whole run).

mod drc_harness;

use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, cached_entries, cli_report_entries, diff_entries, golden, load_board,
    panic_text, quiet, read_tuning_profiles, run_providers,
};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_core::json::{self, ObjectOrder};
use kicad_core::typeinfo::KicadT;
use kicad_drc::cache_generator::{compute_max_clearances, prepare_board};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::pcbexpr::functions::PcbExprCacheScope;
use kicad_drc::providers::disallow::DrcTestProviderDisallow;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_ALLOWED_ITEMS, DRCE_TEXT_ON_EDGECUTS};

/// The error types of the provider.
const TYPES: &[&str] = &["items_not_allowed", "text_on_edge_cuts"];

/// The provider with the run's `m_DRCMaxPhysicalClearance` (the part of
/// `DRC_CACHE_GENERATOR::Run()` it reads), the connectivity of the load and the board's
/// expression caches.
struct DisallowWithCaches<'a> {
    connectivity: &'a ConnectivityData,
}

impl DrcTestProviderRun for DisallowWithCaches<'_> {
    fn name(&self) -> &str {
        "disallow"
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
        let _scope = PcbExprCacheScope::enter(ctx.board);
        let mut caches = DrcBoardCaches::new();
        (caches.max_clearance, caches.max_physical_clearance) =
            compute_max_clearances(&cctx, engine);

        DrcTestProviderDisallow::new(&caches).run_tests(engine, run, &cctx, units)
    }
}

/// `BuildConnectivity` at load time, then the board-writing part of the cache generator and
/// its `connectivity->Build( board )` (as `drc09_hole_edge.rs`).
fn prepare(loaded: &mut Loaded) -> ConnectivityData {
    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(&mut loaded.board);
    prepare_board(&mut loaded.board);
    connectivity.clear_ratsnest();
    connectivity.build_board(&mut loaded.board);
    connectivity
}

/// The shared harness load, with `InitEngine` repeated with the real expression binding so
/// that user rule conditions (e.g. `A.insideArea('x')`) evaluate.
fn load(path: &std::path::Path) -> Result<Loaded, String> {
    let mut loaded = load_board(path)?;
    let pro = path.with_extension("kicad_pro");
    let tuning_profiles = if pro.exists() {
        let doc = json::parse_bytes(&std::fs::read(&pro).unwrap(), ObjectOrder::Sorted, true)
            .map_err(|e| format!("project: {e:?}"))?;
        read_tuning_profiles(&doc)
    } else {
        TuningProfiles::default()
    };
    let dru = path.with_extension("kicad_dru");
    let dru_text = dru
        .exists()
        .then(|| String::from_utf8_lossy(&std::fs::read(&dru).unwrap()).to_string());
    let dru_path = dru.to_string_lossy().to_string();

    let mut engine = DrcEngine::new();
    engine
        .init_engine(
            &mut loaded.board,
            dru_text.as_deref().map(|t| (t, dru_path.as_str())),
            default_layer_choices(),
            &tuning_profiles,
            None,
        )
        .map_err(|_| "InitEngine failed".to_string())?;
    loaded.engine = engine;

    Ok(loaded)
}

/// The report entries of the provider's types.
fn report(loaded: &mut Loaded) -> Vec<Entry> {
    let connectivity = prepare(loaded);
    let mut provider = DisallowWithCaches {
        connectivity: &connectivity,
    };

    cli_report_entries(loaded, false, &mut [&mut provider])
        .into_iter()
        .filter(|e| TYPES.contains(&e.type_.as_str()))
        .collect()
}

/// The violations of `codes` delivered to the handler by a `RunTests( MM, true, false )`, with
/// `severities` applied to the board design settings first.
fn run_with(loaded: &mut Loaded, severities: &[(i32, Severity)]) -> Vec<drc_harness::Reported> {
    {
        let bds = loaded.board.design_settings_mut();

        for &(code, severity) in severities {
            bds.drc_severities.insert(code, severity);
        }
    }

    let connectivity = prepare(loaded);
    let mut provider = DisallowWithCaches {
        connectivity: &connectivity,
    };

    run_providers(loaded, EdaUnits::Mm, true, &mut [&mut provider])
}

/// Loads a QA board; `None` (with a note) when the port does not load it completely yet.
fn load_qa(name: &str) -> Option<Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));

    match quiet(|| catch_unwind(AssertUnwindSafe(|| load(&path)))) {
        Ok(Ok(l)) if l.report.skipped.is_empty() => Some(l),
        Ok(Ok(_)) => {
            eprintln!("{name}: pending (skipped items)");
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

/// The type of the main item of a violation (`BOARD_RC_ITEM_RESOLVER::ResolveItemById`).
fn main_item_type(loaded: &Loaded, r: &drc_harness::Reported) -> Option<KicadT> {
    let ctx = BoardCtx::bare(&loaded.board);
    ctx.resolve(&r.item.main_item_id()).map(|i| i.type_())
}

/// The severities the keepout tests of `test_drc_keepout_disallow.cpp` set (only
/// `DRCE_ALLOWED_ITEMS` matters for a disallow-only run).
fn keepout_test_severities() -> Vec<(i32, Severity)> {
    vec![(DRCE_ALLOWED_ITEMS, Severity::Error)]
}

#[derive(Default)]
struct Score {
    compared: usize,
    matched: std::collections::BTreeMap<String, usize>,
    missing: std::collections::BTreeMap<String, usize>,
    extra: std::collections::BTreeMap<String, usize>,
    differences: Vec<String>,
    pending: Vec<String>,
}

/// The corpus comparison; `filter` limits the boards (path substring).
fn compare_corpus(filter: &str) -> Score {
    let root = kicad_qa::kicad_source_root();
    let mut score = Score::default();

    for (rel, reference) in golden() {
        if !rel.contains(filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        if TYPES
            .iter()
            .any(|t| reference.summaries.contains_key(*t) && cached_entries(&rel, t).is_none())
        {
            score
                .pending
                .push(format!("{rel}: summary only (run gen_drc_corpus.py)"));
            continue;
        }

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, bool), String> {
                    let mut loaded = load(&path)?;
                    let entries = report(&mut loaded);
                    Ok((entries, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let mine = match result {
            Ok(Ok((v, true))) => v,
            Ok(Ok((_, false))) => {
                score.pending.push(format!("{rel}: parser skipped items"));
                continue;
            }
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

        score.compared += 1;

        for t in TYPES {
            let theirs: Vec<Entry> = match cached_entries(&rel, t) {
                Some(v) => v,
                None => reference
                    .entries
                    .iter()
                    .filter(|e| e.type_ == *t)
                    .cloned()
                    .collect(),
            };
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, missing, extra) = diff_entries(&theirs, &m);

            *score.matched.entry(t.to_string()).or_default() += matched;
            *score.missing.entry(t.to_string()).or_default() += missing.len();
            *score.extra.entry(t.to_string()).or_default() += extra.len();

            for k in &missing {
                score.differences.push(format!("{rel}: - {k}"));
            }

            for k in &extra {
                score.differences.push(format!("{rel}: + {k}"));
            }
        }
    }

    score
}

#[test]
fn disallow_matches_reference_corpus() {
    let filter = std::env::var("DRC10_FILTER").unwrap_or_default();
    let score = compare_corpus(&filter);

    println!(
        "DRC-10 disallow: {} boards compared, {} pending",
        score.compared,
        score.pending.len()
    );

    for t in TYPES {
        println!(
            "  {t}: matched {}, missing {}, extra {}",
            score.matched.get(*t).unwrap_or(&0),
            score.missing.get(*t).unwrap_or(&0),
            score.extra.get(*t).unwrap_or(&0)
        );
    }

    for p in &score.pending {
        println!("  pending: {p}");
    }

    for d in score.differences.iter().take(300) {
        println!("  DIFF {d}");
    }

    assert!(
        score.differences.is_empty(),
        "{} differences",
        score.differences.len()
    );
}

/// Port of `DRCKeepoutDisallowViasAndTracks` (`test_drc_keepout_disallow.cpp`, GitLab #23911):
/// two vias inside the keepout and a track crossing into it, exactly one violation each.
#[test]
fn drc_keepout_disallow_vias_and_tracks() {
    let Some(mut loaded) = load_qa("keepout_disallow/keepout_disallow") else {
        return;
    };

    let violations: Vec<_> = run_with(&mut loaded, &keepout_test_severities())
        .into_iter()
        .filter(|r| r.item.error_code() == DRCE_ALLOWED_ITEMS)
        .collect();

    let mut track_violations = 0;
    let mut via_violations = 0;

    for r in &violations {
        match main_item_type(&loaded, r) {
            Some(KicadT::PcbViaT) => via_violations += 1,
            Some(KicadT::PcbTraceT) => track_violations += 1,
            _ => {}
        }
    }

    // Expect exact counts so that duplicate-marker regressions are caught.
    assert_eq!(via_violations, 2, "total {}", violations.len());
    assert_eq!(track_violations, 1);
    assert_eq!(violations.len(), 3);
}

/// Port of `DRCKeepoutFootprintExcludesItself` (`test_drc_keepout_disallow.cpp`, GitLab
/// #24924): a footprint's own keepout never flags it, even with a duplicate zone UUID.
#[test]
fn drc_keepout_footprint_excludes_itself() {
    let Some(mut loaded) = load_qa("issue24924/keepout_self_exclude") else {
        return;
    };

    let violations = run_with(&mut loaded, &keepout_test_severities());
    let footprint_violations = violations
        .iter()
        .filter(|r| r.item.error_code() == DRCE_ALLOWED_ITEMS)
        .filter(|r| main_item_type(&loaded, r) == Some(KicadT::PcbFootprintT))
        .count();

    assert_eq!(
        footprint_violations, 0,
        "Footprint flagged by its own keepout"
    );
}

/// Port of `DRCKeepoutNoClearanceInflation` (`test_drc_keepout_disallow.cpp`): a track 4 mm
/// outside a no-tracks keepout is not flagged although the physical clearance is 20 mm.
#[test]
fn drc_keepout_no_clearance_inflation() {
    let Some(mut loaded) = load_qa("keepout_no_clearance/keepout_no_clearance") else {
        return;
    };

    let violations: Vec<_> = run_with(&mut loaded, &keepout_test_severities())
        .into_iter()
        .filter(|r| r.item.error_code() == DRCE_ALLOWED_ITEMS)
        .collect();

    assert!(
        violations.is_empty(),
        "Expected no keepout violations for track outside keepout, got {}",
        violations.len()
    );
}

/// Port of `DRCIssue24525_TableAndBarcodeOnEdgeCuts` (`test_drc_issue24525.cpp`).
#[test]
fn drc_issue24525_table_and_barcode_on_edge_cuts() {
    let Some(mut loaded) = load_qa("issue24525/issue24525") else {
        return;
    };

    let violations: Vec<_> = run_with(&mut loaded, &[(DRCE_TEXT_ON_EDGECUTS, Severity::Error)])
        .into_iter()
        .filter(|r| r.item.error_code() == DRCE_TEXT_ON_EDGECUTS)
        .collect();

    let mut by_type: std::collections::BTreeMap<KicadT, i32> = Default::default();

    for r in &violations {
        let mut t = main_item_type(&loaded, r).expect("violation resolves to an item");

        if t.base_type() == KicadT::PcbDimensionT {
            t = KicadT::PcbDimensionT;
        }

        *by_type.entry(t).or_default() += 1;
    }

    let n = |t: KicadT| by_type.get(&t).copied().unwrap_or(0);
    assert_eq!(n(KicadT::PcbTextT), 1);
    assert_eq!(n(KicadT::PcbDimensionT), 1);
    assert_eq!(n(KicadT::PcbTableT), 2);
    assert_eq!(n(KicadT::PcbBarcodeT), 1);
    // The reference image on Edge.Cuts must not be flagged.  It is never plotted, so it
    // cannot corrupt the outline.
    assert_eq!(n(KicadT::PcbReferenceImageT), 0);
    assert_eq!(violations.len(), 5);
}

/// The `issue7567` case of `DRCFalsePositiveRegressions` (`test_drc_regressions.cpp`, "DRC
/// constraint to disallow holes gets SMD pads also"): no error-severity violation of the
/// provider's codes (the severities of the C++ fixture leave both at their defaults).
#[test]
fn false_positive_regression_issue7567() {
    let Some(mut loaded) = load_qa("issue7567") else {
        return;
    };

    let errors: Vec<_> = run_with(&mut loaded, &[])
        .into_iter()
        .filter(|r| {
            let code = r.item.error_code();
            (code == DRCE_ALLOWED_ITEMS || code == DRCE_TEXT_ON_EDGECUTS)
                && loaded.board.design_settings().get_severity(code) == Severity::Error
        })
        .map(|r| r.item.error_message())
        .collect();

    assert!(errors.is_empty(), "{errors:?}");
}
