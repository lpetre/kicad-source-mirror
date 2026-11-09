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

//! Port of `qa/tests/libs/kimath/math/test_box2.cpp` (Boost test suite `BOX2TESTS`).

use kicad_geom::math::box2::{Box2D, Box2I};
use kicad_geom::math::vector2::{Vector2D, Vector2I, Vector2L};
use kicad_geom::test_utils::is_vec_within_tol;

/// Port of `BOOST_AUTO_TEST_CASE( DefaultConstructor )`.
#[test]
fn default_constructor() {
    let b = Box2I::default();
    assert!(b.position() == Vector2I::new(0, 0));
    assert!(b.size() == Vector2L::new(0, 0));
}

/// Port of `BOOST_AUTO_TEST_CASE( BasicInt )`.
#[test]
fn basic_int() {
    let b = Box2I::new(Vector2I::new(1, 2), Vector2I::new(3, 4));

    assert!(b.position() == Vector2I::new(1, 2));
    assert!(b.size() == Vector2L::new(3, 4));

    // Check the equality operator
    assert!(b == Box2I::new(Vector2I::new(1, 2), Vector2I::new(3, 4)));

    // Inflate in-place
    let mut inflated: Box2I = b;
    inflated.inflate(1);
    assert!(inflated.position() == Vector2I::new(0, 1));
    assert!(inflated.size() == Vector2L::new(5, 6));

    // GetInflated
    let inflated2: Box2I = b.inflated(1);
    assert!(inflated2 == inflated);
}

/// Port of `BOOST_AUTO_TEST_CASE( Constexpr )`.
///
/// The C++ test checks this with `static_assert`; the port checks at run time.
#[test]
fn constexpr() {
    let box_1_2_3_4 = Box2I::new(Vector2I::new(1, 2), Vector2I::new(3, 4));
    assert!(box_1_2_3_4.position() == Vector2I::new(1, 2));
    assert!(box_1_2_3_4.size() == Vector2L::new(3, 4));

    let box0_1_5_6: Box2I = box_1_2_3_4.inflated(1);
    assert!(box0_1_5_6.position() == Vector2I::new(0, 1));
    assert!(box0_1_5_6.size() == Vector2L::new(5, 6));

    assert!(box_1_2_3_4.squared_diagonal() < box0_1_5_6.squared_diagonal());

    let box1_2_100_4: Box2I = *box_1_2_3_4.with_offset(Vector2I::new(100, 0)).merge(&box_1_2_3_4);
    assert!(box1_2_100_4.position() == Vector2I::new(1, 2));
    assert!(box1_2_100_4.size() == Vector2L::new(103, 4));
}

/// Port of `BOOST_AUTO_TEST_CASE( BasicDouble )`.
#[test]
fn basic_double() {
    let tol: f64 = 0.000001;
    let b = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(3.0, 4.0));

    // Inflate by non-integer amount
    let inflated: Box2D = *Box2D::clone(&b).inflate(1.5);

    // The C++ predicate is instantiated as KI_TEST::IsVecWithinTol<VECTOR2I>, so the VECTOR2D
    // arguments and the tolerance are converted to VECTOR2I / int before the comparison.
    assert!(is_vec_within_tol::<i32>(
        inflated.position().cast(),
        Vector2D::new(-0.5, 0.5).cast(),
        tol as i32
    ));
    assert!(is_vec_within_tol::<i32>(inflated.size().cast(), Vector2D::new(6.0, 7.0).cast(), tol as i32));

    // (Not in the C++ test) the exact values the weakened predicate above cannot see
    assert_eq!(inflated.position(), Vector2D::new(-0.5, 0.5));
    assert_eq!(inflated.size(), Vector2D::new(6.0, 7.0));
}

/// Port of `BOOST_AUTO_TEST_CASE( ByCorners )`.
#[test]
fn by_corners() {
    let box_by_corners = Box2I::by_corners(Vector2I::new(1, 2), Vector2I::new(3, 4));
    let box_by_pos_size = Box2I::new(Vector2I::new(1, 2), Vector2I::new(2, 2));

    assert!(box_by_corners == box_by_pos_size);
}

