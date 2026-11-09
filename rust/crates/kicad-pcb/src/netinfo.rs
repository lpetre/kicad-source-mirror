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

//! Port of `pcbnew/netinfo.h` and `pcbnew/netinfo_item.cpp` and `pcbnew/netinfo_list.cpp` (`NETINFO_ITEM`).
//!
//! Placeholder written by M0a; the type package M0d fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

use crate::ids::NetCode;
use crate::item::BoardItemBase;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `NETINFO_ITEM`.
///
/// PLACEHOLDER (M0a): M0d replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct NetinfoItem {
    /// It is a BOARD_ITEM in C++: uuid, parent = Board; `base.id` is the `ItemKind::Net` handle.
    pub base: BoardItemBase,
    /// m_netCode
    pub net_code: NetCode,
}

impl Invalidate for NetinfoItem {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for NetinfoItem {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbNetinfoT
    }

    fn class_name(&self) -> &'static str {
        "NETINFO_ITEM"
    }
}

/// `NETINFO_LIST`.
///
/// PLACEHOLDER (M0a): M0d fills in the fields (`nets` by code, `names` by name) and `Default`;
/// the lookups of `board/nets.rs` (M3) read them.
#[derive(Clone, Debug, Default)]
pub struct NetinfoList {}
