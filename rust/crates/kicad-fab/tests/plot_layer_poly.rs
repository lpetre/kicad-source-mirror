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

//! FAB-10 verification (`plot_layer_poly.rs`: `GenerateLayerPoly`, `PlotSolderMaskLayer` with
//! a minimum width, `PlotClippedSilkLayer`, `PlotPolySet`) against the reference
//! `kicad-cli pcb export gerbers` (goldens `tests/golden/plot_layer_poly/{crafted,corpus}.txt`,
//! `generate.py`):
//!
//! * `crafted_boards`: the boards of `tests/data/plot_layer_poly` (pads closer than the
//!   minimum width, untented vias, mask tracks / arcs / text / text box / dashed lines, a
//!   filled zone on F.Mask, silk over mask openings) with all layers, with the mask layers as
//!   common layers (`--cl F.Mask,B.Mask`), and the two `--subtract-soldermask` sets below.
//! * `corpus_replay`: every board under `qa/data` and `demos` below 300 KB with
//!   `--subtract-soldermask -l F.SilkS,B.SilkS` (silk primary: the negative mask, through
//!   `PlotSolderMaskLayer`) and `--subtract-soldermask -l F.Cu,B.Cu --cl F.SilkS,B.SilkS`
//!   (silk not primary: `PlotClippedSilkLayer`). Boards that do not load or reach a pending
//!   stub are counted, not failed.
//!
//! The job is replayed as `JobExportGerbers` runs it: per layer of `-l` (in UI order, enabled
//! layers only) one file with the plot sequence `[layer] + --cl`, the board loaded with its
//! project, connectivity and the CLI-style DRC engine as rule resolver.
//! `KICAD_FAB_LAYER_POLY_DUMP=<dir>` writes the port's differing files there,
//! `KICAD_FAB_LAYER_POLY_REPORT=1` prints the per-run result.

mod plot_common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kicad_core::{Lset, PcbLayerId};
use kicad_fab::plot_board_layers::plot_gerber_layer_file;
use kicad_pcb::ctx::BoardCtx;
use plot_common::*;

/// One `B` block of a golden.
struct Case {
    set: String,
    path: String,
    /// `(file name, hash)` in "Plotted to" order.
    files: Vec<(String, String)>,
}

fn cases(name: &str) -> Vec<Case> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/plot_layer_poly")
            .join(name),
    )
    .unwrap();
    let mut cases: Vec<Case> = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("B ") {
            let (set, path) = rest.split_once(' ').unwrap();
            cases.push(Case {
                set: set.to_string(),
                path: path.to_string(),
                files: Vec::new(),
            });
        } else {
            let (file, hash) = line.rsplit_once(' ').unwrap();
            cases
                .last_mut()
                .unwrap()
                .files
                .push((file.to_string(), hash.to_string()));
        }
    }

    cases
}

fn layer(name: &str) -> PcbLayerId {
    match name {
        "F.Cu" => PcbLayerId::FCu,
        "B.Cu" => PcbLayerId::BCu,
        "F.Mask" => PcbLayerId::FMask,
        "B.Mask" => PcbLayerId::BMask,
        "F.SilkS" => PcbLayerId::FSilkS,
        "B.SilkS" => PcbLayerId::BSilkS,
        "Edge.Cuts" => PcbLayerId::EdgeCuts,
        _ => panic!("layer {name}"),
    }
}

/// `(-l, --cl, --subtract-soldermask)` of a set of `generate.py`.
fn set_args(set: &str) -> (&'static str, &'static str, bool) {
    match set {
        "default" => ("F.Cu,B.Cu,F.Mask,B.Mask,F.SilkS,B.SilkS", "", false),
        "mask_cl" => ("F.Cu,Edge.Cuts", "F.Mask,B.Mask", false),
        "sub" => ("F.SilkS,B.SilkS", "", true),
        "sub_cl" => ("F.Cu,B.Cu", "F.SilkS,B.SilkS", true),
        _ => panic!("unknown set {set}"),
    }
}

fn layers(list: &str) -> Vec<PcbLayerId> {
    list.split(',')
        .filter(|s| !s.is_empty())
        .map(layer)
        .collect()
}

#[derive(Default)]
struct Score {
    runs: usize,
    not_loadable: usize,
    pending: usize,
    runs_matched: usize,
    files_matched: usize,
    failures: Vec<String>,
}

