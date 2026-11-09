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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_triangle_index.cpp` (suite
//! `SPSTriangleIndex`, 6 of 6): the spatial index over the cached triangulation.
//!
//! `Collide( SHAPE* )` caches the triangulation through a `const_cast` in C++; the cases that
//! rely on that side effect (the index exists afterwards) use `collide_shape_mut()`, the
//! others the `&self` `collide_shape()`, which gives the same results.  The C++ pointer
//! identity check of `TriPointersSurviveMove` becomes "same triangles, moved".

use std::f64::consts::PI;

use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase};
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

/// `MakeNgon()`: a regular polygon.
fn make_ngon(a_centre: Vector2I, a_radius: i32, a_vertices: i32) -> ShapeLineChain {
    let mut chain = ShapeLineChain::new();

    for ii in 0..a_vertices {
        let angle = 2.0 * PI * ii as f64 / a_vertices as f64;
        chain.append_xy(
            a_centre.x + ki_round(a_radius as f64 * angle.cos()),
            a_centre.y + ki_round(a_radius as f64 * angle.sin()),
            false,
        );
    }

    chain.set_closed(true);
    chain
}

/// `MakeBlob()`: one big outline, two holes, comfortably over 64 triangles.
fn make_blob() -> ShapePolySet {
    let mut poly = ShapePolySet::new();
    poly.add_outline(&make_ngon(v(0, 0), 1000000, 180));
    poly.add_hole(&make_ngon(v(-400000, 0), 150000, 60), -1);
    poly.add_hole(&make_ngon(v(400000, 250000), 120000, 60), -1);
    poly.cache_triangulation(false);
    poly
}

/// `PROBE_RESULT`: the collision flag and distance (the location is deliberately not
/// compared).
#[derive(Debug, PartialEq, Eq)]
struct ProbeResult {
    collides: bool,
    actual: i32,
}

/// `Probe()`
fn probe(a_poly: &ShapePolySet, a_shape: &dyn Shape, a_clearance: i32) -> ProbeResult {
    let mut actual = -1;
    let collides = a_poly.collide_shape(a_shape, a_clearance, Some(&mut actual), None);
    ProbeResult { collides, actual }
}

/// `ReferenceScan()`: an independent full scan over every cached triangle.
fn reference_scan(a_poly: &ShapePolySet, a_shape: &dyn Shape, a_clearance: i32) -> ProbeResult {
    let mut best = i32::MAX;

    for ii in 0..a_poly.triangulated_poly_count() {
        for tri in &a_poly.triangulated_polygon(ii as i32).triangles {
            let mut tri_actual = 0;

            if a_shape.collide(tri, a_clearance, Some(&mut tri_actual), None) {
                best = best.min(tri_actual);
            }
        }
    }

    if best < i32::MAX {
        ProbeResult {
            collides: true,
            actual: 0.max(best),
        }
    } else {
        ProbeResult {
            collides: false,
            actual: -1,
        }
    }
}

/// `ProbeBool()`: `Collide()` with no out-parameters.
fn probe_bool(a_poly: &ShapePolySet, a_shape: &dyn Shape, a_clearance: i32) -> bool {
    a_poly.collide_shape(a_shape, a_clearance, None, None)
}

/// `ReferenceScanBool()`
fn reference_scan_bool(a_poly: &ShapePolySet, a_shape: &dyn Shape, a_clearance: i32) -> bool {
    (0..a_poly.triangulated_poly_count()).any(|ii| {
        a_poly
            .triangulated_polygon(ii as i32)
            .triangles
            .iter()
            .any(|tri| a_shape.collide(tri, a_clearance, None, None))
    })
}

/// `WarmAndRequireIndexed()`: build the index by running one query, and assert it exists
/// afterwards.
fn warm_and_require_indexed(a_poly: &mut ShapePolySet) {
    let warmup = ShapeRect::from_pos_size(v(0, 0), 1000, 1000);
    a_poly.collide_shape_mut(&warmup, 0, None, None);

    assert!(a_poly.triangulated_poly_count() > 0);
    assert!(a_poly.triangulation_index().is_some());
}

/// `g_probes`: fixed probe positions straddling the outline, both holes and empty space.
const G_PROBES: [(i32, i32); 7] = [
    (0, 0),             // deep inside
    (999000, 0),        // just inside the outline
    (1001000, 0),       // just outside the outline
    (-400000, 0),       // centre of hole 1
    (-250000, 0),       // just outside hole 1
    (400000, 250000),   // centre of hole 2
    (3000000, 3000000), // far away
];

#[test]
fn first_cold_collide_builds_the_index() {
    let mut poly = ShapePolySet::new();
    poly.add_outline(&make_ngon(v(0, 0), 1000000, 180));
    poly.add_hole(&make_ngon(v(-400000, 0), 150000, 60), -1);
    poly.add_hole(&make_ngon(v(400000, 250000), 120000, 60), -1);

    let probe_rect = ShapeRect::from_pos_size(v(0, 0), 1000, 1000);
    poly.collide_shape_mut(&probe_rect, 0, None, None);

    assert!(poly.triangulated_poly_count() > 0);
    assert!(poly.triangulation_index().is_some());
}

#[test]
fn tri_pointers_survive_move() {
    let mut poly = make_blob();

    warm_and_require_indexed(&mut poly);

    let before: Vec<Vector2I> = poly
        .triangulated_polys()
        .iter()
        .flat_map(|t| t.triangles.iter().map(|tri| tri.get_point(0)))
        .collect();
    assert!(before.len() > 64);
    assert_eq!(before.len(), poly.indexable_subshape_count());

    let delta = v(123456, -654321);
    poly.move_(delta);

    let after: Vec<Vector2I> = poly
        .triangulated_polys()
        .iter()
        .flat_map(|t| t.triangles.iter().map(|tri| tri.get_point(0)))
        .collect();

    assert_eq!(before.len(), after.len());

    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(*a, *b + delta);
    }
}

#[test]
fn compound_near_miss_not_pruned() {
    let poly = make_blob();

    // Two segments forming a compound, sitting a known distance outside the outline.
    let gap = 20000;
    let a = v(1000000 + gap, -100000);
    let b = v(1000000 + gap, 100000);

    let mut compound = ShapeCompound::new();
    compound.add_shape(Box::new(ShapeSegment::new(a, b, 0)));
    compound.add_shape(Box::new(ShapeSegment::new(b, b + v(50000, 0), 0)));

    // Comfortably clear.
    assert_eq!(probe(&poly, &compound, gap / 2), reference_scan(&poly, &compound, gap / 2));

    // Clearance large enough that the near-miss triangles matter: a pruned candidate set
    // would report no collision here while the full scan reports one.
    assert_eq!(probe(&poly, &compound, gap * 4), reference_scan(&poly, &compound, gap * 4));

    assert!(probe(&poly, &compound, gap * 4).collides);
}

#[test]
fn min_distance_matches_full_scan() {
    let poly = make_blob();

    let cases = [
        ((1000000, 0), 0),     // on the outline, between two triangles
        ((1010000, 0), 10000), // exactly at clearance distance
        ((1010000, 0), 9999),  // one IU inside
        ((1010000, 0), 10001), // one IU outside
        ((-400000, 0), 100000), // inside a hole, reaching its rim
        ((0, 999000), 5000),   // near the outline on another axis
    ];

    for ((x, y), clearance) in cases {
        let probe_rect = ShapeRect::from_pos_size(v(x, y), 1, 1);
        assert_eq!(
            probe(&poly, &probe_rect, clearance),
            reference_scan(&poly, &probe_rect, clearance)
        );
    }
}

#[test]
fn sub_threshold_polygon_matches_full_scan() {
    let mut poly = ShapePolySet::new();
    poly.add_outline(&make_ngon(v(0, 0), 1000000, 8)); // 8 vertices -> ~6 triangles
    poly.cache_triangulation(false);

    let triangles: usize = poly.triangulated_polys().iter().map(|t| t.triangle_count()).sum();

    // If this ever exceeds the threshold (RTREE_THRESHOLD, 32) the case stops testing what it
    // claims to.
    assert!(triangles < 32);

    for (x, y) in [(0, 0), (900000, 0), (1100000, 0), (0, 900000)] {
        let probe_rect = ShapeRect::from_pos_size(v(x, y), 1000, 1000);
        assert_eq!(probe(&poly, &probe_rect, 0), reference_scan(&poly, &probe_rect, 0));
        assert_eq!(probe_bool(&poly, &probe_rect, 0), reference_scan_bool(&poly, &probe_rect, 0));
    }
}

#[test]
fn check_index_dropped() {
    // Move: the index is dropped, and the rebuilt one must describe the new positions.
    let mut poly = make_blob();
    warm_and_require_indexed(&mut poly);

    let delta = v(5000000, 0);
    poly.move_(delta);
    assert!(poly.triangulation_index().is_none());

    for (x, y) in G_PROBES {
        let pt = v(x, y);
        let shifted = ShapeRect::from_pos_size(pt + delta, 1000, 1000);
        assert_eq!(probe(&poly, &shifted, 0), reference_scan(&poly, &shifted, 0));
        assert_eq!(probe_bool(&poly, &shifted, 0), reference_scan_bool(&poly, &shifted, 0));

        // The old location must now be empty.
        let stale = ShapeRect::from_pos_size(pt, 1000, 1000);
        assert_eq!(probe(&poly, &stale, 0), reference_scan(&poly, &stale, 0));
    }

    // RemoveAllContours discards the triangles outright.
    poly = make_blob();
    warm_and_require_indexed(&mut poly);
    poly.remove_all_contours();
    assert!(poly.triangulation_index().is_none());

    // Re-triangulating replaces every polygon, so the index must not survive it either.
    poly = make_blob();
    warm_and_require_indexed(&mut poly);
    poly.add_hole(&make_ngon(v(0, -500000), 100000, 60), -1);
    poly.cache_triangulation(false);
    assert!(poly.triangulation_index().is_none());

    for (x, y) in G_PROBES {
        let probe_rect = ShapeRect::from_pos_size(v(x, y), 1000, 1000);
        assert_eq!(probe(&poly, &probe_rect, 0), reference_scan(&poly, &probe_rect, 0));
        assert_eq!(probe_bool(&poly, &probe_rect, 0), reference_scan_bool(&poly, &probe_rect, 0));
    }

    // The new hole must read as empty.
    let in_new_hole = ShapeRect::from_pos_size(v(0, -500000), 1000, 1000);
    assert_eq!(probe(&poly, &in_new_hole, 0), reference_scan(&poly, &in_new_hole, 0));

    // Copy assignment, against a blob far from the first.
    let mut target = make_blob();
    warm_and_require_indexed(&mut target);

    let mut other = ShapePolySet::new();
    other.add_outline(&make_ngon(v(8000000, 8000000), 900000, 180));
    other.cache_triangulation(false);

    target = other.clone();

    assert!(target.triangulated_poly_count() > 0);
    assert!(target.triangulation_index().is_none());

    for (x, y) in [(8000000, 8000000), (8890000, 8000000), (8910000, 8000000), (0, 0)] {
        let probe_rect = ShapeRect::from_pos_size(v(x, y), 1000, 1000);
        assert_eq!(probe(&target, &probe_rect, 0), reference_scan(&target, &probe_rect, 0));
        assert_eq!(
            probe_bool(&target, &probe_rect, 0),
            reference_scan_bool(&target, &probe_rect, 0)
        );
    }
}
