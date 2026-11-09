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

//! Port of `libs/kimath/include/geometry/intersection.h` and
//! `libs/kimath/src/geometry/intersection.cpp`: the `INTERSECTABLE_GEOM` variant, the
//! `INTERSECTION_VISITOR` and every pair intersection.
//!
//! The C++ `std::visit( INTERSECTION_VISITOR( other, ... ), geom )` is
//! `IntersectionVisitor::new( &other, &mut out ).visit( &geom )`.  The argument order of each
//! pair function (which geometry is "first") follows the C++ dispatch exactly, since it
//! decides the order of the reported points and the reference segment of overlap tests.

use crate::geometry::circle::Circle;
use crate::geometry::half_line::HalfLine;
use crate::geometry::line::Line;
use crate::geometry::seg::Seg;
use crate::geometry::shape::Shape;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_ellipse::ShapeEllipse;
use crate::geometry::shape_utils::{box_to_segs, parameter_along};
use crate::math::box2::Box2I;
use crate::math::util::{ki_round, std_max, std_min};
use crate::math::vector2::Vector2I;

/// Port of `INTERSECTABLE_GEOM` (`std::variant<LINE, HALF_LINE, SEG, CIRCLE, SHAPE_ARC,
/// SHAPE_ELLIPSE, BOX2I>`).
#[derive(Clone, Debug)]
pub enum IntersectableGeom {
    /// `LINE`
    Line(Line),
    /// `HALF_LINE`
    HalfLine(HalfLine),
    /// `SEG`
    Seg(Seg),
    /// `CIRCLE`
    Circle(Circle),
    /// `SHAPE_ARC`
    Arc(ShapeArc),
    /// `SHAPE_ELLIPSE`
    Ellipse(ShapeEllipse),
    /// `BOX2I`
    Box(Box2I),
}

/// Port of `INTERSECTION_CONTACT`: how two geometries meet, beyond where they cross.
///
/// The point list cannot express either case.  A graze gives one point, same as a crossing.  A
/// shared extent gives none, same as no contact.  Set only inside the bounded extent of both
/// geometries.  `SHAPE_ELLIPSE` never sets either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntersectionContact {
    /// `m_Tangent`: touch at one point, no crossing.
    pub tangent: bool,
    /// `m_Overlapping`: collinear or concentric, more than one point shared.  Arcs are
    /// concentric only if the carriers they rebuild from their three points match exactly.
    pub overlapping: bool,
}

/// `OVERLAP_EPSILON` of `intersection.cpp`.
const OVERLAP_EPSILON: f64 = 1e-7;

/// The `LINE_LIKE` concept (`SEG`, `LINE`, `HALF_LINE`): geometries that carry a segment.
trait LineLike {
    /// `extentContains()`: a contact counts only where it lies on both geometries.
    fn extent_contains(&self, a_point: Vector2I) -> bool;
    /// `carrierSeg()`
    fn carrier_seg(&self) -> &Seg;
    /// `extentAlong()`: span along `a_ref`, unbounded ends included.
    fn extent_along(&self, a_ref: &Seg) -> (f64, f64);
}

impl LineLike for Seg {
    fn extent_contains(&self, a_point: Vector2I) -> bool {
        self.contains(a_point)
    }

    fn carrier_seg(&self) -> &Seg {
        self
    }

    fn extent_along(&self, a_ref: &Seg) -> (f64, f64) {
        // std::minmax( a, b ): ( b < a ) ? ( b, a ) : ( a, b )
        let a = parameter_along(a_ref, self.a);
        let b = parameter_along(a_ref, self.b);

        if b < a { (b, a) } else { (a, b) }
    }
}

impl LineLike for Line {
    fn extent_contains(&self, _a_point: Vector2I) -> bool {
        true
    }

    fn carrier_seg(&self) -> &Seg {
        self.contained_seg()
    }

    fn extent_along(&self, _a_ref: &Seg) -> (f64, f64) {
        (f64::NEG_INFINITY, f64::INFINITY)
    }
}

