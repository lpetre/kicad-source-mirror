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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_split_outlines.cpp` (suite
//! `ShapePolySetSplitOutlines`).

mod common_poly;

use common_poly::is_poly_set_valid;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;

#[test]
fn split_coincident_outline_opposite_direction() {
    // ASCII art representation of the polygon:
    //   1-----------------0
    //   |                 |
    //   |                 |
    //   |    4------5     |
    //   |    |      |     |
    //  2/9--3/8     |     |
    //   |    |      |     |
    //   10---7------6----11
    let mut poly = ShapePolySet::new();
    let mut outline1 = ShapeLineChain::from_coords(&[
        7600000, 9000000, 6600000, 9000000, 6600000, 8750000, 7000000, 8750000, 7000000, 9000000,
        7250000, 9000000, 7250000, 8500000, 7000000, 8500000, 7000000, 8750000, 6600000, 8750000,
        6600000, 8000000, 7600000, 8000000,
    ]);
    outline1.set_closed(true);
    poly.add_outline(&outline1);
    poly.simplify();

    assert_eq!(poly.outline_count(), 1);
    assert_eq!(poly.c_outline(0).point_count(), 10); //Why is this 10?  I think it should probably be 8
    assert!(is_poly_set_valid(&poly));
}

#[test]
fn split_coincident_outline_same_direction() {
    // This polygon has a self-intersecting/overlapping path that creates
    // coincident edges going in the same direction (points 7->2 and 2->7)
    let mut poly = ShapePolySet::new();
    let mut outline1 = ShapeLineChain::from_coords(&[
        9912310, 8325057, 9288816, 8948550, 8000000, 8567586, 9428364, 8547698, 9585009, 8652365,
        9613140, 8624234, 9471719, 8482813, 9428364, 8547698, 8000000, 8567586,
    ]);
    outline1.set_closed(true);
    poly.add_outline(&outline1);
    poly.simplify();

    assert_eq!(poly.outline_count(), 1);
    assert_eq!(poly.c_outline(0).point_count(), 7);
    assert!(is_poly_set_valid(&poly));
}
