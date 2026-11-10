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

//! Port of `qa/tests/libs/kimath/geometry/test_roundrect.cpp` (suite `Roundrect`).

use kicad_geom::geometry::roundrect::Roundrect;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::math::vector2::Vector2I;

/// Port of `CheckPolygonFitsRect()`: the polygon must fit its rectangle.  One maxError of slack
/// covers the arc chords.  Catches the maxed-radius circle coming out too big (#24623).
fn check_polygon_fits_rect(a_width: i32, a_height: i32, a_radius: i32) {
    let max_error = 100;
    let pos = Vector2I::new(1000, 2000);
    let rr = Roundrect::new(
        ShapeRect::from_pos_size(pos, a_width, a_height),
        a_radius,
        false,
    );
    let mut poly = ShapePolySet::new();

    rr.transform_to_polygon(&mut poly, max_error);

    assert_eq!(poly.outline_count(), 1);

    let bbox = poly.bbox(0);
    let tol = max_error as i64;

    assert!((bbox.left() as i64 - pos.x as i64).abs() <= tol);
    assert!((bbox.top() as i64 - pos.y as i64).abs() <= tol);
    assert!((bbox.width() - a_width as i64).abs() <= tol);
    assert!((bbox.height() - a_height as i64).abs() <= tol);
}

/// Radius is half of both sides, so it becomes a circle (#24623).
#[test]
fn circle_from_square() {
    check_polygon_fits_rect(10000000, 10000000, 5000000);
}

/// Radius maxed on the height only, giving a horizontal oval.
#[test]
fn oval_wide() {
    check_polygon_fits_rect(20000000, 10000000, 5000000);
}

/// Radius maxed on the width only, giving a vertical oval.
#[test]
fn oval_tall() {
    check_polygon_fits_rect(10000000, 20000000, 5000000);
}

/// Ordinary roundrect, corners below the max radius.
#[test]
fn normal_roundrect() {
    check_polygon_fits_rect(20000000, 10000000, 2000000);
}

/// Zero radius is a plain rectangle.
#[test]
fn plain_rectangle() {
    check_polygon_fits_rect(20000000, 10000000, 0);
}

/// Zero radius corners always come out TL TR BR BL regardless of orientation; pcbnew VERTEX
/// constraints rely on this order (reorder means silent rebind).
#[test]
fn plain_rectangle_corner_order() {
    let pos = Vector2I::new(1000, 2000);
    let w = 20000000;
    let h = 10000000;

    // Normalized rect plus negative extent variant hits normalization recursion
    for rect in [
        ShapeRect::from_pos_size(pos, w, h),
        ShapeRect::from_pos_size(pos + Vector2I::new(w, h), -w, -h),
    ] {
        let rr = Roundrect::new(rect, 0, false);
        let mut poly = ShapePolySet::new();
        rr.transform_to_polygon(&mut poly, 100);

        assert_eq!(poly.outline_count(), 1);

        let outline = poly.c_outline(0);
        assert_eq!(outline.point_count(), 4);
        assert_eq!(outline.c_point(0), pos);
        assert_eq!(outline.c_point(1), pos + Vector2I::new(w, 0));
        assert_eq!(outline.c_point(2), pos + Vector2I::new(w, h));
        assert_eq!(outline.c_point(3), pos + Vector2I::new(0, h));
    }
}
