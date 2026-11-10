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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_nearest_points.cpp`
//! (suite `SHAPE_NEAREST_POINTS_TEST`): `SHAPE::NearestPoints()` and the
//! `SHAPE_ARC::NearestPoints()` overloads.

use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::vector2::Vector2I;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

/// `a.NearestPoints( &b, ptA, ptB )` with default-constructed points.
fn np(a: &dyn Shape, b: &dyn Shape) -> (bool, Vector2I, Vector2I) {
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let r = a.nearest_points(b, &mut pt_a, &mut pt_b);
    (r, pt_a, pt_b)
}

fn chain(pts: &[(i32, i32)]) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();

    for &(x, y) in pts {
        c.append_point(v(x, y), false);
    }

    c
}

fn arc3(a: (i32, i32), m: (i32, i32), e: (i32, i32)) -> ShapeArc {
    ShapeArc::from_start_mid_end(v(a.0, a.1), v(m.0, m.1), v(e.0, e.1), 0)
}

// Circle to Circle tests

#[test]
fn nearest_points_circle_to_circle_separate() {
    let (r, a, b) = np(
        &ShapeCircle::new(v(0, 0), 5),
        &ShapeCircle::new(v(20, 0), 5),
    );
    assert!(r);
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(15, 0));
}

#[test]
fn nearest_points_circle_to_circle_concentric() {
    let (r, a, b) = np(
        &ShapeCircle::new(v(0, 0), 5),
        &ShapeCircle::new(v(0, 0), 10),
    );
    assert!(r);
    // For concentric circles, points should be on arbitrary direction (positive X by
    // convention)
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(10, 0));
}

#[test]
fn nearest_points_circle_to_circle_overlapping() {
    let (r, a, b) = np(&ShapeCircle::new(v(0, 0), 5), &ShapeCircle::new(v(6, 0), 5));
    assert!(r);
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(1, 0));
}

// Circle to Rectangle tests

#[test]
fn nearest_points_circle_to_rect_outside() {
    let rect = ShapeRect::from_corners(v(0, 0), v(10, 10));
    let (r, a, b) = np(&ShapeCircle::new(v(-10, 5), 3), &rect);
    assert!(r);
    assert_eq!(a, v(-7, 5));
    assert_eq!(b, v(0, 5));
}

#[test]
fn nearest_points_circle_to_rect_inside() {
    let rect = ShapeRect::from_corners(v(0, 0), v(10, 10));
    let (r, a, b) = np(&ShapeCircle::new(v(5, 5), 2), &rect);
    assert!(r);
    // Circle center is inside rectangle, should find nearest edge (left edge by the min
    // comparison)
    assert_eq!(a, v(3, 5));
    assert_eq!(b, v(0, 5));
}

// Circle to Segment tests

#[test]
fn nearest_points_circle_to_segment() {
    let segment = ShapeSegment::new(v(-5, 0), v(5, 0), 0);
    let (r, a, b) = np(&ShapeCircle::new(v(0, 5), 2), &segment);
    assert!(r);
    assert_eq!(a, v(0, 3));
    assert_eq!(b, v(0, 0));
}

#[test]
fn nearest_points_circle_to_segment_center_on_segment() {
    let segment = ShapeSegment::new(v(-5, 0), v(5, 0), 0);
    let (r, a, b) = np(&ShapeCircle::new(v(0, 0), 3), &segment);
    assert!(r);
    // When center is on segment, should pick perpendicular direction
    assert_eq!(a, v(0, 3));
    assert_eq!(b, v(0, 0));
}

// Rectangle to Rectangle tests

#[test]
fn nearest_points_rect_to_rect_separate() {
    let rect_a = ShapeRect::from_corners(v(0, 0), v(5, 5));
    let rect_b = ShapeRect::from_corners(v(10, 0), v(25, 25));
    let (r, a, b) = np(&rect_a, &rect_b);
    assert!(r);
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(10, 0));
}

#[test]
fn nearest_points_rect_to_rect_corner() {
    let rect_a = ShapeRect::from_corners(v(0, 0), v(5, 5));
    let rect_b = ShapeRect::from_corners(v(7, 7), v(25, 25));
    let (r, a, b) = np(&rect_a, &rect_b);
    assert!(r);
    assert_eq!(a, v(5, 5));
    assert_eq!(b, v(7, 7));
}

// Line Chain tests

#[test]
fn nearest_points_line_chain_to_line_chain() {
    let chain_a = chain(&[(0, 0), (10, 0), (10, 10)]);
    let chain_b = chain(&[(5, 5), (15, 5)]);
    let (r, a, b) = np(&chain_a, &chain_b);
    assert!(r);
    assert_eq!(a, v(10, 5));
    assert_eq!(b, v(10, 5));
}

#[test]
fn nearest_points_line_chain_with_arc() {
    let chain_a = chain(&[(0, 0), (10, 0)]);
    let mut chain_b = ShapeLineChain::new();
    chain_b.append_arc(&arc3((5, 10), (10, 5), (15, 10)));
    let (r, a, b) = np(&chain_a, &chain_b);
    assert!(r);
    assert_eq!(a, v(10, 0));
    assert_eq!(b, v(10, 10));
}

// Arc tests

#[test]
fn nearest_points_arc_to_arc() {
    let arc_a = arc3((0, 0), (5, 5), (10, 0));
    let arc_b = arc3((15, 0), (20, 5), (25, 0));
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq = 0i64;
    assert!(arc_a.nearest_points_arc(&arc_b, &mut pt_a, &mut pt_b, &mut dist_sq));
    // Points should be on the arcs closest to each other
    assert!(pt_a.x <= 10 && pt_a.x >= 0);
    assert!(pt_b.x >= 15 && pt_b.x <= 25);
}

#[test]
fn nearest_points_arc_to_circle() {
    let arc = arc3((0, 0), (5, 5), (10, 0));
    let circle = ShapeCircle::new(v(5, 10), 3);
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq = 0i64;
    assert!(arc.nearest_points_circle(&circle, &mut pt_a, &mut pt_b, &mut dist_sq));
    // Arc point should be at or near the top of the arc
    assert!(pt_a.y >= 0);
    // Circle point should be at bottom of circle
    assert_eq!(pt_b, v(5, 7));
}

// Segment tests

#[test]
fn nearest_points_segment_to_segment() {
    let seg_a = ShapeSegment::from_seg(Seg::new(v(0, 0), v(10, 0)), 2);
    let seg_b = ShapeSegment::from_seg(Seg::new(v(5, 5), v(5, 15)), 2);
    let (r, a, b) = np(&seg_a, &seg_b);
    assert!(r);
    // Points should account for segment width
    assert_eq!(a, v(5, 1));
    assert_eq!(b, v(5, 4));
}

#[test]
fn nearest_points_segment_to_circle() {
    let segment = ShapeSegment::from_seg(Seg::new(v(0, 0), v(10, 0)), 4);
    let (r, a, b) = np(&segment, &ShapeCircle::new(v(5, 10), 3));
    assert!(r);
    assert_eq!(a, v(5, 2));
    assert_eq!(b, v(5, 7));
}

// Compound Shape tests

#[test]
fn nearest_points_compound_shapes() {
    let mut compound_a = ShapeCompound::new();
    compound_a.add_shape(Box::new(ShapeCircle::new(v(0, 0), 5)));
    compound_a.add_shape(Box::new(ShapeRect::from_corners(v(10, 0), v(5, 5))));

    let mut compound_b = ShapeCompound::new();
    compound_b.add_shape(Box::new(ShapeCircle::new(v(20, 0), 3)));

    let (r, a, b) = np(&compound_a, &compound_b);
    assert!(r);
    // Should find nearest points between rectangle in A and circle in B
    assert_eq!(a, v(10, 0));
    assert_eq!(b, v(17, 0));
}

// Polygon Set tests

#[test]
fn nearest_points_poly_set_to_circle() {
    let mut poly_set = ShapePolySet::new();
    poly_set.new_outline();
    poly_set.append(0, 0, -1, -1, false);
    poly_set.append(10, 0, -1, -1, false);
    poly_set.append(10, 10, -1, -1, false);
    poly_set.append(0, 10, -1, -1, false);

    let (r, _a, b) = np(&poly_set, &ShapeCircle::new(v(15, 5), 3));
    assert!(r);
    assert_eq!(b, v(12, 5));
}

// Missing Basic Shape Combinations

#[test]
fn nearest_points_rect_to_segment() {
    let rect = ShapeRect::from_corners(v(0, 0), v(10, 10));
    let segment = ShapeSegment::new(v(15, 5), v(25, 5), 0);
    let (r, a, b) = np(&rect, &segment);
    assert!(r);
    assert_eq!(a, v(10, 5));
    assert_eq!(b, v(15, 5));
}

#[test]
fn nearest_points_rect_to_line_chain() {
    let rect = ShapeRect::from_corners(v(0, 0), v(5, 5));
    let c = chain(&[(10, 0), (15, 5), (10, 10)]);
    let (r, a, b) = np(&rect, &c);
    assert!(r);
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(10, 0));
}

#[test]
fn nearest_points_segment_to_line_chain() {
    let segment = ShapeSegment::from_seg(Seg::new(v(0, 0), v(100, 0)), 20);
    let c = chain(&[(50, 100), (150, 100), (150, 50)]);
    let (r, a, b) = np(&segment, &c);
    assert!(r);
    assert_eq!(a, v(107, 7));
    assert_eq!(b, v(150, 50));
}

#[test]
fn nearest_points_circle_to_line_chain() {
    let c = chain(&[(10, -5), (10, 0), (10, 5)]);
    let (r, a, b) = np(&ShapeCircle::new(v(0, 0), 3), &c);
    assert!(r);
    assert_eq!(a, v(3, 0));
    assert_eq!(b, v(10, 0));
}

#[test]
fn nearest_points_arc_to_rect() {
    let arc = arc3((0, 0), (5, 5), (10, 0));
    let rect = ShapeRect::from_corners(v(150, 2), v(50, 6));
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq = 0i64;
    assert!(arc.nearest_points_rect(&rect, &mut pt_a, &mut pt_b, &mut dist_sq));
    assert_eq!(pt_a, v(10, 0));
    assert_eq!(pt_b, v(50, 2));
}

#[test]
fn nearest_points_arc_to_segment() {
    // Semicircle
    let arc = arc3((0, 0), (0, 10), (0, 20));
    let segment = Seg::new(v(15, 10), v(25, 10));
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq = 0i64;
    assert!(arc.nearest_points_seg(&segment, &mut pt_a, &mut pt_b, &mut dist_sq));
    // Arc point should be on the rightmost part of the arc
    assert!(pt_a.x >= 0);
    assert_eq!(pt_b, v(15, 10));
}

#[test]
fn nearest_points_arc_to_line_chain() {
    let arc = arc3((0, 0), (5, 5), (10, 0));
    let c = chain(&[(5, 15), (5, 10), (15, 10)]);
    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let result = c.nearest_points(&arc, &mut pt_b, &mut pt_a);
    assert!(result);
    // Arc point should be near the top of the arc
    assert_eq!(pt_a, v(5, 5));
    assert_eq!(pt_b, v(5, 10));
}

// Distance and Symmetry Validation Tests

#[test]
fn nearest_points_distance_validation() {
    let (r, a, b) = np(
        &ShapeCircle::new(v(0, 0), 5),
        &ShapeCircle::new(v(20, 0), 3),
    );
    assert!(r);
    // Calculate expected distance: center distance - both radii
    let expected_distance = 20 - 5 - 3; // 12
    assert_eq!((b - a).euclidean_norm(), expected_distance);
}

#[test]
fn nearest_points_symmetry_test() {
    let rect_a = ShapeRect::from_corners(v(0, 0), v(5, 5));
    let circle_b = ShapeCircle::new(v(10, 2), 2);

    // Test A->B
    let (result1, pt_a1, pt_b1) = np(&rect_a, &circle_b);
    // Test B->A
    let (result2, pt_a2, pt_b2) = np(&circle_b, &rect_a);

    assert!(result1 && result2);

    // Distance should be the same both ways
    assert_eq!(
        (pt_b1 - pt_a1).euclidean_norm(),
        (pt_b2 - pt_a2).euclidean_norm()
    );

    // Points should be swapped (ptA1 should equal ptB2, ptB1 should equal ptA2)
    assert_eq!(pt_a1, pt_b2);
    assert_eq!(pt_b1, pt_a2);
}

#[test]
fn nearest_points_minimum_distance_validation() {
    let segment = ShapeSegment::from_seg(Seg::new(v(0, 0), v(10, 0)), 0);
    let circle = ShapeCircle::new(v(5, 5), 2);
    let (r, a, b) = np(&segment, &circle);
    assert!(r);

    // Verify that this is indeed the minimum distance by checking nearby points
    let min_dist = (b - a).euclidean_norm();

    // Check a few other points on the segment
    for x in (0..=10).step_by(2) {
        let test_pt = v(x, 0);
        let circle_closest =
            circle.get_center() + (test_pt - circle.get_center()).resize(circle.get_radius());
        let test_dist = (circle_closest - test_pt).euclidean_norm();
        assert!(min_dist <= test_dist, "Found shorter distance at x={x}");
    }
}

// Complex Line Chain Tests

#[test]
fn nearest_points_line_chain_with_mixed_arcs() {
    let mut chain_a = chain(&[(0, 0), (10, 0)]);
    chain_a.append_arc(&arc3((10, 0), (15, 5), (20, 0)));
    chain_a.append_point(v(30, 0), false);

    let (r, _a, b) = np(&chain_a, &ShapeCircle::new(v(15, 15), 3));
    assert!(r);
    assert_eq!(b, v(15, 12));
}

// Degenerate Shape Tests

#[test]
fn nearest_points_degenerate_shapes_zero_width_rect() {
    // Zero width
    let rect_a = ShapeRect::from_corners(v(0, 0), v(0, 10));
    let (r, a, b) = np(&rect_a, &ShapeCircle::new(v(5, 5), 2));
    assert!(r);
    assert_eq!(a, v(0, 5));
    assert_eq!(b, v(3, 5));
}

#[test]
fn nearest_points_degenerate_shapes_zero_length_segment() {
    // Point segment
    let zero_seg = ShapeSegment::new(v(5, 5), v(5, 5), 0);
    let (r, a, b) = np(&ShapeCircle::new(v(10, 5), 3), &zero_seg);
    assert!(r);
    assert_eq!(a, v(7, 5));
    assert_eq!(b, v(5, 5));
}

#[test]
fn nearest_points_single_point_line_chain() {
    let single_point = chain(&[(0, 0)]);
    let (r, _, _) = np(&single_point, &ShapeCircle::new(v(5, 0), 2));
    assert!(!r);
}

// Negative Coordinate Tests

#[test]
fn nearest_points_negative_coordinates() {
    let rect = ShapeRect::from_corners(v(0, -2), v(5, 4));
    let (r, a, b) = np(&ShapeCircle::new(v(-10, -5), 3), &rect);
    assert!(r);
    assert_eq!(a, v(-7, -4));
    assert_eq!(b, v(0, -2));
}

// Edge case tests

#[test]
fn nearest_points_identical_shapes() {
    let (r, a, b) = np(&ShapeCircle::new(v(5, 5), 3), &ShapeCircle::new(v(5, 5), 3));
    assert!(r);
    // Identical concentric circles - should return arbitrary points
    assert_eq!(a, v(8, 5));
    assert_eq!(b, v(8, 5));
}

#[test]
fn nearest_points_zero_size_shapes() {
    let (r, a, b) = np(
        &ShapeCircle::new(v(0, 0), 0),
        &ShapeCircle::new(v(10, 0), 5),
    );
    assert!(r);
    assert_eq!(a, v(0, 0));
    assert_eq!(b, v(5, 0));
}

#[test]
fn nearest_points_very_close_shapes() {
    // Just touching
    let (r, a, b) = np(
        &ShapeCircle::new(v(0, 0), 5),
        &ShapeCircle::new(v(10, 0), 5),
    );
    assert!(r);
    assert_eq!(a, v(5, 0));
    assert_eq!(b, v(5, 0));
}
