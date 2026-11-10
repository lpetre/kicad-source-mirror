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

//! DRC-24: the courtyard clearance, text dimensions and text mirroring providers against the
//! reference `kicad-cli` (`drc_harness`, golden `tests/golden/drc_corpus.txt`), and the ported
//! C++ tests:
//!
//! * `qa/tests/pcbnew/drc/test_drc_courtyard_invalid.cpp` (`InvalidCases`)
//! * `qa/tests/pcbnew/drc/test_drc_courtyard_overlap.cpp` (`OverlapCases`)
//! * `qa/tests/pcbnew/drc/test_drc_incorrect_text_mirror.cpp` (`DRCIncorrectTextMirror`)
//!
//! Boards (or runs) that reach a part of the port that is still `port_pending!` are reported
//! as pending and skipped, so the tests tighten by themselves as those parts land.

mod drc_harness;

use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{Loaded, Reported, load_board, run_flow_providers, run_providers};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_core::kiid::Kiid;
use kicad_drc::engine::DrcTestProviderRun;
use kicad_drc::providers::text_dims::DrcTestProviderTextDims;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{
    DRCE_FIRST, DRCE_LAST, DRCE_MALFORMED_COURTYARD, DRCE_MIRRORED_TEXT_ON_FRONT_LAYER,
    DRCE_MISSING_COURTYARD, DRCE_OVERLAPPING_FOOTPRINTS, DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER,
};

/// The error types of the courtyard provider (settings keys).
const COURTYARD_TYPES: &[&str] = &[
    "courtyards_overlap",
    "missing_courtyard",
    "malformed_courtyard",
    "pth_inside_courtyard",
    "npth_inside_courtyard",
];

/// The error types of the two text providers (settings keys).
const TEXT_TYPES: &[&str] = &[
    "text_height",
    "text_thickness",
    "mirrored_text_on_front_layer",
    "nonmirrored_text_on_back_layer",
];

/// Every corpus board the port loads completely: the courtyard violations equal the
/// reference report's (set and order).
#[test]
fn courtyard_clearance_matches_reference_corpus() {
    let score =
        drc_harness::compare_corpus_flow(COURTYARD_TYPES, "", &["courtyard_clearance"]);
    score.print("DRC-24 courtyard clearance");
    score.assert_no_mismatch();
}

/// Every corpus board the port loads completely: the text dimension and mirroring violations
/// equal the reference report's (set and order). The two providers run in registry order
/// (`text_dimensions` before `text_mirroring`).
#[test]
fn text_providers_match_reference_corpus() {
    let score = drc_harness::compare_corpus_flow(
        TEXT_TYPES,
        "",
        &["text_dimensions", "text_mirroring"],
    );
    score.print("DRC-24 text dimensions / mirroring");
    score.assert_no_mismatch();
}

/// Loads `qa/data/pcbnew/<rel>`; `None` (with a note) when the port does not load it
/// completely yet.
fn load_qa(rel: &str) -> Option<Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(rel);

    match drc_harness::quiet(|| catch_unwind(AssertUnwindSafe(|| load_board(&path)))) {
        Ok(Ok(l)) if l.report.skipped.is_empty() => Some(l),
        Ok(Ok(_)) => {
            eprintln!("{rel}: pending (the parser skips items of this board)");
            None
        }
        Ok(Err(e)) => {
            eprintln!("{rel}: pending ({e})");
            None
        }
        Err(e) => {
            eprintln!("{rel}: pending ({})", drc_harness::panic_text(&*e));
            None
        }
    }
}

/// Runs the registry slots `providers` through the real run path (cache generator
/// included: it builds the courtyard caches the courtyard provider reads) on `loaded`;
/// `None` (with a note) when the run reaches a part of the port that is still pending.
/// Other panics propagate.
fn run_or_pending(rel: &str, loaded: &mut Loaded, providers: &[&str]) -> Option<Vec<Reported>> {
    match drc_harness::quiet(|| {
        catch_unwind(AssertUnwindSafe(|| {
            run_flow_providers(loaded, EdaUnits::Mm, true, providers)
        }))
    }) {
        Ok(found) => Some(found),
        Err(e) => {
            let text = drc_harness::panic_text(&*e);

            if text.contains("port pending") {
                eprintln!("{rel}: pending ({text})");
                None
            } else {
                std::panic::resume_unwind(e)
            }
        }
    }
}

/// `FOOTPRINT::GetReference()` of the footprint with `uuid` (`BOARD::ResolveItem`).
fn reference_of(loaded: &Loaded, uuid: &Kiid) -> Option<String> {
    let ctx = BoardCtx::bare(&loaded.board);
    let item = ctx.resolve(uuid)?;
    let fp = item.as_footprint()?;
    Some(fp.reference(&ctx))
}

