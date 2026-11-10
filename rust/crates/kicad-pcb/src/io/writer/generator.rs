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
//! PCBIO-13: `format( const PCB_GENERATOR* )` and `format( const PCB_CONSTRAINT* )` (W:3053-3238).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-13)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::{ConstraintId, GeneratorId};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_GENERATOR* aGenerator )`: `(generated ..)`. Staging stub (PCBIO-13).
    pub(crate) fn format_generator(&mut self, generator: GeneratorId) -> Result<(), IoError> {
        let _ = generator;
        self.pending("generator", "PCBIO-13")
    }

    /// Port of `format( const PCB_CONSTRAINT* aConstraint )`: `(constraint ..)`. Staging stub (PCBIO-13).
    pub(crate) fn format_constraint(&mut self, constraint: ConstraintId) -> Result<(), IoError> {
        let _ = constraint;
        self.pending("constraint", "PCBIO-13")
    }
}
