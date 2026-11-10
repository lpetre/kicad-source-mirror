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

//! DRC-07: the connectivity provider (`drc_test_provider_connectivity.cpp`) against the
//! reference `kicad-cli pcb drc --format json --severity-all --units mm` reports of the shared
//! corpus (`tests/golden/drc_corpus.txt` for the `violations`, `tests/golden/
//! drc07_unconnected.txt` for the `unconnected_items` array, `gen_drc07_unconnected.py`), and
//! the port of `qa/tests/pcbnew/drc/test_drc_copper_thieving.cpp`.
//!
//! The provider needs the connectivity rebuilt by the cache generator and its islands map /
//! zone trees, so it runs through the real run path (`drc_harness::run_flow_providers`:
//! `BOARD_LOADER::Load`'s `BuildConnectivity`, then `run_flow::run_tests` with the cache
//! generator and the connectivity slot only).

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{
    Entry, Loaded, cached_entries, load_board, mask_generated_uuids, panic_text, quiet,
    run_flow_providers, summarize,
};
use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_drc::exclusion::{record_drc_exclusions, resolve_drc_exclusions};
use kicad_drc::item::DrcItemsProvider;
use kicad_drc::report::DrcReport;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::{DRCE_ISOLATED_COPPER, DRCE_LAST};
use kicad_pcb::pcb_marker::MarkerT;
use kicad_pcb::zone_settings::ZoneFillMode;

/// The error types of the provider in the `violations` array.
const TYPES: &[&str] = &[
    "track_dangling",
    "via_dangling",
    "track_on_post_machined_layer",
    "track_not_centered_on_via",
    "isolated_copper",
];

/// The type of the `unconnected_items` array.
const UNCONNECTED: &str = "unconnected_items";

/// The `violations` and `unconnected_items` of the CLI JSON report after a run of the
/// connectivity provider (the steps of `drc_harness::cli_report_entries`).
fn report_entries(loaded: &mut Loaded) -> (Vec<Entry>, Vec<Entry>) {
    for marker in resolve_drc_exclusions(&mut loaded.board, true) {
        loaded.board.add_marker(marker);
    }

    record_drc_exclusions(&mut loaded.board);
    loaded.board.delete_markers(true, true);

    let found = run_flow_providers(loaded, EdaUnits::Mm, false, &["connectivity"]);

    for r in found {
        loaded.board.add_marker(r.marker);
    }

    resolve_drc_exclusions(&mut loaded.board, false);

    let severities = Severity::Error as i32 | Severity::Warning as i32 | Severity::Exclusion as i32;
    let ctx = BoardCtx::bare(&loaded.board).with_rules(&loaded.engine);
    let mut markers = DrcItemsProvider::new(MarkerT::Drc, MarkerT::DrawingSheet);
    let mut ratsnest = DrcItemsProvider::new(MarkerT::Ratsnest, MarkerT::Unspec);
    let mut parity = DrcItemsProvider::new(MarkerT::Parity, MarkerT::Unspec);
    markers.set_severities(&ctx, severities);
    ratsnest.set_severities(&ctx, severities);
    parity.set_severities(&ctx, severities);

    let report =
        DrcReport::new(&ctx, EdaUnits::Mm, &markers, &ratsnest, &parity).json_report("", "");
    let convert = |list: Vec<kicad_drc::rc_item::JsonViolation>| -> Vec<Entry> {
        list.into_iter()
            .map(|v| Entry {
                severity: v.severity,
                excluded: v.excluded,
                type_: v.type_,
                description: v.description,
                items: v
                    .items
                    .into_iter()
                    .map(|i| (i.uuid, i.description, i.pos.x, i.pos.y))
                    .collect(),
            })
            .collect()
    };

    (
        convert(report.violations),
        convert(report.unconnected_items),
    )
}

/// One board of a golden file: full entries per type (report order) and summaries.
#[derive(Default)]
struct GoldenBoard {
    entries: Vec<Entry>,
    summaries: BTreeMap<String, (usize, u64)>,
}

/// Parses a golden in the `drc_corpus.txt` format (`None`: the reference run failed).
fn parse_golden(text: &str) -> BTreeMap<String, Option<GoldenBoard>> {
    let mut boards: BTreeMap<String, Option<GoldenBoard>> = BTreeMap::new();

    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();

        match f[1] {
            "load_error" => {
                boards.insert(f[0].to_string(), None);
                continue;
            }
            "none" => {
                boards.insert(f[0].to_string(), Some(GoldenBoard::default()));
                continue;
            }
            _ => {}
        }

        let board = boards
            .entry(f[0].to_string())
            .or_insert_with(|| Some(GoldenBoard::default()))
            .as_mut()
            .unwrap();

        if f[1] == "#" {
            board.summaries.insert(
                f[2].to_string(),
                (
                    f[3].parse().unwrap(),
                    u64::from_str_radix(f[4], 16).unwrap(),
                ),
            );
            continue;
        }

        board.entries.push(Entry {
            severity: f[2].trim_end_matches("/excluded").to_string(),
            excluded: f[2].ends_with("/excluded"),
            type_: f[3].to_string(),
            description: f[4].to_string(),
            items: f[5..]
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
        });
    }

    boards
}