/// Port of `BOOST_AUTO_TEST_CASE( ByCentre )`.
#[test]
fn by_centre() {
    let box_by_center = Box2I::by_center(Vector2I::new(100, 100), Vector2I::new(20, 20));
    let box_by_pos_size = Box2I::new(Vector2I::new(90, 90), Vector2I::new(20, 20));

    assert!(box_by_center == box_by_pos_size);
}

/// Port of `BOOST_AUTO_TEST_CASE( test_closest_point_to )`.
#[test]
fn test_closest_point_to() {
    let b = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(3.0, 4.0));

    // check all quadrants

    // top left
    assert!(b.nearest_point(Vector2D::new(0.0, 0.0)) == Vector2D::new(1.0, 2.0));

    // top
    assert!(b.nearest_point(Vector2D::new(2.0, 0.0)) == Vector2D::new(2.0, 2.0));

    // top right
    assert!(b.nearest_point(Vector2D::new(6.0, 0.0)) == Vector2D::new(4.0, 2.0));

    // right
    assert!(b.nearest_point(Vector2D::new(6.0, 5.0)) == Vector2D::new(4.0, 5.0));

    // bottom right
    assert!(b.nearest_point(Vector2D::new(6.0, 7.0)) == Vector2D::new(4.0, 6.0));

    // bottom
    assert!(b.nearest_point(Vector2D::new(3.0, 7.0)) == Vector2D::new(3.0, 6.0));

    // bottom left
    assert!(b.nearest_point(Vector2D::new(0.0, 7.0)) == Vector2D::new(1.0, 6.0));

    // left
    assert!(b.nearest_point(Vector2D::new(0.0, 3.0)) == Vector2D::new(1.0, 3.0));

    // inside
    assert!(b.nearest_point(Vector2D::new(2.0, 4.0)) == Vector2D::new(2.0, 4.0));
}

/// Port of `BOOST_AUTO_TEST_CASE( test_farthest_point_to )`.
#[test]
fn test_farthest_point_to() {
    let b = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(3.0, 4.0));

    // note: the farthest point always is on a corner of the box

    // outside:

    // top left
    assert!(b.farthest_point_to(Vector2D::new(0.0, 0.0)) == Vector2D::new(4.0, 6.0));

    // top right
    assert!(b.farthest_point_to(Vector2D::new(6.0, 0.0)) == Vector2D::new(1.0, 6.0));

    // bottom right
    assert!(b.farthest_point_to(Vector2D::new(6.0, 7.0)) == Vector2D::new(1.0, 2.0));

    // bottom left
    assert!(b.farthest_point_to(Vector2D::new(0.0, 7.0)) == Vector2D::new(4.0, 2.0));

    // inside:

    // top left
    assert!(b.farthest_point_to(Vector2D::new(2.0, 3.0)) == Vector2D::new(4.0, 6.0));

    // top right
    assert!(b.farthest_point_to(Vector2D::new(3.0, 3.0)) == Vector2D::new(1.0, 6.0));

    // bottom right
    assert!(b.farthest_point_to(Vector2D::new(3.0, 5.0)) == Vector2D::new(1.0, 2.0));

    // bottom left
    assert!(b.farthest_point_to(Vector2D::new(2.0, 5.0)) == Vector2D::new(4.0, 2.0));
}

/// Port of `BOOST_AUTO_TEST_CASE( test_intersects_circle )`.
#[test]
fn test_intersects_circle() {
    let b = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(6.0, 8.0));

    // box inside circle (touching corners)
    assert!(b.intersects_circle(Vector2D::new(4.0, 6.0), 5));

    // box completely inside circle
    assert!(b.intersects_circle(Vector2D::new(4.0, 6.0), 6));

    // circle completely inside box
    assert!(b.intersects_circle(Vector2D::new(4.0, 6.0), 2));

    // circle outside box
    assert!(!b.intersects_circle(Vector2D::new(14.0, 6.0), 5));
}

