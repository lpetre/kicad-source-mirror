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

//! Port of `pcbnew/zone.h` and `pcbnew/zone.cpp` (`ZONE`).
//!
//! Placeholder written by M0a; the type package M0b fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

pub mod shapes;

use crate::item::BoardItemBase;
use crate::item::ConnectedItemBase;
use crate::traits::BoardConnectedItem;
use crate::traits::BoardItemMut;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `ZONE`.
///
/// PLACEHOLDER (M0a): M0b replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct Zone {
    pub base: ConnectedItemBase,
}

impl Invalidate for Zone {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for Zone {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbZoneT
    }

    fn class_name(&self) -> &'static str {
        "ZONE"
    }

    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }

    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
}

impl BoardConnectedItem for Zone {}

impl BoardItemMut for Zone {}
