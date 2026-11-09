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

//! Port of the generator and constraint parts of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-13):
//! `parseGENERATOR`, `parseGENERATOR_templates`, `parseCONSTRAINT`, `resolveConstraints`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-13)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use super::GeneratorInfo;
use crate::item::ParentRef;

impl PcbParser {
    /// Port of `parseGENERATOR( BOARD_ITEM* aParent )`: records a `GeneratorInfo` (deferred). Staging stub (PCBIO-13).
    pub(crate) fn parse_generator(&mut self, parent: ParentRef) -> Result<(), IoError> {
        let _ = &parent;
        self.pending("PCBIO-13")?;
        Ok(())
    }

    /// Port of `parseGENERATOR_templates( GENERATOR_INFO& )`. Staging stub (PCBIO-13).
    pub(crate) fn parse_generator_templates(&mut self, gen_info: &mut GeneratorInfo) -> Result<(), IoError> {
        let _ = &gen_info;
        self.pending("PCBIO-13")?;
        Ok(())
    }

    /// Port of `parseCONSTRAINT( BOARD_ITEM* aParent )`: records a `ConstraintInfo` (deferred). Staging stub (PCBIO-13).
    pub(crate) fn parse_constraint(&mut self, parent: ParentRef) -> Result<(), IoError> {
        let _ = &parent;
        self.pending("PCBIO-13")?;
        Ok(())
    }

    /// Port of `resolveConstraints( BOARD_ITEM* aParent )`, called by `finishItem` after a board or
    /// footprint is read. Staging stub (PCBIO-13): without a recorded declaration (the only
    /// case while the declaring parser is itself a stub) there is nothing to resolve, which
    /// is exact; anything else fails with "port pending".
    pub(crate) fn resolve_constraints(&mut self, parent: ParentRef) -> Result<(), IoError> {
        let _ = &parent;

        if self.constraint_infos.is_empty() {
            return Ok(());
        }

        Err(IoError::new("port pending: resolveConstraints (PCBIO-13)"))
    }
}
