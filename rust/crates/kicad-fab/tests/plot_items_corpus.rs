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
mod plot_items_fixture;

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

/// Files that differ because of a model bug outside this crate: the pad's local solder mask
/// margin is lost on B.Mask (change request to kicad-pcb, FAB-05 result).
const KNOWN_DIFFERENCES: &[&str] = &[
    "qa/data/pcbnew/ipc2581/round_unequal_drill.kicad_pcb B.Mask",
    "qa/data/pcbnew/issue7086.kicad_pcb B.Mask",
    "qa/data/pcbnew/solder_mask_bridge_test.kicad_pcb B.Mask",
];

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
            ctx.item(id)
                .is_none_or(|item| item.layer_set().contains(layer))
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
        // PlotFootprintTextItems plots visible fields only
        for (id, field) in board.footprint_fields(fp) {
            if field.text.text.is_visible() && on_layer(id.into()) {
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
    let ext = if opts.use_gerber_protel_extensions {
        gerber_protel_extension(layer.0)
    } else {
        "gbr".to_string()
    };
    build_plot_file_name(&mut fn_, "out", &layer_name, &ext);

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
    // `bp_*` crafted boards are plotted with `--board-plot-params` (generate.py)
    let board_params = std::path::Path::new(label)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("bp_"));
    let opts = if board_params {
        board.plot_options.clone()
    } else {
        cli_gerber_opts(false, 6, false, false)
    };
    let mut line = String::new();
    let mut compared = false;

    for (name_hash, file_hash) in files {
        let Some(layer) = board
            .enabled_layers()
            .ui_order()
            .0
            .into_iter()
            .find(|&layer| {
                let mut fn_ = WxFileName::new(&board.file_name);
                let ext = if opts.use_gerber_protel_extensions {
                    gerber_protel_extension(layer.0)
                } else {
                    "gbr".to_string()
                };
                build_plot_file_name(&mut fn_, "", &board.layer_name(layer), &ext);
                fnv(&fn_.full_name())[..8] == *name_hash
            })
        else {
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
                let what = format!("{label} {}", board.layer_name(layer));

                // Known model difference (see `plot_graphics_corpus.rs`, KNOWN_DIFFERENCES):
                // `Pad::set_local_solder_mask_margin` sets the front mask margin only.
                if KNOWN_DIFFERENCES.contains(&what.as_str()) {
                    line.push('k');
                } else {
                    score.failures.push(format!("{what}: file differs"));
                    line.push('X');
                }
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
        let vias = board.tracks().filter(|(_, t)| t.as_via().is_some()).count();
        let arcs = board.tracks().filter(|(_, t)| t.as_arc().is_some()).count();
        println!(
            "{label}: {line} (pads {pads}, vias {vias}, tracks {}, arcs {arcs}, zones {})",
            board.tracks().count() - vias - arcs,
            board.zones().count()
        );
    }
}

/// The `B <path>` / `<name> <header> <file>` golden format of `generate.py`.
fn read_golden(rel: &str) -> Vec<(String, Vec<(String, String)>)> {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
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

fn run(
    golden: &str,
    root: &std::path::Path,
    loader: fn(&std::path::Path) -> Result<Board, String>,
) -> Score {
    let report = std::env::var_os("KICAD_FAB_CORPUS_REPORT").is_some();
    let mut score = Score::default();

    for (path, files) in read_golden(golden) {
        if files.is_empty() {
            continue;
        }

        let mut board = match catch_unwind(|| loader(&root.join(&path))) {
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
        load_board,
    );
    let _ = std::panic::take_hook();
    finish("plot items corpus", &score);
}

#[test]
fn crafted_items_replay() {
    std::panic::set_hook(Box::new(|_| {}));
    // The crafted boards have footprints: the pad parser (PCBIO-07) is pending, so they are
    // built by the fixture until it lands (then `load_board` takes over).
    let loader: fn(&std::path::Path) -> Result<Board, String> = |path| {
        load_board(path).or_else(|_| {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            Ok(plot_items_fixture::load_with_fixture_footprints(
                &text,
                path.to_str().unwrap(),
            ))
        })
    };
    let score = run(
        "tests/golden/plot_items/crafted.txt",
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        loader,
    );
    let _ = std::panic::take_hook();
    finish("plot items crafted", &score);

    // Every crafted board is fully loadable and every layer of it is compared.
    assert_eq!(score.files_pending, 0);
}

/// The FAB-04 part of `PlotStandardLayer` on the SVG engine (black and white): pads of every
/// shape (chamfered pads through the custom-shape fallback), margins on mask and paste, mirror.
/// Against `kicad-cli pcb export svg --mode-single --exclude-drawing-sheet --drill-shape-opt 0
/// --black-and-white` (`tests/golden/plot_items/svg_*.svg`, `generate.py`); the calls are
/// those of the FAB-03 SVG test (`PCB_PLOTTER::Plot` for one layer), the board graphics of
/// `pads.kicad_pcb` are on Edge.Cuts only and plot nothing on these layers.
#[test]
fn svg_items_match_cli() {
    use kicad_core::plot_enums::PlotFormat;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = dir.join("tests/data/plot_items/pads.kicad_pcb");
    let mut board = load_board(&path).unwrap_or_else(|_| {
        plot_items_fixture::load_with_fixture_footprints(
            &std::fs::read_to_string(&path).unwrap(),
            path.to_str().unwrap(),
        )
    });
    let connectivity = build_connectivity(&mut board).expect("connectivity");
    let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);

    for (case, layer, mirror) in [
        ("f_cu", PcbLayerId::FCu, false),
        ("f_mask_mirror", PcbLayerId::FMask, true),
        ("f_paste", PcbLayerId::FPaste, false),
    ] {
        let mut opts = PcbPlotParams::default();
        opts.format = PlotFormat::Svg;
        opts.scale = 1.0;
        opts.auto_scale = false;
        opts.drill_marks = DrillMarks::NoDrillShape;
        opts.set_svg_precision(4);
        opts.svg_fit_page_to_board = false;
        opts.black_and_white = true;
        opts.mirror = mirror;
        opts.sketch_dnp_fps_on_fab_layers = false;
        opts.crossout_dnp_fps_on_fab_layers = false;

        let layer_name = board.layer_name(layer);
        let mut plotter = start_plot_board(
            &ctx,
            &opts,
            layer.0,
            &layer_name,
            "a.svg",
            &layer_name,
            "",
            &layer_name,
            &env(),
        )
        .unwrap();
        plotter.set_title("");

        // PlotBoardLayers / PlotOneBoardLayer / PlotStandardLayer, FAB-04 part
        let mut plot_opt = opts.clone();
        plotter.set_color(&Color4D::BLACK);
        plotter.set_text_mode(plot_opt.text_mode);

        if is_copper_layer(layer) {
            plot_opt.skip_npth_pads = true;
        } else if layer == PcbLayerId::FPaste {
            plot_opt.drill_marks = DrillMarks::NoDrillShape;
        }

        let layer_mask = Lset::from_layers(&[layer]);
        {
            let mut itemplotter = BrditemsPlotter::new(plotter.as_mut(), &ctx, &plot_opt);
            itemplotter.set_layer_set(&layer_mask);
            let flags = StandardLayerFlags::new(&ctx, &layer_mask, &plot_opt);
            plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
        }

        plotter.end_plot();

        let golden = std::fs::read_to_string(
            dir.join("tests/golden/plot_items")
                .join(format!("svg_{case}.svg")),
        )
        .unwrap();
        let strip_date = |t: &str| -> String {
            t.lines()
                .map(|l| match l.find(" date ") {
                    Some(i) if l.starts_with("<title>") => format!("{}<date>", &l[..i]),
                    _ => l.to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(strip_date(plotter.output()), strip_date(&golden), "{case}");
    }
}
