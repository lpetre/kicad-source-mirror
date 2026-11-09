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

//! Port of `pcbnew/pcb_shape.h` and `pcbnew/pcb_shape.cpp` (`PCB_SHAPE`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

pub mod lib_frame;
pub mod shapes;

use crate::eda_shape::EdaShape;
use crate::item::BoardItemBase;
use crate::item::ConnectedItemBase;
use crate::traits::BoardConnectedItem;
use crate::traits::BoardItemMut;
use crate::traits::EdaShapeItem;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `PCB_SHAPE`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbShape {
    pub base: ConnectedItemBase,
    /// The `EDA_SHAPE` part (baked board-frame copy for footprint children).
    pub shape: EdaShape,
}

impl Invalidate for PcbShape {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbShape {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbShapeT
    }

    fn class_name(&self) -> &'static str {
        "PCB_SHAPE"
    }

    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }

    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
}

impl BoardConnectedItem for PcbShape {}

impl BoardItemMut for PcbShape {}

impl EdaShapeItem for PcbShape {
    fn eda_shape(&self) -> &EdaShape {
        &self.shape
    }

    fn eda_shape_mut(&mut self) -> &mut EdaShape {
        &mut self.shape
    }
}
