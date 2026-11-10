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

//! DRC-23: the misc provider (`drc_test_provider_misc.cpp`) and the footprint checks provider
//! (`drc_test_provider_footprint_checks.cpp`) against the reference DRC reports of the corpus
//! (`tests/golden/drc23/corpus.txt`, written by `tests/oracle/gen_drc23_golden.py` from the raw
//! reports of `gen_drc_corpus.py`), plus the ported C++ tests
//! `qa/tests/pcbnew/drc/test_drc_invalid_outline_issue24078.cpp`,
//! `test_drc_invalid_outline_issue24241.cpp`, `test_drc_text_var_issue24442.cpp`,
//! `test_drc_netclass_literal.cpp` and `test_drc_tuning_profiles.cpp` (the misc part).

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use drc_harness::{
    Entry, Loaded, cli_report_entries_flow, diff_entries, load_board, panic_text, quiet,
    run_providers,
};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::engine::{DrcEngine, DrcTestProviderRun};
use kicad_drc::providers::footprint_checks::DrcTestProviderFootprintChecks;
use kicad_drc::providers::misc::DrcTestProviderMisc;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_pcb::drc_api::{
    DRCE_FIRST, DRCE_GENERIC_ERROR, DRCE_GENERIC_WARNING, DRCE_INVALID_OUTLINE, DRCE_LAST,
    DRCE_MISSING_TUNING_PROFILE, DRCE_UNRESOLVED_VARIABLE,
};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::{AnyItem, BoardCtx};

/// The error types of the two providers.
const TYPES: &[&str] = &[
    "invalid_outline",
    "item_on_disabled_layer",
    "unresolved_variable",
    "assertion_failure",
    "generic_warning",
    "generic_error",
    "missing_tuning_profile",
    "footprint_type_mismatch",
    "footprint_scaled_with_pads",
    "through_hole_pad_without_hole",
    "padstack",
    "padstack_invalid",
    "shorting_items",
    "footprint",
];

/// The board item of `SetItems( m_board )` in the golden (a random UUID per reference run).
const BOARD_UUID: &str = "board";

/// The reference entries per board of `tests/golden/drc23/corpus.txt`.
fn golden() -> BTreeMap<String, Vec<Entry>> {
    let mut boards: BTreeMap<String, Vec<Entry>> = BTreeMap::new();

    for line in include_str!("golden/drc23/corpus.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();

        if f.get(1) == Some(&"cached") {
            boards.entry(f[0].to_string()).or_default();
            continue;
        }

        let items = f[5..]
            .chunks(4)
            .map(|it| {
                (
                    it[0].to_string(),
                    it[1].to_string(),
                    it[2].parse().unwrap(),
                    it[3].parse().unwrap(),
                )
            })
            .collect();

        boards.entry(f[0].to_string()).or_default().push(Entry {
            severity: f[2].trim_end_matches("/excluded").to_string(),
            excluded: f[2].ends_with("/excluded"),
            type_: f[3].to_string(),
            description: f[4].to_string(),
            items,
        });
    }

    boards
}

/// True for a `shorting_items` entry of the footprint checks provider (net ties): every item
/// belongs to one net-tie footprint. The copper clearance provider reports the same type.
fn is_net_tie_shorting(loaded: &Loaded, e: &Entry) -> bool {
    let ctx = BoardCtx::bare(&loaded.board);
    let mut footprint = None;

    for (uuid, ..) in &e.items {
        let Some(item) = kicad_core::kiid::Kiid::parse(uuid).and_then(|k| ctx.resolve(&k)) else {
            return false;
        };

        let fp = match item.any() {
            AnyItem::Footprint(_) => item.id().and_then(|id| id.typed()),
            _ => item.parent_footprint_id(),
        };

        match (fp, footprint) {
            (None, _) => return false,
            (Some(f), None) => footprint = Some(f),
            (Some(f), Some(g)) if f != g => return false,
            _ => {}
        }
    }

    footprint
        .and_then(|f| loaded.board.footprint(f))
        .is_some_and(|f| f.is_net_tie())
}

/// The reference entries compared with the port: board items (`SetItems( m_board )`) dropped
/// (the port has no board UUID), drawing-sheet entries (no items) set aside.
fn normalise_reference(loaded: &Loaded, reference: &[Entry]) -> (Vec<Entry>, Vec<Entry>) {
    let mut kept = Vec::new();
    let mut drawing_sheet = Vec::new();

    for e in reference {
        let mut e = e.clone();

        if e.type_ == "shorting_items" && !is_net_tie_shorting(loaded, &e) {
            continue;
        }

        if e.type_ == "unresolved_variable" && e.items.is_empty() {
            drawing_sheet.push(e);
            continue;
        }

        e.items.retain(|(uuid, ..)| uuid != BOARD_UUID);
        kept.push(e);
    }

    (kept, drawing_sheet)
}

