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

//! FAB-05 verification while FAB-06 (`PlotFootprintTextItems`, `PlotText`) is pending: the
//! whole of `PlotStandardLayer` (board graphics, footprint graphics, pads, vias, tracks,
//! zones) against the whole Gerber files of the reference kicad-cli, on every layer where the
//! footprint text loop has nothing to plot.
//!
//! `PlotStandardLayer` runs `PlotBoardGraphicItem` for every board drawing,
//! `PlotFootprintTextItems` for every footprint, `PlotFootprintGraphicItems` for every
//! footprint, then the FAB-04 part. This test runs the same sequence; the footprint text loop
//! is left out on layers that carry no visible footprint field and no footprint text or
//! dimension (it plots nothing there), and the layer counts as pending otherwise. Text plotted
//! by FAB-05 code (text boxes, table cells, dimensions) reaches the pending `PlotText` and
//! makes the layer pending as well. Layers with drill maps (FAB-13) or a solder mask minimum
//! width (FAB-10) are skipped. Goldens: the corpus (`tests/golden/plot_driver/corpus.txt`,
//! CLI defaults), the FAB-04 crafted boards (`tests/golden/plot_items/crafted.txt`) and the
//! FAB-05 crafted boards (`tests/golden/plot_graphics/crafted.txt`, `generate.py` there).
//! `KICAD_FAB_CORPUS_REPORT=1` prints per-board results (`F` match, `.` pending, `X` differs,
//! `-` skipped), `KICAD_FAB_CORPUS_DUMP=<dir>` writes the port's output of differing files.

mod plot_common;
mod plot_items_fixture;

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_core::color4d::Color4D;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::plot_enums::DrillMarks;
use kicad_core::{KicadT, Lset, PcbLayerId};
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

/// Files that differ for a reason outside this crate (board label and layer name, reason).
/// They are reported, not failed; once the cause is fixed they match and count as matched.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    // `PAD::SetLocalSolderMaskMargin` sets the margin of the front AND the back mask layer
    // (`pcbnew/pad.h`); the model's `Pad::set_local_solder_mask_margin` sets the front only,
    // so a pad's local mask margin is lost on B.Mask (change request to kicad-pcb).
    (
        "qa/data/pcbnew/ipc2581/round_unequal_drill.kicad_pcb B.Mask",
        "pad local mask margin (model)",
    ),
    (
        "qa/data/pcbnew/issue7086.kicad_pcb B.Mask",
        "pad local mask margin (model)",
    ),
    (
        "qa/data/pcbnew/solder_mask_bridge_test.kicad_pcb B.Mask",
        "pad local mask margin (model)",
    ),
];

/// How a layer is treated.
#[derive(Debug, PartialEq)]
enum Class {
    /// Compared; `true` when the footprint text loop has something to plot (pending FAB-06).
    Compare { fp_text: bool },
    /// A drill map item on the board (FAB-13).
    DrillMap,
    /// Solder mask with a minimum width (FAB-10).
    MaskMinWidth,
    /// A barcode on the layer: the model has no symbol encoder yet (zint, M25a), so the
    /// barcode polygon is empty and `PlotBarCode` plots nothing.
    Barcode,
}

