// This program source code file is part of KiCad, a free EDA CAD application.
//
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

//! Port of `pcbnew/pcb_field.h` and `pcbnew/pcb_field.cpp` (`PCB_FIELD`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

use crate::eda_text::EdaText;
use crate::item::BoardItemBase;
use crate::pcb_text::PcbText;
use crate::traits::BoardItemMut;
use crate::traits::EdaTextItem;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `PCB_FIELD`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbField {
    /// PCB_FIELD : PCB_TEXT.
    pub text: PcbText,
}

impl Invalidate for PcbField {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbField {
    fn base(&self) -> &BoardItemBase {
        &self.text.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.text.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbFieldT
    }

    fn class_name(&self) -> &'static str {
        "PCB_FIELD"
    }
}

impl BoardItemMut for PcbField {}

/// A field is its own `EDA_TEXT` host (PCB_FIELD overrides `GetShownText`), so that
/// `ItemRef::as_eda_text()` and the generic `eda_text` functions dispatch to the field's
/// overrides and not to the embedded `PcbText`. M0c adds a shim for every `EDA_TEXT` virtual
/// that PCB_FIELD or PCB_TEXT overrides, each forwarding to an inherent `PcbField` method.
impl EdaTextItem for PcbField {
    fn eda_text(&self) -> &EdaText {
        &self.text.text
    }

    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text.text
    }
}