impl LineLike for HalfLine {
    fn extent_contains(&self, a_point: Vector2I) -> bool {
        self.contains(a_point)
    }

    fn carrier_seg(&self) -> &Seg {
        self.contained_seg()
    }

    fn extent_along(&self, a_ref: &Seg) -> (f64, f64) {
        let ray = self.contained_seg();
        let start = parameter_along(a_ref, ray.a);

        if parameter_along(a_ref, ray.b) > start {
            return (start, f64::INFINITY);
        }

        (f64::NEG_INFINITY, start)
    }
}

/// The `CIRCULAR` concept (`CIRCLE`, `SHAPE_ARC`): geometries that carry a circle.
trait Circular {
    /// `extentContains()`
    fn extent_contains(&self, a_point: Vector2I) -> bool;
    /// `carrierCircle()`
    fn carrier_circle(&self) -> Circle;
    /// The arc, for `sharesArcExtent()` (only two arcs can miss).
    fn as_arc(&self) -> Option<&ShapeArc>;
}

impl Circular for Circle {
    fn extent_contains(&self, _a_point: Vector2I) -> bool {
        true
    }

    fn carrier_circle(&self) -> Circle {
        *self
    }

    fn as_arc(&self) -> Option<&ShapeArc> {
        None
    }
}

impl Circular for ShapeArc {
    fn extent_contains(&self, a_point: Vector2I) -> bool {
        self.collide_point(a_point, 0, None, None)
    }

    fn carrier_circle(&self) -> Circle {
        Circle::new(self.get_center(), ki_round(self.get_radius()))
    }

    fn as_arc(&self) -> Option<&ShapeArc> {
        Some(self)
    }
}

/// Port of `checkCollinearOverlap()`: collinear and sharing more than a point (no single
/// intersection).
fn check_collinear_overlap<A: LineLike, B: LineLike>(
    a_a: &A,
    a_b: &B,
    a_contact: Option<&mut IntersectionContact>,
) {
    let Some(contact) = a_contact else {
        return;
    };

    if contact.overlapping {
        return;
    }

    let ref_seg = a_a.carrier_seg();

    if !ref_seg.collinear(a_b.carrier_seg()) {
        return;
    }

    let (a_low, a_high) = a_a.extent_along(ref_seg);
    let (b_low, b_high) = a_b.extent_along(ref_seg);

    if std_min(a_high, b_high) - std_max(a_low, b_low) > OVERLAP_EPSILON {
        contact.overlapping = true;
    }
}

/// Port of `checkLineTangency()`: a graze touches the carrier circle once.
fn check_line_tangency<L: LineLike, C: Circular>(
    a_line: &L,
    a_circular: &C,
    a_contact: Option<&mut IntersectionContact>,
) {
    let Some(contact) = a_contact else {
        return;
    };

    if contact.tangent {
        return;
    }

    let touches = a_circular
        .carrier_circle()
        .intersect_line(a_line.carrier_seg());

    if touches.len() == 1
        && a_line.extent_contains(touches[0])
        && a_circular.extent_contains(touches[0])
    {
        contact.tangent = true;
    }
}

/// Port of `sharesArcExtent()`: a full circle covers its whole carrier; only two arcs can miss.
fn shares_arc_extent<A: Circular, B: Circular>(a_a: &A, a_b: &B) -> bool {
    match (a_a.as_arc(), a_b.as_arc()) {
        (Some(arc_a), Some(arc_b)) => {
            arc_a.collide_point(arc_b.get_arc_mid(), 0, None, None)
                || arc_b.collide_point(arc_a.get_arc_mid(), 0, None, None)
        }
        _ => true,
    }
}

/// Port of `checkCircularContact()`: circular pairs graze at one point, or share a carrier and
/// run together.
fn check_circular_contact<A: Circular, B: Circular>(
    a_a: &A,
    a_b: &B,
    a_contact: Option<&mut IntersectionContact>,
) {
    let Some(contact) = a_contact else {
        return;
    };

    let circle_a = a_a.carrier_circle();
    let circle_b = a_b.carrier_circle();

    if circle_a.center == circle_b.center && circle_a.radius == circle_b.radius {
        if !contact.overlapping && shares_arc_extent(a_a, a_b) {
            contact.overlapping = true;
        }

        return;
    }

    if contact.tangent {
        return;
    }

    let touches = circle_a.intersect(&circle_b);

    if touches.len() == 1 && a_a.extent_contains(touches[0]) && a_b.extent_contains(touches[0]) {
        contact.tangent = true;
    }
}

