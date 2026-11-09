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

//! Port of `qa/tests/libs/kimath/geometry/test_half_line.cpp`.

use kicad_geom::geometry::half_line::HalfLine;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_utils::clip_half_line_to_box;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::segments_have_same_end_points;

const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn hl(ax: i32, ay: i32, bx: i32, by: i32) -> HalfLine {
    HalfLine::from_seg(Seg::new(pt(ax, ay), pt(bx, by)))
}

#[test]
fn contains() {
    let cases: Vec<(&str, HalfLine, Vector2I, bool)> = vec![
        ("Point on the ray", hl(0, 0, 100, 100), pt(50, 50), true),
        ("Point on the ray start", hl(0, 0, 100, 100), pt(0, 0), true),
        ("Point on the ray end", hl(0, 0, 100, 100), pt(100, 100), true),
        ("Point on the ray, past the end", hl(0, 0, 100, 100), pt(150, 150), true),
        ("Point on the infinite line, but on the wrong side", hl(0, 0, 100, 100), pt(-50, -50), false),
        ("Point to one side", hl(0, 0, 100, 100), pt(50, 0), false),
    ];

    for (name, h, p, exp) in cases {
        assert_eq!(h.contains(p), exp, "{name}");
    }
}

#[test]
fn intersect() {
    let cases: Vec<(&str, HalfLine, HalfLine, Option<Vector2I>)> = vec![
        ("Simple cross", hl(-100, -100, 0, 0), hl(100, -100, 0, 0), Some(pt(0, 0))),
        ("Parallel, no intersection", hl(-100, 0, -100, 100), hl(100, 0, 100, 100), None),
    ];

    for (name, a, b, exp) in cases {
        let intersection = a.intersect(&b);
        assert_eq!(intersection.is_some(), exp.is_some(), "{name}");

        if let Some(i) = intersection {
            assert_eq!(i, exp.unwrap(), "{name}");
        }
    }
}

#[test]
fn clip_to_box() {
    let bx = Box2I::new(pt(-1000, -1000), pt(2000, 2000));
    let cases: Vec<(&str, HalfLine, Box2I, Option<Seg>)> = vec![
        ("Center to right edge", hl(0, 0, 100, 0), bx, Some(Seg::new(pt(0, 0), pt(1000, 0)))),
        ("Centre to corner", hl(0, 0, 100, 100), bx, Some(Seg::new(pt(0, 0), pt(1000, 1000)))),
        ("Ray not in the box", hl(1500, 0, 1600, 0), bx, None),
        (
            "Ray starts outside but crosses box",
            hl(-1500, 0, 0, 0),
            bx,
            Some(Seg::new(pt(-1000, 0), pt(1000, 0))),
        ),
    ];

    for (name, h, b, exp) in cases {
        let clipped = clip_half_line_to_box(&h, &b);
        assert_eq!(clipped.is_some(), exp.is_some(), "{name}");

        if let Some(c) = clipped {
            assert!(segments_have_same_end_points(&c, &exp.unwrap()), "{name}: {c:?}");
        }
    }
}
