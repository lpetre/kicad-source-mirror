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

//! Port of `kicad/cli/command_pcb_export_gerber.cpp` (`PCB_EXPORT_GERBER_COMMAND`: the option
//! definitions and `populateJob`), `kicad/cli/command_pcb_export_gerbers.cpp`
//! (`PCB_EXPORT_GERBERS_COMMAND`) and the layer options of
//! `kicad/cli/command_pcb_export_base.cpp` (`addLayerArg`, `addCommonLayersArg`).

use std::path::Path;

use clap::{Arg, ArgMatches, Command};
use kicad_session::jobs::{JobExportPcbGerber, JobExportPcbGerbers};
use kicad_session::{Reporter, exit_codes, run_export_gerbers};

use crate::command::{
    CommonArgSet, CommonArgs, IoType, define_arg, drawing_sheet_arg, flag, flag2, get_flag,
    get_int, get_string, input_arg, int_opt, output_arg, string_opt, variants_arg,
};
use crate::host;

/// `ARG_LAYERS`
pub const ARG_LAYERS: &str = "layers";
/// `ARG_COMMON_LAYERS`
pub const ARG_COMMON_LAYERS: &str = "common-layers";
/// `ARG_SKETCH_PADS_ON_FAB_LAYERS`
pub const ARG_SKETCH_PADS_ON_FAB_LAYERS: &str = "sketch-pads-on-fab-layers";
/// `ARG_SKETCH_PADS_ON_FAB_LAYERS_DESC`
pub const ARG_SKETCH_PADS_ON_FAB_LAYERS_DESC: &str =
    "Draw pad outlines on front and back fab layers";
/// `ARG_SKETCH_PAD_NUMBERS`
pub const ARG_SKETCH_PAD_NUMBERS: &str = "sketch-pad-numbers";
/// `ARG_SKETCH_PAD_NUMBERS_DESC`
pub const ARG_SKETCH_PAD_NUMBERS_DESC: &str =
    "Plot pad numbers on top of sketched pads on fab layers";
/// `ARG_HIDE_DNP_FPS_ON_FAB_LAYERS`
pub const ARG_HIDE_DNP_FPS_ON_FAB_LAYERS: &str = "hide-DNP-footprints-on-fab-layers";
/// `ARG_HIDE_DNP_FPS_ON_FAB_LAYERS_DESC`
pub const ARG_HIDE_DNP_FPS_ON_FAB_LAYERS_DESC: &str =
    "Don't plot text & graphics of DNP footprints on fab layers";
/// `ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS`
pub const ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS: &str = "sketch-DNP-footprints-on-fab-layers";
/// `ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS_DESC`
pub const ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS_DESC: &str =
    "Plot graphics of DNP footprints in sketch mode on fab layers";
/// `ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS`
pub const ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS: &str = "crossout-DNP-footprints-on-fab-layers";
/// `ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS_DESC`
pub const ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS_DESC: &str = "Plot an 'X' over the courtyard of DNP \
     footprints on fab layers, and strikeout their reference designators";
/// `ARG_SUBTRACT_SOLDERMASK`
pub const ARG_SUBTRACT_SOLDERMASK: &str = "subtract-soldermask";
/// `ARG_CHECK_ZONES`
pub const ARG_CHECK_ZONES: &str = "check-zones";
/// `ARG_CHECK_ZONES_DESC`
pub const ARG_CHECK_ZONES_DESC: &str = "Check and refill zones if required";

const ARG_EXCLUDE_REFDES: &str = "exclude-refdes";
const ARG_EXCLUDE_VALUE: &str = "exclude-value";
const ARG_INCLUDE_BORDER_TITLE: &str = "include-border-title";
const ARG_NO_X2: &str = "no-x2";
const ARG_NO_NETLIST: &str = "no-netlist";
const ARG_DISABLE_APERTURE_MACROS: &str = "disable-aperture-macros";
const ARG_USE_DRILL_FILE_ORIGIN: &str = "use-drill-file-origin";
const ARG_PRECISION: &str = "precision";
const ARG_NO_PROTEL_EXTENSION: &str = "no-protel-ext";
const ARG_USE_BOARD_PLOT_PARAMS: &str = "board-plot-params";

