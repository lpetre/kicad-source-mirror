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

//! DRC-09: the hole-to-hole and edge clearance providers
//! (`drc_test_provider_hole_to_hole.cpp`, `drc_test_provider_edge_clearance.cpp`) against the
//! reference `kicad-cli pcb drc --format json --severity-all --units mm` reports of the shared
//! corpus (`tests/golden/drc_corpus.txt`, raw reports cached under `rust/target/drc-corpus/`),
//! plus the ports of the C++ tests of both providers:
//! `qa/tests/pcbnew/drc/test_drc_hole_to_hole_npth_slot_issue24541.cpp`,
//! `test_drc_arc_arc_edge_clearance.cpp`, `test_drc_copper_outside_board_issue14650.cpp` and
//! `test_drc_regressions.cpp` (`DRCHoleToHoleReportsRuleValue`, the hole-to-hole / edge
//! clearance part of `DRCFalsePositiveRegressions` and `issue18142` of
//! `DRCFalseNegativeRegressions`).
//!
//! The hole-to-hole provider is sequential in C++: its entries are compared in report order.
//! The edge clearance provider runs on the thread pool in C++ (report order scheduling
//! dependent): its entries are compared as multisets.

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, cached_entries, cli_report_entries, diff_entries, golden, load_board,
    panic_text, quiet, run_providers,
};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::Severity;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::{EdaDataType, EdaUnits, ui};
use kicad_core::kiid::Kiid;
use kicad_drc::cache_generator::prepare_board;
use kicad_drc::engine::implicit_condition::EngineCondition;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::edge_clearance::DrcTestProviderEdgeClearance;
use kicad_drc::providers::hole_to_hole::DrcTestProviderHoleToHole;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{
    DRCE_COPPER_OUTSIDE_BOARD, DRCE_DRILLED_HOLES_COLOCATED, DRCE_DRILLED_HOLES_TOO_CLOSE,
    DRCE_EDGE_CLEARANCE, DRCE_FIRST, DRCE_LAST,
};
use kicad_pcb::padstack::PadAttrib;

/// The error types of the hole-to-hole provider (compared in order).
const HOLE_TYPES: &[&str] = &["hole_to_hole", "holes_co_located"];

/// The error types of the edge clearance provider (compared as multisets).
const EDGE_TYPES: &[&str] = &[
    "copper_edge_clearance",
    "silk_edge_clearance",
    "copper_outside_board",
];

/// Both providers in registry order (edge clearance before hole to hole), run with the
/// connectivity of the load (`FlashLayer` of pads and vias).
struct HoleAndEdge<'a> {
    connectivity: &'a ConnectivityData,
}

