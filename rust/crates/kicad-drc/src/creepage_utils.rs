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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of `pcbnew/drc/drc_creepage_utils.h` / `drc_creepage_utils.cpp`.
//!
//! Part 1 (work package DRC-32): the shape classes ([`shapes`]), every shape-pair path
//! construction and `PATH_CONNECTION` ([`paths`]), `BuildCreepageBoardEdges`
//! ([`board_edges`]) and the graph model with the board-edge graph construction
//! ([`graph`]). Part 2 (DRC-33): `SegmentIntersectsBoard`, `segmentIntersectsCircle`
//! ([`board_intersect`]), `GetPaths`, `CREEPAGE_GRAPH::Solve`, `Addshape`, `GeneratePaths`,
//! `AddNetElements` ([`solve`]).

pub mod board_edges;
pub mod board_intersect;
pub mod graph;
pub mod paths;
pub mod shapes;
pub mod solve;

/// The placeholder module of part 1 (DRC-32), kept as an alias so its callers keep compiling.
pub mod pending {
    pub use super::board_intersect::segment_intersects_board;
}

pub use board_edges::build_creepage_board_edges;
pub use board_intersect::{segment_intersects_board, segment_intersects_circle};
pub use graph::{
    ConnId, CreepageGraph, GraphConnection, GraphNode, GraphNodeType, NodeId, ShapeId,
};
pub use paths::{CreepageTrackEntry, PathConnection, TrackRtree};
pub use shapes::{
    BeShapeArc, BeShapeCircle, BeShapePoint, CreepItem, CreepShape, CreepShapeBase, CreepShapeType,
    CuShapeArc, CuShapeCircle, CuShapeSegment,
};
pub use solve::get_paths;
