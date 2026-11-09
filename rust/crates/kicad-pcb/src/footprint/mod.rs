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

//! Port of `pcbnew/footprint.h` and `pcbnew/footprint.cpp` (`FOOTPRINT`).
//!
//! Placeholder written by M0a; the type package M0b fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

pub mod compare;
pub mod geometry;
pub mod shapes;
pub mod transform;

use crate::ids::{Body3dId, ConstraintId, FieldId, GroupId, ItemId, PadId, PointId, ZoneId};
use crate::item::BoardItemBase;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;
use kicad_geom::geometry::transform_trs::TransformTrs;
use std::collections::VecDeque;

/// `FOOTPRINT`.
///
/// PLACEHOLDER (M0a): M0b replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct Footprint {
    /// `base.layer` is F_Cu or B_Cu (the side).
    pub base: BoardItemBase,
    /// m_fields (std::deque); always appended. Child lists are `pub(crate)` in the final type.
    pub(crate) fields: VecDeque<FieldId>,
    /// m_drawings (Shape, Text, Textbox, Table, Dimension, Barcode, Image).
    pub(crate) drawings: VecDeque<ItemId>,
    /// m_pads
    pub(crate) pads: VecDeque<PadId>,
    /// m_zones
    pub(crate) zones: VecDeque<ZoneId>,
    /// m_groups
    pub(crate) groups: VecDeque<GroupId>,
    /// m_constraints
    pub(crate) constraints: VecDeque<ConstraintId>,
    /// m_points
    pub(crate) points: VecDeque<PointId>,
    /// m_bodies
    pub(crate) bodies: VecDeque<Body3dId>,
    /// m_transform; written only by `Board::footprint_*` (board-model.md 8.3).
    pub(crate) transform: TransformTrs,
    /// m_flipped
    pub(crate) flipped: bool,
}

impl Invalidate for Footprint {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for Footprint {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbFootprintT
    }

    fn class_name(&self) -> &'static str {
        "FOOTPRINT"
    }
}

impl Footprint {
    /// Drop the geometry caches that depend on the children (bounding boxes, hull,
    /// courtyard), called by `Board::modify_*` after a child changed.
    /// PLACEHOLDER (M0a): the placeholder has no caches; M0b / M20 clear them here.
    pub(crate) fn invalidate_geometry_caches(&mut self) {}
}
