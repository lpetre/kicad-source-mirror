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

//! Port of `qa/tests/libs/kimath/geometry/test_segment.cpp` (Boost test suite `Segment`).
//!
//! Each `BOOST_DATA_TEST_CASE` over a table of named cases becomes one `#[test]` that loops
//! over the same table; the case name is part of the assertion message.

use kicad_geom::geometry::seg::{Ecoord, Seg};
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{Mt19937, uniform_int_libstdcxx, uniform_real_libstdcxx};

/// Shorthand for the brace initialisation `{ { ax, ay }, { bx, by } }` of a `SEG`.
const fn seg(ax: i32, ay: i32, bx: i32, by: i32) -> Seg {
    Seg::new(Vector2I::new(ax, ay), Vector2I::new(bx, by))
}

/// Shorthand for the brace initialisation `{ x, y }` of a `VECTOR2I`.
const fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

/// Port of `SegCollideCorrect`.
///
/// Predicate to check expected collision between two segments
///
/// * `a_seg_a` - the first #SEG
/// * `a_seg_b` - the second #SEG
/// * `a_clearance` - the collision clearance
/// * `a_exp` - expected collision
///
/// Returns: does the distance calculated agree?
fn seg_collide_correct(a_seg_a: &Seg, a_seg_b: &Seg, a_clearance: i32, a_exp: bool) -> bool {
    let a_to_b: bool = a_seg_a.collide(a_seg_b, a_clearance);
    let b_to_a: bool = a_seg_b.collide(a_seg_a, a_clearance);

    let ok: bool = (a_to_b == a_exp) && (b_to_a == a_exp);

    if a_to_b != b_to_a {
        eprintln!(
            "Segment collision is not the same in both directions: expected {a_exp}, got {a_to_b} & {b_to_a}"
        );
    } else if !ok {
        eprintln!("Collision incorrect: expected {a_exp}, got {a_to_b}");
    }

    ok
}

/// Port of `SegDistanceCorrect`.
///
/// Predicate to check expected distance between two segments
///
/// * `a_seg_a` - the first #SEG
/// * `a_seg_b` - the second #SEG
/// * `a_exp` - expected distance
///
/// Returns: does the distance calculated agree?
fn seg_distance_correct(a_seg_a: &Seg, a_seg_b: &Seg, a_exp: i32) -> bool {
    let a_to_b: i32 = a_seg_a.distance_seg(a_seg_b);
    let b_to_a: i32 = a_seg_b.distance_seg(a_seg_a);

    let mut ok: bool = (a_to_b == a_exp) && (b_to_a == a_exp);

    if a_to_b != b_to_a {
        eprintln!(
            "Segment distance is not the same in both directions: expected {a_exp}, got {a_to_b} & {b_to_a}"
        );
    } else if !ok {
        eprintln!("Distance incorrect: expected {a_exp}, got {a_to_b}");
    }

    // Sanity check: the collision should be consistent with the distance
    ok = ok && seg_collide_correct(a_seg_a, a_seg_b, 0, a_exp == 0);

    ok
}

/// Port of `SegVecDistanceCorrect`.
///
/// Predicate to check expected distance between a segment and a point
///
/// * `a_seg` - the segment
/// * `a_vec` - the vector (point)
/// * `a_exp` - expected distance
///
/// Returns: does the distance calculated agree?
fn seg_vec_distance_correct(a_seg: &Seg, a_vec: Vector2I, a_exp: i32) -> bool {
    let squared_distance: Ecoord = a_seg.squared_distance(a_vec);
    assert!(squared_distance >= 0);

    let dist: i32 = a_seg.distance(a_vec);

    let ok: bool = dist == a_exp;

    if !ok {
        eprintln!("Distance incorrect: expected {a_exp}, got {dist}");
    }

    ok
}

/// Port of `SegCollinearCorrect`.
///
/// Predicate to check expected collision between two segments
///
/// * `a_seg_a` - the first #SEG
/// * `a_seg_b` - the second #SEG
/// * `a_exp` - expected collinearity
///
/// Returns: does the collinearity calculated agree?
fn seg_collinear_correct(a_seg_a: &Seg, a_seg_b: &Seg, a_exp: bool) -> bool {
    let a_to_b: bool = a_seg_a.collinear(a_seg_b);
    let b_to_a: bool = a_seg_b.collinear(a_seg_a);

    let ok: bool = (a_to_b == a_exp) && (b_to_a == a_exp);

    if a_to_b != b_to_a {
        eprintln!(
            "Segment collinearity is not the same in both directions: expected {a_exp}, got {a_to_b} & {b_to_a}"
        );
    } else if !ok {
        eprintln!("Collinearity incorrect: expected {a_exp}, got {a_to_b}");
    }

    ok
}

/// Port of `SegParallelCorrect`.
///
/// Predicate to check expected parallelism between two segments
///
/// * `a_seg_a` - the first #SEG
/// * `a_seg_b` - the second #SEG
/// * `a_exp` - expected parallelism: true = segments are parallel
///   false = segments are not parallel
///
/// Returns: does the parallelism calculated agree?
fn seg_parallel_correct(a_seg_a: &Seg, a_seg_b: &Seg, a_exp: bool) -> bool {
    let a_to_b: bool = a_seg_a.approx_parallel(a_seg_b);
    let b_to_a: bool = a_seg_b.approx_parallel(a_seg_a);

    let ok: bool = (a_to_b == a_exp) && (b_to_a == a_exp);

    if a_to_b != b_to_a {
        eprintln!(
            "Segment parallelism is not the same in both directions: expected {a_exp}, got AtoB: {a_to_b} BtoA:{b_to_a}"
        );
    } else if !ok {
        eprintln!("Parallelism incorrect: expected {a_exp}, got {a_to_b}");
    }

    ok
}

/// Port of `SegPerpendicularCorrect`.
///
/// Predicate to check expected perpendicularity between two segments
///
/// * `a_seg_a` - the first #SEG
/// * `a_seg_b` - the second #SEG
/// * `a_exp` - expected perpendicularity: true = segments are perpendicular
///   false = segments are not perpendicular
///
/// Returns: does the perpendicularity calculated agree?
fn seg_perpendicular_correct(a_seg_a: &Seg, a_seg_b: &Seg, a_exp: bool) -> bool {
    let a_to_b: bool = a_seg_a.approx_perpendicular(a_seg_b);
    let b_to_a: bool = a_seg_b.approx_perpendicular(a_seg_a);

    let ok: bool = (a_to_b == a_exp) && (b_to_a == a_exp);

    if a_to_b != b_to_a {
        eprintln!(
            "Segment perpendicularity is not the same in both directions: expected {a_exp}, got AtoB: {a_to_b} BtoA:{b_to_a}"
        );
    } else if !ok {
        eprintln!("Perpendicularity incorrect: expected {a_exp}, got {a_to_b}");
    }

    ok
}

/// Port of `BOOST_AUTO_TEST_CASE( EndpointCtorMod )`.
///
/// Checks whether the construction of a segment referencing external points works
/// and that the endpoints can be modified as normal points.
#[test]
fn endpoint_ctor_mod() {
    let point_a = Vector2I::new(10, 20);
    let point_b = Vector2I::new(100, 200);

    // Build a segment referencing the previous points
    let mut segment = Seg::new(point_a, point_b);

    assert_eq!(point_a, Vector2I::new(10, 20));
    assert_eq!(point_b, Vector2I::new(100, 200));

    // Modify the ends of the segments
    segment.a += Vector2I::new(10, 10);
    segment.b += Vector2I::new(100, 100);

    // Check that the ends in segment are modified
    assert_eq!(segment.a, Vector2I::new(20, 30));
    assert_eq!(segment.b, Vector2I::new(200, 300));
}

