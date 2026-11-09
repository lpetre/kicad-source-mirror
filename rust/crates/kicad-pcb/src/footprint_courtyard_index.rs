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

//! Port of `pcbnew/footprint_courtyard_index.h` and `footprint_courtyard_index.cpp`
//! (`FOOTPRINT_COURTYARD_INDEX`): a packed R-tree over the footprints' courtyard bounding
//! boxes, cached per board time stamp in `BoardCaches` (package M20).

use kicad_geom::Box2I;
use kicad_geom::geometry::rtree::packed_rtree::PackedRTree;

use crate::ctx::BoardCtx;
use crate::ids::FootprintId;

/// `FOOTPRINT_COURTYARD_INDEX`.
#[derive(Clone, Debug, Default)]
pub struct FootprintCourtyardIndex {
    /// m_tree (`KIRTREE::PACKED_RTREE<FOOTPRINT*, int, 2>`): footprint handles.
    #[allow(dead_code)] // built and queried by package M20
    pub(crate) tree: PackedRTree<FootprintId, 2>,
}

impl FootprintCourtyardIndex {
    /// `FOOTPRINT_COURTYARD_INDEX( aBoard )`: build the index from the board's footprint
    /// courtyards (which DRC has already cached).
    pub fn new(ctx: &BoardCtx<'_>) -> Self {
        crate::port_pending!("FOOTPRINT_COURTYARD_INDEX::FOOTPRINT_COURTYARD_INDEX", ctx)
    }

    /// `QueryOverlapping( aBox, aVisitor )`: visit every footprint whose courtyard bounding
    /// box overlaps `bbox`; the visitor returns true to keep searching, false to stop.
    pub fn query_overlapping(&self, bbox: &Box2I, visitor: &mut dyn FnMut(FootprintId) -> bool) {
        crate::port_pending!("FOOTPRINT_COURTYARD_INDEX::QueryOverlapping", bbox, visitor)
    }
}
