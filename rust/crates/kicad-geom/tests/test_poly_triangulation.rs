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

//! Port of `qa/tests/libs/kimath/geometry/test_poly_triangulation.cpp` (suite
//! `PolygonTriangulation`, all 39 test cases).
//!
//! The C++ fixture holds a `TRIANGULATED_POLYGON` and a `POLYGON_TRIANGULATION` referring to
//! it; here the triangulator borrows the result and the checks read it through
//! `PolygonTriangulation::result()`.  Thread based cases run sequentially (the port has no
//! threads); timing checks are kept.

use std::f64::consts::PI;
use std::path::Path;

use kicad_geom::geometry::geometry_predicates::{
    in_circle_delaunay_legal, is_sliver_triangle, orientation_sign,
};
use kicad_geom::geometry::polygon_triangulation::{
    PolygonTriangulation, TRIANGULATE_SIMPLIFICATION_LEVEL,
};
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase};
use kicad_geom::geometry::shape_line_chain::{ShapeLineChain, StringStream};
use kicad_geom::geometry::shape_poly_set::{Polygon, ShapePolySet, TriangulatedPolygon};
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{is_close_pct, triangle_min_angle_deg};

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn chain_of(points: &[(i32, i32)]) -> ShapeLineChain {
    let mut chain = ShapeLineChain::new();

    for &(x, y) in points {
        chain.append_xy(x, y, false);
    }

    chain.set_closed(true);
    chain
}

/// `createSquare()`
fn create_square(size: i32, offset: Vector2I) -> ShapeLineChain {
    chain_of(&[
        (offset.x, offset.y),
        (offset.x + size, offset.y),
        (offset.x + size, offset.y + size),
        (offset.x, offset.y + size),
    ])
}

/// `createTriangle()`
fn create_triangle(size: i32, offset: Vector2I) -> ShapeLineChain {
    chain_of(&[
        (offset.x, offset.y),
        (offset.x + size, offset.y),
        (offset.x + size / 2, offset.y + size),
    ])
}

/// `createConcavePolygon()`
fn create_concave_polygon(size: i32) -> ShapeLineChain {
    chain_of(&[
        (0, 0),
        (size, 0),
        (size, size / 2),
        (size / 2, size / 2),
        (size / 2, size),
        (0, size),
    ])
}

/// `createSerpentinePolygon()`
fn create_serpentine_polygon(step: i32, teeth: i32) -> ShapeLineChain {
    let mut pts = vec![(0, 0)];
    let mut x = 0;

    for _ in 0..teeth {
        x += step;
        pts.push((x, 0));
        pts.push((x, step * 3));
        x += step;
        pts.push((x, step * 3));
        pts.push((x, step * 4));
    }

    pts.push((0, step * 4));
    chain_of(&pts)
}

/// `validateTriangulation()`
fn validate_triangulation(
    result: &TriangulatedPolygon,
    original: &ShapeLineChain,
    strict: bool,
) -> bool {
    if result.vertex_count() == 0 {
        return false;
    }

    if result.triangle_count() == 0 {
        return false;
    }

    let vc = result.vertex_count() as i32;

    for triangle in &result.triangles {
        if triangle.a >= vc || triangle.b >= vc || triangle.c >= vc {
            return false;
        }

        if triangle.a == triangle.b || triangle.b == triangle.c || triangle.a == triangle.c {
            return false;
        }

        if strict && triangle.area() <= 0.0 {
            return false;
        }
    }

    if strict && result.vertex_count() >= original.point_count() as usize {
        for i in 0..original.point_count() {
            if !result.vertices.contains(&original.c_point(i)) {
                return false;
            }
        }
    }

    true
}

/// `countSpikeyTriangles()`
fn count_spikey_triangles(a_result: &TriangulatedPolygon) -> i32 {
    let mut count = 0;

    for tri in &a_result.triangles {
        let pa = tri.get_point(0);
        let pb = tri.get_point(1);
        let pc = tri.get_point(2);

        let ab = pa.distance(pb);
        let bc = pb.distance(pc);
        let ca = pc.distance(pa);

        let longest = ab.max(bc).max(ca);
        let shortest = ab.min(bc).min(ca);

        if shortest > 0.0 && longest / shortest > 10.0 {
            count += 1;
        }
    }

    count
}

