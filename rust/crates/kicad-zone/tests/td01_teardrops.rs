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

//! Differential test of `TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, true )`
//! (package TD-01) against the reference kicad-cli.
//!
//! `kicad-cli pcb drc --refill-zones --save-board` regenerates every teardrop before the fill
//! (`ZONE_FILLER_TOOL::FillAllZones`); the saved teardrop zones (type, layer, net, priority,
//! outline vertices) are the ground truth. Teardrop zones are saved without a UUID, so they
//! are matched by position among the teardrop zones of `Zones()`.
//!
//! * `teardrop_golden` (default run): replays `tests/golden/td01_teardrops.txt` (made from the
//!   reference by `teardrop_corpus` with `TD01_WRITE_GOLDEN=1`) on the `qa/data` inputs.
//! * `teardrop_corpus` (`#[ignore]`, needs `tools/zone-corpus/run.sh --refs-only <boards>`):
//!   compares against the cached reference saves in `target/zone-corpus/`; report in
//!   `target/zone-corpus/td01_report.txt`.

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kicad_connectivity::data::ConnectivityData;
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::board_layer_choices;
use kicad_pcb::Board;
use kicad_pcb::board_item::board_item_ptr_cmp;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::project::board_design_settings_json::DrcExclusionCodec;
use kicad_pcb::project::board_design_settings_json::RawDrcExclusionCodec;
use kicad_pcb::project::{LoadPath, LoadStep, StepOutcome, load_project, run_model_step};

/// The teardrop boards of `qa/data` (every board under 500 KB that needs teardrop
/// regeneration), as cached by `tools/zone-corpus/run.sh`.
const BOARDS: &[&str] = &[
    "pcbnew/api_kitchen_sink.kicad_pcb",
    "pcbnew/ipc2581/round_unequal_drill.kicad_pcb",
    "pcbnew/issue18142.kicad_pcb",
    "pcbnew/issue19405_roundrect_teardrop.kicad_pcb",
    "pcbnew/issue24649.kicad_pcb",
    "pcbnew/issue25580.kicad_pcb",
    "pcbnew/large_circle_teardrop.kicad_pcb",
    "pcbnew/odbpp/custom_pad_mask_split.kicad_pcb",
    "pcbnew/odbpp/pad_margin_shapes.kicad_pcb",
    "pcbnew/off_center_teardrop.kicad_pcb",
    "pcbnew/oval_teardrop.kicad_pcb",
    "pcbnew/padstacks.kicad_pcb",
    "pcbnew/teardrop_arc_anchor.kicad_pcb",
    "pcbnew/teardrop_close_via.kicad_pcb",
    "pcbnew/teardrop_close_via_rotated_pad.kicad_pcb",
    "pcbnew/teardrop_custom_pad_anchor.kicad_pcb",
    "pcbnew/teardrop_elongated_pad.kicad_pcb",
    "pcbnew/teardrop_issue_JPC2.kicad_pcb",
    "pcbnew/teardrop_multi_inside_via.kicad_pcb",
    "pcbnew/teardrop_offcenter_two_segment.kicad_pcb",
    "pcbnew/teardrop_qfn_thermal_pad_zone.kicad_pcb",
    "pcbnew/teardrop_spike.kicad_pcb",
    "pcbnew/two_segment_teardrop.kicad_pcb",
    // Track-to-track teardrops enabled in the project (`td_ontrackend`).
    "pcbnew/issue17967/issue17967.kicad_pcb",
    "pcbnew/issue12609.kicad_pcb",
];

/// Teardrop parameters of the synthetic boards (straight edges, as the C++ tests set).
const TD_PARAMS: &str = "(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) \
     (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments yes) \
     (prefer_zone_connections yes))";

