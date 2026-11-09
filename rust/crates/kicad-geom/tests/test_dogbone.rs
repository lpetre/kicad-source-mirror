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

//! Port of `qa/tests/libs/kimath/geometry/test_dogbone.cpp` (suite `Dogbone`).

use kicad_geom::geometry::corner_operations::{DogboneResult, compute_dogbone};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::segments_have_same_end_points;

fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

struct DogboneTestCase {
    seg_a: Seg,
    seg_b: Seg,
    radius: i32,
    add_slots: bool,
    expected_result: Option<DogboneResult>,
}

fn do_dogbone_test_checks(a_test_case: &DogboneTestCase) {
    // Actally do the chamfer
    let dogbone_result = compute_dogbone(
        &a_test_case.seg_a,
        &a_test_case.seg_b,
        a_test_case.radius,
        a_test_case.add_slots,
    );

    assert_eq!(
        dogbone_result.is_some(),
        a_test_case.expected_result.is_some()
    );

    if let Some(actual_result) = dogbone_result {
        let expected_result = a_test_case.expected_result.as_ref().unwrap();

        let expected_arc_chord =
            Seg::new(expected_result.arc.get_p0(), expected_result.arc.get_p1());
        let actual_arc_chord = Seg::new(actual_result.arc.get_p0(), actual_result.arc.get_p1());

        assert!(segments_have_same_end_points(
            &actual_arc_chord,
            &expected_arc_chord
        ));
        assert_eq!(
            actual_result.arc.get_arc_mid(),
            expected_result.arc.get_arc_mid()
        );

        let check_updated_seg = |updated_seg: Option<Seg>, expected_seg: Option<Seg>| {
            assert_eq!(updated_seg.is_some(), expected_seg.is_some());

            if let (Some(u), Some(e)) = (updated_seg, expected_seg) {
                assert!(segments_have_same_end_points(&u, &e));
            }
        };

        check_updated_seg(actual_result.updated_seg_a, expected_result.updated_seg_a);
        check_updated_seg(actual_result.updated_seg_b, expected_result.updated_seg_b);

        assert_eq!(
            actual_result.small_arc_mouth,
            expected_result.small_arc_mouth
        );
    }
}

#[test]
fn simple_right_angle_at_origin() {
    do_dogbone_test_checks(&DogboneTestCase {
        seg_a: Seg::new(pt(0, 0), pt(100000, 0)),
        seg_b: Seg::new(pt(0, 0), pt(0, 100000)),
        radius: 10000,
        add_slots: false,
        // A right angle is an easy one to see, because the end and center points are
        // all on 45 degree lines from the center
        expected_result: Some(DogboneResult {
            arc: ShapeArc::from_start_mid_end(pt(14142, 0), pt(0, 0), pt(0, 14142), 0),
            updated_seg_a: Some(Seg::new(pt(14142, 0), pt(100000, 0))),
            updated_seg_b: Some(Seg::new(pt(0, 14142), pt(0, 100000))),
            small_arc_mouth: false,
        }),
    });
}
