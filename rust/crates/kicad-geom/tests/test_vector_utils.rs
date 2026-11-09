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

//! Port of `qa/tests/libs/kimath/geometry/test_vector_utils.cpp` (Boost test suite
//! `VectorUtils`).

use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::vector_utils::{
    get_length_ratio_from_start, get_nearest_endpoint, point_is_in_direction,
    point_projects_onto_segment,
};
use kicad_geom::math::vector2::Vector2I;

/// `BOOST_CHECK_CLOSE( a, b, percent )`: both relative differences are within the tolerance,
/// which is given in percent.
fn check_close(a: f64, b: f64, percent: f64) -> bool {
    let diff = (a - b).abs();
    let tol = percent / 100.0;

    diff <= tol * a.abs() && diff <= tol * b.abs()
}

/// Port of `BOOST_AUTO_TEST_CASE( PointIsInDirection )`.
#[test]
fn point_is_in_direction_test() {
    let p0 = Vector2I::new(1000, 1000);

    let ne = Vector2I::new(100, 100);
    let n = Vector2I::new(0, 100);
    let s = Vector2I::new(0, -100);

    // To the east, so not directly inline with ne from p0
    let p1_east: Vector2I = p0 + Vector2I::new(1000, 0);
    let p1_west: Vector2I = p0 - Vector2I::new(1000, 0);

    assert!(point_is_in_direction(p1_east, ne, p0));
    assert!(!point_is_in_direction(p1_west, ne, p0));

    // Test the perpendicular corner case

    // Points on both sides are not in the direction
    assert!(!point_is_in_direction(p1_east, n, p0));
    assert!(!point_is_in_direction(p1_west, n, p0));

    // And they're also not in the opposite direction
    assert!(!point_is_in_direction(p1_east, s, p0));
    assert!(!point_is_in_direction(p1_west, s, p0));
}

/// Port of `BOOST_AUTO_TEST_CASE( ProjectsOntoSeg )`.
#[test]
fn projects_onto_seg() {
    let seg = Seg::new(Vector2I::new(0, 0), Vector2I::new(100, 0));

    assert!(point_projects_onto_segment(Vector2I::new(50, 1000), &seg));
    assert!(!point_projects_onto_segment(Vector2I::new(150, 1000), &seg));
}

/// Port of `BOOST_AUTO_TEST_CASE( LengthRatio )`.
#[test]
fn length_ratio() {
    let seg = Seg::new(Vector2I::new(0, 0), Vector2I::new(100, 0));

    assert!(get_length_ratio_from_start(Vector2I::new(0, 0), &seg) == 0.0);
    assert!(get_length_ratio_from_start(Vector2I::new(100, 0), &seg) == 1.0);
    assert!(get_length_ratio_from_start(Vector2I::new(50, 0), &seg) == 0.5);

    // Points not on the segment also work
    assert!(check_close(
        get_length_ratio_from_start(Vector2I::new(0, 100), &seg),
        1.0,
        0.0001
    ));
    assert!(check_close(
        get_length_ratio_from_start(Vector2I::new(0, 50), &seg),
        0.5,
        0.0001
    ));
}

/// Port of `BOOST_AUTO_TEST_CASE( NearestEndpoint )`.
#[test]
fn nearest_endpoint() {
    struct PtCase {
        point: Vector2I,
        expected_start: bool,
    }

    let seg = Seg::new(Vector2I::new(0, 0), Vector2I::new(100, 0));

    let pc = |x: i32, y: i32, expected_start: bool| PtCase {
        point: Vector2I::new(x, y),
        expected_start,
    };

    let cases: Vec<PtCase> = vec![
        pc(-100, 0, true),
        pc(0, 0, true),
        pc(0, 50, true),
        // Make sure the tie breaks predictably
        // Equidistant points -> start
        pc(50, 0, true),
        pc(50, 50, true),
        pc(50, -50, true),
        // End points
        pc(200, 0, false),
        pc(100, 0, false),
        pc(100, 50, false),
    ];

    for pc in &cases {
        assert!(
            get_nearest_endpoint(&seg, pc.point) == (if pc.expected_start { seg.a } else { seg.b }),
            "Point: {}",
            pc.point.to_stream_string()
        );
    }
}