/// The board of `addCrossingTrack` (`test_zone_filler.cpp`): a 0.3 mm F.Cu track from (0, 0)
/// to (0, 20) mm on net N1, plus `item`.
fn crossing_board(item: &str) -> String {
    format!(
        r#"(kicad_pcb (version 20260206) (generator "pcbnew") (generator_version "10.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user)
    (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "N1")
  (segment (start 0 0) (end 0 20) (width 0.3) (layer "F.Cu") (net "N1")
    (uuid "dddddddd-0000-0000-0000-000000000001"))
  {item}
)"#
    )
}

/// The synthetic boards: (name, board text). `crossing_via` / `crossing_pad` are the boards
/// of `CrossingTrackTeardropOnViaStaysOnTrack` / `CrossingTrackTeardropOnPadStaysOnTrack`
/// (the item at `crossedPosition` (0.1, 10) mm, 0.1 mm beside the track axis).
fn synthetic_board(name: &str) -> Option<String> {
    match name {
        "crossing_via" => Some(crossing_board(&format!(
            r#"(via (at 0.1 10) (size 1) (drill 0.3) (layers "F.Cu" "B.Cu") (net "N1")
    {TD_PARAMS} (uuid "cccccccc-0000-0000-0000-000000000001"))"#
        ))),
        "crossing_pad" => Some(crossing_board(&format!(
            r#"(footprint "X" (layer "F.Cu") (at 0 0) (uuid "eeeeeeee-0000-0000-0000-000000000001")
    (pad "1" thru_hole circle (at 0.1 10) (size 1 1) (drill 0.3) (layers "*.Cu") (net "N1")
      {TD_PARAMS} (uuid "ffffffff-0000-0000-0000-000000000001")))"#
        ))),
        _ => None,
    }
}

/// The synthetic board names (variant `synthetic`).
const SYNTHETIC: &[&str] = &["crossing_via", "crossing_pad"];

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_string())
}

/// `BOARD_LOADER::Load` as kicad-cli runs it, with the DRC engine and connectivity of the
/// session steps (as `zone_fill_corpus.rs`).
fn load_cli(path: &Path) -> Result<(Board, DrcEngine, ConnectivityData), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let name = path.to_str().unwrap();
    let mut board =
        parse_board(&text, name, &ParseOptions::default()).map_err(|e| format!("{e:?}"))?;
    board.file_name = name.to_string();
    kicad_drc::component_class_rules::install(&mut board);

    let pro_path = path.with_extension("kicad_pro");
    let pro = std::fs::read_to_string(&pro_path).ok();
    let project = load_project(pro.as_deref(), None, pro_path.to_str().unwrap());
    let rules_path = path.with_extension("kicad_dru");
    let rules = std::fs::read_to_string(&rules_path).ok();

    let mut engine = DrcEngine::new();
    let mut connectivity = ConnectivityData::new();
    let codec: Rc<dyn DrcExclusionCodec> = Rc::new(RawDrcExclusionCodec);
    let mut project = Some(project);

    for &step in LoadPath::Cli.steps() {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            run_model_step(&mut board, LoadPath::Cli, step, &mut project, &codec)
        }));

        match outcome {
            Ok(Ok(StepOutcome::Session)) => match step {
                LoadStep::InitDrcEngine => {
                    let layer_choices = board_layer_choices(&board);
                    let _ = engine.init_engine(
                        &mut board,
                        rules.as_deref().map(|t| (t, rules_path.to_str().unwrap())),
                        layer_choices,
                        &TuningProfiles::default(),
                        None,
                    );
                }
                LoadStep::BuildConnectivity => {
                    connectivity.build_connectivity(&mut board);
                }
                _ => {}
            },
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(format!("{e:?}")),
            // A model step pending in another package: skipped, as zone_fill_corpus.rs does.
            Err(_) => {}
        }
    }

    Ok((board, engine, connectivity))
}