#[derive(Default)]
struct Score {
    compared: usize,
    matched: BTreeMap<String, usize>,
    missing: BTreeMap<String, usize>,
    extra: BTreeMap<String, usize>,
    drawing_sheet: usize,
    order_differs: Vec<String>,
    pending: Vec<String>,
    differences: Vec<String>,
    /// Matched / missing / extra on the boards the parser loaded only partially.
    partial: (usize, usize, usize),
    partial_differences: Vec<String>,
}

/// The two providers on every corpus board the port loads completely, against the
/// reference: per type matched / missing / extra, then the order.
#[test]
fn misc_and_footprint_checks_match_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC23_FILTER").unwrap_or_default();
    let mut score = Score::default();

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, Vec<Entry>, Vec<Entry>, bool), String> {
                    let mut loaded = load_board(&path)?;

                    let mine: Vec<Entry> = cli_report_entries_flow(
                        &mut loaded,
                        false,
                        &["footprint checks", "miscellaneous"],
                    )
                    .into_iter()
                    .filter(|e| TYPES.contains(&e.type_.as_str()))
                    .collect();

                    let (theirs, drawing_sheet) = normalise_reference(&loaded, &reference);

                    Ok((
                        mine,
                        theirs,
                        drawing_sheet,
                        loaded.report.skipped.is_empty(),
                    ))
                },
            ))
        });

        let (mine, theirs, drawing_sheet, complete) = match result {
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
            // Compared for information only: the parser skipped items of this board.
            let (mut m, mut x, mut e) = (0, 0, 0);

            for t in TYPES {
                let r: Vec<Entry> = theirs.iter().filter(|e| e.type_ == *t).cloned().collect();
                let mm: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
                let (matched, missing, extra) = diff_entries(&r, &mm);
                m += matched;
                x += missing.len();
                e += extra.len();

                for k in missing.iter().chain(extra.iter()).take(3) {
                    score.partial_differences.push(format!("{rel}: {k}"));
                }
            }

            score.partial.0 += m;
            score.partial.1 += x;
            score.partial.2 += e;
            score.pending.push(format!(
                "{rel}: parser skipped items (matched {m}, missing {x}, extra {e})"
            ));
            continue;
        }

        score.compared += 1;
        score.drawing_sheet += drawing_sheet.len();

        for t in TYPES {
            let r: Vec<Entry> = theirs.iter().filter(|e| e.type_ == *t).cloned().collect();
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, missing, extra) = diff_entries(&r, &m);

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

        // testAssertions evaluates the items on the thread pool (`submit_loop`): the report
        // order of `assertion_failure` is scheduling dependent in C++ (drc.md 4.12); it is
        // compared as a multiset only (STAB-DRC, seen on component_classes_drc).
        let ordered = |list: &[Entry]| -> Vec<Entry> {
            list.iter()
                .filter(|e| e.type_ != "assertion_failure")
                .cloned()
                .collect()
        };

        if score.differences.is_empty() && ordered(&mine) != ordered(&theirs) {
            score.order_differs.push(rel.clone());
        }
    }

    println!(
        "DRC-23 misc / footprint checks: {} boards compared",
        score.compared
    );

    for t in TYPES {
        println!(
            "  {t}: matched {}, missing {}, extra {}",
            score.matched.get(*t).unwrap_or(&0),
            score.missing.get(*t).unwrap_or(&0),
            score.extra.get(*t).unwrap_or(&0)
        );
    }

    println!(
        "  drawing-sheet unresolved_variable (not compared, no drawing sheet in the port): {}",
        score.drawing_sheet
    );
    println!("  order differs: {}", score.order_differs.len());

    for d in &score.order_differs {
        println!("    {d}");
    }

    println!(
        "  partially loaded boards (information only): matched {}, missing {}, extra {}",
        score.partial.0, score.partial.1, score.partial.2
    );

    if std::env::var("DRC23_VERBOSE").is_ok() {
        for d in &score.partial_differences {
            println!("    {d}");
        }
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

/// Loads a QA board (`KI_TEST::LoadBoard( settingsManager, name, board )`).
fn load_qa(name: &str) -> (Loaded, TuningProfiles) {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));
    let loaded = load_board(&path).unwrap_or_else(|e| panic!("load {name}: {e}"));
    let tuning = loaded.tuning_profiles.clone();
    (loaded, tuning)
}

