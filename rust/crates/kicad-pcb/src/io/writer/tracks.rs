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
//! PCBIO-03: `format( const PCB_TRACK* )` (segments, arcs, vias, W:3241-3496), `formatTeardropParameters` and `isDefaultTeardropParameters` (W:851-905).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-03)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::TrackId;
use crate::pcb_track::Track;
use crate::teardrop_parameters::TeardropParameters;

impl PcbWriter<'_> {
    /// Port of `format( const PCB_TRACK* aTrack )`: `(segment ..)`, `(arc ..)` or `(via ..)`. Staging stub (PCBIO-03).
    pub(crate) fn format_track(&mut self, track: TrackId) -> Result<(), IoError> {
        let item = match self.src.track(track) {
            Some(Track::Arc(_)) => "arc",
            Some(Track::Via(_)) => "via",
            _ => "segment",
        };
        self.pending(item, "PCBIO-03")
    }

    /// Port of `formatTeardropParameters( const TEARDROP_PARAMETERS& )`: `(teardrops ..)`. Staging stub (PCBIO-03).
    pub(crate) fn format_teardrop_parameters(
        &mut self,
        td_params: &TeardropParameters,
    ) -> Result<(), IoError> {
        let _ = td_params;
        self.pending("teardrops", "PCBIO-03")
    }
}
