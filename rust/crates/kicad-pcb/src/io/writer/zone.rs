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
//! PCBIO-08: `format( const ZONE* )` and `format( const ZONE_LAYER_PROPERTIES&, int, PCB_LAYER_ID )` (W:3499-3790).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-08)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::ZoneId;
use crate::zone_settings::ZoneLayerProperties;
use kicad_core::PcbLayerId;

impl PcbWriter<'_> {
    /// Port of `format( const ZONE* aZone )`: `(zone ..)`. Staging stub (PCBIO-08).
    pub(crate) fn format_zone(&mut self, zone: ZoneId) -> Result<(), IoError> {
        let _ = zone;
        self.pending("zone", "PCBIO-08")
    }

    /// Port of `format( const ZONE_LAYER_PROPERTIES& aZoneLayerProperties, int aNestLevel, PCB_LAYER_ID aLayer )`: `(property (layer ..) ..)` of a zone or of the board zone defaults. Staging stub (PCBIO-08).
    pub(crate) fn format_zone_layer_properties(
        &mut self,
        properties: &ZoneLayerProperties,
        nest_level: i32,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        let _ = (properties, nest_level, layer);
        self.pending("zone_layer_properties", "PCBIO-08")
    }
}