/// The reference `unconnected_items` of a board from the cached raw report.
fn cached_unconnected(rel: &str) -> Option<Vec<Entry>> {
    use kicad_core::json::{self, ObjectOrder};

    let path = kicad_qa::kicad_source_root()
        .join("rust/target/drc-corpus")
        .join(format!("{rel}.json"));
    let doc = json::parse_bytes(&std::fs::read(path).ok()?, ObjectOrder::Sorted, true).ok()?;

    Some(
        doc["unconnected_items"]
            .as_array()?
            .iter()
            .map(|v| Entry {
                severity: v["severity"].as_str().unwrap_or("").to_string(),
                excluded: v["excluded"].as_bool().unwrap_or(false),
                type_: v["type"].as_str().unwrap_or("").to_string(),
                description: v["description"].as_str().unwrap_or("").to_string(),
                items: v["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|i| {
                        (
                            i["uuid"].as_str().unwrap_or("").to_string(),
                            i["description"].as_str().unwrap_or("").to_string(),
                            i["pos"]["x"].as_f64().unwrap_or(0.0),
                            i["pos"]["y"].as_f64().unwrap_or(0.0),
                        )
                    })
                    .collect(),
            })
            .collect(),
    )
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

#[derive(Default)]
struct TypeScore {
    matched: usize,
    missing: usize,
    extra: usize,
}

#[derive(Default)]
struct Score {
    per_type: BTreeMap<String, TypeScore>,
    compared: usize,
    /// Same entries, different report order (per board and type).
    order_differs: Vec<String>,
    differences: Vec<String>,
    pending: Vec<String>,
}

impl Score {
    /// Compares the entries of `type_` on one board: summary (count + hash in report order)
    /// or full entries, with the cached report for the details of a summary mismatch.
    fn compare(
        &mut self,
        rel: &str,
        type_: &str,
        golden: &GoldenBoard,
        mine: &[Entry],
        cached: impl FnOnce() -> Option<Vec<Entry>>,
    ) {
        let mine: Vec<Entry> = mine.iter().filter(|e| e.type_ == type_).cloned().collect();
        let refs: Vec<&Entry> = mine.iter().collect();
        let s = self.per_type.entry(type_.to_string()).or_default();

        let reference: Vec<Entry> = match golden.summaries.get(type_) {
            Some(&(n, h)) => {
                if summarize(&refs) == (n, h) {
                    s.matched += n;
                    return;
                }

                match cached() {
                    Some(c) => c.into_iter().filter(|e| e.type_ == type_).collect(),
                    None => {
                        s.missing += n;
                        s.extra += mine.len();
                        self.differences.push(format!(
                            "{rel}: {type_}: {} entries, reference {n} (hash differs; no cached \
                             report)",
                            mine.len()
                        ));
                        return;
                    }
                }
            }
            None => golden
                .entries
                .iter()
                .filter(|e| e.type_ == type_)
                .cloned()
                .collect(),
        };

        // Items saved without a UUID (teardrops) get a random KIID at every load, also in
        // the reference: compare them by description and position.
        let (mut reference, mut mine) = (reference, mine);
        let board_text =
            std::fs::read_to_string(kicad_qa::kicad_source_root().join(rel)).unwrap_or_default();
        mask_generated_uuids(&mut reference, &board_text);
        mask_generated_uuids(&mut mine, &board_text);

        let (matched, missing, extra) = diff(&reference, &mine);
        s.matched += matched;
        s.missing += missing.len();
        s.extra += extra.len();

        if missing.is_empty() && extra.is_empty() && reference != mine {
            self.order_differs.push(format!("{rel}: {type_}"));
        }

        for k in missing {
            self.differences.push(format!("{rel}: - {k}"));
        }

        for k in extra {
            self.differences.push(format!("{rel}: + {k}"));
        }
    }
}