type Out = Vec<Vector2I>;
type Contact<'c> = Option<&'c mut IntersectionContact>;

/// `findIntersections( SEG, SEG )`
fn seg_seg(a_seg_a: &Seg, a_seg_b: &Seg, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_seg_a, a_seg_b, c);

    if let Some(intersection) = a_seg_a.intersect(a_seg_b) {
        out.push(intersection);
    }
}

/// `findIntersections( SEG, LINE )`
fn seg_line(a_seg: &Seg, a_line: &Line, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_seg, a_line, c);

    if let Some(intersection) = a_line.intersect_seg(a_seg) {
        out.push(intersection);
    }
}

/// `findIntersections( SEG, HALF_LINE )`
fn seg_half_line(a_seg: &Seg, a_half_line: &HalfLine, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_seg, a_half_line, c);

    if let Some(intersection) = a_half_line.intersect_seg(a_seg) {
        out.push(intersection);
    }
}

/// `findIntersections( SEG, CIRCLE )`
fn seg_circle(a_seg: &Seg, a_circle: &Circle, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_seg, a_circle, c);

    out.extend(a_circle.intersect_seg(a_seg));
}

/// `findIntersections( SEG, SHAPE_ARC )`
fn seg_arc(a_seg: &Seg, a_arc: &ShapeArc, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_seg, a_arc, c);

    let mut intersections = Vec::new();
    a_arc.intersect_line(a_seg, &mut intersections);

    // Find only the intersections that are within the segment
    for intersection in intersections {
        if a_seg.contains(intersection) {
            out.push(intersection);
        }
    }
}

/// `findIntersections( LINE, LINE )`
fn line_line(a_line_a: &Line, a_line_b: &Line, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_line_a, a_line_b, c);

    if let Some(intersection) = a_line_a.intersect(a_line_b) {
        out.push(intersection);
    }
}

/// `findIntersections( LINE, HALF_LINE )`
fn line_half_line(a_line: &Line, a_half_line: &HalfLine, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_line, a_half_line, c);

    // Intersect as two infinite lines
    let intersection =
        a_half_line
            .contained_seg()
            .intersect_with(a_line.contained_seg(), false, true);

    // No intersection at all (parallel, or passes on the other side of the start point)
    let Some(intersection) = intersection else {
        return;
    };

    if a_half_line.contains(intersection) {
        out.push(intersection);
    }
}

/// `findIntersections( HALF_LINE, HALF_LINE )`
fn half_line_half_line(a_a: &HalfLine, a_b: &HalfLine, out: &mut Out, c: Contact<'_>) {
    check_collinear_overlap(a_a, a_b, c);

    if let Some(intersection) = a_a.intersect(a_b) {
        out.push(intersection);
    }
}

/// `findIntersections( CIRCLE, LINE )`
fn circle_line(a_circle: &Circle, a_line: &Line, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_line, a_circle, c);

    out.extend(a_circle.intersect_line(a_line.contained_seg()));
}

/// `findIntersections( CIRCLE, HALF_LINE )`
fn circle_half_line(a_circle: &Circle, a_half_line: &HalfLine, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_half_line, a_circle, c);

    for intersection in a_circle.intersect_line(a_half_line.contained_seg()) {
        if a_half_line.contains(intersection) {
            out.push(intersection);
        }
    }
}

/// `findIntersections( CIRCLE, CIRCLE )`
fn circle_circle(a_a: &Circle, a_b: &Circle, out: &mut Out, c: Contact<'_>) {
    check_circular_contact(a_a, a_b, c);

    out.extend(a_a.intersect(a_b));
}