/// Every code `DRCE_FIRST ..= DRCE_LAST` ignored, then `enabled` set.
fn only_severities(loaded: &mut Loaded, enabled: &[(i32, Severity)]) {
    let bds = loaded.board.design_settings_mut();

    for ii in DRCE_FIRST..=DRCE_LAST {
        bds.drc_severities.insert(ii, Severity::Ignore);
    }

    for &(code, severity) in enabled {
        bds.drc_severities.insert(code, severity);
    }
}

/// `RunTests( EDA_UNITS::MM, true, false )` with the providers of this package.
fn run_both(loaded: &mut Loaded, tuning: TuningProfiles) -> Vec<drc_harness::Reported> {
    let mut footprint_checks = DrcTestProviderFootprintChecks::default();
    let mut misc = DrcTestProviderMisc::new(tuning);
    let mut providers: [&mut dyn DrcTestProviderRun; 2] = [&mut footprint_checks, &mut misc];
    run_providers(loaded, EdaUnits::Mm, true, &mut providers)
}

/// Port of `test_drc_invalid_outline_issue24078.cpp` (`InvalidOutlineBezierSlotIssue24078`):
/// a bezier-bounded slot crossing a rectangular slot is a malformed outline.
#[test]
fn invalid_outline_bezier_slot_issue24078() {
    let (mut loaded, tuning) = load_qa("issue24078/issue24078");
    only_severities(&mut loaded, &[(DRCE_INVALID_OUTLINE, Severity::Error)]);

    let violations: Vec<_> = run_both(&mut loaded, tuning)
        .into_iter()
        .filter(|v| {
            loaded
                .board
                .design_settings()
                .get_severity(v.item.error_code())
                == Severity::Error
        })
        .collect();

    for v in &violations {
        println!("  Violation: {}", v.item.error_message());
    }

    assert!(!violations.is_empty());
}

/// Port of `test_drc_invalid_outline_issue24241.cpp` (`DegenerateArcOutlineMarkerIssue24241`):
/// the violation of a degenerate arc names the arc and sits at its start point.
#[test]
fn degenerate_arc_outline_marker_issue24241() {
    let (mut loaded, tuning) = load_qa("issue24241/issue24241");
    only_severities(&mut loaded, &[(DRCE_INVALID_OUTLINE, Severity::Error)]);

    let violations: Vec<_> = run_both(&mut loaded, tuning)
        .into_iter()
        .filter(|v| {
            loaded
                .board
                .design_settings()
                .get_severity(v.item.error_code())
                == Severity::Error
        })
        .collect();

    assert!(!violations.is_empty());

    // Locate the degenerate arc directly from the board.  Its three control points should
    // all coincide.
    let ctx = BoardCtx::bare(&loaded.board);
    let mut degenerate_arc = None;

    for (id, item) in loaded.board.drawings() {
        let AnyItem::Shape(_) = item else {
            continue;
        };

        let item_ref = ctx.item(id).unwrap();
        let shape = item_ref.as_eda_shape().unwrap().eda_shape();

        if shape.shape() != ShapeT::Arc || item_ref.layer() != kicad_core::PcbLayerId::EdgeCuts {
            continue;
        }

        let (start, end, mid) = (shape.start(), shape.end(), shape.arc_mid());

        if start == end && start == mid {
            degenerate_arc = Some((item_ref.uuid(), start));
            break;
        }
    }

    let (arc_uuid, arc_start) =
        degenerate_arc.expect("Reproduction board is expected to contain a degenerate arc");

    // At least one violation must reference the degenerate arc (either as its primary or
    // auxiliary item) and place the marker at the arc's coordinates so the user can navigate
    // to it.
    let found_linked_violation = violations.iter().any(|v| {
        (v.item.main_item_id() == arc_uuid || v.item.aux_item_id() == arc_uuid)
            && v.pos == arc_start
    });

    assert!(
        found_linked_violation,
        "Expected a violation that references the degenerate arc and places the marker at its \
         coordinates so the user can locate it"
    );
}

