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

//! Port of `pcbnew/pcb_dimension.h` and `pcbnew/pcb_dimension.cpp` (`PCB_DIMENSION_BASE and the five PCB_DIM_* classes`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

use crate::item::BoardItemBase;
use crate::pcb_text::PcbText;
use crate::traits::BoardItemMut;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `PCB_DIMENSION_BASE and the five PCB_DIM_* classes`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbDimension {
    /// PCB_DIMENSION_BASE : PCB_TEXT.
    pub text: PcbText,
}

impl Invalidate for PcbDimension {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbDimension {
    fn base(&self) -> &BoardItemBase {
        &self.text.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.text.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbDimAlignedT
    }

    fn class_name(&self) -> &'static str {
        "PCB_DIM_ALIGNED"
    }
}

impl BoardItemMut for PcbDimension {}