/// `parsePolyFileForTest()`
fn parse_poly_file_for_test(a_path: &Path, a_zones: &mut Vec<ShapePolySet>) -> bool {
    let Ok(content) = std::fs::read_to_string(a_path) else {
        return false;
    };

    let mut zone_pos = 0;

    while let Some(found) = content[zone_pos..].find("(zone (layer \"") {
        zone_pos += found;

        if let Some(polyset_start) = content[zone_pos..].find("polyset ") {
            let mut poly_set = ShapePolySet::new();
            let remainder = &content[zone_pos + polyset_start..];
            let mut ss = StringStream::new(remainder);

            if poly_set.parse(&mut ss) {
                a_zones.push(poly_set);
            }
        }

        let Some(layer_end) = content[zone_pos + 14..].find("\")") else {
            break;
        };

        zone_pos = zone_pos + 14 + layer_end + 1;
    }

    !a_zones.is_empty()
}

fn triangulation_data(name: &str) -> std::path::PathBuf {
    kicad_qa::qa_data_dir().join("triangulation").join(name)
}

/// `computeBoardSpikeyRatio()`
fn compute_board_spikey_ratio(a_path: &Path) -> f64 {
    let mut zones = Vec::new();

    if !parse_poly_file_for_test(a_path, &mut zones) {
        return 1.0;
    }

    let mut total_triangles = 0;
    let mut total_spikey = 0;

    for poly_set in &mut zones {
        poly_set.cache_triangulation(false);

        for i in 0..poly_set.triangulated_poly_count() {
            let tri_poly = poly_set.triangulated_polygon(i as i32);
            total_triangles += tri_poly.triangle_count();
            total_spikey += count_spikey_triangles(tri_poly);
        }
    }

    if total_triangles > 0 {
        total_spikey as f64 / total_triangles as f64
    } else {
        0.0
    }
}

fn tri_area_sum(set: &ShapePolySet) -> f64 {
    let mut area = 0.0;

    for ii in 0..set.triangulated_poly_count() {
        for tri in &set.triangulated_polygon(ii as i32).triangles {
            area += tri.area().abs();
        }
    }

    area
}

#[test]
fn fracture_edge_index_matches_linear_scan() {
    // The C++ compares the indexed fracture against the linear scan (ADVANCED_CFG switch).
    // The port has only the indexed variant (PORT_STATUS.md D-P7), so this checks the fixture
    // and that fracturing it gives valid single-contour polygons.
    let poly_path = triangulation_data("vme-wren.kicad_polys");
    let mut zones = Vec::new();

    assert!(parse_poly_file_for_test(&poly_path, &mut zones));

    let mut deepest = 0;

    for zone in &mut zones {
        zone.unfracture();

        for polygon in 0..zone.outline_count() {
            deepest = deepest.max(zone.hole_count(polygon));
        }
    }

    assert!(deepest >= 100);

    for zone in &mut zones {
        zone.fracture(false);

        for polygon in 0..zone.outline_count() {
            assert_eq!(zone.hole_count(polygon), 0);
        }
    }
}

#[test]
fn basic_triangle_triangulation() {
    let mut result = TriangulatedPolygon::new(0);
    let mut triangulator = PolygonTriangulation::new(&mut result);
    let triangle = create_triangle(100, v(0, 0));

    let success = triangulator.tesselate_chain(&triangle, None);

    assert!(success);
    assert_eq!(triangulator.result().vertex_count(), 3);
    assert_eq!(triangulator.result().triangle_count(), 1);
    assert!(validate_triangulation(
        triangulator.result(),
        &triangle,
        true
    ));
}

#[test]
fn basic_square_triangulation() {
    let mut result = TriangulatedPolygon::new(0);
    let mut triangulator = PolygonTriangulation::new(&mut result);
    let square = create_square(100, v(0, 0));

    let success = triangulator.tesselate_chain(&square, None);

    assert!(success);
    assert_eq!(triangulator.result().vertex_count(), 4);
    assert_eq!(triangulator.result().triangle_count(), 2);
    assert!(validate_triangulation(triangulator.result(), &square, true));
}

