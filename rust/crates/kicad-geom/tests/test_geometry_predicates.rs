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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `OrientationSignBeyondDoublePrecision` and `InCircleDelaunayLegalExact` from
//! `qa/tests/libs/kimath/math/test_wide_int.cpp` (suite `WideInt`), which test
//! `geometry/geometry_predicates.h` (the other cases are in `test_wide_int.rs`).

use kicad_geom::geometry::geometry_predicates::{in_circle_delaunay_legal, orientation_sign};
use kicad_geom::math::vector2::Vector2I;

#[test]
fn orientation_sign_beyond_double_precision() {
    // The exact determinant is 1 but a double cross product rounds it to 0
    let a = Vector2I::new(-1500000000, -1500000000);
    let b = Vector2I::new(1500000001, 1500000000);
    let c = Vector2I::new(1500000002, 1500000001);

    assert_eq!(orientation_sign(a, b, c), 1);
    assert_eq!(orientation_sign(a, c, b), -1);
}

#[test]
fn in_circle_delaunay_legal_exact() {
    let a = Vector2I::new(0, 0);
    let b = Vector2I::new(1000000, 0);
    let c = Vector2I::new(0, 1000000);

    // Cocircular is legal, inside is not, outside is
    assert!(in_circle_delaunay_legal(a, b, c, Vector2I::new(1000000, 1000000)));
    assert!(!in_circle_delaunay_legal(a, b, c, Vector2I::new(999999, 999999)));
    assert!(in_circle_delaunay_legal(a, b, c, Vector2I::new(1000001, 1000001)));

    // Exact determinant is 999999999000000000, far below the tolerance of a filtered double test
    assert!(!in_circle_delaunay_legal(
        Vector2I::new(0, 0),
        Vector2I::new(1000000000, 0),
        Vector2I::new(1000000000, 1),
        Vector2I::new(1, 1)
    ));
}
