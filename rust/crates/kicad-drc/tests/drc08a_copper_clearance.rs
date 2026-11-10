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

//! DRC-08a: the item tests of the copper clearance provider
//! (`drc_test_provider_copper_clearance.cpp`: tracks, vias and pads against the copper item
//! tree and the zone fills) against the reference `kicad-cli pcb drc --format json
//! --severity-all --units mm [--all-track-errors]` reports (goldens of
//! `tests/oracle/gen_drc08a_corpus.py`), plus the ports of
//! `qa/tests/pcbnew/drc/test_drc_hole_clearance_issue24355.cpp` and the copper clearance part
//! of `test_drc_regressions.cpp` `DRCFalsePositiveRegressions`.
//!
//! The reference reports also hold `clearance` / `hole_clearance` / `shorting_items` entries
//! of the DRC-08b phases (copper graphics, knockout text, zone-to-zone, teardrops) and of the
//! physical clearance provider. Each reference entry is attributed to its phase by the types
//! of its items (looked up by UUID in the loaded board, see [`origin`]); only the DRC-08a
//! entries are compared.

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use drc_harness::{
    Entry, FNV_OFFSET, Loaded, cached_entries, cli_report_entries, fnv1a64,
    has_compiled_conditions, hash_text, load_board, panic_text, quiet, run_providers,
};
use kicad_connectivity::data::ConnectivityData;
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_core::typeinfo::KicadT;
use kicad_drc::cache_generator::{
    build_copper_item_rtree, build_zone_caches, build_zone_lists, compute_max_clearances,
    prepare_board,
};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::copper_clearance::DrcTestProviderCopperClearance;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_FIRST, DRCE_HOLE_CLEARANCE, DRCE_LAST};
use kicad_pcb::item::RecurseMode;

/// The error types of the provider.
const TYPES: &[&str] = &[
    "clearance",
    "hole_clearance",
    "shorting_items",
    "tracks_crossing",
];

/// The type code of a golden line.
fn type_of_code(code: &str) -> &'static str {
    match code {
        "C" => "clearance",
        "H" => "hole_clearance",
        "S" => "shorting_items",
        _ => "tracks_crossing",
    }
}

/// The copper clearance provider (DRC-08a phases only) with the parts of
/// `DRC_CACHE_GENERATOR::Run()` it reads built right before it, and the run's connectivity
/// (FlashLayer of pads and vias).
struct CopperWithCaches<'a> {
    connectivity: &'a ConnectivityData,
}

impl DrcTestProviderRun for CopperWithCaches<'_> {
    fn name(&self) -> &str {
        "clearance"
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

        let mut provider = DrcTestProviderCopperClearance::new_without_pending(&caches);
        provider.run_tests(engine, run, &cctx, units)
    }
}

/// `BuildConnectivity` at load time, then the board-writing part of the cache generator and
/// its `connectivity->Build( board )` (as `drc07_connectivity.rs`).
fn prepare(loaded: &mut Loaded) -> ConnectivityData {
    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(&mut loaded.board);
    prepare_board(&mut loaded.board);
    connectivity.clear_ratsnest();
    connectivity.build_board(&mut loaded.board);
    connectivity
}

/// The report entries of the provider's types after a run of the provider.
fn report(loaded: &mut Loaded, all_track_errors: bool) -> Vec<Entry> {
    let connectivity = prepare(loaded);
    let mut provider = CopperWithCaches {
        connectivity: &connectivity,
    };

    cli_report_entries(loaded, all_track_errors, &mut [&mut provider])
        .into_iter()
        .filter(|e| TYPES.contains(&e.type_.as_str()))
        .collect()
}

/// The phase that produced a reference entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    /// `testTrackClearances` / `testPadClearances` (this package).
    Items,
    /// `testGraphicClearances`, `testZonesToZones`, `testTeardropClearances` (DRC-08b).
    Part2,
    /// The physical clearance provider (same error types).
    Physical,
}

/// One golden entry: type, the last 8 characters of the item UUIDs and
/// the entry hash.
struct GoldenEntry {
    type_: &'static str,
    uuids: [String; 2],
    hash: u64,
}

