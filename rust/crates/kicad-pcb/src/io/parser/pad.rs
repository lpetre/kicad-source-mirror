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

//! Port of the pad part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-07): `parsePAD`, `parsePAD_primitives`, `parsePAD_option`,
//! `parsePostMachining`, `parsePadstack`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-07)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use kicad_core::PcbLayerId;

use crate::ids::FootprintId;
use crate::pad::Pad;
use crate::padstack::PostMachiningProps;

impl PcbParser {
    /// Port of `parsePAD( FOOTPRINT* aParent )`: the pad as a local value with `parent = Item(fp)`; the caller adds it. Staging stub (PCBIO-07).
    pub(crate) fn parse_pad(
        &mut self,
        parent: Option<FootprintId>,
    ) -> Result<Option<Pad>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-07")?;
        Ok(None)
    }

    /// Port of `parsePAD_primitives( PAD*, PCB_LAYER_ID )`. Staging stub (PCBIO-07).
    pub(crate) fn parse_pad_primitives(
        &mut self,
        pad: &mut Pad,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        let _ = &(pad, layer);
        self.pending("PCBIO-07")?;
        Ok(())
    }

    /// Port of `parsePAD_option( PAD*, PCB_LAYER_ID )`. Staging stub (PCBIO-07).
    pub(crate) fn parse_pad_option(
        &mut self,
        pad: &mut Pad,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        let _ = &(pad, layer);
        self.pending("PCBIO-07")?;
        Ok(())
    }

    /// Port of `parsePostMachining( PADSTACK::POST_MACHINING_PROPS& )`. Staging stub (PCBIO-07).
    pub(crate) fn parse_post_machining(
        &mut self,
        props: &mut PostMachiningProps,
    ) -> Result<(), IoError> {
        let _ = &props;
        self.pending("PCBIO-07")?;
        Ok(())
    }

    /// Port of `parsePadstack( PAD* )`. Staging stub (PCBIO-07).
    pub(crate) fn parse_padstack(&mut self, pad: &mut Pad) -> Result<(), IoError> {
        let _ = &pad;
        self.pending("PCBIO-07")?;
        Ok(())
    }
}
