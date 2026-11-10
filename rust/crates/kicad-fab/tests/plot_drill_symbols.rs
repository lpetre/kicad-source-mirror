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

//! FAB-13: drill maps (`BRDITEMS_PLOTTER::PlotDrillSymbols`, called by `PlotBoardLayers`) and
//! drill chart symbols (`BRDITEMS_PLOTTER::PlotChartSymbols`) against the reference kicad-cli.
//!
//! Boards: the stage-1 cases of kicad-pcb `tests/data/m26/cases.txt` (through, blind, micro and
//! backdrilled vias, NPTH, slots and oval holes; the default, the letters and the size-text
//! drill symbol profiles; the drill charts and stack-up tables of `tables.txt`) with the drill
//! maps of `tests/data/plot_drill_symbols/maps.txt` appended (maps on six layers, two sharing
//! a layer, offsets, guide crosses, slot outlines, all spans / through / micro-via / backdrill
//! span filters). The text is built as `tests/golden/plot_drill_symbols/generate.py` builds
//! it, then loaded as kicad-cli loads it (`plot_common::load_board`, incl.
//! `RefreshGeneratedTables`).
//!
//! Plots: `kicad-cli pcb export gerbers -l <LAYERS>` (one `PadPlotState` for the run) and
//! `pcb export svg --mode-single --exclude-drawing-sheet --drill-shape-opt 0 -l <layer>` per
//! layer, plus `--drill-shape-opt 2 -l Eco1.User` (a layer carrying a map: the global drill
//! mark pass is skipped). Golden: `golden.txt` (FNV-1a of each normalised file). With
//! `KICAD_FAB_DRILLSYM_REF=<generate.py scratch>/ref` a mismatch reports the first differing
//! line of the reference file, `KICAD_FAB_DRILLSYM_DUMP=<dir>` writes the port's differing
//! files.

mod plot_common;

use std::path::{Path, PathBuf};

use kicad_core::plot_enums::{DrillMarks, PlotFormat};
use kicad_core::{Lset, PcbLayerId};
use kicad_fab::color_settings::ColorSettings;
use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::gerber_protel_extension;
use kicad_fab::plot_board_layers::{PadPlotState, plot_board_layers, start_plot_board};
use kicad_fab::wx_file_name::WxFileName;
use kicad_pcb::AnyItem;
use kicad_pcb::board::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use plot_common::*;

const LAYERS: &[&str] = &[
    "Dwgs.User",
    "Cmts.User",
    "Eco1.User",
    "Eco2.User",
    "User.1",
    "B.Fab",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn m26_dir() -> PathBuf {
    crate_dir().join("../kicad-pcb/tests/data/m26")
}

/// The stage-1 cases of `cases.txt`: `(index, name, base board, profile)`.
fn cases() -> Vec<(usize, String, String, String)> {
    let text = std::fs::read_to_string(m26_dir().join("cases.txt")).unwrap();
    let mut out = Vec::new();

    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }

        let f: Vec<&str> = line.split('|').map(str::trim).collect();

        if f[3] == "tables" {
            out.push((out.len(), f[0].to_string(), f[1].to_string(), f[2].to_string()));
        }
    }

    out
}

/// `board_text` of `generate.py`.
fn board_text(index: usize, base: &str, profile: &str) -> String {
    let path = kicad_qa::pcbnew_data_dir().join(base);
    let mut text = std::fs::read_to_string(path)
        .unwrap()
        .trim_end()
        .to_string();

    if profile != "-" {
        let prof = std::fs::read_to_string(m26_dir().join(profile)).unwrap();
        let idx = text.find("(setup").unwrap() + "(setup".len();
        text.insert_str(idx, &format!("\n{}", prof.trim_end_matches('\n')));
    }

    let mut extra = std::fs::read_to_string(m26_dir().join("tables.txt")).unwrap()
        + &std::fs::read_to_string(crate_dir().join("tests/data/plot_drill_symbols/maps.txt"))
            .unwrap();

    for k in 0..12 {
        extra = extra.replace(
            &format!("@UUID{k:X}@"),
            &format!("{:08x}-0000-4000-8000-{k:012}", 0x1300_0000 + index),
        );
    }

    text.pop();
    text + &extra + ")\n"
}

/// The board of a case, written to the test's scratch dir and loaded as kicad-cli does.
fn load_case(index: usize, name: &str, base: &str, profile: &str) -> (Board, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plot_drill_symbols");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.kicad_pcb"));
    std::fs::write(&path, board_text(index, base, profile)).unwrap();
    let board = load_board(&path).unwrap();
    (board, path)
}

/// `golden.txt`: `(case, file) -> hash`.
fn golden() -> Vec<(String, String, String)> {
    let text =
        std::fs::read_to_string(crate_dir().join("tests/golden/plot_drill_symbols/golden.txt"))
            .unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].to_string(), f[1].to_string(), f[2].to_string())
        })
        .collect()
}