/// Port of `SEG_SEG_DISTANCE_CASE`.
struct SegSegDistanceCase {
    case_name: &'static str,
    seg_a: Seg,
    seg_b: Seg,
    exp_dist: i32,
}

/// Port of `seg_seg_dist_cases`.
#[rustfmt::skip]
const SEG_SEG_DIST_CASES: &[SegSegDistanceCase] = &[
    SegSegDistanceCase {
        case_name: "Parallel, 10 apart",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 10, 10, 10 ),
        exp_dist: 10,
    },
    SegSegDistanceCase {
        case_name: "Non-parallel, 10 apart",
        seg_a: seg( 0, -5, 10, 0 ),
        seg_b: seg( 0, 10, 10, 10 ),
        exp_dist: 10,
    },
    SegSegDistanceCase {
        case_name: "Co-incident",
        seg_a: seg( 0, 0, 30, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        exp_dist: 0,
    },
    SegSegDistanceCase {
        case_name: "Crossing",
        seg_a: seg( 0, -10, 0, 10 ),
        seg_b: seg( -20, 0, 20, 0 ),
        exp_dist: 0,
    },
    SegSegDistanceCase {
        case_name: "T-junction",
        seg_a: seg( 0, -10, 0, 10 ),
        seg_b: seg( -20, 0, 0, 0 ),
        exp_dist: 0,
    },
    SegSegDistanceCase {
        case_name: "T-junction (no touch)",
        seg_a: seg( 0, -10, 0, 10 ),
        seg_b: seg( -20, 0, -2, 0 ),
        exp_dist: 2,
    },
    SegSegDistanceCase {
        case_name: "Zero-length segment A",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        exp_dist: 10,
    },
    SegSegDistanceCase {
        case_name: "Zero-length segment B",
        seg_a: seg( 10, 0, 20, 0 ),
        seg_b: seg( 0, 0, 0, 0 ),
        exp_dist: 10,
    },
    SegSegDistanceCase {
        case_name: "Both zero-length",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 10, 0 ),
        exp_dist: 10,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegSegDistance, ... )`.
#[test]
fn seg_seg_distance() {
    for c in SEG_SEG_DIST_CASES {
        assert!(
            seg_distance_correct(&c.seg_a, &c.seg_b, c.exp_dist),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `SEG_VECTOR_DISTANCE_CASE`.
struct SegVectorDistanceCase {
    case_name: &'static str,
    seg: Seg,
    vec: Vector2I,
    exp_dist: i32,
}

/// Port of `seg_vec_dist_cases`.
#[rustfmt::skip]
const SEG_VEC_DIST_CASES: &[SegVectorDistanceCase] = &[
    SegVectorDistanceCase {
        case_name: "On endpoint",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 0, 0 ),
        exp_dist: 0,
    },
    SegVectorDistanceCase {
        case_name: "On segment",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 3, 0 ),
        exp_dist: 0,
    },
    SegVectorDistanceCase {
        case_name: "At side",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 3, 2 ),
        exp_dist: 2,
    },
    SegVectorDistanceCase {
        case_name: "At end (collinear)",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 12, 0 ),
        exp_dist: 2,
    },
    SegVectorDistanceCase {
        case_name: "At end (not collinear)",
        seg: seg( 0, 0, 1000, 0 ),
        vec: pt( 1000 + 200, 200 ),
        exp_dist: 282, // sqrt(200^2 + 200^2) = 282.8, rounded to nearest
    },
    SegVectorDistanceCase {
        case_name: "Issue 18473 (inside hit with rounding error)",
        seg: seg( 187360000, 42510000, 105796472, 42510000 ),
        vec: pt( 106645000, 42510000 ),
        exp_dist: 0,
    },
    SegVectorDistanceCase {
        case_name: "Straight line x distance",
        seg: seg( 187360000, 42510000, 105796472, 42510000 ),
        vec: pt( 197360000, 42510000 ),
        exp_dist: 10000000,
    },
    SegVectorDistanceCase {
        case_name: "Straight line -x distance",
        seg: seg( 187360000, 42510000, 105796472, 42510000 ),
        vec: pt( 104796472, 42510000 ),
        exp_dist: 1000000,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegVecDistance, ... )`.