/// `findIntersections( CIRCLE, SHAPE_ARC )`
fn circle_arc(a_circle: &Circle, a_arc: &ShapeArc, out: &mut Out, c: Contact<'_>) {
    check_circular_contact(a_circle, a_arc, c);

    a_arc.intersect_circle(a_circle, out);
}

/// `findIntersections( SHAPE_ARC, SHAPE_ARC )`
fn arc_arc(a_a: &ShapeArc, a_b: &ShapeArc, out: &mut Out, c: Contact<'_>) {
    check_circular_contact(a_a, a_b, c);

    a_a.intersect_arc(a_b, out);
}

/// `findIntersections( SHAPE_ARC, LINE )`
fn arc_line(a_arc: &ShapeArc, a_line: &Line, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_line, a_arc, c);

    let mut intersections = Vec::new();
    a_arc.intersect_line(a_line.contained_seg(), &mut intersections);

    out.extend(intersections);
}

/// `findIntersections( SHAPE_ARC, HALF_LINE )`
fn arc_half_line(a_arc: &ShapeArc, a_half_line: &HalfLine, out: &mut Out, c: Contact<'_>) {
    check_line_tangency(a_half_line, a_arc, c);

    let mut intersections = Vec::new();
    a_arc.intersect_line(a_half_line.contained_seg(), &mut intersections);

    for intersection in intersections {
        if a_half_line.contains(intersection) {
            out.push(intersection);
        }
    }
}

/*
 * Ellipse overloads take a contact but never set one.  An ellipse has no carrier seg or circle to
 * reduce to.  A caller that refuses ambiguity accepts a grazing ellipse as a clean crossing.
 */

/// `findIntersections( SHAPE_ELLIPSE, SEG )`
fn ellipse_seg(a_ellipse: &ShapeEllipse, a_seg: &Seg, out: &mut Out) {
    out.extend(a_ellipse.intersect_seg(a_seg, false));
}

/// `findIntersections( SHAPE_ELLIPSE, LINE )`
fn ellipse_line(a_ellipse: &ShapeEllipse, a_line: &Line, out: &mut Out) {
    out.extend(a_ellipse.intersect_seg(a_line.contained_seg(), true));
}

/// `findIntersections( SHAPE_ELLIPSE, HALF_LINE )`
fn ellipse_half_line(a_ellipse: &ShapeEllipse, a_half_line: &HalfLine, out: &mut Out) {
    for intersection in a_ellipse.intersect_seg(a_half_line.contained_seg(), true) {
        if a_half_line.contains(intersection) {
            out.push(intersection);
        }
    }
}

/// `findIntersections( SHAPE_ELLIPSE, CIRCLE )`
fn ellipse_circle(a_ellipse: &ShapeEllipse, a_circle: &Circle, out: &mut Out) {
    out.extend(a_ellipse.intersect_circle(a_circle));
}

/// `findIntersections( SHAPE_ELLIPSE, SHAPE_ARC )`
fn ellipse_arc(a_ellipse: &ShapeEllipse, a_arc: &ShapeArc, out: &mut Out) {
    out.extend(a_ellipse.intersect_arc(a_arc));
}

/// `findIntersections( SHAPE_ELLIPSE, SHAPE_ELLIPSE )`
fn ellipse_ellipse(a_a: &ShapeEllipse, a_b: &ShapeEllipse, out: &mut Out) {
    out.extend(a_a.intersect_ellipse(a_b));
}

/// Port of `INTERSECTION_VISITOR`: visits `IntersectableGeom` objects with another one (held
/// as state) and appends the intersections to a vector (which does not have to be empty).
pub struct IntersectionVisitor<'a> {
    /// `m_otherGeometry`
    other_geometry: &'a IntersectableGeom,
    /// `m_intersections`
    intersections: &'a mut Vec<Vector2I>,
    /// `m_contact`: flags are only set, never cleared, so one contact accumulates across visits.
    contact: Option<&'a mut IntersectionContact>,
}

