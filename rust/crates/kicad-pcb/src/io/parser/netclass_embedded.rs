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

//! Port of the net class / net chain / variant / embedded-file parts of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-11): `parseVariants`,
//! `parseNETCLASS`, `parseNET_CHAINS_SECTION`, and the `embedded_fonts` / `embedded_files`
//! branches of `parseBOARD_unchecked` (P:1547-1591, `EMBEDDED_FILES_PARSER` with warning
//! recovery).
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-11)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;

impl PcbParser {
    /// Port of `parseVariants`: call with `CurTok() == T_variants`. Staging stub (PCBIO-11).
    pub(crate) fn parse_variants(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-11")?;
        Ok(())
    }

    /// Port of `parseNETCLASS`: a legacy `(net_class ...)`; the caller sets `m_LegacyNetclassesLoaded`. Staging stub (PCBIO-11).
    pub(crate) fn parse_netclass(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-11")?;
        Ok(())
    }

    /// Port of `parseNET_CHAINS_SECTION`. Staging stub (PCBIO-11).
    pub(crate) fn parse_net_chains_section(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-11")?;
        Ok(())
    }

    /// Port of The `T_embedded_fonts` branch of `parseBOARD_unchecked`. Staging stub (PCBIO-11).
    pub(crate) fn parse_board_embedded_fonts(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-11")?;
        Ok(())
    }

    /// Port of The `T_embedded_files` branch of `parseBOARD_unchecked` (parse errors become warnings). Staging stub (PCBIO-11).
    pub(crate) fn parse_board_embedded_files(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-11")?;
        Ok(())
    }
}
