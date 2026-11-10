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
//! boxes, cached per board time stamp in `BoardCaches` (package M20), and
//! `BOARD::GetFootprintCourtyardIndex()`.

use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_geom::geometry::rtree::packed_rtree::PackedRTree;
use kicad_geom::{Box2I, Shape};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::FootprintId;

/// `FOOTPRINT_COURTYARD_INDEX`.
#[derive(Clone, Debug, Default)]
pub struct FootprintCourtyardIndex {
    /// m_tree (`KIRTREE::PACKED_RTREE<FOOTPRINT*, int, 2>`): footprint handles.
    pub(crate) tree: PackedRTree<FootprintId, 2>,
}

impl FootprintCourtyardIndex {
    /// `FOOTPRINT_COURTYARD_INDEX( aBoard )`: build the index from the board's footprint
    /// courtyards (which DRC has already cached).
    pub fn new(ctx: &BoardCtx<'_>) -> Self {
        let board = ctx.board;
        let mut builder = PackedRTree::<FootprintId, 2>::builder();

        builder.reserve(board.footprint_list.len());

        for (id, footprint) in board.footprints() {
            let mut bbox = Box2I::default();
            let mut has_courtyard = false;

            // Index by the union of the front and back courtyard bounds so a single query
            // serves intersectsCourtyard/Front/Back; the downstream per-side test discards any
            // extra hits.
            for side in [PcbLayerId::FCu, PcbLayerId::BCu] {
                let courtyard = footprint.courtyard(ctx, side);

                if courtyard.outline_count() == 0 {
                    continue;
                }

                if has_courtyard {
                    bbox.merge(&courtyard.bbox(0));
                } else {
                    bbox = courtyard.bbox(0);
                }

                has_courtyard = true;
            }

            if !has_courtyard {
                continue;
            }

            let min = [bbox.left(), bbox.top()];
            let max = [bbox.right(), bbox.bottom()];

            builder.add(&min, &max, id);
        }

        FootprintCourtyardIndex {
            tree: builder.build(),
        }
    }

    /// `QueryOverlapping( aBox, aVisitor )`: visit every footprint whose courtyard bounding
    /// box overlaps `bbox`; the visitor returns true to keep searching, false to stop.
    pub fn query_overlapping(&self, bbox: &Box2I, visitor: &mut dyn FnMut(FootprintId) -> bool) {
        let min = [bbox.left(), bbox.top()];
        let max = [bbox.right(), bbox.bottom()];

        self.tree.search(&min, &max, |&id| visitor(id));
    }
}

impl Board {
    /// `BOARD::GetFootprintCourtyardIndex()`: built lazily and kept until the next time stamp
    /// bump (`increment_time_stamp` clears `BoardCaches`, where C++ resets
    /// `m_footprintCourtyardIndex` in `IncrementTimeStamp`).
    pub fn footprint_courtyard_index(&self, ctx: &BoardCtx<'_>) -> Arc<FootprintCourtyardIndex> {
        let cached = self
            .caches
            .footprint_courtyard_index
            .get_or_build(|_| true, || Arc::new(FootprintCourtyardIndex::new(ctx)));

        Arc::clone(&*cached)
    }
}