#[test]
fn split_first_fracture_partition_produces_multiple_leaves() {
    let mut result = TriangulatedPolygon::new(0);
    let triangulator = PolygonTriangulation::new(&mut result);
    let serpentine = create_serpentine_polygon(20000, 16);
    let fractions = triangulator.partition_area_fractions_for_testing(&serpentine, 4);

    assert_eq!(fractions.len(), 4);

    for fraction in fractions {
        assert!(fraction > 0.15);
    }
}

#[test]
fn ear_lookahead_improves_bad_triangulation_case() {
    let poly_path = triangulation_data("bad_triangulation_case.kicad_polys");

    assert!(poly_path.exists());
    assert!(compute_board_spikey_ratio(&poly_path) < 0.47);
}

#[test]
fn concave_polygon_triangulation() {
    let mut result = TriangulatedPolygon::new(0);
    let mut triangulator = PolygonTriangulation::new(&mut result);
    let concave = create_concave_polygon(100000);

    let success = triangulator.tesselate_chain(&concave, None);

    assert!(success);
    assert!(validate_triangulation(
        triangulator.result(),
        &concave,
        true
    ));
    // L-shaped concave polygons should have 4 triangles
    assert_eq!(triangulator.result().triangle_count(), 4);
}

#[test]
fn hint_data_optimization() {
    let square = create_square(100, v(0, 0));

    let mut result1 = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result1).tesselate_chain(&square, None));

    let mut result2 = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result2).tesselate_chain(&square, Some(&result1)));

    // Results should be identical when hint is applicable
    assert_eq!(result1.vertex_count(), result2.vertex_count());
    assert_eq!(result1.triangle_count(), result2.triangle_count());
}

#[test]
fn hint_data_optimization_with_simplified_input() {
    let noisy_square = chain_of(&[(0, 0), (100, 0), (100, 10), (100, 100), (0, 100)]);

    let mut hint = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut hint).tesselate_chain(&noisy_square, None));

    let mut poisoned_triangles = hint.triangles.clone();
    assert!(poisoned_triangles.len() >= 2);
    poisoned_triangles.reverse();
    hint.set_triangles(&poisoned_triangles);

    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&noisy_square, Some(&hint)));
    assert_eq!(result.triangle_count(), poisoned_triangles.len());

    for (got, expected) in result.triangles.iter().zip(poisoned_triangles.iter()) {
        assert_eq!(got.a, expected.a);
        assert_eq!(got.b, expected.b);
        assert_eq!(got.c, expected.c);
    }
}

#[test]
fn hint_data_invalidation() {
    let triangle = create_triangle(100, v(0, 0));
    let mut hint = TriangulatedPolygon::new(0);
    PolygonTriangulation::new(&mut hint).tesselate_chain(&triangle, None);

    // Try to use hint with different polygon (should ignore hint)
    let square = create_square(100, v(0, 0));
    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&square, Some(&hint)));
    assert!(validate_triangulation(&result, &square, true));
}

#[test]
fn degenerate_polygons() {
    let mut r1 = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut r1).tesselate_chain(&ShapeLineChain::new(), None));

    let mut r2 = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut r2).tesselate_chain(&chain_of(&[(0, 0)]), None));

    let mut r3 = TriangulatedPolygon::new(0);
    assert!(
        PolygonTriangulation::new(&mut r3).tesselate_chain(&chain_of(&[(0, 0), (100, 0)]), None)
    );
}

#[test]
fn zero_area_polygon() {
    let zero_area = chain_of(&[(0, 0), (100, 0), (50, 0), (25, 0)]);
    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&zero_area, None));
}

#[test]
fn memory_management() {
    for i in 0..100 {
        let poly = create_square(100 + i, v(i, i));
        let mut result = TriangulatedPolygon::new(0);
        assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&poly, None));
        assert!(validate_triangulation(&result, &poly, false));
    }
}

#[test]
fn large_polygon_stress_test() {
    let mut large_poly = ShapeLineChain::new();
    let num_vertices = 1000;
    let radius = 10000;

    for i in 0..num_vertices {
        let angle = 2.0 * PI * i as f64 / num_vertices as f64;
        let x = (radius as f64 * angle.cos()) as i32;
        let y = (radius as f64 * angle.sin()) as i32;
        large_poly.append_xy(x, y, false);
    }

    large_poly.set_closed(true);

    let start = std::time::Instant::now();
    let mut result = TriangulatedPolygon::new(0);
    let success = PolygonTriangulation::new(&mut result).tesselate_chain(&large_poly, None);
    let duration = start.elapsed();

    assert!(success);
    assert!(validate_triangulation(&result, &large_poly, false));
    assert!(result.triangle_count() > 0);
    assert!(duration.as_millis() < 10000); // Less than 10 seconds
}

