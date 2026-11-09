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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_distance.cpp` (suite
//! `SPSDistance`).

mod common_poly;

use common_poly::{build_hollow_square, build_polyset, build_square_chain};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{build_h_seg, is_within};

/// Mock up a conversion function
const IU_PER_MM: f64 = 1e3;

fn millimeter2iu(mm: f64) -> i32 {
    (if mm < 0.0 {
        mm * IU_PER_MM - 0.5
    } else {
        mm * IU_PER_MM + 0.5
    }) as i32
}

/// Port of `SPS_DISTANCE_TO_SEG_CASE`.
struct SpsDistanceToSegCase {
    case_name: &'static str,
    polyset: ShapePolySet,
    seg: Seg,
    seg_width: i32,
    exp_dist: i32,
}

fn get_sps_seg_dist_cases() -> Vec<SpsDistanceToSegCase> {
    let v = |x: f64, y: f64| Vector2I::new(millimeter2iu(x), millimeter2iu(y));
    let mm = millimeter2iu;

    // Single 10mm square at origin
    let square_10mm_0_0 = build_polyset(&[build_square_chain(mm(10.0), Vector2I::new(0, 0))]);

    // Double square: 10mm each, one at (0, 0), one at (10, 0)
    let squares_10mm_0_0_and_20_0 = build_polyset(&[
        build_square_chain(mm(10.0), Vector2I::new(0, 0)),
        build_square_chain(mm(10.0), v(20.0, 0.0)),
    ]);

    // Hollow square: 10mm hole in 20mm square, at origin
    let _hollow_square_20_10_at_0_0 = build_hollow_square(mm(20.0), mm(10.0), Vector2I::new(0, 0));

    vec![
        SpsDistanceToSegCase {
            case_name: "Square poly -> 1D segment",
            polyset: square_10mm_0_0.clone(),
            seg: build_h_seg(v(0.0, 15.0), mm(10.0)),
            seg_width: mm(0.0), // 1-d segment
            exp_dist: mm(10.0),
        },
        SpsDistanceToSegCase {
            case_name: "Square poly -> 2D (thick) segment",
            polyset: square_10mm_0_0.clone(),
            seg: build_h_seg(v(0.0, 15.0), mm(10.0)),
            seg_width: mm(2.0), // thick segment
            exp_dist: mm(9.0),
        },
        SpsDistanceToSegCase {
            case_name: "Two Squares poly -> 2D segment (nearest second square)",
            polyset: squares_10mm_0_0_and_20_0,
            seg: build_h_seg(v(15.0, 15.0), mm(10.0)),
            seg_width: mm(2.0), // thick segment
            exp_dist: mm(9.0),  // from line to second square
        },
        SpsDistanceToSegCase {
            case_name: "Square poly -> one intersect",
            polyset: square_10mm_0_0.clone(),
            seg: build_h_seg(v(-5.0, 0.0), mm(10.0)),
            seg_width: mm(0.0), // 1-d segment
            exp_dist: mm(0.0),  // intersect
        },
        SpsDistanceToSegCase {
            case_name: "Square poly -> multiple intersection",
            polyset: square_10mm_0_0.clone(),
            seg: build_h_seg(v(-5.0, 0.0), mm(20.0)),
            seg_width: mm(0.0), // 1-d segment
            exp_dist: mm(0.0),  // intersect
        },
        SpsDistanceToSegCase {
            case_name: "Square poly -> 1D seg touching",
            polyset: square_10mm_0_0.clone(),
            // touch left side at (-5, 0)
            seg: build_h_seg(v(-10.0, 0.0), mm(5.0)),
            seg_width: mm(0.0), // 2D segment
            exp_dist: mm(0.0),  // intersect
        },
        SpsDistanceToSegCase {
            case_name: "Square poly -> 2D seg (end cap is nearest)",
            polyset: square_10mm_0_0,
            seg: build_h_seg(v(-20.0, 0.0), mm(10.0)),
            seg_width: mm(2.0), // 2D segment, 1mm cap radius
            exp_dist: mm(4.0),  // 4mm short, 5mm to wire end, -1mm radius
        },
    ]
}

/// Check segment distances
#[test]
fn seg_distance() {
    for c in get_sps_seg_dist_cases() {
        let polyset = c.polyset.clone();
        // int dist = sqrt( ... ) - ( width / 2 ): double arithmetic, truncated
        let dist = ((polyset.squared_distance_to_seg(&c.seg, None) as f64).sqrt()
            - (c.seg_width / 2) as f64) as i32;

        // right answer?
        assert!(
            is_within(dist, c.exp_dist, 1),
            "{}: {dist} vs {}",
            c.case_name,
            c.exp_dist
        );
    }
}
