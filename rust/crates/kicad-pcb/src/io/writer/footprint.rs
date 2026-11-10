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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-06: `format( const FOOTPRINT*, bool aInLibFile )` and `formatInitialComments` (W:1452-1884).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-06)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::FootprintId;

impl PcbWriter<'_> {
    /// Port of `format( const FOOTPRINT* aFootprint, bool aInLibFile )`: `aInLibFile` leaves the initial comments and version to the enclosing library file. Staging stub (PCBIO-06).
    pub(crate) fn format_footprint(
        &mut self,
        footprint: FootprintId,
        in_lib_file: bool,
    ) -> Result<(), IoError> {
        let _ = (footprint, in_lib_file);
        self.pending("footprint", "PCBIO-06")
    }

    /// Port of `formatInitialComments( const FOOTPRINT* )`: the verbatim `#...` lines (not with `CTL_OMIT_INITIAL_COMMENTS`). Staging stub (PCBIO-06).
    pub(crate) fn format_initial_comments(
        &mut self,
        footprint: FootprintId,
    ) -> Result<(), IoError> {
        let _ = footprint;
        self.pending("initial_comments", "PCBIO-06")
    }
}
