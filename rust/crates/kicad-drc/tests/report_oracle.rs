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

//! Differential test of work package DRC-11 (markers, severities, exclusions, report writers):
//! the Rust twin of `tests/oracle/drc_report_oracle.cpp`. Every scenario of
//! `tests/data/reports/*.scn` is replayed and compared line by line with
//! `tests/golden/report_oracle.txt`, produced by the C++ driver linked against the reference
//! build (`tests/oracle/gen_report_goldens.sh`).
//!
//! A scenario whose replay hits a `port_pending!` stub of another package (item descriptions
//! of the board model) is reported as pending, not as a failure; every other scenario must
//! match exactly.
//!
//! Also here: the comparison with reports of the reference `kicad-cli` (`#[ignore]`d until the
//! DRC engine exists).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_core::json::{ObjectOrder, dump, parse};
use kicad_core::{Kiid, KiidGenerator, PcbLayerId, Severity};
use kicad_drc::exclusion;
use kicad_drc::item::{self, DrcItemsProvider};
use kicad_drc::marker::from_legacy_string;
use kicad_drc::rc_item::strip_markup;
use kicad_drc::report::{DrcReport, KICAD_MAJOR_MINOR_PATCH_VERSION};
use kicad_geom::Vector2I;
use kicad_pcb::Board;
use kicad_pcb::board_design_settings::DrcExclusion;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcRuleInfo;
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn hex(key: &str) -> String {
    if key.is_empty() {
        return "-".to_string();
    }

    key.chars().map(|c| format!("{:02x}", c as u32)).collect()
}

fn fnv(key: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for c in key.chars() {
        h ^= u64::from(c as u32);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{h:016x}")
}

fn json_of(ex: &DrcExclusion) -> String {
    dump(&exclusion::to_json(ex), None, ObjectOrder::Sorted)
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Undefined => "undefined",
        Severity::Info => "info",
        Severity::Exclusion => "exclusion",
        Severity::Action => "action",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Ignore => "ignore",
        Severity::Debug => "debug",
    }
}

fn severity_from_name(name: &str) -> Severity {
    match name {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "ignore" => Severity::Ignore,
        "exclusion" => Severity::Exclusion,
        "info" => Severity::Info,
        _ => Severity::Undefined,
    }
}

/// `dumpMarker` of the C++ driver.
fn dump_marker(tag: &str, index: usize, marker: &PcbMarker, ctx: Option<&BoardCtx<'_>>) -> String {
    let item = marker.rc_item();
    let ex = exclusion::from_marker(marker);
    let ids: Vec<String> = item.ids().iter().map(Kiid::as_string).collect();
    let ids = if ids.is_empty() {
        "-".to_string()
    } else {
        ids.join(",")
    };

    let mut line = format!(
        "{tag}|{index}|code={}|key={}|pos={},{}|layer={}|type={}|ids={ids}|excluded={}|comment={}",
        item.error_code(),
        item.settings_key(),
        marker.pos().x,
        marker.pos().y,
        marker.base.layer.0,
        marker.marker_type() as i32,
        i32::from(marker.is_excluded()),
        marker.comment()
    );

    if let Some(ctx) = ctx {
        line += &format!(
            "|severity={}|treated={}",
            severity_name(marker.severity(ctx)),
            i32::from(marker.is_treated_as_excluded(ctx))
        );
    }

    line += &format!("|key64={}|json={}", fnv(&ex.sort_key), json_of(&ex));
    line
}

fn block(out: &mut Vec<String>, tag: &str, text: &str) {
    let mut lines: Vec<&str> = text.split('\n').collect();

    if lines.last() == Some(&"") {
        lines.pop();
    }

    out.push(format!("{tag}|lines={}", lines.len()));

    for l in lines {
        out.push(format!("> {l}"));
    }
}

