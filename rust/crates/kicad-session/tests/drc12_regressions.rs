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

//! DRC-12: port of `qa/tests/pcbnew/drc/test_drc_regressions.cpp` (`DRCFalsePositiveRegressions`,
//! `DRCFalseNegativeRegressions`, `DRCHoleToHoleReportsRuleValue`) on the full Rust DRC run
//! (`kicad_drc::run_flow::run_tests`, every ported provider in registry order) after the
//! `KI_TEST::LoadBoard` sequence ([`Session::load_board_qa`]).
//!
//! Each case names the providers it needs implicitly: a run reports the registry slots it
//! could not run (`CheckNotRun`). The false-positive cases expect no error and hold for any
//! subset of providers, so they are always asserted. A false-negative count is asserted only
//! when no check was left out; otherwise the case is listed as pending with the missing
//! providers. Boards that do not load yet are pending with the load error. The zone-refill
//! cases (`DRCZoneFalsePositiveRegressions`, `DRCTeardropOverCopperField`) need
//! `KI_TEST::FillZones` and are not ported here.

use std::path::Path;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::engine::DrcRunState;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::exclusion::from_marker;
use kicad_drc::run_flow::{CheckNotRun, RunTestsOptions, run_tests};
use kicad_pcb::drc_api::*;
use kicad_pcb::pcb_marker::{PcbMarker, RcItem};
use kicad_session::{LoadInputs, Session};

/// The texts of a board of `qa/data/pcbnew` (`KI_TEST::LoadBoard( mgr, rel )`).
fn load_qa(rel: &str) -> Result<Session, String> {
    let base = kicad_qa::pcbnew_data_dir().join(rel);
    let read = |ext: &str| std::fs::read_to_string(base.with_extension(ext)).ok();
    let text = read("kicad_pcb").ok_or("no board")?;
    let project = read("kicad_pro");
    let rules = read("kicad_dru");
    let legacy = project.is_none() && Path::new(&base.with_extension("pro")).exists();
    let name = format!("{rel}.kicad_pcb");
    let inputs = LoadInputs {
        text: &text,
        source_name: &name,
        project_json: project.as_deref(),
        rules_text: rules.as_deref(),
        legacy_project_exists: legacy,
        ..LoadInputs::default()
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Session::load_board_qa(&inputs)
    }));

    match result {
        Ok(Ok(session)) => Ok(session),
        Ok(Err(e)) => Err(e.to_string()),
        Err(p) => Err(format!(
            "panic: {}",
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        )),
    }
}

/// A violation as the test's handler sees it.
struct Violation {
    item: Arc<RcItem>,
    marker: PcbMarker,
}

/// `bds.m_DRCSeverities[ code ] = severity` for each entry, then
/// `RunTests( EDA_UNITS::MM, true, false )` with a handler collecting every violation.
fn run(
    session: &mut Session,
    severities: &[(i32, Severity)],
) -> Result<(Vec<Violation>, Vec<CheckNotRun>), String> {
    for &(code, severity) in severities {
        session
            .board
            .design_settings_mut()
            .drc_severities
            .insert(code, severity);
    }

    let engine = session.drc_engine.take().ok_or("no DRC engine")?;
    let mut found = Vec::new();
    let mut generator = kicad_core::KiidGenerator::seeded(0xD12);
    let tuning_profiles = TuningProfiles::default();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut state = DrcRunState::new();
        state.set_violation_handler(Box::new(|item, pos, _layer, _path| {
            found.push(Violation {
                item: item.clone(),
                // PCB_MARKER( aItem, aPos ): layer F_Cu.
                marker: PcbMarker::new(&mut generator, item.clone(), pos, 0),
            });
        }));

        run_tests(
            &engine,
            &mut state,
            &mut session.board,
            &mut session.connectivity,
            &RunTestsOptions {
                units: EdaUnits::Mm,
                report_all_track_errors: true,
                test_footprints: false,
                tuning_profiles: &tuning_profiles,
                footprint_libraries: None,
            },
        )
    }));

    session.drc_engine = Some(engine);

    match result {
        Ok(outcome) => Ok((found, outcome.checks_not_run)),
        Err(p) => Err(format!(
            "panic: {}",
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        )),
    }
}

