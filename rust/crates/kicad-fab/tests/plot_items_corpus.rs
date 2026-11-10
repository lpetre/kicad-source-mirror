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

//! FAB-04 verification while FAB-05 / FAB-06 are pending: the pads, vias, tracks and zone
//! fills of `PlotStandardLayer` (`plot_standard_layer_pads_vias_tracks_zones`) against the
//! whole Gerber files of the reference kicad-cli.
//!
//! `PlotStandardLayer` first runs the board graphics and footprint text / graphic loops
//! (FAB-05, FAB-06, still pending stubs that panic for every item). On a layer that carries
//! no board drawing and no footprint field or graphic item those loops plot nothing, so the
//! file is the driver (`StartPlotBoard`, `PlotOneBoardLayer` with the `kicad-cli pcb export
//! gerbers` defaults) around the FAB-04 part alone. This test plots exactly that for every
//! such layer of the loadable corpus boards (`tests/golden/plot_driver/corpus.txt`, CLI
//! defaults) and of the crafted FAB-04 boards (`tests/golden/plot_items/crafted.txt`), and
//! compares the normalised file. Layers with FAB-05/06 content, drill maps or a solder mask
//! minimum width (FAB-10) are skipped; `plot_driver_corpus.rs` covers them once those
//! packages land. `KICAD_FAB_CORPUS_REPORT=1` prints per-board results,
//! `KICAD_FAB_CORPUS_DUMP=<dir>` writes the port's output of differing files.

mod plot_common;

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_core::color4d::Color4D;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::plot_enums::DrillMarks;
use kicad_core::{Lset, PcbLayerId};
use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::gerber_protel_extension;
use kicad_fab::plot_board_layers::{
    StandardLayerFlags, plot_standard_layer_pads_vias_tracks_zones, start_plot_board,
};
use kicad_fab::plot_brditems_plotter::BrditemsPlotter;
use kicad_fab::wx_file_name::WxFileName;
use kicad_pcb::AnyItem;
use kicad_pcb::board::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use plot_common::*;

/// Why a layer is not compared.
#[derive(Debug, PartialEq)]
enum Skip {
    /// Board drawings or footprint fields / graphics on the layer (FAB-05 / FAB-06).
    Graphics,
    /// A drill map item on the board (FAB-13).
    DrillMap,
    /// Solder mask with a minimum width (FAB-10).
    MaskMinWidth,
}

