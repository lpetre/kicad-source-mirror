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

//! Port of the text part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-05): `parseEDA_TEXT`, `parseRenderCache`, `parsePCB_TEXT`,
//! `parsePCB_TEXT_effects`, `parsePCB_TEXTBOX`, `parseTextBoxContent`, `bakeTextBoxLib`.
//!
//! **Staging stubs** written by PCBIO-01 with the signatures the package fills: each body calls
//! [`PcbParser::pending`] (error "port pending: <token> (PCBIO-05)", or skip and record with
//! `ParseOptions::allow_pending`) and returns nothing. Item parsers return `Ok(None)` only
//! while they are stubs (or where the C++ returns null); once ported they return `Some`.

use kicad_sexpr::IoError;

use super::PcbParser;
use crate::eda_text::EdaText;
use crate::item::ParentRef;
use crate::pcb_text::PcbText;
use crate::pcb_textbox::PcbTextbox;

impl PcbParser {
    /// Port of `parseEDA_TEXT( EDA_TEXT* )`: call with `CurTok() == T_effects`. Staging stub (PCBIO-05).
    pub(crate) fn parse_eda_text(&mut self, text: &mut EdaText) -> Result<(), IoError> {
        let _ = &text;
        self.pending("PCBIO-05")?;
        Ok(())
    }

    /// Port of `parseRenderCache( EDA_TEXT* )`. Staging stub (PCBIO-05).
    pub(crate) fn parse_render_cache(&mut self, text: &mut EdaText) -> Result<(), IoError> {
        let _ = &text;
        self.pending("PCBIO-05")?;
        Ok(())
    }

    /// Port of `parsePCB_TEXT( BOARD_ITEM* aParent, PCB_TEXT* aBaseText )`: `gr_text` / `fp_text`. (Footprint fields are PCBIO-05/06's to route.) Staging stub (PCBIO-05).
    pub(crate) fn parse_pcb_text(
        &mut self,
        parent: ParentRef,
        base_text: Option<&PcbText>,
    ) -> Result<Option<PcbText>, IoError> {
        let _ = &(parent, base_text);
        self.pending("PCBIO-05")?;
        Ok(None)
    }

    /// Port of `parsePCB_TEXT_effects( PCB_TEXT*, PCB_TEXT* aBaseText )`: returns whether the text was hidden. Staging stub (PCBIO-05).
    pub(crate) fn parse_pcb_text_effects(
        &mut self,
        text: &mut PcbText,
        base_text: Option<&PcbText>,
    ) -> Result<bool, IoError> {
        let _ = &(text, base_text);
        self.pending("PCBIO-05")?;
        Ok(false)
    }

    /// Port of `parsePCB_TEXTBOX( BOARD_ITEM* aParent )`: `gr_text_box` / `fp_text_box`. Staging stub (PCBIO-05).
    pub(crate) fn parse_pcb_textbox(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbTextbox>, IoError> {
        let _ = &parent;
        self.pending("PCBIO-05")?;
        Ok(None)
    }

    /// Port of `parseTextBoxContent( PCB_TEXTBOX* )` (shared with table cells). Staging stub (PCBIO-05).
    pub(crate) fn parse_text_box_content(
        &mut self,
        textbox: &mut PcbTextbox,
    ) -> Result<(), IoError> {
        let _ = &textbox;
        self.pending("PCBIO-05")?;
        Ok(())
    }
}
