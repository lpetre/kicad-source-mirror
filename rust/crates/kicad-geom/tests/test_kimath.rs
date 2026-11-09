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

//! Port of the parts of `qa/tests/libs/kimath/test_kimath.cpp` (Boost test suite `KiMath`)
//! that test `trigo.h`: `TestCalcArcCenter3Pts`, `TestInterceptsPositiveX` and
//! `TestInterceptsNegativeX`.
//!
//! `TestHash128` and `TestMMH3Hash` test `hash_128.h` / `mmh3_hash.h`, which are not part of
//! this work package.

use kicad_geom::geometry::eda_angle::EdaAngle;
use kicad_geom::math::vector2::{Vector2D, Vector2I};
use kicad_geom::trigo::{calc_arc_center_d, intercepts_negative_x, intercepts_positive_x, rotate_point_around};

/*
CircleCenterFrom3Points calculate the center of a circle defined by 3 points
It is similar to CalcArcCenter( const VECTOR2D& aStart, const VECTOR2D& aMid, const VECTOR2D& aEnd )
but it was needed to debug CalcArcCenter, so I keep it available for other issues in CalcArcCenter

The perpendicular bisector of the segment between two points is the
set of all points equidistant from both.  So if you take the
perpendicular bisector of (x1,y1) and (x2,y2) and the perpendicular
bisector of the segment from (x2,y2) to (x3,y3) and find the
intersection of those lines, that point will be the center.

To find the equation of the perpendicular bisector of (x1,y1) to (x2,y2),
you know that it passes through the midpoint of the segment:
((x1+x2)/2,(y1+y2)/2), and if the slope of the line
connecting (x1,y1) to (x2,y2) is m, the slope of the perpendicular
bisector is -1/m.  Work out the equations for the two lines, find
their intersection, and bingo!  You've got the coordinates of the center.

An error should occur if the three points lie on a line, and you'll
need special code to check for the case where one of the slopes is zero.

see https://web.archive.org/web/20171223103555/http://mathforum.org/library/drmath/view/54323.html
*/
/// Port of the static helper `Ref0CircleCenterFrom3Points`.
fn ref0_circle_center_from_3_points(p1: Vector2D, p2: Vector2D, p3: Vector2D) -> Option<Vector2D> {
    // Move coordinate origin to p2, to simplify calculations
    let b: Vector2D = p1 - p2;
    let d: Vector2D = p3 - p2;
    let bc: f64 = (b.x * b.x + b.y * b.y) / 2.0;
    let cd: f64 = (-d.x * d.x - d.y * d.y) / 2.0;
    let mut det: f64 = -b.x * d.y + d.x * b.y;

    if det.abs() < 1.0e-6 {
        // arbitrary limit to avoid divide by 0
        return None;
    }

    det = 1.0 / det;

    let mut center = Vector2D::new((-bc * d.y - cd * b.y) * det, (b.x * cd + d.x * bc) * det);
    center += p2;

    Some(center)
}

/// Port of the static helper `Ref1CalcArcCenter`.
///
/// Based on https://paulbourke.net/geometry/circlesphere/
/// Uses the Y'b equation too, depending on slope values
fn ref1_calc_arc_center(a_start: Vector2D, a_mid: Vector2D, a_end: Vector2D) -> Vector2D {
    let mut center = Vector2D::default();
    let y_delta_21: f64 = a_mid.y - a_start.y;
    let x_delta_21: f64 = a_mid.x - a_start.x;
    let y_delta_32: f64 = a_end.y - a_mid.y;
    let x_delta_32: f64 = a_end.x - a_mid.x;

    // This is a special case for aMid as the half-way point when aSlope = 0 and bSlope = inf
    // or the other way around.  In that case, the center lies in a straight line between
    // aStart and aEnd
    if ((x_delta_21 == 0.0) && (y_delta_32 == 0.0)) || ((y_delta_21 == 0.0) && (x_delta_32 == 0.0)) {
        center.x = (a_start.x + a_end.x) / 2.0;
        center.y = (a_start.y + a_end.y) / 2.0;
        return center;
    }

    // Prevent div=0 errors
    /*if( xDelta_21 == 0.0 )
        xDelta_21 = std::numeric_limits<double>::epsilon();

    if( xDelta_32 == 0.0 )
        xDelta_32 = -std::numeric_limits<double>::epsilon();*/

    let a_slope: f64 = y_delta_21 / x_delta_21;
    let b_slope: f64 = y_delta_32 / x_delta_32;

    if a_slope == b_slope {
        if a_start == a_end {
            // This is a special case for a 360 degrees arc.  In this case, the center is halfway between
            // the midpoint and either newEnd point
            center.x = (a_start.x + a_mid.x) / 2.0;
            center.y = (a_start.y + a_mid.y) / 2.0;
            return center;
        } else {
            // If the points are colinear, the center is at infinity, so offset
            // the slope by a minimal amount
            // Warning: This will induce a small error in the center location
            /*aSlope += std::numeric_limits<double>::epsilon();
            bSlope -= std::numeric_limits<double>::epsilon();*/
        }
    }

    center.x = (a_slope * b_slope * (a_start.y - a_end.y) + b_slope * (a_start.x + a_mid.x)
        - a_slope * (a_mid.x + a_end.x))
        / (2.0 * (b_slope - a_slope));

    if a_slope.abs() > b_slope.abs() {
        center.y = ((a_start.x + a_mid.x) / 2.0 - center.x) / a_slope + (a_start.y + a_mid.y) / 2.0;
    } else {
        center.y = ((a_mid.x + a_end.x) / 2.0 - center.x) / b_slope + (a_mid.y + a_end.y) / 2.0;
    }

    center
}

