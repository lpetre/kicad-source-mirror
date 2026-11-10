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
//! PCBIO-05: `format( const PCB_TEXT* )`, `format( const PCB_TEXTBOX* )` and `formatRenderCache` (W:573-598, 2619-2773).
//!
//! **Staging stubs** written by PCBIO-02 with the signatures the package fills: each body calls
//! [`PcbWriter::pending`] (error "port pending: <item kind> (PCBIO-05)", or skip and record with
//! `FormatOptions::allow_pending`). Replace the bodies; add private helpers freely.

use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::ids::{ItemId, TextId, TextboxId};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_TEXT* aText )`: `(gr_text ..)` / `(fp_text ..)`. Staging stub (PCBIO-05).
    pub(crate) fn format_text(&mut self, text: TextId) -> Result<(), IoError> {
        let _ = text;
        self.pending("text", "PCBIO-05")
    }

    /// Port of `format( const PCB_TEXTBOX* aTextBox )`: `(gr_text_box ..)` / `(fp_text_box ..)`. Staging stub (PCBIO-05).
    pub(crate) fn format_textbox(&mut self, textbox: TextboxId) -> Result<(), IoError> {
        let _ = textbox;
        self.pending("textbox", "PCBIO-05")
    }

    /// Port of `formatRenderCache( const EDA_TEXT* aText )`: `(render_cache ..)` of the text item `text` (any item with an `EDA_TEXT`). Staging stub (PCBIO-05).
    pub(crate) fn format_render_cache(&mut self, text: ItemId) -> Result<(), IoError> {
        let _ = text;
        self.pending("render_cache", "PCBIO-05")
    }
}
