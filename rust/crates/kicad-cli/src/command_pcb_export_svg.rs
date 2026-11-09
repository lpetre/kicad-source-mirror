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

//! Port of `kicad/cli/command_pcb_export_svg.cpp` (`CLI::PCB_EXPORT_SVG_COMMAND`).

use std::path::Path;

use clap::{ArgMatches, Command};
use kicad_core::plot_enums::DrillMarks;
use kicad_session::jobs::{JobExportPcbSvg, SvgGenMode};
use kicad_session::{Reporter, exit_codes, run_export_svg};

use crate::command::{
    CommonArgSet, CommonArgs, IoType, define_arg, double_opt, drawing_sheet_arg, flag, get_double,
    get_flag, get_int, get_string, input_arg, int_opt, output_arg, string_opt, variants_arg,
};
use crate::command_pcb_export_gerbers::{
    ARG_CHECK_ZONES, ARG_CHECK_ZONES_DESC, ARG_COMMON_LAYERS, ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS,
    ARG_HIDE_DNP_FPS_ON_FAB_LAYERS, ARG_LAYERS, ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS,
    ARG_SKETCH_PAD_NUMBERS, ARG_SKETCH_PADS_ON_FAB_LAYERS, ARG_SUBTRACT_SOLDERMASK,
    common_layers_arg, fab_layer_flags, layer_arg,
};
use crate::host;

const ARG_EXCLUDE_DRAWING_SHEET: &str = "exclude-drawing-sheet";
const ARG_PAGE_SIZE: &str = "page-size-mode";
const ARG_FIT_PAGE_TO_BOARD: &str = "fit-page-to-board";
const ARG_MODE_SINGLE: &str = "mode-single";
const ARG_MODE_MULTI: &str = "mode-multi";
const ARG_MIRROR: &str = "mirror";
const ARG_THEME: &str = "theme";
const ARG_NEGATIVE: &str = "negative";
const ARG_BLACKANDWHITE: &str = "black-and-white";
const ARG_DRILL_SHAPE_OPTION: &str = "drill-shape-opt";
const ARG_SCALE: &str = "scale";

/// `PCB_EXPORT_SVG_COMMAND::PCB_EXPORT_SVG_COMMAND()`.
pub fn command() -> Command {
    let cmd = Command::new("svg")
        .about("Generate SVG outputs of a given layer list")
        .arg(input_arg(IoType::File))
        .arg(output_arg(IoType::Directory))
        .arg(layer_arg())
        .arg(common_layers_arg())
        .arg(drawing_sheet_arg())
        .arg(define_arg())
        .arg(flag(ARG_SUBTRACT_SOLDERMASK, "Subtract soldermask from silkscreen"))
        .arg(
            flag(
                ARG_MIRROR,
                "Mirror the board (useful for trying to show bottom layers)",
            )
            .short('m'),
        )
        .arg(
            string_opt(
                ARG_THEME,
                "",
                "Color theme to use (will default to PCB editor settings)",
            )
            .short('t')
            .value_name("THEME_NAME"),
        )
        .arg(
            flag(
                ARG_NEGATIVE,
                "Plot as negative (useful for directly etching from the export)",
            )
            .short('n'),
        )
        .arg(flag(ARG_BLACKANDWHITE, "Black and white only"));

    fab_layer_flags(cmd)
        .arg(
            int_opt(
                ARG_PAGE_SIZE,
                "0",
                "Set page sizing mode (0 = page with frame and title block, 1 = current page \
                 size, 2 = board area only)",
            )
            .value_name("MODE"),
        )
        .arg(flag(ARG_FIT_PAGE_TO_BOARD, "Fit the page to the board"))
        .arg(flag(ARG_EXCLUDE_DRAWING_SHEET, "No drawing sheet"))
        .arg(
            int_opt(
                ARG_DRILL_SHAPE_OPTION,
                "2",
                "Set pad/via drill shape option (0 = no shape, 1 = small shape, 2 = actual shape)",
            )
            .value_name("SHAPE_OPTION"),
        )
        .arg(flag(
            ARG_MODE_SINGLE,
            "Generates a single file with the output arg path acting as the complete directory \
             and filename path. COMMON_LAYER_LIST does not function in this mode. Instead \
             LAYER_LIST controls all layers plotted.",
        ))
        .arg(flag(
            ARG_MODE_MULTI,
            "Generates one or more files with behavior similar to the KiCad GUI plotting. The \
             given output path specifies a directory in which files may be output.",
        ))
        .arg(
            double_opt(
                ARG_SCALE,
                "1",
                "Scale for the PCB, not for the border and title. Use 0 for autoscale",
            )
            .value_name("SCALE"),
        )
        .arg(flag(ARG_CHECK_ZONES, ARG_CHECK_ZONES_DESC))
        .arg(variants_arg())
}