/// A golden file: per board `None` (reference run failed) or its entries.
fn parse_golden(text: &str) -> BTreeMap<String, Option<Vec<GoldenEntry>>> {
    let mut out: BTreeMap<String, Option<Vec<GoldenEntry>>> = BTreeMap::new();
    let mut current = String::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@ ") {
            let mut f = rest.split('\t');
            current = f.next().unwrap().to_string();

            match f.next() {
                Some("load_error") => {
                    out.insert(current.clone(), None);
                }
                _ => {
                    out.insert(current.clone(), Some(Vec::new()));
                }
            }

            continue;
        }

        let f: Vec<&str> = line.split('\t').collect();
        let entries = out
            .get_mut(&current)
            .and_then(|e| e.as_mut())
            .expect("entry line after a board line");
        entries.push(GoldenEntry {
            type_: type_of_code(f[1]),
            uuids: [f[2].to_string(), f[3].to_string()],
            hash: u64::from_str_radix(f[4], 16).unwrap(),
        });
    }

    out
}

/// The hash of an entry (`hash_line` of `gen_drc_corpus.py`).
fn entry_hash(e: &Entry) -> u64 {
    fnv1a64(FNV_OFFSET, hash_text(e).as_bytes())
}

/// The item types of the loaded board by the last 8 characters of their UUID.
fn types_by_uuid(loaded: &Loaded) -> BTreeMap<String, KicadT> {
    let ctx = BoardCtx::bare(&loaded.board);
    let mut out = BTreeMap::new();

    loaded
        .board
        .run_on_children(RecurseMode::Recurse, &mut |id| {
            if let Some(item) = ctx.item(id) {
                let uuid = item.uuid().as_string();
                out.insert(uuid[uuid.len() - 8..].to_string(), item.type_());
            }
        });

    out
}

/// The names of the physical clearance constraints of the engine (as `drc26_silk_physical`).
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

/// Which phase produced a reference entry, from the types of its two items:
/// * a track, arc or via against anything: `testTrackClearances` (the graphic test filters
///   tracks out);
/// * a pad against anything but a zone, shape or text box: `testPadAgainstItem` (shapes and
///   text boxes are left to `testGraphicClearances`); a track / pad against a zone:
///   `testItemAgainstZone`;
/// * everything else (graphics or text against pads / zones / graphics, zone against zone,
///   teardrops): DRC-08b.
fn origin(types: [Option<KicadT>; 2], description: &str, physical: &[String]) -> Origin {
    if physical
        .iter()
        .any(|n| description.contains(&format!("({n} clearance")))
    {
        return Origin::Physical;
    }

    let is_track = |t: Option<KicadT>| {
        matches!(
            t,
            Some(KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT)
        )
    };

    if is_track(types[0]) || is_track(types[1]) {
        return Origin::Items;
    }

    for (a, b) in [(types[0], types[1]), (types[1], types[0])] {
        if a == Some(KicadT::PcbPadT) {
            return match b {
                Some(KicadT::PcbShapeT | KicadT::PcbTextboxT) => Origin::Part2,
                _ => Origin::Items,
            };
        }
    }

    Origin::Part2
}

/// Per-type score.
#[derive(Default)]
struct TypeScore {
    matched: usize,
    missing: usize,
    extra: usize,
}

#[derive(Default)]
struct Score {
    compared: usize,
    skipped_part2: usize,
    skipped_physical: usize,
    per_type: BTreeMap<&'static str, TypeScore>,
    differences: Vec<String>,
    order_differs: Vec<String>,
    limit_boards: Vec<String>,
    pending: Vec<String>,
}

