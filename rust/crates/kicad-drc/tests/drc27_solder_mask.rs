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

//! DRC-27: the solder mask provider (`drc_test_provider_solder_mask.cpp`) against the reference
//! DRC reports of the shared corpus, plus the ported C++ tests:
//!
//! * `qa/tests/pcbnew/drc/test_solder_mask_bridging.cpp` (`DRCSolderMaskBridgingTest`,
//!   `DRCSolderMaskBridgeDeterminismTest`)
//! * `qa/tests/pcbnew/drc/test_solder_mask_bridge_all_tracks.cpp`
//!   (`DRCSolderMaskBridgeAllTracksTest`, `DRCSolderMaskBridgeSingleErrorTest`)
//! * `qa/tests/pcbnew/drc/test_solder_mask_track_to_pad.cpp` (`DRCTrackToPadBridgeTest`)
//! * `qa/tests/pcbnew/drc/test_drc_solder_mask_npth_bridge.cpp` (`DRCSolderMaskNPTHBridge`)
//! * `qa/tests/pcbnew/drc/test_drc_regressions.cpp`: the solder mask boards of
//!   `DRCFalsePositiveRegressions` (`issue10906`, `issue14412`, `net_tie_drc`) and
//!   `DRCFalseNegativeRegressions` (`issue18878`, 12 violations).
//!
//! The provider needs the per-run board caches, so the run is `RunTests` with the cache
//! generator's phases (`prepare_board`, zone lists, zone trees) between the time stamp and the
//! provider, as `DRC_ENGINE::RunTests` orders them.

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, Reported, cached_entries, cli_report_entries, golden, has_compiled_conditions,
    load_board, panic_text, quiet, run_providers,
};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::cache_generator::{build_zone_caches, build_zone_lists, prepare_board};
use kicad_drc::caches::DrcBoardCaches;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::providers::solder_mask::DrcTestProviderSolderMask;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_SILK_MASK_CLEARANCE, DRCE_SOLDERMASK_BRIDGE};

/// The error types of the provider.
const TYPES: &[&str] = &["solder_mask_bridge", "silk_over_copper"];

/// The solder mask provider with the parts of `DRC_CACHE_GENERATOR::Run()` it reads (zone
/// lists, zone trees) built from the run's context right before it, as `RunTests` orders them.
/// The board-mutating part of the cache generator ([`prepare_board`]) runs before the run.
struct SolderMaskWithCaches;

impl DrcTestProviderRun for SolderMaskWithCaches {
    fn name(&self) -> &str {
        "solder_mask_issues"
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        let mut caches = DrcBoardCaches::new();
        build_zone_lists(ctx, &mut caches);
        build_zone_caches(ctx, &mut caches);

        let mut provider = DrcTestProviderSolderMask::new(&caches);
        provider.run_tests(engine, run, ctx, units)
    }
}

/// `RunTests( aUnits, aReportAllTrackErrors, false )` with the solder mask provider only.
fn run_solder_mask(loaded: &mut Loaded, report_all_track_errors: bool) -> Vec<Reported> {
    prepare_board(&mut loaded.board);

    let mut provider = SolderMaskWithCaches;
    run_providers(
        loaded,
        EdaUnits::Mm,
        report_all_track_errors,
        &mut [&mut provider],
    )
}

/// The report entries (`kicad-cli pcb drc --format json`) of a run of the solder mask provider.
fn report_solder_mask(loaded: &mut Loaded, report_all_track_errors: bool) -> Vec<Entry> {
    prepare_board(&mut loaded.board);

    let mut provider = SolderMaskWithCaches;
    cli_report_entries(loaded, report_all_track_errors, &mut [&mut provider])
}

