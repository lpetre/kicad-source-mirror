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

//! Port of the zone part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-08): `parseZONE`, `parseZoneDefaults`, `parseZoneLayerProperty`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-08)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use std::collections::BTreeMap;

use kicad_core::PcbLayerId;

use crate::item::ParentRef;
use crate::zone::Zone;
use crate::zone_settings::ZoneLayerProperties;

impl PcbParser {
    /// Port of `parseZONE( BOARD_ITEM_CONTAINER* aParent )`; `parseBOARD` drops a zone without corners. Staging stub (PCBIO-08).
    pub(crate) fn parse_zone(&mut self, parent: ParentRef) -> Result<Option<Zone>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-08")?;
        Ok(None)
    }

    /// Port of `parseZoneDefaults( std::map<PCB_LAYER_ID, ZONE_LAYER_PROPERTIES>& )`. Staging stub (PCBIO-08).
    pub(crate) fn parse_zone_defaults(&mut self, properties: &mut BTreeMap<PcbLayerId, ZoneLayerProperties>) -> Result<(), IoError> {
        let _ = &properties;
        self.pending("PCBIO-08")?;
        Ok(())
    }

    /// Port of `parseZoneLayerProperty( std::map<PCB_LAYER_ID, ZONE_LAYER_PROPERTIES>& )`. Staging stub (PCBIO-08).
    pub(crate) fn parse_zone_layer_property(&mut self, properties: &mut BTreeMap<PcbLayerId, ZoneLayerProperties>) -> Result<(), IoError> {
        let _ = &properties;
        self.pending("PCBIO-08")?;
        Ok(())
    }
}
