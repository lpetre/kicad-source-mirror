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

//! DRC-28: the track angle (`drc_test_provider_track_angle.cpp`), track segment length
//! (`drc_test_provider_track_segment_length.cpp`) and via stack
//! (`drc_test_provider_via_stack.cpp`) providers against the reference DRC reports:
//!
//! * the synthetic boards of `tests/oracle/gen_drc28_boards.py` (`tests/data/drc28/`, goldens
//!   in `tests/golden/drc28/`): every entry of the DRC-28 error types, in report order for the
//!   sequential via stack provider and as a set (plus the order within one track) for the two
//!   thread-pooled track providers;
//! * the shared corpus (`tests/golden/drc_corpus.txt`);
//! * the ported C++ tests: `qa/tests/pcbnew/drc/test_drc_via_stack.cpp` (`DRCViaStack`, the
//!   board-file cases on `qa/data/pcbnew/drc_via_stack/` and the in-code boards rebuilt as
//!   files by the generator, each with the C++ expected counts) and `issue18142` of
//!   `test_drc_regressions.cpp` (`DRCFalseNegativeRegressions`: `microvia_crosses_core`
//!   ignored, the one violation is `hole_to_hole`, so this package's providers report nothing).

mod drc_harness;

use std::collections::BTreeMap;
use std::path::PathBuf;

use drc_harness::{Entry, cli_report_entries, compare_corpus, has_compiled_conditions, load_board};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::track_angle::DrcTestProviderTrackAngle;
use kicad_drc::providers::track_segment_length::DrcTestProviderTrackSegmentLength;
use kicad_drc::providers::via_stack::DrcTestProviderViaStack;
use kicad_pcb::BoardCtx;

/// The error types of the three providers.
const TYPES: [&str; 7] = [
    "track_angle",
    "track_segment_length",
    "malformed_microvia_stack_span",
    "microvia_stack_not_filled",
    "microvia_stack_depth",
    "hole_aspect_ratio",
    "microvia_crosses_core",
];

/// The track angle provider with the board's connectivity (`BOARD::GetConnectivity()`), built
/// from the board the run sees.
struct TrackAngleWithConnectivity;

impl DrcTestProviderRun for TrackAngleWithConnectivity {
    fn name(&self) -> &str {
        "track angle"
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        let mut conn = ConnectivityData::new();
        conn.build(&BoardCtx::bare(ctx.board));
        let mut provider = DrcTestProviderTrackAngle::new(&conn);
        provider.run_tests(engine, run, ctx, units)
    }
}

/// The three providers in registry order (`PROVIDER_ORDER`: track angle, track segment
/// length, via stack).
fn providers() -> Vec<Box<dyn DrcTestProviderRun>> {
    vec![
        Box::new(TrackAngleWithConnectivity),
        Box::new(DrcTestProviderTrackSegmentLength::default()),
        Box::new(DrcTestProviderViaStack::default()),
    ]
}

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/drc28")
}

/// A golden of `gen_drc28_boards.py`.
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

/// The report entries of the DRC-28 types after a run of the three providers.
fn report(path: &std::path::Path) -> Result<Vec<Entry>, String> {
    let mut loaded = load_board(path)?;

    if let Some(first) = loaded.report.skipped.first() {
        return Err(format!("parser skipped {:?}", first.token));
    }

    if has_compiled_conditions(&loaded.engine) {
        return Err("user rule conditions need DRC-21".to_string());
    }

    let mut owned = providers();
    let mut refs: Vec<&mut dyn DrcTestProviderRun> = Vec::new();

    for p in owned.iter_mut() {
        refs.push(&mut **p);
    }

    Ok(cli_report_entries(&mut loaded, false, &mut refs)
        .into_iter()
        .filter(|e| TYPES.contains(&e.type_.as_str()))
        .collect())
}