#[test]
fn concurrent_triangulation() {
    // The C++ runs 4 threads of 10 triangulations; the port has no threads, so the same 40
    // triangulations run in sequence.
    for t in 0..4 {
        for i in 0..10 {
            let poly = create_square(100 + t * 10 + i, v(t * 100, i * 100));
            let mut result = TriangulatedPolygon::new(0);
            assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&poly, None));
            assert!(validate_triangulation(&result, &poly, false));
        }
    }
}

#[test]
fn self_intersecting_polygon() {
    let bowtie = chain_of(&[(0, 0), (100, 100), (100, 0), (0, 100)]);
    let mut result = TriangulatedPolygon::new(0);
    let success = PolygonTriangulation::new(&mut result).tesselate_chain(&bowtie, None);

    assert!(success);
    assert!(validate_triangulation(&result, &bowtie, false));
}

/// GitLab issue #18083: self-intersecting filled shape is not completely filled.
#[test]
fn issue18083_self_intersecting_polygon_area() {
    const SCALE: i32 = 1000000;
    let outline = chain_of(&[
        (165 * SCALE, 87 * SCALE),
        (179 * SCALE, 87 * SCALE),
        (174 * SCALE, 94 * SCALE),
        (169 * SCALE, 87 * SCALE),
        (167 * SCALE, 94 * SCALE),
    ]);

    let mut poly_set = ShapePolySet::new();
    poly_set.add_outline(&outline);

    assert!(poly_set.is_self_intersecting());

    poly_set.cache_triangulation(false);
    assert!(poly_set.is_triangulation_up_to_date());

    let triangulated_area = tri_area_sum(&poly_set);
    let expected_area_mm_sq = 49.0 * SCALE as f64 * SCALE as f64;

    assert!(
        (triangulated_area - expected_area_mm_sq).abs() < expected_area_mm_sq * 0.01,
        "Triangulated area should match expected area of 49 mm2"
    );
}

#[test]
fn issue18083_indexed_self_touching_outline_area() {
    const SCALE: i32 = 1000000;
    let corners = [
        v(165 * SCALE, 87 * SCALE),
        v(179 * SCALE, 87 * SCALE),
        v(174 * SCALE, 94 * SCALE),
        v(169 * SCALE, 87 * SCALE),
        v(167 * SCALE, 94 * SCALE),
    ];
    let mut outline = ShapeLineChain::new();

    // Subdivision preserves the pinch and its area while exercising the indexed split
    for i in 0..corners.len() {
        let a = corners[i];
        let b = corners[(i + 1) % corners.len()];

        for step in 0..8 {
            outline.append_xy(
                a.x + (b.x - a.x) * step / 8,
                a.y + (b.y - a.y) * step / 8,
                false,
            );
        }
    }

    outline.set_closed(true);
    assert_eq!(outline.point_count(), 40);

    let mut poly_set = ShapePolySet::new();
    poly_set.add_outline(&outline);
    let outline_area = poly_set.area();

    poly_set.cache_triangulation(false);
    assert!(poly_set.is_triangulation_up_to_date());

    let triangulated_area = tri_area_sum(&poly_set);

    assert!((triangulated_area - outline_area).abs() < outline_area * 0.01);
}

#[test]
fn issue25141_fracture_corridor_is_not_a_pinch_point() {
    // Vertex 6 sits 1nm off vertex 1 and the corridor runs just under 45 degrees, so it rounds
    // onto segment 1-2.  The folded lobe makes the direct pass over-cover, which is what routes
    // this through splitSelfTouchingOutlines() at all.
    let mut outline = ShapeLineChain::from_coords(&[
        0, 0, 20000, 0, // corridor mouth
        60001, 40000, // hole ring
        60001, 60000, 80000, 50000, 60001, 40000, 20001, 0, // corridor return
        100000, 0, 100000, 100000, 30000, 20000, // fold
        90000, 90000, 0, 100000,
    ]);
    outline.set_closed(true);

    let mut poly_set = ShapePolySet::new();
    poly_set.add_outline(&outline);

    poly_set.cache_triangulation(false);
    assert!(poly_set.is_triangulation_up_to_date());

    let in_hole = v(64000, 50000);
    let triangles = poly_set.indexable_subshapes();

    assert!(!poly_set.contains(in_hole, -1, 0, false));

    for tri in triangles {
        assert!(!tri.collide_point(in_hole, 0, None, None));
    }
}

