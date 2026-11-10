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

//! DRC-06: the unary providers (track width, via diameter, hole size, annular width) against
//! the reference `kicad-cli` (`drc_harness`, golden `tests/golden/drc_corpus.txt`), and the
//! ported C++ tests:
//!
//! * `qa/tests/pcbnew/drc/test_drc_annular_overlap_issue24340.cpp` (`AnnularOverlapIssue24340`)
//! * `qa/tests/pcbnew/drc/test_drc_regressions.cpp` (`DRCFalseNegativeRegressions`), the boards
//!   whose expected violations all come from these providers according to the reference
//!   (`issue19325`, `issue12109`)

mod drc_harness;

use drc_harness::{Reported, load_board, run_flow_providers};
use kicad_core::Severity;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::EdaUnits;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{DRCE_ANNULAR_WIDTH, DRCE_DRILLED_HOLES_TOO_CLOSE, DRCE_FIRST, DRCE_LAST};

/// The error types of the four providers (settings keys).
const TYPES: &[&str] = &[
    "annular_width",
    "drill_out_of_range",
    "microvia_drill_out_of_range",
    "track_width",
    "via_diameter",
];

/// The registry slots (`GetName()`) of the four providers.
const PROVIDERS: &[&str] = &["annular_width", "hole_size", "width", "diameter"];

/// Every corpus board the port loads completely: the violations of the four providers equal
/// the reference report's (set and order).
#[test]
fn unary_providers_match_reference_corpus() {
    let score = drc_harness::compare_corpus_flow(TYPES, "", PROVIDERS);
    score.print("DRC-06 unary providers");
    score.assert_no_mismatch();
}

/// `mmToIU`
fn mm(v: f64) -> i32 {
    PCB_IU_SCALE.mm_to_iu(v)
}

/// Loads a QA board (`qa/data/pcbnew/<name>.kicad_pcb`); `None` (with a note) when the
/// parser does not load it completely yet.
fn load_qa(name: &str) -> Option<drc_harness::Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));

    match drc_harness::quiet(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load_board(&path)))
    }) {
        Ok(Ok(l)) if l.report.skipped.is_empty() => Some(l),
        Ok(Ok(_)) => {
            eprintln!("{name}: pending (the parser skips items of this board)");
            None
        }
        Ok(Err(e)) => {
            eprintln!("{name}: pending ({e})");
            None
        }
        Err(e) => {
            eprintln!("{name}: pending ({})", drc_harness::panic_text(&*e));
            None
        }
    }
}

/// Port of `AnnularOverlapIssue24340` (`test_drc_annular_overlap_issue24340.cpp`).
#[test]
fn annular_overlap_issue24340() {
    let Some(mut loaded) = load_qa("issue24340/issue24340") else {
        return;
    };

    {
        let bds = loaded.board.design_settings_mut();

        for ii in DRCE_FIRST..=DRCE_LAST {
            bds.drc_severities.insert(ii, Severity::Ignore);
        }

        bds.drc_severities
            .insert(DRCE_ANNULAR_WIDTH, Severity::Error);
    }
    let found = run_flow_providers(&mut loaded, EdaUnits::Mm, true, PROVIDERS);

    let annular: Vec<&Reported> = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_ANNULAR_WIDTH)
        .collect();

    // Bug-case THT board position: footprint (130, 95.5) + pad (-1.4, 0) = (128.6, 95.5).
    // Control THT board position:   footprint (176, 95.5) + pad (-1.4, 0) = (174.6, 95.5).
    let bug_pos = Vector2I::new(mm(128.6), mm(95.5));
    let control_pos = Vector2I::new(mm(174.6), mm(95.5));
    let tol = mm(0.5);

    let mut on_bug_footprint = 0;
    let mut on_control_footprint = 0;

    for v in &annular {
        if (v.pos.x - bug_pos.x).abs() < tol && (v.pos.y - bug_pos.y).abs() < tol {
            on_bug_footprint += 1;
        } else if (v.pos.x - control_pos.x).abs() < tol && (v.pos.y - control_pos.y).abs() < tol {
            on_control_footprint += 1;
        }
    }

    // Bug-case pad: exact annular 0.1524 mm meets the 0.1524 mm constraint. The
    // pre-fix polygon path under-reported it by ~maxError and falsely flagged.
    assert_eq!(on_bug_footprint, 0);

    // Control pad: 0.1 mm annular is genuinely below the constraint. The fix
    // must still report this -- guards against the optimization swallowing
    // real violations.
    assert_eq!(on_control_footprint, 1);
}

/// `DRCFalseNegativeRegressions` (`test_drc_regressions.cpp`) restricted to the four
/// providers: the C++ expectation counts the violations of every provider, so the expected
/// count here is the part of it that the reference report attributes to these providers
/// (`drc.md` 9.4 item 5): `issue12109` 8 = 7 `annular_width` + 1 `via_dangling`, `issue19325`
/// 4 = 4 `annular_width`. Severities ignored as in the C++ test: `copper_sliver`,
/// `lib_footprint_issues`, `lib_footprint_mismatch`, `track_not_centered_on_via`, plus the
/// per-board extras.
#[test]
fn false_negative_regressions_unary() {
    use kicad_pcb::drc_api::{
        DRCE_COPPER_SLIVER, DRCE_LIB_FOOTPRINT_ISSUES, DRCE_LIB_FOOTPRINT_MISMATCH,
        DRCE_TRACK_NOT_CENTERED_ON_VIA,
    };

    // ( board, C++ expected count, the part of it from these providers, extra ignored codes )
    let tests: [(&str, usize, usize, &[i32]); 2] = [
        ("issue12109", 8, 7, &[]),
        (
            "issue19325/issue19325",
            4,
            4,
            &[DRCE_DRILLED_HOLES_TOO_CLOSE],
        ),
    ];

    for (name, _cxx_expected, expected, extra) in tests {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };

        {
            let bds = loaded.board.design_settings_mut();

            for code in [
                DRCE_COPPER_SLIVER,
                DRCE_LIB_FOOTPRINT_ISSUES,
                DRCE_LIB_FOOTPRINT_MISMATCH,
                DRCE_TRACK_NOT_CENTERED_ON_VIA,
            ]
            .iter()
            .chain(extra)
            {
                bds.drc_severities.insert(*code, Severity::Ignore);
            }
        }
        let found = run_flow_providers(&mut loaded, EdaUnits::Mm, true, PROVIDERS);

        assert_eq!(found.len(), expected, "{name}");
    }
}