/// Whether a reference entry was produced by this provider: every `solder_mask_bridge`, and
/// the `silk_over_copper` entries with one item (the silk clearance provider reports the same
/// type with two items).
fn is_ours(e: &Entry) -> bool {
    e.type_ == "solder_mask_bridge" || (e.type_ == "silk_over_copper" && e.items.len() == 1)
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

/// The reference entries of the provider for a board: the cached raw report when present
/// (complete), else the golden entries (types stored as a summary cannot be filtered).
fn reference_entries(rel: &str, golden_entries: &[Entry]) -> Option<Vec<Entry>> {
    let mut out = Vec::new();

    for t in TYPES {
        match cached_entries(rel, t) {
            Some(v) => out.extend(v.into_iter().filter(is_ours)),
            None => out.extend(
                golden_entries
                    .iter()
                    .filter(|e| e.type_ == *t && is_ours(e))
                    .cloned(),
            ),
        }
    }

    Some(out)
}

#[derive(Default)]
struct Score {
    matched: BTreeMap<String, usize>,
    missing: BTreeMap<String, usize>,
    extra: BTreeMap<String, usize>,
    compared: usize,
    order_differs: Vec<String>,
    differences: Vec<String>,
    pending: Vec<String>,
}

#[test]
fn solder_mask_matches_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC27_FILTER").unwrap_or_default();
    let mut score = Score::default();

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        // Boards with a type stored as a summary need the cached raw report.
        let summarized = TYPES.iter().any(|t| reference.summaries.contains_key(*t));

        if summarized && cached_entries(&rel, TYPES[0]).is_none() {
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

                    if has_compiled_conditions(&loaded.engine) {
                        return Err("user rule conditions need DRC-21".to_string());
                    }

                    let entries = report_solder_mask(&mut loaded, false)
                        .into_iter()
                        .filter(|e| TYPES.contains(&e.type_.as_str()))
                        .collect();

                    Ok((entries, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let (mine, complete) = match result {
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

        let theirs = reference_entries(&rel, &reference.entries).unwrap_or_default();
        score.compared += 1;

        for t in TYPES {
            let r: Vec<Entry> = theirs.iter().filter(|e| e.type_ == *t).cloned().collect();
            let m: Vec<Entry> = mine.iter().filter(|e| e.type_ == *t).cloned().collect();
            let (matched, missing, extra) = diff(&r, &m);

            *score.matched.entry(t.to_string()).or_default() += matched;
            *score.missing.entry(t.to_string()).or_default() += missing.len();
            *score.extra.entry(t.to_string()).or_default() += extra.len();

            for k in missing {
                score.differences.push(format!("{rel}: - {k}"));
            }

            for k in extra {
                score.differences.push(format!("{rel}: + {k}"));
            }

            if r.len() == m.len() && r != m && diff(&r, &m).1.is_empty() {
                score.order_differs.push(format!("{rel}: {t}"));
            }
        }
    }

    println!("DRC-27 solder mask: {} boards compared", score.compared);

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

/// The golden entries of a synthetic board (`tests/oracle/gen_drc27_boards.py`).
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

/// The synthetic boards (no footprints: tracks, arcs, vias, zones, mask graphics and texts,
/// silkscreen) against the reference: every entry of the provider, in report order, with and
/// without `--all-track-errors`.
#[test]
fn synthetic_boards_match_reference() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/solder_mask");
    let cases: &[(&str, bool, &str)] = &[
        ("mask_web", false, include_str!("golden/drc27/mask_web.txt")),
        (
            "mask_web",
            true,
            include_str!("golden/drc27/mask_web_all.txt"),
        ),
        (
            "mask_noweb",
            false,
            include_str!("golden/drc27/mask_noweb.txt"),
        ),
        (
            "mask_noweb",
            true,
            include_str!("golden/drc27/mask_noweb_all.txt"),
        ),
        (
            "mask_ignore_front",
            false,
            include_str!("golden/drc27/mask_ignore_front.txt"),
        ),
        (
            "mask_ignore_front",
            true,
            include_str!("golden/drc27/mask_ignore_front_all.txt"),
        ),
    ];
    let mut failures = Vec::new();

    for &(name, all_tracks, golden_text) in cases {
        let mut loaded = load_board(&data.join(format!("{name}.kicad_pcb"))).expect(name);
        assert!(
            loaded.report.skipped.is_empty(),
            "{name}: {:?}",
            loaded.report.skipped
        );

        let mine: Vec<Entry> = report_solder_mask(&mut loaded, all_tracks)
            .into_iter()
            .filter(is_ours)
            .collect();
        let reference = synthetic_golden(golden_text);

        if mine != reference {
            let (matched, missing, extra) = diff(&reference, &mine);
            failures.push(format!(
                "{name} (all tracks: {all_tracks}): matched {matched}, missing {missing:#?}, \
                 extra {extra:#?}"
            ));
        } else {
            println!(
                "{name} (all tracks: {all_tracks}): {} entries identical",
                mine.len()
            );
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}

/// Marker positions (not part of the JSON report, but of the exclusion identity): the markers of
/// the Rust run on the synthetic boards are written as DRC exclusions into a copy of the project
/// and the reference is run on it; every entry of the provider must come back excluded, which
/// needs the same error code, items, layer and marker position (`DRC_EXCLUSION::GetSortKey`).
/// Needs the reference `kicad-cli` (skips silently without it).
#[test]
fn marker_positions_match_reference_via_exclusions() {
    use kicad_core::json::{self, ObjectOrder, Value};

    let Some(cli) = kicad_qa::oracle_cli() else {
        return;
    };

    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/solder_mask");
    let scratch = kicad_qa::kicad_source_root().join("rust/target/drc27-oracle");

    for (case, (name, all_tracks)) in [
        ("mask_web", false),
        ("mask_web", false),
        ("mask_web", true),
        ("mask_noweb", true),
        ("mask_ignore_front", false),
    ]
    .into_iter()
    .enumerate()
    {
        let perturb = case == 0;
        let mut loaded = load_board(&data.join(format!("{name}.kicad_pcb"))).expect(name);
        let found = run_solder_mask(&mut loaded, all_tracks);
        let exclusions: Vec<Value> = found
            .iter()
            .filter(|r| {
                r.item.error_code() == DRCE_SOLDERMASK_BRIDGE
                    || r.item.error_code() == DRCE_SILK_MASK_CLEARANCE
            })
            .enumerate()
            .map(|(i, r)| {
                let mut marker = r.marker.clone();

                // Negative control: the first marker of the first case is moved by 1 nm and
                // must then not match.
                if perturb && i == 0 {
                    marker.pos.x += 1;
                }

                kicad_drc::exclusion::from_marker(&marker).json
            })
            .collect();
        let n = exclusions.len();

        let dir = scratch.join(format!("{name}_{all_tracks}_{case}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();

        for ext in ["kicad_pcb", "kicad_dru"] {
            let src = data.join(format!("{name}.{ext}"));

            if src.exists() {
                std::fs::copy(&src, dir.join(format!("{name}.{ext}"))).unwrap();
            }
        }

        let pro_text = std::fs::read(data.join(format!("{name}.kicad_pro"))).unwrap();
        let mut pro = json::parse_bytes(&pro_text, ObjectOrder::Sorted, true).unwrap();
        pro["board"]["design_settings"]["drc_exclusions"] = Value::Array(exclusions);
        std::fs::write(
            dir.join(format!("{name}.kicad_pro")),
            json::dump(&pro, Some(2), ObjectOrder::Sorted),
        )
        .unwrap();

        let out = dir.join("out.json");
        let mut cmd = std::process::Command::new(&cli);
        cmd.args([
            "pcb",
            "drc",
            "--format",
            "json",
            "--severity-all",
            "--units",
            "mm",
        ]);

        if all_tracks {
            cmd.arg("--all-track-errors");
        }

        let status = cmd
            .arg("-o")
            .arg(&out)
            .arg(dir.join(format!("{name}.kicad_pcb")))
            .env("KICAD_ORACLE_HOME", dir.join("home"))
            .current_dir(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(out.exists(), "{name}: reference run failed ({status})");

        let report =
            json::parse_bytes(&std::fs::read(&out).unwrap(), ObjectOrder::Sorted, true).unwrap();
        let mut ours = 0;
        let mut not_excluded = Vec::new();

        for v in report["violations"].as_array().unwrap() {
            let t = v["type"].as_str().unwrap_or("");
            let items = v["items"].as_array().map_or(0, |a| a.len());

            if t == "solder_mask_bridge" || (t == "silk_over_copper" && items == 1) {
                ours += 1;

                if v["excluded"].as_bool() != Some(true) {
                    not_excluded.push(v["description"].to_string());
                }
            }
        }

        assert_eq!(ours, n, "{name}: entry count");

        if perturb {
            assert_eq!(not_excluded.len(), 1, "the moved marker must not match");
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }

        assert!(
            not_excluded.is_empty(),
            "{name} (all tracks: {all_tracks}): marker positions differ for {not_excluded:#?}"
        );
        println!("{name} (all tracks: {all_tracks}): {n} markers matched by exclusion");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Loads a QA board (`qa/data/pcbnew/<name>.kicad_pcb`); `None` (reported) while the port
/// cannot load it completely.
fn load_qa(name: &str) -> Option<Loaded> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));
    let loaded = quiet(|| catch_unwind(AssertUnwindSafe(|| load_board(&path))));

    match loaded {
        Ok(Ok(l)) if l.report.skipped.is_empty() => Some(l),
        Ok(Ok(l)) => {
            println!("{name}: pending, parser skipped {:?}", l.report.skipped);
            None
        }
        Ok(Err(e)) => {
            println!("{name}: pending, {e}");
            None
        }
        Err(e) => {
            println!("{name}: pending, panic: {}", panic_text(&*e));
            None
        }
    }
}

/// Runs the provider, catching `port_pending!` panics of other packages (reported).
fn try_run(loaded: &mut Loaded, report_all_track_errors: bool) -> Option<Vec<Reported>> {
    match quiet(|| {
        catch_unwind(AssertUnwindSafe(|| {
            run_solder_mask(loaded, report_all_track_errors)
        }))
    }) {
        Ok(v) => Some(v),
        Err(e) => {
            println!("pending: panic: {}", panic_text(&*e));
            None
        }
    }
}

/// `bds.GetSeverity( code ) == RPT_SEVERITY_ERROR` violations of `code` (the regression
/// fixture's handler).
fn errors_of(loaded: &Loaded, found: &[Reported], code: i32) -> usize {
    found
        .iter()
        .filter(|r| r.item.error_code() == code)
        .filter(|r| {
            loaded
                .board
                .design_settings()
                .get_severity(r.item.error_code())
                == Severity::Error
        })
        .count()
}

/// `test_solder_mask_bridging.cpp`: `DRCSolderMaskBridgingTest` (4 violations; the C++ refills
/// the zones first, the port runs on the fills saved in the file).
#[test]
fn drc_solder_mask_bridging_test() {
    let Some(mut loaded) = load_qa("solder_mask_bridge_test") else {
        return;
    };
    let Some(found) = try_run(&mut loaded, true) else {
        return;
    };

    // The fixture ignores DRCE_SILK_MASK_CLEARANCE (and codes other providers produce).
    let n = found
        .iter()
        .filter(|r| r.item.error_code() != DRCE_SILK_MASK_CLEARANCE)
        .count();
    assert_eq!(n, 4);
}

/// `test_solder_mask_bridging.cpp`: `DRCSolderMaskBridgeDeterminismTest`: the board saturates
/// the cap, the kept bridges come out sorted, and repeated runs give identical reports.
#[test]
fn drc_solder_mask_bridge_determinism_test() {
    let Some(mut loaded) = load_qa("issue24951/issue24951") else {
        return;
    };

    let sig = |found: &[Reported]| -> Vec<String> {
        found
            .iter()
            .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
            .map(|r| {
                let ids = r.item.ids();
                let id = |i: usize| ids.get(i).map(|k| k.as_string()).unwrap_or_default();
                format!(
                    "{},{},{},{},{},{}",
                    r.layer,
                    r.pos.x,
                    r.pos.y,
                    id(0),
                    id(1),
                    id(2)
                )
            })
            .collect()
    };

    let Some(found) = try_run(&mut loaded, true) else {
        return;
    };
    let reference = sig(&found);

    assert!(reference.len() > 100, "{} bridges", reference.len());

    // Mirror of the provider's reporting order: layer, then the sorted UUIDs.
    let keys: Vec<(i32, Vec<kicad_core::Kiid>)> = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
        .map(|r| {
            let mut ids: Vec<kicad_core::Kiid> = r.item.ids().to_vec();
            ids.sort();
            (r.layer, ids)
        })
        .collect();

    assert!(
        keys.windows(2).all(|w| w[0] <= w[1]),
        "not reported in sorted order"
    );

    for _ in 0..2 {
        let found = try_run(&mut loaded, true).expect("second run");
        assert_eq!(sig(&found), reference);
    }
}

/// `test_solder_mask_bridge_all_tracks.cpp`: `DRCSolderMaskBridgeAllTracksTest` and
/// `DRCSolderMaskBridgeSingleErrorTest`.
#[test]
fn drc_solder_mask_bridge_all_tracks_test() {
    let Some(mut loaded) = load_qa("soldermask_bridge_all_tracks") else {
        return;
    };
    let Some(all) = try_run(&mut loaded, true) else {
        return;
    };
    let all: Vec<&Reported> = all
        .iter()
        .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
        .collect();

    assert!(all.len() > 5, "{} violations", all.len());

    let net_a_items: std::collections::BTreeSet<kicad_core::Kiid> = all
        .iter()
        .filter_map(|r| r.item.ids().get(1).copied())
        .filter(|id| *id != kicad_core::NILUUID)
        .collect();

    assert!(net_a_items.len() >= 4, "{} net A items", net_a_items.len());

    let single = try_run(&mut loaded, false).expect("single error run");
    let single = single
        .iter()
        .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
        .count();

    assert!(all.len() >= single);
}

/// `test_solder_mask_track_to_pad.cpp`: `DRCTrackToPadBridgeTest`.
#[test]
fn drc_track_to_pad_bridge_test() {
    let Some(mut loaded) = load_qa("soldermask_track_to_pad") else {
        return;
    };
    let Some(found) = try_run(&mut loaded, true) else {
        return;
    };
    let bridges: Vec<&Reported> = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
        .collect();

    assert!(!bridges.is_empty());

    let board = &loaded.board;
    let ctx = BoardCtx::bare(board);
    let found_track = bridges.iter().any(|r| {
        r.item.ids().iter().take(2).any(|id| {
            board
                .resolve_item(id)
                .and_then(|i| ctx.item(i))
                .is_some_and(|i| i.type_() == kicad_core::typeinfo::KicadT::PcbTraceT)
        })
    });

    assert!(
        found_track,
        "Expected to find a track-to-pad soldermask violation"
    );
}

/// `test_drc_solder_mask_npth_bridge.cpp`: `DRCSolderMaskNPTHBridge` (the C++ refills the
/// zones first; the port runs on the saved fills).
#[test]
fn drc_solder_mask_npth_bridge() {
    let Some(mut loaded) = load_qa("npth_mask_bridge_test") else {
        return;
    };
    let Some(found) = try_run(&mut loaded, true) else {
        return;
    };

    // Everything exposed by the F.Mask polygon is either GND (zone, vias) or an NPTH (no net).
    // No real bridge exists.
    let n = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_SOLDERMASK_BRIDGE)
        .count();
    assert_eq!(n, 0);
}

/// `test_drc_regressions.cpp`, `DRCFalsePositiveRegressions`: the solder mask boards report no
/// error of this provider.
#[test]
fn drc_false_positive_regressions() {
    for name in ["issue10906", "issue14412", "net_tie_drc"] {
        let Some(mut loaded) = load_qa(name) else {
            continue;
        };
        let Some(found) = try_run(&mut loaded, true) else {
            continue;
        };

        assert_eq!(
            errors_of(&loaded, &found, DRCE_SOLDERMASK_BRIDGE)
                + errors_of(&loaded, &found, DRCE_SILK_MASK_CLEARANCE),
            0,
            "{name}"
        );
    }
}

/// `test_drc_regressions.cpp`, `DRCFalseNegativeRegressions`: `issue18878` (12 violations, all
/// solder mask bridges).
#[test]
fn drc_false_negative_regressions() {
    let Some(mut loaded) = load_qa("issue18878") else {
        return;
    };
    let Some(found) = try_run(&mut loaded, true) else {
        return;
    };

    assert_eq!(errors_of(&loaded, &found, DRCE_SOLDERMASK_BRIDGE), 12);
}