#[test]
fn nearly_collinear_vertices() {
    let nearly_collinear = chain_of(&[
        (0, 0),
        (1000000, 0),
        (2000000, 1), // Very small deviation
        (3000000, 0),
        (1500000, 1000000),
    ]);
    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&nearly_collinear, None));
    assert!(validate_triangulation(&result, &nearly_collinear, false));
}

#[test]
fn duplicate_vertices() {
    let mut duplicate = ShapeLineChain::new();

    for (x, y) in [
        (0, 0),
        (0, 0),
        (100, 0),
        (100, 0),
        (100, 100),
        (100, 100),
        (0, 100),
        (0, 100),
    ] {
        duplicate.append_xy(x, y, false);
    }

    duplicate.set_closed(true);

    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&duplicate, None));
    assert!(validate_triangulation(&result, &duplicate, false));
}

#[test]
fn extreme_coordinates() {
    let large = 1000000000;
    let extreme = chain_of(&[(0, 0), (large, 0), (large, large), (0, large)]);
    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&extreme, None));
    assert!(validate_triangulation(&result, &extreme, false));
}

#[test]
fn error_recovery_and_cleanup() {
    let mut result = TriangulatedPolygon::new(0);
    let mut triangulator = PolygonTriangulation::new(&mut result);

    let test_polygons = vec![
        create_square(100, v(0, 0)),
        chain_of(&[(0, 0)]),
        create_triangle(100, v(0, 0)),
    ];

    for poly in &test_polygons {
        // Each triangulation should start with a clean state
        let mut success = triangulator.tesselate_chain(poly, None);
        // Even if triangulation fails, it should not crash
        success |= triangulator.result().triangle_count() > 0;
        assert!(success);
    }
}

#[test]
fn triangulated_polygon_interface() {
    let square = create_square(100, v(0, 0));
    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&square, None));

    if result.triangle_count() > 0 {
        let (a, b, c) = result.get_triangle(0);

        for p in [a, b, c] {
            assert!(p.x >= 0 && p.x <= 100 && p.y >= 0 && p.y <= 100);
        }
    }

    for tri in &result.triangles {
        assert_eq!(tri.get_point_count(), 3);
        assert_eq!(tri.get_segment_count(), 3);
        assert!(tri.area() > 0.0);
        assert!(tri.is_closed());
        assert!(tri.is_solid());
    }
}

#[test]
fn source_outline_index_tracking() {
    let expected_outline_index = 5;
    let mut result = TriangulatedPolygon::new(expected_outline_index);
    let triangle = create_triangle(100, v(0, 0));

    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&triangle, None));
    assert_eq!(result.source_outline_index(), expected_outline_index);
}

#[test]
fn performance_regression() {
    let test_sizes = [10, 50, 100, 500, 1000];
    let mut durations: Vec<u128> = Vec::new();

    for &size in &test_sizes {
        let mut poly = ShapeLineChain::new();

        for i in 0..size {
            let angle = 2.0 * PI * i as f64 / size as f64;
            poly.append_xy(
                (1000.0 * angle.cos()) as i32,
                (1000.0 * angle.sin()) as i32,
                false,
            );
        }

        poly.set_closed(true);

        let start = std::time::Instant::now();
        let mut result = TriangulatedPolygon::new(0);
        assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&poly, None));
        durations.push(start.elapsed().as_micros().max(1));
    }

    // Check that performance scales reasonably (shouldn't be exponential).  As in C++ this
    // is a sanity check only; the durations are clamped to 1 us so tiny runs cannot divide
    // by zero.
    for i in 1..durations.len() {
        let scale_factor = durations[i] as f64 / durations[i - 1] as f64;
        let size_factor = test_sizes[i] as f64 / test_sizes[i - 1] as f64;
        assert!(scale_factor < size_factor * size_factor * 2.0);
    }
}

