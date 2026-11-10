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

//! `pcb-dump-session`: `pcb-dump-rs board` (`kicad_pcb::dump`) on boards loaded through the
//! session's two entry points, so that the complete load sequences (with the DRC engine,
//! exclusions and connectivity, which `kicad-pcb` cannot run) are compared with the reference
//! `pcb-dump` (`rust/tools/pcb-dump/main.cpp`: `loadBoardQa`, `loadBoardCli`). Test support
//! for package PCBIO-15 (new code).
//!
//! ```text
//! pcb-dump-session board [--loader] [--sections LIST] [--hash] [--strict] FILE...
//! ```
//!
//! Default: `Session::load_board_qa` (`KI_TEST::LoadBoard`, the reference tool's default);
//! `--loader`: `Session::load_board_with` (`BOARD_LOADER::Load`). The sibling `.kicad_pro`,
//! `.kicad_prl`, `.kicad_dru` (and the existence of a legacy `.pro`) are read next to the
//! board. Constructs whose parser package has not landed are skipped unless `--strict`.
//!
//! The section `connectivity` is opt-in as in the C++ tool; only its `rn_net` summary is
//! printed (`name`, `node_count` per net; `unconnected_count`, `unconnected_visible` for the
//! board), the other connectivity sections are compared by `kicad-connectivity`'s own tests.

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kicad_pcb::dump::format::{Out, v_int, v_str};
use kicad_pcb::dump::{DumpOptions, SilentPanics, dump_board, scan_file_uuids};
use kicad_pcb::io::ParseOptions;
use kicad_pcb::project::{LoadPath, LoadStep};
use kicad_pcb::{NetCode, RuleResolver};
use kicad_session::board_loader::load_guarded;
use kicad_session::{LoadInputs, Session};

fn usage() -> std::process::ExitCode {
    eprintln!(
        "usage: pcb-dump-session board [--loader] [--sections LIST] [--hash] [--strict] FILE..."
    );
    std::process::ExitCode::from(2)
}

/// The `rn_net` lines of `DumpRatsnest` (`dump_connectivity.cpp`) without `nodes` / `edges`.
fn dump_rn_net_summary(session: &Session, hash: bool) -> String {
    let mut o = Out::new(hash);
    let data = &session.connectivity;

    for net in 0..data.net_count() {
        let Some(rn) = data.ratsnest_for_net(net) else {
            continue;
        };

        if rn.node_count() == 0 {
            continue;
        }

        let node_count = rn.node_count();

        o.scope("rn_net", &format!("n{net}"), |o| {
            o.emit("name", || {
                v_str(
                    session
                        .board
                        .find_net(NetCode(net))
                        .map_or("?", |n| n.netname()),
                )
            });
            o.emit("node_count", || v_int(node_count as i64));
        });
    }

    o.scope("rn_net", "board", |o| {
        o.emit("unconnected_count", || {
            v_int(i64::from(data.unconnected_count(false)))
        });
        o.emit("unconnected_visible", || {
            v_int(i64::from(data.unconnected_count(true)))
        });
    });

    o.text
}

/// Loads `path` like the reference tool's `loadBoardQa( path, true )` / `loadBoardCli( path )`.
fn load(path: &Path, loader: bool, strict: bool) -> Result<Session, String> {
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let read = |ext: &str| {
        std::fs::read(abs.with_extension(ext))
            .ok()
            .map(|t| String::from_utf8_lossy(&t).into_owned())
    };

    let text = read("kicad_pcb").ok_or_else(|| format!("{}: cannot read", path.display()))?;
    let pro = read("kicad_pro");
    let prl = read("kicad_prl");
    let dru = read("kicad_dru");
    let source_name = abs.to_string_lossy().into_owned();

    let inputs = LoadInputs {
        text: &text,
        source_name: &source_name,
        project_json: pro.as_deref(),
        local_settings_json: prl.as_deref(),
        legacy_project_exists: abs.with_extension("pro").exists(),
        rules_text: dru.as_deref(),
        parse_options: ParseOptions {
            allow_pending: !strict,
        },
    };

    // As pcb-dump-rs (kicad_pcb::dump::load_board_for_dump_mode): a step that reaches a
    // `port_pending!` stub is reported and the sequence goes on with the next step.
    let mut warnings = Vec::new();
    let mut guard = |step: LoadStep, run: &mut dyn FnMut()| {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(run)) {
            warnings.push(format!(
                "pending step {step:?}: {}",
                panic_text(payload.as_ref())
            ));
        }
    };
    let path = if loader { LoadPath::Cli } else { LoadPath::Qa };

    let result = catch_unwind(AssertUnwindSafe(|| {
        load_guarded(path, &inputs, Some(&mut guard))
    }));

    match result {
        Ok(Ok(loaded)) => {
            let mut session = Session::from_loaded(loaded, &inputs);
            session.load_warnings.splice(0..0, warnings);
            Ok(session)
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(payload) => Err(panic_text(payload.as_ref())),
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-string panic)".to_string())
}

/// `dump_board` evaluates the board without the session's DRC engine (`BoardCtx::bare`); the
/// C++ tool's `BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue()` asks `m_DRCEngine`. Replaces
/// that line with the value through the engine the load sequence created.
fn with_engine_values(text: String, session: &Session, hash: bool) -> String {
    const PREFIX: &str = "board|board|ds.biggest_clearance=";

    let Some(engine) = session.drc_engine.as_ref() else {
        return text;
    };

    let value = session
        .board
        .design_settings()
        .get_biggest_clearance_value(Some(engine as &dyn RuleResolver));
    let mut o = Out::new(hash);
    o.line("board", "board", "ds.biggest_clearance", &v_int(value));

    text.lines()
        .map(|l| {
            if l.starts_with(PREFIX) {
                o.text.trim_end().to_string()
            } else {
                l.to_string()
            }
        })
        .fold(String::with_capacity(text.len()), |mut acc, l| {
            acc.push_str(&l);
            acc.push('\n');
            acc
        })
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) != Some("board") {
        return usage();
    }

    let mut opts = DumpOptions::default();
    let mut files = Vec::new();
    let mut loader = false;
    let mut strict = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--hash" => opts.hash = true,
            "--loader" => loader = true,
            "--strict" => strict = true,
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

    if files.is_empty() {
        return usage();
    }

    // The connectivity group is opt-in: never part of "all sections".
    let connectivity = opts.sections.remove("connectivity");

    if connectivity && opts.sections.is_empty() {
        opts.sections.insert("none".to_string());
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut failures = 0;
    let _quiet = SilentPanics::new();

    for file in &files {
        let path = Path::new(file);
        let name = path
            .file_name()
            .map_or_else(|| file.clone(), |n| n.to_string_lossy().into_owned());
        let _ = writeln!(out, "file|{name}|");

        let session = match load(path, loader, strict) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(out, "error|load|{name}|{}", e.replace('\n', " "));
                failures += 1;
                continue;
            }
        };

        for w in &session.load_warnings {
            let _ = writeln!(out, "warn|load|{name}|{}", w.replace('\n', " "));
        }

        let mut o = opts.clone();
        o.file_uuids = Some(
            std::fs::read(path)
                .map(|t| scan_file_uuids(&t))
                .unwrap_or_default(),
        );
        let text = with_engine_values(dump_board(&session.board, &o), &session, opts.hash);
        let _ = out.write_all(text.as_bytes());

        if connectivity {
            let _ = out.write_all(dump_rn_net_summary(&session, opts.hash).as_bytes());
        }
    }

    if failures > 0 {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}
