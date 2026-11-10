// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! `.kicad_pcb` reader and writer (`pcbnew/pcb_io/kicad_sexpr/`); module tree of
//! `docs/analysis/pcb-io.md` 10.1, owned by the PCBIO packages.
//!
//! This module holds the entry points and the file-format constants of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h` (PCBIO-01); the parser is [`parser`].
//!
//! * [`parse_board`] / [`parse_board_report`]: port of `PCB_IO_KICAD_SEXPR::DoLoad` for a new
//!   load (`aIsNewLoad == true`) of board text: the parser (`PCB_IO_KICAD_SEXPR_PARSER::Parse`),
//!   the `FUTURE_FORMAT_ERROR` conversion and the "This file does not contain a PCB." check.
//!   The QA helper `KI_TEST::ReadBoardFromFileOrStream` (the `pcb-dump --no-init` load path)
//!   is the same parse without the board file name.
//! * [`parse_footprint_file`]: the entry for `.kicad_mod` text (PCBIO-14, staging stub).
//! * [`format_board`] / [`format_board_report`]: port of `PCB_IO_KICAD_SEXPR::SaveBoard`, the
//!   writer is [`writer`] (PCBIO-02).

pub mod parser;
pub mod writer;

use kicad_sexpr::IoError;

use crate::board::Board;

pub use parser::{ParseOptions, ParseReport, PcbParser, SkippedItem};
pub use writer::{
    FormatOptions, FormatReport, PcbWriter, SkippedFormat, format_board, format_board_report,
    format_board_to_formatter, format_footprint_lib_file,
};

// ---- version history (pcb_io_kicad_sexpr.h) ------------------------------------------------

/// `SEXPR_BOARD_FILE_VERSION`: the current board file format ("Non-component pads").
pub const SEXPR_BOARD_FILE_VERSION: i32 = 20261031;

/// `BOARD_FILE_HOST_VERSION`: earlier files than this include the host tag.
pub const BOARD_FILE_HOST_VERSION: i32 = 20200825;

/// `LEGACY_ARC_FORMATTING`: these were the last to use old arc formatting.
pub const LEGACY_ARC_FORMATTING: i32 = 20210925;

/// `LEGACY_NET_TIES`: these were the last to use the keywords field to indicate a net-tie.
pub const LEGACY_NET_TIES: i32 = 20220815;

/// `FIRST_NORMALIZED_VERISON` (sic): earlier files did not have normalized bools. Defined in
/// the C++ header but not referenced by the parser (`parseMaybeAbsentBool` does the job).
pub const FIRST_NORMALIZED_VERISON: i32 = 20230924;

/// `FIRST_FP_AFFINE_TRANSFORM`: first version that stores footprint children in library frame.
/// Earlier files stored them in board frame, so zone outlines need to be un-baked on load.
pub const FIRST_FP_AFFINE_TRANSFORM: i32 = 20260616;

/// `SEXPR_FOOTPRINT_LIB_FILE_VERSION`. The footprint library now has a header and is a separate
/// format from the board file FOOTPRINT object parser and formatter.
pub const SEXPR_FOOTPRINT_LIB_FILE_VERSION: i32 = 20261031;

// A footprint body uses the board format, so a library file stamped older than the board format
// would run migrations on current data (the C++ static_assert).
const _: () = assert!(SEXPR_FOOTPRINT_LIB_FILE_VERSION >= SEXPR_BOARD_FILE_VERSION);

/// Load a board from `.kicad_pcb` text: port of `PCB_IO_KICAD_SEXPR::DoLoad` for a new load
/// (the parse half of `PCB_IO::LoadBoard`; setting the board file name and the post-load
/// initialisation of `BOARD_LOADER` are PCBIO-15).
///
/// `source_name` is what the C++ line reader reports as its source (the file name) in error
/// messages. Non-fatal parse warnings and the items skipped by staging stubs are dropped; use
/// [`parse_board_report`] to get them.
///
/// # Errors
///
/// The C++ exceptions as [`IoError`]: `PARSE_ERROR`s with source, line and offset,
/// `FUTURE_FORMAT_ERROR` for files of a newer format, `IO_ERROR`s (undefined layers, layer
/// names not in the fixed layer hash, ...), "This file does not contain a PCB." for a footprint
/// file, and "port pending: ..." for constructs whose parser package has not landed yet
/// (unless `options.allow_pending`).
pub fn parse_board(
    text: &str,
    source_name: &str,
    options: &ParseOptions,
) -> Result<Board, IoError> {
    parse_board_report(text, source_name, options).map(|(board, _)| board)
}

/// [`parse_board`] returning the [`ParseReport`] (parse warnings, `GetParseWarnings()`, and the
/// constructs skipped by staging stubs with `options.allow_pending`) next to the board.
///
/// Port of `PCB_IO_KICAD_SEXPR::DoLoad` (`aIsNewLoad == true`, no append, no preserved
/// stackup): the `revertPartialAppend` steps are no-ops for a new load.
pub fn parse_board_report(
    text: &str,
    source_name: &str,
    options: &ParseOptions,
) -> Result<(Board, ParseReport), IoError> {
    let mut parser = PcbParser::new(text, source_name, Board::new(), true, options.clone());

    let item = match parser.parse() {
        Ok(item) => item,
        // Don't wrap a FUTURE_FORMAT_ERROR in another
        Err(e @ IoError::FutureFormat(_)) => return Err(e),
        Err(IoError::Parse(parse_error)) => {
            if parser.is_too_recent() {
                // GetRequiredVersion() itself throws a PARSE_ERROR for a version number that
                // is no date; that one propagates instead.
                let required = parser.get_required_version()?;
                return Err(kicad_sexpr::FutureFormatError::from_parse_error(
                    &parse_error,
                    &required,
                    "",
                )
                .into());
            }

            return Err(IoError::Parse(parse_error));
        }
        Err(e) => return Err(e),
    };

    if !matches!(item, parser::ParsedItem::Board) {
        // The parser loaded something that was valid, but wasn't a board.
        return Err(parser
            .lexer
            .parse_error("This file does not contain a PCB."));
    }

    // Report any non-fatal parse warnings to the load info reporter: returned in the report.
    Ok(parser.into_board_and_report())
}

/// Load the footprints of a `.kicad_mod` footprint library file (`FP_CACHE::Load` ->
/// `PCB_IO_KICAD_SEXPR_PARSER::ParseFootprintLib`) into a holder board
/// (`BoardUse::FpHolder`, `board-model.md` 2).
///
/// Staging stub of package PCBIO-14: returns "port pending: kicad_footprint_lib (PCBIO-14)".
pub fn parse_footprint_file(
    text: &str,
    source_name: &str,
    options: &ParseOptions,
) -> Result<(Board, ParseReport), IoError> {
    let _ = (text, source_name, options);
    Err(IoError::new("port pending: kicad_footprint_lib (PCBIO-14)"))
}
