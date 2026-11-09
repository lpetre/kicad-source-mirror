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

//! Port of `kicad/cli/command_pcb_upgrade.cpp` (`CLI::PCB_UPGRADE_COMMAND`) and
//! `kicad/cli/command_fp_upgrade.cpp` (`CLI::FP_UPGRADE_COMMAND`), with the filesystem half of
//! `PCBNEW_JOBS_HANDLER::JobExportFpUpgrade`.

use std::path::Path;

use clap::{ArgMatches, Command};
use kicad_core::Severity;
use kicad_session::jobs::{JobFpUpgrade, JobPcbUpgrade};
use kicad_session::{FootprintLibrary, Reporter, exit_codes, run_fp_upgrade, run_pcb_upgrade};

use crate::command::{CommonArgSet, CommonArgs, IoType, flag, get_flag, input_arg, output_arg};
use crate::host;

const ARG_FORCE: &str = "force";

/// `PCB_UPGRADE_COMMAND::PCB_UPGRADE_COMMAND()`.
pub fn pcb_command() -> Command {
    Command::new("upgrade")
        .about("Upgrade the board file's format to the latest one")
        .arg(input_arg(IoType::File))
        .arg(flag(
            ARG_FORCE,
            "Forces the board file to be resaved regardless of versioning",
        ))
}

/// The common arguments `pcb upgrade` registers.
pub const PCB_COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: false,
    drawing_sheet: false,
    define: false,
    variant: false,
};

/// `COMMAND::Perform` + `PCB_UPGRADE_COMMAND::doPerform` + `KIWAY::ProcessJob`.
pub fn pcb_perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, PCB_COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut upgrade_job = JobPcbUpgrade::new();
    upgrade_job.filename = args.input.clone();
    upgrade_job.force = get_flag(m, ARG_FORCE);

    if !Path::new(&upgrade_job.filename).is_file() {
        eprint!("Board file does not exist or is not accessible\n");
        return exit_codes::ERR_INVALID_INPUT_FILE;
    }

    let mut session = match host::get_board(&upgrade_job.filename) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let mut reporter = Reporter::new();
    let result = run_pcb_upgrade(&mut session, &mut upgrade_job, &mut reporter);
    host::finish_job(result, reporter)
}

/// `FP_UPGRADE_COMMAND::FP_UPGRADE_COMMAND()`.
pub fn fp_command() -> Command {
    Command::new("upgrade")
        .about("Upgrades the footprint library to the current kicad version format")
        .arg(input_arg(IoType::FileOrDirectory))
        .arg(output_arg(IoType::Directory))
        .arg(flag(
            ARG_FORCE,
            "Forces the footprint library to be resaved regardless of versioning",
        ))
}

/// The common arguments `fp upgrade` registers.
pub const FP_COMMON: CommonArgSet = CommonArgSet {
    input: true,
    output: true,
    drawing_sheet: false,
    define: false,
    variant: false,
};

/// `PCB_IO_MGR::GuessPluginTypeFromLibPath( path ) == KICAD_SEXP`: `IO_BASE::CanReadLibrary`
/// of the s-expression plugin, a directory holding at least one `*.kicad_mod` file
/// (case-insensitive extension). Nested library tables and the other plugins are not ported.
fn is_kicad_sexpr_library(path: &str) -> bool {
    let Ok(dir) = std::fs::read_dir(path) else {
        return false;
    };

    dir.flatten().any(|entry| {
        entry.file_type().map(|t| t.is_file()).unwrap_or(false)
            && entry
                .file_name()
                .to_string_lossy()
                .rsplit_once('.')
                .map(|(_, ext)| ext.eq_ignore_ascii_case("kicad_mod"))
                .unwrap_or(false)
    })
}

/// Read the `.kicad_mod` files of a `.pretty` directory, sorted by name.
fn read_library(path: &str) -> Option<FootprintLibrary> {
    let mut files: Vec<(String, String)> = Vec::new();

    for entry in std::fs::read_dir(path).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if name.to_lowercase().ends_with(".kicad_mod") {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            files.push((name, text));
        }
    }

    files.sort();
    Some(files)
}

/// `COMMAND::Perform` + `FP_UPGRADE_COMMAND::doPerform` + `JobExportFpUpgrade`.
pub fn fp_perform(m: &ArgMatches) -> i32 {
    let args = match CommonArgs::from_matches(m, FP_COMMON) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut fp_job = JobFpUpgrade::new();
    fp_job.library_path = args.input.clone();
    fp_job.output_library_path = args.output.clone();
    fp_job.force = get_flag(m, ARG_FORCE);

    let mut reporter = Reporter::new();
    let is_sexpr = is_kicad_sexpr_library(&fp_job.library_path);

    if !fp_job.output_library_path.is_empty() {
        if Path::new(&fp_job.output_library_path).exists() {
            reporter.report(
                "Output path must not conflict with existing path\n",
                Severity::Error,
            );
            host::print_report(reporter.lines());
            return exit_codes::ERR_INVALID_OUTPUT_CONFLICT;
        }
    } else if !is_sexpr {
        reporter.report(
            "Output path must be specified to convert legacy and non-KiCad libraries\n",
            Severity::Error,
        );
        host::print_report(reporter.lines());
        return exit_codes::ERR_INVALID_OUTPUT_CONFLICT;
    }

    if is_sexpr {
        if !Path::new(&fp_job.library_path).is_dir() {
            reporter.report(
                "Footprint library path does not exist or is not accessible\n",
                Severity::Error,
            );
            host::print_report(reporter.lines());
            return exit_codes::ERR_INVALID_INPUT_FILE;
        }

        let Some(library) = read_library(&fp_job.library_path) else {
            reporter.report("Unable to load library\n", Severity::Error);
            host::print_report(reporter.lines());
            return exit_codes::ERR_UNKNOWN;
        };

        let result = run_fp_upgrade(&library, &mut fp_job, &mut reporter);
        return host::finish_job(result, reporter);
    }

    // PCB_IO_MGR::ConvertLibrary for legacy and non-KiCad libraries
    host::print_report(reporter.lines());
    host::report_session_error(&kicad_session::SessionError::not_yet_ported(
        "kicad-pcb: PCB_IO_MGR::ConvertLibrary (legacy and non-KiCad footprint libraries)",
    ))
}