/// The corpus comparison: every board the port loads completely (no skipped items) is
/// compared on the provider's error types; the
/// `isolated_copper` order may differ between zones (`ZONE*` address order in C++, board
/// order in the port, `drc.md` 4.12) and is reported, not failed.
#[test]
fn connectivity_matches_reference_corpus() {
    let root = kicad_qa::kicad_source_root();
    let filter = std::env::var("DRC07_FILTER").unwrap_or_default();
    let violations = drc_harness::golden();
    let unconnected = parse_golden(include_str!("golden/drc07_unconnected.txt"));
    let mut score = Score::default();

    for (rel, reference) in &violations {
        if !rel.contains(&filter) {
            continue;
        }

        let (Some(reference), Some(Some(unconnected_ref))) = (reference, unconnected.get(rel))
        else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        let path = root.join(rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, Vec<Entry>, bool), String> {
                    let mut loaded = load_board(&path)?;

                    let (v, u) = report_entries(&mut loaded);
                    Ok((v, u, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let (mine, mine_unconnected, complete) = match result {
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

        let golden_ref = GoldenBoard {
            entries: reference.entries.clone(),
            summaries: reference.summaries.clone(),
        };

        score.compared += 1;

        for t in TYPES {
            score.compare(rel, t, &golden_ref, &mine, || cached_entries(rel, t));
        }

        score.compare(rel, UNCONNECTED, unconnected_ref, &mine_unconnected, || {
            cached_unconnected(rel)
        });
    }

    eprintln!(
        "DRC-07 connectivity: {} boards compared, {} pending",
        score.compared,
        score.pending.len()
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
        eprintln!("  order differs: {d}");
    }

    for p in &score.pending {
        eprintln!("  pending: {p}");
    }

    let unexpected_order: Vec<&String> = score
        .order_differs
        .iter()
        .filter(|d| !d.ends_with(": isolated_copper"))
        .collect();

    assert!(
        score.differences.is_empty() && unexpected_order.is_empty(),
        "{} differences, {} order differences",
        score.differences.len(),
        unexpected_order.len()
    );
}

/// Port of `qa/tests/pcbnew/drc/test_drc_copper_thieving.cpp`
/// (`ThievingZoneProducesNoIsolatedCopperViolations`): a copper-thieving fill is netless dummy
/// copper; none of its stamps is reported as isolated copper.
#[test]
fn thieving_zone_produces_no_isolated_copper_violations() {
    let path = kicad_qa::pcbnew_data_dir().join("drc_copper_thieving/copper_thieving.kicad_pcb");
    let mut loaded = load_board(&path).expect("copper_thieving board");

    // A board with no thieving fill also reports no isolated copper, so confirm
    // the stamps the exclusion has to cover are actually present.
    assert_eq!(loaded.board.zones().count(), 1);

    let (_, zone) = loaded.board.zones().next().unwrap();
    assert_eq!(zone.fill_mode(), ZoneFillMode::CopperThieving);
    assert!(
        zone.filled_polys_list(kicad_core::PcbLayerId::FCu)
            .outline_count()
            > 1
    );

    // Disable all DRC checks except the one we care about so the test focuses on
    // the isolated-copper path.
    for code in 0..DRCE_LAST {
        if code != DRCE_ISOLATED_COPPER {
            loaded
                .board
                .design_settings_mut()
                .drc_severities
                .insert(code, Severity::Ignore);
        }
    }

    // InitEngine: the error limits follow the severities.
    let found = run_flow_providers(&mut loaded, EdaUnits::Mm, true, &["connectivity"]);
    let isolated_count = found
        .iter()
        .filter(|r| r.item.error_code() == DRCE_ISOLATED_COPPER)
        .count();

    assert_eq!(isolated_count, 0);
}

/// The synthetic board of `gen_drc07_boards.py` (backdrilled / tertiary-drilled vias,
/// off-centre track ends, dangling items, an unconnected net) against its reference report:
/// every entry of the provider's types, in report order.
#[test]
fn synthetic_post_machined_board_matches_reference() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/drc07/post_machined.kicad_pcb");
    let golden = parse_golden(include_str!("golden/drc07_synthetic.txt"));
    let reference = golden
        .values()
        .next()
        .and_then(|b| b.as_ref())
        .expect("reference report");

    let mut loaded = load_board(&path).expect("synthetic board");
    assert!(
        loaded.report.skipped.is_empty(),
        "{:?}",
        loaded.report.skipped
    );

    let (violations, unconnected) = report_entries(&mut loaded);
    let mine: Vec<String> = violations
        .iter()
        .chain(unconnected.iter())
        .filter(|e| TYPES.contains(&e.type_.as_str()) || e.type_ == UNCONNECTED)
        .map(Entry::key)
        .collect();
    let theirs: Vec<String> = reference.entries.iter().map(Entry::key).collect();

    assert!(reference.summaries.is_empty());
    assert_eq!(theirs.len(), 23);
    assert_eq!(mine, theirs);
}
