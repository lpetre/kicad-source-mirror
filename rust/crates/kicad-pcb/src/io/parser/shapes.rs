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

//! Port of the graphic-shape part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-03): `parseLineEnding`, `parsePCB_SHAPE`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-03)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::item::ParentRef;
use crate::line_ending::LineEnding;
use crate::pcb_shape::PcbShape;

impl PcbParser {
    /// Port of `parseLineEnding( LINE_ENDING& )`. Staging stub (PCBIO-03).
    pub(crate) fn parse_line_ending(&mut self, ending: &mut LineEnding) -> Result<(), IoError> {
        let _ = &ending;
        self.pending("PCBIO-03")?;
        Ok(())
    }

    /// Port of `parsePCB_SHAPE( BOARD_ITEM* aParent )`: `gr_*` / `fp_*` shapes (and pad primitives). Staging stub (PCBIO-03).
    pub(crate) fn parse_pcb_shape(&mut self, parent: ParentRef) -> Result<Option<PcbShape>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-03")?;
        Ok(None)
    }
}