/// The parallel partition path (a task submitter and a polygon large enough to trigger
/// balanced partitioning: > 50K vertices produces 4 leaves), run sequentially.
#[test]
fn parallel_partition_triangulation() {
    let mut poly_set = ShapePolySet::new();
    let mut outline = ShapeLineChain::new();
    const VERTEX_COUNT: i32 = 120000;
    const CENTER_X: i32 = 5000000;
    const CENTER_Y: i32 = 5000000;
    const RADIUS: i32 = 4000000;

    for i in 0..VERTEX_COUNT {
        let angle = 2.0 * PI * i as f64 / VERTEX_COUNT as f64;
        let x = CENTER_X + (RADIUS as f64 * angle.cos()) as i32;
        let y = CENTER_Y + (RADIUS as f64 * angle.sin()) as i32;
        outline.append_xy(x, y, false);
    }

    outline.set_closed(true);
    poly_set.add_outline(&outline);

    poly_set.cache_triangulation_with(false, true, false);

    assert!(poly_set.is_triangulation_up_to_date());
    assert!(poly_set.triangulated_poly_count() > 1);

    let original_area = outline.area(true).abs();
    let tri_area = tri_area_sum(&poly_set);

    // Partitioning may clip edges, so allow a few percent tolerance
    if original_area > 0.0 {
        let coverage = tri_area / original_area;
        assert!(coverage > 0.90);
        assert!(coverage < 1.10);
    }
}

fn outer_square_10000() -> ShapeLineChain {
    chain_of(&[(0, 0), (10000, 0), (10000, 10000), (0, 10000)])
}

/// Regression test for issue #24059: infinite loop in eliminateHoles() when filterPoints()
/// removes the vertex used as its loop sentinel.
#[test]
fn issue24059_hole_elimination_sentinel_removal() {
    let hole = chain_of(&[(0, 0), (1000, 1000), (0, 2000)]);
    let polygon: Polygon = vec![outer_square_10000(), hole];

    let mut result = TriangulatedPolygon::new(0);
    let ok = PolygonTriangulation::new(&mut result).tesselate_polygon(&polygon, None);

    assert!(ok);
    assert!(result.triangle_count() > 0);
}

/// Regression test for issue #24121: degenerate hole whose points are all duplicates.
#[test]
fn issue24121_degenerate_hole_all_duplicates() {
    let mut hole = ShapeLineChain::new();

    for _ in 0..4 {
        hole.append_xy(5000, 5000, false);
    }

    hole.set_closed(true);

    let polygon: Polygon = vec![outer_square_10000(), hole];
    let mut result = TriangulatedPolygon::new(0);

    assert!(PolygonTriangulation::new(&mut result).tesselate_polygon(&polygon, None));
    assert!(result.triangle_count() > 0);
}

/// Companion regression for #24121: hole vertices spaced below the simplification level.
#[test]
fn issue24121_degenerate_hole_below_simplification() {
    let tiny_hole = chain_of(&[(5000, 5000), (5002, 5000), (5002, 5002), (5000, 5002)]);
    let polygon: Polygon = vec![outer_square_10000(), tiny_hole];
    let mut result = TriangulatedPolygon::new(0);

    assert!(PolygonTriangulation::new(&mut result).tesselate_polygon(&polygon, None));
    assert!(result.triangle_count() > 0);
}

#[test]
fn cache_triangulation_after_move_fresh_poly() {
    let mut poly_set = ShapePolySet::new();
    poly_set.add_outline(&outer_square_10000());

    assert!(!poly_set.is_triangulation_up_to_date());

    // Move() sets m_hashValid=true without triangulating
    poly_set.move_(v(1, 1));
    poly_set.cache_triangulation(false);

    assert!(poly_set.is_triangulation_up_to_date());
    assert!(poly_set.triangulated_poly_count() > 0);
}

#[test]
fn cache_triangulation_after_update_triangulation_data_hash() {
    let mut poly_set = ShapePolySet::new();
    poly_set.add_outline(&outer_square_10000());

    poly_set.update_triangulation_data_hash();
    poly_set.cache_triangulation(false);

    assert!(poly_set.is_triangulation_up_to_date());
    assert!(poly_set.triangulated_poly_count() > 0);
}