impl<'a> IntersectionVisitor<'a> {
    /// Port of `INTERSECTION_VISITOR( aOtherGeometry, aIntersections )`.
    pub fn new(
        a_other_geometry: &'a IntersectableGeom,
        a_intersections: &'a mut Vec<Vector2I>,
    ) -> Self {
        IntersectionVisitor {
            other_geometry: a_other_geometry,
            intersections: a_intersections,
            contact: None,
        }
    }

    /// Port of `INTERSECTION_VISITOR( aOtherGeometry, aIntersections, aContact )`.
    pub fn with_contact(
        a_other_geometry: &'a IntersectableGeom,
        a_intersections: &'a mut Vec<Vector2I>,
        a_contact: &'a mut IntersectionContact,
    ) -> Self {
        IntersectionVisitor {
            other_geometry: a_other_geometry,
            intersections: a_intersections,
            contact: Some(a_contact),
        }
    }

    /// `std::visit( visitor, aGeom )`: dispatch to the `operator()` for the kind of `a_geom`.
    pub fn visit(&mut self, a_geom: &IntersectableGeom) {
        match a_geom {
            IntersectableGeom::Seg(s) => self.visit_seg(s),
            IntersectableGeom::Line(l) => self.visit_line(l),
            IntersectableGeom::HalfLine(h) => self.visit_half_line(h),
            IntersectableGeom::Circle(c) => self.visit_circle(c),
            IntersectableGeom::Arc(a) => self.visit_arc(a),
            IntersectableGeom::Ellipse(e) => self.visit_ellipse(e),
            IntersectableGeom::Box(b) => self.visit_box(b),
        }
    }

    /// Port of `operator()( const SEG& )`.
    pub fn visit_seg(&mut self, a_seg: &Seg) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;

