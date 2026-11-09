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

//! Port of `qa/tests/libs/kimath/geometry/test_circle.cpp`.

use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::MIN_PRECISION_IU;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::check_unordered_matches;

const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

const fn cir(x: i32, y: i32, r: i32) -> Circle {
    Circle::new(pt(x, y), r)
}

fn seg(ax: i32, ay: i32, bx: i32, by: i32) -> Seg {
    Seg::new(pt(ax, ay), pt(bx, by))
}

fn min_precision_45deg() -> i32 {
    ki_round(MIN_PRECISION_IU as f64 * 0.7071)
}

/// Checks whether two lengths are equal, within `MIN_PRECISION_IU`.
fn compare_length(a_length_a: i32, a_length_b: i32) -> bool {
    !(a_length_a > (a_length_b + MIN_PRECISION_IU) || a_length_a < (a_length_b - MIN_PRECISION_IU))
}

/// Checks whether two vectors are equal, within `MIN_PRECISION_IU` per coordinate.
fn compare_vector2i(a_vec_a: &Vector2I, a_vec_b: &Vector2I) -> bool {
    compare_length(a_vec_a.x, a_vec_b.x) && compare_length(a_vec_a.y, a_vec_b.y)
}

/// Checks the default constructor, then the parameter constructor and setters.
#[test]
fn parameter_ctor_mod() {
    let center = pt(10, 20);
    let radius = 10;

    let mut circle = Circle::new(center, radius);

    assert_eq!(circle.center, pt(10, 20));
    assert_eq!(circle.radius, 10);

    circle.center += pt(10, 10);
    circle.radius += 20;

    assert_eq!(circle.center, pt(20, 30));
    assert_eq!(circle.radius, 30);
}

#[test]
fn contains() {
    let p = MIN_PRECISION_IU;
    let p45 = min_precision_45deg();
    let cases: Vec<(&str, Circle, Vector2I, bool)> = vec![
        ("on center", cir(100, 100, 200), pt(100, 100), false),
        ("0 deg", cir(100, 100, 200), pt(300, 100), true),
        ("0 deg, allowed tolerance pos", cir(100, 100, 200), pt(100, 300 + p), true),
        ("0 deg, allowed tolerance neg", cir(100, 100, 200), pt(100, 300 - p), true),
        ("0 deg, allowed tolerance pos + 1", cir(100, 100, 200), pt(100, 300 + p + 1), false),
        ("0 deg, allowed tolerance neg - 1", cir(100, 100, 200), pt(100, 300 - p - 1), false),
        ("45 deg", cir(100, 100, 200), pt(241, 241), true),
        ("45 deg, allowed tolerance pos", cir(100, 100, 200), pt(241 + p45, 241 + p45), true),
        ("45 deg, allowed tolerance pos + 1", cir(100, 100, 200), pt(241 + p45 + 1, 241 + p45 + 1), false),
        ("90 deg", cir(100, 100, 200), pt(100, 300), true),
        ("180 deg", cir(100, 100, 200), pt(-100, 100), true),
        ("270 deg", cir(100, 100, 200), pt(100, -100), true),
    ];

    for (name, circle, point, exp) in cases {
        // the test data is const: the const overload of CIRCLE::Contains()
        assert_eq!(circle.contains(point), exp, "{name}");
    }
}

#[test]
fn nearest_point() {
    let cases: Vec<(&str, Circle, Vector2I, Vector2I)> = vec![
        // special case: when at the circle return a point on the x axis
        ("on center", cir(10, 10, 20), pt(10, 10), pt(30, 10)),
        ("inside", cir(10, 10, 20), pt(10, 20), pt(10, 30)),
        ("outside", cir(10, 10, 20), pt(10, 50), pt(10, 30)),
        ("angled", cir(10, 10, 20), pt(50, 50), pt(24, 24)),
    ];

    for (name, circle, point, exp) in cases {
        assert_eq!(circle.nearest_point(point), exp, "{name}");
    }
}

#[test]
fn intersect_circle() {
    let cases: Vec<(&str, Circle, Circle, Vec<Vector2I>)> = vec![
        ("two point aligned", cir(10, 10, 20), cir(10, 45, 20), vec![pt(0, 27), pt(21, 27)]),
        ("two point angled", cir(10, 10, 20), cir(20, 20, 20), vec![pt(2, 28), pt(28, 2)]),
        ("tangent aligned, external", cir(10, 10, 20), cir(10, 50, 20), vec![pt(10, 30)]),
        ("tangent aligned, internal", cir(10, 10, 40), cir(10, 30, 20), vec![pt(10, 50)]),
        ("no intersection", cir(10, 10, 20), cir(10, 51, 20), vec![]),
        ("KiROUND overflow 1", cir(44798001, -94001999, 200001), cir(44797999, -94001999, 650001), vec![]),
        ("KiROUND overflow 2", cir(50747999, -92402001, 650001), cir(50748001, -92402001, 200001), vec![]),
        ("KiROUND overflow 3", cir(43947999, -92402001, 650001), cir(43948001, -92402001, 200001), vec![]),
        ("KiROUND overflow 4", cir(46497999, -94001999, 200001), cir(46498001, -94001999, 650001), vec![]),
        // Exercise d=0
        (
            "Co-centered, same radius",
            cir(205999999, 136367974, 3742026),
            cir(205999999, 136367974, 3742026),
            vec![],
        ),
    ];

    for (name, c1, c2, exp) in cases {
        let ret1 = c1.intersect(&c2);
        assert_eq!(exp.len(), ret1.len(), "{name} Case 1");
        let fails = check_unordered_matches(&exp, &ret1, compare_vector2i);
        assert!(fails.is_empty(), "{name} Case 1: {fails:?}");

        let ret2 = c2.intersect(&c1);
        assert_eq!(exp.len(), ret2.len(), "{name} Case 2");
        let fails = check_unordered_matches(&exp, &ret2, compare_vector2i);
        assert!(fails.is_empty(), "{name} Case 2: {fails:?}");
    }
}

