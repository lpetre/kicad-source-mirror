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

//! Port of `kicad/cli/command_pcb_export_drill.cpp` (`CLI::PCB_EXPORT_DRILL_COMMAND`).

use clap::{ArgMatches, Command};
use kicad_session::jobs::{
    DrillFormat, DrillOrigin, DrillUnits, JobExportPcbDrill, MapFormat, ZerosFormat,
};
use kicad_session::{Reporter, exit_codes, run_export_drill};

use crate::command::{
    CommonArgSet, CommonArgs, IoType, flag, get_flag, get_int, get_string, input_arg, int_opt,
    output_arg, string_opt,
};
use crate::host;

const ARG_FORMAT: &str = "format";
const ARG_EXCELLON_MIRRORY: &str = "excellon-mirror-y";
const ARG_EXCELLON_MINIMALHEAD: &str = "excellon-min-header";
const ARG_EXCELLON_SEPARATE_TH: &str = "excellon-separate-th";
const ARG_EXCELLON_ZEROS_FORMAT: &str = "excellon-zeros-format";
const ARG_EXCELLON_OVAL_FORMAT: &str = "excellon-oval-format";
const ARG_GERBER_PRECISION: &str = "gerber-precision";
const ARG_EXCELLON_UNITS: &str = "excellon-units";
const ARG_GENERATE_MAP: &str = "generate-map";
const ARG_GENERATE_REPORT: &str = "generate-report";
const ARG_REPORT_PATH: &str = "report-path";
const ARG_GENERATE_TENTING: &str = "generate-tenting";
const ARG_MAP_FORMAT: &str = "map-format";
const ARG_DRILL_ORIGIN: &str = "drill-origin";

/// `PCB_EXPORT_DRILL_COMMAND::PCB_EXPORT_DRILL_COMMAND()`.
pub fn command() -> Command {
    Command::new("drill")
        .about("Generate Drill Files")
        .arg(input_arg(IoType::File))
        .arg(output_arg(IoType::Directory))
        .arg(
            string_opt(ARG_FORMAT, "excellon", "Valid options excellon, gerber.")
                .value_name("FORMAT"),
        )
        .arg(
            string_opt(ARG_DRILL_ORIGIN, "absolute", "Valid options are: absolute, plot")
                .value_name("DRILL_ORIGIN"),
        )
        .arg(
            string_opt(
                ARG_EXCELLON_ZEROS_FORMAT,
                "decimal",
                "Valid options are: decimal, suppressleading, suppresstrailing, keep.",
            )
            .value_name("ZEROS_FORMAT"),
        )
        .arg(
            string_opt(
                ARG_EXCELLON_OVAL_FORMAT,
                "alternate",
                "Valid options are: route, alternate.",
            )
            .value_name("OVAL_FORMAT"),
        )
        .arg(
            string_opt(ARG_EXCELLON_UNITS, "mm", "Output units, valid options: in, mm")
                .short('u')
                .value_name("UNITS"),
        )
        .arg(flag(ARG_EXCELLON_MIRRORY, "Mirror Y axis"))
        .arg(flag(ARG_EXCELLON_MINIMALHEAD, "Minimal header"))
        .arg(flag(
            ARG_EXCELLON_SEPARATE_TH,
            "Generate independent files for NPTH and PTH holes",
        ))
        .arg(flag(ARG_GENERATE_MAP, "Generate map / summary of drill hits"))
        .arg(flag(ARG_GENERATE_REPORT, "Generate report of drill hits"))
        .arg(string_opt(ARG_REPORT_PATH, "", "Report output file path").value_name("REPORT_FILE"))
        .arg(flag(ARG_GENERATE_TENTING, "Generate a file specifically for tenting"))
        .arg(
            string_opt(ARG_MAP_FORMAT, "pdf", "Valid options: pdf, gerberx2, ps, dxf, svg")
                .value_name("MAP_FORMAT"),
        )
        .arg(
            int_opt(ARG_GERBER_PRECISION, "6", "Precision of Gerber coordinates (5 or 6)")
                .value_name("VAR"),
        )
}

/// The common arguments this command registers.
pub const COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: true,
    drawing_sheet: false,
    define: false,
    variant: false,
};