fn classify(ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Class {
    let board = ctx.board;

    if matches!(layer, PcbLayerId::FMask | PcbLayerId::BMask)
        && board.design_settings().solder_mask_min_width != 0
    {
        return Class::MaskMinWidth;
    }

    if board
        .drawings()
        .any(|(_, item)| matches!(item, AnyItem::DrillMap(_)))
    {
        return Class::DrillMap;
    }

    let on_layer = |id| {
        catch_unwind(AssertUnwindSafe(|| {
            ctx.item(id)
                .is_none_or(|item| item.layer_set().contains(layer))
        }))
        .unwrap_or(true)
    };
    let mut fp_text = false;
    let is_barcode = |id| {
        ctx.item(id)
            .is_some_and(|item| item.type_() == KicadT::PcbBarcodeT)
    };

    if board
        .drawings()
        .any(|(id, _)| is_barcode(id) && on_layer(id))
        || board.footprints().any(|(fp, _)| {
            board
                .footprint_drawings(fp)
                .any(|(id, _)| is_barcode(id) && on_layer(id))
        })
    {
        return Class::Barcode;
    }

    for (fp, _) in board.footprints() {
        for (id, field) in board.footprint_fields(fp) {
            if field.text.text.is_visible() && on_layer(id.into()) {
                fp_text = true;
            }
        }

        // PlotFootprintTextItems also plots every graphic PCB_TEXT (and, through the
        // PCB_TEXT base, every dimension) of the footprint.
        for (id, _) in board.footprint_drawings(fp) {
            let is_text = ctx.item(id).is_some_and(|item| {
                matches!(
                    item.type_(),
                    KicadT::PcbTextT
                        | KicadT::PcbDimAlignedT
                        | KicadT::PcbDimCenterT
                        | KicadT::PcbDimRadialT
                        | KicadT::PcbDimOrthogonalT
                        | KicadT::PcbDimLeaderT
                )
            });

            if is_text && on_layer(id) {
                fp_text = true;
            }
        }
    }

    Class::Compare { fp_text }
}

/// The number of board drawings and footprint graphic items on `layer`.
fn graphic_items_on(ctx: &BoardCtx<'_>, layer: PcbLayerId) -> usize {
    let board = ctx.board;
    let on = |id| {
        ctx.item(id)
            .is_some_and(|item| item.layer_set().contains(layer))
    };
    board.drawings().filter(|(id, _)| on(*id)).count()
        + board
            .footprints()
            .map(|(fp, _)| {
                board
                    .footprint_drawings(fp)
                    .filter(|(id, _)| on(*id))
                    .count()
            })
            .sum::<usize>()
}

/// `plot_gerber_layer_file` for one layer with the CLI options: `StartPlotBoard`,
/// `PlotBoardLayers` / `PlotOneBoardLayer` / `PlotStandardLayer` for a Gerber plotter
/// (the footprint text loop only when `fp_text`), `EndPlot`.
fn plot_layer(
    ctx: &BoardCtx<'_>,
    opts: &PcbPlotParams,
    layer: PcbLayerId,
    fp_text: bool,
) -> String {
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

        // Plot edge layer and graphic items
        for (id, _) in board.drawings() {
            if let Some(item) = ctx.item(id) {
                // While `PlotText` is pending (FAB-06), leave out the text items it would
                // return from at once (`!m_layerMask[aLayer]`): a text, text box or table
                // on another layer plots nothing (the box border and the table borders
                // check the layer too, and Gerber ignores `SetTextMode`).
                let text_only = matches!(
                    item.type_(),
                    KicadT::PcbTextT
                        | KicadT::PcbTextboxT
                        | KicadT::PcbTableT
                        | KicadT::PcbDrillChartT
                        | KicadT::PcbStackupTableT
                );

                if text_only && item.layer() != layer {
                    continue;
                }

                itemplotter.plot_board_graphic_item(item);
            }
        }

        // Draw footprint texts:
        if fp_text {
            for (footprint, _) in board.footprints() {
                itemplotter.plot_footprint_text_items(footprint);
            }
        }

        // Draw footprint other graphic items:
        for (footprint, _) in board.footprints() {
            itemplotter.plot_footprint_graphic_items(footprint);
        }

        plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
    }

    plotter.end_plot();
    normalise(plotter.output())
}

#[derive(Default, Debug)]
struct Score {
    boards: usize,
    files_matched: usize,
    /// Board drawings and footprint graphic items on the layers of the matched files.
    graphics_matched: usize,
    files_pending: usize,
    skipped_drill_map: usize,
    skipped_mask_min_width: usize,
    skipped_barcode: usize,
    /// Files in `KNOWN_DIFFERENCES` that still differ.
    known: usize,
    /// Pending files per panic message (the first line).
    pending_reasons: std::collections::BTreeMap<String, usize>,
    failures: Vec<String>,
}

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

        let fp_text = match classify(&ctx, layer) {
            Class::Compare { fp_text } => fp_text,
            Class::DrillMap => {
                score.skipped_drill_map += 1;
                line.push('-');
                continue;
            }
            Class::MaskMinWidth => {
                score.skipped_mask_min_width += 1;
                line.push('-');
                continue;
            }
            Class::Barcode => {
                score.skipped_barcode += 1;
                line.push('-');
                continue;
            }
        };

        match catch_unwind(AssertUnwindSafe(|| plot_layer(&ctx, &opts, layer, fp_text))) {
            Ok(text) if fnv(&text)[..8] == *file_hash => {
                score.files_matched += 1;
                score.graphics_matched += graphic_items_on(&ctx, layer);
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

                if KNOWN_DIFFERENCES.iter().any(|(k, _)| *k == what) {
                    score.known += 1;
                    line.push('k');
                } else {
                    score.failures.push(format!("{what}: file differs"));
                    line.push('X');
                }
            }
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                let msg = msg.lines().next().unwrap_or_default().to_string();
                *score.pending_reasons.entry(msg).or_default() += 1;
                score.files_pending += 1;
                line.push('.');
            }
        }
    }

    if report {
        println!("{label}: {line}");
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
        "{what}: {} boards loaded: files {} matched ({} graphic items on them) / {} pending, skipped {} (drill map) + {} \
         (mask min width) + {} (barcode), {} known differences, {} failures",
        score.boards,
        score.files_matched,
        score.graphics_matched,
        score.files_pending,
        score.skipped_drill_map,
        score.skipped_mask_min_width,
        score.skipped_barcode,
        score.known,
        score.failures.len()
    );

    for (reason, count) in &score.pending_reasons {
        println!("  pending {count:5}: {reason}");
    }

    assert!(
        score.failures.is_empty(),
        "{} differences:\n{}",
        score.failures.len(),
        score.failures.join("\n")
    );
}

