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

//! Port of `kicad/cli/command_pcb_drc.cpp` (`CLI::PCB_DRC_COMMAND`).

use clap::{ArgMatches, Command};
use kicad_core::Severity;
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat, RcUnits};
use kicad_session::{Reporter, exit_codes, run_drc};

use crate::command::{
    CommonArgSet, CommonArgs, IoType, define_arg, flag, get_flag, get_string, input_arg,
    output_arg, string_opt,
};
use crate::host;

const ARG_FORMAT: &str = "format";
const ARG_ALL_TRACK_ERRORS: &str = "all-track-errors";
const ARG_UNITS: &str = "units";
const ARG_SEVERITY_ALL: &str = "severity-all";
const ARG_SEVERITY_ERROR: &str = "severity-error";
const ARG_SEVERITY_WARNING: &str = "severity-warning";
const ARG_SEVERITY_EXCLUSIONS: &str = "severity-exclusions";
const ARG_EXIT_CODE_VIOLATIONS: &str = "exit-code-violations";
const ARG_PARITY: &str = "schematic-parity";
const ARG_ZONE_FILL: &str = "refill-zones";
const ARG_SAVE_BOARD: &str = "save-board";

/// `PCB_DRC_COMMAND::PCB_DRC_COMMAND()`.
pub fn command() -> Command {
    Command::new("drc")
        .about("Runs the Design Rules Check (DRC) on the PCB and creates a report")
        .arg(input_arg(IoType::File))
        .arg(output_arg(IoType::File))
        .arg(define_arg())
        .arg(
            string_opt(
                ARG_FORMAT,
                "report",
                "Output file format, options: json, report",
            )
            .value_name("FORMAT"),
        )
        .arg(flag(
            ARG_ALL_TRACK_ERRORS,
            "Report all errors for each track",
        ))
        .arg(flag(
            ARG_PARITY,
            "Test for parity between PCB and schematic",
        ))
        .arg(
            string_opt(ARG_UNITS, "mm", "Report units; valid options: in, mm, mils")
                .value_name("UNITS"),
        )
        .arg(flag(
            ARG_SEVERITY_ALL,
            "Report all DRC violations, this is equivalent to including all the other severity \
             arguments",
        ))
        .arg(flag(
            ARG_SEVERITY_ERROR,
            "Report all DRC error level violations, this can be combined with the other \
             severity arguments",
        ))
        .arg(flag(
            ARG_SEVERITY_WARNING,
            "Report all DRC warning level violations, this can be combined with the other \
             severity arguments",
        ))
        .arg(flag(
            ARG_SEVERITY_EXCLUSIONS,
            "Report all excluded DRC violations, this can be combined with the other severity \
             arguments",
        ))
        .arg(flag(
            ARG_EXIT_CODE_VIOLATIONS,
            "Return a nonzero exit code if DRC violations exist",
        ))
        .arg(flag(ARG_ZONE_FILL, "Refill zones before running DRC"))
        .arg(flag(
            ARG_SAVE_BOARD,
            "Save the board after DRC, must be used with --refill-zones",
        ))
}

/// The common arguments this command registers.
pub const COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: true,
    drawing_sheet: false,
    define: true,
    variant: false,
};

/// Port of `PCB_DRC_COMMAND::doPerform`: fill a `JOB_PCB_DRC` from the arguments.
/// `Err( exit code )` after an argument error message.
pub fn build_job(m: &ArgMatches, args: &CommonArgs) -> Result<JobPcbDrc, i32> {
    let mut drc_job = JobPcbDrc::new();

    drc_job.rc.job.set_configured_output_path(&args.output);
    drc_job.rc.filename = args.input.clone();
    drc_job.rc.job.set_var_overrides(args.define_vars.clone());
    drc_job.report_all_track_errors = get_flag(m, ARG_ALL_TRACK_ERRORS);
    drc_job.rc.exit_code_violations = get_flag(m, ARG_EXIT_CODE_VIOLATIONS);

    let mut severity = 0;

    if get_flag(m, ARG_SEVERITY_ALL) {
        severity =
            Severity::Error.as_i32() | Severity::Warning.as_i32() | Severity::Exclusion.as_i32();
    }

    if get_flag(m, ARG_SEVERITY_ERROR) {
        severity |= Severity::Error.as_i32();
    }

    if get_flag(m, ARG_SEVERITY_WARNING) {
        severity |= Severity::Warning.as_i32();
    }

    if get_flag(m, ARG_SEVERITY_EXCLUSIONS) {
        severity |= Severity::Exclusion.as_i32();
    }

    // override the default only if something we configured
    if severity != 0 {
        drc_job.rc.severity = severity;
    }

    drc_job.report_all_track_errors = get_flag(m, ARG_ALL_TRACK_ERRORS);

    let units = get_string(m, ARG_UNITS);

    if units == "mm" {
        drc_job.rc.units = RcUnits::Mm;
    } else if units == "in" {
        drc_job.rc.units = RcUnits::Inch;
    } else if units == "mils" {
        drc_job.rc.units = RcUnits::Mils;
    } else if !units.is_empty() {
        eprintln!("Invalid units specified");
        return Err(exit_codes::ERR_ARGS);
    }

    let format = get_string(m, ARG_FORMAT);

    if format == "report" {
        drc_job.rc.format = RcOutputFormat::Report;
    } else if format == "json" {
        drc_job.rc.format = RcOutputFormat::Json;
    } else {
        eprintln!("Invalid report format");
        return Err(exit_codes::ERR_ARGS);
    }

    drc_job.parity = get_flag(m, ARG_PARITY);
    drc_job.refill_zones = get_flag(m, ARG_ZONE_FILL);
    drc_job.save_board = get_flag(m, ARG_SAVE_BOARD);

    Ok(drc_job)
}

/// `COMMAND::Perform` + `PCB_DRC_COMMAND::doPerform` + `KIWAY::ProcessJob`.
pub fn perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut job = match build_job(m, &args) {
        Ok(j) => j,
        Err(code) => return code,
    };

    let mut session = match host::get_board(&job.rc.filename) {
        Ok(s) => s,
        Err(code) => return code,
    };

    // Host inputs of JobExportDrc (the C++ reads them from the clock / filesystem)
    job.report_date = host::host_clock().iso8601();
    job.schematic_exists = host::schematic_exists(&job.rc.filename);
    job.marker_uuid_seed = host::uuid_seed();

    let mut reporter = Reporter::new();
    let result = run_drc(&mut session, &mut job, &mut reporter);
    host::print_log_warnings(&mut session);
    host::finish_job(result, reporter)
}