/// The comparison: the via stack types in report order (the provider is sequential in C++);
/// the two track types as a multiset plus their order within one track (the first item: one
/// thread-pool task per track in C++, `drc.md` 4.12).
fn compare(theirs: &[Entry], mine: &[Entry]) -> Vec<String> {
    let pooled = |e: &Entry| e.type_ == "track_angle" || e.type_ == "track_segment_length";
    let mut diffs = Vec::new();

    let seq =
        |v: &[Entry]| -> Vec<String> { v.iter().filter(|e| !pooled(e)).map(Entry::key).collect() };

    if seq(theirs) != seq(mine) {
        diffs.push(format!(
            "via stack entries differ:\n  reference {:#?}\n  port {:#?}",
            seq(theirs),
            seq(mine)
        ));
    }

    let per_task = |v: &[Entry]| -> BTreeMap<String, Vec<String>> {
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for e in v.iter().filter(|e| pooled(e)) {
            let task = e.items.first().map(|i| i.0.clone()).unwrap_or_default();
            m.entry(task).or_default().push(e.key());
        }

        m
    };

    let (a, b) = (per_task(theirs), per_task(mine));

    for task in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
        if a.get(task) != b.get(task) {
            diffs.push(format!(
                "track {task}:\n  reference {:#?}\n  port {:#?}",
                a.get(task),
                b.get(task)
            ));
        }
    }

    diffs
}

/// Every synthetic board against its reference golden.
#[test]
fn synthetic_boards_match_reference() {
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "kicad_pcb")
                .then(|| p.file_stem().unwrap().to_string_lossy().to_string())
        })
        .collect();
    names.sort();

    let golden_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/drc28");
    let (mut compared, mut entries) = (0, 0);
    let mut pending = Vec::new();
    let mut diffs = Vec::new();

    for name in &names {
        let theirs = synthetic_golden(
            &std::fs::read_to_string(golden_dir.join(format!("{name}.txt"))).unwrap(),
        );

        match report(&data_dir().join(format!("{name}.kicad_pcb"))) {
            Ok(mine) => {
                compared += 1;
                entries += theirs.len();

                for d in compare(&theirs, &mine) {
                    diffs.push(format!("{name}: {d}"));
                }
            }
            Err(e) => pending.push(format!("{name}: {e}")),
        }
    }

    println!(
        "DRC-28 synthetic boards: {compared} of {} compared ({entries} reference entries), \
         {} differences",
        names.len(),
        diffs.len()
    );

    for p in &pending {
        println!("  pending: {p}");
    }

    for d in &diffs {
        println!("  {d}");
    }

    assert!(diffs.is_empty(), "{} differences", diffs.len());
}

/// The number of entries of `type_` in a reference golden / a port run of the case.
fn counts(name: &str, type_: &str) -> (usize, Result<usize, String>) {
    let golden_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/drc28");
    let theirs =
        synthetic_golden(&std::fs::read_to_string(golden_dir.join(format!("{name}.txt"))).unwrap());
    let reference = theirs.iter().filter(|e| e.type_ == type_).count();
    let mine = report(&data_dir().join(format!("{name}.kicad_pcb")))
        .map(|v| v.iter().filter(|e| e.type_ == type_).count());

    (reference, mine)
}

