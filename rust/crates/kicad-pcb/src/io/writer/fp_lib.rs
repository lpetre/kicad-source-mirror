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
//! PCBIO-14: `FormatFootprintLibFile` (W:354-371; the `FootprintSave` normalisation W:4176-4202 belongs here too).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-14)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::FootprintId;

impl PcbWriter<'_> {
    /// Port of `FormatFootprintLibFile( FOOTPRINT* aFootprint )`: `(kicad_footprint_lib (version ..) (generator "kicad_footprint_editor") (generator_version "..")` + the footprint (`aInLibFile = true`) + `)`. The C++ first embeds or clears the footprint fonts (on a copy of a live footprint). Staging stub (PCBIO-14).
    pub(crate) fn format_footprint_lib_file(
        &mut self,
        footprint: FootprintId,
    ) -> Result<(), IoError> {
        let _ = footprint;
        self.pending("kicad_footprint_lib", "PCBIO-14")
    }
}