/// `strip_svg_date` of `generate.py`.
fn strip_svg_date(text: &str) -> String {
    text.split('\n')
        .map(|l| match l.find(" date ") {
            Some(i) if l.starts_with("<title>") => format!("{}<date>", &l[..i]),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The SVG options of `kicad-cli pcb export svg --mode-single --exclude-drawing-sheet
/// --drill-shape-opt <n>` (as in `plot_driver.rs`).
fn svg_opts(drill: DrillMarks) -> PcbPlotParams {
    let mut opts = PcbPlotParams::default();
    opts.format = PlotFormat::Svg;
    opts.scale = 1.0;
    opts.auto_scale = false;
    opts.drill_marks = drill;
    opts.black_and_white = false;
    opts.set_svg_precision(4);
    opts.svg_fit_page_to_board = false;
    opts.sketch_dnp_fps_on_fab_layers = false;
    opts.crossout_dnp_fps_on_fab_layers = false;
    opts
}

/// Every file of a case as `(file name, normalised text)`, in `golden.txt` order.
fn plot_case(board: &mut Board, path: &Path) -> Vec<(String, String)> {
    let engine = drc_engine(board, path);
    let connectivity = build_connectivity(board).expect("connectivity");
    let ctx = BoardCtx::bare(board)
        .with_connectivity(&connectivity)
        .with_rules(&engine);
    let layers: Vec<PcbLayerId> = LAYERS.iter().map(|&n| board.layer_id(n)).collect();
    let mut files = Vec::new();

    // JobExportGerbers: the enabled layers of the list, UI order, one board for the run
    let opts = cli_gerber_opts(false, 6, false, false);
    let pad_state = PadPlotState::new();
    let gerber_layers = &Lset::from_layers(&layers) & &board.enabled_layers();

    for layer in gerber_layers.ui_order().0 {
        let layer_name = board.layer_name(layer);
        let mut fn_ = WxFileName::new(&board.file_name);
        build_plot_file_name(&mut fn_, "", &layer_name, &gerber_protel_extension(layer.0));

        let mut plotter = start_plot_board(
            &ctx,
            &opts,
            layer.0,
            &layer_name,
            "out/x",
            "",
            "",
            "1",
            "",
            1,
            &ColorSettings::default(),
            &env(),
        )
        .unwrap();
        plot_board_layers(
            &ctx,
            plotter.as_mut(),
            &[layer],
            &opts,
            ColorSettings::default(),
            &pad_state,
        );
        plotter.end_plot();
        files.push((fn_.full_name(), normalise(plotter.output())));
    }

    // One SVG run (a freshly loaded board) per layer
    let svg_runs = LAYERS
        .iter()
        .map(|&n| (n, DrillMarks::NoDrillShape, "0"))
        .chain([("Eco1.User", DrillMarks::FullDrillShape, "2")]);

    for (name, drill, drill_opt) in svg_runs {
        let layer = board.layer_id(name);
        let opts = svg_opts(drill);
        let layer_name = board.layer_name(layer);
        let out_name = format!("{}-{drill_opt}.svg", name.replace('.', "_"));
        let mut plotter = start_plot_board(
            &ctx,
            &opts,
            layer.0,
            &layer_name,
            &out_name,
            &layer_name,
            "",
            &layer_name,
            "",
            1,
            &ColorSettings::builtin_default(),
            &env(),
        )
        .unwrap();
        plotter.set_title("");
        plot_board_layers(
            &ctx,
            plotter.as_mut(),
            &[layer],
            &opts,
            ColorSettings::builtin_default(),
            &PadPlotState::new(),
        );
        plotter.end_plot();
        files.push((out_name, strip_svg_date(plotter.output())));
    }

    files
}

/// The first line where `ours` and the reference file differ (with `KICAD_FAB_DRILLSYM_REF`).
fn first_difference(case: &str, file: &str, ours: &str) -> String {
    let Ok(dir) = std::env::var("KICAD_FAB_DRILLSYM_REF") else {
        return String::new();
    };
    let Ok(reference) = std::fs::read_to_string(Path::new(&dir).join(case).join(file)) else {
        return " (no reference file)".to_string();
    };

    for (i, (a, b)) in reference.split('\n').zip(ours.split('\n')).enumerate() {
        if a != b {
            return format!(" (line {}: reference `{a}`, port `{b}`)", i + 1);
        }
    }

    " (one file is a prefix of the other)".to_string()
}

/// Every case of `golden.txt`: the files of each case in order, byte-identical after
/// normalisation; the boards carry the drill maps and charts the test is about.
#[test]
fn drill_symbols_match_kicad_cli() {
    let golden = golden();
    let mut failures = Vec::new();
    let mut compared = 0;

    for (index, name, base, profile) in cases() {
        let (mut board, path) = load_case(index, &name, &base, &profile);

        let maps = board
            .drawings()
            .filter(|(_, item)| matches!(item, AnyItem::DrillMap(_)))
            .count();
        assert_eq!(maps, 7, "{name}: drill maps");

        let expected: Vec<&(String, String, String)> =
            golden.iter().filter(|(c, _, _)| *c == name).collect();
        let files = plot_case(&mut board, &path);
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        let expected_names: Vec<&str> = expected.iter().map(|(_, f, _)| f.as_str()).collect();
        assert_eq!(names, expected_names, "{name}: files");

        for ((file, text), (_, _, hash)) in files.iter().zip(&expected) {
            compared += 1;

            if fnv(text) != *hash {
                if let Ok(dir) = std::env::var("KICAD_FAB_DRILLSYM_DUMP") {
                    let dir = Path::new(&dir).join(&name);
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(file), text).unwrap();
                }

                failures.push(format!(
                    "{name} {file}: differs{}",
                    first_difference(&name, file, text)
                ));
            }
        }
    }

    println!("plot_drill_symbols: {compared} files compared, {} differ", failures.len());
    assert_eq!(compared, golden.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
