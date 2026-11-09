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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_arc.cpp`.

use kicad_geom::geometry::eda_angle::{ANGLE_90, DEGREES_T, EdaAngle};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase, ShapeType};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::geometry::shape_simple::ShapeSimple;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::{Vector2, Vector2D, Vector2I};
use kicad_geom::test_utils::{
    are_points_near_circle, is_box_within_tol, is_close_pct, is_small, is_vec_within_tol, is_within,
    is_within_wrapped, pcb_mm_to_iu,
};
use kicad_geom::trigo::{calc_arc_center, calc_arc_center_d, calc_arc_center_from_angle};

const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn seg(ax: i32, ay: i32, bx: i32, by: i32) -> Seg {
    Seg::new(pt(ax, ay), pt(bx, by))
}

fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
}

/// All properties of an arc (depending on how it's constructed, some of these
/// might be the same as the constructor params)
#[derive(Clone, Copy, Debug)]
struct ArcProperties {
    center_point: Vector2I,
    start_point: Vector2I,
    end_point: Vector2I,
    center_angle: f64,
    start_angle: f64,
    end_angle: f64,
    radius: i32,
    bbox: Box2I,
}

fn props(
    center: Vector2I,
    start: Vector2I,
    end: Vector2I,
    center_angle: f64,
    start_angle: f64,
    end_angle: f64,
    radius: i32,
    bbox: (Vector2I, Vector2I),
) -> ArcProperties {
    ArcProperties {
        center_point: center,
        start_point: start,
        end_point: end,
        center_angle,
        start_angle,
        end_angle,
        radius,
        bbox: Box2I::new(bbox.0, bbox.1),
    }
}

/// Check a `ShapeArc` against a given set of geometric properties
/// (`a_syn_err_iu`: permitted error for synthetic points and dimensions, currently radius and
/// center).
fn check_arc_geom(a_arc: &ShapeArc, a_props: &ArcProperties, a_syn_err_iu: i32) {
    // Angular error - note this can get quite large for very small arcs,
    // as the integral position rounding has a relatively greater effect
    let angle_tol_deg = 2.0;

    // Position error - rounding to nearest integer
    let pos_tol = 1;

    // (the C++ compares the expected start point with itself)
    assert!(is_vec_within_tol(a_props.start_point, a_props.start_point, pos_tol));
    assert!(is_vec_within_tol(a_arc.get_p1(), a_props.end_point, pos_tol), "{a_arc}");
    assert!(is_vec_within_tol(a_arc.get_center(), a_props.center_point, a_syn_err_iu), "{a_arc}");
    assert!(
        is_within_wrapped(a_arc.get_central_angle().as_degrees(), a_props.center_angle, 360.0, angle_tol_deg),
        "{a_arc}: central angle {}",
        a_arc.get_central_angle().as_degrees()
    );
    assert!(
        is_within_wrapped(a_arc.get_start_angle().as_degrees(), a_props.start_angle, 360.0, angle_tol_deg),
        "{a_arc}"
    );
    assert!(
        is_within_wrapped(a_arc.get_end_angle().as_degrees(), a_props.end_angle, 360.0, angle_tol_deg),
        "{a_arc}"
    );
    assert!(is_within(a_arc.get_radius(), a_props.radius as f64, a_syn_err_iu as f64), "{a_arc}");

    // Angle normalization contracts
    assert!(a_arc.get_start_angle().as_degrees() >= 0.0);
    assert!(a_arc.get_start_angle().as_degrees() <= 360.0);
    assert!(a_arc.get_end_angle().as_degrees() >= 0.0);
    assert!(a_arc.get_end_angle().as_degrees() <= 360.0);
    assert!(a_arc.get_central_angle().as_degrees() >= -360.0);
    assert!(a_arc.get_central_angle().as_degrees() <= 360.0);

    // Check the chord agrees
    let chord = a_arc.get_chord();

    assert!(is_vec_within_tol(chord.a, a_props.start_point, pos_tol));
    assert!(is_vec_within_tol(chord.b, a_props.end_point, pos_tol));

    // All arcs are solid
    assert!(a_arc.is_solid());

    assert!(
        is_box_within_tol(&a_arc.bbox(0), &a_props.bbox, pos_tol),
        "{a_arc}: bbox {:?}",
        a_arc.bbox(0)
    );

    // Collisions will be checked elsewhere.
}

/// Check an arcs geometry and other class functions
fn check_arc(a_arc: &ShapeArc, a_props: &ArcProperties, a_syn_err_iu: i32) {
    // Check the original arc
    check_arc_geom(a_arc, a_props, a_syn_err_iu);

    // Test the Clone function (also tests copy-ctor)
    let new_shape = a_arc.clone_shape();

    assert_eq!(new_shape.type_(), ShapeType::Arc);

    let kicad_geom::geometry::shape::ShapeRef::Arc(new_arc) = new_shape.as_shape_ref() else {
        panic!("not an arc");
    };

    // Should have identical geom props
    check_arc_geom(new_arc, a_props, a_syn_err_iu);
}

#[test]
fn null_ctor() {
    let arc = ShapeArc::new();

    assert_eq!(arc.get_width(), 0);

    let null_props = props(pt(0, 0), pt(0, 0), pt(0, 0), 0.0, 0.0, 0.0, 0, (pt(0, 0), pt(0, 0)));

    check_arc(&arc, &null_props, 1);
}

#[test]
fn basic_sme_geom() {
    let cases: Vec<(&str, [Vector2I; 3], i32, ArcProperties)> = vec![
        (
            "S(-100,0), M(0,100), E(100,0)",
            [pt(-100, 0), pt(0, 100), pt(100, 0)],
            0,
            props(pt(0, 0), pt(-100, 0), pt(100, 0), 180.0, 180.0, 0.0, 100, (pt(-100, 0), pt(200, 100))),
        ),
        (
            "S(100,0), M(0,100), E(-100,0) (reversed)",
            [pt(100, 0), pt(0, 100), pt(-100, 0)],
            0,
            props(pt(0, 0), pt(100, 0), pt(-100, 0), -180.0, 0.0, 180.0, 100, (pt(-100, 0), pt(200, 100))),
        ),
        (
            // This data has a midpoint not exactly at the midway point of the arc.
            // This should be corrected by the constructor.
            // The mid point should be at about (-71, -71) for a 270 degree arc, with the
            // bottom right quadrant open.
            "S(100,0), M(-100,0), E(0,100) (bad midpoint)",
            [pt(100, 0), pt(-100, 0), pt(0, 100)],
            0,
            props(pt(0, 0), pt(100, 0), pt(0, 100), -270.0, 0.0, 90.0, 100, (pt(-100, -100), pt(200, 200))),
        ),
    ];

    for (name, g, width, p) in cases {
        let this_arc = ShapeArc::from_start_mid_end(g[0], g[1], g[2], width);
        println!("{name}");
        check_arc(&this_arc, &p, 1);
    }
}

#[test]
fn basic_cpa_geom() {
    let cases: Vec<(&str, (Vector2I, Vector2I, f64), i32, ArcProperties)> = vec![
        (
            "C(0,0) 114 + 360 degree",
            (pt(0, 0), pt(-306451, 687368), 360.0),
            0,
            props(
                pt(0, 0),
                pt(-306451, 687368),
                pt(-306451, 687368),
                360.0,
                113.95929,
                113.95929,
                752587,
                (pt(-752587, -752587), pt(1505174, 1505174)),
            ),
        ),
        (
            "C(0,0) 180 + 360 degree",
            (pt(0, 0), pt(-100, 0), 360.0),
            0,
            props(pt(0, 0), pt(-100, 0), pt(-100, 0), 360.0, 180.0, 180.0, 100, (pt(-100, -100), pt(200, 200))),
        ),
        (
            "C(0,0) 180 + 90 degree",
            (pt(0, 0), pt(-100, 0), 90.0),
            0,
            props(pt(0, 0), pt(-100, 0), pt(0, -100), 90.0, 180.0, 270.0, 100, (pt(-100, -100), pt(100, 100))),
        ),
        (
            "C(100,200)  0 - 30 degree",
            (pt(100, 200), pt(300, 200), -30.0),
            0,
            props(
                pt(100, 200),
                pt(300, 200),
                pt(273, 100), // 200 * sin(30) = 100, 200* cos(30) = 173
                -30.0,
                0.0,
                330.0,
                200,
                (pt(273, 100), pt(27, 100)),
            ),
        ),
        (
            // This is a "fan shape" which includes the top quadrant point,
            // so it exercises the bounding box code (centre and end points
            // do not contain the top quadrant)
            "C(0,0) 30 + 120 degree",
            (pt(0, 0), pt(17320, 10000), 120.0),
            0,
            props(
                pt(0, 0),
                pt(17320, 10000),
                pt(-17320, 10000), // 200 * sin(30) = 100, 200* cos(30) = 173
                120.0,
                30.0,
                150.0,
                20000,
                // bbox defined by: centre, top quadrant point, two endpoints
                (pt(-17320, 10000), pt(17320 * 2, 10000)),
            ),
        ),
        (
            // An arc that covers three quadrant points (L/R, bottom)
            "C(0,0) 150 + 240 degree",
            (pt(0, 0), pt(-17320, 10000), 240.0),
            0,
            props(
                pt(0, 0),
                pt(-17320, 10000),
                pt(17320, 10000),
                240.0,
                150.0,
                30.0,
                20000,
                // bbox defined by: L/R quads, bottom quad and start/end
                (pt(-20000, -20000), pt(40000, 30000)),
            ),
        ),
        (
            // Same as above but reverse direction
            "C(0,0) 30 - 300 degree",
            (pt(0, 0), pt(17320, 10000), -240.0),
            0,
            props(
                pt(0, 0),
                pt(17320, 10000),
                pt(-17320, 10000),
                -240.0,
                30.0,
                150.0,
                20000,
                // bbox defined by: L/R quads, bottom quad and start/end
                (pt(-20000, -20000), pt(40000, 30000)),
            ),
        ),
    ];

    for (name, g, width, p) in cases {
        let this_arc = ShapeArc::from_center_start_angle(g.0, g.1, deg(g.2), width);
        println!("{name}");
        check_arc(&this_arc, &p, 1);
    }
}

#[test]
fn basic_ttr_geom() {
    let cases: Vec<(&str, (Seg, Seg, i32), i32, ArcProperties)> = vec![
        (
            "90 degree segments intersecting",
            (seg(0, 0, 0, 1000), seg(0, 0, 1000, 0), 1000),
            0,
            props(
                pt(1000, 1000),
                pt(0, 1000), //start on first segment
                pt(1000, 0), //end on second segment
                90.0,        //positive angle due to start/end
                180.0,
                270.0,
                1000,
                (pt(0, 0), pt(1000, 1000)),
            ),
        ),
        (
            "45 degree segments intersecting",
            (seg(0, 0, 0, 1000), seg(0, 0, 1000, 1000), 1000),
            0,
            props(
                pt(1000, 2414),
                pt(0, 2414),    //start on first segment
                pt(1707, 1707), //end on second segment
                135.0,          //positive angle due to start/end
                180.0,
                315.0,
                1000,
                (pt(0, 1414), pt(1707, 1000)),
            ),
        ),
        (
            "135 degree segments intersecting",
            (seg(0, 0, 0, 1000), seg(0, 0, 1000, -1000), 1000),
            0,
            props(
                pt(1000, 414),
                pt(0, 414),    //start on first segment ( radius * tan(45 /2) )
                pt(293, -293), //end on second segment (radius * 1-cos(45)) )
                45.0,          //positive angle due to start/end
                180.0,
                225.0,
                1000,
                (pt(0, -293), pt(293, 707)),
            ),
        ),
    ];

    for (name, g, width, c_props) in cases {
        for test_case in 0..8 {
            let mut seg1 = g.0;
            let mut seg2 = g.1;
            let mut p = c_props;

            if test_case > 3 {
                //Swap input segments.
                seg1 = g.1;
                seg2 = g.0;

                //The result should swap start and end points and invert the angles:
                p.end_point = c_props.start_point;
                p.start_point = c_props.end_point;
                p.start_angle = c_props.end_angle;
                p.end_angle = c_props.start_angle;
                p.center_angle = -c_props.center_angle;
            }

            //Test all combinations of start and end points for the segments
            if (test_case % 4) == 1 || (test_case % 4) == 3 {
                //Swap start and end points for seg1
                std::mem::swap(&mut seg1.a, &mut seg1.b);
            }

            if (test_case % 4) == 2 || (test_case % 4) == 3 {
                //Swap start and end points for seg2
                std::mem::swap(&mut seg2.a, &mut seg2.b);
            }

            let this_arc = ShapeArc::from_tangent_segments(&seg1, &seg2, g.2, width);

            println!("{name} {test_case}");

            // Error of 4 IU permitted for the center and radius calculation
            check_arc(&this_arc, &p, kicad_geom::geometry::shape::MIN_PRECISION_IU);
        }
    }
}

#[test]
fn basic_sec_geom() {
    let cases: Vec<(&str, (Vector2I, Vector2I, Vector2I), bool, Vector2I)> = vec![
        ("180 deg, clockwise", (pt(100, 0), pt(0, 0), pt(50, 0)), true, pt(50, -50)),
        ("180 deg, anticlockwise", (pt(100, 0), pt(0, 0), pt(50, 0)), false, pt(50, 50)),
        ("180 deg flipped, clockwise", (pt(0, 0), pt(100, 0), pt(50, 0)), true, pt(50, 50)),
        ("180 deg flipped, anticlockwise", (pt(0, 0), pt(100, 0), pt(50, 0)), false, pt(50, -50)),
        ("90 deg, clockwise", (pt(-100, 0), pt(0, 100), pt(0, 0)), true, pt(-71, 71)),
        ("90 deg, anticlockwise", (pt(-100, 0), pt(0, 100), pt(0, 0)), false, pt(71, -71)),
    ];

    for (name, g, cw, exp_mid) in cases {
        let mut this_arc = ShapeArc::new();
        this_arc.construct_from_start_end_center(g.0, g.1, g.2, cw, 0.0);

        assert_eq!(this_arc.get_arc_mid(), exp_mid, "{name}");
    }
}

#[test]
fn collide_circle() {
    let cases: Vec<(&str, [Vector2I; 3], i32, Vector2I, i32, bool, i32)> = vec![(
        " Issue 20336, large arc",
        [pt(183000000, 65710001), pt(150496913, 147587363), pt(116291153, 66406583)],
        2000000 / 2,
        pt(116300000, 133100000),
        300000,
        true,
        53319,
    )];

    for (name, g, arc_clearance, circle_center, circle_radius, exp_result, exp_distance) in cases {
        let mut arc = ShapeArc::from_start_mid_end(g[0], g[1], g[2], 0);
        let circle = ShapeCircle::new(circle_center, circle_radius);

        // Test a zero width arc (distance should equal the clearance)
        {
            let mut dist = -1;
            assert_eq!(arc.collide(&circle, arc_clearance, Some(&mut dist), None), exp_result, "{name}");
            assert_eq!(dist, exp_distance, "{name}");
        }

        // Test by changing the width of the arc (distance should equal zero)
        {
            let mut dist = -1;
            arc.set_width(arc_clearance * 2);
            assert_eq!(arc.collide(&circle, 0, Some(&mut dist), None), exp_result, "{name}");

            if exp_result {
                assert_eq!(dist, 0, "{name}");
            } else {
                assert_eq!(dist, -1, "{name}");
            }
        }
    }
}

#[test]
fn collide_pt() {
    let a = |c: Vector2I, s: Vector2I, ang: f64| (c, s, ang);
    let cases: Vec<(&str, (Vector2I, Vector2I, f64), i32, Vector2I, bool, i32)> = vec![
        (" 270deg, 0 cl, 0   deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(100, 0), true, 0),
        (" 270deg, 0 cl, 90  deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(0, 100), true, 0),
        (" 270deg, 0 cl, 180 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(-100, 0), true, 0),
        (" 270deg, 0 cl, 270 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(0, -100), true, 0),
        (" 270deg, 0 cl, 45  deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(71, 71), true, 0),
        (" 270deg, 0 cl, -45 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, pt(71, -71), false, -1),
        ("-270deg, 0 cl, 0   deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(100, 0), true, 0),
        ("-270deg, 0 cl, 90  deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(0, 100), true, 0),
        ("-270deg, 0 cl, 180 deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(-100, 0), true, 0),
        ("-270deg, 0 cl, 270 deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(0, -100), true, 0),
        ("-270deg, 0 cl, 45  deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(71, 71), false, -1),
        ("-270deg, 0 cl, -45 deg    ", a(pt(0, 0), pt(100, 0), -270.0), 0, pt(71, -71), true, 0),
        (" 270deg, 5 cl, 0   deg, 5 pos X", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(105, 0), true, 5),
        (" 270deg, 5 cl, 0  deg, 5 pos Y", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(100, -5), true, 5),
        (" 270deg, 5 cl, 90  deg, 5 pos", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(0, 105), true, 5),
        (" 270deg, 5 cl, 180 deg, 5 pos", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(-105, 0), true, 5),
        (" 270deg, 5 cl, 270 deg, 5 pos", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(0, -105), true, 5),
        (" 270deg, 5 cl, 0   deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(105, 0), true, 5),
        (" 270deg, 5 cl, 90  deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(0, 105), true, 5),
        (" 270deg, 5 cl, 180 deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(-105, 0), true, 5),
        (" 270deg, 5 cl, 270 deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(0, -105), true, 5),
        (" 270deg, 5 cl, 45  deg, 5 pos", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(74, 75), true, 5), // 74.246, -74.246
        (" 270deg, 5 cl, -45 deg, 5 pos", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(74, -75), false, -1), //74.246, -74.246
        (" 270deg, 5 cl, 45  deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(67, 67), true, 5), // 67.17, 67.17
        (" 270deg, 5 cl, -45 deg, 5 neg", a(pt(0, 0), pt(100, 0), 270.0), 5, pt(67, -67), false, -1), // 67.17, -67.17
        (" 270deg, 4 cl, 0   deg pos", a(pt(0, 0), pt(100, 0), 270.0), 4, pt(105, 0), false, -1),
        (" 270deg, 4 cl, 90  deg pos", a(pt(0, 0), pt(100, 0), 270.0), 4, pt(0, 105), false, -1),
        (" 270deg, 4 cl, 180 deg pos", a(pt(0, 0), pt(100, 0), 270.0), 4, pt(-105, 0), false, -1),
        (" 270deg, 4 cl, 270 deg pos", a(pt(0, 0), pt(100, 0), 270.0), 4, pt(0, -105), false, -1),
        ("  90deg, 0 cl,   0 deg    ", a(pt(0, 0), pt(71, -71), 90.0), 0, pt(71, -71), true, 0),
        ("  90deg, 0 cl,  45 deg    ", a(pt(0, 0), pt(71, -71), 90.0), 0, pt(100, 0), true, 0),
        ("  90deg, 0 cl,  90 deg    ", a(pt(0, 0), pt(71, -71), 90.0), 0, pt(71, 71), true, 0),
        ("  90deg, 0 cl, 135 deg    ", a(pt(0, 0), pt(71, -71), 90.0), 0, pt(0, -100), false, -1),
        ("  90deg, 0 cl, -45 deg    ", a(pt(0, 0), pt(71, -71), 90.0), 0, pt(0, 100), false, -1),
        (" -90deg, 0 cl,   0 deg    ", a(pt(0, 0), pt(71, 71), -90.0), 0, pt(71, -71), true, 0),
        (" -90deg, 0 cl,  45 deg    ", a(pt(0, 0), pt(71, 71), -90.0), 0, pt(100, 0), true, 0),
        (" -90deg, 0 cl,  90 deg    ", a(pt(0, 0), pt(71, 71), -90.0), 0, pt(71, 71), true, 0),
        (" -90deg, 0 cl, 135 deg    ", a(pt(0, 0), pt(71, 71), -90.0), 0, pt(0, -100), false, -1),
        (" -90deg, 0 cl, -45 deg    ", a(pt(0, 0), pt(71, 71), -90.0), 0, pt(0, 100), false, -1),
        (
            "issue 11358 collide",
            a(pt(119888000, 60452000), pt(120904000, 60452000), 360.0),
            0,
            pt(120395500, 59571830),
            true,
            0,
        ),
        (
            "issue 11358 dist",
            a(pt(119888000, 60452000), pt(120904000, 60452000), 360.0),
            100,
            pt(118872050, 60452000),
            true,
            50,
        ),
    ];

    for (name, g, arc_clearance, point, exp_result, exp_distance) in cases {
        let mut arc = ShapeArc::from_center_start_angle(g.0, g.1, deg(g.2), 0);

        // Test a zero width arc (distance should equal the clearance)
        {
            let mut dist = -1;
            assert_eq!(arc.collide_point(point, arc_clearance, Some(&mut dist), None), exp_result, "{name}");
            assert_eq!(dist, exp_distance, "{name}");
        }

        // Test by changing the width of the arc (distance should equal zero)
        {
            let mut dist = -1;
            arc.set_width(arc_clearance * 2);
            assert_eq!(arc.collide_point(point, 0, Some(&mut dist), None), exp_result, "{name}");

            if exp_result {
                assert_eq!(dist, 0, "{name}");
            } else {
                assert_eq!(dist, -1, "{name}");
            }
        }
    }
}

#[test]
fn collide_seg() {
    let a = |c: Vector2I, s: Vector2I, ang: f64| (c, s, ang);
    let cases: Vec<(&str, (Vector2I, Vector2I, f64), i32, Seg, bool, i32, Vector2I)> = vec![
        ("0   deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(100, 0, 50, 0), true, 0, pt(100, 0)),
        ("90  deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(0, 100, 0, 50), true, 0, pt(0, 100)),
        ("180 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(-100, 0, -50, 0), true, 0, pt(-100, 0)),
        ("270 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(0, -100, 0, -50), true, 0, pt(0, -100)),
        ("45  deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(71, 71, 35, 35), true, 0, pt(70, 70)),
        ("-45 deg    ", a(pt(0, 0), pt(100, 0), 270.0), 0, seg(71, -71, 35, -35), false, -1, pt(0, 0)),
        ("seg inside arc start", a(pt(0, 0), pt(71, -71), 90.0), 10, seg(90, 0, -35, 0), true, 10, pt(100, 0)),
        ("seg inside arc end", a(pt(0, 0), pt(71, -71), 90.0), 10, seg(-35, 0, 90, 0), true, 10, pt(100, 0)),
        (
            "large diameter arc",
            a(pt(172367922, 82282076), pt(162530000, 92120000), -45.0),
            433300,
            seg(162096732, 92331236, 162096732, 78253268),
            true,
            433268,
            pt(162530000, 92120000),
        ),
        (
            "upside down collide",
            a(pt(26250000, 16520000), pt(28360000, 16520000), 90.0),
            0,
            seg(27545249, 18303444, 27545249, 18114500),
            true,
            0,
            pt(27545249, 18185662),
        ),
    ];

    for (name, g, arc_clearance, s, exp_result, exp_distance, collide_point) in cases {
        let mut arc = ShapeArc::from_center_start_angle(g.0, g.1, deg(g.2), 0);

        // Test a zero width arc (distance should equal the clearance)
        {
            let mut dist = -1;
            assert_eq!(arc.collide_seg(&s, arc_clearance, Some(&mut dist), None), exp_result, "{name}");
            assert_eq!(dist, exp_distance, "{name}");
        }

        // Test by changing the width of the arc (distance should equal zero)
        {
            let mut dist = -1;
            arc.set_width(arc_clearance * 2);
            assert_eq!(arc.collide_seg(&s, 0, Some(&mut dist), None), exp_result, "{name}");

            if exp_result {
                assert_eq!(dist, 0, "{name}");
            } else {
                assert_eq!(dist, -1, "{name}");
            }
        }

        // Test Collide Point
        {
            let mut location = Vector2I::default();
            let mut dist = -1;

            if exp_result {
                arc.collide_seg(&s, arc_clearance, Some(&mut dist), Some(&mut location));
                assert_eq!(location, collide_point, "{name}");
            }
        }
    }
}

/// `ARC_DATA_MM`: coordinates and dimensions in millimeters
#[derive(Clone, Copy)]
struct ArcDataMm {
    center_x: f64,
    center_y: f64,
    start_x: f64,
    start_y: f64,
    center_angle: f64,
    width: f64,
}

impl ArcDataMm {
    const fn new(cx: f64, cy: f64, sx: f64, sy: f64, a: f64, w: f64) -> Self {
        ArcDataMm {
            center_x: cx,
            center_y: cy,
            start_x: sx,
            start_y: sy,
            center_angle: a,
            width: w,
        }
    }

    fn generate_arc(&self) -> ShapeArc {
        // SHAPE_ARC( VECTOR2D( ... ), VECTOR2D( ... ), ... ): VECTOR2D to VECTOR2I conversion
        let c = Vector2D::new(pcb_mm_to_iu(self.center_x) as f64, pcb_mm_to_iu(self.center_y) as f64);
        let s = Vector2D::new(pcb_mm_to_iu(self.start_x) as f64, pcb_mm_to_iu(self.start_y) as f64);

        ShapeArc::from_center_start_angle(
            Vector2I::from_vec(c),
            Vector2I::from_vec(s),
            deg(self.center_angle),
            pcb_mm_to_iu(self.width),
        )
    }
}

fn arc_arc_collide_cases() -> Vec<(&'static str, ArcDataMm, ArcDataMm, f64, bool)> {
    let a = ArcDataMm::new;
    vec![
        (
            "case 1: No intersection",
            a(73.843527, 74.355869, 71.713528, 72.965869, -76.36664803, 0.2),
            a(71.236473, 74.704131, 73.366472, 76.094131, -76.36664803, 0.2),
            0.0,
            false,
        ),
        (
            "case 2: No intersection",
            a(82.542335, 74.825975, 80.413528, 73.435869, -76.4, 0.2),
            a(76.491192, 73.839894, 78.619999, 75.23, -76.4, 0.2),
            0.0,
            false,
        ),
        (
            "case 3: No intersection",
            a(89.318807, 74.810106, 87.19, 73.42, -76.4, 0.2),
            a(87.045667, 74.632941, 88.826472, 75.794131, -267.9, 0.2),
            0.0,
            false,
        ),
        (
            "case 4: Co-centered not intersecting",
            a(94.665667, 73.772941, 96.446472, 74.934131, -267.9, 0.2),
            a(94.665667, 73.772941, 93.6551, 73.025482, -255.5, 0.2),
            0.0,
            false,
        ),
        (
            "case 5: Not intersecting, but end points very close",
            a(72.915251, 80.493054, 73.570159, 81.257692, -260.5, 0.2),
            a(73.063537, 82.295989, 71.968628, 81.581351, -255.5, 0.2),
            0.0,
            false,
        ),
        (
            "case 6: Coincident centers, colliding due to arc thickness",
            a(79.279991, 80.67988, 80.3749, 81.394518, -255.5, 0.3),
            a(79.279991, 80.67988, 80.3749, 81.694518, -255.5, 0.3),
            0.0,
            true,
        ),
        (
            "case 7: Single intersection",
            a(88.495265, 81.766089, 90.090174, 82.867869, -255.5, 0.2),
            a(86.995265, 81.387966, 89.090174, 82.876887, -255.5, 0.2),
            0.0,
            true,
        ),
        (
            "case 8: Double intersection",
            a(96.149734, 81.792126, 94.99, 83.37, -347.2, 0.2),
            a(94.857156, 81.240589, 95.91, 83.9, -288.5, 0.2),
            0.0,
            true,
        ),
        (
            "case 9: Endpoints within arc width",
            a(72.915251, 86.493054, 73.970159, 87.257692, -260.5, 0.2),
            a(73.063537, 88.295989, 71.968628, 87.581351, -255.5, 0.2),
            0.0,
            true,
        ),
        (
            "case 10: Endpoints close, outside, no collision",
            a(78.915251, 86.393054, 79.970159, 87.157692, 99.5, 0.2),
            a(79.063537, 88.295989, 77.968628, 87.581351, -255.5, 0.2),
            0.0,
            false,
        ),
        (
            "case 11: Endpoints close, inside, collision due to arc width",
            a(85.915251, 86.993054, 86.970159, 87.757692, 99.5, 0.2),
            a(86.063537, 88.295989, 84.968628, 87.581351, -255.5, 0.2),
            0.0,
            true,
        ),
        (
            "case 12: Simulated differential pair length-tuning",
            a(94.6551, 88.296, 95.6551, 88.296, 90.0, 0.1),
            a(94.6551, 88.296, 95.8551, 88.296, 90.0, 0.1),
            0.1,
            false,
        ),
        (
            "case 13: One arc fully enclosed in other, non-concentric",
            a(73.77532, 93.413654, 75.70532, 93.883054, 60.0, 0.1),
            a(73.86532, 93.393054, 75.86532, 93.393054, 90.0, 0.3),
            0.0,
            true,
        ),
        (
            "case 14: One arc fully enclosed in other, concentric",
            a(79.87532, 93.413654, 81.64532, 94.113054, 60.0, 0.1),
            a(79.87532, 93.413654, 81.86532, 93.393054, 90.0, 0.3),
            0.0,
            true,
        ),
        (
            "case 15: Arcs separated by clearance",
            a(303.7615, 149.9252, 303.695968, 149.925237, 90.0262, 0.065),
            a(303.6345, 149.2637, 303.634523, 148.85619, 89.9957, 0.065),
            0.15,
            false,
        ),
    ]
}

/// `CollideArc`: the arc-to-arc, arc-to-chain and chain-to-arc collisions.  The chain-to-chain
/// part is [`collide_arc_chain_to_chain`].
#[test]
fn collide_arc() {
    for (name, d1, d2, cl, exp_result) in arc_arc_collide_cases() {
        let arc1 = d1.generate_arc();
        let arc2 = d2.generate_arc();
        let mut arc1_slc = ShapeLineChain::from_arc(&d1.generate_arc(), false, None);
        arc1_slc.set_width(0);
        let mut arc2_slc = ShapeLineChain::from_arc(&d2.generate_arc(), false, None);
        arc2_slc.set_width(0);

        let mut actual = 0;
        let mut location = Vector2I::default();

        let arc1_sh: &dyn Shape = &arc1;
        let arc2_sh: &dyn Shape = &arc2;
        let arc1_slc_sh: &dyn Shape = &arc1_slc;
        let arc2_slc_sh: &dyn Shape = &arc2_slc;

        let result_arc_to_arc =
            arc1_sh.collide(arc2_sh, pcb_mm_to_iu(cl), Some(&mut actual), Some(&mut location));

        // For arc to chain collisions, we need to re-calculate the clearances because the
        // SHAPE_LINE_CHAIN is zero width
        let mut clearance = pcb_mm_to_iu(cl) + (arc2.get_width() / 2);

        let result_arc_to_chain = arc1_sh.collide(arc2_slc_sh, clearance, Some(&mut actual), Some(&mut location));

        clearance = pcb_mm_to_iu(cl) + (arc1.get_width() / 2);
        let result_chain_to_arc = arc1_slc_sh.collide(arc2_sh, clearance, Some(&mut actual), Some(&mut location));

        assert_eq!(result_arc_to_arc, exp_result, "{name}: arc to arc");
        assert_eq!(result_arc_to_chain, exp_result, "{name}: arc to chain");
        assert_eq!(result_chain_to_arc, exp_result, "{name}: chain to arc");
    }
}

/// The chain-to-chain part of `CollideArc`.
#[test]
#[ignore = "needs G5: SHAPE_LINE_CHAIN vs SHAPE_LINE_CHAIN collisions"]
fn collide_arc_chain_to_chain() {
    for (name, d1, d2, _cl, exp_result) in arc_arc_collide_cases() {
        let arc1 = d1.generate_arc();
        let arc2 = d2.generate_arc();
        let arc1_slc = ShapeLineChain::from_arc(&arc1, false, None);
        let arc2_slc = ShapeLineChain::from_arc(&arc2, false, None);

        let mut actual = 0;
        let mut location = Vector2I::default();
        let clearance = (arc1.get_width() / 2) + (arc2.get_width() / 2);
        let result_chain_to_chain = arc1_slc.collide(&arc2_slc, clearance, Some(&mut actual), Some(&mut location));

        assert_eq!(result_chain_to_chain, exp_result, "{name}: chain to chain");
    }
}

#[test]
fn collide_arc_to_shape_line_chain() {
    let arc = ShapeArc::from_start_mid_end(
        pt(206000000, 140110000),
        pt(201574617, 139229737),
        pt(197822958, 136722959),
        250000,
    );
    let lc = ShapeLineChain::from_points(
        &[
            pt(159600000, 142500000),
            pt(159600000, 142600000),
            pt(166400000, 135800000),
            pt(166400000, 111600000),
            pt(190576804, 111600000),
            pt(192242284, 113265480),
            pt(192255720, 113265480),
            pt(203682188, 124691948),
            pt(203682188, 140332188),
            pt(206000000, 142650000),
        ],
        false,
    );

    let arc_sh: &dyn Shape = &arc;
    let lc_sh: &dyn Shape = &lc;

    assert!(arc_sh.collide(&lc, 100000, None, None));
    assert!(lc_sh.collide(&arc, 100000, None, None));

    let s = Seg::new(pt(203682188, 124691948), pt(203682188, 140332188));
    assert!(arc.collide_seg(&s, 0, None, None));
}

#[test]
fn collide_arc_to_closed_chain_matches_segment_scan() {
    let semicircle = ShapeArc::from_start_mid_end(pt(-100, 0), pt(0, 100), pt(100, 0), 0);
    let major = ShapeArc::from_center_start_angle(pt(0, 0), pt(-87, -50), deg(300.0), 0);

    assert!(major.get_central_angle().as_degrees() > 180.0);
    assert!(major.bbox(150).left() > -238);

    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| {
        ShapeSimple::from_line_chain(&ShapeLineChain::from_points(
            &[pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)],
            true,
        ))
    };

    // SHAPE_SIMPLE routes through the SHAPE_LINE_CHAIN_BASE overload that carries the box reject
    let cases: Vec<(ShapeArc, ShapeSimple, i32, bool)> = vec![
        (ShapeArc::with_width(&semicircle, 4), rect(-10, 102, 10, 110), 0, true),
        (semicircle.clone(), rect(-10, 105, 10, 110), 5, false),
        (major.clone(), rect(-239, -10, -238, 10), 150, true),
    ];

    for (arc, chain, clearance, collides) in cases {
        assert!(!chain.point_inside(arc.get_p0(), 0, false));

        let mut expected_actual = i32::MAX;
        let mut expected_location = Vector2I::default();

        for i in 0..chain.get_segment_count() {
            let mut segment_actual = 0;
            let mut segment_location = Vector2I::default();

            if arc.collide_seg(
                &chain.get_segment(i as i32),
                clearance,
                Some(&mut segment_actual),
                Some(&mut segment_location),
            ) && segment_actual < expected_actual
            {
                expected_actual = segment_actual;
                expected_location = segment_location;
            }
        }

        let expected = expected_actual == 0 || expected_actual < clearance;
        assert_eq!(expected, collides);

        let mut actual = 0;
        let mut location = Vector2I::default();
        let collided = (&arc as &dyn Shape).collide(&chain, clearance, Some(&mut actual), Some(&mut location));

        assert_eq!(collided, expected);

        if expected {
            assert_eq!(actual, expected_actual);
            assert_eq!(location, expected_location);
        }
    }
}

#[test]
#[ignore = "needs TransformArcToPolygon (convert_basic_shapes_to_polygon), SHAPE_POLY_SET triangulation and collisions (G4/G5)"]
fn collide_arc_to_polygon_approximation() {
    // SHAPE_ARC arc( VECTOR2I( 73843527, 74355869 ), VECTOR2I( 71713528, 72965869 ),
    //                EDA_ANGLE( -76.36664803, DEGREES_T ), 1000000 );
    // Create a polyset approximation from the arc - error outside (simulating the zone filler),
    // make a reasonably large rectangular outline around the arc shape, create a synthetic
    // "zone fill" polygon with the approximation as a hole and check
    //   zoneFill.Collide( &arc, clearance + epsilon ) == true
    //   zoneFill.Collide( &arc, clearance - epsilon ) == false
    panic!("not ported yet");
}

/// Predicate for checking a polyline has all the points on (near) a circle of
/// given centre and radius
fn are_polyline_end_points_near_circle(
    a_polyline: &ShapeLineChain,
    a_centre: Vector2I,
    a_rad: i32,
    a_tolerance: i32,
) -> bool {
    let points: Vec<Vector2I> = (0..a_polyline.point_count()).map(|i| a_polyline.c_point(i)).collect();

    are_points_near_circle(&points, a_centre, a_rad, a_tolerance)
}

/// Predicate for checking a polyline has all the segment mid points on
/// (near) a circle of given centre and radius
fn are_polyline_mid_points_near_circle(
    a_polyline: &ShapeLineChain,
    a_centre: Vector2I,
    a_rad: i32,
    a_tolerance: i32,
) -> bool {
    let points: Vec<Vector2I> = (0..a_polyline.point_count() - 1)
        .map(|i| (a_polyline.c_point(i) + a_polyline.c_point(i + 1)) / 2)
        .collect();

    are_points_near_circle(&points, a_centre, a_rad, a_tolerance)
}

#[test]
fn arc_to_polyline() {
    let cases: Vec<(&str, (Vector2I, Vector2I, f64))> = vec![
        ("Zero rad", (pt(0, 0), pt(0, 0), 180.0)),
        ("Semicircle", (pt(0, 0), pt(-1000000, 0), 180.0)),
        // check that very small circles don't fall apart and that reverse angles
        // work too
        ("Extremely small semicircle", (pt(0, 0), pt(-1000, 0), -180.0)),
        // Make sure it doesn't only work for "easy" angles
        ("Non-round geometry", (pt(0, 0), pt(1234567, 0), 42.22)),
    ];

    for (name, g) in cases {
        let width = 0;

        // Note: do not expect accuracies around 1 to work.  We use integers internally so we're
        // liable to rounding errors.  In PCBNew accuracy defaults to 5000 and we don't recommend
        // anything lower than 1000 (for performance reasons).
        let accuracy = 100;
        let epsilon = 1;

        let this_arc = ShapeArc::from_center_start_angle(g.0, g.1, deg(g.2), width);

        let chain = this_arc.convert_to_polyline(accuracy);

        println!("{name}: Polyline has {} points", chain.point_count());

        // Start point (exactly) where expected
        assert_eq!(chain.c_point(0), g.1, "{name}");

        // End point (exactly) where expected
        assert_eq!(chain.c_last_point(), this_arc.get_p1(), "{name}");

        let radius = (g.0 - g.1).euclidean_norm();

        // Other points within accuracy + epsilon (for rounding) of where they should be
        assert!(are_polyline_end_points_near_circle(&chain, g.0, radius, accuracy + epsilon), "{name}");
        assert!(are_polyline_mid_points_near_circle(&chain, g.0, radius, accuracy + epsilon), "{name}");
    }
}

/// Test that TransformArcToPolygon handles shallow arcs (where the mid-point is nearly
/// collinear with start and end points) without producing invalid geometry due to
/// integer overflow from extremely large radii.
///
/// This is a regression test for issue #22475 where shallow-radius arc segments
/// caused polygon pour rendering artifacts.
#[test]
#[ignore = "needs TransformArcToPolygon (convert_basic_shapes_to_polygon, later package)"]
fn transform_shallow_arc_to_polygon() {
    // start( 0, 0 ), end( 10000000, 0 ), mid( 5000000, 5 ), width 250000, aError 5000,
    // ERROR_INSIDE: at least one closed outline with >= 3 points, bbox width <= end.x + 2 * width,
    // bbox height <= 2 * width + 100
    panic!("not ported yet");
}

/// Test that arcs with extremely large radii (greater than INT_MAX/2) are
/// properly converted to line segments.
#[test]
#[ignore = "needs TransformArcToPolygon (convert_basic_shapes_to_polygon, later package)"]
fn transform_very_shallow_arc_to_polygon() {
    // start( 0, 0 ), end( 50000000, 0 ), mid( 25000000, 1 ), width 250000, aError 5000:
    // at least one closed outline with >= 3 points
    panic!("not ported yet");
}

/// Test arc with values similar to problematic arcs in issue #22475 board.
/// These arcs have distToMid around 12-13µm with radius around 24mm.
#[test]
#[ignore = "needs TransformArcToPolygon (convert_basic_shapes_to_polygon, later package)"]
fn transform_issue22475_arc_to_polygon() {
    // start( 0, 0 ), end( 1620000, 0 ), mid( 810000, 12960 ), width 200000, aError 5000:
    // closed outline, >= 3 points, bbox width <= 3000000, bbox height <= 1000000
    panic!("not ported yet");
}

/// Test that SHAPE_ARC::Collide handles arcs with near-INT_MAX radius without crashing.
/// Reproduces a crash during PADS ASCII import where a near-collinear arc produced a
/// radius exceeding INT_MAX, overflowing the CIRCLE(int) constructor and KiROUND.
#[test]
fn collide_nearly_flat_arc_does_not_overflow() {
    // Values from the core dump: a nearly-flat arc with enormous radius
    let start = pt(68208364, -8000);
    let mid = pt(771364, 500000);
    let end = pt(35224335, -7999);
    let width = 1270000;

    let arc = ShapeArc::from_start_mid_end(start, mid, end, width);

    // Radius should be near or above INT_MAX/2, triggering the segment fallback
    assert!(arc.get_radius() >= i32::MAX as f64 / 2.0);

    // Point near the arc endpoints.  Must not crash.
    let test_pt = pt(35224298, -5381);
    let mut actual = 0;
    let mut location = Vector2I::default();

    arc.collide_point(test_pt, 635000, Some(&mut actual), Some(&mut location));

    // Segment near the arc.  Must not crash.
    let test_seg = Seg::new(pt(35224298, -5381), pt(35696364, -32988651));
    arc.collide_seg(&test_seg, 635000, Some(&mut actual), Some(&mut location));
}

/// Three near-coincident points must not fabricate a multi-metre circumcircle.
///
///   (arc (start 135.674 84.576744)
///        (mid   135.673999 84.576744)
///        (end   135.673998 84.576744) ...)
///
/// Previously CalcArcCenter() slope-epsilon fallback for colinear inputs
/// produced a centre ~2 m away and a radius ~2 m, and the PNS drag preview
/// drew a board-spanning ghost circle through the three coincident points.
#[test]
fn degenerate_arc_coincident_points() {
    let arc = ShapeArc::from_start_mid_end(
        pt(135674000, 84576744),
        pt(135673999, 84576744),
        pt(135673998, 84576744),
        100000,
    );

    assert!(arc.get_radius() < 10.0);

    let poly = arc.convert_to_polyline(ShapeArc::default_accuracy_for_pcb());
    assert!(poly.bbox(0).width() < 1000); // < 1 µm
    assert!(poly.bbox(0).height() < 1000);
    assert!(arc.get_length() < 1000.0);
}

// Collinear points from the reported board.  The coincident start/mid sends CalcArcCenter() to
// the chord midpoint, which makes start and end antipodal and the raw sweep a clean half turn
#[test]
fn collinear_arc_sweep_is_not_a_full_turn() {
    let arc = ShapeArc::from_start_mid_end(pt(2275000, 3123714), pt(2275000, 3123715), pt(2275000, 3123720), 127000);

    assert!(arc.get_central_angle().as_degrees().abs() < 1.0);

    // The 6 nm chord is the whole run; a fabricated sweep inflates this without bound
    assert!(is_close_pct(arc.get_length(), 6.0, 1.0));

    let poly = arc.convert_to_polyline(ShapeArc::default_accuracy_for_pcb());
    assert!(poly.bbox(0).width() < 10);
    assert!(poly.bbox(0).height() < 10);
}

// The guard must not catch major arcs, which are the only users of the full-turn correction
#[test]
fn curved_arcs_keep_their_sweep() {
    let quarter = ShapeArc::from_start_mid_end(pt(1000000, 0), pt(707107, 707107), pt(0, 1000000), 127000);
    assert!(is_close_pct(quarter.get_central_angle().as_degrees(), 90.0, 0.01));

    let major = ShapeArc::from_start_mid_end(pt(1000000, 0), pt(-1000000, 0), pt(0, -1000000), 127000);
    assert!(is_close_pct(major.get_central_angle().as_degrees(), 270.0, 0.01));
}

// Coincident start/mid plus a distant end previously produced a center far off from the inputs
#[test]
fn calc_arc_center_two_coincident_start_mid() {
    let start = Vector2D::new(0.0, 0.0);
    let mid = Vector2D::new(0.0, 0.0);
    let end = Vector2D::new(1000.0, 0.0);

    let center = calc_arc_center_d(start, mid, end);
    assert!(is_close_pct(center.x, 500.0, 1e-9));
    assert!(is_small(center.y, 1e-9));
}

#[test]
fn calc_arc_center_two_coincident_mid_end() {
    let start = Vector2D::new(-1000.0, 0.0);
    let mid = Vector2D::new(0.0, 0.0);
    let end = Vector2D::new(0.0, 0.0);

    let center = calc_arc_center_d(start, mid, end);
    assert!(is_close_pct(center.x, -500.0, 1e-9));
    assert!(is_small(center.y, 1e-9));
}

#[test]
fn calc_arc_center_two_coincident_start_end() {
    // Coincident start/end with a distinct mid is a 360-degree arc, center is midpoint to mid
    let start = Vector2D::new(0.0, 0.0);
    let mid = Vector2D::new(1000.0, 0.0);
    let end = Vector2D::new(0.0, 0.0);

    let center = calc_arc_center_d(start, mid, end);
    assert!(is_close_pct(center.x, 500.0, 1e-9));
    assert!(is_small(center.y, 1e-9));
}

// Three near-coincident points collapse to the centroid via the bbox guard, not the pairwise guard
#[test]
fn calc_arc_center_three_near_coincident() {
    let start = Vector2D::new(100.0, 200.0);
    let mid = Vector2D::new(101.0, 200.0);
    let end = Vector2D::new(100.0, 201.0);

    let center = calc_arc_center_d(start, mid, end);
    assert!(is_close_pct(center.x, (start.x + mid.x + end.x) / 3.0, 1e-9));
    assert!(is_close_pct(center.y, (start.y + mid.y + end.y) / 3.0, 1e-9));
}

// A thin arc must not trip the coincident-point guards despite its short chord and small sagitta
#[test]
fn calc_arc_center_thin_arc_not_degenerate() {
    let start = Vector2D::new(0.0, 0.0);
    let mid = Vector2D::new(10.0, 1.0);
    let end = Vector2D::new(20.0, 0.0);

    let center = calc_arc_center_d(start, mid, end);

    let rs = (center - start).euclidean_norm();
    let rm = (center - mid).euclidean_norm();
    let re = (center - end).euclidean_norm();

    // Snapping to the 10 nm grid moves the center off the exact (10, -49.5) by half an IU
    assert!(is_small(rs - rm, 1.0));
    assert!(is_small(rm - re, 1.0));

    // True circumradius is 50.5 IU; a wrongly-triggered midpoint guard would give ~10 IU
    assert!(rs > 40.0);
}

// A few-IU arc rounds its own mid point off the true circle, but the three points still span a
// healthy triangle.  Reading that as a coincident pair collapses the centre onto the chord and
// turns a quarter turn into a reflex sweep
#[test]
fn calc_arc_center_few_unit_arc_keeps_its_circumcircle() {
    let arc = ShapeArc::from_center_start_angle(pt(0, 0), pt(5, 0), ANGLE_90, 0);

    assert!(arc.get_central_angle().as_degrees().abs() < 180.0);

    // The chord midpoint fallback sits at (3, 3), inside the arc it is supposed to circumscribe
    assert!(((arc.get_center() - arc.get_arc_mid()).euclidean_norm() as f64) > arc.get_radius() / 2.0);
}

// A board-scale arc must not regress from the new pairwise coincidence guards
#[test]
fn calc_arc_center_board_scale_sanity() {
    let r = 50000000.0;
    let start = Vector2D::new(r, 0.0);
    let mid = Vector2D::new(0.0, r);
    let end = Vector2D::new(-r, 0.0);

    let center = calc_arc_center_d(start, mid, end);
    assert!(is_small(center.x, 1.0));
    assert!(is_small(center.y, 1.0));
}

/// Circumcenter of integer points relative to start.  The C++ uses `long double`; the
/// 128-bit integer numerators below are exact, and the quotients are compared with a margin
/// that is far below the tolerance of the checks.
fn exact_circumcenter(a_s: Vector2I, a_m: Vector2I, a_e: Vector2I) -> (f64, f64) {
    let bx = a_m.x as i128 - a_s.x as i128;
    let by = a_m.y as i128 - a_s.y as i128;
    let cx = a_e.x as i128 - a_s.x as i128;
    let cy = a_e.y as i128 - a_s.y as i128;
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let d = 2 * (bx * cy - by * cx);

    (
        a_s.x as f64 + (b2 * cy - c2 * by) as f64 / d as f64,
        a_s.y as f64 + (c2 * bx - b2 * cx) as f64 / d as f64,
    )
}

/// Point on the circle about `a_center`, rounded to integer IU
fn arc_point(a_center: Vector2D, a_radius: f64, a_degrees: f64, a_offset: Vector2I) -> Vector2I {
    let a = a_degrees * std::f64::consts::PI / 180.0;

    Vector2I::new(
        ki_round(a_center.x + a_radius * a.cos()) + a_offset.x,
        ki_round(a_center.y + a_radius * a.sin()) + a_offset.y,
    )
}

// A 150 um arc must get the same center wherever it sits.  The slope formulation drifted by
// 147 nm at 10 cm and 1244 nm at 1 m
#[test]
fn calc_arc_center_translation_invariant() {
    let c0 = Vector2D::new(18833.0, 51544.24);
    let radius = 150000.0;
    let offsets = [0, 100000000, 1000000000, -1000000000];
    let mut base_center = Vector2I::default();

    for off in offsets {
        let o = pt(off, off);
        let s = arc_point(c0, radius, 20.0, o);
        let m = arc_point(c0, radius, 60.0, o);
        let e = arc_point(c0, radius, 100.0, o);

        let center = calc_arc_center(s, m, e);
        let exact = exact_circumcenter(s, m, e);
        let dcenter = calc_arc_center_d(s.into(), m.into(), e.into());

        assert!((center.x as f64 - exact.0).abs() <= 1.5);
        assert!((center.y as f64 - exact.1).abs() <= 1.5);

        let rs = (dcenter - Vector2D::from(s)).euclidean_norm();
        let re = (dcenter - Vector2D::from(e)).euclidean_norm();
        assert!((rs - re).abs() <= 1.0);

        if off == 0 {
            base_center = center;
        } else {
            assert!(((center - o - base_center).euclidean_norm() as f64) <= 1.5);
        }
    }
}

// Codex found the slope form returning (1e9 + 5, 1e9) for this right-angled arc
#[test]
fn calc_arc_center_far_from_origin() {
    let s = pt(1000000000, 1000000000);
    let m = pt(1000000010, 1000000000);
    let e = pt(1000000000, 1000000010);

    assert_eq!(calc_arc_center(s, m, e), pt(1000000005, 1000000005));
    assert_eq!(
        calc_arc_center_d(s.into(), m.into(), e.into()),
        Vector2D::new(1000000005.0, 1000000005.0)
    );
}

// A center a nm or two off the 100 nm grid snaps to it, since every radius still agrees
#[test]
fn calc_arc_center_snaps_to_grid_when_radii_agree() {
    let c = pt(1000000, 2000000);
    let s = c + pt(250000, 0);
    let m = c + pt(150000, 200001);
    let e = c + pt(-150000, 200000);

    let exact = exact_circumcenter(s, m, e);
    assert!((exact.0 - c.x as f64).abs() + (exact.1 - c.y as f64).abs() > 1.0);

    assert_eq!(calc_arc_center(s, m, e), c);
    assert_eq!(calc_arc_center_d(s.into(), m.into(), e.into()), Vector2D::from(c));
}

// The 10 nm grid point would put the radii 4 nm apart, so the exact center is kept
#[test]
fn calc_arc_center_keeps_exact_when_snap_breaks_radii() {
    let s = pt(0, 0);
    let m = pt(10, 0);
    let e = pt(0, 10);

    assert_eq!(calc_arc_center(s, m, e), pt(5, 5));
}

// Near a half turn r^2 - chord^2/4 cancels to nothing; the center must sit at the true offset
#[test]
fn calc_arc_center_from_angle_near_half_turn() {
    let start = Vector2D::new(0.0, 0.0);
    let end = Vector2D::new(1.0e9, 0.0);
    let angle = deg(179.999999);

    let center = calc_arc_center_from_angle(start, end, angle);

    // The C++ reference value is computed in long double; tan( half ) is far from a rounding
    // boundary here, so double gives the same pass / fail result
    let half = angle.as_radians() / 2.0;
    let offset = 5.0e8 * half.cos() / half.sin();

    assert!((center.x - 5.0e8).abs() <= 1e-3);
    assert!((center.y.abs() - offset).abs() <= 0.1);
}

// The center is rounded once from an exact numerator, so a .4999999 fraction cannot be pushed up
#[test]
fn calc_arc_center_rounds_once_near_half() {
    let s = pt(-2123435277, -1114039370);
    let m = pt(-1828331619, -517415848);
    let e = pt(2011509719, -1246150838);

    assert_eq!(calc_arc_center(s, m, e), pt(-74371215, -1756259108));
}

// GetClearance passes an INT_MAX / 2 sentinel that must not overflow the arc bounding box
#[test]
fn get_clearance_against_segment() {
    let arc = ShapeArc::from_center_start_angle(pt(0, 0), pt(1000000, 0), ANGLE_90, 50000);
    let s = ShapeSegment::new(pt(2000000, 0), pt(2000000, 0), 50000);

    // Gap between the arc start point and the segment minus both half widths
    assert_eq!(arc.get_clearance(&s), 1000000 - 25000 - 25000);
    assert_eq!(s.get_clearance(&arc), 1000000 - 25000 - 25000);
}

// Beyond the search cutoff no collision is reported, which must not read as zero clearance
#[test]
fn get_clearance_beyond_cutoff() {
    let a = ShapeSegment::new(pt(0, 0), pt(0, 0), 0);
    let b = ShapeSegment::new(pt(1500000000, 0), pt(1500000000, 0), 0);

    assert_eq!(a.get_clearance(&b), i32::MAX);
}

/// Unused helper kept for parity with the C++ test file.
#[allow(dead_code)]
fn unused_vector_type(_v: Vector2<f64>) {}
