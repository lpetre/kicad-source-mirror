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

//! `pcb-dump-rs`: prints the Rust board model in the line format of the C++ `pcb-dump` tool
//! (`rust/tools/pcb-dump/main.cpp`, `docs/PCB_DUMP_FORMAT.md`).
//!
//! ```text
//! pcb-dump-rs defaults [--hash]
//! pcb-dump-rs board [--sections LIST] [--hash] FILE...
//! ```

use std::io::Write;
use std::path::Path;

use kicad_pcb::dump::{
    DumpLoadMode, DumpOptions, dump_board, dump_defaults, load_board_for_dump_mode, scan_file_uuids,
};

fn usage() -> std::process::ExitCode {
    eprintln!(
        "usage: pcb-dump-rs defaults [--hash]\n       pcb-dump-rs props-table [--hash]\n       pcb-dump-rs board [--sections LIST] [--hash] [--loader | --no-init] FILE..."
    );
    std::process::ExitCode::from(2)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        return usage();
    };
    let mut opts = DumpOptions::default();
    let mut files = Vec::new();
    let mut mode = DumpLoadMode::Qa;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--hash" => opts.hash = true,
            "--loader" => mode = DumpLoadMode::Loader,
            "--no-init" => mode = DumpLoadMode::NoInit,
            "--sections" if i + 1 < args.len() => {
                i += 1;
                opts.sections = args[i]
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            a if a.starts_with("--") => return usage(),
            f => files.push(f.to_string()),
        }

        i += 1;
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    match cmd.as_str() {
        "props-table" => {
            let _ = out.write_all(kicad_pcb::dump::dump_props_table(opts.hash).as_bytes());
            std::process::ExitCode::SUCCESS
        }
        "defaults" => {
            let _ = out.write_all(dump_defaults(opts.hash).as_bytes());
            std::process::ExitCode::SUCCESS
        }
        "board" if !files.is_empty() => {
            let mut failures = 0;

            for file in &files {
                let path = Path::new(file);
                let name = path
                    .file_name()
                    .map_or_else(|| file.clone(), |n| n.to_string_lossy().into_owned());
                let _ = writeln!(out, "file|{name}|");

                let board = match load_board_for_dump_mode(path, mode) {
                    Ok((b, warnings)) => {
                        for w in warnings {
                            let _ = writeln!(out, "warn|load|{name}|{}", w.replace('\n', " "));
                        }
                        b
                    }
                    Err(e) => {
                        let _ = writeln!(out, "error|load|{name}|{}", e.replace('\n', " "));
                        failures += 1;
                        continue;
                    }
                };

                let mut o = opts.clone();
                o.file_uuids = Some(
                    std::fs::read(path)
                        .map(|t| scan_file_uuids(&t))
                        .unwrap_or_default(),
                );
                let _ = out.write_all(dump_board(&board, &o).as_bytes());
            }

            if failures > 0 {
                std::process::ExitCode::from(1)
            } else {
                std::process::ExitCode::SUCCESS
            }
        }
        _ => usage(),
    }
}
