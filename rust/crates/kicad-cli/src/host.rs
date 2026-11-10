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
use kicad_session::{
    HostClock, JobOutcome, LoadInputs, ReportLine, Reporter, Session, SessionError, exit_codes,
};

/// Replace the extension of `path` (`wxFileName::SetExt`).
fn with_extension(path: &str, ext: &str) -> String {
    let p = Path::new(path);
    p.with_extension(ext).to_string_lossy().into_owned()
}

/// `CLI_REPORTER::Report` for every recorded message: `RPT_SEVERITY_ERROR` to stderr, the
/// others to stdout, debug messages dropped (not verbose).
/// Port of `CLI_REPORTER::Report` for the recorded lines: errors to stderr, the rest to
/// stdout, debug messages skipped (no verbose mode), and a `\n` appended to a message that
/// does not end with one.
pub fn print_report(lines: &[ReportLine]) {
    for line in lines {
        let nl = if line.text.ends_with('\n') { "" } else { "\n" };

        match line.severity {
            Severity::Error => eprint!("{}{nl}", line.text),
            Severity::Debug => {}
            _ => print!("{}{nl}", line.text),
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
/// `wxFileName::MakeAbsolute()` (Unix): `path` against the working directory, `.` and `..`
/// components normalised (`wxPATH_NORM_DOTS | wxPATH_NORM_ABSOLUTE`).
pub fn make_absolute(path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => format!("{}/{}", cwd.to_string_lossy(), path),
            Err(_) => return path.to_string(),
        }
    };

    let mut parts: Vec<&str> = Vec::new();

    for comp in joined.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }

    format!("/{}", parts.join("/"))
}

/// Environment variable: a fixed `YYYY-MM-DDTHH:MM:SS` for every date the jobs write (DRC
/// report date, Gerber / drill creation dates, `CURRENT_DATE` / `CURRENT_TIME_*`), for
/// reproducible output. Not in the reference (it always reads the clock).
pub const ENV_FIXED_DATE: &str = "KICAD_RS_DATE";

/// Environment variable: a fixed seed (decimal `u32`) for the UUIDs a run generates (new
/// items, DRC markers). Not in the reference (its generator is seeded from the OS).
pub const ENV_UUID_SEED: &str = "KICAD_RS_UUID_SEED";

/// `wxDateTime::Now()`: the fixed date of [`ENV_FIXED_DATE`], else the system clock. The
/// local time zone is not queried (no libc access): local time is UTC.
pub fn host_clock() -> HostClock {
    if let Ok(text) = std::env::var(ENV_FIXED_DATE) {
        if let Some(clock) = HostClock::parse_iso(&text) {
            return clock;
        }

        eprintln!("kicad-rs: ignoring {ENV_FIXED_DATE}='{text}' (expected YYYY-MM-DDTHH:MM:SS)");
    }

    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);

    HostClock::from_unix_seconds(secs, 0)
}

/// The UUID seed of the run: [`ENV_UUID_SEED`], else 32 bits of OS randomness
/// (`/dev/urandom`; the clock when that cannot be read).
pub fn uuid_seed() -> u32 {
    if let Ok(text) = std::env::var(ENV_UUID_SEED) {
        if let Ok(seed) = text.trim().parse::<u32>() {
            return seed;
        }

        eprintln!("kicad-rs: ignoring {ENV_UUID_SEED}='{text}' (expected a decimal u32)");
    }

    use std::io::Read;
    let mut bytes = [0u8; 4];

    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok()
    {
        return u32::from_le_bytes(bytes);
    }

    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() ^ (d.as_secs() as u32))
}

/// `JobExportDrc`'s schematic lookup: `<board>.kicad_sch`, else `<board>.sch`, made absolute,
/// exists.
pub fn schematic_exists(board_path: &str) -> bool {
    let abs = make_absolute(board_path);
    Path::new(&with_extension(&abs, "kicad_sch")).exists()
        || Path::new(&with_extension(&abs, "sch")).exists()
}

/// Port of `PCBNEW_JOBS_HANDLER::getBoard( aPath )` for the host: reads the board, its
/// project (`getProjectForBoard`: `<board>.kicad_pro` made absolute, plus the `.kicad_prl`)
/// and rule file, loads it (`BOARD_LOADER::Load`), then applies the host inputs (UUID seed,
/// clock) and prints the outline font fallback warnings on stderr.
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

    // getProjectForBoard: <board>.kicad_pro made absolute, loaded (or defaulted) by the
    // settings manager, with its .kicad_prl
    let project_path = make_absolute(&with_extension(path, "kicad_pro"));
    let project = std::fs::read_to_string(&project_path).ok();
    let local_settings = std::fs::read_to_string(with_extension(&project_path, "kicad_prl")).ok();
    // InitEngine( <board>.kicad_dru )
    let rules = std::fs::read_to_string(with_extension(path, "kicad_dru")).ok();

    let inputs = LoadInputs {
        text: &text,
        source_name: path,
        project_path: Some(&project_path),
        project_json: project.as_deref(),
        local_settings_json: local_settings.as_deref(),
        rules_text: rules.as_deref(),
        ..LoadInputs::default()
    };

    match Session::load_board_with(&inputs) {
        Ok(mut session) => {
            session.apply_host_inputs(&host_clock(), uuid_seed());

            for warning in session.outline_font_warnings() {
                eprintln!("Warning: {warning}");
            }

            Ok(session)
        }
        Err(err) => Err(report_session_error(&err)),
    }
}

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
