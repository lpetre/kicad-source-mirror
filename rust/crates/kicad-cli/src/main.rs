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

//! `kicad-rs`: port of `kicad/kicad_cli.cpp` (`PGM_KICAD::OnPgmRun`) for the commands the Rust
//! port supports: `version`, `pcb drc`, `pcb export gerbers`, `pcb export drill`,
//! `pcb export svg`, `pcb upgrade`, `fp upgrade`.
//!
//! Options, defaults, validation, messages and exit codes follow `kicad/cli/command_*.cpp`
//! (see `PORT_STATUS.md` for the differences: clap's help layout, the commands that are not
//! ported). The binary does the file I/O; the jobs run in `kicad_session`.

mod argparse_compat;
mod command;
mod command_pcb_drc;
mod command_pcb_export_drill;
mod command_pcb_export_gerbers;
mod command_pcb_export_svg;
mod command_upgrade;
mod command_version;
mod host;

use clap::{Arg, ArgAction, Command};
use kicad_session::exit_codes;

/// The `commandStack` of `kicad_cli.cpp`, restricted to the ported commands.
fn build_cli() -> Command {
    Command::new("kicad-rs")
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .arg(
            Arg::new("version")
                .short('v')
                .long("version")
                .help("prints version information and exits")
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("fp")
                .about("Footprint and Footprint Libraries")
                .disable_help_subcommand(true)
                .subcommand(command_upgrade::fp_command()),
        )
        .subcommand(
            Command::new("pcb")
                .about("PCB")
                .disable_help_subcommand(true)
                .subcommand(command_pcb_drc::command())
                .subcommand(
                    Command::new("export")
                        .about("Export utilities (Gerbers, drill, position files, etc)")
                        .disable_help_subcommand(true)
                        .subcommand(command_pcb_export_drill::command())
                        .subcommand(command_pcb_export_gerbers::command())
                        .subcommand(command_pcb_export_svg::command()),
                )
                .subcommand(command_upgrade::pcb_command()),
        )
        .subcommand(command_version::command())
}

/// The command path the arguments reach (`recurseArgParserSubCommandUsed`): subcommand names
/// in order, stopping at the first word that is not one.
fn command_path(root: &Command, args: &[String]) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = root;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            continue;
        }

        match current.find_subcommand(arg) {
            Some(sub) => {
                path.push(arg.clone());
                current = sub;
            }
            None => break,
        }
    }

    path
}

/// The command at `path` below `root`.
fn command_at<'a>(root: &'a Command, path: &[String]) -> &'a Command {
    let mut current = root;

    for name in path {
        if let Some(sub) = current.find_subcommand(name) {
            current = sub;
        }
    }

    current
}

/// `COMMAND::PrintHelp()` / `printHelp( argParser )`: the help of the command at `path`.
fn print_help(root: &Command, path: &[String]) {
    let mut root = root.clone();
    root.build();
    let cmd = command_at(&root, path);
    print!("{}", cmd.clone().render_help());
}

/// The usage text of the first option of `cmd` that takes a value (argparse's
/// `get_usage_full()`, e.g. `--format VAR`).
fn first_value_option(cmd: &Command) -> Option<String> {
    cmd.get_arguments()
        .find(|a| {
            a.get_long().is_some() && a.get_num_args().map(|n| n.takes_values()).unwrap_or(false)
        })
        .map(|a| {
            let value = a
                .get_value_names()
                .and_then(|v| v.first())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "VAR".to_string());
            format!("--{} {}", a.get_long().unwrap_or(""), value)
        })
}

/// Port of `PGM_KICAD::OnPgmRun`.
fn run(args: Vec<String>) -> i32 {
    let root = build_cli();

    let matches = match root.clone().try_get_matches_from(&args) {
        Ok(m) => m,
        Err(err) => {
            let requested_help = args.iter().skip(1).any(|a| a == "-h" || a == "--help");
            let path = command_path(&root, &args);

            if requested_help {
                print_help(&root, &path);
                return 0;
            }

            let mut built = root.clone();
            built.build();
            let cmd = command_at(&built, &path);
            let subcommands: Vec<&str> = cmd.get_subcommands().map(|c| c.get_name()).collect();
            let takes_input = cmd
                .get_arguments()
                .any(|a| a.get_id() == command::ARG_INPUT);
            let first_opt = first_value_option(cmd);

            println!(
                "{}",
                argparse_compat::argparse_message(
                    &err,
                    &subcommands,
                    takes_input,
                    first_opt.as_deref()
                )
            );
            print_help(&root, &path);
            return exit_codes::ERR_ARGS;
        }
    };

    // the version arg gets redirected to the version subcommand
    if matches.get_flag("version") {
        return command_version::perform(None);
    }

    let path = command_path(&root, &args);

    match matches.subcommand() {
        Some(("version", m)) => command_version::perform(Some(m)),
        Some(("fp", m)) => match m.subcommand() {
            Some(("upgrade", m)) => command_upgrade::fp_perform(m),
            _ => {
                // COMMAND::doPerform: print the help
                print_help(&root, &path);
                exit_codes::OK
            }
        },
        Some(("pcb", m)) => match m.subcommand() {
            Some(("drc", m)) => command_pcb_drc::perform(m),
            Some(("upgrade", m)) => command_upgrade::pcb_perform(m),
            Some(("export", m)) => match m.subcommand() {
                Some(("drill", m)) => command_pcb_export_drill::perform(m),
                Some(("gerbers", m)) => command_pcb_export_gerbers::perform(m),
                Some(("svg", m)) => command_pcb_export_svg::perform(m),
                _ => {
                    print_help(&root, &path);
                    exit_codes::OK
                }
            },
            _ => {
                print_help(&root, &path);
                exit_codes::OK
            }
        },
        _ => {
            print_help(&root, &path);
            exit_codes::ERR_ARGS
        }
    }
}

fn main() {
    let code = run(std::env::args().collect());
    std::process::exit(code);
}