/// The teardrop zones of a board in the writer's save order (`std::set<BOARD_ITEM*,
/// BOARD_ITEM::ptr_cmp>`: type, layer set, UUID), one line each:
/// `type|layer|net|priority|x,y;x,y;...`. A reloaded reference is already in that order
/// (and its teardrops got fresh UUIDs: `sort` false), the port's `Zones()` is in creation
/// order (`sort` true).
fn teardrop_lines(board: &Board, sort: bool) -> Vec<String> {
    let ctx = BoardCtx::bare(board);
    let mut lines = Vec::new();
    let mut zones: Vec<ZoneId> = board
        .zones()
        .filter(|(_, zone)| zone.is_teardrop_area())
        .map(|(id, _)| id)
        .collect();

    if sort {
        zones.sort_by(|a, b| board_item_ptr_cmp(&ctx, (*a).into(), (*b).into()));
    }

    for id in zones {
        let zone = board.zone(id).unwrap();

        let netname = ctx.item(id.into()).map_or("", |r| r.netname()).to_string();
        let outline = zone.library_outline();
        let mut pts = String::new();

        for oo in 0..outline.outline_count() {
            if oo > 0 {
                pts.push('|');
            }

            let chain = outline.c_outline(oo);

            for ii in 0..chain.point_count() {
                let p = chain.c_point(ii);

                if ii > 0 {
                    pts.push(';');
                }

                write!(pts, "{},{}", p.x, p.y).unwrap();
            }
        }

        lines.push(format!(
            "{:?}|{}|{}|{}|{}",
            zone.teardrop_type,
            zone.first_layer().0,
            netname,
            zone.assigned_priority(),
            pts
        ));
    }

    lines
}

/// Loads `input` like kicad-cli and regenerates the teardrops with the port.
fn port_lines(input: &Path) -> Result<Vec<String>, String> {
    let loaded = catch_unwind(AssertUnwindSafe(|| load_cli(input)));
    let (mut board, engine, mut connectivity) = match loaded {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => return Err(format!("load error: {e}")),
        Err(p) => return Err(format!("load panic: {}", panic_text(p))),
    };

    let run = catch_unwind(AssertUnwindSafe(|| {
        kicad_zone::tool::update_teardrops(&mut board, &mut connectivity, &engine, true);
    }));

    if let Err(p) = run {
        return Err(format!("update panic: {}", panic_text(p)));
    }

    Ok(teardrop_lines(&board, true))
}

/// Compares the port's teardrop lines with the reference; (identical, report text).
fn compare(reference: &[String], port: &[String]) -> (usize, String) {
    let mut identical = 0;
    let mut text = String::new();

    if reference.len() != port.len() {
        writeln!(
            text,
            "  teardrop count {} (reference) vs {} (port)",
            reference.len(),
            port.len()
        )
        .unwrap();
    }

    for (ii, (r, p)) in reference.iter().zip(port.iter()).enumerate() {
        if r == p {
            identical += 1;
        } else {
            let in_set = reference.contains(p);
            writeln!(
                text,
                "  #{ii} differs{}:\n    ref  {r}\n    port {p}",
                if in_set {
                    " (port line matches another reference teardrop: order)"
                } else {
                    ""
                }
            )
            .unwrap();
        }
    }

    (identical, text)
}

fn corpus_name(board: &str) -> String {
    board.trim_end_matches(".kicad_pcb").replace('/', "__")
}

/// Parameter variants applied to the corpus boards to reach more of the code (curved /
/// straight sides, clipping by the max width, two-segment teardrops, pads in zones,
/// track-to-track teardrops). `base` is the board as it is.
const VARIANTS: &[&str] = &["base", "curved", "sized", "trackend", "mask", "wide"];

