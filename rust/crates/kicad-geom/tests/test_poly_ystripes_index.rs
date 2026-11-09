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

//! Port of `qa/tests/libs/kimath/geometry/test_poly_ystripes_index.cpp` (suite
//! `PolyYStripesIndex`, 3 of 3).

use kicad_geom::geometry::poly_ystripes_index::PolyYStripesIndex;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{Mt19937, uniform_int_libstdcxx};

fn rect_chain(x0: i32, y0: i32, x1: i32, y1: i32) -> ShapeLineChain {
    let mut outline = ShapeLineChain::new();
    outline.append_xy(x0, y0, false);
    outline.append_xy(x1, y0, false);
    outline.append_xy(x1, y1, false);
    outline.append_xy(x0, y1, false);
    outline.set_closed(true);
    outline
}

/// `makeSquare()`
fn make_square(a_size: i32) -> ShapePolySet {
    let mut poly = ShapePolySet::new();
    poly.add_outline(&rect_chain(0, 0, a_size, a_size));
    poly
}

/// `generateRandomPoints()`.  GCC evaluates the `emplace_back( distX( gen ), distY( gen ) )`
/// arguments right to left, so y is drawn first.
fn generate_random_points(a_bbox: &Box2I, a_count: i32, a_seed: u32) -> Vec<Vector2I> {
    let mut gen_ = Mt19937::new(a_seed);
    let mut points = Vec::with_capacity(a_count as usize);

    for _ in 0..a_count {
        let y = uniform_int_libstdcxx(&mut gen_, a_bbox.top() as i64, a_bbox.bottom() as i64) as i32;
        let x = uniform_int_libstdcxx(&mut gen_, a_bbox.left() as i64, a_bbox.right() as i64) as i32;
        points.push(Vector2I::new(x, y));
    }

    points
}

fn contains(poly: &ShapePolySet, pt: Vector2I) -> bool {
    poly.contains(pt, -1, 0, false)
}

#[test]
fn correctness_all_strategies_agree() {
    // An axis-aligned square is the one subject whose containment is decidable without
    // consulting any implementation under test, so the sweep can assert truth rather than
    // consensus.
    const SIZE: i32 = 1000000;
    const MARGIN: i32 = 1000;

    let square = make_square(SIZE);

    let mut ystripes = PolyYStripesIndex::new();
    ystripes.build(&square);

    let sample_area = Box2I::new(Vector2I::new(-SIZE / 2, -SIZE / 2), Vector2I::new(2 * SIZE, 2 * SIZE));

    let mut tested = 0;
    let mut inside = 0;

    let near_edge = |v: i32, lo: i32, hi: i32| (v - lo).abs() <= MARGIN || (v - hi).abs() <= MARGIN;

    for pt in generate_random_points(&sample_area, 10000, 42) {
        // Points hugging an edge are genuinely ambiguous; they are not what this case is about
        if near_edge(pt.x, 0, SIZE) || near_edge(pt.y, 0, SIZE) {
            continue;
        }

        let expected = pt.x > 0 && pt.x < SIZE && pt.y > 0 && pt.y < SIZE;

        tested += 1;
        inside += expected as i32;

        assert_eq!(contains(&square, pt), expected, "point {pt:?}");
        assert_eq!(ystripes.contains(pt, 0), expected, "point {pt:?}");
    }

    // A sweep that sampled only one side of the boundary would prove nothing
    assert!(tested > 5000);
    assert!(inside > 0);
    assert!(inside < tested);
}

#[test]
fn correctness_edge_cases() {
    let square = make_square(1000000);

    let mut ystripes = PolyYStripesIndex::new();
    ystripes.build(&square);

    let cases = [
        ((500000, 500000), true, "center"),
        ((100000, 100000), true, "inside near corner"),
        ((900000, 900000), true, "inside far corner"),
        ((-100000, 500000), false, "outside left"),
        ((500000, -100000), false, "outside above"),
        ((1500000, 500000), false, "outside right"),
        ((500000, 1500000), false, "outside below"),
        ((500000, -1000000), false, "outside Y range above"),
        ((500000, 3000000), false, "outside Y range below"),
    ];

    for ((x, y), expected, desc) in cases {
        let pt = Vector2I::new(x, y);
        assert_eq!(contains(&square, pt), expected, "{desc}");
        assert_eq!(ystripes.contains(pt, 0), expected, "{desc}");
    }
}

#[test]
fn correctness_polygon_with_holes() {
    let mut poly = ShapePolySet::new();
    poly.add_outline(&rect_chain(0, 0, 1000, 1000));
    poly.add_hole(&rect_chain(400, 400, 600, 600), -1);

    let mut ystripes = PolyYStripesIndex::new();
    ystripes.build(&poly);

    let cases = [((100, 100), true), ((500, 500), false), ((1500, 500), false), ((800, 200), true)];

    for ((x, y), expected) in cases {
        let pt = Vector2I::new(x, y);
        assert_eq!(ystripes.contains(pt, 0), expected, "point {pt:?}");
        assert_eq!(contains(&poly, pt), expected, "point {pt:?}");
    }

    // The proximity fallback must skip hole edges.  Probe a vertical one; Build() drops
    // horizontal edges, so a point near the hole's top would never reach that branch
    assert!(!ystripes.contains(Vector2I::new(410, 500), 20));

    // Same distance from a real outline edge, to prove the probe above is not passing
    // simply because the fallback found nothing
    assert!(ystripes.contains(Vector2I::new(-10, 500), 20));

    let bbox = poly.bbox(0);
    let mut mismatches = 0;

    for pt in generate_random_points(&bbox, 10000, 42) {
        if ystripes.contains(pt, 0) != contains(&poly, pt) {
            mismatches += 1;
        }
    }

    assert_eq!(mismatches, 0);
}