/// `runScenario` of the C++ driver. Also returns why the replay cannot be complete (the
/// board parser skipped constructs of packages that have not landed), empty if it can.
fn run_scenario(root: &Path, path: &Path) -> (Vec<String>, String) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = vec![format!(
        "scenario|{}",
        path.file_name().unwrap().to_string_lossy()
    )];
    let mut board = Board::default();
    let mut generator = KiidGenerator::default();
    let mut skipped = String::new();

    for (line_no, line) in text.split('\n').enumerate() {
        let line_no = line_no + 1;

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let f: Vec<&str> = line.split('\t').collect();
        let field = |i: usize| f.get(i).copied().unwrap_or("");

        match f[0] {
            "board" => {
                let full = root.join(f[1]);
                let text = std::fs::read_to_string(&full).unwrap();
                // Constructs whose parser package has not landed yet (e.g. `(setup ...)`,
                // PCBIO-04) are skipped; the scenarios read nothing from them.
                let options = kicad_pcb::io::ParseOptions {
                    allow_pending: true,
                };
                let (loaded, report) = kicad_pcb::io::parse_board_report(&text, "", &options)
                    .unwrap_or_else(|e| panic!("cannot load {}: {}", f[1], e.what()));
                board = loaded;

                if !report.skipped.is_empty() {
                    let packages: Vec<&str> = report.skipped.iter().map(|s| s.package).collect();
                    skipped = format!("board parser pending: {packages:?}");
                }
                board.file_name = full.to_string_lossy().to_string();
                out.push(format!(
                    "board|{}",
                    full.file_name().unwrap().to_string_lossy()
                ));
            }
            "severity" => {
                let code = item::create_from_key(f[1]).unwrap().error_code;
                board
                    .design_settings_mut()
                    .drc_severities
                    .insert(code, severity_from_name(f[2]));
            }
            "exclusion" => {
                let ex = if f[1] == "json" {
                    let Ok(json) = parse(f[2], ObjectOrder::Sorted) else {
                        out.push(format!("error|{line_no}|bad json"));
                        continue;
                    };

                    exclusion::from_json(&json)
                } else {
                    exclusion::from_legacy_strings(f[2], field(3), &mut generator)
                };

                out.push(format!(
                    "exclusion|{line_no}|sortkey={}|comment={}|json={}",
                    hex(&ex.sort_key),
                    ex.comment,
                    json_of(&ex)
                ));

                // BOARD_DESIGN_SETTINGS reader: only entries with a sort key
                if !ex.sort_key.is_empty() {
                    board.design_settings_mut().drc_exclusions.insert(ex);
                }
            }
            "marker" => {
                let Some(mut drc_item) = item::create_from_key(f[1]) else {
                    out.push(format!("error|{line_no}|unknown key {}", f[1]));
                    continue;
                };

                let ids: Vec<Kiid> = if f[5] == "-" {
                    Vec::new()
                } else {
                    f[5].split(',')
                        .map(|s| Kiid::from_string_or_random(s, &mut generator))
                        .collect()
                };

                drc_item.set_item_ids(ids);

                if let Some(detail) = f[6].strip_prefix("D:") {
                    drc_item.set_error_detail(detail);
                } else if let Some(message) = f[6].strip_prefix("M:") {
                    drc_item.set_error_message(message);
                }

                if f[7] != "-" {
                    drc_item.violating_rule = Some(DrcRuleInfo {
                        name: f[7].to_string(),
                        severity: severity_from_name(f[8]),
                        implicit: false,
                        rule_index: None,
                    });
                }

                let layer = if f[4] == "DS" {
                    kicad_core::layer_ids::LAYER_DRAWINGSHEET
                } else {
                    f[4].parse().unwrap()
                };

                let pos = Vector2I::new(f[2].parse().unwrap(), f[3].parse().unwrap());
                let mut marker =
                    PcbMarker::new(&mut board.kiid_generator, Arc::new(drc_item), pos, layer);

                if f[9] == "1" {
                    marker.set_excluded(true, field(10));
                }

                board.add_marker(marker);
            }
            "resolve" => {
                let created = exclusion::resolve_drc_exclusions(&mut board, f[1] == "1");

                for (i, marker) in created.iter().enumerate() {
                    out.push(dump_marker("created", i, marker, None));
                }

                for marker in created {
                    board.add_marker(marker);
                }
            }
            "record" => exclusion::record_drc_exclusions(&mut board),
            "bds" => {
                for (i, ex) in board.design_settings().drc_exclusions.iter().enumerate() {
                    out.push(format!(
                        "bds|{i}|key64={}|comment={}|json={}",
                        fnv(&ex.sort_key),
                        ex.comment,
                        json_of(ex)
                    ));
                }
            }
            "report" => {
                let units = match f[1] {
                    "in" => EdaUnits::Inch,
                    "mils" => EdaUnits::Mils,
                    _ => EdaUnits::Mm,
                };
                let mask: i32 = f[2].parse().unwrap();
                let ctx = BoardCtx::bare(&board);

                for (i, (_, marker)) in board.markers().enumerate() {
                    out.push(dump_marker("marker", i, marker, Some(&ctx)));
                }

                for (i, ex) in board.design_settings().drc_exclusions.iter().enumerate() {
                    out.push(format!(
                        "bds|{i}|key64={}|comment={}|json={}",
                        fnv(&ex.sort_key),
                        ex.comment,
                        json_of(ex)
                    ));
                }

                let mut markers = DrcItemsProvider::new(MarkerT::Drc, MarkerT::DrawingSheet);
                let mut ratsnest = DrcItemsProvider::new(MarkerT::Ratsnest, MarkerT::Unspec);
                let mut fp_warnings = DrcItemsProvider::new(MarkerT::Parity, MarkerT::Unspec);
                markers.set_severities(&ctx, mask);
                ratsnest.set_severities(&ctx, mask);
                fp_warnings.set_severities(&ctx, mask);

                out.push(format!(
                    "counts|{}|{}|{}|errors={}|warnings={}",
                    markers.count(),
                    ratsnest.count(),
                    fp_warnings.count(),
                    markers.count_of_severity(&ctx, Severity::Error as i32),
                    markers.count_of_severity(&ctx, Severity::Warning as i32)
                ));

                let writer = DrcReport::new(&ctx, units, &markers, &ratsnest, &fp_warnings);
                block(&mut out, "text", &writer.write_text_report("<DATE>"));
                block(
                    &mut out,
                    "json",
                    &writer.write_json_report("<DATE>", KICAD_MAJOR_MINOR_PATCH_VERSION),
                );
            }
            "legacy" => match from_legacy_string(field(1), &mut generator) {
                Some(marker) => out.push(dump_marker("legacy", line_no, &marker, None)),
                None => out.push(format!("legacy|{line_no}|null")),
            },
            "strip" => out.push(format!("strip|{line_no}|{}", strip_markup(field(1)))),
            other => out.push(format!("error|{line_no}|unknown command {other}")),
        }
    }

    (out, skipped)
}

