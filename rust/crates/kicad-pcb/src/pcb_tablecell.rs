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

//! Port of `pcbnew/pcb_tablecell.h` and `pcbnew/pcb_tablecell.cpp` (`PCB_TABLECELL`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

use crate::item::BoardItemBase;
use crate::item::ConnectedItemBase;
use crate::pcb_textbox::PcbTextbox;
use crate::traits::BoardConnectedItem;
use crate::traits::BoardItemMut;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `PCB_TABLECELL`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbTablecell {
    /// PCB_TABLECELL : PCB_TEXTBOX. `parent == ParentRef::Item(table)`.
    pub textbox: PcbTextbox,
}

impl Invalidate for PcbTablecell {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbTablecell {
    fn base(&self) -> &BoardItemBase {
        &self.textbox.shape.base.item
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.textbox.shape.base.item
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbTablecellT
    }

    fn class_name(&self) -> &'static str {
        "PCB_TABLECELL"
    }

    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.textbox.shape.base)
    }

    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.textbox.shape.base)
    }
}

impl BoardConnectedItem for PcbTablecell {}

impl BoardItemMut for PcbTablecell {}