/// Port of `PCB_EXPORT_DRILL_COMMAND::doPerform` up to `ProcessJob`.
pub fn build_job(m: &ArgMatches, args: &CommonArgs) -> Result<JobExportPcbDrill, i32> {
    let mut drill_job = JobExportPcbDrill::new();

    drill_job.filename = args.input.clone();
    drill_job.job.set_configured_output_path(&args.output);

    // The C++ checks `wxFileName( path, wxEmptyString ).IsDir()`, which is true for any path
    // (the whole path is taken as the directory part), so "Output must be a directory" is never
    // printed; a path like "out.drl" becomes a directory.

    let format = get_string(m, ARG_FORMAT);

    if format == "excellon" {
        drill_job.format = DrillFormat::Excellon;
    } else if format == "gerber" {
        drill_job.format = DrillFormat::Gerber;
    } else {
        eprint!("Invalid drill format\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let units = get_string(m, ARG_EXCELLON_UNITS);

    if units == "mm" {
        drill_job.drill_units = DrillUnits::Mm;
    } else if units == "in" {
        drill_job.drill_units = DrillUnits::Inch;
    } else {
        eprint!("Invalid units specified\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let zero_format = get_string(m, ARG_EXCELLON_ZEROS_FORMAT);

    if zero_format == "decimal" {
        drill_job.zero_format = ZerosFormat::Decimal;
    } else if zero_format == "suppressleading" {
        drill_job.zero_format = ZerosFormat::SuppressLeading;
    } else if zero_format == "suppresstrailing" {
        drill_job.zero_format = ZerosFormat::SuppressTrailing;
    } else if zero_format == "keep" {
        drill_job.zero_format = ZerosFormat::KeepZeros;
    } else {
        eprint!("Invalid zeros format specified\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let drill_format = get_string(m, ARG_EXCELLON_OVAL_FORMAT);

    if drill_format == "route" {
        drill_job.excellon_oval_drill_route = true;
    } else if drill_format == "alternate" {
        drill_job.excellon_oval_drill_route = false;
    } else {
        eprint!("Invalid oval drill format specified\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let map_format = get_string(m, ARG_MAP_FORMAT);

    if map_format == "pdf" {
        drill_job.map_format = MapFormat::Pdf;
    } else if map_format == "ps" {
        drill_job.map_format = MapFormat::Postscript;
    } else if map_format == "gerberx2" {
        drill_job.map_format = MapFormat::GerberX2;
    } else if map_format == "dxf" {
        drill_job.map_format = MapFormat::Dxf;
    } else if map_format == "svg" {
        drill_job.map_format = MapFormat::Svg;
    } else {
        eprint!("Invalid map format specified\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let origin = get_string(m, ARG_DRILL_ORIGIN);

    if origin == "absolute" {
        drill_job.drill_origin = DrillOrigin::Abs;
    } else if origin == "plot" {
        drill_job.drill_origin = DrillOrigin::Plot;
    } else {
        eprint!("Invalid origin mode specified\n");
        return Err(exit_codes::ERR_ARGS);
    }

    drill_job.excellon_mirror_y = get_flag(m, ARG_EXCELLON_MIRRORY);
    drill_job.excellon_minimal_header = get_flag(m, ARG_EXCELLON_MINIMALHEAD);
    drill_job.excellon_combine_pth_npth = !get_flag(m, ARG_EXCELLON_SEPARATE_TH);
    drill_job.generate_map = get_flag(m, ARG_GENERATE_MAP);
    drill_job.generate_report = get_flag(m, ARG_GENERATE_REPORT);
    drill_job.generate_tenting = get_flag(m, ARG_GENERATE_TENTING);
    drill_job.gerber_precision = get_int(m, ARG_GERBER_PRECISION);

    if drill_job.gerber_precision != 5 && drill_job.gerber_precision != 6 {
        eprint!("Gerber coordinate precision should be either 5 or 6\n");
        return Err(exit_codes::ERR_ARGS);
    }

    let report_path = get_string(m, ARG_REPORT_PATH);
    drill_job.report_path = report_path.clone();

    if drill_job.generate_report {
        drill_job.report_path = report_path;
    } else if !report_path.is_empty() {
        eprint!(
            "Warning: Report path supplied without --generate-report, no report will be \
             generated\n"
        );
    }

    Ok(drill_job)
}

/// `COMMAND::Perform` + `PCB_EXPORT_DRILL_COMMAND::doPerform` + `KIWAY::ProcessJob`.
pub fn perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut job = match build_job(m, &args) {
        Ok(j) => j,
        Err(code) => return code,
    };

    let mut session = match host::get_board(&job.filename) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let mut reporter = Reporter::new();
    let result = run_export_drill(&mut session, &mut job, &mut reporter);
    host::finish_job(result, reporter)
}