/// The golden output per scenario (and the version line).
fn golden() -> (String, BTreeMap<String, Vec<String>>) {
    let text = std::fs::read_to_string(crate_dir().join("tests/golden/report_oracle.txt")).unwrap();
    let mut version = String::new();
    let mut scenarios: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in text.lines() {
        if let Some(v) = line.strip_prefix("version|") {
            version = v.to_string();
        } else if let Some(name) = line.strip_prefix("scenario|") {
            current = Some(name.to_string());
            scenarios.insert(name.to_string(), vec![line.to_string()]);
        } else if let Some(name) = &current {
            scenarios.get_mut(name).unwrap().push(line.to_string());
        }
    }

    (version, scenarios)
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else {
        String::new()
    }
}

#[test]
fn report_oracle_replay() {
    let root = kicad_qa::kicad_source_root();
    let (version, scenarios) = golden();
    assert_eq!(version, KICAD_MAJOR_MINOR_PATCH_VERSION);

    let mut matched = Vec::new();
    let mut pending = Vec::new();
    let mut failures = Vec::new();

    for (name, want) in &scenarios {
        let path = crate_dir().join("tests/data/reports").join(name);
        let result = std::panic::catch_unwind(|| run_scenario(&root, &path));

        match result {
            Ok((got, skipped)) => {
                if &got == want {
                    matched.push(name.clone());
                } else if !skipped.is_empty() {
                    pending.push(format!("{name}: {skipped}"));
                } else {
                    let first = got
                        .iter()
                        .zip(want.iter())
                        .position(|(a, b)| a != b)
                        .unwrap_or(got.len().min(want.len()));
                    failures.push(format!(
                        "{name}: first difference at line {first}:\n  rust: {:?}\n  c++:  {:?}\n  ({} vs {} lines)",
                        got.get(first),
                        want.get(first),
                        got.len(),
                        want.len()
                    ));
                }
            }
            Err(payload) => {
                let text = panic_text(payload.as_ref());

                if text.contains("port pending") {
                    pending.push(format!("{name}: {text}"));
                } else {
                    failures.push(format!("{name}: panic: {text}"));
                }
            }
        }
    }

    eprintln!("report oracle: matched {matched:?}");

    for p in &pending {
        eprintln!("report oracle: PENDING {p}");
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // The scenarios without board items replay today; those with items need the board parser
    // (footprints, tracks) and the item descriptions of the board model.
    for name in [
        "exclusions_cli_noitems.scn",
        "exclusions_noitems.scn",
        "legacy_strip.scn",
        "report_sections_noitems.scn",
    ] {
        assert!(matched.iter().any(|m| m == name), "{name} must match");
    }
}

/// Exclusions survive a store / load cycle through the project-file JSON (the form
/// `BOARD_DESIGN_SETTINGS` writes) unchanged, including the set order.
#[test]
fn exclusion_json_round_trip_keeps_set() {
    let mut generator = KiidGenerator::default();
    let a = Kiid::from_name("a").as_string();
    let entries = [
        format!("clearance|1|2|{a}|{a}"),
        format!("unconnected_items|5|6|{a}|{a}"),
        format!("copper_sliver|-1|-2|{a}|B.Cu"),
        "unresolved_variable|9|9||".to_string(),
    ];

    let set: std::collections::BTreeSet<DrcExclusion> = entries
        .iter()
        .map(|e| exclusion::from_legacy_strings(e, "c", &mut generator))
        .collect();
    let reloaded: Vec<DrcExclusion> = set
        .iter()
        .map(|ex| {
            let text = dump(&exclusion::to_json(ex), Some(2), ObjectOrder::Sorted);
            exclusion::from_json(&parse(&text, ObjectOrder::Sorted).unwrap())
        })
        .collect();

    assert_eq!(set.iter().cloned().collect::<Vec<_>>(), reloaded);
    assert!(reloaded.iter().all(|ex| ex.comment == "c"));
    assert_eq!(PcbLayerId::BCu.0, 2);
}

// ---- reference kicad-cli reports (tests/golden/reports) --------------------------------------

/// A text report in comparable form: the creation date replaced, and inside each section the
/// entries (`[key]: ...` with their continuation lines) sorted, because the reference DRC runs
/// its providers on a thread pool and the order of some violations changes between runs (see
/// `tests/golden/reports/cases.txt`).
fn normalise_text_report(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut entries: Vec<String> = Vec::new();

    let flush = |out: &mut Vec<String>, entries: &mut Vec<String>| {
        entries.sort();
        out.append(entries);
    };

    for line in text.lines() {
        if line.starts_with("** Created on ") {
            flush(&mut out, &mut entries);
            out.push("** Created on <DATE> **".to_string());
        } else if line.starts_with('[') {
            entries.push(line.to_string());
        } else if line.starts_with("    ") && !line.starts_with("    - ") && !entries.is_empty() {
            let last = entries.last_mut().unwrap();
            last.push('\n');
            last.push_str(line);
        } else {
            flush(&mut out, &mut entries);
            out.push(line.to_string());
        }
    }

    flush(&mut out, &mut entries);
    out
}

/// A JSON report in comparable form: the date replaced and the three violation lists sorted
/// by their serialised entries (see [`normalise_text_report`]).
fn normalise_json_report(text: &str) -> String {
    let mut json = parse(text, ObjectOrder::Sorted).unwrap();
    let map = json.as_object_mut().unwrap();
    map.insert(
        "date".to_string(),
        kicad_core::json::Value::String("<DATE>".into()),
    );

    for key in ["violations", "unconnected_items", "schematic_parity"] {
        if let Some(kicad_core::json::Value::Array(list)) = map.get_mut(key) {
            list.sort_by_key(|v| dump(v, None, ObjectOrder::Sorted));
        }
    }

    dump(&json, Some(4), ObjectOrder::Sorted)
}

/// The cases of `tests/golden/reports/cases.txt`: (board, kicad-cli options).
fn reference_cases() -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(crate_dir().join("tests/golden/reports/cases.txt")).unwrap();

    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut words = l.split_whitespace().map(str::to_string);
            let board = words.next().unwrap();
            (board, words.collect())
        })
        .collect()
}

