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

//! Port of the remaining item parsers of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-12): `parsePCB_REFERENCE_IMAGE`, `parsePCB_BARCODE`, `parsePCB_TABLECELL`,
//! `parsePCB_TABLE`, `parseTableBody*`, `parseGeneratedTable*`, `parseDrillSpanBody`,
//! `parsePCB_DRILL_MAP`, `parsePCB_TARGET`, `parsePCB_POINT`, `parsePCB_GRID_ITEM`.
//!
//! Tables are created in place (`create_table` / `table_add_cell`) and returned unattached;
//! the caller attaches them (`attach_table`).
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-12)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::ids::TableId;
use crate::item::ParentRef;
use crate::pcb_barcode::PcbBarcode;
use crate::pcb_drill_map::{DrillSpan, PcbDrillMap};
use crate::pcb_generated_table::GeneratedTableKind;
use crate::pcb_grid_item::PcbGridItem;
use crate::pcb_point::PcbPoint;
use crate::pcb_reference_image::PcbReferenceImage;
use crate::pcb_target::PcbTarget;

impl PcbParser {
    /// Port of `parsePCB_REFERENCE_IMAGE( BOARD_ITEM* aParent )`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_reference_image(&mut self, parent: ParentRef) -> Result<Option<PcbReferenceImage>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_BARCODE( BOARD_ITEM* aParent )`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_barcode(&mut self, parent: ParentRef) -> Result<Option<PcbBarcode>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_TABLE( BOARD_ITEM* aParent )`: a created, unattached table. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_table(&mut self, parent: ParentRef) -> Result<Option<TableId>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parseGeneratedTable( std::unique_ptr<PCB_GENERATED_TABLE> )`: `drill_chart` / `stackup_table`, created, unattached. Staging stub (PCBIO-12).
    pub(crate) fn parse_generated_table(&mut self, kind: GeneratedTableKind, parent: ParentRef) -> Result<Option<TableId>, IoError> {
        let _ = &(kind, parent);
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parseDrillSpanBody`. Staging stub (PCBIO-12).
    pub(crate) fn parse_drill_span_body(&mut self) -> Result<Option<DrillSpan>, IoError> {
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_DRILL_MAP( BOARD_ITEM* aParent )`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_drill_map(&mut self, parent: ParentRef) -> Result<Option<PcbDrillMap>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_TARGET`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_target(&mut self) -> Result<Option<PcbTarget>, IoError> {
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_POINT`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_point(&mut self) -> Result<Option<PcbPoint>, IoError> {
        self.pending("PCBIO-12")?;
        Ok(None)
    }

    /// Port of `parsePCB_GRID_ITEM`. Staging stub (PCBIO-12).
    pub(crate) fn parse_pcb_grid_item(&mut self) -> Result<Option<PcbGridItem>, IoError> {
        self.pending("PCBIO-12")?;
        Ok(None)
    }
}