#[test]
fn intersect() {
    let cases: Vec<(&str, Circle, Seg, Vec<Vector2I>)> = vec![
        ("two point aligned", cir(0, 0, 20), seg(10, -40, 10, 40), vec![pt(10, -17), pt(10, 17)]),
        ("two point angled", cir(0, 0, 20), seg(-20, -40, 20, 40), vec![pt(8, 17), pt(-8, -17)]),
        ("tangent", cir(0, 0, 20), seg(20, 0, 20, 40), vec![pt(20, 0)]),
        ("no intersection", cir(0, 0, 20), seg(25, 0, 25, 40), vec![]),
        ("no intersection: seg end points inside circle", cir(0, 0, 20), seg(0, 10, 0, -10), vec![]),
    ];

    for (name, circle, s, exp) in cases {
        let ret = circle.intersect_seg(&s);
        assert_eq!(exp.len(), ret.len(), "{name}");
        let fails = check_unordered_matches(&exp, &ret, compare_vector2i);
        assert!(fails.is_empty(), "{name}: {fails:?}");
    }
}

#[test]
fn intersect_line() {
    let cases: Vec<(&str, Circle, Seg, Vec<Vector2I>)> = vec![
        ("two point aligned", cir(0, 0, 20), seg(10, 45, 10, 40), vec![pt(10, -17), pt(10, 17)]),
        ("two point angled", cir(0, 0, 20), seg(-20, -40, 20, 40), vec![pt(8, 17), pt(-8, -17)]),
        ("tangent", cir(0, 0, 20), seg(20, 0, 20, 40), vec![pt(20, 0)]),
        ("no intersection", cir(0, 0, 20), seg(25, 0, 25, 40), vec![]),
        (
            "intersection, seg end points inside circle",
            cir(0, 0, 20),
            seg(0, 10, 0, -10),
            vec![pt(0, 20), pt(0, -20)],
        ),
    ];

    for (name, circle, s, exp) in cases {
        let ret = circle.intersect_line(&s);
        assert_eq!(exp.len(), ret.len(), "{name}");
        let fails = check_unordered_matches(&exp, &ret, compare_vector2i);
        assert!(fails.is_empty(), "{name}: {fails:?}");
    }
}

#[test]
fn construct_from_tan_tan_pt() {
    let cases: Vec<(&str, Seg, Seg, Vector2I, Circle)> = vec![
        // result from simple geometric inference
        ("90 degree segs, point on seg", seg(0, 0, 0, 1000), seg(0, 0, 1000, 0), pt(0, 400), cir(400, 400, 400)),
        // result from LibreCAD 2.2.0-rc2
        ("90 degree segs, point floating", seg(0, 0, 0, 1000), seg(0, 0, 1000, 0), pt(200, 100), cir(500, 500, 500)),
        // result from LibreCAD 2.2.0-rc2
        ("45 degree segs, point on seg", seg(0, 0, 1000, 0), seg(0, 0, 1000, 1000), pt(400, 0), cir(400, 166, 166)),
        // result from LibreCAD 2.2.0-rc2
        (
            "45 degree segs, point floating",
            seg(0, 0, 1000000, 0),
            seg(0, 0, 1000000, 1000000),
            pt(200000, 100000),
            cir(332439, 137701, 137701),
        ),
        // amended to get the test to pass
        (
            "135 degree segs, point on seg",
            seg(0, 0, 1000000, 0),
            seg(0, 0, -1000000, 1000000),
            pt(400000, 0),
            cir(400009, 965709, 965709),
        ),
        // amended to get the test to pass
        (
            "135 degree segs, point floating",
            seg(0, 0, 1000, 0),
            seg(0, 0, -1000, 1000),
            pt(200, 100),
            cir(814, 1964, 1964),
        ),
        // special case: radius=0
        ("point on intersection", seg(10, 0, 1000, 0), seg(10, 0, -1000, 1000), pt(10, 0), cir(10, 0, 0)),
    ];

    for (name, seg_a, seg_b, p, exp) in cases {
        let mut circle = Circle::default();
        circle.construct_from_tan_tan_pt(&seg_a, &seg_b, p);

        assert!(
            compare_vector2i(&exp.center, &circle.center),
            "{name}: Center point mismatch: Got: {:?} Expected: {:?}",
            circle.center,
            exp.center
        );
        assert!(
            compare_length(exp.radius, circle.radius),
            "{name}: Radius mismatch: Got: {} Expected: {}",
            circle.radius,
            exp.radius
        );
    }
}