/// The common arguments this command registers.
pub const COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: true,
    drawing_sheet: true,
    define: true,
    variant: true,
};

/// `static_cast<DRILL_MARKS>( drillShape )`. Values outside 0..2 are not an enumerator in C++
/// either; the port maps them to `FULL_DRILL_SHAPE` (recorded in `PORT_STATUS.md`).
fn drill_marks_from_int(v: i32) -> DrillMarks {
    match v {
        0 => DrillMarks::NoDrillShape,
        1 => DrillMarks::SmallDrillShape,
        _ => DrillMarks::FullDrillShape,
    }
}

/// Port of `PCB_EXPORT_SVG_COMMAND::doPerform` up to `ProcessJob`.
pub fn build_job(m: &ArgMatches, args: &CommonArgs) -> Result<JobExportPcbSvg, i32> {
    let mut svg_job = JobExportPcbSvg::new();
    let plot = &mut svg_job.plot;

    plot.mirror = get_flag(m, ARG_MIRROR);
    plot.black_and_white = get_flag(m, ARG_BLACKANDWHITE);
    plot.negative = get_flag(m, ARG_NEGATIVE);
    plot.sketch_pads_on_fab_layers = get_flag(m, ARG_SKETCH_PADS_ON_FAB_LAYERS);
    plot.plot_pad_numbers = plot.sketch_pads_on_fab_layers && get_flag(m, ARG_SKETCH_PAD_NUMBERS);
    plot.hide_dnp_fps_on_fab_layers = get_flag(m, ARG_HIDE_DNP_FPS_ON_FAB_LAYERS);
    plot.sketch_dnp_fps_on_fab_layers = get_flag(m, ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS);
    plot.crossout_dnp_fps_on_fab_layers = get_flag(m, ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS);
    let drill_shape = get_int(m, ARG_DRILL_SHAPE_OPTION);
    plot.drill_shape_option = drill_marks_from_int(drill_shape);
    plot.drawing_sheet = args.drawing_sheet.clone();
    plot.subtract_solder_mask_from_silk = get_flag(m, ARG_SUBTRACT_SOLDERMASK);
    plot.scale = get_double(m, ARG_SCALE);
    plot.check_zones_before_plot = get_flag(m, ARG_CHECK_ZONES);

    svg_job.fit_page_to_board = get_flag(m, ARG_FIT_PAGE_TO_BOARD);

    // legacy compat, should eliminate this arg eventually
    let legacy_page_size_mode = get_int(m, ARG_PAGE_SIZE);

    if legacy_page_size_mode == 0 {
        svg_job.plot.plot_drawing_sheet = true;
    } else if legacy_page_size_mode == 1 {
        svg_job.plot.plot_drawing_sheet = false;
    } else if legacy_page_size_mode == 2 {
        svg_job.fit_page_to_board = true;
        svg_job.plot.plot_drawing_sheet = false;
    }

    let plot = &mut svg_job.plot;
    plot.filename = args.input.clone();
    plot.job.set_configured_output_path(&args.output);
    plot.color_theme = get_string(m, ARG_THEME);
    plot.plot_drawing_sheet = !get_flag(m, ARG_EXCLUDE_DRAWING_SHEET);
    plot.job.set_var_overrides(args.define_vars.clone());

    if let Some(first) = args.variant_names.first() {
        plot.variant = first.clone();
    }

    plot.arg_layers = Some(get_string(m, ARG_LAYERS));
    plot.arg_common_layers = Some(get_string(m, ARG_COMMON_LAYERS));

    if !Path::new(&plot.filename).is_file() {
        eprint!("Board file does not exist or is not accessible\n");
        return Err(exit_codes::ERR_INVALID_INPUT_FILE);
    }

    if get_flag(m, ARG_MODE_MULTI) {
        svg_job.gen_mode = SvgGenMode::Multi;
    } else if get_flag(m, ARG_MODE_SINGLE) {
        svg_job.gen_mode = SvgGenMode::Single;
    } else if svg_job.gen_mode == SvgGenMode::Single {
        // TODO remove this block when changing the default for JOB_EXPORT_PCB_SVG::m_genMode
        println!(
            "\x1b[33;1m{}\x1b[0m",
            "This command has deprecated behavior as of KiCad 9.0, the default behavior of this \
             command will change in a future release."
        );
        println!("\x1b[33;1m{}\x1b[0m", "The new behavior will match --mode-multi");
    }

    Ok(svg_job)
}

/// `COMMAND::Perform` + `PCB_EXPORT_SVG_COMMAND::doPerform` + `KIWAY::ProcessJob`.
pub fn perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut job = match build_job(m, &args) {
        Ok(j) => j,
        Err(code) => return code,
    };

    let mut session = match host::get_board(&job.plot.filename) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let mut reporter = Reporter::new();
    let result = run_export_svg(&mut session, &mut job, &mut reporter);
    host::finish_job(result, reporter)
}