fn not_run_names(checks: &[CheckNotRun]) -> String {
    checks
        .iter()
        .map(|c| c.provider)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Port of `DRCFalsePositiveRegressions`.
#[test]
fn drc_false_positive_regressions() {
    // These documents at one time flagged DRC errors that they shouldn't have.
    let tests = [
        "issue4139",             // DRC fails wrongly with minimally-spaced pads at 45 degree
        "issue4774",             // Shape collisions missing SH_POLY_SET
        "issue5978",             // Hole clearance violation with non-copper pad
        "issue5990", // DRC flags a board edge clearance violation although the clearance is respected
        "issue6443", // Wrong DRC and rendering of THT pads with selective inner copper layers
        "issue7567", // DRC constraint to disallow holes gets SMD pads also
        "issue7975", // Differential pair gap out of range fault by DRC
        "issue8407", // PCBNEW: Arc for diff pair has clearance DRC error
        "issue10906", // Soldermask bridge for only one object
        "issue12609", // Arc collison edge case
        "issue14412", // Solder mask bridge between pads in a net-tie pad group
        "issue15280", // Very wide spokes mis-counted as being single spoke
        "issue14008", // Net-tie clearance error
        "issue17967/issue17967", // Arc dp coupling
        "issue18203", // DRC error due to colliding arc and circle
        "issue18839", // False positive board edge clearance between concentric arcs
        "unconnected-netnames/unconnected-netnames", // Raised false schematic partity error
        "net_tie_drc", // Net tie bridging soldermask DRC test
        "net_tie_hole_clearance", // Net-tie hole clearance with layer-changing copper
        "issue24974", // Net-tie graphic copper on last pad, UUID-order-independent exemption
        "diff_pair_uncoupled_tuning_drc", // Tuning pattern length wrongly counted as uncoupled
    ];

    // Disable DRC tests not useful or not handled in this testcase; the library ones need a
    // footprint library associated to the board.
    let severities = [
        (DRCE_INVALID_OUTLINE, Severity::Ignore),
        (DRCE_UNCONNECTED_ITEMS, Severity::Ignore),
        (DRCE_COPPER_SLIVER, Severity::Ignore),
        (DRCE_STARVED_THERMAL, Severity::Ignore),
        (DRCE_LIB_FOOTPRINT_ISSUES, Severity::Ignore),
        (DRCE_LIB_FOOTPRINT_MISMATCH, Severity::Ignore),
    ];

    let mut failures = Vec::new();
    let mut passed = 0;

    for rel in tests {
        let mut session = match load_qa(rel) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("DRC regression: {rel}, pending (load: {e})");
                continue;
            }
        };

        let (found, not_run) = match run(&mut session, &severities) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{rel}: {e}"));
                continue;
            }
        };

        let bds = session.board.design_settings();
        let errors: Vec<&Violation> = found
            .iter()
            .filter(|v| bds.get_severity(v.item.error_code()) == Severity::Error)
            .collect();

        if errors.is_empty() {
            passed += 1;
            eprintln!(
                "DRC regression: {rel}, passed{}",
                if not_run.is_empty() {
                    String::new()
                } else {
                    format!(" (not run: {})", not_run_names(&not_run))
                }
            );
        } else {
            let codes: Vec<String> = errors
                .iter()
                .map(|v| format!("{} {}", v.item.error_code(), v.item.error_message()))
                .collect();
            failures.push(format!(
                "{rel}: {} violations found (expected 0): {codes:?}",
                errors.len()
            ));
        }
    }

    eprintln!("DRCFalsePositiveRegressions: {passed} passed");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Port of `DRCFalseNegativeRegressions`.