/// `mask` variant: every top-level segment / arc on an outer copper layer gets
/// `(layers "F.Cu" "F.Mask") (solder_mask_margin 0.05)` (has_solder_mask + a local margin), so
/// teardrops get their
/// mask siblings (`createTeardropMask`).
fn add_track_masks(board: &str) -> String {
    let mut out = String::with_capacity(board.len());
    let mut in_track = false;

    for line in board.split_inclusive('\n') {
        if line.starts_with("\t(segment") || line.starts_with("\t(arc") {
            in_track = true;
        } else if line.starts_with("\t(") || line.starts_with("\t)") || line.starts_with(')') {
            in_track = line.starts_with("\t\t") && in_track;
        }

        let side = match line.trim() {
            "(layer \"F.Cu\")" if in_track => Some("F"),
            "(layer \"B.Cu\")" if in_track => Some("B"),
            _ => None,
        };

        match side {
            // parseLayersForCuItemWithSoldermask: the mask layer sets has_solder_mask.
            Some(side) => {
                out.push_str(&format!("\t\t(layers \"{side}.Cu\" \"{side}.Mask\")\n"));
                out.push_str("\t\t(solder_mask_margin 0.05)\n");
            }
            None => out.push_str(line),
        }
    }

    out
}

/// Every (variant, boards) pair: the corpus boards in every parameter variant, then the
/// synthetic boards.
fn variant_boards() -> Vec<(&'static str, &'static [&'static str])> {
    let mut list: Vec<(&'static str, &'static [&'static str])> =
        VARIANTS.iter().map(|v| (*v, BOARDS)).collect();
    list.push(("synthetic", SYNTHETIC));
    list
}

/// Replaces the value of every `(token value)` in `text`.
fn set_token(text: &str, token: &str, value: impl Fn(&str) -> String) -> String {
    let key = format!("({token} ");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(pos) = rest.find(&key) {
        let after = &rest[pos + key.len()..];
        let Some(end) = after.find(')') else {
            break;
        };

        out.push_str(&rest[..pos + key.len()]);
        out.push_str(&value(&after[..end]));
        rest = &after[end..];
    }

    out.push_str(rest);
    out
}

fn flip(v: &str) -> String {
    if v == "yes" { "no" } else { "yes" }.to_string()
}

/// The board and project texts of a variant.
fn apply_variant(variant: &str, board: &str, pro: Option<&str>) -> (String, Option<String>) {
    match variant {
        "curved" => (
            set_token(board, "curved_edges", flip),
            pro.map(str::to_string),
        ),
        "sized" => {
            let mut b = set_token(board, "best_length_ratio", |_| "0.8".to_string());
            b = set_token(&b, "max_length", |_| "0.5".to_string());
            b = set_token(&b, "best_width_ratio", |_| "0.7".to_string());
            b = set_token(&b, "max_width", |_| "0.6".to_string());
            b = set_token(&b, "allow_two_segments", flip);
            b = set_token(&b, "prefer_zone_connections", flip);
            (b, pro.map(str::to_string))
        }
        "mask" => (add_track_masks(board), pro.map(str::to_string)),
        // No max width / length: the teardrops run into neighbouring copper and go through
        // the clearance fit (collidesWithOtherNets, the width bisection).
        "wide" => {
            let mut b = set_token(board, "max_length", |_| "0".to_string());
            b = set_token(&b, "max_width", |_| "0".to_string());
            b = set_token(&b, "best_length_ratio", |_| "1.2".to_string());
            b = set_token(&b, "best_width_ratio", |_| "1".to_string());
            b = set_token(&b, "filter_ratio", |_| "0.95".to_string());
            (b, pro.map(str::to_string))
        }
        "trackend" => (
            board.to_string(),
            pro.map(|p| p.replace("\"td_ontrackend\": false", "\"td_ontrackend\": true")),
        ),
        _ => (board.to_string(), pro.map(str::to_string)),
    }
}

/// Writes the variant input of a `qa/data` board (board, `.kicad_pro`, `.kicad_dru`) into
/// `dir`; returns the board path.
fn write_variant(variant: &str, board: &str, dir: &Path) -> Option<PathBuf> {
    if variant == "synthetic" {
        std::fs::create_dir_all(dir).ok()?;
        let out = dir.join(format!("{board}.kicad_pcb"));
        std::fs::write(&out, synthetic_board(board)?).ok()?;
        return Some(out);
    }

    let src = kicad_qa::qa_data_dir().join(board);
    let text = std::fs::read_to_string(&src).ok()?;
    let pro = std::fs::read_to_string(src.with_extension("kicad_pro")).ok();
    let (text, pro) = apply_variant(variant, &text, pro.as_deref());

    std::fs::create_dir_all(dir).ok()?;
    let out = dir.join(format!("{}.kicad_pcb", corpus_name(board)));
    std::fs::write(&out, text).ok()?;

    if let Some(pro) = pro {
        std::fs::write(out.with_extension("kicad_pro"), pro).ok()?;
    }

    if let Ok(dru) = std::fs::read_to_string(src.with_extension("kicad_dru")) {
        std::fs::write(out.with_extension("kicad_dru"), dru).ok()?;
    }

    Some(out)
}

/// Boards whose variant equals the base board are skipped (nothing new to compare).
fn variant_differs(variant: &str, board: &str) -> bool {
    if variant == "synthetic" {
        return synthetic_board(board).is_some();
    }

    if variant == "base" {
        return true;
    }

    let src = kicad_qa::qa_data_dir().join(board);
    let text = std::fs::read_to_string(&src).unwrap_or_default();
    let pro = std::fs::read_to_string(src.with_extension("kicad_pro")).ok();
    let (vtext, vpro) = apply_variant(variant, &text, pro.as_deref());

    vtext != text || vpro != pro
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/td01_teardrops.txt")
}

/// Reads the golden file: (variant, board, teardrop lines).
fn read_golden() -> Vec<(String, String, Vec<String>)> {
    let Ok(text) = std::fs::read_to_string(golden_path()) else {
        return Vec::new();
    };

    let mut boards: Vec<(String, String, Vec<String>)> = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("BOARD ") {
            let (variant, name) = rest.split_once(' ').unwrap_or(("base", rest));
            boards.push((variant.to_string(), name.to_string(), Vec::new()));
        } else if let Some(td) = line.strip_prefix("TD ") {
            if let Some(last) = boards.last_mut() {
                last.2.push(td.to_string());
            }
        }
    }

    boards
}

/// The variant directory of the oracle inputs (`TD01_WRITE_VARIANTS=1`).
fn variants_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/td01-variants")
}

/// The zone-corpus cache directory of a (variant, board).
fn cache_dir(corpus: &Path, variant: &str, board: &str) -> PathBuf {
    if variant == "base" {
        corpus.join(corpus_name(board))
    } else if variant == "synthetic" {
        corpus.join(format!("rust__target__td01-variants__synthetic__{board}"))
    } else {
        corpus.join(format!(
            "rust__target__td01-variants__{variant}__{}",
            corpus_name(board)
        ))
    }
}

#[test]
#[ignore = "needs the zone-corpus reference cache (tools/zone-corpus/run.sh --refs-only)"]
fn teardrop_corpus() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zone-corpus");

    // TD01_WRITE_VARIANTS=1: write the variant inputs for tools/zone-corpus/run.sh and list
    // them in target/td01-variants/boards.txt.
    if std::env::var("TD01_WRITE_VARIANTS").is_ok_and(|v| v == "1") {
        let mut list = String::new();

        for (variant, boards) in variant_boards().iter().filter(|(v, _)| *v != "base") {
            for board in boards {
                if variant_differs(variant, board) {
                    if let Some(path) = write_variant(variant, board, &variants_dir().join(variant))
                    {
                        writeln!(list, "{}", path.canonicalize().unwrap().display()).unwrap();
                    }
                }
            }
        }

        std::fs::write(variants_dir().join("boards.txt"), list).unwrap();
        return;
    }

    let mut report = String::new();
    let mut golden = String::new();
    let (mut total, mut same) = (0, 0);

    for (variant, boards) in variant_boards() {
        for board in boards {
            if !variant_differs(variant, board) {
                continue;
            }

            let dir = cache_dir(&corpus, variant, board);
            let label = format!("{variant} {board}");

            let Some(reference) = std::fs::read_dir(dir.join("ref")).ok().and_then(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .find(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
            }) else {
                writeln!(report, "{label}: no reference").unwrap();
                continue;
            };
            let input = dir.join("in").join(reference.file_name().unwrap());

            let ref_text = std::fs::read_to_string(&reference).unwrap_or_default();
            let ref_board = match parse_board(&ref_text, "ref.kicad_pcb", &ParseOptions::default())
            {
                Ok(b) => b,
                Err(e) => {
                    writeln!(report, "{label}: reference parse error {e:?}").unwrap();
                    continue;
                }
            };

            let ref_lines = teardrop_lines(&ref_board, false);

            writeln!(golden, "BOARD {label}").unwrap();

            for line in &ref_lines {
                writeln!(golden, "TD {line}").unwrap();
            }

            total += ref_lines.len();

            match port_lines(&input) {
                Ok(port) => {
                    let (identical, text) = compare(&ref_lines, &port);
                    same += identical;
                    writeln!(
                        report,
                        "{label}: {identical} / {} teardrops identical{}",
                        ref_lines.len(),
                        if port.len() != ref_lines.len() {
                            format!(" (port made {})", port.len())
                        } else {
                            String::new()
                        }
                    )
                    .unwrap();
                    report.push_str(&text);
                }
                Err(e) => {
                    writeln!(report, "{label}: {e}").unwrap();
                }
            }
        }
    }

    writeln!(report, "TOTAL: {same} / {total} teardrops identical").unwrap();
    println!("{report}");

    let _ = std::fs::write(corpus.join("td01_report.txt"), &report);

    if std::env::var("TD01_WRITE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(golden_path().parent().unwrap()).unwrap();
        std::fs::write(golden_path(), golden).unwrap();
    }
}