/// `PCB_EXPORT_BASE_COMMAND::addLayerArg`.
pub fn layer_arg() -> Arg {
    string_opt(
        ARG_LAYERS,
        "",
        "Comma separated list of untranslated layer names to include such as F.Cu,B.Cu",
    )
    .short('l')
    .value_name("LAYER_LIST")
}

/// `PCB_EXPORT_BASE_COMMAND::addCommonLayersArg`.
pub fn common_layers_arg() -> Arg {
    string_opt(
        ARG_COMMON_LAYERS,
        "",
        "Layers to include on each plot, comma separated list of untranslated layer names to \
         include such as F.Cu,B.Cu",
    )
    .visible_alias("cl")
    .value_name("COMMON_LAYER_LIST")
}

/// The fab-layer flags shared by the Gerber and SVG commands.
pub fn fab_layer_flags(cmd: Command) -> Command {
    cmd.arg(flag2(
        "sp",
        ARG_SKETCH_PADS_ON_FAB_LAYERS,
        ARG_SKETCH_PADS_ON_FAB_LAYERS_DESC,
    ))
    .arg(flag2(
        "spn",
        ARG_SKETCH_PAD_NUMBERS,
        ARG_SKETCH_PAD_NUMBERS_DESC,
    ))
    .arg(flag2(
        "hdnp",
        ARG_HIDE_DNP_FPS_ON_FAB_LAYERS,
        ARG_HIDE_DNP_FPS_ON_FAB_LAYERS_DESC,
    ))
    .arg(flag2(
        "sdnp",
        ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS,
        ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS_DESC,
    ))
    .arg(flag2(
        "cdnp",
        ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS,
        ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS_DESC,
    ))
}

/// `PCB_EXPORT_GERBER_COMMAND::PCB_EXPORT_GERBER_COMMAND( "gerbers", IO_TYPE::DIRECTORY )` +
/// `PCB_EXPORT_GERBERS_COMMAND::PCB_EXPORT_GERBERS_COMMAND()`.
pub fn command() -> Command {
    let cmd = Command::new("gerbers")
        .about(
            "Plot multiple Gerbers for a PCB, including the ability to use stored board plot \
             settings",
        )
        .arg(input_arg(IoType::File))
        .arg(output_arg(IoType::Directory))
        .arg(layer_arg())
        .arg(common_layers_arg())
        .arg(drawing_sheet_arg())
        .arg(define_arg())
        .arg(flag2(
            "erd",
            ARG_EXCLUDE_REFDES,
            "Exclude the reference designator text",
        ))
        .arg(flag2("ev", ARG_EXCLUDE_VALUE, "Exclude the value text"))
        .arg(flag2(
            "ibt",
            ARG_INCLUDE_BORDER_TITLE,
            "Include the border and title block",
        ));

    fab_layer_flags(cmd)
        .arg(flag(ARG_NO_X2, "Do not use the extended X2 format"))
        .arg(flag(ARG_NO_NETLIST, "Do not generate netlist attributes"))
        .arg(flag(
            ARG_SUBTRACT_SOLDERMASK,
            "Subtract soldermask from silkscreen",
        ))
        .arg(flag(ARG_DISABLE_APERTURE_MACROS, "Disable aperture macros"))
        .arg(flag(
            ARG_USE_DRILL_FILE_ORIGIN,
            "Use drill/place file origin",
        ))
        .arg(
            int_opt(
                ARG_PRECISION,
                "6",
                "Precision of Gerber coordinates, valid options: 5 or 6",
            )
            .value_name("PRECISION"),
        )
        .arg(flag(
            ARG_NO_PROTEL_EXTENSION,
            "Use KiCad Gerber file extension",
        ))
        .arg(flag(ARG_CHECK_ZONES, ARG_CHECK_ZONES_DESC))
        .arg(variants_arg())
        .arg(flag(
            ARG_USE_BOARD_PLOT_PARAMS,
            "Use the Gerber plot settings already configured in the board file",
        ))
}

/// The common arguments this command registers.
pub const COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: true,
    drawing_sheet: true,
    define: true,
    variant: true,
};

