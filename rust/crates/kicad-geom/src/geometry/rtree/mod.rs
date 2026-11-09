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

//! Ports of KiCad's own r-trees in `libs/kimath/include/geometry/rtree/` (namespace
//! `KIRTREE`), which replaced the third-party `thirdparty/rtree`:
//!
//! | C++ | Rust |
//! |---|---|
//! | `rtree_node.h`: `HilbertXY2D`, `HilbertND2D`, `OverlapMask2D`, `RTREE_NODE`, `SLAB_ALLOCATOR` | [`rtree_node`] |
//! | `packed_rtree.h`: `PACKED_RTREE`, `PACKED_RTREE::Builder` | [`PackedRTree`], [`PackedRTreeBuilder`] |
//! | `dynamic_rtree.h`: `DYNAMIC_RTREE`, `BULK_ENTRY`, `Iterator`, `SearchRange` | [`DynamicRTree`], [`BulkEntry`], [`DynamicRTreeIter`] |
//! | `dynamic_rtree_cow.h`: `COW_RTREE` | not ported (only used by the interactive router) |
//!
//! The coordinate type is `i32` (`ELEMTYPE = int` in every KiCad instantiation).  Query
//! boxes are closed intervals `[min, max]` per axis.  The order in which the trees report
//! items is part of their behaviour (DRC reports the first violation it finds), and is
//! reproduced exactly, including the libstdc++ `std::sort` tie order.

pub mod dynamic_rtree;
pub mod packed_rtree;
pub mod rtree_node;

pub use dynamic_rtree::{BulkEntry, DynamicRTree, Iter as DynamicRTreeIter};
pub use packed_rtree::{PackedRTree, PackedRTreeBuilder};
