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

//! The error type of the session layer (new code).
//!
//! Failures that the C++ jobs handler reports through its `REPORTER` and an exit code are not
//! errors here: they come back as a [`JobOutcome`](crate::JobOutcome) with the C++ messages and
//! exit code. `SessionError` covers what has no C++ equivalent: a subsystem of the port that has
//! not landed yet, a bad argument of the Rust/JS API, and a stale item handle.

use std::fmt;

/// Error of a [`Session`](crate::Session) call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// The call needs a part of the port that is still missing. `subsystem` names the crate and
    /// the C++ entry point, e.g. `"kicad-drc: DRC_ENGINE::RunTests"`.
    NotYetPorted {
        /// Crate and C++ entry point that is missing.
        subsystem: String,
    },
    /// The board could not be loaded (C++ `getBoard` failure: `Failed to load board: <what>`).
    Load {
        /// The `IO_ERROR::What()` text of the loader.
        message: String,
    },
    /// A malformed argument of the Rust / JavaScript API (not a C++ condition).
    InvalidArgument {
        /// What was wrong.
        message: String,
    },
    /// An item handle that does not name a live item of this session's board.
    StaleId {
        /// The handle (`ItemId::to_bits`).
        id: u64,
    },
}

impl SessionError {
    /// A `NotYetPorted` error for `subsystem`.
    pub fn not_yet_ported(subsystem: impl Into<String>) -> Self {
        SessionError::NotYetPorted {
            subsystem: subsystem.into(),
        }
    }

    /// An `InvalidArgument` error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        SessionError::InvalidArgument {
            message: message.into(),
        }
    }

    /// True for [`SessionError::NotYetPorted`].
    pub fn is_not_yet_ported(&self) -> bool {
        matches!(self, SessionError::NotYetPorted { .. })
    }

    /// The short machine-readable kind: `"NotYetPorted"`, `"Load"`, `"InvalidArgument"` or
    /// `"StaleId"` (the JavaScript error `name` without the `Error` suffix).
    pub fn kind(&self) -> &'static str {
        match self {
            SessionError::NotYetPorted { .. } => "NotYetPorted",
            SessionError::Load { .. } => "Load",
            SessionError::InvalidArgument { .. } => "InvalidArgument",
            SessionError::StaleId { .. } => "StaleId",
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotYetPorted { subsystem } => {
                write!(f, "not yet ported: {subsystem}")
            }
            SessionError::Load { message } => {
                if message.is_empty() {
                    write!(f, "Failed to load board")
                } else {
                    write!(f, "Failed to load board: {message}")
                }
            }
            SessionError::InvalidArgument { message } => write!(f, "invalid argument: {message}"),
            SessionError::StaleId { id } => write!(f, "no live item with id {id}"),
        }
    }
}

impl std::error::Error for SessionError {}