        match self.other_geometry {
            G::Box(b) => {
                // Seg-Rect via decomposition into segments
                for a_rect_seg in &box_to_segs(b) {
                    seg_seg(a_seg, a_rect_seg, out, self.contact.as_deref_mut());
                }
            }
            // Ellipse-Seg
            G::Ellipse(e) => ellipse_seg(e, a_seg, out),
            // In all other segment comparisons, the SEG is the first argument
            G::Seg(o) => seg_seg(a_seg, o, out, self.contact.as_deref_mut()),
            G::Line(o) => seg_line(a_seg, o, out, self.contact.as_deref_mut()),
            G::HalfLine(o) => seg_half_line(a_seg, o, out, self.contact.as_deref_mut()),
            G::Circle(o) => seg_circle(a_seg, o, out, self.contact.as_deref_mut()),
            G::Arc(o) => seg_arc(a_seg, o, out, self.contact.as_deref_mut()),
        }
    }

    /// Port of `operator()( const LINE& )`.
    pub fn visit_line(&mut self, a_line: &Line) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;
        let c = self.contact.as_deref_mut();

        match self.other_geometry {
            // Seg-Line, Line-Line, Circle-Line, Arc-Line, Ellipse-Line
            G::Seg(o) => seg_line(o, a_line, out, c),
            G::Line(o) => line_line(o, a_line, out, c),
            G::Circle(o) => circle_line(o, a_line, out, c),
            G::Arc(o) => arc_line(o, a_line, out, c),
            G::Ellipse(o) => ellipse_line(o, a_line, out),
            // Line-HalfLine
            G::HalfLine(o) => line_half_line(a_line, o, out, c),
            // Line-Rect via decomposition into segments
            G::Box(b) => {
                let mut c = c;

                for a_rect_seg in &box_to_segs(b) {
                    seg_line(a_rect_seg, a_line, out, c.as_deref_mut());
                }
            }
        }
    }

    /// Port of `operator()( const HALF_LINE& )`.
    pub fn visit_half_line(&mut self, a_half_line: &HalfLine) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;
        let c = self.contact.as_deref_mut();

        match self.other_geometry {
            // Seg-HalfLine, HalfLine-HalfLine, Circle-HalfLine, Arc-HalfLine, Ellipse-HalfLine
            G::Seg(o) => seg_half_line(o, a_half_line, out, c),
            G::HalfLine(o) => half_line_half_line(o, a_half_line, out, c),
            G::Circle(o) => circle_half_line(o, a_half_line, out, c),
            G::Arc(o) => arc_half_line(o, a_half_line, out, c),
            G::Ellipse(o) => ellipse_half_line(o, a_half_line, out),
            // Line-HalfLine
            G::Line(o) => line_half_line(o, a_half_line, out, c),
            // HalfLine-Rect via decomposition into segments
            G::Box(b) => {
                let mut c = c;

                for a_rect_seg in &box_to_segs(b) {
                    seg_half_line(a_rect_seg, a_half_line, out, c.as_deref_mut());
                }
            }
        }
    }

    /// Port of `operator()( const CIRCLE& )`.
    pub fn visit_circle(&mut self, a_circle: &Circle) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;
        let c = self.contact.as_deref_mut();

        match self.other_geometry {
            // Seg-Circle, Circle-Circle, Ellipse-Circle
            G::Seg(o) => seg_circle(o, a_circle, out, c),
            G::Circle(o) => circle_circle(o, a_circle, out, c),
            G::Ellipse(o) => ellipse_circle(o, a_circle, out),
            // Circle-Arc, Circle-Line, Circle-HalfLine
            G::Arc(o) => circle_arc(a_circle, o, out, c),
            G::Line(o) => circle_line(a_circle, o, out, c),
            G::HalfLine(o) => circle_half_line(a_circle, o, out, c),
            // Circle-Rect via decomposition into segments
            G::Box(b) => {
                let mut c = c;

                for a_rect_seg in &box_to_segs(b) {
                    seg_circle(a_rect_seg, a_circle, out, c.as_deref_mut());
                }
            }
        }
    }

    /// Port of `operator()( const SHAPE_ARC& )`.
    pub fn visit_arc(&mut self, a_arc: &ShapeArc) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;
        let c = self.contact.as_deref_mut();

        match self.other_geometry {
            // Seg-Arc, Circle-Arc, Arc-Arc, Ellipse-Arc
            G::Seg(o) => seg_arc(o, a_arc, out, c),
            G::Circle(o) => circle_arc(o, a_arc, out, c),
            G::Arc(o) => arc_arc(o, a_arc, out, c),
            G::Ellipse(o) => ellipse_arc(o, a_arc, out),
            // Arc-Line, Arc-HalfLine
            G::Line(o) => arc_line(a_arc, o, out, c),
            G::HalfLine(o) => arc_half_line(a_arc, o, out, c),
            // Arc-Rect via decomposition into segments
            G::Box(b) => {
                let mut c = c;

                for a_rect_seg in &box_to_segs(b) {
                    seg_arc(a_rect_seg, a_arc, out, c.as_deref_mut());
                }
            }
        }
    }

    /// Port of `operator()( const SHAPE_ELLIPSE& )`: the ellipse is always the first argument.
    pub fn visit_ellipse(&mut self, a_ellipse: &ShapeEllipse) {
        use IntersectableGeom as G;
        let out = &mut *self.intersections;

        match self.other_geometry {
            G::Seg(o) => ellipse_seg(a_ellipse, o, out),
            G::Line(o) => ellipse_line(a_ellipse, o, out),
            G::HalfLine(o) => ellipse_half_line(a_ellipse, o, out),
            G::Circle(o) => ellipse_circle(a_ellipse, o, out),
            G::Arc(o) => ellipse_arc(a_ellipse, o, out),
            G::Ellipse(o) => ellipse_ellipse(a_ellipse, o, out),
            // Ellipse-Rect via decomposition into segments
            G::Box(b) => {
                for a_rect_seg in &box_to_segs(b) {
                    ellipse_seg(a_ellipse, a_rect_seg, out);
                }
            }
        }
    }

    /// Port of `operator()( const BOX2I& )`: defer to the SEG visitor repeatedly.
    ///
    /// Note - in some cases, points can be repeated in the intersection list if that's an
    /// issue, both directions of the visitor can be implemented to take care of that.
    pub fn visit_box(&mut self, a_rect: &Box2I) {
        let segs = box_to_segs(a_rect);

        for seg in &segs {
            self.visit_seg(seg);
        }
    }
}