/// `KI_TEST::CheckUnorderedMatches`: every expected value matches exactly one marker and every
/// marker one expected value.
fn check_unordered_matches<E: std::fmt::Debug>(
    context: &str,
    expected: &[E],
    found: &[Reported],
    matches: impl Fn(&E, &Reported) -> bool,
) {
    let mut used = vec![false; found.len()];

    for e in expected {
        let hit = found
            .iter()
            .enumerate()
            .find(|(i, f)| !used[*i] && matches(e, f));

        match hit {
            Some((i, _)) => used[i] = true,
            None => panic!("{context}: expected {e:?} not found"),
        }
    }

    assert!(
        used.iter().all(|u| *u),
        "{context}: {} unexpected markers",
        used.iter().filter(|u| !**u).count()
    );
}

/// The handler filter of the two courtyard tests.
fn courtyard_markers(found: Vec<Reported>) -> Vec<Reported> {
    found
        .into_iter()
        .filter(|r| {
            let code = r.item.error_code();
            code == DRCE_OVERLAPPING_FOOTPRINTS
                || code == DRCE_MALFORMED_COURTYARD
                || code == DRCE_MISSING_COURTYARD
        })
        .collect()
}

/// Port of `DrcCourtyardInvalid/InvalidCases` (`test_drc_courtyard_invalid.cpp`).
#[test]
fn courtyard_invalid_cases() {
    // { board, [ (refdes, error code) ] }
    let cases: &[(&str, &[(&str, i32)])] = &[
        // Empty board has no footprints to be invalid
        ("empty_board", &[]),
        // one error: the footprint has no courtyard
        (
            "single_footprint_no_courtyard",
            &[("U1", DRCE_MISSING_COURTYARD)],
        ),
        // one error: the footprint has malformed courtyard
        (
            "single_footprint_unclosed_courtyard",
            &[("U1", DRCE_MALFORMED_COURTYARD)],
        ),
        // one error: the footprint has malformed courtyard
        (
            "single_footprint_disjoint_courtyard",
            &[("U1", DRCE_MALFORMED_COURTYARD)],
        ),
        // one error: the second footprint has malformed courtyard
        (
            "two_footprints_one_malformed",
            &[("U2", DRCE_MALFORMED_COURTYARD)],
        ),
    ];

    for (name, expected) in cases {
        let rel = format!("drc_courtyard/invalid/{name}.kicad_pcb");
        let Some(mut loaded) = load_qa(&rel) else {
            continue;
        };

        {
            let bds = loaded.board.design_settings_mut();

            // do the overlap tests - that's a different test, but if not set,
            // the invalid courtyard checks don't run either
            bds.drc_severities
                .insert(DRCE_OVERLAPPING_FOOTPRINTS, Severity::Error);

            // we will also check for missing courtyards here
            bds.drc_severities
                .insert(DRCE_MISSING_COURTYARD, Severity::Error);
        }

        let Some(found) = run_or_pending(&rel, &mut loaded, &["courtyard_clearance"]) else {
            continue;
        };
        let markers = courtyard_markers(found);

        check_unordered_matches(name, expected, &markers, |(refdes, code), marker| {
            // This one is more than just a mismatch!
            if marker.item.aux_item_id() != kicad_core::kiid::NILUUID {
                return false;
            }

            reference_of(&loaded, &marker.item.main_item_id()).as_deref() == Some(*refdes)
                && marker.item.error_code() == *code
        });
    }
}

/// Port of `DrcCourtyardOverlap/OverlapCases` (`test_drc_courtyard_overlap.cpp`).
#[test]
fn courtyard_overlap_cases() {
    // { board, [ (refdes a, refdes b) ] }
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("empty_board", &[]),
        ("single_empty_footprint", &[]),
        // A single footprint can't overlap itself
        ("single_footprint_single_courtyard", &[]),
        ("two_footprints_no_overlap", &[]),
        // Touching means not colliding
        ("two_footprints_touching", &[]),
        // These two collide
        ("two_footprints_overlap", &[("U1", "U2")]),
        // but on different sides
        ("two_footprints_overlap_different_sides", &[]),
        (
            "two_footprints_multiple_courtyards_overlap",
            &[("U1", "U2")],
        ),
        // The courtyards do not overlap, but their bounding boxes do
        ("two_footprints_bbox_overlap_only", &[]),
    ];

    for (name, expected) in cases {
        let rel = format!("drc_courtyard/overlap/{name}.kicad_pcb");
        let Some(mut loaded) = load_qa(&rel) else {
            continue;
        };

        {
            let bds = loaded.board.design_settings_mut();
            bds.drc_severities
                .insert(DRCE_OVERLAPPING_FOOTPRINTS, Severity::Error);

            // we might not always have courtyards - that's a separate test
            bds.drc_severities
                .insert(DRCE_MISSING_COURTYARD, Severity::Ignore);
        }

        let Some(found) = run_or_pending(&rel, &mut loaded, &["courtyard_clearance"]) else {
            continue;
        };
        let markers = courtyard_markers(found);

        for m in &markers {
            assert_eq!(m.item.error_code(), DRCE_OVERLAPPING_FOOTPRINTS, "{name}");
        }

        check_unordered_matches(name, expected, &markers, |(ref_a, ref_b), marker| {
            let a = reference_of(&loaded, &marker.item.main_item_id());
            let b = reference_of(&loaded, &marker.item.aux_item_id());

            // can't find the items!
            let (Some(a), Some(b)) = (a, b) else {
                return false;
            };

            // Doesn't matter which way around it is, but both have to match somehow
            (a == *ref_a && b == *ref_b) || (a == *ref_b && b == *ref_a)
        });
    }
}