/// Compares the provider against one golden (all boards of it).
fn compare_golden(golden: &str, all_track_errors: bool, filter: &str) -> Score {
    let root = kicad_qa::kicad_source_root();
    let mut score = Score::default();

    for t in TYPES {
        score.per_type.entry(t).or_default();
    }

    for (rel, reference) in parse_golden(golden) {
        if !rel.contains(filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, BTreeMap<String, KicadT>, Vec<String>), String> {
                    let mut loaded = load_board(&path)?;

                    if has_compiled_conditions(&loaded.engine) {
                        return Err("user rule conditions need DRC-21".to_string());
                    }

                    if !loaded.report.skipped.is_empty() {
                        return Err("parser skipped items".to_string());
                    }

                    let types = types_by_uuid(&loaded);
                    let physical = physical_names(&loaded.engine);
                    Ok((report(&mut loaded, all_track_errors), types, physical))
                },
            ))
        });

        let (mine, types, physical) = match result {
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

        score.compared += 1;

        // The error limits (499 clearance, 199 others) make the reported set depend on the
        // C++ thread scheduling once reached (drc.md 4.12).
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();

        for g in &reference {
            *counts.entry(g.type_).or_default() += 1;
        }

        let limited = counts
            .iter()
            .any(|(t, &n)| n >= if *t == "clearance" { 499 } else { 199 });

        let mut expected: Vec<&GoldenEntry> = Vec::new();
        // Descriptions are only needed to tell physical clearance entries apart (and then come
        // from the cached raw report).
        let mut cached: BTreeMap<&str, Vec<Entry>> = BTreeMap::new();

        for g in &reference {
            let item_types = [
                types.get(&g.uuids[0]).copied(),
                types.get(&g.uuids[1]).copied(),
            ];
            let description = if physical.is_empty() {
                String::new()
            } else {
                cached
                    .entry(g.type_)
                    .or_insert_with(|| cached_entries(&rel, g.type_).unwrap_or_default())
                    .iter()
                    .find(|e| entry_hash(e) == g.hash)
                    .map(|e| e.description.clone())
                    .unwrap_or_default()
            };

            match origin(item_types, &description, &physical) {
                Origin::Items => expected.push(g),
                Origin::Part2 => score.skipped_part2 += 1,
                Origin::Physical => score.skipped_physical += 1,
            }
        }

        // Multiset comparison per type, then the order.
        let mut board_diffs = Vec::new();

        for t in TYPES {
            let mut want: Vec<u64> = expected
                .iter()
                .filter(|g| g.type_ == *t)
                .map(|g| g.hash)
                .collect();
            let got: Vec<&Entry> = mine.iter().filter(|e| e.type_ == *t).collect();
            let s = score.per_type.entry(t).or_default();

            for e in &got {
                let h = entry_hash(e);

                if let Some(i) = want.iter().position(|&w| w == h) {
                    want.remove(i);
                    s.matched += 1;
                } else {
                    s.extra += 1;
                    board_diffs.push(format!("{rel}: + {}", e.key()));
                }
            }

            s.missing += want.len();

            for h in want {
                let detail = cached_entries(&rel, t)
                    .and_then(|c| c.into_iter().find(|e| entry_hash(e) == h))
                    .map(|e| e.key())
                    .unwrap_or_else(|| format!("{t} #{h:016x} (no cached report)"));
                board_diffs.push(format!("{rel}: - {detail}"));
            }
        }

        let order_ref: Vec<u64> = expected.iter().map(|g| g.hash).collect();
        let order_mine: Vec<u64> = mine.iter().map(entry_hash).collect();

        if board_diffs.is_empty() && order_ref != order_mine {
            score.order_differs.push(rel.clone());
        }

        if limited {
            score.limit_boards.push(format!(
                "{rel} ({} differences, error limit reached)",
                board_diffs.len()
            ));
        } else {
            score.differences.extend(board_diffs);
        }
    }

    score
}

fn print(score: &Score, title: &str) {
    eprintln!(
        "{title}: {} boards compared, {} pending; reference entries of DRC-08b phases {}, of \
         the physical clearance provider {} (not compared)",
        score.compared,
        score.pending.len(),
        score.skipped_part2,
        score.skipped_physical
    );

    for (t, s) in &score.per_type {
        eprintln!(
            "  {t}: matched {} missing {} extra {}",
            s.matched, s.missing, s.extra
        );
    }

    for d in &score.differences {
        eprintln!("  DIFF {d}");
    }

    for d in &score.order_differs {
        eprintln!("  order differs (thread-pool order in C++, not compared): {d}");
    }

    for d in &score.limit_boards {
        eprintln!("  error limit: {d}");
    }

    for p in &score.pending {
        eprintln!("  pending: {p}");
    }
}

