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
//! PCBIO-04: `formatSetup` (incl. the `BOARD_STACKUP::FormatBoardStackup` and `PCB_PLOT_PARAMS::Format` calls) and `formatDrillSymbolProfile` (W:601-745).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-04)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::board::Board;
use crate::board_design_settings::BoardDesignSettings;

impl PcbWriter<'_> {
    /// Port of `formatSetup( const BOARD* aBoard )`: the `(setup ..)` section. Staging stub (PCBIO-04).
    pub(crate) fn format_setup(&mut self, board: &Board) -> Result<(), IoError> {
        let _ = board;
        self.pending("setup", "PCBIO-04")
    }

    /// Port of `formatDrillSymbolProfile( const BOARD_DESIGN_SETTINGS& )`: nothing for the default profile. Staging stub (PCBIO-04).
    pub(crate) fn format_drill_symbol_profile(
        &mut self,
        settings: &BoardDesignSettings,
    ) -> Result<(), IoError> {
        let _ = settings;
        self.pending("drill_symbol_profile", "PCBIO-04")
    }
}