#[test]
fn teardrop_golden() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("td01");
    let mut failures = String::new();

    for (variant, board, ref_lines) in read_golden() {
        if variant != "synthetic" && !kicad_qa::qa_data_dir().join(&board).exists() {
            continue;
        }

        let Some(input) = write_variant(&variant, &board, &scratch.join(&variant)) else {
            writeln!(failures, "{variant} {board}: cannot write the input").unwrap();
            continue;
        };

        match port_lines(&input) {
            Ok(port) => {
                let (identical, text) = compare(&ref_lines, &port);

                if identical != ref_lines.len() || port.len() != ref_lines.len() {
                    writeln!(failures, "{variant} {board}:\n{text}").unwrap();
                }
            }
            Err(e) => writeln!(failures, "{variant} {board}: {e}").unwrap(),
        }
    }

    assert!(
        failures.is_empty(),
        "teardrops differ from the reference:\n{failures}"
    );
}

/// Port of `checkCrossingTrackTeardropsStayOnCopper` (`test_zone_filler.cpp`): two
/// teardrops, every corner on the track or on the crossed item.
fn check_crossing_track_teardrops_stay_on_copper(name: &str) {
    let mut board = parse_board(
        &synthetic_board(name).unwrap(),
        "t.kicad_pcb",
        &ParseOptions::default(),
    )
    .unwrap();
    let mut engine = DrcEngine::new();
    engine
        .init_engine(
            &mut board,
            None,
            kicad_drc::rule_parser::default_layer_choices(),
            &TuningProfiles::default(),
            None,
        )
        .expect("implicit rules");
    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(&mut board);

    let crossed: kicad_pcb::ids::ItemId = {
        let mut found = None;

        for (id, track) in board.tracks() {
            if matches!(track, kicad_pcb::pcb_track::Track::Via(_)) {
                found = Some(id.into());
            }
        }

        found.unwrap_or_else(|| board.get_pads()[0].into())
    };
    let track: kicad_pcb::ids::ItemId = board
        .tracks()
        .find(|(_, t)| matches!(t, kicad_pcb::pcb_track::Track::Trace(_)))
        .map(|(id, _)| id.into())
        .unwrap();

    kicad_zone::tool::update_teardrops(&mut board, &mut connectivity, &engine, true);

    // A straight edged teardrop has five corners. Two sit on the track, two on the crossed
    // item and one behind its centre. The pad hit test uses an inscribed polygon, so allow its
    // error.
    let tolerance = board
        .design_settings()
        .max_error()
        .max(kicad_core::PCB_IU_SCALE.mm_to_iu(0.001));
    let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);
    let mut teardrops = 0;

    for (_, zone) in board.zones() {
        if !zone.is_teardrop_area() {
            continue;
        }

        teardrops += 1;

        for &pt in zone.library_outline().c_outline(0).c_points() {
            let on_track = ctx
                .item(track)
                .unwrap()
                .as_dyn()
                .hit_test(&ctx, pt, tolerance);
            let on_crossed = ctx
                .item(crossed)
                .unwrap()
                .as_dyn()
                .hit_test(&ctx, pt, tolerance);

            assert!(
                on_track || on_crossed,
                "Teardrop corner {pt:?} lies outside both the track and the crossed item"
            );
        }
    }

    assert_eq!(teardrops, 2);
}