fn crafted_loader(path: &std::path::Path) -> Result<Board, String> {
    load_board(path).or_else(|_| {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(plot_items_fixture::load_with_fixture_footprints(
            &text,
            path.to_str().unwrap(),
        ))
    })
}

#[test]
fn corpus_graphics_replay() {
    std::panic::set_hook(Box::new(|_| {}));
    let score = run(
        "tests/golden/plot_driver/corpus.txt",
        &kicad_qa::kicad_source_root(),
        load_board,
    );
    let _ = std::panic::take_hook();
    finish("plot graphics corpus", &score);
}

#[test]
fn crafted_graphics_replay() {
    std::panic::set_hook(Box::new(|_| {}));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fab04 = run("tests/golden/plot_items/crafted.txt", dir, crafted_loader);
    let fab05 = run(
        "tests/golden/plot_graphics/crafted.txt",
        dir,
        crafted_loader,
    );
    let _ = std::panic::take_hook();
    finish("plot graphics, FAB-04 crafted boards", &fab04);
    finish("plot graphics, FAB-05 crafted boards", &fab05);
}

/// The FAB-05 items through the SVG engine (black and white): solid, dashed and filled shapes
/// of every type, native Bezier curves, hatching, line endings, targets, footprint graphics,
/// mirror, mask expansion. Against `kicad-cli pcb export svg --mode-single
/// --exclude-drawing-sheet --drill-shape-opt 0 --black-and-white`
/// (`tests/golden/plot_graphics/svg_*.svg`, `generate.py`); the calls are those of the FAB-03
/// SVG test (`PCB_PLOTTER::Plot` for one layer). The footprint fields of `graphics.kicad_pcb`
/// are hidden, so the footprint text loop plots nothing and is left out.
#[test]
fn svg_graphics_match_cli() {
    use kicad_core::plot_enums::PlotFormat;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = dir.join("tests/data/plot_graphics/graphics.kicad_pcb");
    let mut board = load_board(&path).expect("graphics.kicad_pcb loads");
    let connectivity = build_connectivity(&mut board).expect("connectivity");
    let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);

    for (case, layer, mirror) in [
        ("f_silks", PcbLayerId::FSilkS, false),
        ("dwgs_mirror", PcbLayerId::DwgsUser, true),
        ("f_mask", PcbLayerId::FMask, false),
    ] {
        assert_eq!(classify(&ctx, layer), Class::Compare { fp_text: false });

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

        // PlotBoardLayers / PlotOneBoardLayer / PlotStandardLayer
        let mut plot_opt = opts.clone();
        plotter.set_color(&Color4D::BLACK);
        plotter.set_text_mode(plot_opt.text_mode);

        if is_copper_layer(layer) {
            plot_opt.skip_npth_pads = true;
        }

        let layer_mask = Lset::from_layers(&[layer]);
        {
            let mut itemplotter = BrditemsPlotter::new(plotter.as_mut(), &ctx, &plot_opt);
            itemplotter.set_layer_set(&layer_mask);
            let flags = StandardLayerFlags::new(&ctx, &layer_mask, &plot_opt);

            for (id, _) in board.drawings() {
                itemplotter.plot_board_graphic_item(ctx.item(id).unwrap());
            }

            for (footprint, _) in board.footprints() {
                itemplotter.plot_footprint_graphic_items(footprint);
            }

            plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
        }

        plotter.end_plot();

        let golden = std::fs::read_to_string(
            dir.join("tests/golden/plot_graphics")
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
        let ours = strip_date(plotter.output());

        if ours != strip_date(&golden)
            && let Some(dump) = std::env::var_os("KICAD_FAB_CORPUS_DUMP")
        {
            std::fs::write(
                std::path::Path::new(&dump).join(format!("svg_{case}.svg")),
                plotter.output(),
            )
            .unwrap();
        }

        assert_eq!(ours, strip_date(&golden), "{case}");
    }
}
