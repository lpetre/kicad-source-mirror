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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set.cpp` (suite `ShapePolySet`).

use kicad_geom::geometry::shape_poly_set::ShapePolySet;

#[test]
fn remove_null_segments() {
    let mut base_set = ShapePolySet::new();

    base_set.new_outline();
    base_set.append(0, 0, -1, -1, true);
    base_set.append(0, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 0, -1, -1, true);

    let removed = base_set.remove_null_segments();

    assert_eq!(removed, 3);
    assert_eq!(base_set.vertex_count(-1, -1), 4);

    base_set.delete_polygon(0);

    base_set.new_outline();
    base_set.append(0, 0, -1, -1, true);
    base_set.append(0, 10, -1, -1, true);
    base_set.append(0, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 0, -1, -1, true);
    base_set.append(10, 0, -1, -1, true);
    base_set.append(0, 0, -1, -1, true);

    let removed = base_set.remove_null_segments();

    assert_eq!(removed, 4);
    assert_eq!(base_set.vertex_count(-1, -1), 4);
}

#[test]
fn get_neighbour_indexes() {
    let mut base_set = ShapePolySet::new();

    base_set.new_outline();
    base_set.append(0, 0, -1, -1, true);
    base_set.append(0, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 0, -1, -1, true);

    // Check we're testing what we think
    assert!(base_set.outline_count() == 1);
    assert!(base_set.full_point_count() == 4);

    assert_eq!(base_set.get_neighbour_indexes(0), Some((3, 1)));
    assert_eq!(base_set.get_neighbour_indexes(1), Some((0, 2)));
    assert_eq!(base_set.get_neighbour_indexes(2), Some((1, 3)));
    assert_eq!(base_set.get_neighbour_indexes(3), Some((2, 0)));
    assert_eq!(base_set.get_neighbour_indexes(4), None);
    assert_eq!(base_set.get_neighbour_indexes(-1), None);
}

#[test]
fn get_neighbour_indexes_multi_outline() {
    let mut base_set = ShapePolySet::new();

    base_set.new_outline();
    base_set.append(0, 0, -1, -1, true);
    base_set.append(0, 10, -1, -1, true);
    base_set.append(10, 10, -1, -1, true);
    base_set.append(10, 0, -1, -1, true);

    base_set.new_outline();
    base_set.append(20, 0, -1, -1, true);
    base_set.append(20, 10, -1, -1, true);
    base_set.append(30, 10, -1, -1, true);
    base_set.append(30, 0, -1, -1, true);

    // Check we're testing what we think
    assert!(base_set.outline_count() == 2);
    assert!(base_set.full_point_count() == 8);

    // Can we still get outline 0?
    assert_eq!(base_set.get_neighbour_indexes(0), Some((3, 1)));
    // End out outline 0
    assert_eq!(base_set.get_neighbour_indexes(3), Some((2, 0)));
    // Check outline 1
    assert_eq!(base_set.get_neighbour_indexes(4), Some((7, 5)));
    // End out outline 1
    assert_eq!(base_set.get_neighbour_indexes(7), Some((6, 4)));
    // Bad indexes
    assert_eq!(base_set.get_neighbour_indexes(8), None);
    assert_eq!(base_set.get_neighbour_indexes(-1), None);
}
