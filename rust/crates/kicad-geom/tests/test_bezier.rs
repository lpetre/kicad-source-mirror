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

//! Port of `qa/tests/libs/kimath/geometry/test_bezier.cpp` (suite `Bezier`).

use kicad_geom::bezier_curves::Bezier;
use kicad_geom::math::vector2::Vector2D;
use kicad_geom::test_utils::is_small;

fn check_vec_close(a_actual: Vector2D, a_expected: Vector2D) {
    const TOL: f64 = 1e-9;
    assert!(
        is_small(a_actual.x - a_expected.x, TOL),
        "{a_actual:?} vs {a_expected:?}"
    );
    assert!(
        is_small(a_actual.y - a_expected.y, TOL),
        "{a_actual:?} vs {a_expected:?}"
    );
}

fn curve() -> Bezier<f64> {
    Bezier::new(
        Vector2D::new(0.0, 0.0),
        Vector2D::new(30.0, 90.0),
        Vector2D::new(70.0, -40.0),
        Vector2D::new(100.0, 0.0),
    )
}

#[test]
fn split_preserves_endpoints() {
    let curve = curve();
    let (left, right) = curve.split(0.35);

    check_vec_close(left.start, curve.start);
    check_vec_close(left.end, curve.point_at(0.35));
    check_vec_close(right.start, left.end);
    check_vec_close(right.end, curve.end);
}

#[test]
fn sub_curve_preserves_original_parameterization() {
    let curve = curve();
    let t0 = 0.2;
    let t1 = 0.8;
    let sub_curve = curve.sub_curve(t0, t1);

    check_vec_close(sub_curve.start, curve.point_at(t0));
    check_vec_close(sub_curve.end, curve.point_at(t1));
    check_vec_close(
        sub_curve.point_at(0.5),
        curve.point_at(t0 + (t1 - t0) * 0.5),
    );
}
