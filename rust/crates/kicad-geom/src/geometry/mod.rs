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

//! Ports of the headers in `libs/kimath/include/geometry` (and `libs/kimath/src/geometry`).

pub mod approximation;
pub mod arc_chord_params;
pub mod arc_construction;
pub mod circle;
pub mod convex_hull;
pub mod corner_strategy;
pub mod distribute;
pub mod eda_angle;
pub mod ellipse;
pub mod fracture_edge_index_utils;
pub mod geometry_predicates;
pub mod geometry_utils;
pub mod grid_geometry;
pub mod half_line;
pub mod line;
pub mod point_types;
pub mod rtree;
pub mod seg;
pub mod segment_index;
pub mod shape;
pub mod shape_arc;
pub mod shape_circle;
pub mod shape_collisions;
pub mod shape_compound;
pub mod shape_line_chain;
pub mod shape_null;
pub mod shape_poly_set;
pub mod shape_poly_set_fracture;
pub mod shape_rect;
pub mod shape_segment;
pub mod shape_simple;
pub mod shape_utils;
pub mod transform_trs;
pub mod vector_utils;
