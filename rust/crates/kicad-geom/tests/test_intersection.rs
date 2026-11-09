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

//! Port of `qa/tests/libs/kimath/geometry/test_intersection.cpp` (suite `Intersection`).

use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::intersection::{
    IntersectableGeom as G, IntersectionContact, IntersectionVisitor,
};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::math::vector2::Vector2I;

fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn seg(ax: i32, ay: i32, bx: i32, by: i32) -> G {
    G::Seg(Seg::new(pt(ax, ay), pt(bx, by)))
}

fn circle(x: i32, y: i32, r: i32) -> G {
    G::Circle(Circle::new(pt(x, y), r))
}

fn arc(a: (i32, i32), m: (i32, i32), e: (i32, i32)) -> G {
    G::Arc(ShapeArc::from_start_mid_end(
        pt(a.0, a.1),
        pt(m.0, m.1),
        pt(e.0, e.1),
        0,
    ))
}

fn contact_of(a_a: &G, a_b: &G) -> IntersectionContact {
    let mut intersections = Vec::new();
    let mut contact = IntersectionContact::default();
    IntersectionVisitor::with_contact(a_b, &mut intersections, &mut contact).visit(a_a);
    contact
}

#[test]
fn crossing_geometry_reports_no_special_contact() {
    let contact = contact_of(&seg(0, 0, 100, 0), &seg(50, -50, 50, 50));
    assert!(!contact.tangent);
    assert!(!contact.overlapping);
}

#[test]
fn collinear_segments_sharing_an_extent_overlap() {
    let contact = contact_of(&seg(0, 0, 100, 0), &seg(20, 0, 80, 0));

    // The point list gives one point here, same as a crossing.  Hence the flag.
    assert!(contact.overlapping);
    assert!(!contact.tangent);
}

/// A shared carrier is not enough.  Only a shared extent counts.
#[test]
fn segments_without_a_shared_extent_do_not_overlap() {
    let source = seg(0, 0, 100, 0);

    assert!(!contact_of(&source, &seg(100, 0, 200, 0)).overlapping); // meeting at one end
    assert!(!contact_of(&source, &seg(200, 0, 300, 0)).overlapping); // elsewhere on the line
    assert!(!contact_of(&source, &seg(0, 10, 100, 10)).overlapping); // parallel, off the line
}

#[test]
fn segment_grazing_a_circle_is_tangent() {
    let source = seg(0, 0, 100, 0);

    assert!(contact_of(&source, &circle(50, 50, 50)).tangent);
    assert!(!contact_of(&seg(0, 25, 100, 25), &circle(50, 50, 50)).tangent);

    // Carrier line grazes it.  Touch point is off the segment.
    assert!(!contact_of(&source, &circle(500, 50, 50)).tangent);
}

#[test]
fn segment_grazing_an_arc_is_tangent_only_where_the_arc_runs() {
    // Upper half of a circle above the segment.  Arc dips down to touch.
    let touching = arc((0, 100), (50, 50), (100, 100));
    let away_from_touch = arc((100, 100), (50, 150), (0, 100));

    assert!(contact_of(&seg(0, 50, 100, 50), &touching).tangent);
    assert!(!contact_of(&seg(0, 50, 100, 50), &away_from_touch).tangent);
}

#[test]
fn tangent_circles_report_tangency() {
    assert!(contact_of(&circle(0, 0, 50), &circle(100, 0, 50)).tangent);
    assert!(!contact_of(&circle(0, 0, 50), &circle(80, 0, 50)).tangent);
    assert!(!contact_of(&circle(0, 0, 50), &circle(500, 0, 50)).tangent);
}

#[test]
fn arcs_on_a_common_circle_overlap_only_when_their_sweeps_do() {
    // All points sit on r=100 about the origin.  Rebuilt carriers match exactly.
    let first = arc((100, 0), (80, 60), (60, 80));
    let overlapping = arc((80, 60), (60, 80), (0, 100));
    let touching_at_one_end = arc((60, 80), (0, 100), (-60, 80));

    assert!(contact_of(&first, &overlapping).overlapping);
    assert!(!contact_of(&first, &touching_at_one_end).overlapping);
}

#[test]
fn contact_accumulates_across_visits() {
    let mut intersections = Vec::new();
    let mut contact = IntersectionContact::default();
    let source = seg(0, 0, 100, 0);

    IntersectionVisitor::with_contact(&seg(50, -50, 50, 50), &mut intersections, &mut contact)
        .visit(&source);
    assert!(!contact.overlapping);

    IntersectionVisitor::with_contact(&seg(20, 0, 80, 0), &mut intersections, &mut contact)
        .visit(&source);
    assert!(contact.overlapping);
}
