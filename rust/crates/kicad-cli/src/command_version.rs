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

//! Port of `kicad/cli/command_version.cpp` (`CLI::VERSION_COMMAND`).

use clap::{ArgMatches, Command};
use kicad_session::{build_version, exit_codes};

use crate::command::{get_string, string_opt};

/// `ARG_FORMAT`
const ARG_FORMAT: &str = "format";

/// `VERSION_COMMAND::VERSION_COMMAND()`.
pub fn command() -> Command {
    Command::new("version")
        .about("Reports the version info in various formats")
        .arg(
            string_opt(
                ARG_FORMAT,
                "plain",
                "version info format (plain, commit, about)",
            )
            .value_name("VAR"),
        )
}

/// `VERSION_COMMAND::doPerform`; `m` is `None` when redirected from `--version`.
pub fn perform(m: Option<&ArgMatches>) -> i32 {
    let Some(m) = m else {
        // were we redirected from the --version?
        println!("{}", build_version::major_minor_patch_version());
        return exit_codes::OK;
    };

    let format = get_string(m, ARG_FORMAT);

    if format == "plain" {
        println!("{}", build_version::major_minor_patch_version());
    } else if format == "commit" {
        println!("{}", build_version::commit_hash());
    } else if format == "about" {
        println!("{}", build_version::version_info_data("kicad-rs"));
    } else {
        eprint!("Invalid format\n");
        return exit_codes::ERR_ARGS;
    }

    exit_codes::OK
}