fn replay(golden: &str, score: &mut Score) {
    let report = std::env::var_os("KICAD_FAB_LAYER_POLY_REPORT").is_some();
    let root = kicad_qa::kicad_source_root();

    // The pending stubs panic by design; keep the output readable.
    std::panic::set_hook(Box::new(|_| {}));

    let all = cases(golden);
    let mut i = 0;

    while i < all.len() {
        // The sets of a board are adjacent: load it once.
        let path_rel = all[i].path.clone();
        let mut j = i;
        while j < all.len() && all[j].path == path_rel {
            j += 1;
        }
        let board_cases = &all[i..j];
        i = j;
        score.runs += board_cases.len();

        if board_cases.iter().all(|c| c.files.is_empty()) {
            score.runs_matched += board_cases.len();
            continue;
        }

        let path = root.join(&path_rel);
        let mut board = match catch_unwind(|| load_board(&path)) {
            Ok(Ok(board)) => board,
            _ => {
                score.not_loadable += board_cases.len();
                if report {
                    println!("{path_rel}: not loadable");
                }
                continue;
            }
        };
        let engine = drc_engine(&mut board, &path);
        let Some(connectivity) = build_connectivity(&mut board) else {
            score.not_loadable += board_cases.len();
            continue;
        };
        let ctx = BoardCtx::bare(&board)
            .with_connectivity(&connectivity)
            .with_rules(&engine);
        let enabled = board.enabled_layers();

        for case in board_cases {
            let (l, cl, subtract) = set_args(&case.set);
            let mut opts = cli_gerber_opts(false, 6, false, false);
            opts.subtract_mask_from_silk = subtract;
            let common = layers(cl);
            let to_plot = Lset::from_layers(&layers(l)) & enabled.clone();

            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut files = Vec::new();

                for primary in to_plot.ui_order().0 {
                    let mut sequence = vec![primary];

                    for &c in &common {
                        if !sequence.contains(&c) {
                            sequence.push(c);
                        }
                    }

                    let out =
                        plot_gerber_layer_file(&ctx, &opts, primary, &sequence, "out", &env())
                            .unwrap();
                    let name = out.name.rsplit('/').next().unwrap().to_string();
                    files.push((name, normalise(std::str::from_utf8(&out.contents).unwrap())));
                }

                files
            }));

            let Ok(files) = result else {
                score.pending += 1;
                if report {
                    println!("{path_rel} {}: pending", case.set);
                }
                continue;
            };

            let mut ok = files.len() == case.files.len();

            if !ok {
                score.failures.push(format!(
                    "{path_rel} {}: {} files != {}",
                    case.set,
                    files.len(),
                    case.files.len()
                ));
            }

            for ((name, text), (exp_name, exp_hash)) in files.iter().zip(&case.files) {
                if name == exp_name && fnv(text)[..8] == *exp_hash {
                    score.files_matched += 1;
                } else {
                    if let Some(dir) = std::env::var_os("KICAD_FAB_LAYER_POLY_DUMP") {
                        let _ = std::fs::write(
                            Path::new(&dir).join(format!("{}-{name}", case.set)),
                            text,
                        );
                    }
                    score
                        .failures
                        .push(format!("{path_rel} {} {name}: differs", case.set));
                    ok = false;
                }
            }

            if ok {
                score.runs_matched += 1;
            }

            if report {
                println!(
                    "{path_rel} {}: {}",
                    case.set,
                    if ok { "identical" } else { "DIFFERS" }
                );
            }
        }
    }
}

#[test]
fn crafted_boards() {
    let mut score = Score::default();
    replay("crafted.txt", &mut score);
    println!(
        "plot_layer_poly crafted: {} runs, {} identical, {} files identical",
        score.runs, score.runs_matched, score.files_matched
    );
    assert!(score.failures.is_empty(), "{}", score.failures.join("\n"));
    assert_eq!(
        score.runs_matched, score.runs,
        "every crafted run is compared"
    );
}

#[test]
fn corpus_replay() {
    let mut score = Score::default();
    replay("corpus.txt", &mut score);
    println!(
        "plot_layer_poly corpus: {} runs, {} not loadable, {} pending, {} identical runs, {} \
         files identical, {} failures",
        score.runs,
        score.not_loadable,
        score.pending,
        score.runs_matched,
        score.files_matched,
        score.failures.len()
    );
    assert!(score.failures.is_empty(), "{}", score.failures.join("\n"));
}
