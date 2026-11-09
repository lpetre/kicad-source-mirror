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

//! Port of `libs/kimath/include/geometry/segment_index.h` (`SEGMENT_INDEX`).

use crate::geometry::rtree::packed_rtree::PackedRTree;
use crate::geometry::seg::Seg;

/// Port of `SEGMENT_INDEX`: immutable owning spatial snapshot of straight segments.
///
/// Candidate order is unspecified (it is the packed r-tree's visit order, which the port
/// reproduces).  Queries conservatively include every segment whose endpoint bounds overlap
/// the padded query bounds.
#[derive(Debug, Default)]
pub struct SegmentIndex {
    segments: Vec<Seg>,
    tree: PackedRTree<i32, 2>,
}

impl SegmentIndex {
    /// Port of `SEGMENT_INDEX( std::vector<SEG> aSegments )`.
    pub fn new(a_segments: Vec<Seg>) -> Self {
        let mut builder = PackedRTree::<i32, 2>::builder();
        builder.reserve(a_segments.len());

        for (i, segment) in a_segments.iter().enumerate() {
            let min = [segment.a.x.min(segment.b.x), segment.a.y.min(segment.b.y)];
            let max = [segment.a.x.max(segment.b.x), segment.a.y.max(segment.b.y)];
            builder.add(&min, &max, i as i32);
        }

        SegmentIndex {
            segments: a_segments,
            tree: builder.build(),
        }
    }

    /// Port of `SEGMENT_INDEX::size()`.
    pub fn size(&self) -> usize {
        self.segments.len()
    }

    /// Port of `SEGMENT_INDEX::Segment()`.
    pub fn segment(&self, a_index: i32) -> &Seg {
        &self.segments[a_index as usize]
    }

    /// Port of `SEGMENT_INDEX::VisitCandidates()`: visit candidates overlapping `a_query`'s
    /// endpoint bounds expanded by `a_padding`.
    ///
    /// Negative padding is treated as zero so the query never shrinks below the query bounds
    /// and stays conservative.  The visitor receives the original segment index and returns
    /// true to continue or false to stop.
    pub fn visit_candidates<F>(&self, a_query: &Seg, a_padding: i32, mut a_visitor: F)
    where
        F: FnMut(i32) -> bool,
    {
        let padding = a_padding.max(0) as i64;
        let min_x = a_query.a.x.min(a_query.b.x) as i64 - padding;
        let min_y = a_query.a.y.min(a_query.b.y) as i64 - padding;
        let max_x = a_query.a.x.max(a_query.b.x) as i64 + padding;
        let max_y = a_query.a.y.max(a_query.b.y) as i64 + padding;
        let min = [clamp_coordinate(min_x), clamp_coordinate(min_y)];
        let max = [clamp_coordinate(max_x), clamp_coordinate(max_y)];

        self.tree.search(&min, &max, |&i| a_visitor(i));
    }
}

/// Port of `SEGMENT_INDEX::clampCoordinate()`.
fn clamp_coordinate(a_value: i64) -> i32 {
    a_value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}