/// The recorded reports are complete and their normalised forms are stable.
#[test]
fn reference_cli_reports_recorded() {
    let cases = reference_cases();
    assert_eq!(cases.len(), 10);

    for (board, _) in &cases {
        let dir = crate_dir().join("tests/golden/reports");
        let text = std::fs::read_to_string(dir.join(format!("{board}.rpt"))).unwrap();
        let json = std::fs::read_to_string(dir.join(format!("{board}.json"))).unwrap();
        assert!(text.ends_with("** End of Report **\n"), "{board}");
        assert_eq!(
            normalise_text_report(&text).last().map(String::as_str),
            Some("** End of Report **")
        );
        assert!(normalise_json_report(&json).contains("\"kicad_version\": \"10.99.0\""));
    }
}

/// Comparison of the Rust DRC (engine + report writers) with `kicad-cli pcb drc` on the
/// reference boards (normalised, see [`normalise_text_report`]). Needs the DRC engine (DRC-03
/// and later) and the CLI flow (DRC-12): until then the run function is pending.
#[test]
#[ignore = "needs the DRC engine (DRC-03..) and the CLI flow (DRC-12)"]
fn reference_cli_reports() {
    let dir = crate_dir().join("tests/golden/reports");

    for (board, options) in reference_cases() {
        let want_text = std::fs::read_to_string(dir.join(format!("{board}.rpt"))).unwrap();
        let want_json = std::fs::read_to_string(dir.join(format!("{board}.json"))).unwrap();
        let (got_text, got_json) = run_rust_drc(&board, &options);
        assert_eq!(
            normalise_text_report(&got_text),
            normalise_text_report(&want_text),
            "{board} text report"
        );
        assert_eq!(
            normalise_json_report(&got_json),
            normalise_json_report(&want_json),
            "{board} json report"
        );
    }
}

/// The Rust counterpart of the recorded `kicad-cli pcb drc` runs: load
/// `qa/data/pcbnew/<board>.kicad_pcb` with its project, run the engine with the options, write
/// both reports.
fn run_rust_drc(board: &str, options: &[String]) -> (String, String) {
    kicad_drc::port_pending!(
        "kicad-cli pcb drc flow (DRC-12) for the reference reports",
        board,
        options
    )
}
