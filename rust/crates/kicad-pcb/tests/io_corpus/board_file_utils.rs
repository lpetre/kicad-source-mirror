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

//! Port of the board / footprint part of `qa/pcbnew_utils/board_file_utils.cpp` (namespace
//! `KI_TEST`); the file-system part is `kicad_qa::board_file_utils`.
//!
//! C++ streams become file reads: `ReadBoardFromFileOrStream` with an empty file name (read
//! stdin) has no counterpart in a test.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use kicad_pcb::board::Board;
use kicad_pcb::ids::FootprintId;
use kicad_pcb::io::{
    FormatOptions, ParseOptions, format_board, format_footprint_lib_file, parse_board,
    parse_footprint_file,
};
use kicad_qa::board_file_utils::{pcbnew_qa_data_path, read_file_text};
use kicad_sexpr::IoError;

/// What `parse_board` reports for a file holding another item than a board.
const NOT_A_PCB: &str = "This file does not contain a PCB.";

/// Port of `KI_TEST::ReadBoardFromFile` / `ReadBoardFromFileOrStream` with a file name: parse
/// the board in `path` with a parser that has no board file name (`PCB_IO_KICAD_SEXPR_PARSER(
/// &reader, nullptr, nullptr )`, the `pcb-dump --no-init` path).
///
/// `Ok(None)` when the file cannot be opened or holds another item than a board (C++ returns
/// `nullptr`: `ReadItemFromStream` downcasts); parse errors are returned (C++ rethrows).
pub fn read_board_from_file(path: &Path) -> Result<Option<Board>, IoError> {
    read_board_from_file_with(path, &ParseOptions::default())
}

/// [`read_board_from_file`] with explicit parse options (`allow_pending` for staging).
pub fn read_board_from_file_with(
    path: &Path,
    options: &ParseOptions,
) -> Result<Option<Board>, IoError> {
    let Some(text) = read_file_text(path) else {
        return Ok(None);
    };

    match parse_board(&text, "", options) {
        Ok(board) => Ok(Some(board)),
        Err(e) if e.what().starts_with(NOT_A_PCB) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Port of `KI_TEST::RequireBoardFromPcbnewQAData`: load `qa/data/pcbnew/<file_name>`; a board
/// that does not load fails the test (`BOOST_REQUIRE`).
pub fn require_board_from_pcbnew_qa_data(file_name: impl AsRef<Path>) -> Board {
    let path = pcbnew_qa_data_path(file_name);

    // Make it really clear in the test context which board is being loaded
    match read_board_from_file(&path) {
        Ok(Some(board)) => board,
        Ok(None) => panic!("Loading board {}: no board", path.display()),
        Err(e) => panic!("Loading board {}: {}", path.display(), e.what()),
    }
}

/// Port of `KI_TEST::DumpBoardToFile`: `PCB_IO_KICAD_SEXPR::SaveBoard` into `file_name`.
///
/// # Panics
///
/// When the board cannot be formatted or the file cannot be written (C++ throws).
pub fn dump_board_to_file(board: &Board, file_name: &Path) {
    let text = format_board(board, &FormatOptions::default())
        .unwrap_or_else(|e| panic!("saving {}: {}", file_name.display(), e.what()));
    std::fs::write(file_name, text)
        .unwrap_or_else(|e| panic!("writing {}: {e}", file_name.display()));
}

/// Port of `KI_TEST::ReadFootprintFromFile`: load footprint `footprint_name` from the library
/// of `file_name` (the directory of a `.kicad_mod` file, else `file_name` itself as the
/// `.pretty` directory), i.e. `<lib>/<footprint_name>.kicad_mod`. Returns the holder board and
/// the footprint, or `None` on an `IO_ERROR` (C++ logs a checkpoint and returns `nullptr`).
///
/// The footprint library parser is PCBIO-14 (`parse_footprint_file` is a staging stub until
/// then, so this returns `None`).
pub fn read_footprint_from_file(
    file_name: &Path,
    footprint_name: &str,
) -> Option<(Board, FootprintId)> {
    let lib_path: PathBuf = if file_name
        .extension()
        .is_some_and(|e| e.to_string_lossy().to_lowercase() == "kicad_mod")
    {
        file_name.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        file_name.to_path_buf()
    };

    let path = lib_path.join(format!("{footprint_name}.kicad_mod"));
    let text = read_file_text(&path)?;

    match parse_footprint_file(&text, &path.to_string_lossy(), &ParseOptions::default()) {
        Ok((holder, _)) => {
            let fp = holder.footprints().next().map(|(id, _)| id)?;
            Some((holder, fp))
        }
        Err(e) => {
            eprintln!(
                "Exception '{}' occurred reading footprint file '{}'",
                e.what(),
                file_name.display()
            );
            None
        }
    }
}

/// Port of `KI_TEST::DumpFootprintToFile`: `FormatFootprintLibFile` of `fp` (held by `holder`)
/// into `library_path` through a prettified formatter. Errors are logged, as in C++.
pub fn dump_footprint_to_file(holder: &Board, fp: FootprintId, library_path: &Path) {
    // `PCB_IO_KICAD_SEXPR io;`: the default control flags (`CTL_FOR_BOARD`).
    match format_footprint_lib_file(holder, fp, &FormatOptions::default()) {
        Ok((text, _)) => {
            if let Err(e) = std::fs::write(library_path, text) {
                eprintln!(
                    "Exception '{e}' occurred writing footprint file '{}'",
                    library_path.display()
                );
            }
        }
        Err(e) => eprintln!(
            "Exception '{}' occurred writing footprint file '{}'",
            e.what(),
            library_path.display()
        ),
    }
}
