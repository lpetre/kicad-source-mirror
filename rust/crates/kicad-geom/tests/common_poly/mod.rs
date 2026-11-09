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

//! Shared poly-set test helpers: the polygon sets of `KI_TEST::CommonTestData`
//! (`qa/tests/libs/kimath/geometry/fixtures_geometry.h`), `GEOM_TEST::IsPolySetValid` and
//! `GEOM_TEST::FilletPolySet` (`geom_test_utils.h`), and `KI_TEST::BuildPolyset`,
//! `BuildHollowSquare`, `BuildRectChain`, `BuildSquareChain` (`qa_utils/geometry`).

#![allow(dead_code)]

use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{CommonTestData, is_outline_valid};

/// `KI_TEST::CommonTestData`: the polygon sets (the point / segment vectors are in `data`).
pub struct CommonPolyData {
    pub data: CommonTestData,
    pub empty_poly_set: ShapePolySet,
    pub unique_vertex_poly_set: ShapePolySet,
    pub solid_poly_set: ShapePolySet,
    pub holey_poly_set: ShapePolySet,
    pub curved_poly_wrap_round: ShapePolySet,
    pub holey_curved_poly_single: ShapePolySet,
    pub holey_curved_poly_multi: ShapePolySet,
    pub holey_curved_poly_inter: ShapePolySet,
}

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn arc(s: (i32, i32), m: (i32, i32), e: (i32, i32)) -> ShapeArc {
    ShapeArc::from_start_mid_end(v(s.0, s.1), v(m.0, m.1), v(e.0, e.1), 0)
}

impl CommonPolyData {
    /// Port of `CommonTestData::CommonTestData()`.
    pub fn new() -> Self {
        let data = CommonTestData::new();
        let mut unique_vertex_poly_set = ShapePolySet::new();
        let mut solid_poly_set = ShapePolySet::new();
        let mut holey_poly_set = ShapePolySet::new();

        let mut poly_line = ShapeLineChain::new();
        let mut hole = ShapeLineChain::new();

        // Create a polygon set with a unique vertex
        poly_line.append_point(data.unique_points[0], false);
        poly_line.set_closed(true);
        unique_vertex_poly_set.add_outline(&poly_line);

        // Create a polygon set without holes
        solid_poly_set.new_outline();
        solid_poly_set.new_outline();
        solid_poly_set.new_outline();

        // Adds a new squared outline
        poly_line.clear();

        for i in 0..4 {
            poly_line.append_point(data.holey_points[i], false);
        }

        poly_line.set_closed(true);
        holey_poly_set.add_outline(&poly_line);

        // Adds a new hole (a pentagon)
        for i in 4..9 {
            hole.append_point(data.holey_points[i], false);
        }

        hole.set_closed(true);
        holey_poly_set.add_hole(&hole, -1);

        // Adds a new hole (a triangle)
        hole.clear();

        for i in 9..12 {
            hole.append_point(data.holey_points[i], false);
        }

        hole.set_closed(true);
        holey_poly_set.add_hole(&hole, -1);

        //GENERATE CURVED POLYGON THAT CAUSES WRAPAROUND
        let mut curved_poly_wrap_round = ShapePolySet::new();
        let mut wrap_line = ShapeLineChain::new();
        wrap_line.append_arc(&arc(
            (-4300000, -6950000),
            (2000000, 0),
            (-4300000, 6950000),
        ));
        wrap_line.append_arc(&arc(
            (-4300000, 2200000),
            (-2700000, 0),
            (-4300000, -2200000),
        ));
        wrap_line.set_closed(true);
        curved_poly_wrap_round.add_outline(&wrap_line);

        // GENERATE CURVED POLYGON WITH HOLES
        let mut holey_curved_poly_single = ShapePolySet::new();

        // First shape:
        let mut oline0 = ShapeLineChain::new();
        oline0.append_point(v(244000, 180000), false);
        oline0.append_arc(&arc((582000, 232000), (614000, 354000), (450000, 436000)));
        oline0.append_arc(&arc((450000, 436000), (310000, 480000), (502000, 614000)));
        oline0.append_point(v(872000, 202000), false);
        oline0.set_closed(true);
        holey_curved_poly_single.add_outline(&oline0);

        let mut o0h1 = ShapeLineChain::new();
        o0h1.append_point(v(370000, 482000), false);
        o0h1.append_arc(&arc((342000, 576000), (436000, 552000), (474000, 622000)));
        o0h1.append_arc(&arc((474000, 622000), (534000, 520000), (518000, 470000)));
        o0h1.set_closed(true);
        holey_curved_poly_single.add_hole(&o0h1, -1);

        let mut o0h2 = ShapeLineChain::new();
        o0h2.append_arc(&arc((680000, 286000), (728000, 306000), (760000, 258000)));
        o0h2.append_arc(&arc((760000, 258000), (746000, 222000), (686000, 260000)));
        o0h2.set_closed(true);
        holey_curved_poly_single.add_hole(&o0h2, -1);

        let mut holey_curved_poly_multi = holey_curved_poly_single.clone();

        // Second shape:
        let mut oline1 = ShapeLineChain::new();
        oline1.append_point(v(640000, 840000), false);
        oline1.append_arc(&arc((829000, 959000), (990000, 736000), (951000, 461000)));
        oline1.append_arc(&arc((951000, 461000), (600000, 572000), (620000, 726000)));
        oline1.set_closed(true);
        holey_curved_poly_multi.add_outline(&oline1);

        let mut o1h1 = ShapeLineChain::new();
        o1h1.append_point(v(670000, 482000), false);
        o1h1.append_arc(&arc((642000, 576000), (736000, 652000), (774000, 622000)));
        o1h1.append_arc(&arc((774000, 622000), (834000, 520000), (818000, 470000)));
        o1h1.set_closed(true);
        holey_curved_poly_multi.add_hole(&o1h1, -1);

        let mut o1h2 = ShapeLineChain::new();
        o1h2.append_arc(&arc((680000, 286000), (728000, 306000), (760000, 258000)));
        o1h2.append_arc(&arc((760000, 258000), (746000, 222000), (686000, 260000)));
        o1h2.set_closed(true);
        holey_curved_poly_multi.add_hole(&o1h2, -1);

        // Intersecting shape:
        let mut holey_curved_poly_inter = ShapePolySet::new();
        let mut oline2 = ShapeLineChain::new();
        oline2.append_point(v(340000, 540000), false);
        oline2.append_arc(&arc((629000, 659000), (790000, 436000), (751000, 161000)));
        oline2.append_arc(&arc((651000, 161000), (300000, 272000), (320000, 426000)));
        oline2.set_closed(true);
        holey_curved_poly_inter.add_outline(&oline2);

        let mut o2h2 = ShapeLineChain::new();
        o2h2.append_arc(&arc((680000, 486000), (728000, 506000), (760000, 458000)));
        o2h2.append_arc(&arc((760000, 458000), (746000, 422000), (686000, 460000)));
        o2h2.set_closed(true);
        holey_curved_poly_inter.add_hole(&o2h2, -1);

        CommonPolyData {
            data,
            empty_poly_set: ShapePolySet::new(),
            unique_vertex_poly_set,
            solid_poly_set,
            holey_poly_set,
            curved_poly_wrap_round,
            holey_curved_poly_single,
            holey_curved_poly_multi,
            holey_curved_poly_inter,
        }
    }
}

