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

//! Port of the marker / DRC-exclusion functions of `pcbnew/board.cpp`
//! (`ResolveDRCExclusions`, `RecordDRCExclusions`, `DeleteMARKERs`). Package M22; M0d wrote
//! the signatures (`board-model-types.md` 18).

use crate::board::Board;
use crate::ids::MarkerId;

impl Board {
    /// `BOARD::ResolveDRCExclusions( aCreateMarkers )`: the markers created for exclusions
    /// that match no existing marker.
    pub fn resolve_drc_exclusions(&mut self, create_markers: bool) -> Vec<MarkerId> {
        crate::port_pending!("BOARD::ResolveDRCExclusions", create_markers)
    }

    /// `BOARD::RecordDRCExclusions`.
    pub fn record_drc_exclusions(&mut self) {
        crate::port_pending!("BOARD::RecordDRCExclusions")
    }

    /// `BOARD::DeleteMARKERs( aWarningsAndErrors, aExclusions )`.
    pub fn delete_markers(&mut self, warnings_and_errors: bool, exclusions: bool) {
        crate::port_pending!("BOARD::DeleteMARKERs", warnings_and_errors, exclusions)
    }
}