/// Port of `TEST_CALC_ARC_CENTER_CASE`.
struct TestCalcArcCenterCase {
    istart: Vector2I,
    imid: Vector2I,
    iend: Vector2I,
}

/// Port of `BOOST_AUTO_TEST_CASE( TestCalcArcCenter3Pts )`.
///
/// As in the C++ test, the list of cases is empty (its only entry is commented out upstream
/// as "Fails currently", and so is the loop over random points), so the body below does not
/// execute; it is ported so that the test is ready when upstream enables cases.
#[test]
fn test_calc_arc_center_3_pts() {
    // Order: start, mid, end
    let calc_center_cases: Vec<TestCalcArcCenterCase> = vec![
        //{ { 183000000, 89000000 }, { 185333332, 89000000 }, { 185333333, 91333333 } } // Fails currently
    ];

    let tolerance: f64 = 1.0;

    // Check random points (fails currently)
    /*for( int i = 0; i < 100; i++ )
    {
        TEST_CALC_ARC_CENTER_CASE cas;

        cas.istart.x = rand();
        cas.istart.y = rand();

        cas.imid.x = rand();
        cas.imid.y = rand();

        cas.iend.x = rand();
        cas.iend.y = rand();

        calc_center_cases.push_back( cas );
    }*/

    for entry in &calc_center_cases {
        let mut msg = String::new();

        let start: Vector2D = entry.istart.cast();
        let mid: Vector2D = entry.imid.cast();
        let end: Vector2D = entry.iend.cast();

        let calc_center: Vector2D = calc_arc_center_d(start, mid, end);

        let crs: f64 = (calc_center - start).euclidean_norm();
        let crm: f64 = (calc_center - mid).euclidean_norm();
        let cre: f64 = (calc_center - end).euclidean_norm();

        let cavg: f64 = (crs + crm + cre) / 3.0;

        if (crs - cavg).abs() > tolerance || (crm - cavg).abs() > tolerance || (cre - cavg).abs() > tolerance {
            msg += "CalcArcCenter failed.";

            msg += &format!("\nstart: {}", entry.istart.format());
            msg += &format!("\nmid: {}", entry.imid.format());
            msg += &format!("\nend: {}", entry.iend.format());

            {
                msg += &format!("\nCalculated center: {:.15}, {:.15}", calc_center.x, calc_center.y);

                msg += &format!("\n  Avg radius: {cavg:.15}");
                msg += &format!("\nStart radius: {crs:.15}");
                msg += &format!("\n  Mid radius: {crm:.15}");
                msg += &format!("\n  End radius: {cre:.15}");
                msg += "\n";

                // Check mid/end points using the calculated center (like SHAPE_ARC)
                let ang_start = EdaAngle::from_vector(start - calc_center);
                let _ang_mid = EdaAngle::from_vector(mid - calc_center);
                let ang_end = EdaAngle::from_vector(end - calc_center);

                let ang_center: EdaAngle = ang_end - ang_start;

                let mut new_mid: Vector2D = start;
                let mut new_end: Vector2D = start;

                rotate_point_around(&mut new_mid, calc_center, -ang_center / 2.0);
                rotate_point_around(&mut new_end, calc_center, -ang_center);

                msg += &format!("\nNew mid: {:.15}, {:.15}", new_mid.x, new_mid.y);

                msg += &format!("\nNew end: {:.15}, {:.15}", new_end.x, new_end.y);
                msg += "\n";

                let ends_dist: f64 = (new_end - end).euclidean_norm();

                msg += &format!("\nNew end is off by {ends_dist:.15}");
                msg += "\n";
            }

            {
                let ref0_center: Vector2D = ref0_circle_center_from_3_points(start, mid, end).unwrap_or_default();

                let r0_rs: f64 = (ref0_center - start).euclidean_norm();
                let r0_rm: f64 = (ref0_center - mid).euclidean_norm();
                let r0_rre: f64 = (ref0_center - end).euclidean_norm();

                let r0_ravg: f64 = (r0_rs + r0_rm + r0_rre) / 3.0;

                msg += &format!("\nReference0 center: {:.15}, {:.15}", ref0_center.x, ref0_center.y);

                msg += &format!("\nRef0   Avg radius: {r0_ravg:.15}");
                msg += &format!("\nRef0 Start radius: {r0_rs:.15}");
                msg += &format!("\nRef0   Mid radius: {r0_rm:.15}");
                msg += &format!("\nRef0   End radius: {r0_rre:.15}");
                msg += "\n";
            }

            {
                let ref1_center: Vector2D = ref1_calc_arc_center(start, mid, end);

                let r1_rs: f64 = (ref1_center - start).euclidean_norm();
                let r1_rm: f64 = (ref1_center - mid).euclidean_norm();
                let r1_rre: f64 = (ref1_center - end).euclidean_norm();

                let r1_ravg: f64 = (r1_rs + r1_rm + r1_rre) / 3.0;

                msg += &format!("\nReference1 center: {:.15}, {:.15}", ref1_center.x, ref1_center.y);

                msg += &format!("\nRef1   Avg radius: {r1_ravg:.15}");
                msg += &format!("\nRef1 Start radius: {r1_rs:.15}");
                msg += &format!("\nRef1   Mid radius: {r1_rm:.15}");
                msg += &format!("\nRef1   End radius: {r1_rre:.15}");
                msg += "\n";
            }

            panic!("{msg}");
        }
    }
}

