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

//! Port of `PCB_IO_KICAD_SEXPR_PARSER::ParseFootprintLib` of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-14): the footprints
//! of a `.kicad_mod` file (bare `(footprint ...)` / `(module ...)` or
//! `(kicad_footprint_lib (version N) ...)`), parsed into the holder board.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-14)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::ids::FootprintId;

impl PcbParser {
    /// Port of `ParseFootprintLib( aFootprints )`: the parsed footprints (name -> entry map: PCBIO-14). Staging stub (PCBIO-14).
    pub(crate) fn parse_footprint_lib(&mut self) -> Result<Vec<FootprintId>, IoError> {
        self.pending("PCBIO-14")?;
        Ok(Vec::new())
    }
}
