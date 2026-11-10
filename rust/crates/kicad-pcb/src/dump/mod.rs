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

//! Rust side of the item-level reference dump (`rust/tools/pcb-dump`, `docs/PCB_DUMP_FORMAT.md`).
//!
//! Mirrors the C++ tool file by file: [`format`] (`format.cpp`, `pcb_dump.h`), [`board`]
//! (`dump_board.cpp`), [`items`] (`dump_items.cpp`), [`settings`] (`dump_settings.cpp`),
//! [`defaults`] (`dump_defaults.cpp`). Every field is evaluated under a panic guard: a
//! `port_pending!` stub prints `pending|SECTION|KEY|FIELD=CXX::Name`, any other panic
//! `rust-error|SECTION|KEY|FIELD=message`; `pending|SECTION|KEY|*=...` marks a whole class whose
//! constructor is pending; a failure FIELD with `*` / `|` stands for every field a failed
//! condition controls. Compare with `rust/tools/pcb-dump/compare.py` or [`compare::compare`].
//!
//! Quirk kept from the C++ tool: `EMIT( "x." + std::to_string( i++ ), .. )` evaluates its field
//! expression twice, so those list indices are 1, 3, 5, ... (`default_fp_text`,
//! `netclass_pattern`, footprint `pad` / `drawing` / `zone`); the Rust side prints `2 * i + 1`.

use std::collections::BTreeSet;

pub mod board;
pub mod compare;
pub mod defaults;
pub mod format;
pub mod items;
pub mod props;
pub mod settings;

pub use format::{Out, SilentPanics, fnv1a64, scan_file_uuids};
pub use props::dump_props_table;

use crate::board::Board;

/// `DUMP_OPTIONS`.
#[derive(Clone, Debug, Default)]
pub struct DumpOptions {
    /// Sections to print; empty = all.
    pub sections: BTreeSet<String>,
    /// `--hash`: values replaced by their FNV-1a hash.
    pub hash: bool,
    /// The UUIDs written in the board file ([`scan_file_uuids`]); `None` = no file, every item
    /// gets an ordinal key and `item.uuid=generated`.
    pub file_uuids: Option<BTreeSet<String>>,
}

impl DumpOptions {
    /// `DUMP_OPTIONS::Want`.
    pub fn want(&self, section: &str) -> bool {
        self.sections.is_empty() || self.sections.contains(section)
    }
}

/// `DumpBoard`: all sections of one board, as text lines (without the `file|` line).
pub fn dump_board(board: &Board, opts: &DumpOptions) -> String {
    let _quiet = SilentPanics::new();
    let mut o = Out::new(opts.hash);
    o.file_uuids = opts.file_uuids.clone();
    board::dump_board_into(&mut o, board, opts);
    o.text
}

/// `DumpDefaults`: the `defaults` command.
pub fn dump_defaults(hash: bool) -> String {
    let _quiet = SilentPanics::new();
    let mut o = Out::new(hash);
    defaults::dump_defaults_into(&mut o);
    o.text
}

/// Load path of `pcb-dump-rs board` (`main.cpp` of the C++ tool).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DumpLoadMode {
    /// Default: `KI_TEST::LoadBoard` (`loadBoardQa( path, true )`).
    #[default]
    Qa,
    /// `--loader`: the kicad-cli path (`loadBoardCli`: `<base>.kicad_pro`, created in memory
    /// when missing, then `BOARD_LOADER::Load`).
    Loader,
    /// `--no-init`: the parser only.
    NoInit,
}

/// Loads a `.kicad_pcb` for `pcb-dump-rs board` on the default (QA) path, see
/// [`load_board_for_dump_mode`].
pub fn load_board_for_dump(path: &std::path::Path) -> Result<Board, String> {
    load_board_for_dump_mode(path, DumpLoadMode::Qa).map(|(board, _)| board)
}

/// Loads a `.kicad_pcb` for `pcb-dump-rs board` the way the C++ tool does for `mode`; returns
/// the board and `warn|load|...` texts.
///
/// The parser runs with the constructs of packages that have not landed yet skipped
/// (`ParseOptions::allow_pending`); the QA path reads the stream (source name "", no board
/// file name), the loader path names the file. Then the model steps of
/// `project::board_loader` in C++ order (package SET-5), with the project texts read next to
/// the board: QA binds `<base>.kicad_pro` (or, for a lone legacy `<base>.pro`, an empty
/// project: the `.pro` migration is not ported) and `<base>.kicad_prl` only when one exists;
/// the loader always binds one. The session steps (DRC engine, exclusions, connectivity) are
/// not run here (PCBIO-15); they are reported as `warn` lines. A step that hits a
/// `port_pending!` stub is reported as `warn|load|pending step <step>: <C++ name>` and the
/// sequence continues with the next step.
pub fn load_board_for_dump_mode(
    path: &std::path::Path,
    mode: DumpLoadMode,
) -> Result<(Board, Vec<String>), String> {
    use crate::project::board_design_settings_json::{DrcExclusionCodec, RawDrcExclusionCodec};
    use crate::project::{LoadPath, StepOutcome, load_project, run_model_step};

    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let options = crate::io::ParseOptions {
        allow_pending: true,
    };

    let source_name = match mode {
        DumpLoadMode::Loader => path.to_string_lossy().into_owned(),
        _ => String::new(),
    };

    let mut board = crate::io::parse_board(&text, &source_name, &options)
        .map_err(|e| e.what().to_string())?;

    let load_path = match mode {
        DumpLoadMode::NoInit => return Ok((board, Vec::new())),
        DumpLoadMode::Qa => LoadPath::Qa,
        DumpLoadMode::Loader => {
            // PCB_IO::LoadBoard: newBoard->SetFileName( aFileName )
            board.file_name = source_name.clone();
            LoadPath::Cli
        }
    };

    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let read = |ext: &str| std::fs::read(abs.with_extension(ext)).ok();
    let pro_text = read("kicad_pro").map(|t| String::from_utf8_lossy(&t).into_owned());
    let prl_text = read("kicad_prl").map(|t| String::from_utf8_lossy(&t).into_owned());
    let has_legacy_pro = abs.with_extension("pro").exists();
    let pro_name = abs.with_extension("kicad_pro").to_string_lossy().into_owned();

    let mut project = if load_path == LoadPath::Cli || pro_text.is_some() || has_legacy_pro {
        Some(load_project(
            pro_text.as_deref(),
            prl_text.as_deref(),
            &pro_name,
        ))
    } else {
        None
    };

    let codec: std::rc::Rc<dyn DrcExclusionCodec> = std::rc::Rc::new(RawDrcExclusionCodec);
    let mut warnings = Vec::new();
    let _quiet = SilentPanics::new();

    for &step in load_path.steps() {
        let result = format::attempt(|| {
            run_model_step(&mut board, load_path, step, &mut project, &codec)
        });

        match result {
            Ok(Ok(StepOutcome::Done)) => {}
            Ok(Ok(StepOutcome::Session)) => {
                warnings.push(format!("session step not run: {step:?}"));
            }
            Ok(Ok(StepOutcome::Warning(w))) => warnings.push(w),
            // The exception escapes the C++ load: the board is not loaded.
            Ok(Err(e)) => return Err(format!("{step:?}: {}", e.message)),
            Err(format::Failure::Pending(name)) => {
                warnings.push(format!("pending step {step:?}: {name}"));
            }
            Err(format::Failure::Error(msg)) => {
                warnings.push(format!("rust-error in step {step:?}: {msg}"));
            }
        }
    }

    Ok((board, warnings))
}

#[cfg(test)]
mod tests;
