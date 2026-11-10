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
//! PCBIO-11: the aggregated `(net_chains ..)` section of `format( const BOARD* )` (W:962-1038).
//! The legacy net classes are never written; the consolidated `(embedded_files ..)` section
//! (W:1040-1063) is written by `board.rs` through `EMBEDDED_FILES::WriteEmbeddedFiles`.
//!
//! **Staging stub** written by PCBIO-02 with the signature the package fills: the body calls
//! [`PcbWriter::pending`] (error "port pending: net_chains (PCBIO-11)", or skip and record with
//! `FormatOptions::allow_pending`) when a net carries a chain name or a terminal pad, i.e. when
//! the C++ could write the section; a board without either writes nothing, exactly like the
//! C++. Replace the body; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::board::Board;

impl PcbWriter<'_> {
    /// Port of the net-chain part of `format( const BOARD* aBoard )`: nets aggregated by chain
    /// name (`ValueStringCompare` order), written as `(net_chains (net_chain (name ..)
    /// (members (net ..) ..) [(terminal_pad ..)]) ..)`. Staging stub (PCBIO-11).
    pub(crate) fn format_net_chains(&mut self, board: &Board) -> Result<(), IoError> {
        // "nothing to aggregate" for every net: the C++ writes nothing.
        let any = board.nets().any(|net| {
            !net.net_chain().is_empty()
                || net.terminal_pad(0).is_some()
                || net.terminal_pad(1).is_some()
        });

        if !any {
            return Ok(());
        }

        self.pending("net_chains", "PCBIO-11")
    }
}
