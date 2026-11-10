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

        for variant in VARIANTS.iter().filter(|v| **v != "base") {
            for board in BOARDS {
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

    for variant in VARIANTS {
        for board in BOARDS {
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
        if !kicad_qa::qa_data_dir().join(&board).exists() {
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
