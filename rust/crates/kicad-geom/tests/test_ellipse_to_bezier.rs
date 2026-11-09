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

//! Port of `qa/tests/libs/kimath/geometry/test_ellipse_to_bezier.cpp` (suite
//! `EllipseToBezier`).

use kicad_geom::bezier_curves::{Bezier, transform_ellipse_to_beziers};
use kicad_geom::geometry::eda_angle::{ANGLE_0, ANGLE_180, EdaAngle, FULL_CIRCLE, RADIANS_T};
use kicad_geom::geometry::ellipse::Ellipse;
use kicad_geom::math::vector2::Vector2D;

/// Allows for rounding in the testcases
const MAX_ERROR: f64 = 0.01;

type P = (f64, f64);

fn bz(s: P, c1: P, c2: P, e: P) -> Bezier<f64> {
    let v = |p: P| Vector2D::new(p.0, p.1);
    Bezier::new(v(s), v(c1), v(c2), v(e))
}

fn ell(center: P, major: P, ratio: f64, start: EdaAngle, end: EdaAngle) -> Ellipse<f64> {
    Ellipse::from_major_axis(
        Vector2D::new(center.0, center.1),
        Vector2D::new(major.0, major.1),
        ratio,
        start,
        end,
    )
}

#[test]
fn ellipse_to_bezier() {
    let rad = |r: f64| EdaAngle::new(r, RADIANS_T);

    #[rustfmt::skip]
    let cases: Vec<(&str, Ellipse<f64>, Vec<Bezier<f64>>)> = vec![
        (
            "full circle",
            ell((0.0, 0.0), (100.0, 0.0), 1.0, ANGLE_0, FULL_CIRCLE),
            vec![
                bz((100.0, 0.0), (100.0, -55.22847498307934), (55.228474983079344, -100.0), (0.0, -100.0)),
                bz((0.0, -100.0), (-55.22847498307934, -100.0), (-100.0, -55.228474983079344), (-100.0, 0.0)),
                bz((-100.0, 0.0), (-100.0, 55.22847498307934), (-55.228474983079344, 100.0), (0.0, 100.0)),
                bz((0.0, 100.0), (55.22847498307934, 100.0), (100.0, 55.228474983079344), (100.0, 0.0)),
            ],
        ),
        (
            "ellipse",
            ell((0.0, 0.0), (-100.0, 0.0), 0.5, ANGLE_0, FULL_CIRCLE),
            vec![
                bz((-100.0, 0.0), (-100.0, 27.61423749153967), (-55.228474983079344, 50.0), (0.0, 50.0)),
                bz((0.0, 50.0), (55.22847498307934, 50.0), (100.0, 27.614237491539672), (100.0, 0.0)),
                bz((100.0, 0.0), (100.0, -27.61423749153967), (55.228474983079344, -50.0), (0.0, -50.0)),
                bz((0.0, -50.0), (-55.22847498307934, -50.0), (-100.0, -27.614237491539672), (-100.0, 0.0)),
            ],
        ),
        (
            "arc1",
            ell((0.0, 0.0), (100.0, 0.0), 0.5, ANGLE_180, FULL_CIRCLE),
            vec![
                bz((-100.0, 0.0), (-100.0, 27.61423749153967), (-55.228474983079344, 50.0), (0.0, 50.0)),
                bz((0.0, 50.0), (55.22847498307934, 50.0), (100.0, 27.614237491539672), (100.0, 0.0)),
            ],
        ),
        (
            "arc2",
            ell((223.0, 165.0), (372.0, 634.0), 0.96, rad(4.437), rad(0.401)),
            vec![
                bz((-463.86, 336.27), (-389.81, 608.33), (-170.75, 818.49), (99.52, 876.73)),
                bz((99.52, 876.73), (369.80, 934.98), (643.36, 831.0), (803.07, 609.31)),
            ],
        ),
        (
            "arc3",
            ell((112.75, 490.24), (304.54, 129.16), 7.14, rad(1.90), rad(1.09)),
            vec![
                bz((886.98, -1609.17), (608.61, -1333.45), (110.16, -394.92), (-305.88, 636.89)),
                bz((-305.88, 636.89), (-721.93, 1668.69), (-940.88, 2509.32), (-829.87, 2648.63)),
                bz((-829.87, 2648.63), (-718.85, 2787.95), (-308.47, 2187.55), (152.23, 1211.78)),
                bz((152.23, 1211.78), (612.93, 236.01), (996.95, -846.11), (1071.24, -1377.92)),
            ],
        ),
    ];

    for (name, input, expected) in &cases {
        let mut out: Vec<Bezier<f64>> = Vec::new();
        transform_ellipse_to_beziers(input, &mut out);

        assert_eq!(expected.len(), out.len(), "{name}");

        for (e, o) in expected.iter().zip(out.iter()) {
            assert!((e.start - o.start).euclidean_norm() <= MAX_ERROR, "{name}: {o:?}");
            assert!((e.c1 - o.c1).euclidean_norm() <= MAX_ERROR, "{name}: {o:?}");
            assert!((e.c2 - o.c2).euclidean_norm() <= MAX_ERROR, "{name}: {o:?}");
            assert!((e.end - o.end).euclidean_norm() <= MAX_ERROR, "{name}: {o:?}");
        }
    }
}
