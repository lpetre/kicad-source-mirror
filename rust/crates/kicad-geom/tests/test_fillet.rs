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

//! Port of `qa/tests/libs/kimath/geometry/test_fillet.cpp` (suite `Fillet`).

mod common_poly;

use common_poly::{build_square_chain, fillet_poly_set};
use kicad_geom::geometry::eda_angle::ANGLE_90;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{
    Quadrant, are_perpendicular, is_within_and_below, segment_completely_in_quadrant,
    segment_completely_within_radius,
};

/// Port of `TestFilletSegmentConstraints()`: a single segment of a fillet complies with the
/// geometric constraints (end points at radius from the centre, acceptable mid point error,
/// segment perpendicular to the radius at its mid point).
fn test_fillet_segment_constraints(
    a_seg: &Seg,
    a_rad_centre: Vector2I,
    a_radius: i32,
    a_error: i32,
) {
    let diff_a = a_rad_centre - a_seg.a;
    let diff_b = a_rad_centre - a_seg.b;
    let diff_c = a_rad_centre - a_seg.center();

    // Check 1: radii (error of 1 for rounding)
    assert!(is_within_and_below(diff_a.euclidean_norm(), a_radius, 1));
    assert!(is_within_and_below(diff_b.euclidean_norm(), a_radius, 1));

    // Check 2: Mid-point error
    assert!(is_within_and_below(
        diff_c.euclidean_norm(),
        a_radius,
        a_error + 1
    ));

    // Check 3: Mid-point -> radius centre perpendicular
    let perpendularity_max_error = ANGLE_90 / 10;
    assert!(are_perpendicular(
        diff_c,
        a_seg.a - a_seg.b,
        perpendularity_max_error
    ));
}

/// Port of `TestSquareFillet()`: create a square, fillet it, and check a corner for correctness.
fn test_square_fillet(a_square_size: i32, a_radius: i32, a_error: i32) {
    let mut square_poly_set = ShapePolySet::new();
    square_poly_set.add_outline(&build_square_chain(a_square_size, Vector2I::new(0, 0)));

    let filleted = fillet_poly_set(&mut square_poly_set, a_radius, a_error);

    // expect a single filleted polygon
    assert_eq!(filleted.outline_count(), 1);

    let rad_centre = Vector2I::new(a_square_size / 2 - a_radius, a_square_size / 2 - a_radius);

    let mut checked = 0;

    for seg in filleted.iterate_segments() {
        // Only check the first Quadrant
        if segment_completely_in_quadrant(&seg, Quadrant::Q1) {
            test_fillet_segment_constraints(&seg, rad_centre, a_radius, a_error);
            checked += 1;
        }
    }

    // we expect there to be at least one segment in the fillet
    assert!(checked > 0);
}

/// Port of `TestConcaveSquareFillet()`: create a square concave corner, fillet and check
/// correctness.
fn test_concave_square_fillet(a_square_size: i32, a_radius: i32, a_error: i32) {
    let mut poly_set = ShapePolySet::new();
    let mut poly_line = ShapeLineChain::new();

    /*
     * L-shape:
     *    ----
     *    |   |
     * ----   |
     * |      |
     * --------
     */
    poly_line.append_point(Vector2I::new(0, 0), false);
    poly_line.append_point(Vector2I::new(0, a_square_size / 2), false);
    poly_line.append_point(Vector2I::new(a_square_size / 2, a_square_size / 2), false);
    poly_line.append_point(Vector2I::new(a_square_size / 2, a_square_size), false);
    poly_line.append_point(Vector2I::new(a_square_size, a_square_size), false);
    poly_line.append_point(Vector2I::new(a_square_size, 0), false);
    poly_line.set_closed(true);

    poly_set.add_outline(&poly_line);

    let filleted = fillet_poly_set(&mut poly_set, a_radius, a_error);

    // expect a single filleted polygon
    assert_eq!(filleted.outline_count(), 1);

    let rad_centre = Vector2I::new(a_square_size / 2 - a_radius, a_square_size / 2 + a_radius);

    let mut checked = 0;

    for seg in filleted.iterate_segments() {
        // Only check segments around the concave corner
        if segment_completely_within_radius(&seg, rad_centre, a_radius + 1) {
            test_fillet_segment_constraints(&seg, rad_centre, a_radius, a_error);
            checked += 1;
        }
    }

    // we expect there to be at least one segment in the fillet
    assert!(checked > 0);
}

/// `squareFilletCases`: square size, radius, error.
const SQUARE_FILLET_CASES: [(i32, i32, i32); 4] = [
    (1000, 120, 10),
    (1000, 10, 1),
    // Large error relative to fillet
    (1000, 10, 5),
    // Very small error relative to fillet(many segments in interpolation)
    (70000, 1000, 1),
];

/// Tests the SHAPE_POLY_SET::FilletPolygon method against certain geometric constraints.
#[test]
fn square_fillet() {
    for (size, radius, error) in SQUARE_FILLET_CASES {
        test_square_fillet(size, radius, error);
    }
}

#[test]
fn square_concave_fillet() {
    for (size, radius, error) in SQUARE_FILLET_CASES {
        test_concave_square_fillet(size, radius, error);
    }
}