/// Whether `PlotStandardLayer`'s FAB-05 / FAB-06 loops plot nothing on `layer`.
fn skip_reason(ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Option<Skip> {
    let board = ctx.board;

    if matches!(layer, PcbLayerId::FMask | PcbLayerId::BMask)
        && board.design_settings().solder_mask_min_width != 0
    {
        return Some(Skip::MaskMinWidth);
    }

    let on_layer = |id| {
        catch_unwind(AssertUnwindSafe(|| {
            ctx.item(id).is_none_or(|item| item.layer_set().contains(layer))
        }))
        .unwrap_or(true)
    };

    for (id, item) in board.drawings() {
        if matches!(item, AnyItem::DrillMap(_)) {
            return Some(Skip::DrillMap);
        }

        if on_layer(id) {
            return Some(Skip::Graphics);
        }
    }

    for (fp, _) in board.footprints() {
        for (id, _) in board.footprint_fields(fp) {
            if on_layer(id.into()) {
                return Some(Skip::Graphics);
            }
        }

        for (id, _) in board.footprint_drawings(fp) {
            if on_layer(id) {
                return Some(Skip::Graphics);
            }
        }
    }

    None
}

/// `plot_gerber_layer_file` for one layer with `PlotStandardLayer` reduced to its FAB-04 part
/// (valid for layers where `skip_reason` is `None`): `StartPlotBoard`, `PlotBoardLayers` /
/// `PlotOneBoardLayer` for a Gerber plotter and the CLI options, `EndPlot`.
fn plot_layer_items(ctx: &BoardCtx<'_>, opts: &PcbPlotParams, layer: PcbLayerId) -> String {
    let board = ctx.board;
    let layer_name = board.layer_name(layer);
    let mut fn_ = WxFileName::new(&board.file_name);
    build_plot_file_name(
        &mut fn_,
        "out",
        &layer_name,
        &gerber_protel_extension(layer.0),
    );

    let mut plotter = start_plot_board(
        ctx,
        opts,
        layer.0,
        &layer_name,
        &fn_.full_path(),
        "",
        "",
        "1",
        &env(),
    )
    .unwrap();

    // PlotBoardLayers: no drill marks on a Gerber plotter.
    let mut plot_opt = opts.clone();
    plot_opt.drill_marks = DrillMarks::NoDrillShape;

    // PlotOneBoardLayer
    plotter.set_color(&Color4D::BLACK);
    plotter.set_text_mode(plot_opt.text_mode);

    if is_copper_layer(layer) {
        plot_opt.skip_npth_pads = true;
    } else if matches!(
        layer,
        PcbLayerId::BAdhes | PcbLayerId::FAdhes | PcbLayerId::BPaste | PcbLayerId::FPaste
    ) {
        plot_opt.drill_marks = DrillMarks::NoDrillShape;
    }

    // PlotStandardLayer (subtract-mask-from-silk is off in the CLI defaults)
    let layer_mask = Lset::from_layers(&[layer]);
    {
        let mut itemplotter = BrditemsPlotter::new(plotter.as_mut(), ctx, &plot_opt);
        itemplotter.set_layer_set(&layer_mask);
        let flags = StandardLayerFlags::new(ctx, &layer_mask, &plot_opt);
        plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
    }

    plotter.end_plot();
    normalise(plotter.output())
}

#[derive(Default, Debug)]
struct Score {
    boards: usize,
    boards_compared: usize,
    files_matched: usize,
    files_pending: usize,
    skipped: Vec<(Skip, usize)>,
    failures: Vec<String>,
}

impl Score {
    fn skip(&mut self, why: Skip) {
        if let Some(entry) = self.skipped.iter_mut().find(|(w, _)| *w == why) {
            entry.1 += 1;
        } else {
            self.skipped.push((why, 1));
        }
    }
}

/// Compares every comparable layer of one board against `(file name hash, file hash)` goldens.
fn replay_board(
    label: &str,
    board: &mut Board,
    files: &[(String, String)],
    score: &mut Score,
    report: bool,
) {
    score.boards += 1;

    let Some(connectivity) = build_connectivity(board) else {
        if report {
            println!("{label}: connectivity pending");
        }
        return;
    };
    let ctx = BoardCtx::bare(board).with_connectivity(&connectivity);
    let opts = cli_gerber_opts(false, 6, false, false);
    let mut line = String::new();
    let mut compared = false;

    for (name_hash, file_hash) in files {
        let Some(layer) = board.enabled_layers().ui_order().0.into_iter().find(|&layer| {
            let mut fn_ = WxFileName::new(&board.file_name);
            build_plot_file_name(
                &mut fn_,
                "",
                &board.layer_name(layer),
                &gerber_protel_extension(layer.0),
            );
            fnv(&fn_.full_name())[..8] == *name_hash
        }) else {
            score
                .failures
                .push(format!("{label}: no layer gives the file name {name_hash}"));
            continue;
        };

        if let Some(why) = skip_reason(&ctx, layer) {
            score.skip(why);
            line.push('-');
            continue;
        }

        compared = true;

        match catch_unwind(AssertUnwindSafe(|| plot_layer_items(&ctx, &opts, layer))) {
            Ok(text) if fnv(&text)[..8] == *file_hash => {
                score.files_matched += 1;
                line.push('F');
            }
            Ok(text) => {
                if let Some(dir) = std::env::var_os("KICAD_FAB_CORPUS_DUMP") {
                    let name = format!(
                        "{}-{}.gbr",
                        label.replace('/', "_"),
                        board.layer_name(layer)
                    );
                    std::fs::write(std::path::Path::new(&dir).join(name), &text).unwrap();
                }
                score
                    .failures
                    .push(format!("{label} {}: file differs", board.layer_name(layer)));
                line.push('X');
            }
            Err(_) => {
                score.files_pending += 1;
                line.push('.');
            }
        }
    }

    if compared {
        score.boards_compared += 1;
    }

    if report {
        let pads: usize = board
            .footprints()
            .map(|(fp, _)| board.footprint_pads(fp).count())
            .sum();
        let vias = board
            .tracks()
            .filter(|(_, t)| t.as_via().is_some())
            .count();
        let arcs = board
            .tracks()
            .filter(|(_, t)| t.as_arc().is_some())
            .count();
        println!(
            "{label}: {line} (pads {pads}, vias {vias}, tracks {}, arcs {arcs}, zones {})",
            board.tracks().count() - vias - arcs,
            board.zones().count()
        );
    }
}

/// The `B <path>` / `<name> <header> <file>` golden format of `generate.py`.
fn read_golden(rel: &str) -> Vec<(String, Vec<(String, String)>)> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel),
    )
    .unwrap();
    let mut cases: Vec<(String, Vec<(String, String)>)> = Vec::new();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("B ") {
            cases.push((path.to_string(), Vec::new()));
        } else {
            let f: Vec<&str> = line.split(' ').collect();
            assert_eq!(f.len(), 3, "bad line {line:?}");
            cases
                .last_mut()
                .unwrap()
                .1
                .push((f[0].to_string(), f[2].to_string()));
        }
    }

    cases
}

fn run(golden: &str, root: &std::path::Path) -> Score {
    let report = std::env::var_os("KICAD_FAB_CORPUS_REPORT").is_some();
    let mut score = Score::default();

    for (path, files) in read_golden(golden) {
        if files.is_empty() {
            continue;
        }

        let mut board = match catch_unwind(|| load_board(&root.join(&path))) {
            Ok(Ok(board)) => board,
            _ => {
                if report {
                    println!("{path}: not loadable");
                }
                continue;
            }
        };

        replay_board(&path, &mut board, &files, &mut score, report);
    }

    score
}

fn finish(what: &str, score: &Score) {
    println!(
        "{what}: {} boards loaded, {} with compared layers: files {} matched / {} pending, \
         skipped {:?}, {} failures",
        score.boards,
        score.boards_compared,
        score.files_matched,
        score.files_pending,
        score.skipped,
        score.failures.len()
    );

    assert!(
        score.failures.is_empty(),
        "{} differences:\n{}",
        score.failures.len(),
        score.failures.join("\n")
    );
}

#[test]
fn corpus_items_replay() {
    std::panic::set_hook(Box::new(|_| {}));
    let score = run(
        "tests/golden/plot_driver/corpus.txt",
        &kicad_qa::kicad_source_root(),
    );
    let _ = std::panic::take_hook();
    finish("plot items corpus", &score);
}

#[test]
fn crafted_items_replay() {
    std::panic::set_hook(Box::new(|_| {}));
    let score = run(
        "tests/golden/plot_items/crafted.txt",
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    );
    let _ = std::panic::take_hook();
    finish("plot items crafted", &score);

    // Every crafted board is fully loadable and every layer of it is compared.
    assert_eq!(score.files_pending, 0);
}