#[test]
fn seg_vec_distance() {
    for c in SEG_VEC_DIST_CASES {
        assert!(
            seg_vec_distance_correct(&c.seg, c.vec, c.exp_dist),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `SEG_SEG_COLLIDE_CASE`.
///
/// Test cases for collisions (with clearance, for no clearance,
/// it's just a SEG_SEG_DISTANCE_CASE of 0)
struct SegSegCollideCase {
    case_name: &'static str,
    seg_a: Seg,
    seg_b: Seg,
    clearance: i32,
    exp_coll: bool,
}

/// Port of `seg_seg_coll_cases`.
#[rustfmt::skip]
const SEG_SEG_COLL_CASES: &[SegSegCollideCase] = &[
    SegSegCollideCase {
        case_name: "Parallel, 10 apart, 5 clear",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 10, 10, 10 ),
        clearance: 5,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Parallel, 10 apart, 10 clear",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 10, 10, 10 ),
        clearance: 10,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Parallel, 10 apart, 11 clear",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 10, 10, 10 ),
        clearance: 11,
        exp_coll: true,
    },
    SegSegCollideCase {
        case_name: "T-junction, 2 apart, 2 clear",
        seg_a: seg( 0, -10, 0, 0 ),
        seg_b: seg( -20, 0, -2, 0 ),
        clearance: 2,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "T-junction, 2 apart, 3 clear",
        seg_a: seg( 0, -10, 0, 0 ),
        seg_b: seg( -20, 0, -2, 0 ),
        clearance: 3,
        exp_coll: true,
    },
    SegSegCollideCase {
        case_name: "Zero-length segment A, 10 apart",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        clearance: 0,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length segment A, 10 apart, 9 clear",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        clearance: 9,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length segment A, 10 apart, 10 clear",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        clearance: 10,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length segment A, 10 apart, 11 clear",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        clearance: 11,
        exp_coll: true,
    },
    SegSegCollideCase {
        case_name: "Zero-length segment B, 10 apart",
        seg_a: seg( 10, 0, 20, 0 ),
        seg_b: seg( 0, 0, 0, 0 ),
        clearance: 0,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Both zero-length, same point",
        seg_a: seg( 5, 5, 5, 5 ),
        seg_b: seg( 5, 5, 5, 5 ),
        clearance: 0,
        exp_coll: true,
    },
    SegSegCollideCase {
        case_name: "Both zero-length, 10 apart",
        seg_a: seg( 0, 0, 0, 0 ),
        seg_b: seg( 10, 0, 10, 0 ),
        clearance: 0,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length on segment",
        seg_a: seg( 5, 0, 5, 0 ),
        seg_b: seg( 0, 0, 10, 0 ),
        clearance: 0,
        exp_coll: true,
    },
    SegSegCollideCase {
        case_name: "Zero-length near segment, x overlaps but y differs",
        seg_a: seg( 5, 5, 5, 5 ),
        seg_b: seg( 0, 0, 10, 0 ),
        clearance: 0,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length near segment, x overlaps but y differs, 4 clear",
        seg_a: seg( 5, 5, 5, 5 ),
        seg_b: seg( 0, 0, 10, 0 ),
        clearance: 4,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length near segment, x overlaps but y differs, 5 clear",
        seg_a: seg( 5, 5, 5, 5 ),
        seg_b: seg( 0, 0, 10, 0 ),
        clearance: 5,
        exp_coll: false,
    },
    SegSegCollideCase {
        case_name: "Zero-length near segment, x overlaps but y differs, 6 clear",
        seg_a: seg( 5, 5, 5, 5 ),
        seg_b: seg( 0, 0, 10, 0 ),
        clearance: 6,
        exp_coll: true,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegSegCollision, ... )`.
#[test]
fn seg_seg_collision() {
    for c in SEG_SEG_COLL_CASES {
        assert!(
            seg_collide_correct(&c.seg_a, &c.seg_b, c.clearance, c.exp_coll),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `SEG_SEG_BOOLEAN_CASE`.
///
/// Struct to hold general cases for collinearity, parallelism and perpendicularity
struct SegSegBooleanCase {
    case_name: &'static str,
    seg_a: Seg,
    seg_b: Seg,
    exp_result: bool,
}

/// Port of `seg_vec_collinear_cases`.
///
/// Test cases for collinearity
#[rustfmt::skip]
const SEG_VEC_COLLINEAR_CASES: &[SegSegBooleanCase] = &[
    SegSegBooleanCase {
        case_name: "coincident",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 10, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "end-to-end",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "In segment",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 0, 7, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "At side, parallel",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 1, 7, 1 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "crossing",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 5, -5, 5, 5 ),
        exp_result: false,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegSegCollinear, ... )`.
#[test]
fn seg_seg_collinear() {
    for c in SEG_VEC_COLLINEAR_CASES {
        assert!(
            seg_collinear_correct(&c.seg_a, &c.seg_b, c.exp_result),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `seg_vec_parallel_cases`.
///
/// Test cases for parallelism
#[rustfmt::skip]
const SEG_VEC_PARALLEL_CASES: &[SegSegBooleanCase] = &[
    SegSegBooleanCase {
        case_name: "coincident",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 10, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "end-to-end",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "In segment",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 0, 7, 0 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "At side, parallel",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 1, 7, 1 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "crossing",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 5, -5, 5, 5 ),
        exp_result: false,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegSegParallel, ... )`.
#[test]
fn seg_seg_parallel() {
    for c in SEG_VEC_PARALLEL_CASES {
        assert!(
            seg_parallel_correct(&c.seg_a, &c.seg_b, c.exp_result),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `seg_vec_perpendicular_cases`.
///
/// Test cases for perpendicularity
#[rustfmt::skip]
const SEG_VEC_PERPENDICULAR_CASES: &[SegSegBooleanCase] = &[
    SegSegBooleanCase {
        case_name: "coincident",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 10, 0 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "end-to-end",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 10, 0, 20, 0 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "In segment",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 0, 7, 0 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "At side, parallel",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 4, 1, 7, 1 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "crossing 45 deg",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 5, 5 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "very nearly perpendicular",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 1, 10 ),
        exp_result: true, //allow error margin of 1 IU
    },
    SegSegBooleanCase {
        case_name: "not really perpendicular",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 3, 10 ),
        exp_result: false,
    },
    SegSegBooleanCase {
        case_name: "perpendicular",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 0, 0, 0, 10 ),
        exp_result: true,
    },
    SegSegBooleanCase {
        case_name: "perpendicular not intersecting",
        seg_a: seg( 0, 0, 10, 0 ),
        seg_b: seg( 15, 5, 15, 10 ),
        exp_result: true,
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegSegPerpendicular, ... )`.
#[test]
fn seg_seg_perpendicular() {
    for c in SEG_VEC_PERPENDICULAR_CASES {
        assert!(
            seg_perpendicular_correct(&c.seg_a, &c.seg_b, c.exp_result),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `SEG_VEC_CASE`.
///
/// Struct to hold cases for operations with a #SEG, and a #VECTOR2I
struct SegVecCase {
    case_name: &'static str,
    seg: Seg,
    vec: Vector2I,
}

/// Port of `segment_and_point_cases`.
///
/// Test cases to create segments passing through a point
#[rustfmt::skip]
const SEGMENT_AND_POINT_CASES: &[SegVecCase] = &[
    SegVecCase {
        case_name: "Horizontal: point on edge of seg",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 0, 0 ),
    },
    SegVecCase {
        case_name: "Horizontal: point in middle of seg",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 5, 0 ),
    },
    SegVecCase {
        case_name: "Horizontal: point outside seg",
        seg: seg( 0, 0, 10, 0 ),
        vec: pt( 20, 20 ),
    },
    SegVecCase {
        case_name: "Vertical: point on edge of seg",
        seg: seg( 0, 0, 0, 10 ),
        vec: pt( 0, 0 ),
    },
    SegVecCase {
        case_name: "Vertical: point in middle of seg",
        seg: seg( 0, 0, 0, 10 ),
        vec: pt( 0, 5 ),
    },
    SegVecCase {
        case_name: "Vertical: point outside seg",
        seg: seg( 0, 0, 0, 10 ),
        vec: pt( 20, 20 ),
    },
];

/// Port of `BOOST_DATA_TEST_CASE( SegCreateParallel, ... )`.
#[test]
fn seg_create_parallel() {
    for c in SEGMENT_AND_POINT_CASES {
        let perpendicular: Seg = c.seg.parallel_seg(c.vec);

        assert!(
            seg_parallel_correct(&perpendicular, &c.seg, true),
            "case: {}",
            c.case_name
        );
        assert!(
            seg_vec_distance_correct(&perpendicular, c.vec, 0),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `BOOST_DATA_TEST_CASE( SegCreatePerpendicular, ... )`.
#[test]
fn seg_create_perpendicular() {
    for c in SEGMENT_AND_POINT_CASES {
        let perpendicular: Seg = c.seg.perpendicular_seg(c.vec);

        assert!(
            seg_perpendicular_correct(&perpendicular, &c.seg, true),
            "case: {}",
            c.case_name
        );
        assert!(
            seg_vec_distance_correct(&perpendicular, c.vec, 0),
            "case: {}",
            c.case_name
        );
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( LineDistance )`.
#[test]
fn line_distance() {
    let seg = seg(0, 0, 10, 0);

    assert!(seg.line_distance(pt(5, 0)) == 0);
    assert!(seg.line_distance(pt(5, 8)) == 8);
}

/// Port of `BOOST_AUTO_TEST_CASE( SquaredDistanceLongSegmentNoCancellation )`.
#[test]
fn squared_distance_long_segment_no_cancellation() {
    let seg = seg(0, 0, 700000000, 700000001);
    let pt = Vector2I::new(350001004, 350000992);

    // True distance is 8.84 nm, the old |ap|^2 - e^2/f form returned 0
    assert!((seg.squared_distance(pt) - 78).abs() <= 1);
    assert_eq!(seg.distance(pt), 8);
    assert!(!seg.collide(&Seg::new(pt, pt), 0));
}

/// Port of `BOOST_AUTO_TEST_CASE( SquaredDistanceLongSegmentRandom )`.
///
/// The C++ test draws its data from `std::mt19937` through the libstdc++ distributions, in
/// the order in which GCC evaluates the constructor arguments (right to left, so the y
/// coordinate of each point is drawn before its x coordinate).  The port reproduces that
/// sequence; the first points and a checksum over all inputs and results (obtained from the
/// C++ test code built with GCC 13 on x86-64 Linux) are asserted below in addition to the
/// original check.
///
/// The C++ reference value is computed in `long double`; the port checks the same bound
/// exactly with 128-bit integers: `| sd - cr^2 / f | <= 1` is `| sd * f - cr^2 | <= f`.
#[test]
fn squared_distance_long_segment_random() {
    let mut rng = Mt19937::new(12345);
    let coord = |rng: &mut Mt19937| uniform_int_libstdcxx(rng, -500000000, 500000000) as i32;
    let offs = |rng: &mut Mt19937| uniform_int_libstdcxx(rng, -40, 40) as i32;
    let frac = |rng: &mut Mt19937| uniform_real_libstdcxx(rng, 0.05, 0.95);

    // FNV-1a over the little-endian bytes of the 64-bit values
    let mut hash: u64 = 1469598103934665603;
    let mut mix = |v: i64| {
        for byte in (v as u64).to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
    };

    let mut checked = 0;
    let mut first: Vec<(Vector2I, Vector2I, Vector2I, i64)> = Vec::new();

    for i in 0..2000 {
        let ay = coord(&mut rng);
        let ax = coord(&mut rng);
        let a = Vector2I::new(ax, ay);
        let by = coord(&mut rng);
        let bx = coord(&mut rng);
        let b = Vector2I::new(bx, by);
        let t: f64 = frac(&mut rng);
        let oy = offs(&mut rng);
        let ox = offs(&mut rng);
        let p = Vector2I::new(
            (a.x as f64 + t * (b.x as f64 - a.x as f64)) as i32 + ox,
            (a.y as f64 + t * (b.y as f64 - a.y as f64)) as i32 + oy,
        );

        let abx: i128 = b.x as i128 - a.x as i128;
        let aby: i128 = b.y as i128 - a.y as i128;
        let apx: i128 = p.x as i128 - a.x as i128;
        let apy: i128 = p.y as i128 - a.y as i128;
        let e: i128 = apx * abx + apy * aby;
        let f: i128 = abx * abx + aby * aby;

        for v in [a.x, a.y, b.x, b.y, p.x, p.y] {
            mix(v as i64);
        }

        if i < 3 {
            first.push((a, b, p, Seg::new(a, b).squared_distance(p)));
        }

        if e <= 0 || e >= f {
            continue;
        }

        checked += 1;

        let cr: i128 = abx * apy - aby * apx;
        let sd: i64 = Seg::new(a, b).squared_distance(p);

        // std::abs( (long double) SEG( a, b ).SquaredDistance( p ) - expected ) <= 1.0L
        assert!(
            (sd as i128 * f - cr * cr).abs() <= f,
            "a={a} b={b} p={p} sd={sd}"
        );

        mix(sd);
    }

    assert_eq!(
        first,
        vec![
            (
                pt(390154713, 429616087),
                pt(-369292706, -183624440),
                pt(325006645, 377010115),
                1221
            ),
            (
                pt(32077916, 67725026),
                pt(456309975, 95544703),
                pt(229657106, 80681598),
                87
            ),
            (
                pt(-126078823, 248906637),
                pt(-345026645, 153569875),
                pt(-312865295, 167573990),
                2322
            ),
        ]
    );
    assert_eq!(checked, 2000);
    assert_eq!(hash, 0x71c69337e967699c);
}

/// Port of `BOOST_AUTO_TEST_CASE( LineDistanceSided )`.
#[test]
fn line_distance_sided() {
    let seg = seg(0, 0, 10, 0);

    assert!(seg.line_distance_sided(pt(5, 8), true) == 8);
    assert!(seg.line_distance_sided(pt(5, -8), true) == -8);
}

/// Port of `SEG_SEG_INTERSECT_CASE`.
///
/// Test cases for segment intersection
struct SegSegIntersectCase {
    case_name: &'static str,
    seg_a: Seg,
    seg_b: Seg,
    ignore_endpoints: bool,
    lines: bool,
    exp_intersect: bool,
    exp_point: Vector2I,
}

const fn isect(
    case_name: &'static str,
    seg_a: Seg,
    seg_b: Seg,
    ignore_endpoints: bool,
    lines: bool,
    exp_intersect: bool,
    exp_point: Vector2I,
) -> SegSegIntersectCase {
    SegSegIntersectCase {
        case_name,
        seg_a,
        seg_b,
        ignore_endpoints,
        lines,
        exp_intersect,
        exp_point,
    }
}

/// Port of `seg_intersect_cases`.
#[rustfmt::skip]
const SEG_INTERSECT_CASES: &[SegSegIntersectCase] = &[
    // Basic crossing cases
    isect(
        "Crossing at origin",
        seg( -10, 0, 10, 0 ),
        seg( 0, -10, 0, 10 ),
        false, false, true,
        pt( 0, 0 )
    ),
    isect(
        "Crossing at (5,5)",
        seg( 0, 5, 10, 5 ),
        seg( 5, 0, 5, 10 ),
        false, false, true,
        pt( 5, 5 )
    ),
    isect(
        "T-junction intersection",
        seg( 0, 0, 10, 0 ),
        seg( 5, -5, 5, 0 ),
        false, false, true,
        pt( 5, 0 )
    ),

    // Non-intersecting cases
    isect(
        "Parallel segments",
        seg( 0, 0, 10, 0 ),
        seg( 0, 5, 10, 5 ),
        false, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Separated segments",
        seg( 0, 0, 5, 0 ),
        seg( 10, 0, 15, 0 ),
        false, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Lines would intersect, but segments don't",
        seg( 0, 0, 2, 0 ),
        seg( 5, -5, 5, 5 ),
        false, false, false,
        pt( 0, 0 )
    ),

    // Endpoint intersection cases
    isect(
        "Endpoint touching - should intersect",
        seg( 0, 0, 10, 0 ),
        seg( 10, 0, 20, 0 ),
        false, false, true,
        pt( 10, 0 )
    ),
    isect(
        "Endpoint touching - ignore endpoints",
        seg( 0, 0, 10, 0 ),
        seg( 10, 0, 20, 0 ),
        true, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Endpoint touching at angle",
        seg( 0, 0, 10, 0 ),
        seg( 10, 0, 15, 5 ),
        false, false, true,
        pt( 10, 0 )
    ),

    // Collinear cases
    isect(
        "Collinear overlapping segments",
        seg( 0, 0, 10, 0 ),
        seg( 5, 0, 15, 0 ),
        false, false, true,
        pt( 7, 0 )  // Midpoint of overlap [5,10]
    ),
    isect(
        "Collinear non-overlapping segments",
        seg( 0, 0, 5, 0 ),
        seg( 10, 0, 15, 0 ),
        false, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Collinear touching at endpoint",
        seg( 0, 0, 10, 0 ),
        seg( 10, 0, 20, 0 ),
        false, false, true,
        pt( 10, 0 )
    ),
    isect(
        "Collinear contained segment",
        seg( 0, 0, 20, 0 ),
        seg( 5, 0, 15, 0 ),
        false, false, true,
        pt( 10, 0 )  // Midpoint of contained segment
    ),
    isect(
        "Collinear vertical overlapping",
        seg( 5, 0, 5, 10 ),
        seg( 5, 5, 5, 15 ),
        false, false, true,
        pt( 5, 7 )  // Midpoint of overlap [5,10]
    ),

    // Line mode cases (infinite lines)
    isect(
        "Lines intersect, segments don't",
        seg( 0, 0, 2, 0 ),
        seg( 5, -5, 5, 5 ),
        false, true, true,
        pt( 5, 0 )
    ),
    isect(
        "Parallel lines (infinite)",
        seg( 0, 0, 10, 0 ),
        seg( 0, 5, 10, 5 ),
        false, true, false,
        pt( 0, 0 )
    ),
    isect(
        "Collinear lines (infinite)",
        seg( 0, 0, 10, 0 ),
        seg( 20, 0, 30, 0 ),
        false, true, true,
        pt( 10, 0 )  // Midpoint between segment starts
    ),

    // Edge cases
    isect(
        "Zero-length segment intersection",
        seg( 5, 5, 5, 5 ),
        seg( 0, 5, 10, 5 ),
        false, false, true,
        pt( 5, 5 )
    ),
    isect(
        "Both zero-length, same point",
        seg( 5, 5, 5, 5 ),
        seg( 5, 5, 5, 5 ),
        false, false, true,
        pt( 5, 5 )
    ),
    isect(
        "Both zero-length, different points",
        seg( 5, 5, 5, 5 ),
        seg( 10, 10, 10, 10 ),
        false, false, false,
        pt( 0, 0 )
    ),

    // Diagonal intersection cases
    isect(
        "45-degree crossing",
        seg( 0, 0, 10, 10 ),
        seg( 0, 10, 10, 0 ),
        false, false, true,
        pt( 5, 5 )
    ),
    isect(
        "Arbitrary angle crossing",
        seg( 0, 0, 6, 8 ),
        seg( 0, 8, 6, 0 ),
        false, false, true,
        pt( 3, 4 )
    ),

    // Bounding box optimization test cases
    isect(
        "Far apart horizontal segments",
        seg( 0, 0, 10, 0 ),
        seg( 100, 0, 110, 0 ),
        false, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Far apart vertical segments",
        seg( 0, 0, 0, 10 ),
        seg( 0, 100, 0, 110 ),
        false, false, false,
        pt( 0, 0 )
    ),
    isect(
        "Far apart diagonal segments",
        seg( 0, 0, 10, 10 ),
        seg( 100, 100, 110, 110 ),
        false, false, false,
        pt( 0, 0 )
    ),
];

/// Port of `SegIntersectCorrect`.
///
/// Predicate to check expected intersection between two segments
///
/// * `a_case` - the test case containing all parameters
///
/// Returns: does the intersection calculated agree?
fn seg_intersect_correct(a_case: &SegSegIntersectCase) -> bool {
    let result_a =
        a_case
            .seg_a
            .intersect_with(&a_case.seg_b, a_case.ignore_endpoints, a_case.lines);
    let result_b =
        a_case
            .seg_b
            .intersect_with(&a_case.seg_a, a_case.ignore_endpoints, a_case.lines);

    let intersects_a: bool = result_a.is_some();
    let intersects_b: bool = result_b.is_some();

    let mut ok: bool =
        (intersects_a == a_case.exp_intersect) && (intersects_b == a_case.exp_intersect);

    if intersects_a != intersects_b {
        eprintln!(
            "Segment intersection is not the same in both directions: expected {}, got {} & {}",
            a_case.exp_intersect, intersects_a, intersects_b
        );
        ok = false;
    } else if !ok {
        eprintln!(
            "Intersection incorrect: expected {}, got {}",
            a_case.exp_intersect, intersects_a
        );
    }

    // Check intersection point if intersection was expected
    if ok && a_case.exp_intersect && a_case.exp_point != Vector2I::default() {
        // Allow some tolerance for intersection point calculation
        let tolerance: i32 = 1;

        match (result_a, result_b) {
            (Some(point_a), Some(point_b)) => {
                let point_ok: bool = (point_a.x - a_case.exp_point.x).abs() <= tolerance
                    && (point_a.y - a_case.exp_point.y).abs() <= tolerance
                    && (point_b.x - a_case.exp_point.x).abs() <= tolerance
                    && (point_b.y - a_case.exp_point.y).abs() <= tolerance;

                if !point_ok {
                    eprintln!(
                        "Intersection point incorrect: expected {}, got {} & {}",
                        a_case.exp_point.format(),
                        point_a.format(),
                        point_b.format()
                    );
                    ok = false;
                }
            }
            _ => {
                eprintln!("Expected intersection but got nullopt");
                ok = false;
            }
        }
    }

    ok
}

/// Port of `BOOST_DATA_TEST_CASE( SegSegIntersection, ... )`.
#[test]
fn seg_seg_intersection() {
    for c in SEG_INTERSECT_CASES {
        assert!(seg_intersect_correct(c), "case: {}", c.case_name);
    }
}

// Additional focused test cases for specific scenarios

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLargeCoordinates )`.
#[test]
fn intersect_large_coordinates() {
    // Test with large coordinates to verify overflow protection
    let seg_a = seg(1000000000, 0, -1000000000, 0);
    let seg_b = seg(0, 1000000000, 0, -1000000000);

    let intersection = seg_a.intersect_with(&seg_b, false, false);

    assert!(intersection.is_some());
    assert_eq!(intersection.unwrap().x, 0);
    assert_eq!(intersection.unwrap().y, 0);
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectOverflowDetection )`.
#[test]
fn intersect_overflow_detection() {
    // Test intersection that would overflow coordinate range
    const MAX_COORD: i32 = i32::MAX;

    let seg_a = seg(0, 0, MAX_COORD, MAX_COORD);
    let seg_b = seg(MAX_COORD, 0, 0, MAX_COORD);

    // This should either work or return nullopt due to overflow protection
    let intersection = seg_a.intersect_with(&seg_b, false, false);

    // The test passes if it doesn't crash - the exact result depends on overflow handling
    println!(
        "Overflow test completed without crash. Has intersection: {}",
        intersection.is_some()
    );

    if let Some(p) = intersection {
        println!("Intersection point: {}", p.format());
    }
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectPrecisionEdgeCases )`.
#[test]
fn intersect_precision_edge_cases() {
    // Test cases that might have precision issues
    let seg_a = seg(0, 0, 1000000, 1);
    let seg_b = seg(500000, -1, 500000, 2);

    let intersection = seg_a.intersect_with(&seg_b, false, false);

    assert!(intersection.is_some());
    // The intersection should be very close to (500000, 0.5), rounded to (500000, 1) or (500000, 0)
    assert_eq!(intersection.unwrap().x, 500000);
    assert!(intersection.unwrap().y >= 0 && intersection.unwrap().y <= 1);
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectIgnoreEndpointsEdgeCases )`.
#[test]
fn intersect_ignore_endpoints_edge_cases() {
    // Test edge cases with ignore endpoints
    let seg_a = seg(0, 0, 10, 0);
    let seg_b = seg(5, -5, 5, 5);

    // Normal intersection should work
    let intersection1 = seg_a.intersect_with(&seg_b, false, false);
    assert!(intersection1.is_some());
    assert_eq!(intersection1.unwrap(), Vector2I::new(5, 0));

    // Should still work when ignoring endpoints (this is a middle intersection)
    let intersection2 = seg_a.intersect_with(&seg_b, true, false);
    assert!(intersection2.is_some());
    assert_eq!(intersection2.unwrap(), Vector2I::new(5, 0));

    // Test actual endpoint intersection
    let seg_c = seg(10, 0, 20, 0);
    let intersection3 = seg_a.intersect_with(&seg_c, false, false);
    assert!(intersection3.is_some());
    assert_eq!(intersection3.unwrap(), Vector2I::new(10, 0));

    // Should not intersect when ignoring endpoints
    let intersection4 = seg_a.intersect_with(&seg_c, true, false);
    assert!(intersection4.is_none());
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectCollinearRegressionTests )`.
#[test]
fn intersect_collinear_regression_tests() {
    // Regression tests for collinear segment handling

    // Test case: horizontal segments with partial overlap
    let seg1 = seg(0, 5, 10, 5);
    let seg2 = seg(5, 5, 15, 5);

    let intersection = seg1.intersect_with(&seg2, false, false);

    assert!(intersection.is_some());
    assert_eq!(intersection.unwrap().y, 5);
    assert!(intersection.unwrap().x >= 5 && intersection.unwrap().x <= 10); // Should be in overlap region

    // Test case: vertical segments with complete overlap (one contained in other)
    let seg3 = seg(3, 0, 3, 20);
    let seg4 = seg(3, 5, 3, 15);

    let intersection2 = seg3.intersect_with(&seg4, false, false);

    assert!(intersection2.is_some());
    assert_eq!(intersection2.unwrap().x, 3);
    assert!(intersection2.unwrap().y >= 5 && intersection2.unwrap().y <= 15); // Should be in contained segment

    // Test case: diagonal collinear segments
    let seg5 = seg(0, 0, 10, 10);
    let seg6 = seg(5, 5, 15, 15);

    let intersection3 = seg5.intersect_with(&seg6, false, false);

    assert!(intersection3.is_some());
    assert!(intersection3.unwrap().x >= 5 && intersection3.unwrap().x <= 10);
    assert!(intersection3.unwrap().y >= 5 && intersection3.unwrap().y <= 10);
    assert_eq!(intersection3.unwrap().x, intersection3.unwrap().y); // Should maintain diagonal relationship

    // Test case: collinear segments that touch only at endpoints
    let seg7 = seg(0, 0, 5, 0);
    let seg8 = seg(5, 0, 10, 0);

    let intersection4 = seg7.intersect_with(&seg8, false, false);
    assert!(intersection4.is_some());
    assert_eq!(intersection4.unwrap(), Vector2I::new(5, 0));

    // Same test but ignoring endpoints
    let intersection5 = seg7.intersect_with(&seg8, true, false);
    assert!(intersection5.is_none());

    // Test case: collinear segments that don't overlap
    let seg9 = seg(0, 0, 5, 0);
    let seg10 = seg(10, 0, 15, 0);

    let intersection6 = seg9.intersect_with(&seg10, false, false);
    assert!(intersection6.is_none());
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectBoundingBoxOptimization )`.
#[test]
fn intersect_bounding_box_optimization() {
    // Test that bounding box optimization works correctly

    // Segments that are clearly separated - should be rejected quickly
    let seg1 = seg(0, 0, 10, 10);
    let seg2 = seg(100, 100, 110, 110);

    let intersection = seg1.intersect_with(&seg2, false, false);
    assert!(intersection.is_none());

    // Segments with overlapping bounding boxes but no intersection
    let seg3 = seg(0, 0, 10, 0);
    let seg4 = seg(5, 5, 15, 5);

    let intersection2 = seg3.intersect_with(&seg4, false, false);
    assert!(intersection2.is_none());

    // Segments with touching bounding boxes and actual intersection
    let seg5 = seg(0, 0, 10, 10);
    let seg6 = seg(10, 0, 0, 10);

    let intersection3 = seg5.intersect_with(&seg6, false, false);
    assert!(intersection3.is_some());
    assert_eq!(intersection3.unwrap(), Vector2I::new(5, 5));
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineVsSegmentMode )`.
#[test]
fn intersect_line_vs_segment_mode() {
    // Test the difference between line mode and segment mode

    let seg1 = seg(0, 0, 5, 0);
    let seg2 = seg(10, -5, 10, 5);

    // In segment mode, these don't intersect
    let segment_intersect = seg1.intersect_with(&seg2, false, false);
    assert!(segment_intersect.is_none());

    // In line mode, they should intersect
    let line_intersect = seg1.intersect_with(&seg2, false, true);
    assert!(line_intersect.is_some());
    assert_eq!(line_intersect.unwrap(), Vector2I::new(10, 0));

    // Test collinear case in line mode
    let seg3 = seg(0, 0, 10, 0);
    let seg4 = seg(20, 0, 30, 0);

    // Segments don't intersect
    let segment_intersect2 = seg3.intersect_with(&seg4, false, false);
    assert!(segment_intersect2.is_none());

    // Lines (infinite) do intersect (collinear)
    let line_intersect2 = seg3.intersect_with(&seg4, false, true);
    assert!(line_intersect2.is_some());
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectNumericalStability )`.
#[test]
fn intersect_numerical_stability() {
    // Test cases designed to stress numerical precision

    // Very small segments
    let seg1 = seg(0, 0, 1, 1);
    let seg2 = seg(0, 1, 1, 0);

    let intersection = seg1.intersect_with(&seg2, false, false);

    assert!(intersection.is_some());
    // Intersection should be very close to (0.5, 0.5), rounded to (0,0), (0,1), (1,0), or (1,1)
    assert!(intersection.unwrap().x >= 0 && intersection.unwrap().x <= 1);
    assert!(intersection.unwrap().y >= 0 && intersection.unwrap().y <= 1);

    // Nearly parallel segments
    let seg3 = seg(0, 0, 1000, 1);
    let seg4 = seg(0, 1, 1000, 2);

    let intersection2 = seg3.intersect_with(&seg4, false, false);
    assert!(intersection2.is_none()); // Should be detected as parallel/non-intersecting

    // Segments that intersect at a very acute angle
    let seg5 = seg(0, 0, 1000000, 1);
    let seg6 = seg(500000, -1, 500000, 2);

    let intersection3 = seg5.intersect_with(&seg6, false, false);
    assert!(intersection3.is_some());
    assert_eq!(intersection3.unwrap().x, 500000);
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectZeroLengthSegments )`.
#[test]
fn intersect_zero_length_segments() {
    // Comprehensive tests for zero-length segments (points)

    let point1 = Vector2I::new(5, 5);
    let point2 = Vector2I::new(10, 10);

    let point_seg1 = Seg::new(point1, point1); // Zero-length segment (point)
    let point_seg2 = Seg::new(point2, point2); // Another zero-length segment
    let normal_seg = seg(0, 5, 10, 5); // Normal segment

    // Point intersecting with normal segment
    let intersection1 = point_seg1.intersect_with(&normal_seg, false, false);
    assert!(intersection1.is_some());
    assert_eq!(intersection1.unwrap(), point1);

    // Point not intersecting with normal segment
    let intersection2 = point_seg2.intersect_with(&normal_seg, false, false);
    assert!(intersection2.is_none());

    // Two points at same location
    let point_seg3 = Seg::new(point1, point1);
    let intersection3 = point_seg1.intersect_with(&point_seg3, false, false);
    assert!(intersection3.is_some());
    assert_eq!(intersection3.unwrap(), point1);

    // Two points at different locations
    let intersection4 = point_seg1.intersect_with(&point_seg2, false, false);
    assert!(intersection4.is_none());

    // Point on line (infinite mode)
    let line_seg = seg(0, 0, 1, 1); // Diagonal line segment
    let point_on_line = seg(100, 100, 100, 100); // Point on extended line

    let intersection5 = point_on_line.intersect_with(&line_seg, false, false);
    assert!(intersection5.is_none()); // Point not on segment

    let intersection6 = point_on_line.intersect_with(&line_seg, false, true);
    assert!(intersection6.is_some()); // Point on infinite line
    assert_eq!(intersection6.unwrap(), Vector2I::new(100, 100));
}

/// Port of `SEG_LINE_INTERSECT_CASE`.
///
/// Test cases for segment-line intersection
struct SegLineIntersectCase {
    case_name: &'static str,
    seg: Seg,
    slope: f64,
    offset: f64,
    exp_intersect: bool,
    exp_point: Vector2I,
}

const fn line_case(
    case_name: &'static str,
    seg: Seg,
    slope: f64,
    offset: f64,
    exp_intersect: bool,
    exp_point: Vector2I,
) -> SegLineIntersectCase {
    SegLineIntersectCase {
        case_name,
        seg,
        slope,
        offset,
        exp_intersect,
        exp_point,
    }
}

/// Port of `SegLineIntersectCorrect`.
///
/// Predicate to check expected intersection between a segment and an infinite line
///
/// * `a_seg` - the segment
/// * `a_slope` - the line slope
/// * `a_offset` - the line y-intercept
/// * `a_exp_intersect` - expected intersection result
/// * `a_exp_point` - expected intersection point (if intersection occurs)
///
/// Returns: does the intersection calculated agree?
fn seg_line_intersect_correct(
    a_seg: &Seg,
    a_slope: f64,
    a_offset: f64,
    a_exp_intersect: bool,
    a_exp_point: Vector2I,
) -> bool {
    let result = a_seg.intersects_line(a_slope, a_offset);
    let intersects: bool = result.is_some();
    let intersection: Vector2I = result.unwrap_or_default();

    let mut ok: bool = intersects == a_exp_intersect;

    if !ok {
        eprintln!("Line intersection incorrect: expected {a_exp_intersect}, got {intersects}");
    }

    // Check intersection point if intersection was expected
    if ok && a_exp_intersect && a_exp_point != Vector2I::default() {
        // Allow some tolerance for intersection point calculation
        let tolerance: i32 = 1;

        let point_ok: bool = (intersection.x - a_exp_point.x).abs() <= tolerance
            && (intersection.y - a_exp_point.y).abs() <= tolerance;

        if !point_ok {
            eprintln!(
                "Intersection point incorrect: expected {}, got {}",
                a_exp_point.format(),
                intersection.format()
            );
            ok = false;
        }
    }

    ok
}

/// Port of `seg_line_intersect_cases`.
#[rustfmt::skip]
const SEG_LINE_INTERSECT_CASES: &[SegLineIntersectCase] = &[
    // Basic intersection cases
    line_case(
        "Horizontal segment, diagonal line",
        seg( 0, 5, 10, 5 ),
        1.0, 0.0,  // y = x
        true,
        pt( 5, 5 )
    ),
    line_case(
        "Vertical segment, horizontal line",
        seg( 5, 0, 5, 10 ),
        0.0, 3.0,  // y = 3
        true,
        pt( 5, 3 )
    ),
    line_case(
        "Diagonal segment, horizontal line crossing",
        seg( 0, 0, 10, 10 ),
        0.0, 5.0,  // y = 5
        true,
        pt( 5, 5 )
    ),
    line_case(
        "Diagonal segment, vertical line (steep slope)",
        seg( 0, 0, 10, 10 ),
        1000.0, -5000.0,  // Very steep line: y = 1000x - 5000, crosses at x=5
        true,
        pt( 5, 5 )
    ),

    // Non-intersecting cases
    line_case(
        "Horizontal segment, parallel horizontal line",
        seg( 0, 5, 10, 5 ),
        0.0, 10.0,  // y = 10 (parallel to y = 5)
        false,
        pt( 0, 0 )
    ),
    line_case(
        "Diagonal segment, parallel line",
        seg( 0, 0, 10, 10 ),
        1.0, 5.0,  // y = x + 5 (parallel to y = x)
        false,
        pt( 0, 0 )
    ),
    line_case(
        "Segment above line",
        seg( 0, 10, 10, 10 ),
        0.0, 5.0,  // y = 5
        false,
        pt( 0, 0 )
    ),
    line_case(
        "Segment to left of steep line",
        seg( 0, 0, 2, 2 ),
        1.0, 10.0,  // y = x + 10
        false,
        pt( 0, 0 )
    ),

    // Collinear cases (segment lies on line)
    line_case(
        "Horizontal segment on horizontal line",
        seg( 0, 5, 10, 5 ),
        0.0, 5.0,  // y = 5
        true,
        pt( 5, 5 )  // Midpoint
    ),
    line_case(
        "Diagonal segment on diagonal line",
        seg( 0, 0, 10, 10 ),
        1.0, 0.0,  // y = x
        true,
        pt( 5, 5 )  // Midpoint
    ),
    line_case(
        "Vertical segment, any line slope (collinear impossible)",
        seg( 5, 0, 5, 10 ),
        2.0, -5.0,  // y = 2x - 5, passes through (5, 5)
        true,
        pt( 5, 5 )
    ),

    // Edge cases
    line_case(
        "Zero-length segment (point) on line",
        seg( 3, 7, 3, 7 ),
        2.0, 1.0,  // y = 2x + 1, point (3,7) should be on this line
        true,
        pt( 3, 7 )
    ),
    line_case(
        "Zero-length segment (point) not on line",
        seg( 3, 5, 3, 5 ),
        2.0, 1.0,  // y = 2x + 1, point (3,5) not on line (should be y=7)
        false,
        pt( 0, 0 )
    ),
    line_case(
        "Line with zero slope (horizontal)",
        seg( 0, 0, 10, 5 ),
        0.0, 2.5,  // y = 2.5
        true,
        pt( 5, 2 )  // Intersection at x=5, y=2.5 rounded to y=2 or 3
    ),
    line_case(
        "Very steep positive slope",
        seg( 0, 0, 10, 1 ),
        100.0, -250.0,  // y = 100x - 250, intersects at x=2.5
        true,
        pt( 2, 0 )  // Approximately (2.5, 0)
    ),
    line_case(
        "Very steep negative slope",
        seg( 0, 0, 10, 10 ),
        -100.0, 505.0,  // y = -100x + 505, intersects at x=5.05, y≈0
        true,
        pt( 5, 5 )  // Approximately (5.05, 0) but segment has y=5 at x=5
    ),
    line_case(
        "Fractional slope",
        seg( 0, 0, 12, 8 ),
        0.5, 1.0,  // y = 0.5x + 1
        true,
        pt( 6, 4 )  // Intersection where segment y = 2x/3 meets line y = 0.5x + 1
    ),

    // Endpoint intersections
    line_case(
        "Line passes through segment start point",
        seg( 2, 3, 80, 90 ),
        1.0, 1.0,  // y = x + 1, passes through (2,3)
        true,
        pt( 2, 3 )
    ),
    line_case(
        "Line passes through segment end point",
        seg( 20, 30, 8, 9 ),
        1.0, 1.0,  // y = x + 1, passes through (8,9)
        true,
        pt( 8, 9 )
    ),
    line_case(
        "Line intersects near endpoint",
        seg( 0, 0, 10, 0 ),
        0.0, 0.0,  // y = 0, same as segment
        true,
        pt( 5, 0 )  // Collinear, returns midpoint
    ),

    // Precision edge cases
    line_case(
        "Nearly parallel lines",
        seg( 0, 0, 1000, 1 ),
        0.0011, -0.05,  // Very slightly different slope
        true,
        pt( 500, 1 )  // At 500, y will round up to 1 in both cases
    ),
    line_case(
        "Line intersection outside segment bounds",
        seg( 5, 5, 10, 10 ),
        1.0, -10.0,  // y = x - 10, would intersect extended line at (15, 5)
        false,
        pt( 0, 0 )
    ),
];

/// Port of `BOOST_DATA_TEST_CASE( SegLineIntersection, ... )`.
#[test]
fn seg_line_intersection() {
    for c in SEG_LINE_INTERSECT_CASES {
        assert!(
            seg_line_intersect_correct(&c.seg, c.slope, c.offset, c.exp_intersect, c.exp_point),
            "case: {}",
            c.case_name
        );
    }
}

// Additional focused test cases for specific scenarios

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineVerticalSegments )`.
#[test]
fn intersect_line_vertical_segments() {
    // Test vertical segments with various line slopes
    let vertical_seg = seg(5, 0, 5, 10);

    // Horizontal line intersecting vertical segment
    let intersects1 = vertical_seg.intersects_line(0.0, 7.0);
    assert!(intersects1.is_some());
    assert_eq!(intersects1.unwrap(), Vector2I::new(5, 7));

    // Diagonal line intersecting vertical segment
    let intersects2 = vertical_seg.intersects_line(2.0, -5.0); // y = 2x - 5
    assert!(intersects2.is_some());
    assert_eq!(intersects2.unwrap(), Vector2I::new(5, 5)); // At x=5: y = 2*5 - 5 = 5

    // Line that misses vertical segment
    let intersects3 = vertical_seg.intersects_line(1.0, 20.0); // y = x + 20
    assert!(intersects3.is_none());
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineVerticalSegmentsCorrection )`.
#[test]
fn intersect_line_vertical_segments_correction() {
    // Corrected test for vertical segments
    let vertical_seg = seg(5, 0, 5, 10);

    // Line that misses vertical segment (intersection outside y-range)
    let intersects1 = vertical_seg.intersects_line(1.0, 20.0); // y = x + 20
    assert!(intersects1.is_none()); // At x=5: y = 25, which is outside [0,10]

    // Line that intersects within segment bounds
    let intersects2 = vertical_seg.intersects_line(0.5, 2.0); // y = 0.5x + 2
    assert!(intersects2.is_some());
    assert_eq!(intersects2.unwrap(), Vector2I::new(5, 5)); // At x=5: y = 0.5*5 + 2 = 4.5 ≈ 5 (round up)
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineVerticalSegmentMissBeyondIntRange )`.
///
/// The hatch generator sweeps unbounded lines past every segment, so most offsets miss by a
/// wide margin.  Rounding the miss into an int before rejecting it faulted on a discarded
/// value.  (The C++ test arms a throwing wxAssert handler to detect that; the port has no
/// assertion in `ki_round`, the checks on the results are the same.)
#[test]
fn intersect_line_vertical_segment_miss_beyond_int_range() {
    let vertical_seg = seg(95808800, -71602600, 95808800, -66903600);

    // At x = 95808800 this line sits at y = -2315308800, past the bottom of the int range
    let hit = vertical_seg.intersects_line(-1.0, -2219500000.0);
    assert!(hit.is_none());

    // ...and the same going the other way
    let hit = vertical_seg.intersects_line(1.0, 2219500000.0);
    assert!(hit.is_none());

    // A hit still lands, so the reordered range check did not simply reject everything
    let hit = vertical_seg.intersects_line(-1.0, 26000000.0);

    assert!(hit.is_some());
    assert_eq!(hit.unwrap(), Vector2I::new(95808800, -69808800));
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineParallelDetection )`.
#[test]
fn intersect_line_parallel_detection() {
    // Test parallel line detection using cross products

    // Horizontal segment with horizontal line
    let horizontal_seg = seg(0, 5, 10, 5);

    // Parallel but not collinear
    let intersects1 = horizontal_seg.intersects_line(0.0, 8.0); // y = 8
    assert!(intersects1.is_none());

    // Collinear (segment lies on line)
    let intersects2 = horizontal_seg.intersects_line(0.0, 5.0); // y = 5
    assert!(intersects2.is_some());
    assert_eq!(intersects2.unwrap(), Vector2I::new(5, 5)); // Midpoint

    // Diagonal segment with parallel line
    let diagonal_seg = seg(0, 0, 10, 10);

    // Parallel but offset
    let intersects3 = diagonal_seg.intersects_line(1.0, 3.0); // y = x + 3
    assert!(intersects3.is_none());

    // Collinear
    let intersects4 = diagonal_seg.intersects_line(1.0, 0.0); // y = x
    assert!(intersects4.is_some());
    assert_eq!(intersects4.unwrap(), Vector2I::new(5, 5)); // Midpoint
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLinePrecisionEdgeCases )`.
#[test]
fn intersect_line_precision_edge_cases() {
    // Test precision-sensitive cases

    // Very shallow segment with steep line
    let shallow_seg = seg(0, 100, 1000000, 101); // Almost horizontal

    let intersects = shallow_seg.intersects_line(1000.0, -499900.0);
    // Line: y = 1000x - 499900
    // This should intersect around x = 500, y ≈ 100.001

    if let Some(intersection) = intersects {
        assert!(intersection.x >= 0 && intersection.x <= 1000000);
        assert!(intersection.y >= 100 && intersection.y <= 101);
    }

    // Test with very large coordinates
    let large_seg = seg(1000000, 1000000, 2000000, 2000000);
    let intersects2 = large_seg.intersects_line(1.0, 0.0); // y = x
    assert!(intersects2.is_some());
    assert_eq!(intersects2.unwrap(), Vector2I::new(1500000, 1500000)); // Midpoint
}

/// Port of `BOOST_AUTO_TEST_CASE( IntersectLineZeroLengthSegments )`.
#[test]
fn intersect_line_zero_length_segments() {
    // Test with zero-length segments (points)

    let point = Vector2I::new(10, 20);
    let point_seg = Seg::new(point, point);

    // Point lies on line
    let intersects1 = point_seg.intersects_line(2.0, 0.0); // y = 2x
    assert!(intersects1.is_some()); // Point (10, 20) is on line y = 2x
    assert_eq!(intersects1.unwrap(), point);

    // Point does not lie on line
    let intersects2 = point_seg.intersects_line(3.0, 0.0); // y = 3x
    assert!(intersects2.is_none()); // Point (10, 20) not on line y = 3x (would be y = 30)

    // Point on horizontal line
    let intersects3 = point_seg.intersects_line(0.0, 20.0); // y = 20
    assert!(intersects3.is_some());
    assert_eq!(intersects3.unwrap(), point);
}