/// Port of `test_drc_via_stack.cpp` (`DRCViaStack`): the C++ expected count of each case
/// (the board-file cases on `qa/data/pcbnew/drc_via_stack/`, the in-code boards rebuilt by
/// `gen_drc28_boards.py`). Both the reference golden and the port must give it; a case the
/// port cannot run yet (rule conditions: DRC-21; pads skipped by the parser) checks the
/// reference only and says so.
#[test]
fn drc_via_stack() {
    let qa = kicad_qa::pcbnew_data_dir().join("drc_via_stack");
    let mut pending = Vec::new();

    // Board-file cases without a rule: the run of the providers only, the C++ counts.
    let files: &[(&str, &[(&str, usize)])] = &[
        (
            "StackNotTilingItsSpanReported",
            &[("malformed_microvia_stack_span", 1)],
        ),
        (
            "UnfilledInnerHopReported",
            &[
                ("malformed_microvia_stack_span", 0),
                ("microvia_stack_not_filled", 1),
            ],
        ),
        (
            "LooseStackedMicroviasReportNotFilled",
            &[("microvia_stack_not_filled", 1)],
        ),
        (
            "GeneratorStackIsNotReportedTwice",
            &[("microvia_stack_not_filled", 2)],
        ),
        (
            "OverlappingButOffsetMicroviasAreOneStructure",
            &[("microvia_stack_not_filled", 1)],
        ),
        (
            "ClearOfTheViaBelowIsAStaggerNotAStack",
            &[
                ("microvia_stack_not_filled", 0),
                ("microvia_stack_depth", 0),
            ],
        ),
        (
            "EveryHopOfAStackMustBeFilled",
            &[("microvia_stack_not_filled", 2)],
        ),
        (
            "ALoneMicroviaMayBeUnfilled",
            &[("microvia_stack_not_filled", 0)],
        ),
        (
            "ZeroSpanMicroviasDoNotMultiplyViolations",
            &[("microvia_stack_not_filled", 0)],
        ),
        (
            "FillFromBoardHonoursTheBoardDefault",
            &[("microvia_stack_not_filled", 0)],
        ),
        (
            "AViaCarriedTwiceIsReportedOnce",
            &[("microvia_stack_not_filled", 1)],
        ),
        (
            "TouchingHolesFormAColumn",
            &[("microvia_stack_not_filled", 1)],
        ),
        (
            "ASecondHopSharingALandingViaIsStillChecked",
            &[("microvia_stack_not_filled", 2)],
        ),
    ];

    for (board, expected) in files {
        let mine = report(&qa.join(format!("{board}.kicad_pcb")));

        for &(type_, n) in *expected {
            match &mine {
                Ok(v) => assert_eq!(
                    v.iter().filter(|e| e.type_ == type_).count(),
                    n,
                    "{board}: {type_}"
                ),
                Err(e) => pending.push(format!("{board}: {e}")),
            }
        }
    }

    // Cases with a rule or an in-code board: (generated case, C++ test, [(type, count)]).
    let cases: &[(&str, &str, &[(&str, usize)])] = &[
        (
            "vs_good_stack",
            "GoodStackNoViolations",
            &[
                ("malformed_microvia_stack_span", 0),
                ("microvia_stack_not_filled", 0),
                ("microvia_stack_depth", 0),
                ("hole_aspect_ratio", 0),
                ("microvia_crosses_core", 0),
            ],
        ),
        (
            "vs_duplicate_hop",
            "DuplicateHopReported",
            &[("malformed_microvia_stack_span", 1)],
        ),
        (
            "qa_StackTooDeepReported",
            "StackTooDeepReported",
            &[
                ("malformed_microvia_stack_span", 0),
                ("microvia_stack_depth", 1),
            ],
        ),
        (
            "qa_LooseStackedMicroviasReportDepth",
            "LooseStackedMicroviasReportDepth",
            &[("microvia_stack_depth", 1)],
        ),
        (
            "qa_CoaxialButSeparatedMicroviasAreNotOneStack",
            "CoaxialButSeparatedMicroviasAreNotOneStack",
            &[
                ("microvia_stack_depth", 0),
                ("microvia_stack_not_filled", 0),
            ],
        ),
        (
            "vs_too_deep",
            "MicroviaTooDeepForItsDiameterIsReported",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_no_limit",
            "AspectRatioIsSilentWithoutALimit (no rule)",
            &[("hole_aspect_ratio", 0)],
        ),
        (
            "vs_too_deep",
            "AspectRatioIsSilentWithoutALimit (rule)",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_others_ignored",
            "AspectRatioSurvivesTheOtherChecksBeingIgnored",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_default_stackup",
            "AspectRatioFallsBackToTheDefaultStackup",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "qa_MicroviaStackDepthRuleIsHonoured_2",
            "MicroviaStackDepthRuleIsHonoured (max 2)",
            &[("microvia_stack_depth", 1)],
        ),
        (
            "qa_MicroviaStackDepthRuleIsHonoured_4",
            "MicroviaStackDepthRuleIsHonoured (max 4)",
            &[("microvia_stack_depth", 0)],
        ),
        (
            "qa_MicroviaStackDepthRuleIsHonoured_cond",
            "MicroviaStackDepthRuleIsHonoured (A.isStackedVia())",
            &[("microvia_stack_depth", 1)],
        ),
        (
            "vs_ignored_depth",
            "IgnoredDepthRuleIsSilent",
            &[("microvia_stack_depth", 0)],
        ),
        (
            "vs_ignored_aspect",
            "IgnoredAspectRuleIsSilent",
            &[("hole_aspect_ratio", 0)],
        ),
        (
            "vs_sublayers",
            "AspectRatioCountsEveryDielectricSublayer",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_back_foil",
            "AspectRatioUsesTheFoilTheLaserEnters",
            &[("hole_aspect_ratio", 1)],
        ),
        // The generated board adds a microvia through two dielectrics (a second core
        // crossing) and a through via to the C++ pair: 1 + 1.
        (
            "vs_core",
            "MicroviaThroughCoreIsReported",
            &[("microvia_crosses_core", 2)],
        ),
        (
            "vs_ignored_aspect_core",
            "AnIgnoredAspectRatioDoesNotStopTheCoreCheck",
            &[("hole_aspect_ratio", 0), ("microvia_crosses_core", 2)],
        ),
        (
            "vs_through",
            "ThroughViaTooDeepForItsDrillIsReported",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_blind",
            "BlindViaMeasuresItsSpanNotTheBoard",
            &[("hole_aspect_ratio", 0)],
        ),
        (
            "vs_pads",
            "PlatedPadHoleUsesItsNarrowDimension",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_via_type_condition",
            "AViaTypeConditionScopesTheLimit",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_buried",
            "BuriedViaCountsTheFoilItDrillsOutOf",
            &[("hole_aspect_ratio", 1)],
        ),
        (
            "vs_npth",
            "UnplatedHoleIsNotChecked",
            &[("hole_aspect_ratio", 0)],
        ),
    ];

    for (name, test, expected) in cases {
        for &(type_, n) in *expected {
            let (reference, mine) = counts(name, type_);
            assert_eq!(reference, n, "{test}: reference {type_}");

            match mine {
                Ok(m) => assert_eq!(m, n, "{test}: {type_}"),
                Err(e) => pending.push(format!("{test}: {e}")),
            }
        }
    }

    pending.dedup();

    for p in &pending {
        println!("pending: {p}");
    }
}

