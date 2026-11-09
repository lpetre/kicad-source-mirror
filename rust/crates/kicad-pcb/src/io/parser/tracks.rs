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

//! Port of the track part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-03): `parseTEARDROP_PARAMETERS`, `parseARC`, `parsePCB_TRACK`,
//! `parsePCB_VIA`, `parseViastack`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-03)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::pcb_track::{PcbVia, Track};
use crate::teardrop_parameters::TeardropParameters;

impl PcbParser {
    /// Port of `parseTEARDROP_PARAMETERS( TEARDROP_PARAMETERS* )`: call with `CurTok() == T_teardrops`. Staging stub (PCBIO-03).
    pub(crate) fn parse_teardrop_parameters(&mut self, td_params: &mut TeardropParameters) -> Result<(), IoError> {
        let _ = &td_params;
        self.pending("PCBIO-03")?;
        Ok(())
    }

    /// Port of `parseARC`: a track arc (`Track::Arc`); `None` on a non-copper layer (C++ null). Staging stub (PCBIO-03).
    pub(crate) fn parse_arc(&mut self) -> Result<Option<Track>, IoError> {
        self.pending("PCBIO-03")?;
        Ok(None)
    }

    /// Port of `parsePCB_TRACK`: a segment (`Track::Segment`); `None` on a non-copper layer (C++ null). Staging stub (PCBIO-03).
    pub(crate) fn parse_pcb_track(&mut self) -> Result<Option<Track>, IoError> {
        self.pending("PCBIO-03")?;
        Ok(None)
    }

    /// Port of `parsePCB_VIA`: a via (`Track::Via`). Staging stub (PCBIO-03).
    pub(crate) fn parse_pcb_via(&mut self) -> Result<Option<Track>, IoError> {
        self.pending("PCBIO-03")?;
        Ok(None)
    }

    /// Port of `parseViastack( PCB_VIA* )`: call with `CurTok() == T_padstack`. Staging stub (PCBIO-03).
    pub(crate) fn parse_viastack(&mut self, via: &mut PcbVia) -> Result<(), IoError> {
        let _ = &via;
        self.pending("PCBIO-03")?;
        Ok(())
    }
}
