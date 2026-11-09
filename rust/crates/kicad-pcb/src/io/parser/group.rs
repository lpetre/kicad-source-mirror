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

//! Port of the group part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-09): `parseGROUP_members`, `parseGROUP`, `resolveGroups`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-09)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use super::GroupInfo;
use crate::item::ParentRef;

impl PcbParser {
    /// Port of `parseGROUP_members( GROUP_INFO& )`. Staging stub (PCBIO-09).
    pub(crate) fn parse_group_members(&mut self, group_info: &mut GroupInfo) -> Result<(), IoError> {
        let _ = &group_info;
        self.pending("PCBIO-09")?;
        Ok(())
    }

    /// Port of `parseGROUP( BOARD_ITEM* aParent )`: records a `GroupInfo` (deferred). Staging stub (PCBIO-09).
    pub(crate) fn parse_group(&mut self, parent: ParentRef) -> Result<(), IoError> {
        let _ = &parent;
        self.pending("PCBIO-09")?;
        Ok(())
    }

    /// Port of `resolveGroups( BOARD_ITEM* aParent )`, called by `finishItem` after a board or
    /// footprint is read. Staging stub (PCBIO-09): without a recorded declaration (the only
    /// case while the declaring parser is itself a stub) there is nothing to resolve, which
    /// is exact; anything else fails with "port pending".
    pub(crate) fn resolve_groups(&mut self, parent: ParentRef) -> Result<(), IoError> {
        let _ = &parent;

        if self.group_infos.is_empty() && self.generator_infos.is_empty() {
            return Ok(());
        }

        Err(IoError::new("port pending: resolveGroups (PCBIO-09)"))
    }
}