/// `test_drc_regressions.cpp` `DRCFalseNegativeRegressions`, `issue18142` (1 violation with
/// `microvia_crosses_core` ignored): the one violation is `hole_to_hole` (another provider), so with the
/// crossing-core check ignored these providers report nothing; the reference reports one
/// `microvia_crosses_core` warning when it is not ignored (corpus), which the port matches.
#[test]
fn issue18142() {
    let path = kicad_qa::pcbnew_data_dir().join("issue18142.kicad_pcb");

    match report(&path) {
        Ok(v) => {
            let core = v
                .iter()
                .filter(|e| e.type_ == "microvia_crosses_core")
                .count();
            let others = v.len() - core;
            assert_eq!(core, 1, "microvia_crosses_core with the default severity");
            assert_eq!(others, 0, "{v:#?}");
        }
        Err(e) => println!("issue18142 pending: {e}"),
    }
}

/// Every corpus board the port loads completely, the DRC-28 types only, against the reference
/// reports. `DRC28_FILTER=<path substring>` limits the boards.
#[test]
fn track_angle_via_stack_match_reference_corpus() {
    let filter = std::env::var("DRC28_FILTER").unwrap_or_default();
    let score = compare_corpus(&TYPES, &filter, &mut providers);
    score.print("DRC-28 track angle / segment length / via stack");
    score.assert_no_mismatch();
}
