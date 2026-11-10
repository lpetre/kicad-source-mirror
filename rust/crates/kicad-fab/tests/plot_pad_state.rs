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

//! FIX-FAB-1: the pad that the `plotPadLayer` lambda of `PlotStandardLayer`
//! (`pcbnew/plot_board_layers.cpp` L446-681) leaves on the board for the layers plotted after
//! it, in a footprint with a scale (`PadPlotState`).
//!
//! Boards (`tests/data/plot_pad_state`, KiCad 8 syntax canonicalised with
//! `kicad-cli pcb upgrade --force`, then `(scale 1 1)` edited and upgraded again):
//! * `one.kicad_pcb`: `rust/docs/design/review-fab/gen/one_pad.py one.kicad_pcb 90 2 0.5 15
//!   cham` (one footprint at 90 degrees, `(scale 2 0.5)`, one chamfered pad 2.2 x 1.4 mm at 15
//!   degrees, board mask clearance 0.05 mm);
//! * `uniform.kicad_pcb`: one footprint at 33 degrees with `(scale 0.3 0.3)`, six pads
//!   (chamfered, round rect, rect, oval, trapezoid, custom) rotated in the footprint, with sizes
//!   that are not multiples of 10 nm.
//!
//! Goldens (`tests/golden/plot_pad_state`, dates and versions normalised): the reference
//! `kicad-cli pcb export gerbers -l F.Cu,F.Paste,F.Mask` (`all-*`) and `-l F.Mask` (`mask-*`).
//! The two `F.Mask` files differ in the reference (1 to 3 nm in an aperture of each board):
//! the mask plotted after the copper and paste layers starts from the restored pads.

mod plot_common;

use kicad_core::{Lset, PcbLayerId};
use kicad_fab::color_settings::ColorSettings;
use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::gerber_protel_extension;
use kicad_fab::plot_board_layers::{PadPlotState, plot_board_layers, start_plot_board};
use kicad_fab::wx_file_name::WxFileName;
use kicad_pcb::ctx::BoardCtx;
use plot_common::*;

fn crate_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The files of one `JobExportGerbers` run over `a_layers` (UI order, one file per layer, no
/// common layers), all from one loaded board and therefore one `PadPlotState`, as
/// `(file name, normalised contents)`.
fn export_gerbers(name: &str, a_layers: &[PcbLayerId]) -> (Vec<(String, String)>, usize) {
    let path = crate_dir()
        .join("tests/data/plot_pad_state")
        .join(format!("{name}.kicad_pcb"));
    let mut board = load_board(&path).unwrap();
    let engine = drc_engine(&mut board, &path);
    let connectivity = build_connectivity(&mut board).expect("connectivity");
    let ctx = BoardCtx::bare(&board)
        .with_connectivity(&connectivity)
        .with_rules(&engine);
    let opts = cli_gerber_opts(false, 6, false, false);
    let pad_state = PadPlotState::new();
    let mut files = Vec::new();

    for layer in Lset::from_layers(a_layers).ui_order().0 {
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

    (files, pad_state.len())
}

fn golden(file: &str) -> String {
    std::fs::read_to_string(crate_dir().join("tests/golden/plot_pad_state").join(file)).unwrap()
}

fn check(name: &str) {
    // All three layers from one board: F.Mask after F.Cu and F.Paste.
    let (files, modified) = export_gerbers(
        name,
        &[PcbLayerId::FCu, PcbLayerId::FPaste, PcbLayerId::FMask],
    );
    assert_eq!(files.len(), 3);
    assert!(modified > 0, "{name}: no pad recorded in the plot state");

    for (file, text) in &files {
        assert_eq!(*text, golden(&format!("all-{file}")), "{name}: {file}");
    }

    // F.Mask alone: the pads as in the file.
    let (mask_only, _) = export_gerbers(name, &[PcbLayerId::FMask]);
    assert_eq!(mask_only.len(), 1);
    let (file, text) = &mask_only[0];
    assert_eq!(
        *text,
        golden(&format!("mask-{file}")),
        "{name}: {file} alone"
    );

    // The test needs the two reference masks to differ.
    assert_ne!(
        golden(&format!("mask-{file}")),
        golden(&format!("all-{file}"))
    );
}

#[test]
fn non_uniform_scale_chamfered_pad() {
    check("one");
}

#[test]
fn uniform_scale_below_one() {
    check("uniform");
}
