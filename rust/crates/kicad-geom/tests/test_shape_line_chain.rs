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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_line_chain.cpp` (suite
//! `TestShapeLineChain`) and of the line chain tests of `test_shape_line_chain_collision.cpp`.
//!
//! NOTE: Collision of SHAPE_LINE_CHAIN with arcs is tested in test_shape_arc.cpp

use kicad_geom::geometry::eda_angle::{ANGLE_180, ANGLE_360, DEGREES_T, EdaAngle};
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_line_chain::{ClipperZValue, ShapeLineChain, StringStream};
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::is_outline_valid;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn arc3(s: Vector2I, m: Vector2I, e: Vector2I) -> ShapeArc {
    ShapeArc::from_start_mid_end(s, m, e, 0)
}

/// `SLC_CASES`
#[derive(Clone)]
struct SlcCases {
    circle1_arc: ShapeLineChain,
    circle2_arcs: ShapeLineChain,
    arcs_coincident: ShapeLineChain,
    arcs_coincident_closed: ShapeLineChain,
    arcs_independent: ShapeLineChain,
    duplicate_arcs: ShapeLineChain,
    arcs_and_seg_mixed: ShapeLineChain,
    arc_and_point: ShapeLineChain,
    seg_and_arc_coincident: ShapeLineChain,
    empty_chain: ShapeLineChain,
    one_point: ShapeLineChain,
    two_points: ShapeLineChain,
    three_points: ShapeLineChain,
}

impl SlcCases {
    fn new() -> Self {
        // Full Circle arc
        let arc_circle = arc3(
            v(183450000, 128360000),
            v(183850000, 128360000),
            v(183450000, 128360000),
        );
        // First half of a circle
        let arc0a = arc3(
            v(183450000, 128360000),
            v(183650000, 128560000),
            v(183850000, 128360000),
        );
        // Second half of a circle
        let arc0b = arc3(
            v(183850000, 128360000),
            v(183650000, 128160000),
            v(183450000, 128360000),
        );
        // start coincident with Arc0a end
        let arc1 = arc3(
            v(183850000, 128360000),
            v(183638550, 128640305),
            v(183500000, 129204974),
        );
        // Independent arc
        let arc2 = arc3(
            v(283450000, 228360000),
            v(283650000, 228560000),
            v(283850000, 228360000),
        );
        // Arc with angle >180
        let arc3_ = arc3(v(0, 0), v(24142136, 10000000), v(0, 20000000));

        let mut circle1_arc = ShapeLineChain::new();
        circle1_arc.append_arc_with_error(&arc_circle, ARC_HIGH_DEF);
        circle1_arc.set_closed(true);

        let mut circle2_arcs = ShapeLineChain::new();
        circle2_arcs.append_arc_with_error(&arc0a, ARC_HIGH_DEF);
        circle2_arcs.append_arc_with_error(&arc0b, ARC_HIGH_DEF);
        circle2_arcs.set_closed(true);

        let mut arcs_coincident = ShapeLineChain::new();
        arcs_coincident.append_arc_with_error(&arc0a, ARC_HIGH_DEF);
        arcs_coincident.append_arc_with_error(&arc1, ARC_HIGH_DEF);

        let mut arcs_coincident_closed = arcs_coincident.clone();
        arcs_coincident_closed.set_closed(true);

        let mut arcs_independent = ShapeLineChain::new();
        arcs_independent.append_arc_with_error(&arc0a, ARC_HIGH_DEF);
        arcs_independent.append_arc_with_error(&arc2, ARC_HIGH_DEF);

        let mut duplicate_arcs = arcs_coincident.clone();
        // should add a segment between end of the chain and new copy of the arc
        duplicate_arcs.append_arc_with_error(&arc1, ARC_HIGH_DEF);

        let mut arc_and_point = ShapeLineChain::new();
        arc_and_point.append_arc_with_error(&arc0a, ARC_HIGH_DEF);
        arc_and_point.append_point(v(233450000, 228360000), false);

        let mut arcs_and_seg_mixed = arc_and_point.clone();
        arcs_and_seg_mixed.append_arc_with_error(&arc2, ARC_HIGH_DEF);

        let mut one_point = ShapeLineChain::new();
        one_point.append_point(v(233450000, 228360000), false);

        let mut two_points = ShapeLineChain::new();
        two_points.append_point(v(233450000, 228360000), false);
        two_points.append_point(v(263450000, 258360000), false);

        let mut three_points = two_points.clone();
        three_points.append_point(v(263450000, 308360000), false);

        let mut seg_and_arc_coincident = ShapeLineChain::new();
        seg_and_arc_coincident.append_point(v(0, 20000000), false);
        seg_and_arc_coincident.append_arc_with_error(&arc3_, ARC_HIGH_DEF);

        SlcCases {
            circle1_arc,
            circle2_arcs,
            arcs_coincident,
            arcs_coincident_closed,
            arcs_independent,
            duplicate_arcs,
            arcs_and_seg_mixed,
            arc_and_point,
            seg_and_arc_coincident,
            empty_chain: ShapeLineChain::new(),
            one_point,
            two_points,
            three_points,
        }
    }
}

#[test]
fn clipper_constructor_case1() {
    // Case of an arc followed by a segment
    // The clipper path is not in order (on purpose), to simulate the typical return from clipper
    let path_clipper2: Vec<(i64, i64, i64)> = vec![
        (125663951, 120099260, 24),
        (125388111, 120170850, 25),
        (125124975, 120280270, 26),
        (124879705, 120425376, 27),
        (124657110, 120603322, 28),
        (124461556, 120810617, 29),
        (124296876, 121043198, 30),
        (124166301, 121296503, 31),
        (124072391, 121565564, 32),
        (124016988, 121845106, 33),
        (124001177, 122129646, 34),
        (124025270, 122413605, 35),
        (124088794, 122691414, 36),
        (124190502, 122957625, 37),
        (124328401, 123207018, 38),
        (124499787, 123434703, 39),
        (124598846, 123537154, 40),
        (127171000, 123786000, 4),
        (127287862, 123704439, 5),
        (127499716, 123513831, 6),
        (127682866, 123295498, 7),
        (127833720, 123053722, 8),
        (127949321, 122793242, 9),
        (128027402, 122519168, 10),
        (128066430, 122236874, 11),
        (128065642, 121951896, 12),
        (128025053, 121669823, 13),
        (127945457, 121396185, 14),
        (127828417, 121136349, 15),
        (127676227, 120895410, 16),
        (127491873, 120678094, 17),
        (127278968, 120488661, 18),
        (127041689, 120330827, 19),
        (126784688, 120207687, 20),
        (126513005, 120121655, 21),
        (126231968, 120074419, 22),
        (125947087, 120066905, 23),
    ];

    let mut z_values = vec![ClipperZValue::from_shape_indices((-1, -1), 0); 4];
    z_values.extend(std::iter::repeat_n(
        ClipperZValue::from_shape_indices((0, -1), 0),
        37,
    ));

    let arcs = vec![arc3(
        v(127171000, 123786000),
        v(126231718, 120077003),
        v(124598846, 123537154),
    )];

    let clipper2chain = ShapeLineChain::from_clipper2(&path_clipper2, &z_values, &arcs);

    assert!(is_outline_valid(&clipper2chain));
    assert_eq!(clipper2chain.point_count(), 37);
    assert_eq!(clipper2chain.arc_count(), 1);
    assert_eq!(clipper2chain.shape_count(), 2);
    assert!(clipper2chain.is_closed());
}

#[test]
fn arc_to_polyline() {
    let mut base_chain = ShapeLineChain::from_points(&[v(0, 0), v(0, 1000), v(1000, 0)], false);

    let chain_insert = ShapeLineChain::from_points(&[v(0, 1500), v(1500, 1500), v(1500, 0)], false);

    let arc_insert1 = ShapeLineChain::from_arc(
        &ShapeArc::from_center_start_angle(v(0, -100), v(0, -200), ANGLE_180, 0),
        false,
        Some(ARC_HIGH_DEF),
    );

    let arc_insert2 = ShapeLineChain::from_arc(
        &ShapeArc::from_center_start_angle(v(0, 500), v(0, 400), ANGLE_180, 0),
        false,
        Some(ARC_HIGH_DEF),
    );

    assert_eq!(base_chain.c_shapes().len(), base_chain.c_points().len());
    assert_eq!(arc_insert1.c_shapes().len(), arc_insert1.c_points().len());
    assert_eq!(arc_insert2.c_shapes().len(), arc_insert2.c_points().len());

    assert!(is_outline_valid(&base_chain));
    assert!(is_outline_valid(&arc_insert1));
    assert!(is_outline_valid(&arc_insert2));

    base_chain.insert_arc_with_error(
        0,
        &ShapeArc::from_center_start_angle(v(0, -100), v(0, -200), ANGLE_180, 0),
        ARC_HIGH_DEF,
    );
    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.c_shapes().len(), base_chain.c_points().len());

    base_chain.replace_chain(0, 2, &chain_insert);
    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.c_shapes().len(), base_chain.c_points().len());
}

// Similar test to above but with larger coordinates, so we have more than one point per arc
#[test]
fn arc_to_polyline_large_coords() {
    let mut base_chain = ShapeLineChain::from_points(&[v(0, 0), v(0, 100000), v(100000, 0)], false);

    let chain_insert =
        ShapeLineChain::from_points(&[v(0, 1500000), v(1500000, 1500000), v(1500000, 0)], false);

    base_chain.append_arc_with_error(
        &ShapeArc::from_center_start_angle(v(200000, 0), v(300000, 100000), ANGLE_180, 0),
        ARC_HIGH_DEF,
    );

    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.point_count(), 11);

    base_chain.insert_point(9, v(250000, 0));
    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.point_count(), 12);
    assert_eq!(base_chain.arc_count(), 2); // Should have two arcs after the split

    base_chain.replace_chain(5, 6, &chain_insert);
    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.point_count(), 13); // Adding 3 points, removing 2
    assert_eq!(base_chain.arc_count(), 3); // Should have three arcs after the split

    base_chain.replace_point(4, 6, v(550000, 0));
    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.point_count(), 11); // Adding 1 point, removing 3
    assert_eq!(base_chain.arc_count(), 3); // Should still have three arcs

    // Test ClearArcs
    base_chain.set_closed(true);
    let area_prior_to_arc_removal = base_chain.area(true);
    base_chain.clear_arcs();

    assert!(is_outline_valid(&base_chain));
    assert_eq!(base_chain.c_points().len(), base_chain.c_shapes().len());
    assert_eq!(base_chain.point_count(), 11); // We should have the same number of points
    assert_eq!(base_chain.arc_count(), 0); // All arcs should have been removed
    assert_eq!(base_chain.area(true), area_prior_to_arc_removal); // Area should not have changed
}

// Test that duplicate point gets removed when line is set to be closed and added where required
#[test]
fn set_closed_duplicate_point() {
    // Test from issue #9843
    let mut chain = ShapeLineChain::new();

    chain.append_arc_with_error(
        &arc3(
            v(-859598, 2559876),
            v(-1632771, 1022403),
            v(-3170244, 249230),
        ),
        ARC_HIGH_DEF,
    );

    chain.append_arc_with_error(
        &arc3(
            v(-3170244, -1657832),
            v(-292804, -317564),
            v(1047464, 2559876),
        ),
        ARC_HIGH_DEF,
    );

    chain.append_point(v(-859598, 2559876), false); // add point that is equal to first arc start

    assert!(is_outline_valid(&chain));
    assert_eq!(chain.point_count(), 31);

    // CLOSED CHAIN
    chain.set_closed(true);
    assert_eq!(chain.c_points().len(), chain.c_shapes().len());
    assert_eq!(chain.point_count(), 30); // (-1) should have removed coincident points
    assert!(is_outline_valid(&chain));

    // Special case: arc wrapping around to start (e.g. circle)
    let mut c = SlcCases::new();
    assert!(is_outline_valid(&c.circle2_arcs));
    assert!(c.circle2_arcs.is_closed());
    assert_eq!(c.circle2_arcs.point_count(), 16);
    assert!(c.circle2_arcs.is_arc_segment(15));
    assert_eq!(c.circle2_arcs.shape_count(), 2);
    c.circle2_arcs.set_closed(false);
    assert!(is_outline_valid(&c.circle2_arcs));
    assert!(!c.circle2_arcs.is_closed());
    assert_eq!(c.circle2_arcs.point_count(), 17);
    assert!(c.circle2_arcs.is_arc_segment(15));
    assert!(!c.circle2_arcs.is_arc_segment(16)); // last point doesn't join up
}

#[test]
fn toggle_closed() {
    let c = SlcCases::new();

    // ( name, chain, closed, shape count, point count, expected shape count, expected points )
    let cases: Vec<(&str, &ShapeLineChain, bool, i32, i32, i32, i32)> = vec![
        ("Circle1Arc", &c.circle1_arc, true, 1, 15, 1, 16),
        ("Circle2Arcs", &c.circle2_arcs, true, 2, 16, 2, 17),
        ("ArcsCoincident", &c.arcs_coincident, false, 2, 14, 3, 14),
        (
            "ArcsCoincidentClosed",
            &c.arcs_coincident_closed,
            true,
            3,
            14,
            2,
            14,
        ),
        ("ArcsIndependent", &c.arcs_independent, false, 3, 18, 4, 18),
        // SegAndArcCoincident will remove the segment after SetClosed(true) and SetClosed(false)
        // disable test for now
        //{ "SegAndArcCoincident",  SLC_CASES().SegAndArcCoincident,  false, 2, 92, 2, 91 },
        ("DuplicateArcs", &c.duplicate_arcs, false, 4, 20, 5, 20),
        ("ArcAndPoint", &c.arc_and_point, false, 2, 10, 3, 10),
        (
            "ArcsAndSegMixed",
            &c.arcs_and_seg_mixed,
            false,
            4,
            19,
            5,
            19,
        ),
        ("OnePoint", &c.one_point, false, 0, 1, 0, 1), // no shapes
        ("TwoPoints", &c.two_points, false, 1, 2, 2, 2), // there and back
        ("ThreePoints", &c.three_points, false, 2, 3, 3, 3),
    ];

    for (name, chain, closed, shape_count, point_count, exp_shapes, exp_points) in cases {
        let mut slc_case = chain.clone(); // make a copy to edit
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.is_closed(), closed, "{name}");
        assert_eq!(slc_case.shape_count(), shape_count, "{name}");
        assert_eq!(slc_case.point_count(), point_count, "{name}");
        slc_case.set_closed(!closed);
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.is_closed(), !closed, "{name}");
        assert_eq!(slc_case.shape_count(), exp_shapes, "{name}");
        assert_eq!(slc_case.point_count(), exp_points, "{name}");
        slc_case.set_closed(closed); // toggle back to normal
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.is_closed(), closed, "{name}");
        assert_eq!(slc_case.shape_count(), shape_count, "{name}");
        assert_eq!(slc_case.point_count(), point_count, "{name}");
    }
}

#[test]
fn point_in_polygon() {
    let pts1 = [
        (1316455, 913576),
        (1316455, 901129),
        (1321102, 901129),
        (1322152, 901191),
        (1323055, 901365),
        (1323830, 901639),
        (1324543, 902036),
        (1325121, 902521),
        (1325581, 903100),
        (1325914, 903759),
        (1326120, 904516),
        (1326193, 905390),
        (1326121, 906253),
        (1325915, 907005),
        (1325581, 907667),
        (1325121, 908248),
        (1324543, 908735),
        (1323830, 909132),
        (1323055, 909406),
        (1322153, 909579),
        (1321102, 909641),
        (1317174, 909641),
        (1317757, 909027),
        (1317757, 913576),
    ];
    let pts2 = [
        (1297076, 916244),
        (1284629, 916244),
        (1284629, 911597),
        (1284691, 910547),
        (1284865, 909644),
        (1285139, 908869),
        (1285536, 908156),
        (1286021, 907578),
        (1286600, 907118),
        (1287259, 906785),
        (1288016, 906579),
        (1288890, 906506),
        (1289753, 906578),
        (1290505, 906784),
        (1291167, 907118),
        (1291748, 907578),
        (1292235, 908156),
        (1292632, 908869),
        (1292906, 909644),
        (1293079, 910546),
        (1293141, 911597),
        (1293141, 915525),
        (1292527, 914942),
        (1297076, 914942),
    ];
    let to_v = |p: &[(i32, i32)]| p.iter().map(|&(x, y)| v(x, y)).collect::<Vec<_>>();
    let mut outline1 = ShapeLineChain::from_points(&to_v(&pts1), false);
    let mut outline2 = ShapeLineChain::from_points(&to_v(&pts2), false);

    // Test a point inside the polygon
    let point1 = v(1317757, 909133);
    let point2 = v(1292633, 914942);

    outline1.set_closed(true);
    outline2.set_closed(true);

    assert!(outline1.point_inside(point1, 0, false));
    assert!(outline2.point_inside(point2, 0, false));
}

// Test that duplicate point gets removed when we call simplify
#[test]
fn simplify_duplicate_point() {
    let mut chain = ShapeLineChain::new();

    chain.append_point(v(100, 100), false);
    chain.append_point(v(100, 100), true); //duplicate point to simplify
    chain.append_point(v(200, 100), false);

    assert!(is_outline_valid(&chain));
    assert_eq!(chain.point_count(), 3);

    chain.simplify(0);

    assert_eq!(chain.c_points().len(), chain.c_shapes().len());
    assert_eq!(chain.point_count(), 2); // (-1) should have removed coincident points
    assert!(is_outline_valid(&chain));
}

// Test that duplicate point gets removed when we call simplify
#[test]
fn simplify_keep_end_point() {
    let mut chain = ShapeLineChain::new();

    chain.append_point(v(114772424, 90949410), false);
    chain.append_point(v(114767360, 90947240), false);
    chain.append_point(v(114772429, 90947228), false);
    chain.set_closed(true);

    assert!(is_outline_valid(&chain));
    assert_eq!(chain.point_count(), 3);

    chain.simplify(0);

    assert_eq!(chain.c_points().len(), chain.c_shapes().len());
    assert_eq!(chain.point_count(), 3);
    assert!(is_outline_valid(&chain));
}

#[test]
fn simplify_pns_chain() {
    let mut chain = ShapeLineChain::new();

    for (x, y) in [
        (157527820, 223074385),
        (186541122, 159990156),
        (186528624, 159977658),
        (186528624, 159770550),
        (186528625, 159366691),
        (186541122, 159354195),
        (186541122, 155566877),
        (187291125, 154816872),
        (187291125, 147807837),
        (189301788, 145797175),
        (194451695, 145797175),
        (195021410, 146366890),
    ] {
        chain.append_point(v(x, y), false);
    }

    assert!(is_outline_valid(&chain));
    assert_eq!(chain.point_count(), 12);

    // The chain should be open, so the points should not be simplified
    // between the begining and the end.
    chain.simplify(10);

    assert_eq!(chain.point_count(), 11);
}

#[test]
fn simplify_complex_chain() {
    let mut chain = ShapeLineChain::new();

    // Append points
    for (x, y) in [
        (130000, 147320),
        (125730, 147320),
        (125730, 150630),
        (128800, 153700),
        (150300, 153700),
        (151500, 152500),
        (151500, 148900),
        (149920, 147320),
        (140000, 147320),
    ] {
        chain.append_point(v(x, y), false);
    }

    assert!(is_outline_valid(&chain));
    assert_eq!(chain.point_count(), 9);

    // The chain should be open, so the points should not be simplified
    // between the begining and the end.
    chain.simplify(0);

    assert_eq!(chain.point_count(), 9);

    chain.set_closed(true);
    chain.simplify(0);

    assert_eq!(chain.point_count(), 8);
}

/// `remove_shape_cases`: ( name, chain, shape count, arc count, remove index, expected shape
/// count, expected arc count )
fn remove_shape_cases(
    c: &SlcCases,
) -> Vec<(&'static str, ShapeLineChain, i32, usize, i32, i32, usize)> {
    let c1 = &c.circle1_arc;
    let c2 = &c.circle2_arcs;
    let ac = &c.arcs_coincident;
    let acc = &c.arcs_coincident_closed;
    let ai = &c.arcs_independent;
    let da = &c.duplicate_arcs;
    let am = &c.arcs_and_seg_mixed;

    vec![
        (
            "Circle1Arc - 1st arc - index on start",
            c1.clone(),
            1,
            1,
            0,
            0,
            0,
        ),
        (
            "Circle1Arc - 1st arc - index on mid",
            c1.clone(),
            1,
            1,
            8,
            0,
            0,
        ),
        (
            "Circle1Arc - 1st arc - index on end",
            c1.clone(),
            1,
            1,
            14,
            0,
            0,
        ),
        (
            "Circle1Arc - 1st arc - index on  -1",
            c1.clone(),
            1,
            1,
            -1,
            0,
            0,
        ),
        ("Circle1Arc - invalid index", c1.clone(), 1, 1, 15, 1, 1),
        (
            "Circle2Arcs - 1st arc - index on start",
            c2.clone(),
            2,
            2,
            0,
            2,
            1,
        ),
        (
            "Circle2Arcs - 1st arc - index on mid",
            c2.clone(),
            2,
            2,
            3,
            2,
            1,
        ),
        (
            "Circle2Arcs - 1st arc - index on end",
            c2.clone(),
            2,
            2,
            7,
            2,
            1,
        ),
        (
            "Circle2Arcs - 2nd arc - index on start",
            c2.clone(),
            2,
            2,
            8,
            2,
            1,
        ),
        (
            "Circle2Arcs - 2nd arc - index on mid",
            c2.clone(),
            2,
            2,
            11,
            2,
            1,
        ),
        (
            "Circle2Arcs - 2nd arc - index on end",
            c2.clone(),
            2,
            2,
            15,
            2,
            1,
        ),
        (
            "Circle2Arcs - 2nd arc - index on  -1",
            c2.clone(),
            2,
            2,
            -1,
            2,
            1,
        ),
        ("Circle2Arcs - invalid index", c2.clone(), 2, 2, 16, 2, 2),
        (
            "ArcsCoinc. - 1st arc - idx on start",
            ac.clone(),
            2,
            2,
            0,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 1st arc - idx on mid",
            ac.clone(),
            2,
            2,
            3,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 1st arc - idx on end",
            ac.clone(),
            2,
            2,
            7,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 2nd arc - idx on start",
            ac.clone(),
            2,
            2,
            8,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 2nd arc - idx on mid",
            ac.clone(),
            2,
            2,
            10,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 2nd arc - idx on end",
            ac.clone(),
            2,
            2,
            13,
            1,
            1,
        ),
        (
            "ArcsCoinc. - 2nd arc - idx on  -1",
            ac.clone(),
            2,
            2,
            -1,
            1,
            1,
        ),
        ("ArcsCoinc. - invalid idx", ac.clone(), 2, 2, 14, 2, 2),
        (
            "ArcsCoinc. - 1st arc - idx on start",
            ac.clone(),
            2,
            2,
            0,
            1,
            1,
        ),
        (
            "A.Co.Closed - 1st arc - idx on start",
            acc.clone(),
            3,
            2,
            1,
            2,
            1,
        ),
        (
            "A.Co.Closed - 1st arc - idx on mid",
            acc.clone(),
            3,
            2,
            3,
            2,
            1,
        ),
        (
            "A.Co.Closed - 1st arc - idx on end",
            acc.clone(),
            3,
            2,
            7,
            2,
            1,
        ),
        (
            "A.Co.Closed - 2nd arc - idx on start",
            acc.clone(),
            3,
            2,
            8,
            2,
            1,
        ),
        (
            "A.Co.Closed - 2nd arc - idx on mid",
            acc.clone(),
            3,
            2,
            10,
            2,
            1,
        ),
        (
            "A.Co.Closed - 2nd arc - idx on end",
            acc.clone(),
            3,
            2,
            13,
            2,
            1,
        ),
        (
            "A.Co.Closed - 2nd arc - idx on  -1",
            acc.clone(),
            3,
            2,
            -1,
            2,
            1,
        ),
        ("A.Co.Closed - invalid idx", acc.clone(), 3, 2, 14, 3, 2),
        (
            "ArcsIndep. - 1st arc - idx on start",
            ai.clone(),
            3,
            2,
            0,
            1,
            1,
        ),
        (
            "ArcsIndep. - 1st arc - idx on mid",
            ai.clone(),
            3,
            2,
            3,
            1,
            1,
        ),
        (
            "ArcsIndep. - 1st arc - idx on end",
            ai.clone(),
            3,
            2,
            8,
            1,
            1,
        ),
        (
            "ArcsIndep. - 2nd arc - idx on start",
            ai.clone(),
            3,
            2,
            9,
            1,
            1,
        ),
        (
            "ArcsIndep. - 2nd arc - idx on mid",
            ai.clone(),
            3,
            2,
            12,
            1,
            1,
        ),
        (
            "ArcsIndep. - 2nd arc - idx on end",
            ai.clone(),
            3,
            2,
            17,
            1,
            1,
        ),
        (
            "ArcsIndep. - 2nd arc - idx on  -1",
            ai.clone(),
            3,
            2,
            -1,
            1,
            1,
        ),
        ("ArcsIndep. - invalid idx", ai.clone(), 3, 2, 18, 3, 2),
        (
            "Dup.Arcs - 1st arc - idx on start",
            da.clone(),
            4,
            3,
            0,
            3,
            2,
        ),
        ("Dup.Arcs - 1st arc - idx on mid", da.clone(), 4, 3, 3, 3, 2),
        ("Dup.Arcs - 1st arc - idx on end", da.clone(), 4, 3, 7, 3, 2),
        (
            "Dup.Arcs - 2nd arc - idx on start",
            da.clone(),
            4,
            3,
            8,
            3,
            2,
        ),
        (
            "Dup.Arcs - 2nd arc - idx on mid",
            da.clone(),
            4,
            3,
            10,
            3,
            2,
        ),
        (
            "Dup.Arcs - 2nd arc - idx on end",
            da.clone(),
            4,
            3,
            13,
            3,
            2,
        ),
        (
            "Dup.Arcs - 3rd arc - idx on start",
            da.clone(),
            4,
            3,
            14,
            2,
            2,
        ),
        (
            "Dup.Arcs - 3rd arc - idx on mid",
            da.clone(),
            4,
            3,
            17,
            2,
            2,
        ),
        (
            "Dup.Arcs - 3rd arc - idx on end",
            da.clone(),
            4,
            3,
            19,
            2,
            2,
        ),
        (
            "Dup.Arcs - 3rd arc - idx on  -1",
            da.clone(),
            4,
            3,
            -1,
            2,
            2,
        ),
        ("Dup.Arcs - invalid idx", da.clone(), 4, 3, 20, 4, 3),
        (
            "Arcs Mixed - 1st arc - idx on start",
            am.clone(),
            4,
            2,
            0,
            2,
            1,
        ),
        (
            "Arcs Mixed - 1st arc - idx on mid",
            am.clone(),
            4,
            2,
            3,
            2,
            1,
        ),
        (
            "Arcs Mixed - 1st arc - idx on end",
            am.clone(),
            4,
            2,
            8,
            2,
            1,
        ),
        ("Arcs Mixed - Straight segment", am.clone(), 4, 2, 9, 3, 2),
        (
            "Arcs Mixed - 2nd arc - idx on start",
            am.clone(),
            4,
            2,
            10,
            2,
            1,
        ),
        (
            "Arcs Mixed - 2nd arc - idx on mid",
            am.clone(),
            4,
            2,
            14,
            2,
            1,
        ),
        (
            "Arcs Mixed - 2nd arc - idx on end",
            am.clone(),
            4,
            2,
            18,
            2,
            1,
        ),
        (
            "Arcs Mixed - 2nd arc - idx on  -1",
            am.clone(),
            4,
            2,
            -1,
            2,
            1,
        ),
        ("Arcs Mixed - invalid idx", am.clone(), 4, 2, 19, 4, 2),
    ]
}

#[test]
fn remove_shape() {
    let c = SlcCases::new();

    for (name, chain, shapes, arcs, idx, exp_shapes, exp_arcs) in remove_shape_cases(&c) {
        let mut slc_case = chain; // make a copy to edit
        assert_eq!(slc_case.shape_count(), shapes, "{name}");
        assert_eq!(slc_case.arc_count(), arcs, "{name}");
        assert!(is_outline_valid(&slc_case), "{name}");
        slc_case.remove_shape(idx);
        assert_eq!(slc_case.shape_count(), exp_shapes, "{name}");
        assert_eq!(slc_case.arc_count(), exp_arcs, "{name}");
        assert!(is_outline_valid(&slc_case), "{name}");
    }
}

#[test]
fn remove_shape_after_simplify() {
    let c = SlcCases::new();

    for (name, chain, shapes, arcs, idx, exp_shapes, exp_arcs) in remove_shape_cases(&c) {
        let mut slc_case = chain; // make a copy to edit
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.shape_count(), shapes, "{name}");
        assert_eq!(slc_case.arc_count(), arcs, "{name}");
        slc_case.simplify(0);
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.shape_count(), shapes, "{name}");
        assert_eq!(slc_case.arc_count(), arcs, "{name}");
        slc_case.remove_shape(idx);
        assert!(is_outline_valid(&slc_case), "{name}");
        assert_eq!(slc_case.shape_count(), exp_shapes, "{name}");
        assert_eq!(slc_case.arc_count(), exp_arcs, "{name}");
    }
}

#[test]
fn shape_count() {
    let c = SlcCases::new();
    assert_eq!(c.circle1_arc.shape_count(), 1);
    assert_eq!(c.circle2_arcs.shape_count(), 2);
    assert_eq!(c.arcs_coincident.shape_count(), 2);
    assert_eq!(c.arcs_coincident_closed.shape_count(), 3);
    assert_eq!(c.duplicate_arcs.shape_count(), 4);
    assert_eq!(c.arc_and_point.shape_count(), 2);
    assert_eq!(c.arcs_and_seg_mixed.shape_count(), 4);
    assert_eq!(c.seg_and_arc_coincident.shape_count(), 2);
    assert_eq!(c.empty_chain.shape_count(), 0);
    assert_eq!(c.one_point.shape_count(), 0);
    assert_eq!(c.two_points.shape_count(), 1);
    assert_eq!(c.three_points.shape_count(), 2);
}

#[test]
fn next_shape() {
    let c = SlcCases::new();
    assert_eq!(c.circle1_arc.next_shape(0), -1); //only one arc

    assert_eq!(c.circle2_arcs.next_shape(0), 8); // next shape "Arc0b"
    assert_eq!(c.circle2_arcs.next_shape(8), -1); //no more shapes (last point joins with first, part of arc)

    assert_eq!(c.arcs_coincident.next_shape(0), 8); // next shape "Arc1"
    assert_eq!(c.arcs_coincident.next_shape(8), -1); //no more shapes

    assert_eq!(c.arcs_coincident_closed.next_shape(0), 8); // next shape "Arc1"
    assert_eq!(c.arcs_coincident_closed.next_shape(8), 13); //next shape is hidden segment joining last/first
    assert_eq!(c.arcs_coincident_closed.next_shape(13), -1); //no more shapes

    assert_eq!(c.arcs_independent.next_shape(0), 8); // next shape straight seg
    assert_eq!(c.arcs_independent.next_shape(8), 9); //next shape second arc
    assert_eq!(c.arcs_independent.next_shape(9), -1); //no more shapes

    assert_eq!(c.duplicate_arcs.next_shape(0), 8); // next shape "Arc1"
    assert_eq!(c.duplicate_arcs.next_shape(8), 13); // next shape hidden segment joining the 2 duplicate arcs
    assert_eq!(c.duplicate_arcs.next_shape(13), 14); // next shape "Arc1" (duplicate)
    assert_eq!(c.duplicate_arcs.next_shape(14), -1); //no more shapes

    assert_eq!(c.arc_and_point.next_shape(0), 8); // next shape straight segment (end of arc->point)
    assert_eq!(c.arc_and_point.next_shape(8), -1); //no more shapes

    assert_eq!(c.arcs_and_seg_mixed.next_shape(0), 8); // next shape straight segment (end of arc->point)
    assert_eq!(c.arcs_and_seg_mixed.next_shape(8), 9); // next shape straight segment (point->begining of arc)
    assert_eq!(c.arcs_and_seg_mixed.next_shape(9), 10); //next shape second arc
    assert_eq!(c.arcs_and_seg_mixed.next_shape(10), -1); //no more shapes
    assert_eq!(c.arcs_and_seg_mixed.next_shape(20), -1); //invalid indices should still work
    assert_eq!(c.arcs_and_seg_mixed.next_shape(-50), -1); //invalid indices should still work

    assert_eq!(c.seg_and_arc_coincident.next_shape(0), 1); // next shape Arc3
    assert_eq!(c.seg_and_arc_coincident.next_shape(1), -1); //no more shapes

    assert_eq!(c.empty_chain.next_shape(0), -1);
    assert_eq!(c.empty_chain.next_shape(1), -1); //invalid indices should still work
    assert_eq!(c.empty_chain.next_shape(2), -1); //invalid indices should still work
    assert_eq!(c.empty_chain.next_shape(-2), -1); //invalid indices should still work

    assert_eq!(c.one_point.next_shape(0), -1);
    assert_eq!(c.one_point.next_shape(-1), -1);
    assert_eq!(c.one_point.next_shape(1), -1); //invalid indices should still work
    assert_eq!(c.one_point.next_shape(2), -1); //invalid indices should still work
    assert_eq!(c.one_point.next_shape(-2), -1); //invalid indices should still work

    assert_eq!(c.two_points.next_shape(0), -1);
    assert_eq!(c.two_points.next_shape(1), -1);
    assert_eq!(c.two_points.next_shape(-1), -1);

    assert_eq!(c.three_points.next_shape(0), 1);
    assert_eq!(c.three_points.next_shape(1), -1);
    assert_eq!(c.three_points.next_shape(2), -1);
    assert_eq!(c.three_points.next_shape(-1), -1);
}

#[test]
fn append_arc() {
    // Case 1: Arc mid point nearly collinear
    {
        let arc = arc3(v(100000, 0), v(0, 2499), v(-100000, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 2);
        assert_eq!(chain.get_point(0), v(100000, 0)); //arc start
        assert_eq!(chain.get_point(1), v(-100000, 0)); //arc end
        assert_eq!(chain.get_point(-1), v(-100000, 0)); //arc end
    }

    // Case 2: Arc = Large Circle
    {
        let arc = arc3(v(100000, 0), v(0, 0), v(100000, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 1);
        assert_eq!(chain.point_count(), 10);
        assert_eq!(chain.get_point(0), v(100000, 0)); //arc start
        assert_eq!(chain.get_point(9), v(100000, 0)); //arc end
        assert_eq!(chain.get_point(-1), v(100000, 0)); //arc end
    }

    // Case 3: Arc = Small Circle (approximate to point)
    {
        let arc = arc3(v(2499, 0), v(0, 0), v(2499, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 1);
        assert_eq!(chain.get_point(0), v(2499, 0)); //arc start
    }

    // Case 3: Small Arc (approximate to segment)
    {
        let arc = arc3(v(1767, 0), v(2499, 2499), v(0, 1767));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 2);
        assert_eq!(chain.get_point(0), v(1767, 0)); //arc start
        assert_eq!(chain.get_point(1), v(0, 1767)); //arc end
    }

    // Case 4: Arc = null arc (all points coincident)
    {
        let arc = arc3(v(2499, 0), v(2499, 0), v(2499, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 1);
        assert_eq!(chain.get_point(0), v(2499, 0)); //arc start
    }

    // Case 5: Arc = infinite radius (all points very close)
    {
        let arc = arc3(v(2499, 0), v(2500, 0), v(2501, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 2);
        assert_eq!(chain.get_point(0), v(2499, 0)); //arc start
        assert_eq!(chain.get_point(1), v(2501, 0)); //arc end
    }

    // Case 6: Arc = large radius (all points very close)
    {
        let arc = arc3(v(-100000, 0), v(0, 1), v(100000, 0));
        let mut chain = ShapeLineChain::new();
        chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
        assert!(is_outline_valid(&chain));
        assert_eq!(chain.arc_count(), 0);
        assert_eq!(chain.point_count(), 2);
        assert_eq!(chain.get_point(0), v(-100000, 0)); //arc start
        assert_eq!(chain.get_point(1), v(100000, 0)); //arc end
    }
}

// Test special case where the last arc in the chain has a shared point with the first arc
#[test]
fn arc_wrapping_to_start_shared_points() {
    // represent a circle with two semicircular arcs
    let arc1 = arc3(v(100000, 0), v(0, 100000), v(-100000, 0));
    let arc2 = arc3(v(-100000, 0), v(0, -100000), v(100000, 0));

    // Start a chain with the two arcs
    let mut chain = ShapeLineChain::new();
    chain.append_arc_with_error(&arc1, ARC_HIGH_DEF);
    chain.append_arc_with_error(&arc2, ARC_HIGH_DEF);
    assert_eq!(chain.point_count(), 13);
    //BOOST_CHECK( GEOM_TEST::IsOutlineValid( chain ) );

    // OPEN CHAIN
    // Start of the chain is not yet a shared point, so can't be an arc end either
    assert!(!chain.is_shared_pt(0));
    assert!(!chain.is_arc_end(0));
    assert!(chain.is_arc_start(0));

    // Index 6 is the shared point between the two arcs in the middle of the chain
    assert!(chain.is_shared_pt(6));
    assert!(chain.is_arc_end(6));
    assert!(chain.is_arc_start(6));

    // End index is not yet a shared point
    let mut end_index = chain.point_count() as usize - 1;
    assert!(!chain.is_shared_pt(end_index));
    assert!(chain.is_arc_end(end_index));
    assert!(!chain.is_arc_start(end_index));

    for i in 0..chain.point_count() as usize {
        assert!(chain.is_pt_on_arc(i)); // all points in the chain are arcs
    }

    // CLOSED CHAIN
    chain.set_closed(true);
    assert_eq!(chain.point_count(), 12); // (-1) should have removed coincident points
    //BOOST_CHECK( GEOM_TEST::IsOutlineValid( chain ) );

    // Start of the chain should be a shared point now, so can't be an arc end either
    assert!(chain.is_shared_pt(0));
    assert!(chain.is_arc_end(0));
    assert!(chain.is_arc_start(0));

    // Index 6 is the shared point between the two arcs in the middle of the chain
    assert!(chain.is_shared_pt(6));
    assert!(chain.is_arc_end(6));
    assert!(chain.is_arc_start(6));

    // End index is in the middle of an arc, so not an end point or shared point
    end_index = chain.point_count() as usize - 1;
    assert!(!chain.is_shared_pt(end_index));
    assert!(!chain.is_arc_end(end_index));
    assert!(!chain.is_arc_start(end_index));
}

/// The chain of `Split` and `NearestPointPt`: seg1, a half circle, seg2.
fn seg_arc_seg_chain() -> (Seg, Seg, ShapeArc, ShapeLineChain) {
    let seg1 = Seg::new(v(0, 100000), v(50000, 0));
    let seg2 = Seg::new(v(200000, 0), v(300000, 0));
    let arc = ShapeArc::from_center_start_angle(v(200000, 0), v(300000, 0), ANGLE_180, 0);

    // Start a chain with 2 points (seg1)
    let mut chain = ShapeLineChain::from_points(&[seg1.a, seg1.b], false);
    assert_eq!(chain.point_count(), 2);
    // Add first arc
    chain.append_arc_with_error(&arc, ARC_HIGH_DEF);
    assert_eq!(chain.point_count(), 9);
    // Add two points (seg2)
    chain.append_point(seg2.a, false);
    chain.append_point(seg2.b, false);
    assert_eq!(chain.point_count(), 11);
    assert!(is_outline_valid(&chain));

    (seg1, seg2, arc, chain)
}

// Test SHAPE_LINE_CHAIN::Split()
#[test]
fn split() {
    let (seg1, _seg2, arc, chain) = seg_arc_seg_chain();

    // Case 1: Point not in the chain
    {
        let mut chain_copy = chain.clone();
        assert_eq!(chain_copy.split(v(400000, 0), false), -1);
        assert_eq!(chain_copy.point_count(), chain.point_count());
        assert_eq!(chain_copy.arc_count(), chain.arc_count());
    }

    // Case 2: Point close to start of a segment
    {
        let mut chain_copy = chain.clone();
        let split_point = seg1.a + v(5, -10);
        assert_eq!(chain_copy.split(split_point, false), 1);
        assert!(is_outline_valid(&chain_copy));
        assert_eq!(chain_copy.get_point(1), split_point);
        assert_eq!(chain_copy.point_count(), chain.point_count() + 1); // new point added
        assert_eq!(chain_copy.arc_count(), chain.arc_count());
    }

    // Case 3: Point exactly on the segment
    {
        let mut chain_copy = chain.clone();
        let split_point = seg1.b;
        assert_eq!(chain_copy.split(split_point, false), 1);
        assert!(is_outline_valid(&chain_copy));
        assert_eq!(chain_copy.get_point(1), split_point);
        assert_eq!(chain_copy.point_count(), chain.point_count());
        assert_eq!(chain_copy.arc_count(), chain.arc_count());
    }

    // Case 4: Point at start of arc
    {
        let mut chain_copy = chain.clone();
        let split_point = arc.get_p0();
        assert_eq!(chain_copy.split(split_point, false), 2);
        assert!(is_outline_valid(&chain_copy));
        assert_eq!(chain_copy.get_point(2), split_point);
        assert_eq!(chain_copy.point_count(), chain.point_count());
        assert_eq!(chain_copy.arc_count(), chain.arc_count());
    }

    // Case 5: Point close to start of arc
    {
        let mut chain_copy = chain.clone();
        let split_point = arc.get_p0() + v(-10, 130);
        assert_eq!(chain_copy.split(split_point, false), 3);
        assert!(is_outline_valid(&chain_copy));
        assert_eq!(chain_copy.get_point(3), split_point);
        assert!(chain_copy.is_shared_pt(3)); // must be a shared point
        assert_eq!(chain_copy.point_count(), chain.point_count() + 1); // new point added
        assert_eq!(chain_copy.arc_count(), chain.arc_count() + 1); // new arc should have been created
    }
}

// Test SHAPE_LINE_CHAIN::Slice()
#[test]
fn slice() {
    let target_segment = Seg::new(v(200000, 0), v(300000, 0));
    let first_arc = ShapeArc::from_center_start_angle(v(200000, 0), v(300000, 0), ANGLE_180, 0);
    let second_arc =
        ShapeArc::from_center_start_angle(v(-200000, -200000), v(-300000, -100000), -ANGLE_180, 0);
    let tol = ShapeArc::default_accuracy_for_pcb(); // Tolerance for arc collisions
    let collides = |a: &ShapeArc, p: Vector2I| a.collide_point(p, tol, None, None);

    let expected_arc = |start: Vector2I, end: Vector2I, like: &ShapeArc| {
        let mut a = ShapeArc::new();
        a.construct_from_start_end_center(start, end, like.get_center(), like.is_clockwise(), 0.0);
        a
    };

    // Start a chain with 3 points
    let mut chain = ShapeLineChain::from_points(&[v(0, 0), v(0, 100000), v(100000, 0)], false);
    assert_eq!(chain.point_count(), 3);
    // Add first arc
    chain.append_arc_with_error(&first_arc, ARC_HIGH_DEF);
    assert_eq!(chain.point_count(), 10);
    // Add two points (target segment)
    chain.append_point(target_segment.a, false);
    chain.append_point(target_segment.b, false);
    assert_eq!(chain.point_count(), 12);
    // Add a second arc
    chain.append_arc_with_error(&second_arc, ARC_HIGH_DEF);
    assert_eq!(chain.point_count(), 20);
    assert!(is_outline_valid(&chain));

    // CASE 1: Start at arc endpoint, finish middle of arc
    {
        let slice_result = chain.slice_with_error(9, 18, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        let expected = expected_arc(second_arc.get_p0(), chain.get_point(18), &second_arc);

        assert_eq!(slice_result.arc(0).get_p0(), expected.get_p0()); // equal arc start points
        assert!(collides(slice_result.arc(0), expected.get_arc_mid()));
        assert!(collides(slice_result.arc(0), expected.get_p1()));

        assert_eq!(slice_result.point_count(), 10);
        assert_eq!(slice_result.get_point(0), first_arc.get_p1()); // equal to arc end
        assert_eq!(slice_result.get_point(1), target_segment.a);
        assert_eq!(slice_result.get_point(2), target_segment.b);
        assert_eq!(slice_result.get_point(3), expected.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(3));

        for i in 4..=8 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 3..=7 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(9));
        assert_eq!(slice_result.get_point(9), expected.get_p1()); // equal to arc end
    }

    // CASE 2: Start at middle of an arc, finish at arc startpoint
    {
        let slice_result = chain.slice_with_error(5, 12, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        let expected = expected_arc(chain.get_point(5), first_arc.get_p1(), &first_arc);

        assert_eq!(slice_result.arc(0).get_p1(), expected.get_p1()); // equal arc end points
        assert!(collides(slice_result.arc(0), expected.get_arc_mid()));
        assert!(collides(slice_result.arc(0), expected.get_p0()));

        assert_eq!(slice_result.point_count(), 8);
        assert_eq!(slice_result.get_point(0), expected.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=4 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=3 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(4));
        assert_eq!(slice_result.get_point(4), expected.get_p1()); // equal to arc end

        assert_eq!(slice_result.get_point(5), target_segment.a);
        assert_eq!(slice_result.get_point(6), target_segment.b);
        assert_eq!(slice_result.get_point(7), second_arc.get_p0());
    }

    // CASE 3: Full arc, nothing else
    {
        let slice_result = chain.slice_with_error(3, 9, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        let slice_arc0 = slice_result.arc(0).clone();

        // Equal arc to original inserted arc
        assert_eq!(first_arc.get_p1(), slice_arc0.get_p1());
        assert_eq!(first_arc.get_arc_mid(), slice_arc0.get_arc_mid());
        assert_eq!(first_arc.get_p1(), slice_arc0.get_p1());

        assert_eq!(slice_result.point_count(), 7);
        assert_eq!(slice_result.get_point(0), slice_arc0.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=6 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=5 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(6));
        assert_eq!(slice_result.get_point(6), slice_arc0.get_p1()); // equal to arc end
    }

    // CASE 4: Full arc, and straight segments to next arc start
    {
        let slice_result = chain.slice_with_error(3, 12, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        let slice_arc0 = slice_result.arc(0).clone();

        // Equal arc to original inserted arc
        assert_eq!(first_arc.get_p1(), slice_arc0.get_p1());
        assert_eq!(first_arc.get_arc_mid(), slice_arc0.get_arc_mid());
        assert_eq!(first_arc.get_p1(), slice_arc0.get_p1());

        assert_eq!(slice_result.point_count(), 10);
        assert_eq!(slice_result.get_point(0), slice_arc0.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=6 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=5 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(6));
        assert_eq!(slice_result.get_point(6), slice_arc0.get_p1()); // equal to arc end

        assert_eq!(slice_result.get_point(7), target_segment.a);
        assert_eq!(slice_result.get_point(8), target_segment.b);
        assert_eq!(slice_result.get_point(9), second_arc.get_p0());
    }

    // Case 5: Chain ends in arc and point
    {
        let mut chain_copy = chain.clone();
        chain_copy.append_point(v(400000, 400000), false);

        let slice_result = chain_copy.slice_with_error(11, -1, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));
        assert_eq!(slice_result.get_point(-1), v(400000, 400000));
    }

    // Case 6: Start to end, chain with one point
    {
        let chain_copy = SlcCases::new().one_point;

        let slice_result = chain_copy.slice_with_error(0, -1, ARC_HIGH_DEF);
        assert_eq!(slice_result.point_count(), 1);
        assert_eq!(slice_result.get_point(0), v(233450000, 228360000));
        assert_eq!(slice_result.get_point(-1), v(233450000, 228360000)); // Same as index 0
    }

    // Case 7: Start to end, chain with two points
    {
        let chain_copy = SlcCases::new().two_points;

        let slice_result = chain_copy.slice_with_error(0, -1, ARC_HIGH_DEF);
        assert_eq!(slice_result.point_count(), 2);
        assert_eq!(slice_result.get_point(0), v(233450000, 228360000));
        assert_eq!(slice_result.get_point(1), v(263450000, 258360000));
        assert_eq!(slice_result.get_point(-1), v(263450000, 258360000)); // Same as index 1
    }

    // Case 8: Full 2nd arc, nothing else
    {
        let slice_result = chain.slice_with_error(12, 19, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        let slice_arc0 = slice_result.arc(0).clone();

        // Equal arc to original inserted arc
        assert_eq!(second_arc.get_p1(), slice_arc0.get_p1());
        assert_eq!(second_arc.get_arc_mid(), slice_arc0.get_arc_mid());
        assert_eq!(second_arc.get_p1(), slice_arc0.get_p1());

        assert_eq!(slice_result.point_count(), 8);
        assert_eq!(slice_result.get_point(0), slice_arc0.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=7 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=6 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(7));
        assert_eq!(slice_result.get_point(7), slice_arc0.get_p1()); // equal to arc end
    }

    // Case 9: Start at middle of a 2nd arc, finish at end
    {
        let slice_result = chain.slice_with_error(16, 19, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);

        let expected = expected_arc(chain.get_point(16), second_arc.get_p1(), &second_arc);

        assert_eq!(slice_result.arc(0).get_p1(), expected.get_p1()); // equal arc end points
        assert!(collides(slice_result.arc(0), expected.get_arc_mid()));
        assert!(collides(slice_result.arc(0), expected.get_p0()));

        assert_eq!(slice_result.point_count(), 4);
        assert_eq!(slice_result.get_point(0), expected.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=3 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=2 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(3));
        assert_eq!(slice_result.get_point(3), expected.get_p1()); // equal to arc end
    }

    // Case 10: New chain, start at arc middle, finish at end
    {
        let mut chain10 = ShapeLineChain::new();
        chain10.append_arc_with_error(&first_arc, ARC_HIGH_DEF);

        let slice_result = chain10.slice_with_error(3, 6, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);

        let expected = expected_arc(chain10.get_point(3), first_arc.get_p1(), &first_arc);

        assert_eq!(slice_result.arc(0).get_p1(), expected.get_p1()); // equal arc end points
        assert!(collides(slice_result.arc(0), expected.get_arc_mid()));
        assert!(collides(slice_result.arc(0), expected.get_p0()));

        assert_eq!(slice_result.point_count(), 4);
        assert_eq!(slice_result.get_point(0), expected.get_p0()); // equal to arc start
        assert!(slice_result.is_arc_start(0));

        for i in 1..=3 {
            assert!(!slice_result.is_arc_start(i));
        }

        for i in 0..=2 {
            assert!(!slice_result.is_arc_end(i));
        }

        assert!(slice_result.is_arc_end(3));
        assert_eq!(slice_result.get_point(3), expected.get_p1()); // equal to arc end
    }

    // Case 11: Start and finish in the middle of the same arc
    {
        let slice_result = chain.slice_with_error(14, 16, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 1);
        assert_eq!(slice_result.point_count(), 3);

        assert_eq!(slice_result.arc(0).get_p0(), chain.get_point(14));
        assert_eq!(slice_result.arc(0).get_p1(), chain.get_point(16));
        assert!(collides(slice_result.arc(0), chain.get_point(15)));

        assert!(slice_result.is_arc_start(0));
        assert!(slice_result.is_arc_end(2));
    }

    // Case 12: Single point in the middle of an arc
    {
        let slice_result = chain.slice_with_error(15, 15, ARC_HIGH_DEF);
        assert!(is_outline_valid(&slice_result));

        assert_eq!(slice_result.arc_count(), 0);
        assert_eq!(slice_result.point_count(), 1);
        assert_eq!(slice_result.get_point(0), chain.get_point(15));
    }
}

// Test SHAPE_LINE_CHAIN::NearestPoint( VECTOR2I )
#[test]
fn nearest_point_pt() {
    let (_seg1, _seg2, arc, chain) = seg_arc_seg_chain();

    let pt_on_arc_close_to_start = v(297553, 31697); //should be index 3 in chain
    let pt_on_arc_close_to_end = v(139709, 82983); //should be index 6 in chain

    assert_eq!(
        chain.nearest_point(pt_on_arc_close_to_start, true),
        pt_on_arc_close_to_start
    );
    assert_eq!(
        chain.nearest_point(pt_on_arc_close_to_start, false),
        arc.get_p0()
    );

    assert_eq!(
        chain.nearest_point(pt_on_arc_close_to_end, true),
        pt_on_arc_close_to_end
    );
    assert_eq!(
        chain.nearest_point(pt_on_arc_close_to_end, false),
        arc.get_p1()
    );
}

// Test SHAPE_LINE_CHAIN::Replace( SHAPE_LINE_CHAIN )
#[test]
fn replace_chain() {
    // 8949 crash
    let line_pts: Vec<Vector2I> = [
        (206000000, 140110000),
        (192325020, 140110000),
        (192325020, 113348216),
        (192251784, 113274980),
        (175548216, 113274980),
        (175474980, 113348216),
        (175474980, 136694980),
        (160774511, 121994511),
        (160774511, 121693501),
        (160086499, 121005489),
        (159785489, 121005489),
        (159594511, 120814511),
        (160086499, 120814511),
        (160774511, 120126499),
        (160774511, 119153501),
        (160086499, 118465489),
        (159113501, 118465489),
        (158425489, 119153501),
        (158425489, 119645489),
        (157325020, 118545020),
        (157325020, 101925020),
        (208674980, 101925020),
        (208674980, 145474980),
        (192325020, 145474980),
        (192325020, 140110000),
    ]
    .iter()
    .map(|&(x, y)| v(x, y))
    .collect();

    let mut base_chain = ShapeLineChain::from_points(&line_pts, false);
    base_chain.set_width(250000);
    assert_eq!(base_chain.point_count() as usize, line_pts.len());

    let replace_chain = ShapeLineChain::from_points(&[v(192325020, 140110000)], false);
    assert_eq!(replace_chain.point_count(), 1);

    base_chain.replace_chain(1, 23, &replace_chain);

    assert_eq!(base_chain.point_count() as usize, line_pts.len() - (23 - 1));

    // Replacing the last point in a chain is special-cased
    let last = base_chain.point_count() - 1;
    base_chain.replace_point(last, last, v(-1, -1));

    assert_eq!(base_chain.c_last_point(), v(-1, -1));
}

#[test]
fn compare_geometry() {
    let mut chain1 = ShapeLineChain::new();
    chain1.append_xy(0, 0, false);
    chain1.append_xy(100, 0, false);
    chain1.append_xy(100, 100, false);
    chain1.append_xy(0, 100, false);
    chain1.set_closed(true);

    let mut chain2 = chain1.clone();

    // 1. Identical chains
    assert!(chain1.compare_geometry(&chain2, false, 0));

    // 2. Different chains
    chain2.set_point(2, v(101, 101));
    assert!(!chain1.compare_geometry(&chain2, false, 0));

    // 3. Epsilon tolerance
    assert!(chain1.compare_geometry(&chain2, false, 2));

    // 4. Cyclical compare (chain1 but points in started at different vertex)
    let mut chain3 = ShapeLineChain::new();
    chain3.append_xy(100, 0, false);
    chain3.append_xy(100, 100, false);
    chain3.append_xy(0, 100, false);
    chain3.append_xy(0, 0, false);
    chain3.set_closed(true);

    assert!(!chain1.compare_geometry(&chain3, false, 0));
    assert!(chain1.compare_geometry(&chain3, true, 0));

    // 5. Different number of points
    chain3.append_xy(50, 50, false); // Add a point
    assert!(!chain1.compare_geometry(&chain3, true, 0));

    // 6. Simplify check (chain1 should match chain4 because CompareGeometry calls Simplify())
    let mut chain4 = ShapeLineChain::new();
    chain4.append_xy(0, 0, false);
    chain4.append_xy(50, 0, false); // Collinear point
    chain4.append_xy(100, 0, false);
    chain4.append_xy(100, 100, false);
    chain4.append_xy(0, 100, false);
    chain4.set_closed(true);

    assert!(chain1.compare_geometry(&chain4, false, 0));
}

#[test]
fn compare_geometry_reversed() {
    // Square
    let pts_a = [v(0, 0), v(100, 0), v(100, 100), v(0, 100)];
    // Same points, same start, reversed
    let pts_b = [v(0, 0), v(0, 100), v(100, 100), v(100, 0)];

    let chain_a = ShapeLineChain::from_points(&pts_a, true);
    let chain_b = ShapeLineChain::from_points(&pts_b, true);

    assert!(!chain_a.compare_geometry(&chain_b, false, 0));
    assert!(chain_a.compare_geometry(&chain_b, true, 0));
}

/// Test for issue #22597: Simplify with tolerance should reduce a polygon
/// created from a rotated rounded rectangle (many small line segments approximating arcs).
/// This polygon has 164 points that form a rounded rectangle rotated 45 degrees.
#[test]
fn simplify_with_tolerance_issue22597() {
    let mut chain = ShapeLineChain::new();

    // All 164 points from the reproduction case in issue #22597.
    // Coordinates are in nanometers (KiCad internal units).
    #[rustfmt::skip]
    let pts: [(i32, i32); 164] = [
        (135095398, 233618441), (135024554, 233546880), (134887999, 233398857), (134757514, 233245455),
        (134633313, 233086923), (134515595, 232923519), (134404553, 232755507), (134300366, 232583161),
        (134203203, 232406759), (134113222, 232226587), (134030567, 232042939), (133955377, 231856111),
        (133887768, 231666408), (133827854, 231474135), (133775731, 231279607), (133731482, 231083137),
        (133695180, 230885045), (133666884, 230685652), (133646639, 230485281), (133634480, 230284257),
        (133630425, 230082907), (133634480, 229881557), (133646639, 229680533), (133666884, 229480162),
        (133695180, 229280769), (133731482, 229082677), (133775731, 228886207), (133827854, 228691679),
        (133887768, 228499406), (133955377, 228309703), (134030567, 228122875), (134113222, 227939227),
        (134203203, 227759055), (134300366, 227582653), (134404553, 227410307), (134515595, 227242295),
        (134633313, 227078891), (134757514, 226920359), (134887999, 226766957), (135024554, 226618934),
        (135095398, 226547373), (148530427, 213112344), (148601988, 213041500), (148750011, 212904945),
        (148903413, 212774460), (149061945, 212650259), (149225349, 212532541), (149393361, 212421499),
        (149565707, 212317312), (149742109, 212220149), (149922281, 212130168), (150105929, 212047514),
        (150292757, 211972323), (150482460, 211904715), (150674733, 211844800), (150869261, 211792677),
        (151065731, 211748428), (151263823, 211712126), (151463216, 211683830),
        (151710655, 211863478), // Suspicious point
        (151864611, 211651426), (152065961, 211647371), (152267311, 211651426), (152468335, 211663586),
        (152668706, 211683830), (152868099, 211712126), (153066191, 211748428), (153262661, 211792677),
        (153457189, 211844800), (153649462, 211904715), (153839165, 211972323), (154025993, 212047514),
        (154209641, 212130168), (154389813, 212220149), (154566215, 212317312), (154738561, 212421499),
        (154906573, 212532541), (155069977, 212650259), (155228509, 212774460), (155381911, 212904945),
        (155529934, 213041500), (155601495, 213112344), (160551242, 218062092), (160622086, 218133653),
        (160758641, 218281676), (160889126, 218435078), (161013327, 218593610), (161131045, 218757014),
        (161242087, 218925026), (161346274, 219097372), (161443437, 219273774), (161533418, 219453946),
        (161616072, 219637594), (161691263, 219824422), (161758871, 220014125), (161818786, 220206398),
        (161870909, 220400926), (161915158, 220597396), (161951460, 220795488), (161979756, 220994881),
        (162000000, 221195252), (162012160, 221396276), (162016215, 221597626), (162012160, 221798976),
        (162000000, 222000000), (161979756, 222200371), (161951460, 222399764), (161915158, 222597856),
        (161870909, 222794326), (161818786, 222988854), (161758871, 223181127), (161691263, 223370830),
        (161616072, 223557658), (161533418, 223741306), (161443437, 223921478), (161346274, 224097880),
        (161242087, 224270226), (161131045, 224438238), (161013327, 224601642), (160889126, 224760174),
        (160758641, 224913576), (160622086, 225061599), (160551242, 225133160), (147116213, 238568188),
        (147044657, 238639037), (146896633, 238775592), (146743231, 238906077), (146584699, 239030279),
        (146421295, 239147996), (146253283, 239259039), (146080936, 239363226), (145904534, 239460389),
        (145724362, 239550371), (145540714, 239633024), (145353886, 239708216), (145164182, 239775824),
        (144971909, 239835739), (144777380, 239887863), (144580910, 239932111), (144382818, 239968413),
        (144183424, 239996709), (143983053, 240016953), (143782029, 240029113), (143580679, 240033169),
        (143379329, 240029113), (143178305, 240016953), (142977934, 239996709), (142778540, 239968413),
        (142580448, 239932111), (142383978, 239887863), (142189449, 239835739), (141997176, 239775824),
        (141807472, 239708216), (141620644, 239633024), (141436996, 239550371), (141256824, 239460389),
        (141080422, 239363226), (140908075, 239259039), (140740063, 239147996), (140576659, 239030279),
        (140418127, 238906077), (140264725, 238775592), (140116701, 238639037), (140045145, 238568188),
    ];

    for (x, y) in pts {
        chain.append_point(v(x, y), false);
    }

    chain.set_closed(true);

    assert!(is_outline_valid(&chain));
    let original_point_count = chain.point_count();
    assert_eq!(original_point_count, 164);

    // With 2mm tolerance (2000000 nm), the many small segments approximating arcs
    // should be simplified significantly. A properly working algorithm should
    // reduce the point count substantially.
    chain.simplify(2000000);

    let simplified_count = chain.point_count();

    assert!(is_outline_valid(&chain));
    assert!(simplified_count < original_point_count);

    // The polygon is a rotated rounded rectangle with 4 corners.
    // A 2mm tolerance should reduce it to approximately 4-8 points.
    // If it's only slightly reduced, there may be an issue.
    assert!(
        simplified_count <= 20,
        "Simplified point count: {simplified_count}"
    );
}

#[test]
fn simplify_with_arcs() {
    let mm_arc =
        |s: (i32, i32), m: (i32, i32), e: (i32, i32)| arc3(v(s.0, s.1), v(m.0, m.1), v(e.0, e.1));

    // 1 segment, arc, 2 collinear segments
    {
        let mut original = ShapeLineChain::new();
        original.append_point(v(0, 0), false);
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (2500000, 500000), (3000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(4000000, 0), false);
        original.append_point(v(5000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert_eq!(simplified.arc_count(), original.arc_count());
        assert!(simplified.point_count() < before_points);
        assert_eq!(simplified.arc(0).get_p0(), v(2000000, 0));
        assert_eq!(simplified.arc(0).get_p1(), v(3000000, 0));

        assert!(simplified.find(v(4000000, 0), 0) < 0);
        assert!(simplified.find(v(3000000, 0), 0) >= 0);
    }

    // Arc, two collinear segments
    {
        let mut original = ShapeLineChain::new();
        original.append_arc_with_error(
            &mm_arc((0, 0), (1000000, 500000), (2000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(3000000, 0), false);
        original.append_point(v(4000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert_eq!(simplified.arc_count(), 1);
        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));

        assert!(simplified.find(v(3000000, 0), 0) < 0);
        assert_eq!(simplified.arc(0).get_p0(), v(0, 0));
        assert_eq!(simplified.arc(0).get_p1(), v(2000000, 0));
    }

    // 2 collinear segments, arc, 2 collinear segments
    {
        let mut original = ShapeLineChain::new();
        original.append_point(v(0, 0), false);
        original.append_point(v(1000000, 0), false);
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (2500000, 500000), (3000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(4000000, 0), false);
        original.append_point(v(5000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs);
        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));

        assert_eq!(simplified.arc(0).get_p0(), v(2000000, 0));
        assert_eq!(simplified.arc(0).get_p1(), v(3000000, 0));

        assert!(simplified.find(v(4000000, 0), 0) < 0);
        assert!(simplified.find(v(5000000, 0), 0) >= 0);
    }

    // 2 collinear segments, arc
    {
        let mut original = ShapeLineChain::new();
        original.append_point(v(0, 0), false);
        original.append_point(v(1000000, 0), false);
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (2500000, 500000), (3000000, 0)),
            ARC_HIGH_DEF,
        );

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert_eq!(simplified.arc_count(), 1);
        assert!(simplified.point_count() < before_points);
        assert!(simplified.find(v(1000000, 0), 0) < 0);
        assert!(is_outline_valid(&simplified));

        assert_eq!(simplified.arc(0).get_p0(), v(2000000, 0));
        assert_eq!(simplified.arc(0).get_p1(), v(3000000, 0));
    }

    // collinear segments before and after an arc (open chain)
    {
        let mut original = ShapeLineChain::new();
        original.append_point(v(0, 0), false);
        original.append_point(v(1000000, 0), false);
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (2500000, 500000), (3000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(4000000, 0), false);
        original.append_point(v(5000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs);

        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));

        assert_eq!(simplified.arc(0).get_p0(), v(2000000, 0));
        assert_eq!(simplified.arc(0).get_p1(), v(3000000, 0));

        assert!(simplified.find(v(4000000, 0), 0) < 0);
        assert!(simplified.find(v(5000000, 0), 0) >= 0);
    }

    // arc at start, two collinear segments after arc
    {
        let mut original = ShapeLineChain::new();
        original.append_arc_with_error(
            &mm_arc((0, 0), (1000000, 500000), (2000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(3000000, 0), false);
        original.append_point(v(4000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs || simplified.arc_count() == before_arcs - 1);
        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));
    }

    // tolerance semantics - zero vs small positive
    {
        let mut original = ShapeLineChain::new();

        original.append_point(v(0, 0), false);
        // perturb the intermediate point by 1 unit so it's not exactly collinear
        original.append_point(v(1000000, 1), false);
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (2500000, 500000), (3000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(4000000, 0), false);
        original.append_point(v(5000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();

        let mut simplified_zero = original.clone();
        simplified_zero.simplify(0);

        // Expect a single point removal at zero tolerance
        assert_eq!(simplified_zero.point_count(), before_points - 1);

        let mut simplified_one = original.clone();
        simplified_one.simplify(1);

        // Expect one more point to be removed with tolerance 1
        assert_eq!(simplified_one.point_count(), before_points - 2);
        assert_eq!(simplified_one.arc_count(), original.arc_count());
    }

    // arc, arc (adjacent)
    {
        let mut original = ShapeLineChain::new();
        original.append_arc_with_error(
            &mm_arc((0, 0), (1000000, 500000), (2000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_arc_with_error(
            &mm_arc((2000000, 0), (3000000, 500000), (4000000, 0)),
            ARC_HIGH_DEF,
        );

        assert!(is_outline_valid(&original));
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs);
        assert!(is_outline_valid(&simplified));

        assert_eq!(simplified.arc(0).get_p1(), simplified.arc(1).get_p0());
        assert_eq!(simplified.arc(0).get_p0(), v(0, 0));
        assert_eq!(simplified.arc(1).get_p1(), v(4000000, 0));
    }

    // 2 collinear segments, arc, 2 collinear segments
    {
        let mut original = ShapeLineChain::new();
        original.append_point(v(-1000000, 0), false);
        original.append_arc_with_error(
            &mm_arc((0, 0), (500000, 500000), (1000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_arc_with_error(
            &mm_arc((1000000, 0), (1500000, -500000), (2000000, 0)),
            ARC_HIGH_DEF,
        );
        original.append_point(v(3000000, 0), false);
        original.append_point(v(4000000, 0), false);

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs);
        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));

        assert!(simplified.find(v(-1000000, 0), 0) >= 0);
        assert!(simplified.find(v(4000000, 0), 0) >= 0);
    }

    // arc, collinear point, arc
    {
        let mut original = ShapeLineChain::new();
        original.append_arc_with_error(
            &mm_arc((0, 0), (1000000, 500000), (2000000, 0)),
            ARC_HIGH_DEF,
        );

        // collinear point
        original.append_point(v(2500000, 0), false);

        original.append_arc_with_error(
            &mm_arc((3000000, 0), (3500000, 500000), (4000000, 0)),
            ARC_HIGH_DEF,
        );

        assert!(is_outline_valid(&original));
        let before_points = original.point_count();
        let before_arcs = original.arc_count();

        let mut simplified = original.clone();
        simplified.simplify(0);

        assert!(simplified.arc_count() == before_arcs);
        assert!(simplified.point_count() < before_points);
        assert!(is_outline_valid(&simplified));

        assert_eq!(simplified.arc(0).get_p1(), v(2000000, 0));
        assert_eq!(simplified.arc(1).get_p0(), v(3000000, 0));
    }
}

fn chain_of(pts: &[(i32, i32)]) -> ShapeLineChain {
    let pts: Vec<Vector2I> = pts.iter().map(|&(x, y)| v(x, y)).collect();
    ShapeLineChain::from_points(&pts, false)
}

#[test]
fn self_intersecting_no_intersection_open_chain() {
    let chain = chain_of(&[(0, 0), (1000, 0), (2000, 1000), (3000, 0)]);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_no_intersection_closed_chain() {
    let mut chain = chain_of(&[(0, 0), (10000, 0), (10000, 10000), (0, 10000)]);
    chain.set_closed(true);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_crossing_segments() {
    // An X shape: two segments that cross
    //   (0,0)-(10000,10000) and (10000,0)-(0,10000)
    let chain = chain_of(&[(0, 0), (10000, 10000), (10000, 0), (0, 10000)]);

    let result = chain.self_intersecting().expect("has value");
    assert_eq!(result.index_our, 0);
    assert_eq!(result.index_their, 2);
}

#[test]
fn self_intersecting_closed_figure_eight() {
    // Closed bowtie: segments (0,0)-(10000,10000) and (10000,0)-(0,10000) cross at center
    let mut chain = chain_of(&[(0, 0), (10000, 10000), (10000, 0), (0, 10000)]);
    chain.set_closed(true);

    assert!(chain.self_intersecting().is_some());
}

#[test]
fn self_intersecting_vertex_on_segment() {
    // Third vertex lies exactly on the first segment
    let chain = chain_of(&[
        (0, 0),
        (20000, 0),
        (20000, 10000),
        (10000, 0),
        (10000, -10000),
    ]);

    let result = chain.self_intersecting().expect("has value");
    assert_eq!(result.p.x, 10000);
    assert_eq!(result.p.y, 0);
}

#[test]
fn self_intersecting_two_segments() {
    let chain = chain_of(&[(0, 0), (10000, 0)]);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_single_point() {
    let mut chain = ShapeLineChain::new();
    chain.append_point(v(0, 0), false);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_adjacent_segments_ignored() {
    // A simple zigzag where adjacent segments share endpoints but don't self-intersect
    let chain = chain_of(&[
        (0, 0),
        (5000, 10000),
        (10000, 0),
        (15000, 10000),
        (20000, 0),
    ]);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_closed_triangle() {
    let mut chain = chain_of(&[(0, 0), (10000, 0), (5000, 10000)]);
    chain.set_closed(true);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_closed_last_first_not_false_positive() {
    // Closed rectangle. The last segment shares its endpoint with the first segment.
    // This must NOT be reported as a self-intersection.
    let mut chain = chain_of(&[(0, 0), (10000, 0), (10000, 10000), (0, 10000)]);
    chain.set_closed(true);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_spatially_distant() {
    // Segments are far apart spatially, exercising the bbox rejection path
    let chain = chain_of(&[
        (0, 0),
        (1000, 0),
        (1000, 1000000),
        (2000, 1000000),
        (2000, 2000000),
        (3000, 2000000),
    ]);

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_large_non_intersecting() {
    // Build a spiral-like chain with many segments that don't self-intersect
    let mut chain = ShapeLineChain::new();

    for i in 0..200 {
        chain.append_point(v(i * 1000, (i % 2) * 5000), false);
    }

    assert!(chain.self_intersecting().is_none());
}

#[test]
fn self_intersecting_large_with_crossing() {
    // Many non-intersecting segments, then one that crosses back over an earlier one
    let mut chain = ShapeLineChain::new();

    for i in 0..50 {
        chain.append_point(v(i * 1000, 0), false);
    }

    // Last segment crosses back over the first few segments
    chain.append_point(v(5000, 10000), false);
    chain.append_point(v(5000, -10000), false);

    assert!(chain.self_intersecting().is_some());
}

// The router collides a one-point head line while placing, which leaves no segment to measure.
// (The C++ test arms an assert thrower; overflow checks of the test build play that role here.)
#[test]
fn collide_seg_single_point_chain() {
    let mut chain = ShapeLineChain::new();
    chain.append_point(v(23025000, 9475000), false);

    let seg = Seg::new(v(24800000, 4650000), v(28520000, 8370000));

    let hit = chain.collide_seg(&seg, 1179499, None, None);

    assert!(!hit);
}

// --- not in the C++ test file: the parts of the header without a ported C++ test ---

#[test]
fn parse_format_round_trip() {
    let mut chain = chain_of(&[(0, 0), (1000, 0), (1000, 2000)]);
    chain.set_closed(true);

    assert_eq!(
        Shape::format(&chain, true),
        "SHAPE_LINE_CHAIN( { VECTOR2I( 0, 0), VECTOR2I( 1000, 0), VECTOR2I( 1000, 2000)}, true );"
    );

    let text = "3 1 1 0 0 -1 1000 0 0 1000 2000 0 500 0 0 0 90.0";
    let mut ss = StringStream::new(text);
    let mut parsed = ShapeLineChain::new();
    assert!(parsed.parse(&mut ss));
    assert_eq!(parsed.point_count(), 3);
    assert!(parsed.is_closed());
    assert_eq!(parsed.arc_count(), 1);
    assert_eq!(parsed.c_shapes()[1], (0, -1));
    assert_eq!(
        parsed.arc(0).get_p0(),
        ShapeArc::from_center_start_angle(v(500, 0), v(0, 0), EdaAngle::new(90.0, DEGREES_T), 0)
            .get_p0()
    );

    let mut bad = StringStream::new("100000 1 0");
    assert!(!ShapeLineChain::new().parse(&mut bad));
}

#[test]
fn clipper2_round_trip() {
    let mut chain = ShapeLineChain::new();
    chain.append_arc_with_error(
        &ShapeArc::from_center_start_angle(v(0, 0), v(1000000, 0), ANGLE_360, 0),
        ARC_HIGH_DEF,
    );
    chain.set_closed(true);

    let mut z = Vec::new();
    let mut arcs = Vec::new();
    let mut path = chain.convert_to_clipper2(true, &mut z, &mut arcs);
    path.rotate_left(3);
    let back = ShapeLineChain::from_clipper2(&path, &z, &arcs);

    assert!(is_outline_valid(&back));
    assert_eq!(back.point_count(), chain.point_count());
    assert_eq!(back.arc_count(), 1);
    assert!(back.is_arc_start(0));
}

// --- test_shape_line_chain_collision.cpp: these go through SHAPE::Collide( SHAPE* ), i.e.
// the collision matrix (package G5) ---

#[test]
fn collide_line_to_line() {
    let line_a = chain_of(&[(0, 0), (10, 0)]);
    let line_b = chain_of(&[(5, 5), (5, -5)]);

    let mut location = Vector2I::default();
    let mut actual = 0;
    let collided = line_a.collide(&line_b, 0, Some(&mut actual), Some(&mut location));

    assert!(collided);
    assert_eq!(actual, 0);
    assert_eq!(location, v(5, 0));
}

#[test]
fn collide_line_to_arc() {
    let line_a = chain_of(&[(0, 0), (10, 0)]);
    let mut arc_b = ShapeLineChain::new();
    arc_b.append_arc(&arc3(v(5, 5), v(6, 4), v(7, 0)));

    let mut location = Vector2I::default();
    let mut actual = 0;
    let collided = line_a.collide(&arc_b, 0, Some(&mut actual), Some(&mut location));

    assert!(collided);
    assert_eq!(actual, 0);
    assert_eq!(location, v(7, 0));
}

#[test]
fn collide_arc_to_arc() {
    let mut arc_a = ShapeLineChain::new();
    arc_a.append_arc(&arc3(v(0, 0), v(10, 0), v(5, 5)));
    let mut arc_b = ShapeLineChain::new();
    arc_b.append_arc(&arc3(v(5, 5), v(5, -5), v(10, 0)));

    let mut location = Vector2I::default();
    let mut actual = 0;
    let collided = arc_a.collide(&arc_b, 0, Some(&mut actual), Some(&mut location));

    assert!(collided);
    assert_eq!(actual, 0);
    assert_eq!(location, v(5, 5));
}

#[test]
fn collide_with_clearance() {
    let line_a = chain_of(&[(0, 0), (10, 0)]);
    let line_b = chain_of(&[(5, 6), (-5, 6)]);

    let mut location = Vector2I::default();
    let mut actual = 0;
    let collided = line_a.collide(&line_b, 7, Some(&mut actual), Some(&mut location));

    assert!(collided);
    assert_eq!(actual, 6);
    assert_eq!(location, v(0, 0));
}

#[test]
fn collide_no_clearance() {
    let line_a = chain_of(&[(0, 0), (10, 0)]);
    let line_b = chain_of(&[(5, 6), (-5, 6)]);

    let mut location = Vector2I::default();
    let mut actual = 0;
    let collided = line_a.collide(&line_b, 0, Some(&mut actual), Some(&mut location));

    assert!(!collided);
    assert_eq!(actual, 0);
}

// --- test_shape_line_chain_collision.cpp: the corpus tests (indexed vs. reference algorithm) ---

/// Sentinels far outside the fixtures' range, so an untouched output cannot look like a real
/// one
const NO_ACTUAL: i32 = 123456789;
const NO_LOCATION: Vector2I = Vector2I::new(123456789, -123456789);

const WANT_NOTHING: i32 = 0;
const WANT_ACTUAL: i32 = 1;
const WANT_LOCATION: i32 = 2;
const WANT_BOTH: i32 = 3;

/// `loadZone()`: the polyset of zone `a_zone` of a file of `qa/data/triangulation`.
fn load_zone(a_filename: &str, a_zone: i32) -> kicad_geom::geometry::shape_poly_set::ShapePolySet {
    let path = kicad_qa::qa_data_dir()
        .join("triangulation")
        .join(a_filename);
    let data = std::fs::read_to_string(&path).expect("Unable to open zone file");
    let mut pos = 0;

    for _ in 0..=a_zone {
        pos += data[pos..].find("(zone (layer \"").expect("Missing zone");
        pos += 1;
    }

    pos += data[pos..].find("polyset ").expect("Missing polyset");

    // Parse consumes only the polyset expression, so handing it the rest of the file is
    // harmless
    let mut serialized = StringStream::new(&data[pos..]);
    let mut result = kicad_geom::geometry::shape_poly_set::ShapePolySet::new();
    assert!(result.parse(&mut serialized), "Unable to parse zone");

    result
}

/// `extractSubchain()`
fn extract_subchain(
    a_source: &ShapeLineChain,
    a_start: i32,
    a_segment_count: i32,
) -> ShapeLineChain {
    let mut result = ShapeLineChain::new();
    result.append_point(a_source.c_segment(a_start).a, false);

    for i in 0..a_segment_count {
        result.append_point(a_source.c_segment(a_start + i).b, true);
    }

    result
}

#[derive(Debug, PartialEq)]
struct CollisionOutput {
    result: bool,
    actual: i32,
    location: Vector2I,
}

/// `referenceCollision()`: the pre-index algorithm, which walked every segment pair in sorted
/// order.  Valid only for arc-free chains, since it skips the arc refinement pass.
fn reference_collision(
    a_a: &ShapeLineChain,
    a_b: &ShapeLineChain,
    a_clearance: i32,
    a_outputs: i32,
) -> CollisionOutput {
    let mut closest = i32::MAX;
    let mut nearest = Vector2I::default();

    if a_b.is_closed() && a_a.get_point_count() > 0 && a_b.point_inside(a_a.get_point(0), 0, false)
    {
        closest = 0;
        nearest = a_a.get_point(0);
    } else if a_a.is_closed()
        && a_b.get_point_count() > 0
        && a_a.point_inside(a_b.get_point(0), 0, false)
    {
        closest = 0;
        nearest = a_b.get_point(0);
    } else if a_clearance >= 0 {
        let mut a_segments: Vec<Seg> = (0..a_a.segment_count()).map(|i| a_a.c_segment(i)).collect();
        let mut b_segments: Vec<Seg> = (0..a_b.segment_count()).map(|i| a_b.c_segment(i)).collect();

        let segment_sort = |a_first: &Seg, a_second: &Seg| {
            a_first.a.x < a_second.a.x
                || (a_first.a.x == a_second.a.x && a_first.a.y < a_second.a.y)
        };

        kicad_geom::stdsort::std_sort(&mut a_segments, segment_sort);
        kicad_geom::stdsort::std_sort(&mut b_segments, segment_sort);

        for a in &a_segments {
            for b in &b_segments {
                let (collides, distance) = a.collide_actual(b, a_clearance);

                if collides {
                    if distance < closest {
                        nearest = a.nearest_point_seg(b);
                        closest = distance;
                    }

                    if closest == 0 || (a_outputs & WANT_ACTUAL) == 0 {
                        break;
                    }
                }
            }
        }
    }

    let mut result = CollisionOutput {
        result: false,
        actual: NO_ACTUAL,
        location: NO_LOCATION,
    };

    if closest == 0 || closest < a_clearance {
        result.result = true;

        if a_outputs & WANT_ACTUAL != 0 {
            result.actual = closest;
        }

        if a_outputs & WANT_LOCATION != 0 {
            result.location = nearest;
        }
    }

    result
}

/// `indexedCollision()`
fn indexed_collision(
    a_a: &ShapeLineChain,
    a_b: &ShapeLineChain,
    a_clearance: i32,
    a_outputs: i32,
) -> CollisionOutput {
    let mut actual = NO_ACTUAL;
    let mut location = NO_LOCATION;
    let result = (a_a as &dyn Shape).collide(
        a_b,
        a_clearance,
        if a_outputs & WANT_ACTUAL != 0 {
            Some(&mut actual)
        } else {
            None
        },
        if a_outputs & WANT_LOCATION != 0 {
            Some(&mut location)
        } else {
            None
        },
    );

    CollisionOutput {
        result,
        actual,
        location,
    }
}

/// `checkCollisionParity()`
fn check_collision_parity(a_a: &ShapeLineChain, a_b: &ShapeLineChain, a_clearance: i32) {
    for outputs in WANT_NOTHING..=WANT_BOTH {
        let expected = reference_collision(a_a, a_b, a_clearance, outputs);
        let actual = indexed_collision(a_a, a_b, a_clearance, outputs);

        assert_eq!(actual.result, expected.result);
        assert_eq!(actual.actual, expected.actual);
        assert_eq!(actual.location, expected.location);
    }
}

#[test]
fn collide_real_contour_parity() {
    let cases = [
        ("One-Air-Max.kicad_polys", 18, 0, 3),
        ("issue5093.kicad_polys", 3, 0, 1),
        ("bad_triangulation_case.kicad_polys", 47, 0, 3),
    ];

    for (file, zone, outline_a, outline_b) in cases {
        let poly = load_zone(file, zone);
        assert!(poly.outline_count() > outline_a.max(outline_b));

        let a = poly.c_outline(outline_a);
        let b = poly.c_outline(outline_b);
        assert_eq!(a.arc_count(), 0);
        assert_eq!(b.arc_count(), 0);

        for clearance in [-1, 0, 200000, 2000000] {
            check_collision_parity(a, b, clearance);
            check_collision_parity(b, a, clearance);
        }
    }
}

#[test]
fn collide_real_contour_gate_boundaries() {
    let poly = load_zone("issue5093.kicad_polys", 3);
    let source_a = poly.c_outline(0);
    let source_b = poly.c_outline(1);
    // Sizes straddle the output gate of A>=32, B>=64 and 4096 pairs, so 31x133 stays direct
    // while 66x63 and 63x65 cross it in one argument order only
    let sizes = [(31, 133), (32, 128), (66, 63), (63, 65), (64, 64)];
    assert_eq!(source_a.segment_count(), 265);
    assert_eq!(source_b.segment_count(), 307);

    for (count_a, count_b) in sizes {
        for start_a in [0, source_a.segment_count() - count_a] {
            let a = extract_subchain(source_a, start_a, count_a);
            let b = extract_subchain(source_b, 0, count_b);

            for clearance in [-1, 0, 200000, 2000000] {
                check_collision_parity(&a, &b, clearance);
                check_collision_parity(&b, &a, clearance);
            }
        }
    }
}

#[test]
fn collide_real_contour_shared_segments() {
    let poly = load_zone("issue5093.kicad_polys", 3);
    let source = poly.c_outline(0);

    // Overlapping subchains share 32 segments, which drives the zero-distance exact-hit exit
    let a = extract_subchain(source, 0, 64);
    let b = extract_subchain(source, 32, 128);

    check_collision_parity(&a, &b, 0);
    check_collision_parity(&b, &a, 0);
}

#[test]
fn segment_index_real_contour_candidate_set() {
    let poly = load_zone("One-Air-Max.kicad_polys", 18);
    let contour = poly.c_outline(0);
    let segments: Vec<Seg> = (0..contour.segment_count())
        .map(|i| contour.c_segment(i))
        .collect();

    // Enough segments to force a multi-level tree rather than a single leaf
    let index = kicad_geom::geometry::segment_index::SegmentIndex::new(segments.clone());
    assert!(index.size() > 256);

    for query_index in (0..segments.len()).step_by(17) {
        let query = &segments[query_index];

        for padding in [0, 200000, 2000000] {
            let mut expected = Vec::new();
            let min_x = query.a.x.min(query.b.x) as i64 - padding as i64;
            let min_y = query.a.y.min(query.b.y) as i64 - padding as i64;
            let max_x = query.a.x.max(query.b.x) as i64 + padding as i64;
            let max_y = query.a.y.max(query.b.y) as i64 + padding as i64;

            for (i, segment) in segments.iter().enumerate() {
                if segment.a.x.max(segment.b.x) as i64 >= min_x
                    && segment.a.x.min(segment.b.x) as i64 <= max_x
                    && segment.a.y.max(segment.b.y) as i64 >= min_y
                    && segment.a.y.min(segment.b.y) as i64 <= max_y
                {
                    expected.push(i as i32);
                }
            }

            let mut actual = Vec::new();
            index.visit_candidates(query, padding, |a_item| {
                actual.push(a_item);
                true
            });
            actual.sort();
            assert_eq!(actual, expected);
        }
    }

    let mut calls = 0;
    index.visit_candidates(&segments[0], i32::MAX, |_| {
        calls += 1;
        false
    });
    assert_eq!(calls, 1);
}