/// Port of `GEOM_TEST::IsPolySetValid()`.
pub fn is_poly_set_valid(a_set: &ShapePolySet) -> bool {
    for i in 0..a_set.outline_count() {
        if !is_outline_valid(a_set.c_outline(i)) {
            return false;
        }

        for j in 0..a_set.hole_count(i) {
            if !is_outline_valid(a_set.c_hole(i, j)) {
                return false;
            }
        }
    }

    true
}

/// Port of `GEOM_TEST::FilletPolySet()`.
pub fn fillet_poly_set(a_poly_set: &mut ShapePolySet, a_radius: i32, a_error: i32) -> ShapePolySet {
    let mut filleted_poly_set = ShapePolySet::new();

    for i in 0..a_poly_set.outline_count() {
        let filleted = a_poly_set.fillet_polygon(a_radius as u32, a_error, i);

        filleted_poly_set.add_outline(&filleted[0]);
    }

    filleted_poly_set
}

/// Port of `KI_TEST::BuildRectChain()`.
pub fn build_rect_chain(a_size: Vector2I, a_centre: Vector2I) -> ShapeLineChain {
    let pts = [
        v(a_centre.x - a_size.x / 2, a_centre.y - a_size.y / 2),
        v(a_centre.x - a_size.x / 2, a_centre.y + a_size.y / 2),
        v(a_centre.x + a_size.x / 2, a_centre.y + a_size.y / 2),
        v(a_centre.x + a_size.x / 2, a_centre.y - a_size.y / 2),
    ];

    let mut chain = ShapeLineChain::from_points(&pts, false);
    chain.set_closed(true);

    chain
}

/// Port of `KI_TEST::BuildSquareChain()`.
pub fn build_square_chain(a_size: i32, a_centre: Vector2I) -> ShapeLineChain {
    build_rect_chain(v(a_size, a_size), a_centre)
}

/// Port of `KI_TEST::BuildPolyset()`.
pub fn build_polyset(a_outlines: &[ShapeLineChain]) -> ShapePolySet {
    let mut polyset = ShapePolySet::new();

    for outline in a_outlines {
        polyset.add_outline(outline);
    }

    polyset
}

/// Port of `KI_TEST::BuildHollowSquare()`.
pub fn build_hollow_square(
    a_outer_size: i32,
    a_inner_size: i32,
    a_centre: Vector2I,
) -> ShapePolySet {
    let mut polyset = ShapePolySet::new();

    polyset.add_outline(&build_rect_chain(v(a_outer_size, a_outer_size), a_centre));
    polyset.add_hole(
        &build_rect_chain(v(a_inner_size, a_inner_size), a_centre),
        -1,
    );

    polyset
}