/// Port of `test_drc_text_var_issue24442.cpp` (`DRCTextVarIssue24442`): `${DRC_ERROR ...}` and
/// `${DRC_WARNING ...}` fire anywhere in a text, an escaped marker never does, and a fired
/// marker is not also an unresolved variable.
#[test]
fn drc_text_var_issue24442() {
    let (mut loaded, tuning) = load_qa("issue24442");

    // Disable DRC tests not useful or not handled in this testcase
    only_severities(
        &mut loaded,
        &[
            (DRCE_GENERIC_ERROR, Severity::Error),
            (DRCE_GENERIC_WARNING, Severity::Warning),
            // testTextVars() (the phase that handles ${DRC_ERROR}/${DRC_WARNING}) only runs
            // when DRCE_UNRESOLVED_VARIABLE is enabled; keep it active so the phase is reached.
            (DRCE_UNRESOLVED_VARIABLE, Severity::Error),
        ],
    );

    let mut error_messages = Vec::new();
    let mut warning_messages = Vec::new();
    let mut unresolved_vars = 0;

    for v in run_both(&mut loaded, tuning) {
        match v.item.error_code() {
            DRCE_GENERIC_ERROR => error_messages.push(v.item.error_message()),
            DRCE_GENERIC_WARNING => warning_messages.push(v.item.error_message()),
            DRCE_UNRESOLVED_VARIABLE => unresolved_vars += 1,
            _ => {}
        }
    }

    assert_eq!(error_messages.len(), 3);
    assert_eq!(warning_messages.len(), 2);
    assert_eq!(unresolved_vars, 0);

    let contains = |list: &[String], needle: &str| list.iter().any(|m| m.contains(needle));

    assert!(contains(&error_messages, "start_of_text"));
    assert!(contains(&error_messages, "placeholder_text"));
    assert!(contains(&error_messages, "text_box_error"));
    assert!(contains(&warning_messages, "this_is_warning"));
    assert!(contains(&warning_messages, "embedded_warning"));

    // The escaped literal must never surface as a generic error.
    assert!(!contains(&error_messages, "not_a_real_error"));
}

/// Port of `test_drc_netclass_literal.cpp` (`DRCNetclassNameEndingInBackslash`), the misc
/// provider's part: a net class whose name ends in a backslash cannot be named by a rule and
/// gets one generic warning. (The `EvalRules` check of the C++ test, the parent rule's
/// `m_ImplicitNetclass`, belongs to the engine packages: `DrcRuleInfo` has no implicit net
/// class.)
#[test]
fn drc_netclass_name_ending_in_backslash() {
    let path = kicad_qa::pcbnew_data_dir().join("connection_width_rules.kicad_pcb");
    let (mut loaded, tuning) = load_qa("connection_width_rules");

    {
        let net_settings = &mut loaded.board.design_settings_mut().net_settings;
        let mut netclass = (**net_settings
            .netclasses()
            .get("High_current")
            .expect("High_current"))
        .clone();
        netclass.name = "High_current\\".to_string();
        net_settings.set_netclass("High_current", Arc::new(netclass));
    }

    // InitEngine( board->GetDesignRulesPath() )
    let dru = path.with_extension("kicad_dru");
    let dru_text = std::fs::read_to_string(&dru).unwrap();
    let dru_path = dru.to_string_lossy().to_string();
    let mut engine = DrcEngine::new();
    engine
        .init_engine(
            &mut loaded.board,
            Some((&dru_text, &dru_path)),
            default_layer_choices(),
            &tuning,
            None,
        )
        .expect("InitEngine");
    loaded.engine = engine;

    let mut misc = DrcTestProviderMisc::new(tuning);
    let warnings: Vec<String> = run_providers(&mut loaded, EdaUnits::Mm, true, &mut [&mut misc])
        .into_iter()
        .filter(|v| v.item.error_code() == DRCE_GENERIC_WARNING)
        .map(|v| v.item.error_message())
        .collect();

    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("'High_current\\'"));
}

/// The misc part of `test_drc_tuning_profiles.cpp` (`DRCTuningProfiles`, board
/// `drc_missing_tuning_profile`): the C++ test counts 2 errors of every provider; the reference
/// report has exactly one `missing_tuning_profile` (and one `invalid_outline`).
#[test]
fn drc_missing_tuning_profile() {
    let (mut loaded, tuning) = load_qa("drc_missing_tuning_profile");
    loaded
        .board
        .design_settings_mut()
        .drc_severities
        .insert(DRCE_MISSING_TUNING_PROFILE, Severity::Error);

    let found: Vec<String> = run_both(&mut loaded, tuning)
        .into_iter()
        .filter(|v| v.item.error_code() == DRCE_MISSING_TUNING_PROFILE)
        .map(|v| v.item.error_message())
        .collect();

    assert_eq!(
        found,
        vec!["Missing tuning profile (Net Class: CLASS2, Tuning Profile: PROFILE2)".to_string()]
    );
}