/// The reference implementations above are only reached when a case fails; exercise them
/// once so that they stay correct (not part of the C++ test).
#[test]
fn reference_arc_centers_agree_on_a_simple_arc() {
    let start = Vector2D::new(1000.0, 0.0);
    let mid = Vector2D::new(0.0, 1000.0);
    let end = Vector2D::new(-1000.0, 0.0);

    let ref0 = ref0_circle_center_from_3_points(start, mid, end).unwrap();
    assert!(ref0.x.abs() < 1e-9 && ref0.y.abs() < 1e-9);

    let ref1 = ref1_calc_arc_center(start, mid, end);
    assert!(ref1.x.abs() < 1e-9 && ref1.y.abs() < 1e-9);

    assert_eq!(calc_arc_center_d(start, mid, end), Vector2D::new(0.0, 0.0));
    assert!(ref0_circle_center_from_3_points(start, start, end).is_none());
}

/// Port of `BOOST_AUTO_TEST_CASE( TestInterceptsPositiveX )`.
#[test]
fn test_intercepts_positive_x() {
    assert!(!intercepts_positive_x(10.0, 20.0));
    assert!(!intercepts_positive_x(10.0, 120.0));
    assert!(!intercepts_positive_x(10.0, 220.0));
    assert!(!intercepts_positive_x(10.0, 320.0));
    assert!(intercepts_positive_x(20.0, 10.0));
    assert!(intercepts_positive_x(345.0, 15.0));
}

/// Port of `BOOST_AUTO_TEST_CASE( TestInterceptsNegativeX )`.
#[test]
fn test_intercepts_negative_x() {
    assert!(!intercepts_negative_x(10.0, 20.0));
    assert!(!intercepts_negative_x(10.0, 120.0));
    assert!(intercepts_negative_x(10.0, 220.0));
    assert!(intercepts_negative_x(10.0, 320.0));
    assert!(intercepts_negative_x(20.0, 10.0));
    assert!(!intercepts_negative_x(345.0, 15.0));
    assert!(intercepts_negative_x(145.0, 225.0));
}
