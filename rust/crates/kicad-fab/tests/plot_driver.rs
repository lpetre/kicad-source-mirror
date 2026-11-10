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

//! FAB-03 plot driver against the reference kicad-cli: `StartPlotBoard`, `initializePlotter`,
//! `PlotBoardLayers` / `PlotOneBoardLayer`, the X2 header (`AddGerberX2Attribute`), Protel
//! extensions and `BuildPlotFileName`, end to end on boards whose layers are empty
//! (`tests/data/plot_driver`, goldens `tests/golden/plot_driver/crafted.txt` made by
//! `generate.py crafted`).

mod plot_common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use kicad_core::PcbLayerId;
use kicad_fab::pcb_plotter::get_plot_sequence;
use kicad_fab::plot_board_layers::plot_gerber_layer_file;
use kicad_pcb::ctx::BoardCtx;
use plot_common::*;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(board stem, set)` -> `[(file name, normalised text)]` in "Plotted to" order.
fn crafted_goldens() -> BTreeMap<(String, String), Vec<(String, String)>> {
    let text =
        std::fs::read_to_string(crate_dir().join("tests/golden/plot_driver/crafted.txt")).unwrap();
    let mut cases: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    let mut current: Option<(String, String)> = None;

    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("#case ") {
            let (stem, set) = rest.trim_end().rsplit_once(' ').unwrap();
            let key = (stem.to_string(), set.to_string());
            cases.insert(key.clone(), Vec::new());
            current = Some(key);
        } else if let Some(file) = line.strip_prefix("#file ") {
            let key = current.as_ref().unwrap();
            cases
                .get_mut(key)
                .unwrap()
                .push((file.trim_end().to_string(), String::new()));
        } else {
            let key = current.as_ref().unwrap();
            cases
                .get_mut(key)
                .unwrap()
                .last_mut()
                .unwrap()
                .1
                .push_str(line);
        }
    }

    cases
}

#[test]
fn crafted_boards_match_cli_goldens() {
    let goldens = crafted_goldens();
    assert_eq!(goldens.len(), 4 * 7, "4 boards x 7 option sets");
    let mut compared = 0;

    for ((stem, set), files) in &goldens {
        let path = crate_dir().join(format!("tests/data/plot_driver/{stem}.kicad_pcb"));
        let board = load_board(&path).unwrap();
        let ctx = BoardCtx::bare(&board);

        let (opts, common): (_, Vec<PcbLayerId>) = match set.as_str() {
            "default" => (cli_gerber_opts(false, 6, false, false), vec![]),
            "no_x2" => (cli_gerber_opts(true, 6, false, false), vec![]),
            "prec5" => (cli_gerber_opts(false, 5, false, false), vec![]),
            "no_protel" => (cli_gerber_opts(false, 6, true, false), vec![]),
            "drill_origin" => (cli_gerber_opts(false, 6, false, true), vec![]),
            "board_params" => (
                board.plot_options.clone(),
                board.plot_options.plot_on_all_layers_sequence.clone(),
            ),
            "common" => (
                cli_gerber_opts(false, 6, false, false),
                vec![PcbLayerId::EdgeCuts, PcbLayerId::FFab],
            ),
            _ => panic!("unknown set {set}"),
        };

        if set == "default" {
            // kicad-cli plots every enabled layer, in UI order
            let expected: Vec<String> = board
                .enabled_layers()
                .ui_order()
                .0
                .into_iter()
                .map(|l| {
                    let out = plot_gerber_layer_file(&ctx, &opts, l, &[l], "out", &env()).unwrap();
                    out.name.strip_prefix("out/").unwrap().to_string()
                })
                .collect();
            let golden: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
            assert_eq!(expected, golden, "{stem}: file names / order");
        }

        for (file, golden) in files {
            let layer = layer_of_file(&board, &opts, file)
                .unwrap_or_else(|| panic!("{stem} {set}: no layer for {file}"));
            let seq = get_plot_sequence(layer, &common);
            let out = plot_gerber_layer_file(&ctx, &opts, layer, &seq, "out", &env()).unwrap();
            assert_eq!(out.name, format!("out/{file}"));
            let text = normalise(std::str::from_utf8(&out.contents).unwrap());
            assert_eq!(&text, golden, "{stem} {set} {file}");
            compared += 1;
        }
    }

    assert_eq!(compared, 247);
}

#[test]
fn normaliser_replaces_only_date_and_version_lines() {
    let text = "%TF.GenerationSoftware,KiCad,Pcbnew,1.0*%\n%TF.CreationDate,x*%\n\
                G04 Created by KiCad (PCBNEW 1) date 2*\nG04 #@! TF.CreationDate,3*\n\
                %TF.ProjectId,a,b,c*%\nM02*\n";
    assert_eq!(
        normalise(text),
        "<normalised>\n<normalised>\n<normalised>\n<normalised>\n%TF.ProjectId,a,b,c*%\nM02*\n"
    );
    assert_eq!(
        header("a\nG04 APERTURE LIST*\nb\n"),
        "a\nG04 APERTURE LIST*\n"
    );
}

/// `StartPlotBoard` / `initializePlotter` / `FillNegativeKnockout` for SVG on an empty board,
/// against `kicad-cli pcb export svg --mode-single --exclude-drawing-sheet --drill-shape-opt 0`
/// with `--black-and-white -l F.Cu -o a.svg` and `-n -m -l B.Cu -o c.svg` (board
/// `hdr_4layer`; stored as `svg_bw_f_cu.svg`, `svg_neg_mirror_b_cu.svg`). The calls are those of
/// `PCB_PLOTTER::Plot` (FAB-11) for one layer: `StartPlotBoard` with the layer name as page
/// name, `SetTitle` (empty title block), `PlotBoardLayers`, `EndPlot`.
#[test]
fn svg_start_plot_board_matches_cli() {
    use kicad_core::plot_enums::{DrillMarks, PlotFormat};
    use kicad_fab::plot_board_layers::{plot_board_layers, start_plot_board};
    use kicad_pcb::pcb_plot_params::PcbPlotParams;

    let board =
        load_board(&crate_dir().join("tests/data/plot_driver/hdr_4layer.kicad_pcb")).unwrap();
    let ctx = BoardCtx::bare(&board);

    // (golden, the -o name kicad-cli was given, layer, black and white, negative, mirror)
    for (file, out_name, layer, bw, negative, mirror) in [
        (
            "svg_bw_f_cu.svg",
            "a.svg",
            PcbLayerId::FCu,
            true,
            false,
            false,
        ),
        (
            "svg_neg_mirror_b_cu.svg",
            "c.svg",
            PcbLayerId::BCu,
            false,
            true,
            true,
        ),
    ] {
        let mut opts = PcbPlotParams::default();
        opts.format = PlotFormat::Svg;
        opts.scale = 1.0;
        opts.auto_scale = false;
        opts.drill_marks = DrillMarks::NoDrillShape;
        opts.set_svg_precision(4);
        opts.svg_fit_page_to_board = false;
        opts.black_and_white = bw;
        opts.negative = negative;
        opts.mirror = mirror;
        opts.sketch_dnp_fps_on_fab_layers = false;
        opts.crossout_dnp_fps_on_fab_layers = false;

        let layer_name = board.layer_name(layer);
        let mut plotter = start_plot_board(
            &ctx,
            &opts,
            layer.0,
            &layer_name,
            out_name,
            &layer_name,
            "",
            &layer_name,
            "",
            1,
            &kicad_fab::color_settings::ColorSettings::builtin_default(),
            &env(),
        )
        .unwrap();
        plotter.set_title("");
        plot_board_layers(
            &ctx,
            plotter.as_mut(),
            &[layer],
            &opts,
            kicad_fab::color_settings::ColorSettings::builtin_default(),
            &kicad_fab::plot_board_layers::PadPlotState::new(),
        );
        plotter.end_plot();

        let golden =
            std::fs::read_to_string(crate_dir().join("tests/golden/plot_driver").join(file))
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
        assert_eq!(strip_date(plotter.output()), strip_date(&golden), "{file}");
    }
}