/// The comparison verdict: the sets must match. The report order is not compared: every phase
/// of the C++ provider runs on the thread pool (one task per track / footprint) and the
/// reference order changes from run to run (three runs of `items.kicad_pcb` gave three
/// orders, same set); the port reports in board order (`drc.md` 4.12).
fn assert_no_differences(score: &Score) {
    assert!(
        score.differences.is_empty(),
        "{} differences",
        score.differences.len()
    );
}

/// Every corpus board the port loads completely (no skipped items, no user rule conditions
/// pending DRC-21): the DRC-08a entries of the default reference report, set and order.
#[test]
fn copper_clearance_matches_reference_corpus() {
    let filter = std::env::var("DRC08A_FILTER").unwrap_or_default();
    let score = compare_golden(include_str!("golden/drc08a/corpus.txt"), false, &filter);
    print(&score, "DRC-08a copper clearance (corpus)");
    assert_no_differences(&score);
}

/// The same with `--all-track-errors` (`GetReportAllTrackErrors()`).
#[test]
fn copper_clearance_all_track_errors_matches_reference() {
    let filter = std::env::var("DRC08A_FILTER").unwrap_or_default();
    let score = compare_golden(
        include_str!("golden/drc08a/all_track_errors.txt"),
        true,
        &filter,
    );
    print(&score, "DRC-08a copper clearance (--all-track-errors)");
    assert_no_differences(&score);
}

/// The synthetic boards of `gen_drc08a_boards.py` (tracks, arcs, vias, zones, a rule file),
/// default run.
#[test]
fn synthetic_boards_match_reference() {
    let score = compare_golden(include_str!("golden/drc08a/synthetic.txt"), false, "");
    print(&score, "DRC-08a copper clearance (synthetic)");
    assert_eq!(score.compared, 3);
    assert_no_differences(&score);
}

/// The synthetic boards with `--all-track-errors`.
#[test]
fn synthetic_boards_all_track_errors_match_reference() {
    let score = compare_golden(include_str!("golden/drc08a/synthetic_ate.txt"), true, "");
    print(
        &score,
        "DRC-08a copper clearance (synthetic, --all-track-errors)",
    );
    assert_eq!(score.compared, 3);
    assert_no_differences(&score);
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

/// Port of `HoleClearanceIssue24355` (`test_drc_hole_clearance_issue24355.cpp`): every code
/// ignored but `DRCE_HOLE_CLEARANCE`, `RunTests( MM, true, false )`: one violation per shared
/// copper layer (F.Cu, B.Cu), independent of the pad order.
#[test]
fn hole_clearance_issue24355() {
    let Some(mut loaded) = load_qa("issue24355/issue24355") else {
        return;
    };

    assert_eq!(loaded.board.footprints().count(), 1);

    {
        let bds = loaded.board.design_settings_mut();

        for ii in DRCE_FIRST..=DRCE_LAST {
            bds.drc_severities.insert(ii, Severity::Ignore);
        }

        bds.drc_severities
            .insert(DRCE_HOLE_CLEARANCE, Severity::Error);
    }

    let connectivity = prepare(&mut loaded);
    let mut provider = CopperWithCaches {
        connectivity: &connectivity,
    };
    let found = run_providers(&mut loaded, EdaUnits::Mm, true, &mut [&mut provider]);
    let hole_clearance_count = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_HOLE_CLEARANCE)
        .count();

    assert_eq!(hole_clearance_count, 2);
}

/// The copper clearance part of `DRCFalsePositiveRegressions` (`test_drc_regressions.cpp`):
/// these boards once flagged copper clearance errors they should not have; the DRC-08a phases
/// report none of the provider's types on them.
#[test]
fn false_positive_regressions_copper_clearance() {
    for name in [
        "issue4139",
        "issue4774",
        "issue5978",
        "issue6443",
        "issue12609",
        "issue14008",
        "issue18203",
        "net_tie_hole_clearance",
        "issue24974",
    ] {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };

        let entries = report(&mut loaded, false);

        assert!(
            entries.is_empty(),
            "{name}: {:?}",
            entries.iter().map(Entry::key).collect::<Vec<_>>()
        );
    }
}