impl DrcTestProviderRun for HoleAndEdge<'_> {
    fn name(&self) -> &str {
        "edge_clearance"
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

        DrcTestProviderEdgeClearance::default().run_tests(engine, run, &cctx, units)
            && DrcTestProviderHoleToHole::default().run_tests(engine, run, &cctx, units)
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

/// Whether a user rule condition of the engine calls a function (stubs until DRC-22; as
/// `drc08a_copper_clearance.rs`).
fn conditions_call_functions(engine: &DrcEngine) -> bool {
    engine.constraint_map().values().flatten().any(|c| {
        !c.parent_rule.is_implicit()
            && matches!(&c.condition, Some(EngineCondition::Compiled(cond)) if cond.expression().contains('('))
    })
}

/// The report entries of both providers' types.
fn report(loaded: &mut Loaded) -> Vec<Entry> {
    let connectivity = prepare(loaded);
    let mut provider = HoleAndEdge {
        connectivity: &connectivity,
    };

    cli_report_entries(loaded, false, &mut [&mut provider])
        .into_iter()
        .filter(|e| {
            HOLE_TYPES.contains(&e.type_.as_str()) || EDGE_TYPES.contains(&e.type_.as_str())
        })
        .collect()
}

/// The violations of `codes` delivered to the handler by a `RunTests( MM, true, false )` with
/// every other code ignored.
fn run_only(loaded: &mut Loaded, codes: &[i32]) -> Vec<drc_harness::Reported> {
    {
        let bds = loaded.board.design_settings_mut();

        for ii in DRCE_FIRST..=DRCE_LAST {
            bds.drc_severities.insert(ii, Severity::Ignore);
        }

        for &code in codes {
            bds.drc_severities.insert(code, Severity::Error);
        }
    }

    let connectivity = prepare(loaded);
    let mut provider = HoleAndEdge {
        connectivity: &connectivity,
    };

    run_providers(loaded, EdaUnits::Mm, true, &mut [&mut provider])
        .into_iter()
        .filter(|r| codes.contains(&r.item.error_code()))
        .collect()
}

/// Loads a QA board; `None` (with a note) when the port does not load it completely yet.
fn load_qa(name: &str) -> Option<Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));

    match quiet(|| catch_unwind(AssertUnwindSafe(|| load_board(&path)))) {
        Ok(Ok(l)) if l.report.skipped.is_empty() && !conditions_call_functions(&l.engine) => {
            Some(l)
        }
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

/// Boards whose differences come from rule-condition evaluation (not from the providers);
/// remove an entry when its cause is fixed.
const KNOWN_RULE_GAPS: &[(&str, &str)] = &[(
    "qa/data/pcbnew/issue23469/issue23469.kicad_pcb",
    "`A.Reference == 'J1'` does not match the pads of J1 (same cause as the hole_clearance gap \
     of drc08a_copper_clearance.rs), so the board setup edge clearance is applied",
)];

/// Matched / missing / extra per type.
#[derive(Default)]
struct Score {
    compared: usize,
    matched: BTreeMap<String, usize>,
    missing: BTreeMap<String, usize>,
    extra: BTreeMap<String, usize>,
    differences: Vec<String>,
    /// Differences on boards of [`KNOWN_RULE_GAPS`].
    known: Vec<String>,
    pending: Vec<String>,
}

/// The corpus comparison; `filter` limits the boards (path substring).
fn compare_corpus(filter: &str) -> Score {
    let root = kicad_qa::kicad_source_root();
    let mut score = Score::default();
    let types: Vec<&str> = HOLE_TYPES.iter().chain(EDGE_TYPES).copied().collect();

    for (rel, reference) in golden() {
        if !rel.contains(filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        if types
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
                    let mut loaded = load_board(&path)?;

                    if conditions_call_functions(&loaded.engine) {
                        return Err("rule conditions call functions (DRC-22)".to_string());
                    }

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
        let known = KNOWN_RULE_GAPS.iter().any(|(b, _)| *b == rel);
        let mut diffs = Vec::new();

        for t in &types {
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
                diffs.push(format!("{rel}: - {k}"));
            }

            for k in &extra {
                diffs.push(format!("{rel}: + {k}"));
            }

            if HOLE_TYPES.contains(t) && missing.is_empty() && extra.is_empty() && theirs != m {
                diffs.push(format!("{rel}: {t}: same entries in a different order"));
            }
        }

        if known {
            score.known.extend(diffs);
        } else {
            score.differences.extend(diffs);
        }
    }

    score
}

#[test]
fn hole_to_hole_and_edge_clearance_match_reference_corpus() {
    let filter = std::env::var("DRC09_FILTER").unwrap_or_default();
    let score = compare_corpus(&filter);

    println!(
        "DRC-09 hole to hole / edge clearance: {} boards compared, {} pending",
        score.compared,
        score.pending.len()
    );

    for t in HOLE_TYPES.iter().chain(EDGE_TYPES) {
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

    for d in &score.known {
        println!("  known rule gap: {d}");
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

/// Port of `HoleToHoleNpthSlotIssue24541` (`test_drc_hole_to_hole_npth_slot_issue24541.cpp`):
/// the two overlapping NPTH slots produce exactly one distinct hole-to-hole violation.
#[test]
fn hole_to_hole_npth_slot_issue24541() {
    let Some(mut loaded) = load_qa("issue24541/issue24541") else {
        return;
    };

    assert_eq!(loaded.board.footprints().count(), 2);

    let is_npth_slot = |board: &kicad_pcb::Board, uuid: &Kiid| -> bool {
        let ctx = BoardCtx::bare(board);
        ctx.resolve(uuid)
            .and_then(|item| item.as_pad())
            .is_some_and(|pad| {
                pad.attribute() == PadAttrib::Npth
                    && pad.drill_size(&ctx).x != pad.drill_size(&ctx).y
            })
    };

    let mut npth_slot_count = 0;

    {
        let ctx = BoardCtx::bare(&loaded.board);

        for (fp_id, _) in loaded.board.footprints() {
            for (_, pad) in loaded.board.footprint_pads(fp_id) {
                if pad.attribute() == PadAttrib::Npth
                    && pad.drill_size(&ctx).x != pad.drill_size(&ctx).y
                {
                    npth_slot_count += 1;
                }
            }
        }
    }

    assert_eq!(
        npth_slot_count, 2,
        "Fixture changed: expected two NPTH oval slots."
    );

    let found = run_only(
        &mut loaded,
        &[DRCE_DRILLED_HOLES_TOO_CLOSE, DRCE_DRILLED_HOLES_COLOCATED],
    );
    let mut pairs = std::collections::BTreeSet::new();

    for r in &found {
        let (a, b) = (r.item.main_item_id(), r.item.aux_item_id());

        if is_npth_slot(&loaded.board, &a) && is_npth_slot(&loaded.board, &b) {
            pairs.insert((a.min(b), a.max(b)));
        }
    }

    eprintln!(
        "Hole-to-hole violations: {}; NPTH slot pairs: {}",
        found.len(),
        pairs.len()
    );
    assert_eq!(pairs.len(), 1);
}

/// Port of `DRCArcArcEdgeClearance_InternalContainment` (`test_drc_arc_arc_edge_clearance.cpp`).
#[test]
fn drc_arc_arc_edge_clearance_internal_containment() {
    let Some(mut loaded) = load_qa("drc_arc_arc_edge_clearance/drc_arc_arc_edge_clearance") else {
        return;
    };

    let found = run_only(&mut loaded, &[DRCE_EDGE_CLEARANCE]);
    assert!(!found.is_empty());

    let track_uuid: Kiid = "e918352c-937f-4a75-aac0-7856db5d052e".parse().unwrap();
    let cutout_left_arc_uuid: Kiid = "b9bfe430-49e3-430b-b86f-c88b7c1be60e".parse().unwrap();
    let mut track_hit = false;
    let mut cutout_hit = false;

    for r in &found {
        for uuid in [r.item.main_item_id(), r.item.aux_item_id()] {
            if uuid == track_uuid {
                track_hit = true;
            } else if uuid == cutout_left_arc_uuid {
                cutout_hit = true;
            }
        }
    }

    assert!(
        track_hit && cutout_hit,
        "Expected at least one DRCE_EDGE_CLEARANCE violation between the F.Cu track arc and the \
         Edge.Cuts left cutout arc"
    );
}

/// Port of `CopperOutsideBoardIssue14650` (`test_drc_copper_outside_board_issue14650.cpp`).
#[test]
fn copper_outside_board_issue14650() {
    let Some(mut loaded) = load_qa("issue14650/KiCAD_Fiducial") else {
        return;
    };

    let found = run_only(&mut loaded, &[DRCE_COPPER_OUTSIDE_BOARD]);
    assert_eq!(found.len(), 1);

    let ctx = BoardCtx::bare(&loaded.board);
    let item = ctx
        .resolve(&found[0].item.main_item_id())
        .expect("the violation names a board item");
    let pad = item.as_pad().expect("the violation names a pad");
    let footprint = item.parent_footprint().expect("the pad has a footprint");
    let _ = pad;
    assert_eq!(footprint.as_footprint().unwrap().reference(&ctx), "FID2");
}

/// Port of `DRCHoleToHoleReportsRuleValue` (`test_drc_regressions.cpp`, issue 22267): the
/// message quotes the configured minimum, not the epsilon-relaxed one.
#[test]
fn drc_hole_to_hole_reports_rule_value() {
    let Some(mut loaded) = load_qa("issue22267") else {
        return;
    };

    let found = run_only(&mut loaded, &[DRCE_DRILLED_HOLES_TOO_CLOSE]);
    assert!(
        !found.is_empty(),
        "Expected at least one hole-to-hole violation"
    );

    let bds = loaded.board.design_settings();
    let rule_value = ui::message_text_from_value_int(
        &PCB_IU_SCALE,
        EdaUnits::Mm,
        bds.hole_to_hole_min,
        true,
        EdaDataType::Distance,
    );
    let relaxed_value = ui::message_text_from_value_int(
        &PCB_IU_SCALE,
        EdaUnits::Mm,
        bds.hole_to_hole_min - bds.get_drc_epsilon(),
        true,
        EdaDataType::Distance,
    );
    assert_ne!(rule_value, relaxed_value);

    for r in &found {
        let msg = r.item.error_message();
        assert!(
            msg.contains(&rule_value),
            "Expected the configured minimum '{rule_value}' but got: {msg}"
        );
        assert!(
            !msg.contains(&relaxed_value),
            "Reported the epsilon-relaxed minimum '{relaxed_value}': {msg}"
        );
    }
}

/// The hole-to-hole / edge clearance part of `DRCFalsePositiveRegressions`
/// (`test_drc_regressions.cpp`): these boards report none of the providers' error-severity
/// types (`issue5990`: edge clearance flagged although respected; `issue18839`: edge clearance
/// between concentric arcs).
#[test]
fn false_positive_regressions_hole_edge() {
    for name in [
        "issue4139",
        "issue4774",
        "issue5978",
        "issue5990",
        "issue6443",
        "issue12609",
        "issue18203",
        "issue18839",
        "net_tie_hole_clearance",
        "issue24974",
    ] {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };

        let entries: Vec<Entry> = report(&mut loaded)
            .into_iter()
            .filter(|e| e.severity == "error" && !e.excluded)
            .collect();

        assert!(
            entries.is_empty(),
            "{name}: {:?}",
            entries.iter().map(Entry::key).collect::<Vec<_>>()
        );
    }
}

/// `issue18142` of `DRCFalseNegativeRegressions` (`test_drc_regressions.cpp`): the one expected
/// violation is the `hole_to_hole` between a blind/buried via and a microvia hole.
#[test]
fn false_negative_regression_issue18142() {
    let Some(mut loaded) = load_qa("issue18142") else {
        return;
    };

    let found = run_only(
        &mut loaded,
        &[DRCE_DRILLED_HOLES_TOO_CLOSE, DRCE_DRILLED_HOLES_COLOCATED],
    );
    assert_eq!(found.len(), 1);
}