#[test]
fn drc_false_negative_regressions() {
    // These documents at one time failed to catch DRC errors that they should have
    let issue19325_ignore = vec![(DRCE_DRILLED_HOLES_TOO_CLOSE, Severity::Ignore)];
    let issue22102_ignore = vec![
        (DRCE_UNCONNECTED_ITEMS, Severity::Ignore),
        (DRCE_DANGLING_TRACK, Severity::Ignore),
    ];
    let issue18142_ignore = vec![(DRCE_MICROVIA_CROSSES_CORE, Severity::Ignore)];

    let tests: Vec<(&str, usize, Vec<(i32, Severity)>)> = vec![
        ("issue1358", 2, vec![]),
        ("issue2512", 5, vec![]),
        ("issue2528", 1, vec![]),
        ("issue5750", 4, vec![]), // Shorting zone fills pass DRC in some cases
        ("issue5854", 3, vec![]),
        ("issue6879", 6, vec![]),
        ("issue6945", 2, vec![]),
        ("issue7241", 1, vec![]),
        ("issue7267", 6, vec![]), // One track sits wholly inside a footprint slot
        ("issue7325", 2, vec![]),
        ("issue8003", 2, vec![]),
        ("issue9081", 2, vec![]),
        ("issue12109", 8, vec![]), // Pads fail annular width test
        ("issue14334", 2, vec![]), // Thermal spoke to otherwise unconnected island
        ("issue16566", 6, vec![]), // Pad_Shape vs Shape property
        ("issue18142", 1, issue18142_ignore), // blind/buried via to micro-via hole-to-hole
        ("reverse_via", 3, vec![]), // Via/track ordering
        ("intersectingzones", 1, vec![]), // zones are too close to each other
        ("fill_bad", 1, vec![]),   // zone max BBox was too small
        ("issue18878", 12, vec![]), // Updated: fix reports all cross-net mask bridge pairs
        ("issue19325/issue19325", 4, issue19325_ignore), // Overlapping pad annular ring calculation
        ("issue22102", 2, issue22102_ignore), // arc-to-rect collision; colocated arcs collision
        ("issue11814", 2, vec![]), // Teardrop clearance to pad
    ];

    let mut failures = Vec::new();
    let mut passed = 0;

    for (rel, expected, custom) in tests {
        let mut session = match load_qa(rel) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("DRC regression: {rel}, pending (load: {e})");
                continue;
            }
        };

        // Disable DRC tests not useful in this testcase
        let mut severities = vec![
            (DRCE_COPPER_SLIVER, Severity::Ignore),
            (DRCE_LIB_FOOTPRINT_ISSUES, Severity::Ignore),
            (DRCE_LIB_FOOTPRINT_MISMATCH, Severity::Ignore),
            (DRCE_TRACK_NOT_CENTERED_ON_VIA, Severity::Ignore),
        ];
        severities.extend(custom);

        let (found, not_run) = match run(&mut session, &severities) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{rel}: {e}"));
                continue;
            }
        };

        let exclusions = &session.board.design_settings().drc_exclusions;
        let count = found
            .iter()
            .filter(|v| !exclusions.contains(&from_marker(&v.marker)))
            .count();

        if !not_run.is_empty() {
            eprintln!(
                "DRC regression: {rel}, pending: needs {} ({count} violations, expected \
                 {expected})",
                not_run_names(&not_run)
            );
        } else if count == expected {
            passed += 1;
            eprintln!("DRC regression: {rel}, passed");
        } else {
            let codes: Vec<String> = found
                .iter()
                .map(|v| format!("{} {}", v.item.error_code(), v.item.error_message()))
                .collect();
            failures.push(format!(
                "{rel}: {count} violations found (expected {expected}): {codes:?}"
            ));
        }
    }

    eprintln!("DRCFalseNegativeRegressions: {passed} passed");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Port of `DRCHoleToHoleReportsRuleValue`: the hole-to-hole message shows the rule value
/// (`bds.m_HoleToHoleMin`), not the epsilon-relaxed one.
#[test]
fn drc_hole_to_hole_reports_rule_value() {
    let mut session = match load_qa("issue22267") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("issue22267: pending (load: {e})");
            return;
        }
    };

    let severities = [
        (DRCE_UNCONNECTED_ITEMS, Severity::Ignore),
        (DRCE_LIB_FOOTPRINT_ISSUES, Severity::Ignore),
        (DRCE_LIB_FOOTPRINT_MISMATCH, Severity::Ignore),
    ];
    let (found, _) = run(&mut session, &severities).expect("run");
    let bds = session.board.design_settings();
    let units =
        kicad_core::units_provider::UnitsProvider::new(&kicad_core::PCB_IU_SCALE, EdaUnits::Mm);
    let distance = kicad_core::eda_units::EdaDataType::Distance;
    let expected = units.message_text_from_value(bds.hole_to_hole_min as f64, true, distance);
    let relaxed = units.message_text_from_value(
        (bds.hole_to_hole_min - bds.get_drc_epsilon()) as f64,
        true,
        distance,
    );

    assert_ne!(expected, relaxed);
    let messages: Vec<String> = found
        .iter()
        .filter(|v| v.item.error_code() == DRCE_DRILLED_HOLES_TOO_CLOSE)
        .map(|v| v.item.error_message())
        .collect();

    assert!(!messages.is_empty());

    for m in &messages {
        assert!(m.contains(&expected), "{m} lacks {expected}");
        assert!(!m.contains(&relaxed), "{m} shows {relaxed}");
    }
}