fn mesh_min_angle_deg(a_tri: &TriangulatedPolygon) -> f64 {
    let mut min_angle: f64 = 180.0;

    for tri in &a_tri.triangles {
        min_angle = min_angle.min(triangle_min_angle_deg(
            tri.get_point(0),
            tri.get_point(1),
            tri.get_point(2),
        ));
    }

    min_angle
}

fn mesh_area(a_tri: &TriangulatedPolygon) -> f64 {
    a_tri.triangles.iter().map(|t| t.area()).sum()
}

fn mesh_has_edge(a_tri: &TriangulatedPolygon, a_u: i32, a_v: i32) -> bool {
    a_tri.triangles.iter().any(|tri| {
        let has_u = tri.a == a_u || tri.b == a_u || tri.c == a_u;
        let has_v = tri.a == a_v || tri.b == a_v || tri.c == a_v;
        has_u && has_v
    })
}

fn mesh_spikey_count(a_tri: &TriangulatedPolygon) -> i32 {
    a_tri
        .triangles
        .iter()
        .filter(|t| is_sliver_triangle(t.get_point(0), t.get_point(1), t.get_point(2)))
        .count() as i32
}

fn quad(points: [(i32, i32); 4], tris: &[(i32, i32, i32)]) -> TriangulatedPolygon {
    let mut tp = TriangulatedPolygon::new(0);

    for (x, y) in points {
        tp.add_vertex(v(x, y));
    }

    for &(a, b, c) in tris {
        tp.add_triangle(a, b, c);
    }

    tp
}

#[test]
fn refine_flips_non_delaunay_diagonal() {
    // Vertex 3 lies inside the circumcircle of 0-1-2, so diagonal 0-2 is non-Delaunay and must
    // flip to 1-3.
    let mut tp = quad(
        [(0, 0), (10000, 0), (10000, 10000), (2000, 8000)],
        &[(0, 1, 2), (0, 2, 3)],
    );

    let before_angle = mesh_min_angle_deg(&tp);
    let before_area = mesh_area(&tp);

    tp.refine();

    assert_eq!(tp.triangle_count(), 2);
    assert!(is_close_pct(mesh_area(&tp), before_area, 0.001));
    assert!(mesh_min_angle_deg(&tp) > before_angle);

    assert!(mesh_has_edge(&tp, 0, 1));
    assert!(mesh_has_edge(&tp, 1, 2));
    assert!(mesh_has_edge(&tp, 2, 3));
    assert!(mesh_has_edge(&tp, 0, 3));

    assert!(!mesh_has_edge(&tp, 0, 2));
    assert!(mesh_has_edge(&tp, 1, 3));
}

#[test]
fn refine_prefers_fewer_slivers_over_delaunay() {
    let mut tp = quad(
        [(0, 0), (284000, 42000), (276000, 68000), (38000, 126000)],
        &[(0, 1, 2), (0, 2, 3)],
    );

    // Precondition: 0-2 is Delaunay-legal, so a Delaunay-only refine would not touch it.
    assert!(in_circle_delaunay_legal(
        v(0, 0),
        v(284000, 42000),
        v(276000, 68000),
        v(38000, 126000)
    ));
    assert_eq!(mesh_spikey_count(&tp), 1);

    let before_area = mesh_area(&tp);

    tp.refine();

    assert_eq!(tp.triangle_count(), 2);
    assert!(is_close_pct(mesh_area(&tp), before_area, 0.001));
    assert_eq!(mesh_spikey_count(&tp), 0);
    assert!(!mesh_has_edge(&tp, 0, 2));
    assert!(mesh_has_edge(&tp, 1, 3));
}

#[test]
fn refine_leaves_delaunay_mesh_unchanged() {
    let mut tp = quad(
        [(0, 0), (10000, 0), (10000, 10000), (0, 10000)],
        &[(0, 1, 2), (0, 2, 3)],
    );

    let before_area = mesh_area(&tp);

    tp.refine();

    assert_eq!(tp.triangle_count(), 2);
    assert!(is_close_pct(mesh_area(&tp), before_area, 0.001));
    assert!(mesh_has_edge(&tp, 0, 2));
}

