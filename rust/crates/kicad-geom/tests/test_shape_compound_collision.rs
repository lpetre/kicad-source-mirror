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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_compound_collision.cpp` (suite
//! `SCompoundCollision`) and `test_shape_rect_corner.cpp`.

use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::math::vector2::Vector2I;

/// `ShapeCompoundCollisionFixture`
struct ShapeCompoundCollisionFixture {
    shapes_a: Vec<ShapeCircle>,
    shapes_b: Vec<ShapeCircle>,
    shapes_c: Vec<ShapeCircle>,
    compound_a: ShapeCompound,
    compound_b: ShapeCompound,
    compound_c: ShapeCompound,
}

impl ShapeCompoundCollisionFixture {
    fn new() -> Self {
        let c = |x, y| ShapeCircle::new(Vector2I::new(x, y), 100);
        let shapes_a = vec![c(0, 0), c(80, 0)];
        let shapes_b = vec![c(0, 80), c(80, 80)];
        let shapes_c = vec![c(0, 280), c(80, 280)];

        let compound = |v: &Vec<ShapeCircle>| {
            ShapeCompound::from_shapes(v.iter().map(|s| s.clone_shape()).collect())
        };

        Self {
            compound_a: compound(&shapes_a),
            compound_b: compound(&shapes_b),
            compound_c: compound(&shapes_c),
            shapes_a,
            shapes_b,
            shapes_c,
        }
    }
}

#[test]
fn shape_compound_collide() {
    let f = ShapeCompoundCollisionFixture::new();
    let mut actual = 0;

    // Check points on corners
    assert!(
        f.compound_a
            .collide(&f.compound_b, 0, Some(&mut actual), None)
    );
    assert_eq!(actual, 0);

    assert!(
        !f.compound_a
            .collide(&f.compound_c, 0, Some(&mut actual), None)
    );
    assert_eq!(actual, 0);

    assert!(
        f.compound_a
            .collide(&f.compound_c, 100, Some(&mut actual), None)
    );
    assert_eq!(actual, 80);

    assert!(f.shapes_a[0].collide(&f.compound_b, 0, None, None));
    assert!(f.shapes_a[1].collide(&f.compound_b, 0, None, None));
    assert!(f.compound_b.collide(&f.shapes_a[0], 0, None, None));
    assert!(f.compound_b.collide(&f.shapes_a[1], 0, None, None));

    assert!(f.shapes_b[0].collide(&f.compound_a, 0, None, None));
    assert!(f.shapes_b[1].collide(&f.compound_a, 0, None, None));
    assert!(f.compound_a.collide(&f.shapes_b[0], 0, None, None));
    assert!(f.compound_a.collide(&f.shapes_b[1], 0, None, None));

    assert!(!f.shapes_c[0].collide(&f.compound_a, 0, None, None));
    assert!(!f.shapes_c[1].collide(&f.compound_a, 0, None, None));
    assert!(!f.compound_a.collide(&f.shapes_c[0], 0, None, None));
    assert!(!f.compound_a.collide(&f.shapes_c[1], 0, None, None));

    assert!(!f.shapes_a[0].collide(&f.compound_c, 0, None, None));
    assert!(!f.shapes_a[1].collide(&f.compound_c, 0, None, None));
    assert!(!f.compound_c.collide(&f.shapes_a[0], 0, None, None));
    assert!(!f.compound_c.collide(&f.shapes_a[1], 0, None, None));

    assert!(f.shapes_c[0].collide(&f.compound_a, 100, Some(&mut actual), None));
    assert_eq!(actual, 80);
    assert!(f.shapes_c[1].collide(&f.compound_a, 100, Some(&mut actual), None));
    assert_eq!(actual, 80);
    assert!(
        f.compound_a
            .collide(&f.shapes_c[0], 100, Some(&mut actual), None)
    );
    assert_eq!(actual, 80);
    assert!(
        f.compound_a
            .collide(&f.shapes_c[1], 100, Some(&mut actual), None)
    );
    assert_eq!(actual, 80);
}

/// `test_shape_rect_corner.cpp`: `ShapeRectCornerRadius`
#[test]
fn shape_rect_corner_radius() {
    let mut rect = ShapeRect::from_corners(Vector2I::new(0, 0), Vector2I::new(10, 10));
    rect.set_radius(2);
    assert_eq!(rect.get_radius(), 2);
}

/// Indexed sub-shape access (no C++ counterpart; DRC r-tree payloads).
#[test]
fn indexable_subshape_by_index() {
    let f = ShapeCompoundCollisionFixture::new();
    assert_eq!(f.compound_a.indexable_subshape_count(), 2);

    for (i, s) in f.compound_a.indexable_subshapes().iter().enumerate() {
        let by_index = f.compound_a.indexable_subshape(i).unwrap();
        assert_eq!(by_index.bbox(0), s.bbox(0));
    }

    assert!(f.compound_a.indexable_subshape(2).is_none());
    assert!(f.shapes_a[0].indexable_subshape(0).is_none());
}
