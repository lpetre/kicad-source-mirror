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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_arcs.cpp` (suite `CurvedPolys`).

mod common_poly;

use common_poly::{CommonPolyData, is_poly_set_valid};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::test_utils::{check_unordered_matches, is_close_pct};

/// Simplify the polygon a large number of times and check that the area
/// does not change and also that the arcs are the same before and after
#[test]
fn test_simplify() {
    let test_data = CommonPolyData::new();

    let polys_to_test: Vec<(&str, &ShapePolySet)> = vec![
        (
            "Case 1: Single polygon",
            &test_data.holey_curved_poly_single,
        ),
        (
            "Case 2: Wraparound polygon",
            &test_data.curved_poly_wrap_round,
        ),
        // ("Case 3: Multi polygon", &test_data.holey_curved_poly_multi) // This test fails right
        // now: clipper seems to not handle multiple outlines correctly
    ];

    for (name, poly) in polys_to_test {
        let mut test_poly = poly.clone();

        let original_area = test_poly.area();

        let mut original_arcs: Vec<ShapeArc> = Vec::new();
        test_poly.get_arcs(&mut original_arcs);

        for i in 1..=3 {
            test_poly.simplify();

            let mut found_arcs: Vec<ShapeArc> = Vec::new();
            test_poly.get_arcs(&mut found_arcs);

            assert!(is_poly_set_valid(&test_poly), "{name}, iteration {i}");
            assert_eq!(test_poly.area(), original_area, "{name}, iteration {i}");
            assert_eq!(
                original_arcs.len(),
                found_arcs.len(),
                "{name}, iteration {i}"
            );
            let failures = check_unordered_matches(&original_arcs, &found_arcs, |a_a, a_b| {
                // We accept that the arcs could be reversed after Simplify
                a_a == a_b || a_a.reversed() == *a_b
            });
            assert!(failures.is_empty(), "{name}, iteration {i}: {failures:?}");
        }
    }
}

/// Check intersection and union between two polygons
#[test]
fn test_intersect_union() {
    let test_data = CommonPolyData::new();

    let polys_to_test: Vec<(&str, &ShapePolySet)> = vec![
        (
            "Case 1: Single polygon",
            &test_data.holey_curved_poly_single,
        ),
        // ("Case 2: Multi polygon", &test_data.holey_curved_poly_multi) // This test fails right
        // now: clipper seems to not handle multiple outlines correctly
    ];

    for (name, poly) in polys_to_test {
        let mut test_poly = poly.clone();
        let mut op_poly = test_data.holey_curved_poly_inter.clone();

        // Remove all arcs before any booleanOps
        op_poly.clear_arcs();
        test_poly.clear_arcs();

        assert!(is_poly_set_valid(&test_poly), "{name}");
        assert!(is_poly_set_valid(&op_poly), "{name}");

        let test_poly_area = test_poly.area();
        let op_poly_area = op_poly.area();

        let mut intersection_poly = test_poly.clone();
        intersection_poly.boolean_intersection(&op_poly);
        let intersect_area = intersection_poly.area();

        assert!(is_poly_set_valid(&intersection_poly), "{name}");

        let mut union_poly = test_poly.clone();
        union_poly.boolean_add(&op_poly);
        let union_area = union_poly.area();

        assert!(is_poly_set_valid(&union_poly), "{name}");

        // Acceptable error of 0.01% (fails at 0.001% for some - this is a Clipper limitation)
        assert!(
            is_close_pct(
                test_poly_area + op_poly_area - intersect_area,
                union_area,
                0.01
            ),
            "{name}"
        );
    }
}

/// Test SHAPE_POLY_SET::ClearArcs
#[test]
fn test_clear_arcs() {
    let test_data = CommonPolyData::new();

    let polys_to_test: Vec<(&str, &ShapePolySet)> = vec![
        (
            "Case 1: Single polygon",
            &test_data.holey_curved_poly_single,
        ),
        (
            "Case 2: Intersect polygon",
            &test_data.holey_curved_poly_inter,
        ),
        ("Case 3: Multi polygon", &test_data.holey_curved_poly_multi),
    ];

    for (name, poly) in polys_to_test {
        let mut test_poly = poly.clone();
        let original_area = test_poly.area();
        test_poly.clear_arcs();

        assert!(is_poly_set_valid(&test_poly), "{name}");
        assert_eq!(test_poly.area(), original_area, "{name}"); // Area should not have changed

        let mut arc_buffer: Vec<ShapeArc> = Vec::new();
        test_poly.get_arcs(&mut arc_buffer);

        assert_eq!(arc_buffer.len(), 0, "{name}"); // All arcs should have been removed
    }
}