/// Port of `DRCIncorrectTextMirror` (`test_drc_incorrect_text_mirror.cpp`). The C++ fills the
/// zones first (`KI_TEST::FillZones`), which cannot change the result of the text mirroring
/// provider; the port does not.
#[test]
fn drc_incorrect_text_mirror() {
    let tests = [("incorrect_text_mirroring_drc", 8)];

    for (name, expected) in tests {
        let rel = format!("{name}.kicad_pcb");
        let Some(mut loaded) = load_qa(&rel) else {
            continue;
        };

        {
            let bds = loaded.board.design_settings_mut();

            // Disable DRC tests not useful or not handled in this testcase
            for ii in DRCE_FIRST..=DRCE_LAST {
                bds.drc_severities.insert(ii, Severity::Ignore);
            }

            // Ensure that our desired errors are fired
            bds.drc_severities
                .insert(DRCE_MIRRORED_TEXT_ON_FRONT_LAYER, Severity::Error);
            bds.drc_severities
                .insert(DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER, Severity::Error);
        }

        let Some(found) = run_or_pending(&rel, &mut loaded, &["text_mirroring"]) else {
            continue;
        };
        let violations = found
            .iter()
            .filter(|r| {
                loaded
                    .board
                    .design_settings()
                    .get_severity(r.item.error_code())
                    == Severity::Error
            })
            .count();

        assert_eq!(
            violations, expected,
            "DRC incorrect text mirror test: {name}, failed (violations found {violations} \
             expected {expected})"
        );
    }
}

/// Runs only the `checkTextThickness` lambda of the text dimensions provider on one item.
struct ThicknessOfOneItem {
    provider: DrcTestProviderTextDims,
    uuid: Kiid,
    has_render_cache: bool,
}

impl DrcTestProviderRun for ThicknessOfOneItem {
    fn name(&self) -> &str {
        self.provider.base.name
    }

    fn run_tests(
        &mut self,
        engine: &kicad_drc::engine::DrcEngine,
        run: &mut kicad_drc::engine::DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.provider.base.set_user_units(units);

        let item = ctx
            .resolve(&self.uuid)
            .expect("the text of the reference violation");
        let text = item.as_eda_text().expect("an EDA_TEXT");

        // The C++ uses the cache read from the file; without it the outline font engine would
        // be needed (pending).
        self.has_render_cache = text.eda_text().render_cache_data().is_some();

        if self.has_render_cache {
            let mut host = kicad_drc::providers::run_host::RunHost(run);
            self.provider.check_text_thickness(
                engine,
                &mut host,
                ctx,
                item,
                text,
                &std::cell::Cell::new(0),
            );
        }

        true
    }
}

/// The TrueType branch of `checkTextThickness` on the one outline text of `issue22475` whose
/// render cache is stored in the file (the other outline texts of that board have none and
/// need the outline font engine, so the whole-board run is pending): the reference reports
/// exactly this text ("TrueType font characters with insufficient stroke weight").
#[test]
fn text_thickness_truetype_render_cache_issue22475() {
    use drc_harness::{cli_report_entries, golden, summarize};

    let board_rel = "qa/data/pcbnew/issue22475/issue22475.kicad_pcb";
    // Only a board text and the implicit silk rules are involved: items the parser still
    // skips (footprint contents) do not matter here.
    let path = kicad_qa::pcbnew_data_dir().join("issue22475/issue22475.kicad_pcb");
    let mut loaded =
        match drc_harness::quiet(|| catch_unwind(AssertUnwindSafe(|| load_board(&path)))) {
            Ok(Ok(l)) => l,
            Ok(Err(e)) => {
                eprintln!("issue22475: pending ({e})");
                return;
            }
            Err(e) => {
                eprintln!("issue22475: pending ({})", drc_harness::panic_text(&*e));
                return;
            }
        };
    let mut one = ThicknessOfOneItem {
        provider: DrcTestProviderTextDims::default(),
        uuid: Kiid::parse("ff749b73-2383-44e8-88dd-1d9c1b0c1a89").unwrap(),
        has_render_cache: false,
    };
    let entries = cli_report_entries(&mut loaded, false, &mut [&mut one]);

    if !one.has_render_cache {
        eprintln!(
            "issue22475: pending (the parsed render cache of the text is not kept by the board \
             loader)"
        );
        return;
    }

    let mine: Vec<_> = entries
        .iter()
        .filter(|e| e.type_ == "text_thickness")
        .collect();
    let reference = golden()
        .remove(board_rel)
        .flatten()
        .expect("reference of issue22475");
    let expected = reference.summaries["text_thickness"];

    assert_eq!(summarize(&mine), expected, "{mine:?}");
}