/// Port of `CrossingTrackTeardropOnViaStaysOnTrack` (`qa/tests/pcbnew/test_zone_filler.cpp`).
#[test]
fn crossing_track_teardrop_on_via_stays_on_track() {
    check_crossing_track_teardrops_stay_on_copper("crossing_via");
}

/// Port of `CrossingTrackTeardropOnPadStaysOnTrack` (`qa/tests/pcbnew/test_zone_filler.cpp`).
#[test]
fn crossing_track_teardrop_on_pad_stays_on_track() {
    check_crossing_track_teardrops_stay_on_copper("crossing_pad");
}

/// Port of `NoTeardropOnZoneConnectedThermalPad` (`qa/tests/pcbnew/test_teardrop_shorts.cpp`):
/// the QFN's exposed pad, held by the GND pour through thermal spokes, gets no teardrop
/// although its centre falls in the thermal gap punched for the via underneath it. The C++
/// fills the zones first (`KI_TEST::FillZones` + `BuildConnectivity`), then updates the
/// teardrops.
#[test]
fn no_teardrop_on_zone_connected_thermal_pad() {
    let input = kicad_qa::pcbnew_data_dir().join("teardrop_qfn_thermal_pad_zone.kicad_pcb");
    let (mut board, engine, mut connectivity) = load_cli(&input).unwrap();

    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();
    kicad_zone::ZoneFiller::new(&board).fill(&mut board, &mut connectivity, &engine, &zones);
    connectivity.build_connectivity(&mut board);

    kicad_zone::tool::update_teardrops(&mut board, &mut connectivity, &engine, true);

    let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);
    let mut exposed_pad = None;

    for (fp_id, fp) in board.footprints() {
        for (_, pad) in board.footprint_pads(fp_id) {
            // The footprint splits the exposed pad into eleven sub-pads all numbered 33.
            if fp.reference(&ctx) == "U2"
                && pad.number == "33"
                && pad.size(&ctx, kicad_core::PcbLayerId::FCu).x
                    > kicad_core::PCB_IU_SCALE.mm_to_iu(3.0)
            {
                exposed_pad = Some(pad);
            }
        }
    }

    let exposed_pad = exposed_pad.expect("exposed pad");
    let mut pad_poly = kicad_geom::ShapePolySet::new();
    exposed_pad.transform_shape_to_polygon(
        &ctx,
        &mut pad_poly,
        kicad_core::PcbLayerId::FCu,
        0,
        board.design_settings().max_error(),
        kicad_geom::geometry::approximation::ErrorLoc::Inside,
        false,
    );

    let mut teardrops_on_pad = 0;

    for (_, zone) in board.zones() {
        if !zone.is_teardrop_area() || zone.first_layer() != kicad_core::PcbLayerId::FCu {
            continue;
        }

        let mut overlap = zone.library_outline().clone();
        overlap.boolean_intersection(&pad_poly);

        if !overlap.is_empty() {
            teardrops_on_pad += 1;
        }
    }

    assert_eq!(teardrops_on_pad, 0);
}