/// Port of `BOOST_AUTO_TEST_CASE( test_intersects_circle_edge )`.
#[test]
fn test_intersects_circle_edge() {
    let b = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(6.0, 8.0));

    // box touching edge
    assert!(b.intersects_circle_edge(Vector2D::new(4.0, 6.0), 5, 1));

    // box completely inside circle
    assert!(!b.intersects_circle_edge(Vector2D::new(4.0, 6.0), 6, 1));

    // circle completely inside box
    assert!(b.intersects_circle_edge(Vector2D::new(4.0, 6.0), 2, 1));

    // circle outside box
    assert!(!b.intersects_circle_edge(Vector2D::new(14.0, 6.0), 5, 1));
}

/// Port of the static helper `refSquaredDistance`.
///
/// Reference distance from a point to a box by clamping per axis
fn ref_squared_distance(a_box: &Box2I, a_p: Vector2I) -> i64 {
    let x1: i64 = (a_box.left() as i64).min(a_box.right() as i64);
    let x2: i64 = (a_box.left() as i64).max(a_box.right() as i64);
    let y1: i64 = (a_box.top() as i64).min(a_box.bottom() as i64);
    let y2: i64 = (a_box.top() as i64).max(a_box.bottom() as i64);
    let px: i64 = a_p.x as i64;
    let py: i64 = a_p.y as i64;
    let dx: i64 = if px < x1 {
        x1 - px
    } else if px > x2 {
        px - x2
    } else {
        0
    };
    let dy: i64 = if py < y1 {
        y1 - py
    } else if py > y2 {
        py - y2
    } else {
        0
    };

    dx * dx + dy * dy
}

/// Port of `BOOST_AUTO_TEST_CASE( SquaredDistanceToPoint )`.
#[test]
fn squared_distance_to_point() {
    let b = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 100));

    assert_eq!(b.squared_distance_point(Vector2I::new(1000, 50)), 810000);
    assert_eq!(b.squared_distance_point(Vector2I::new(-20, 50)), 400);
    assert_eq!(b.squared_distance_point(Vector2I::new(50, 1000)), 810000);
    assert_eq!(b.squared_distance_point(Vector2I::new(50, -30)), 900);
    assert_eq!(b.squared_distance_point(Vector2I::new(50, 50)), 0);

    let coords: [i32; 11] = [-300, -100, -1, 0, 1, 50, 99, 100, 101, 250, 1000];

    for bx in [
        b,
        Box2I::new(Vector2I::new(10, -40), Vector2I::new(60, 25)),
        Box2I::new(Vector2I::new(100, 100), Vector2L::new(-100, -100)),
    ] {
        for x in coords {
            for y in coords {
                assert_eq!(
                    bx.squared_distance_point(Vector2I::new(x, y)),
                    ref_squared_distance(&bx, Vector2I::new(x, y))
                );
            }
        }
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( InflateSentinelDoesNotWrap )`.
#[test]
fn inflate_sentinel_does_not_wrap() {
    let delta: i32 = i32::MAX / 2 + 50000;

    let mut b = Box2I::new(Vector2I::new(1000, 2000), Vector2I::new(300, 400));
    b.inflate(delta);

    assert_eq!(b.left(), 1000 - delta);
    assert_eq!(b.top(), 2000 - delta);
    assert_eq!(b.width(), 300 + 2 * delta as i64);
    assert_eq!(b.height(), 400 + 2 * delta as i64);

    // Far enough to saturate the coordinate range, the box must still cover the original
    let mut huge = Box2I::new(Vector2I::new(-1000, 500), Vector2I::new(2000, 100));
    huge.inflate(i32::MAX);

    assert_eq!(huge.left(), i32::MIN);
    assert!(huge.top() <= 500);
    assert!(huge.left() as i64 + huge.width() >= 1000);
    assert!(huge.top() as i64 + huge.height() >= 600);
}