#[test]
fn predicates_handle_full_coordinate_range() {
    // The 4e9 nm width exceeds a 32-bit difference; a->b->c is counter-clockwise and must read
    // so.
    let a = v(-2000000000, 0);
    let b = v(2000000000, 0);
    let c = v(2000000000, 1000);

    assert_eq!(orientation_sign(a, b, c), 1);
    assert_eq!(orientation_sign(a, c, b), -1);

    // The same triangle is a needle; the sliver must still register at this span.
    assert!(is_sliver_triangle(a, b, c));
}

#[test]
fn refine_preserves_non_manifold_edge() {
    // Three triangles share edge 0-1, as a hole-bridge pinch produces. That non-manifold edge
    // must act as a boundary and never flip.
    let mut tp = TriangulatedPolygon::new(0);

    for (x, y) in [
        (0, 0),
        (100000, 0),
        (50000, 30000),
        (50000, -30000),
        (50000, 60000),
    ] {
        tp.add_vertex(v(x, y));
    }

    tp.add_triangle(0, 1, 2);
    tp.add_triangle(0, 1, 3);
    tp.add_triangle(0, 1, 4);

    let flat = |tp: &TriangulatedPolygon| -> Vec<i32> {
        tp.triangles.iter().flat_map(|t| [t.a, t.b, t.c]).collect()
    };

    let before = flat(&tp);
    tp.refine();
    let after = flat(&tp);

    assert_eq!(before, after);
    assert!(mesh_has_edge(&tp, 0, 1));
}

fn subdivided_square(size: i32, steps: i32) -> ShapeLineChain {
    let mut chain = ShapeLineChain::new();

    for i in 0..steps {
        chain.append_xy(i * size / steps, 0, false);
    }

    for i in 0..steps {
        chain.append_xy(size, i * size / steps, false);
    }

    for i in 0..steps {
        chain.append_xy(size - i * size / steps, size, false);
    }

    for i in 0..steps {
        chain.append_xy(0, size - i * size / steps, false);
    }

    chain.set_closed(true);
    chain
}

#[test]
fn decimate_removes_collinear_run() {
    let size = 1000000;
    let chain = subdivided_square(size, 20);

    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&chain, None));

    assert_eq!(result.triangle_count(), 2);
    assert!(is_close_pct(
        mesh_area(&result),
        size as f64 * size as f64,
        1e-6
    ));
}

#[test]
fn decimate_keeps_vertices_beyond_band() {
    // Teeth of amplitude just past the simplification level must all survive.
    let mut chain = ShapeLineChain::new();
    let amp = TRIANGULATE_SIMPLIFICATION_LEVEL * 2;
    let half_pitch = 25000;
    let teeth = 10;

    for i in 0..=teeth * 2 {
        chain.append_xy(i * half_pitch, if i % 2 != 0 { amp } else { 0 }, false);
    }

    chain.append_xy(teeth * 2 * half_pitch, -5000000, false);
    chain.append_xy(0, -5000000, false);
    chain.set_closed(true);

    let mut result = TriangulatedPolygon::new(0);
    assert!(PolygonTriangulation::new(&mut result).tesselate_chain(&chain, None));

    assert_eq!(result.triangle_count(), chain.point_count() as usize - 2);
}

#[test]
fn decimate_keeps_fracture_bridges() {
    let size = 1000000;
    let outline = subdivided_square(size, 25);
    let hole = chain_of(&[
        (400000, 400000),
        (400000, 600000),
        (600000, 600000),
        (600000, 400000),
    ]);

    let mut poly = ShapePolySet::new();
    poly.add_outline(&outline);
    poly.add_hole(&hole, -1);

    let area = poly.area();

    poly.fracture(true);
    poly.cache_triangulation(false);

    let mut mesh_total = 0.0;
    let mut triangles = 0;

    for i in 0..poly.triangulated_poly_count() {
        let tp = poly.triangulated_polygon(i as i32);
        triangles += tp.triangle_count();
        mesh_total += mesh_area(tp);
    }

    assert!(is_close_pct(mesh_total, area, 1e-6));

    // The 100 subdivision points carry no geometry; without decimation the fractured ring
    // keeps them all and the mesh is an order of magnitude larger.
    assert!(triangles < 20);
}
