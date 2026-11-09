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

//! Port of the setup part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-04): `parseBoardStackup`, `parseSetup`, `parseDrillSymbolProfile`,
//! `parseDefaults`, `parseDefaultTextDims` (and `PCB_PLOT_PARAMS_PARSER` via `pcb_plot_params`).
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-04)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;

impl PcbParser {
    /// Port of `parseBoardStackup`: call with `CurTok() == T_stackup`. Staging stub (PCBIO-04).
    pub(crate) fn parse_board_stackup(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-04")?;
        Ok(())
    }

    /// Port of `parseSetup`: call with `CurTok() == T_setup`. Staging stub (PCBIO-04).
    pub(crate) fn parse_setup(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-04")?;
        Ok(())
    }

    /// Port of `parseDrillSymbolProfile`. Staging stub (PCBIO-04).
    pub(crate) fn parse_drill_symbol_profile(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-04")?;
        Ok(())
    }

    /// Port of `parseDefaults( BOARD_DESIGN_SETTINGS& )` on the board's design settings. Staging stub (PCBIO-04).
    pub(crate) fn parse_defaults(&mut self) -> Result<(), IoError> {
        self.pending("PCBIO-04")?;
        Ok(())
    }

    /// Port of `parseDefaultTextDims( BOARD_DESIGN_SETTINGS&, int aLayer )` (`aLayer` = layer class). Staging stub (PCBIO-04).
    pub(crate) fn parse_default_text_dims(&mut self, layer: usize) -> Result<(), IoError> {
        let _ = &layer;
        self.pending("PCBIO-04")?;
        Ok(())
    }
}
