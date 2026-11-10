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
//! PCBIO-12: `format( const PCB_REFERENCE_IMAGE* )`, `format( const PCB_POINT* )`, `format( const PCB_TARGET* )`, `format( const PCB_GRID_ITEM* )` (W:1334-1449), `format( const PCB_BARCODE* )`, `format( const PCB_TABLE* )`, `format( const PCB_GENERATED_TABLE* )`, `formatGeneratedTableExtras`, `format( const PCB_DRILL_MAP* )`, `formatTableData` (W:2552-2616, 2776-2999).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-12)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::{BarcodeId, DrillMapId, GridItemId, ImageId, PointId, TableId, TargetId};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_REFERENCE_IMAGE* aBitmap )`: `(image ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_reference_image(&mut self, image: ImageId) -> Result<(), IoError> {
        let _ = image;
        self.pending("image", "PCBIO-12")
    }

    /// Port of `format( const PCB_POINT* aPoint )`: `(point ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_point(&mut self, point: PointId) -> Result<(), IoError> {
        let _ = point;
        self.pending("point", "PCBIO-12")
    }

    /// Port of `format( const PCB_TARGET* aTarget )`: `(target ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_target(&mut self, target: TargetId) -> Result<(), IoError> {
        let _ = target;
        self.pending("target", "PCBIO-12")
    }

    /// Port of `format( const PCB_GRID_ITEM* aGridItem )`: `(grid_item ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_grid_item(&mut self, grid_item: GridItemId) -> Result<(), IoError> {
        let _ = grid_item;
        self.pending("grid_item", "PCBIO-12")
    }

    /// Port of `format( const PCB_BARCODE* aBarcode )`: `(barcode ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_barcode(&mut self, barcode: BarcodeId) -> Result<(), IoError> {
        let _ = barcode;
        self.pending("barcode", "PCBIO-12")
    }

    /// Port of `format( const PCB_TABLE* aTable )`: `(table ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_table(&mut self, table: TableId) -> Result<(), IoError> {
        let _ = table;
        self.pending("table", "PCBIO-12")
    }

    /// Port of `format( const PCB_GENERATED_TABLE* aTable )`: `(drill_chart ..)` / `(stackup_table ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_generated_table(&mut self, table: TableId) -> Result<(), IoError> {
        let _ = table;
        self.pending("generated_table", "PCBIO-12")
    }

    /// Port of `formatGeneratedTableExtras( const PCB_GENERATED_TABLE* )`: what only one kind of generated table carries, written after the shared settings. Staging stub (PCBIO-12).
    pub(crate) fn format_generated_table_extras(&mut self, table: TableId) -> Result<(), IoError> {
        let _ = table;
        self.pending("generated_table_extras", "PCBIO-12")
    }

    /// Port of `format( const PCB_DRILL_MAP* aMap )`: `(drill_map ..)`. Staging stub (PCBIO-12).
    pub(crate) fn format_drill_map(&mut self, drill_map: DrillMapId) -> Result<(), IoError> {
        let _ = drill_map;
        self.pending("drill_map", "PCBIO-12")
    }

    /// Port of `formatTableData( const PCB_TABLE* aTable )`: geometry and cells with no identity, so an enclosing form owns uuid/layer/locked. Staging stub (PCBIO-12).
    pub(crate) fn format_table_data(&mut self, table: TableId) -> Result<(), IoError> {
        let _ = table;
        self.pending("table_data", "PCBIO-12")
    }
}
