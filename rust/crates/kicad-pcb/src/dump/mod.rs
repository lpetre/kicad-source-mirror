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
//! constructor is pending. Compare with `rust/tools/pcb-dump/compare.py`.

use std::collections::BTreeSet;

pub mod board;
pub mod defaults;
pub mod format;
pub mod items;
pub mod settings;

pub use format::{Out, SilentPanics, fnv1a64, scan_file_uuids};

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

/// Loads a `.kicad_pcb` for `pcb-dump-rs board`.
///
/// PARSER HOOK: the `.kicad_pcb` parser package replaces this body with the Rust equivalent of
/// the C++ tool's default load path (`KI_TEST::LoadBoard`: parse, then `BuildListOfNets`,
/// `BuildConnectivity`, ...). Until then it reports that the parser is pending.
pub fn load_board_for_dump(path: &std::path::Path) -> Result<Board, String> {
    Err(format!(
        "{}: the .kicad_pcb parser is not ported yet (kicad_pcb::dump::load_board_for_dump)",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
