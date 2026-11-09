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

//! Port of `qa/tests/libs/kimath/geometry/test_chamfer.cpp` (suite `Chamfer`).

use kicad_geom::geometry::corner_operations::{ChamferParams, ChamferResult, compute_chamfer_points};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::segments_have_same_end_points;

fn seg(ax: i32, ay: i32, bx: i32, by: i32) -> Seg {
    Seg::new(Vector2I::new(ax, ay), Vector2I::new(bx, by))
}

struct TwoLineChamferTestCase {
    seg_a: Seg,
    seg_b: Seg,
    params: ChamferParams,
    expected_result: Option<ChamferResult>,
}

fn do_chamfer_test_checks(a_test_case: &TwoLineChamferTestCase) {
    // Actally do the chamfer
    let chamfer_result = compute_chamfer_points(&a_test_case.seg_a, &a_test_case.seg_b, &a_test_case.params);

    assert_eq!(chamfer_result.is_some(), a_test_case.expected_result.is_some());

    if let Some(actual_result) = chamfer_result {
        let expected_result = a_test_case.expected_result.unwrap();

        assert!(segments_have_same_end_points(&actual_result.chamfer, &expected_result.chamfer));

        let check_updated_seg = |updated_seg: Option<Seg>, expected_seg: Option<Seg>| {
            assert_eq!(updated_seg.is_some(), expected_seg.is_some());

            if let (Some(u), Some(e)) = (updated_seg, expected_seg) {
                assert!(segments_have_same_end_points(&u, &e));
            }
        };

        check_updated_seg(actual_result.updated_seg_a, expected_result.updated_seg_a);
        check_updated_seg(actual_result.updated_seg_b, expected_result.updated_seg_b);
    }
}

fn params(a: i32, b: i32) -> ChamferParams {
    ChamferParams { chamfer_setback_a: a, chamfer_setback_b: b }
}

#[test]
fn simple_chamfer_at_origin() {
    do_chamfer_test_checks(&TwoLineChamferTestCase {
        seg_a: seg(0, 0, 1000, 0),
        seg_b: seg(0, 0, 0, 1000),
        params: params(10, 10),
        expected_result: Some(ChamferResult {
            chamfer: seg(10, 0, 0, 10),                 // chamfer
            updated_seg_a: Some(seg(10, 0, 1000, 0)), // rest of the line A
            updated_seg_b: Some(seg(0, 10, 0, 1000)), // rest of the line B
        }),
    });
}

#[test]
fn simple_chamfer_not_at_origin() {
    // Same as above but the intersection is not at the origin
    do_chamfer_test_checks(&TwoLineChamferTestCase {
        seg_a: seg(1000, 1000, 2000, 1000),
        seg_b: seg(1000, 1000, 1000, 2000),
        params: params(10, 10),
        expected_result: Some(ChamferResult {
            chamfer: seg(1010, 1000, 1000, 1010),
            updated_seg_a: Some(seg(1010, 1000, 2000, 1000)),
            updated_seg_b: Some(seg(1000, 1010, 1000, 2000)),
        }),
    });
}

#[test]
fn asymmetric_chamfer() {
    do_chamfer_test_checks(&TwoLineChamferTestCase {
        seg_a: seg(0, 0, 1000, 0),
        seg_b: seg(0, 0, 0, 1000),
        params: params(10, 100),
        expected_result: Some(ChamferResult {
            chamfer: seg(10, 0, 0, 100),
            updated_seg_a: Some(seg(10, 0, 1000, 0)),
            updated_seg_b: Some(seg(0, 100, 0, 1000)),
        }),
    });
}

#[test]
fn chamfer_full_length() {
    // Chamfer consumes the entire length of a line
    do_chamfer_test_checks(&TwoLineChamferTestCase {
        seg_a: seg(0, 0, 1000, 0),
        seg_b: seg(0, 0, 0, 100),
        params: params(100, 100),
        expected_result: Some(ChamferResult {
            chamfer: seg(100, 0, 0, 100),
            updated_seg_a: Some(seg(100, 0, 1000, 0)),
            updated_seg_b: None, // line b no longer exists
        }),
    });
}

#[test]
fn chamfer_over_full_length() {
    // Chamfer consumes the entire length of a line
    do_chamfer_test_checks(&TwoLineChamferTestCase {
        seg_a: seg(0, 0, 1000, 0),
        seg_b: seg(0, 0, 0, 100),
        params: params(150, 150), // > 100
        expected_result: None,
    });
}
