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

//! Port of the footprint part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-06): `parseFOOTPRINT`, `parseFOOTPRINT_unchecked`, `parse3DModel`,
//! `parseFootprintStackup`, `parseFootprintVariant`.
//!
//! The footprint is created in place (`Board::create_footprint`, `board-model.md` 8.1 / 13),
//! filled with `footprint_add` and returned unattached; the caller attaches it
//! (`attach_footprint( fp, BulkAppend )` in `parseBOARD`).
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-06)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::footprint::Fp3dmodel;
use crate::ids::FootprintId;

impl PcbParser {
    /// Port of `parseFOOTPRINT( wxArrayString* aInitialComments )`: `parseFOOTPRINT_unchecked` with a `PARSE_ERROR` of a too recent footprint turned into a `FUTURE_FORMAT_ERROR`. Staging stub (PCBIO-06).
    pub(crate) fn parse_footprint(
        &mut self,
        initial_comments: Option<Vec<String>>,
    ) -> Result<Option<FootprintId>, IoError> {
        let _ = &initial_comments;
        self.pending("PCBIO-06")?;
        Ok(None)
    }

    /// Port of `parse3DModel( bool aFileNameAlreadyParsed )`. Staging stub (PCBIO-06).
    pub(crate) fn parse_3d_model(
        &mut self,
        file_name_already_parsed: bool,
    ) -> Result<Option<Fp3dmodel>, IoError> {
        let _ = &file_name_already_parsed;
        self.pending("PCBIO-06")?;
        Ok(None)
    }

    /// Port of `parseFootprintStackup( FOOTPRINT& )`. Staging stub (PCBIO-06).
    pub(crate) fn parse_footprint_stackup(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let _ = &fp;
        self.pending("PCBIO-06")?;
        Ok(())
    }

    /// Port of `parseFootprintVariant( FOOTPRINT* )`. Staging stub (PCBIO-06).
    pub(crate) fn parse_footprint_variant(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let _ = &fp;
        self.pending("PCBIO-06")?;
        Ok(())
    }
}