/// Ports of `qa/tests/pcbnew/test_teardrop_uuid.cpp` (`KIID::Combine` / `Increment` as the
/// teardrop UUIDs use them).
mod teardrop_uuid {
    use kicad_core::{Kiid, KiidGenerator};
    use kicad_zone::teardrop::TeardropManager;

    #[test]
    fn combine_track_and_pad_uuids() {
        let mut generator = KiidGenerator::seeded(1);
        let track_uuid = Kiid::new(&mut generator);
        let pad_uuid = Kiid::new(&mut generator);

        let teardrop_uuid = TeardropManager::teardrop_uuid(&track_uuid, &pad_uuid, 0);

        // The combined UUID should be different from both inputs
        assert_ne!(teardrop_uuid, track_uuid);
        assert_ne!(teardrop_uuid, pad_uuid);

        // The same inputs should always produce the same output
        let teardrop_uuid2 = Kiid::combine(&track_uuid, &pad_uuid);
        assert_eq!(teardrop_uuid.as_string(), teardrop_uuid2.as_string());
    }

    #[test]
    fn mask_uuid_is_incremented_from_teardrop_uuid() {
        let mut generator = KiidGenerator::seeded(2);
        let track_uuid = Kiid::new(&mut generator);
        let pad_uuid = Kiid::new(&mut generator);

        let teardrop_uuid = Kiid::combine(&track_uuid, &pad_uuid);
        let mask_uuid = TeardropManager::mask_uuid_for(&teardrop_uuid);

        // The mask UUID should be different from the teardrop UUID
        assert_ne!(mask_uuid, teardrop_uuid);

        // Both should be deterministic
        let teardrop_uuid2 = Kiid::combine(&track_uuid, &pad_uuid);
        let mut mask_uuid2 = Kiid::combine(&track_uuid, &pad_uuid);
        mask_uuid2.increment();

        assert_eq!(teardrop_uuid.as_string(), teardrop_uuid2.as_string());
        assert_eq!(mask_uuid.as_string(), mask_uuid2.as_string());
    }

