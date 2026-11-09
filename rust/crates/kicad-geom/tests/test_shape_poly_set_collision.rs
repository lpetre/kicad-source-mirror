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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_collision.cpp` (suite
//! `SPSCollision`, fixture `CollisionFixture`).  None of its cases needs the triangulation.

mod common_poly;

use common_poly::CommonPolyData;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

/// Port of `CollisionFixture`.
struct CollisionFixture {
    common: CommonPolyData,
    colliding_points: Vec<Vector2I>,
    non_colliding_points: Vec<Vector2I>,
    /// segment under test, collision result, and intersection point
    segs: Vec<(Seg, bool, Vector2I)>,
}

impl CollisionFixture {
    fn new() -> Self {
        CollisionFixture {
            common: CommonPolyData::new(),
            colliding_points: vec![
                // Inside the polygon
                v(10, 90),
                // Inside the polygon, but on a re-entrant angle of a hole
                v(15, 16),
                // On a hole edge => inside the polygon
                v(40, 25),
                // On the outline edge => inside the polygon
                v(0, 10),
            ],
            non_colliding_points: vec![
                // Completely outside of the polygon
                v(200, 200),
                // Inside the outline and inside a hole => outside the polygon
                v(15, 12),
            ],
            segs: vec![
                // Seg crossing the edge
                (Seg::new(v(90, 90), v(110, 110)), true, v(100, 100)),
                (Seg::new(v(110, 110), v(90, 90)), true, v(100, 100)),
                (Seg::new(v(50, -10), v(50, 50)), true, v(50, 0)),
                (Seg::new(v(50, 50), v(50, -10)), true, v(50, 0)),
                // Seg fully inside
                (Seg::new(v(80, 80), v(90, 90)), true, v(85, 85)),
                (Seg::new(v(90, 90), v(80, 80)), true, v(85, 85)),
                // Seg fully outside
                (Seg::new(v(110, 110), v(120, 120)), false, v(0, 0)),
                // Seg touching
                (Seg::new(v(100, 100), v(110, 110)), true, v(100, 100)),
                (Seg::new(v(110, 110), v(100, 100)), true, v(100, 100)),
            ],
        }
    }
}

/// Simple dummy test to check that HasHoles() definition is right
#[test]
fn has_holes() {
    let f = CollisionFixture::new();
    assert!(!f.common.solid_poly_set.has_holes());
    assert!(f.common.holey_poly_set.has_holes());
}

/// This test checks basic behaviour of PointOnEdge, testing if points on corners, outline edges
/// and hole edges are detected as colliding.
#[test]
fn point_on_edge() {
    let f = CollisionFixture::new();
    let s = &f.common.holey_poly_set;

    // Check points on corners
    assert!(s.point_on_edge(v(0, 50), 0));
    // Check points on outline edges
    assert!(s.point_on_edge(v(0, 10), 0));
    // Check points on hole edges
    assert!(s.point_on_edge(v(10, 11), 0));
    // Check points inside a hole -> not in edge
    assert!(!s.point_on_edge(v(12, 12), 0));
    // Check points inside the polygon and outside any hole -> not on edge
    assert!(!s.point_on_edge(v(90, 90), 0));
    // Check points outside the polygon -> not on edge
    assert!(!s.point_on_edge(v(200, 200), 0));
}

/// This test checks that the function Contains, whose behaviour has been updated to also manage
/// holey polygons, does the right work.
#[test]
fn point_in_polygon_set() {
    let f = CollisionFixture::new();

    // Check that the set contains the points that collide with it
    for point in &f.colliding_points {
        assert!(
            f.common.holey_poly_set.contains(*point, -1, 0, false),
            "Point {point:?}"
        );
    }

    // Check that the set does not contain any point outside of it
    for point in &f.non_colliding_points {
        assert!(
            !f.common.holey_poly_set.contains(*point, -1, 0, false),
            "Point {point:?}"
        );
    }
}

/// This test checks the behaviour of the Collide (with a point) method.
#[test]
fn collide() {
    let f = CollisionFixture::new();
    let s = &f.common.holey_poly_set;

    // When clearance = 0, the behaviour should be the same as with Contains
    // Check that the set collides with the colliding points
    for point in &f.colliding_points {
        assert!(s.collide_point(*point, 0, None, None), "Point {point:?}");
    }

    // Check that the set does not collide with the non colliding points
    for point in &f.non_colliding_points {
        assert!(!s.collide_point(*point, 0, None, None), "Point {point:?}");
    }

    // Checks with clearance > 0

    // Point at the offset zone outside of the outline => collision!
    assert!(s.collide_point(v(-1, 10), 5, None, None));

    // Point at the offset zone outside of a hole => collision!
    assert!(s.collide_point(v(11, 11), 5, None, None));
}

/// This test checks the behaviour of the CollideVertex method, testing whether the collision with
/// vertices is well detected
#[test]
fn collide_vertex() {
    let f = CollisionFixture::new();

    // Check that the set collides with the colliding points
    for point in &f.common.data.holey_points {
        assert!(
            f.common.holey_poly_set.collide_vertex(*point, None, 0),
            " Point {}, {} does not collide with holeyPolySet polygon",
            point.x,
            point.y
        );
    }
}

/// This test checks the behaviour of the CollideVertex method, testing whether the collision with
/// vertices is well detected
#[test]
fn collide_vertex_with_clearance() {
    let f = CollisionFixture::new();

    // Check that the set collides with the colliding points
    for point in &f.common.data.holey_points {
        assert!(
            f.common
                .holey_poly_set
                .collide_vertex(*point + v(1, 1), None, 2)
        );
    }
}

/// Check that SHAPE_POLY_SET::Collide does the right thing for segments
#[test]
fn collide_segments() {
    let f = CollisionFixture::new();

    for (seg, expected_result, expected_location) in &f.segs {
        let mut location = Vector2I::default();

        assert!(
            f.common
                .holey_poly_set
                .collide_seg(seg, 0, None, Some(&mut location))
                == *expected_result
        );

        if *expected_result {
            assert_eq!(location, *expected_location);
        }
    }
}

/// regression for keepout collision location reported at the origin
#[test]
fn collide_segment_location_near_miss() {
    let mut square = ShapePolySet::new();
    let mut outline = ShapeLineChain::new();
    outline.append_xy(1000, 1000, false);
    outline.append_xy(2000, 1000, false);
    outline.append_xy(2000, 2000, false);
    outline.append_xy(1000, 2000, false);
    outline.set_closed(true);
    square.add_outline(&outline);

    let clearance = 100;

    // first segment sits outside the first iterated edge
    let near_misses = [
        Seg::new(v(1400, 950), v(1600, 950)),
        Seg::new(v(1400, 2050), v(1600, 2050)),
        Seg::new(v(950, 1400), v(950, 1600)),
        Seg::new(v(2050, 1400), v(2050, 1600)),
    ];

    for seg in &near_misses {
        let mut actual = -1;
        let mut location = v(-1, -1);

        assert!(square.collide_seg(seg, clearance, Some(&mut actual), Some(&mut location)));
        assert!(location != v(0, 0));
        assert!(square.point_on_edge(location, 0));
        assert_eq!(actual, 50);
    }
}
