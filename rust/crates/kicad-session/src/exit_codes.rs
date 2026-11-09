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

//! Port of `include/cli/exit_codes.h` (`CLI::EXIT_CODES`).
//!
//! The job runners of this crate return these values exactly where
//! `pcbnew/pcbnew_jobs_handler.cpp` does; `kicad-cli` passes them to the process exit.

/// `AVOID_CLOSING`
pub const AVOID_CLOSING: i32 = -1;
/// `SUCCESS`
pub const SUCCESS: i32 = 0;
/// `OK`
pub const OK: i32 = 0;
/// `ERR_ARGS`
pub const ERR_ARGS: i32 = 1;
/// `ERR_UNKNOWN`
pub const ERR_UNKNOWN: i32 = 2;
/// `ERR_INVALID_INPUT_FILE`
pub const ERR_INVALID_INPUT_FILE: i32 = 3;
/// `ERR_INVALID_OUTPUT_CONFLICT`
pub const ERR_INVALID_OUTPUT_CONFLICT: i32 = 4;
/// `ERR_RC_VIOLATIONS`: rules check violation count was greater than 0.
pub const ERR_RC_VIOLATIONS: i32 = 5;
/// `ERR_JOBS_RUN_FAILED`
pub const ERR_JOBS_RUN_FAILED: i32 = 6;
/// `ERR_UNKNOWN_FILE_FORMAT`: no plugin for the requested face recognized the input file
/// format.
pub const ERR_UNKNOWN_FILE_FORMAT: i32 = 7;
/// `ERR_CANCELLED`: the user cancelled the operation.
pub const ERR_CANCELLED: i32 = 8;

/// Not a KiCad code: the exit code `kicad-rs` uses when a run reaches a part of the port that
/// has not landed yet (`SessionError::NotYetPorted`). It is `ERR_UNKNOWN`, so that scripts
/// written for the reference `kicad-cli` see an ordinary failure; the message on stderr names
/// the missing subsystem.
pub const NOT_YET_PORTED: i32 = ERR_UNKNOWN;
