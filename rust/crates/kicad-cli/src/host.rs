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

//! The host side of `KIWAY::ProcessJob( KIWAY::FACE_PCB, job )`: everything the C++ jobs
//! handler does with the filesystem and the terminal, around the pure `kicad_session` runners.
//!
//! * `getBoard( aPath )` (`pcbnew_jobs_handler.cpp` L540-629): find the `.kicad_pro` next to
//!   the board, pick the plugin (`FindPluginTypeFromBoardPath`), read the board and the
//!   `.kicad_dru` (`BOARD::GetDesignRulesPath()`), load.
//! * `CLI_REPORTER::Report` (`common/reporter.cpp`): errors to stderr, the rest to stdout.
//! * writing the produced files (`PATHS::EnsurePathExists` + the writers' file output).

use std::path::Path;

use kicad_core::Severity;
use kicad_session::session::can_read_board;
use kicad_session::{JobOutcome, ReportLine, Reporter, Session, SessionError, exit_codes};

/// Replace the extension of `path` (`wxFileName::SetExt`).
fn with_extension(path: &str, ext: &str) -> String {
    let p = Path::new(path);
    p.with_extension(ext).to_string_lossy().into_owned()
}

/// `CLI_REPORTER::Report` for every recorded message: `RPT_SEVERITY_ERROR` to stderr, the
/// others to stdout, debug messages dropped (not verbose).
pub fn print_report(lines: &[ReportLine]) {
    for line in lines {
        match line.severity {
            Severity::Error => eprint!("{}", line.text),
            Severity::Debug => {}
            _ => print!("{}", line.text),
        }
    }
}

/// Print a `NotYetPorted` (or other session) error the way `kicad-rs` reports it and return
/// the exit code to use.
pub fn report_session_error(err: &SessionError) -> i32 {
    match err {
        SessionError::NotYetPorted { subsystem } => {
            eprintln!("kicad-rs: not yet ported: {subsystem}");
            exit_codes::NOT_YET_PORTED
        }
        SessionError::Load { message } => {
            if message.is_empty() {
                eprintln!("Failed to load board");
            } else {
                eprintln!("Failed to load board: {message}");
            }
            exit_codes::ERR_INVALID_INPUT_FILE
        }
        other => {
            eprintln!("kicad-rs: {other}");
            exit_codes::ERR_UNKNOWN
        }
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::getBoard( aPath )` for the CLI: `Err( exit code )` after the
/// message has been printed.
pub fn get_board(path: &str) -> Result<Session, i32> {
    let fail = |message: &str| -> i32 {
        report_session_error(&SessionError::Load {
            message: message.to_string(),
        })
    };

    // PCB_IO_MGR::FindPluginTypeFromBoardPath( aBoardPath, KICTL_KICAD_ONLY )
    let lower = path.to_lowercase();

    if lower.ends_with(".brd") {
        // PCB_IO_MGR::LEGACY
        return Err(report_session_error(&SessionError::not_yet_ported(
            "kicad-pcb: legacy .brd board reader (PCB_IO_KICAD_LEGACY)",
        )));
    }

    let Ok(bytes) = std::fs::read(path) else {
        return Err(fail(""));
    };

    let text = String::from_utf8_lossy(&bytes).into_owned();

    if !can_read_board(path, &text) {
        return Err(fail(""));
    }

    // getProjectForBoard: <board>.kicad_pro, loaded (or defaulted) by the settings manager
    let project = std::fs::read_to_string(with_extension(path, "kicad_pro")).ok();
    // BOARD::GetDesignRulesPath(): the project path with the .kicad_dru extension
    let rules = std::fs::read_to_string(with_extension(path, "kicad_dru")).ok();

    match Session::load_board(&text, path, project.as_deref(), rules.as_deref()) {
        Ok(session) => Ok(session),
        Err(err) => Err(report_session_error(&err)),
    }
}

/// Create the parent directory of `file` (`PATHS::EnsurePathExists`).
fn ensure_parent_dir(file: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(file).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    Ok(())
}

/// Print the messages of a run, write its files and return its exit code.
pub fn finish_job(result: Result<JobOutcome, SessionError>, reporter: Reporter) -> i32 {
    print_report(reporter.lines());

    match result {
        Ok(outcome) => {
            for file in &outcome.files {
                if ensure_parent_dir(&file.name).is_err() {
                    eprintln!("Failed to create output directory");
                    return exit_codes::ERR_INVALID_OUTPUT_CONFLICT;
                }

                if std::fs::write(&file.name, &file.contents).is_err() {
                    eprintln!("Failed to write '{}'.", file.name);
                    return exit_codes::ERR_INVALID_OUTPUT_CONFLICT;
                }
            }

            outcome.exit_code
        }
        Err(err) => report_session_error(&err),
    }
}