    #[test]
    fn different_tracks_produce_different_teardrop_uuids() {
        let mut generator = KiidGenerator::seeded(3);
        let track1_uuid = Kiid::new(&mut generator);
        let track2_uuid = Kiid::new(&mut generator);
        let pad_uuid = Kiid::new(&mut generator);

        let teardrop1_uuid = Kiid::combine(&track1_uuid, &pad_uuid);
        let teardrop2_uuid = Kiid::combine(&track2_uuid, &pad_uuid);

        // Different tracks should produce different teardrops
        assert_ne!(teardrop1_uuid, teardrop2_uuid);

        // Each should still be deterministic
        assert_eq!(
            teardrop1_uuid.as_string(),
            Kiid::combine(&track1_uuid, &pad_uuid).as_string()
        );
        assert_eq!(
            teardrop2_uuid.as_string(),
            Kiid::combine(&track2_uuid, &pad_uuid).as_string()
        );
    }

    #[test]
    fn stable_across_regenerations() {
        let track_uuid = Kiid::parse("12345678-1234-1234-1234-123456789012").unwrap();
        let pad_uuid = Kiid::parse("abcdef01-abcd-abcd-abcd-abcdef012345").unwrap();

        // First generation, then regeneration: identical.
        let teardrop_uuid1 = TeardropManager::teardrop_uuid(&track_uuid, &pad_uuid, 0);
        let teardrop_uuid2 = TeardropManager::teardrop_uuid(&track_uuid, &pad_uuid, 0);

        assert_eq!(teardrop_uuid1.as_string(), teardrop_uuid2.as_string());

        // A slot spans two UUIDs (the copper and its mask).
        let mut expected = teardrop_uuid1;
        expected.increment();
        expected.increment();
        assert_eq!(
            TeardropManager::teardrop_uuid(&track_uuid, &pad_uuid, 1),
            expected
        );
    }
}