/// Port of `PCB_EXPORT_GERBER_COMMAND::populateJob`.
pub fn populate_job(m: &ArgMatches, args: &CommonArgs, job: &mut JobExportPcbGerber) -> i32 {
    let plot = &mut job.plot;

    plot.filename = args.input.clone();
    plot.job.set_configured_output_path(&args.output);
    plot.drawing_sheet = args.drawing_sheet.clone();
    plot.sketch_pads_on_fab_layers = get_flag(m, ARG_SKETCH_PADS_ON_FAB_LAYERS);
    plot.plot_pad_numbers = plot.sketch_pads_on_fab_layers && get_flag(m, ARG_SKETCH_PAD_NUMBERS);
    plot.hide_dnp_fps_on_fab_layers = get_flag(m, ARG_HIDE_DNP_FPS_ON_FAB_LAYERS);
    plot.sketch_dnp_fps_on_fab_layers = get_flag(m, ARG_SKETCH_DNP_FPS_ON_FAB_LAYERS);
    plot.crossout_dnp_fps_on_fab_layers = get_flag(m, ARG_CROSSOUT_DNP_FPS_ON_FAB_LAYERS);
    plot.job.set_var_overrides(args.define_vars.clone());

    if let Some(first) = args.variant_names.first() {
        plot.variant = first.clone();
    }

    plot.plot_footprint_values = !get_flag(m, ARG_EXCLUDE_VALUE);
    plot.plot_ref_des = !get_flag(m, ARG_EXCLUDE_REFDES);
    plot.plot_drawing_sheet = get_flag(m, ARG_INCLUDE_BORDER_TITLE);
    job.disable_aperture_macros = get_flag(m, ARG_DISABLE_APERTURE_MACROS);
    job.plot.subtract_solder_mask_from_silk = get_flag(m, ARG_SUBTRACT_SOLDERMASK);
    job.include_netlist_attributes = !get_flag(m, ARG_NO_NETLIST);
    job.use_x2_format = !get_flag(m, ARG_NO_X2);
    job.plot.use_drill_origin = get_flag(m, ARG_USE_DRILL_FILE_ORIGIN);
    job.use_protel_file_extension = !get_flag(m, ARG_NO_PROTEL_EXTENSION);
    job.precision = get_int(m, ARG_PRECISION);
    job.plot.check_zones_before_plot = get_flag(m, ARG_CHECK_ZONES);

    job.plot.arg_layers = Some(get_string(m, ARG_LAYERS));
    job.plot.arg_common_layers = Some(get_string(m, ARG_COMMON_LAYERS));

    if !Path::new(&job.plot.filename).is_file() {
        eprintln!("Board file does not exist or is not accessible");
        return exit_codes::ERR_INVALID_INPUT_FILE;
    }

    if job.precision != 5 && job.precision != 6 {
        eprintln!("Gerber coordinate precision should be either 5 or 6");
        return exit_codes::ERR_ARGS;
    }

    exit_codes::OK
}

/// `COMMAND::Perform` + `PCB_EXPORT_GERBERS_COMMAND::doPerform` + `KIWAY::ProcessJob`.
pub fn perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut gerber_job = JobExportPcbGerbers::new();
    let exit_code = populate_job(m, &args, &mut gerber_job.gerber);

    if exit_code != exit_codes::OK {
        return exit_code;
    }

    gerber_job.gerber.plot.arg_layers = Some(get_string(m, ARG_LAYERS));
    gerber_job.gerber.plot.arg_common_layers = Some(get_string(m, ARG_COMMON_LAYERS));
    gerber_job.use_board_plot_params = get_flag(m, ARG_USE_BOARD_PLOT_PARAMS);

    let mut session = match host::get_board(&gerber_job.gerber.plot.filename) {
        Ok(s) => s,
        Err(code) => return code,
    };

    // wxDateTime::Now() of the Gerber file attributes
    gerber_job.gerber.plot.creation_date = host::host_clock().gbr_creation_date();

    let mut reporter = Reporter::new();
    let result = run_export_gerbers(&mut session, &mut gerber_job, &mut reporter);
    host::print_log_warnings(&mut session);
    host::finish_job(result, reporter)
}
