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

//! Port of `qa/tests/libs/kimath/geometry/test_oval.cpp` (suite `Oval`).

use kicad_geom::geometry::eda_angle::ANGLE_45;
use kicad_geom::geometry::oval::{OVAL_ALL_KEY_POINTS, get_oval_key_points};
use kicad_geom::geometry::point_types::{
    PT_CENTER, PT_END, PT_MID, PT_QUADRANT, PointType, TypedPoint2I,
};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::trigo::get_rotated;

/// Port of `CHECK_COLLECTIONS_SAME_UNORDERED`: the collections contain the same elements,
/// ignoring order (and have the same length, to weed out unexpected duplicates).
fn check_collections_same_unordered(expected: &[TypedPoint2I], actual: &[TypedPoint2I]) {
    for p in expected {
        assert!(actual.contains(p), "Expected item not found: {p:?}");
    }

    for p in actual {
        assert!(expected.contains(p), "Unexpected item: {p:?}");
    }

    assert_eq!(expected.len(), actual.len());
}

fn tp(x: i32, y: i32, t: PointType) -> TypedPoint2I {
    TypedPoint2I::new(Vector2I::new(x, y), t)
}

fn do_oval_point_test_checks(a_oval: &ShapeSegment, a_expected_points: &[TypedPoint2I]) {
    let actual_points = get_oval_key_points(a_oval, OVAL_ALL_KEY_POINTS);
    check_collections_same_unordered(a_expected_points, &actual_points);
}

#[test]
fn simple_oval_vertical() {
    let oval = ShapeSegment::from_seg(
        Seg::new(Vector2I::new(0, -1000), Vector2I::new(0, 1000)),
        1000,
    );
    do_oval_point_test_checks(
        &oval,
        &[
            tp(0, 0, PT_CENTER),
            // Main points
            tp(0, 1500, PT_QUADRANT),
            tp(0, -1500, PT_QUADRANT),
            tp(500, 0, PT_MID),
            tp(-500, 0, PT_MID),
            // Cap centres
            tp(0, 1000, PT_CENTER),
            tp(0, -1000, PT_CENTER),
            // Side segment ends
            tp(500, 1000, PT_END),
            tp(500, -1000, PT_END),
            tp(-500, 1000, PT_END),
            tp(-500, -1000, PT_END),
            // No quadrants
        ],
    );
}

#[test]
fn simple_oval_horizontal() {
    let oval = ShapeSegment::from_seg(
        Seg::new(Vector2I::new(-1000, 0), Vector2I::new(1000, 0)),
        1000,
    );
    do_oval_point_test_checks(
        &oval,
        &[
            tp(0, 0, PT_CENTER),
            // Main points
            tp(0, 500, PT_MID),
            tp(0, -500, PT_MID),
            tp(1500, 0, PT_QUADRANT),
            tp(-1500, 0, PT_QUADRANT),
            // Cap centres
            tp(1000, 0, PT_CENTER),
            tp(-1000, 0, PT_CENTER),
            // Side segment ends
            tp(1000, 500, PT_END),
            tp(1000, -500, PT_END),
            tp(-1000, 500, PT_END),
            tp(-1000, -500, PT_END),
            // No quadrants
        ],
    );
}

#[test]
fn simple_oval_45_degrees() {
    // In this case, it's useful to keep in mind the hypotenuse of
    // isoceles right-angled triangles is sqrt(2) times the length of the sides
    //  500 / sqrt(2) = 354
    // 1000 / sqrt(2) = 707
    // 1500 / sqrt(2) = 1061
    // 2000 / sqrt(2) = 1414
    let oval = ShapeSegment::from_seg(
        Seg::new(
            get_rotated(Vector2I::new(-1500, 0), ANGLE_45),
            get_rotated(Vector2I::new(1500, 0), ANGLE_45),
        ),
        1000,
    );
    do_oval_point_test_checks(
        &oval,
        &[
            tp(0, 0, PT_CENTER),
            // Main points
            tp(1414, -1414, PT_END),
            tp(-1414, 1414, PT_END),
            tp(354, 354, PT_MID),
            tp(-354, -354, PT_MID),
            // Side segment ends
            tp(-1414, 707, PT_END),
            tp(1414, -707, PT_END),
            tp(-707, 1414, PT_END),
            tp(707, -1414, PT_END),
            // Cap centres
            tp(1061, -1061, PT_CENTER),
            tp(-1061, 1061, PT_CENTER),
            // Extremum points (always one of NSEW of a cap centre because 45 degrees)
            tp(-1061 - 500, 1061, PT_QUADRANT),
            tp(-1061, 1061 + 500, PT_QUADRANT),
            tp(1061 + 500, -1061, PT_QUADRANT),
            tp(1061, -1061 - 500, PT_QUADRANT),
        ],
    );
}
