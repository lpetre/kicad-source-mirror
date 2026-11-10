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

//! Helpers shared by the FAB-03 plot driver tests (`plot_driver.rs`, `plot_driver_corpus.rs`):
//! board loading as kicad-cli does it, the CLI default plot options of
//! `kicad-cli pcb export gerbers`, the normalisation of `tests/golden/plot_driver/generate.py`.

#![allow(dead_code)]

use std::path::Path;
use std::rc::Rc;

use kicad_core::PcbLayerId;
use kicad_core::plot_enums::{DrillMarks, PlotFormat};
use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::{PlotEnv, gerber_protel_extension};
use kicad_fab::wx_file_name::WxFileName;
use kicad_pcb::board::Board;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::project::board_design_settings_json::RawDrcExclusionCodec;
use kicad_pcb::project::load_project;
use kicad_plot::gbr_metadata::GbrCreationDate;

/// The values the oracle prints (the date is normalised away).
pub fn env() -> PlotEnv {
    PlotEnv {
        build_version: "10.99.0-unknown".to_string(),
        creation_date: GbrCreationDate {
            year: 2026,
            month: 11,
            day: 10,
            hour: 1,
            minute: 2,
            second: 3,
            utc_offset_minutes: 60,
        },
    }
}

/// Loads a board like `BOARD_LOADER::Load` in kicad-cli as far as the plot header depends on
/// it: parse, then bind the project `<board>.kicad_pro` (if present; text variables) with
/// `BOARD::SetProject`. The other post-load steps (`initialize_loaded_board`) still reach
/// pending model stubs and do not influence the parts verified here.
pub fn load_board(path: &Path) -> Result<Board, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let name = path.to_str().unwrap();
    let mut board =
        kicad_pcb::io::parse_board(&text, name, &kicad_pcb::io::ParseOptions::default())
            .map_err(|e| format!("{e:?}"))?;
    board.file_name = name.to_string();

    let pro_path = path.with_extension("kicad_pro");
    let pro = std::fs::read_to_string(&pro_path).ok();
    let project = load_project(pro.as_deref(), None, pro_path.to_str().unwrap());

    board
        .set_project(project, Rc::new(RawDrcExclusionCodec))
        .map_err(|e| format!("{e:?}"))?;

    Ok(board)
}

/// `PCB_PLOTTER::PlotJobToPlotOpts` for a `JOB_EXPORT_PCB_GERBERS` filled by
/// `kicad-cli pcb export gerbers` (`docs/analysis/plot-fab.md` 1.2, 1.5); the flags are the
/// options the goldens use.
pub fn cli_gerber_opts(
    no_x2: bool,
    precision: i32,
    no_protel: bool,
    drill_origin: bool,
) -> PcbPlotParams {
    let mut o = PcbPlotParams::default();
    o.gerber_disable_apert_macros = false;
    o.use_gerber_protel_extensions = !no_protel;
    o.use_gerber_x2_format = !no_x2;
    o.include_gerber_netlist_info = true;
    o.create_gerber_job_file = true;
    o.set_gerber_precision(precision);
    o.drill_marks = DrillMarks::NoDrillShape;
    o.use_aux_origin = drill_origin;
    o.plot_drawing_sheet = false;
    o.subtract_mask_from_silk = false;
    o.plot_reference = true;
    o.plot_value = true;
    o.sketch_pads_on_fab_layers = false;
    o.hide_dnp_fps_on_fab_layers = false;
    o.sketch_dnp_fps_on_fab_layers = false;
    o.crossout_dnp_fps_on_fab_layers = false;
    o.plot_pad_numbers = false;
    o.mirror = false;
    o.negative = false;
    o.format = PlotFormat::Gerber;
    o
}

/// The enabled layer whose Gerber file name (as `JobExportGerbers` builds it) is `file`.
pub fn layer_of_file(board: &Board, opts: &PcbPlotParams, file: &str) -> Option<PcbLayerId> {
    board
        .enabled_layers()
        .ui_order()
        .0
        .into_iter()
        .find(|&layer| {
            let ext = if opts.use_gerber_protel_extensions {
                gerber_protel_extension(layer.0)
            } else {
                "gbr".to_string()
            };
            let mut fn_ = WxFileName::new(&board.file_name);
            build_plot_file_name(&mut fn_, "", &board.layer_name(layer), &ext);
            fn_.full_name() == file
        })
}

/// Replaces the lines carrying the version and the date by `<normalised>`
/// (`docs/analysis/plot-fab.md` 9.3; same rules as `generate.py`).
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let replace = (body.starts_with("%TF.GenerationSoftware,KiCad,Pcbnew,")
            && body.ends_with("*%"))
            || (body.starts_with("%TF.CreationDate,") && body.ends_with("*%"))
            || (body.starts_with("G04 Created by KiCad (PCBNEW ")
                && body.contains(") date ")
                && body.ends_with('*'))
            || (body.starts_with("G04 #@! TF.GenerationSoftware,") && body.ends_with('*'))
            || (body.starts_with("G04 #@! TF.CreationDate,") && body.ends_with('*'));

        if replace {
            out.push_str("<normalised>");
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
        }
    }

    out
}

/// The text up to and including `G04 APERTURE LIST*` (what `StartPlot` writes).
pub fn header(text: &str) -> &str {
    const MARK: &str = "G04 APERTURE LIST*\n";
    match text.find(MARK) {
        Some(i) => &text[..i + MARK.len()],
        None => text,
    }
}

/// FNV-1a 64 as 16 hex digits.
pub fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
