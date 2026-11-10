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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of the creepage shape classes of `pcbnew/drc/drc_creepage_utils.h` (`CREEP_SHAPE`,
//! `CU_SHAPE`, `BE_SHAPE`, `CU_SHAPE_SEGMENT`, `CU_SHAPE_CIRCLE`, `CU_SHAPE_ARC`,
//! `BE_SHAPE_POINT`, `BE_SHAPE_CIRCLE`, `BE_SHAPE_ARC`) and of the shape helpers of
//! `drc_creepage_utils.cpp` (`compareShapes`, `areEquivalent`, `segmentIntersectsArc`,
//! `segments_intersect`, `BE_SHAPE_ARC::IsThereATangentPassingThroughPoint`).
//!
//! The C++ class hierarchy becomes one struct per concrete class plus the [`CreepShape`] enum
//! (the `CREEP_SHAPE*` held by the graph). A derived class embeds its base (`CU_SHAPE_ARC`
//! embeds `CU_SHAPE_CIRCLE`, `BE_SHAPE_ARC` embeds `BE_SHAPE_CIRCLE`). The virtual accessors
//! (`GetRadius`, `GetStartAngle`, ...) are methods of [`CreepShape`].

use kicad_geom::cmath;
use kicad_geom::geometry::intersection::{IntersectableGeom, IntersectionVisitor};
use kicad_geom::{ANGLE_360, EdaAngle, Seg, ShapeArc, Vector2D, Vector2I};
use kicad_pcb::ItemId;

/// The `BOARD_ITEM*` a creepage shape or a board edge refers to.
///
/// The C++ stores raw pointers: board items (pads, tracks, board and footprint drawings) and the
/// transient `PCB_SHAPE`s owned by `CREEPAGE_GRAPH::m_ownedBoardEdges` (Bezier pieces, NPTH hole
/// outlines). Pointer identity becomes the identity of this value; the owned shapes are referred
/// to by their index in the owning vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CreepItem {
    /// An item of the board (`BOARD_ITEM*` into the board).
    Item(ItemId),
    /// A transient `PCB_SHAPE` owned by the graph: index into
    /// `CreepageGraph::owned_board_edges` (`m_ownedBoardEdges`).
    OwnedEdge(usize),
}

/// Port of `CREEP_SHAPE::TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CreepShapeType {
    /// `UNDEFINED`
    Undefined = 0,
    /// `POINT_TYPE`
    PointType,
    /// `CIRCLE`
    Circle,
    /// `ARC`
    Arc,
}

/// The data members of `CREEP_SHAPE`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreepShapeBase {
    /// `m_conductive`
    pub conductive: bool,
    /// `m_parent`
    pub parent: Option<CreepItem>,
    /// `m_pos`
    pub pos: Vector2I,
    /// `m_type`
    pub type_: CreepShapeType,
}

impl CreepShapeBase {
    /// Port of the `CREEP_SHAPE()` constructor.
    pub fn new() -> Self {
        CreepShapeBase {
            conductive: false,
            parent: None,
            pos: Vector2I::new(0, 0),
            type_: CreepShapeType::Undefined,
        }
    }

    /// Port of the `CU_SHAPE()` constructor: a conductive shape.
    pub fn new_cu() -> Self {
        CreepShapeBase {
            conductive: true,
            ..CreepShapeBase::new()
        }
    }

    /// Port of the `BE_SHAPE()` constructor: a board edge shape.
    pub fn new_be() -> Self {
        CreepShapeBase {
            conductive: false,
            ..CreepShapeBase::new()
        }
    }
}

impl Default for CreepShapeBase {
    fn default() -> Self {
        CreepShapeBase::new()
    }
}

/// `int( aDouble )` / an implicit `double` to `int` conversion as the reference build executes
/// it (PORTING.md section 9).
#[inline]
pub(crate) fn to_int(x: f64) -> i32 {
    cmath::f64_to_i32_x86(x)
}

/// `VECTOR2I` -> `VECTOR2D` (lossless implicit conversion).
#[inline]
pub(crate) fn vd(v: Vector2I) -> Vector2D {
    Vector2D::new(v.x as f64, v.y as f64)
}

/// `VECTOR2D` -> `VECTOR2I` (the converting constructor: clamp, then truncate).
#[inline]
pub(crate) fn vi(v: Vector2D) -> Vector2I {
    Vector2I::from_vec(v)
}

/// `aA - aB` of two `VECTOR2I` as the reference build executes it: a point converted from a
/// NaN path end (`INT_MIN`, PORTING.md section 9) makes the `int` subtraction overflow, which
/// wraps on x86-64.
#[inline]
pub(crate) fn wrapping_sub(a_a: Vector2I, a_b: Vector2I) -> Vector2I {
    Vector2I::new(a_a.x.wrapping_sub(a_b.x), a_a.y.wrapping_sub(a_b.y))
}

/// Port of `CU_SHAPE_SEGMENT`: a conductive segment.
#[derive(Clone, Debug, PartialEq)]
pub struct CuShapeSegment {
    /// The `CREEP_SHAPE` members.
    pub base: CreepShapeBase,
    /// `m_start`
    pub start: Vector2I,
    /// `m_end`
    pub end: Vector2I,
    /// `m_width`
    pub width: f64,
}

impl CuShapeSegment {
    /// Port of `CU_SHAPE_SEGMENT( VECTOR2I aStart, VECTOR2I aEnd, double aWidth = 0 )`.
    pub fn new(a_start: Vector2I, a_end: Vector2I, a_width: f64) -> Self {
        let mut base = CreepShapeBase::new_cu();
        // ( aStart + aEnd ) / 2 is VECTOR2I::operator/( double ): KiROUND per component
        base.pos = (a_start + a_end) / 2.0;

        CuShapeSegment {
            base,
            start: a_start,
            end: a_end,
            width: a_width,
        }
    }

    /// `GetStart()`.
    pub fn start(&self) -> Vector2I {
        self.start
    }

    /// `GetEnd()`.
    pub fn end(&self) -> Vector2I {
        self.end
    }

    /// `GetWidth()`.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// Port of `CU_SHAPE_SEGMENT::GetRadius()`.
    pub fn radius(&self) -> i32 {
        (self.start - self.end).euclidean_norm() / 2 + to_int(self.width / 2.0)
    }
}

/// Port of `CU_SHAPE_CIRCLE`: a conductive circle. Its `m_type` stays `UNDEFINED`.
#[derive(Clone, Debug, PartialEq)]
pub struct CuShapeCircle {
    /// The `CREEP_SHAPE` members.
    pub base: CreepShapeBase,
    /// `m_radius` (a `double`; `GetRadius()` truncates it to `int`).
    pub radius: f64,
}

impl CuShapeCircle {
    /// Port of `CU_SHAPE_CIRCLE( VECTOR2I aPos, double aRadius = 0 )`.
    pub fn new(a_pos: Vector2I, a_radius: f64) -> Self {
        let mut base = CreepShapeBase::new_cu();
        base.pos = a_pos;

        CuShapeCircle {
            base,
            radius: a_radius,
        }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.base.pos
    }

    /// Port of `CU_SHAPE_CIRCLE::GetRadius()` (also `CU_SHAPE_ARC::GetRadius()`): `m_radius`
    /// converted to `int`.
    pub fn radius(&self) -> i32 {
        to_int(self.radius)
    }
}

/// Port of `CU_SHAPE_ARC`: a conductive arc.
#[derive(Clone, Debug, PartialEq)]
pub struct CuShapeArc {
    /// The `CU_SHAPE_CIRCLE` base.
    pub circle: CuShapeCircle,
    /// `m_width` (an `int`, read through `double GetWidth()`).
    pub width: i32,
    /// `m_startAngle`
    pub start_angle: EdaAngle,
    /// `m_endAngle`
    pub end_angle: EdaAngle,
    /// `m_startPoint`
    pub start_point: Vector2I,
    /// `m_endPoint`
    pub end_point: Vector2I,
}

impl CuShapeArc {
    /// Port of `CU_SHAPE_ARC( VECTOR2I aPos, double aRadius, EDA_ANGLE aStartAngle,
    /// EDA_ANGLE aEndAngle, VECTOR2D aStartPoint, VECTOR2D aEndPoint )`.
    pub fn new(
        a_pos: Vector2I,
        a_radius: f64,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
        a_start_point: Vector2D,
        a_end_point: Vector2D,
    ) -> Self {
        let mut circle = CuShapeCircle::new(a_pos, a_radius);
        circle.base.type_ = CreepShapeType::Arc;

        CuShapeArc {
            circle,
            width: 0,
            start_angle: a_start_angle,
            end_angle: a_end_angle,
            start_point: vi(a_start_point),
            end_point: vi(a_end_point),
        }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.circle.base.pos
    }

    /// `GetRadius()`.
    pub fn radius(&self) -> i32 {
        self.circle.radius()
    }

    /// `GetWidth()`.
    pub fn width(&self) -> f64 {
        self.width as f64
    }

    /// `SetWidth( double aW )`: stored as `int`.
    pub fn set_width(&mut self, a_w: f64) {
        self.width = to_int(a_w);
    }

    /// `GetStartAngle()`.
    pub fn start_angle(&self) -> EdaAngle {
        self.start_angle
    }

    /// `GetEndAngle()`.
    pub fn end_angle(&self) -> EdaAngle {
        self.end_angle
    }

    /// `GetStartPoint()`.
    pub fn start_point(&self) -> Vector2I {
        self.start_point
    }

    /// `GetEndPoint()`.
    pub fn end_point(&self) -> Vector2I {
        self.end_point
    }

    /// Port of `CU_SHAPE_ARC::AngleBetweenStartAndEnd()`.
    pub fn angle_between_start_and_end(&self, a_point: Vector2I) -> EdaAngle {
        angle_between_start_and_end(self.pos(), self.start_angle(), self.end_angle(), a_point)
    }
}

/// The common body of `BE_SHAPE_ARC::AngleBetweenStartAndEnd()` and
/// `CU_SHAPE_ARC::AngleBetweenStartAndEnd()`.
fn angle_between_start_and_end(
    pos: Vector2I,
    start_angle: EdaAngle,
    end_angle: EdaAngle,
    a_point: Vector2I,
) -> EdaAngle {
    let mut angle = EdaAngle::from_vector(wrapping_sub(a_point, pos));

    while angle < start_angle {
        angle += ANGLE_360;
    }

    while angle > end_angle + ANGLE_360 {
        angle -= ANGLE_360;
    }

    angle
}

/// Port of `BE_SHAPE_POINT`: a board edge point.
#[derive(Clone, Debug, PartialEq)]
pub struct BeShapePoint {
    /// The `CREEP_SHAPE` members.
    pub base: CreepShapeBase,
}

impl BeShapePoint {
    /// Port of `BE_SHAPE_POINT( VECTOR2I aPos )`.
    pub fn new(a_pos: Vector2I) -> Self {
        let mut base = CreepShapeBase::new_be();
        base.pos = a_pos;
        base.type_ = CreepShapeType::PointType;

        BeShapePoint { base }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.base.pos
    }
}

/// Port of `BE_SHAPE_CIRCLE`: a board edge circle.
#[derive(Clone, Debug, PartialEq)]
pub struct BeShapeCircle {
    /// The `CREEP_SHAPE` members.
    pub base: CreepShapeBase,
    /// `m_radius`
    pub radius: i32,
}

impl BeShapeCircle {
    /// Port of `BE_SHAPE_CIRCLE( VECTOR2I aPos = VECTOR2I( 0, 0 ), int aRadius = 0 )`.
    pub fn new(a_pos: Vector2I, a_radius: i32) -> Self {
        let mut base = CreepShapeBase::new_be();
        base.pos = a_pos;
        base.type_ = CreepShapeType::Circle;

        BeShapeCircle {
            base,
            radius: a_radius,
        }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.base.pos
    }

    /// `GetRadius()`.
    pub fn radius(&self) -> i32 {
        self.radius
    }
}

/// Port of `BE_SHAPE_ARC`: a board edge arc.
#[derive(Clone, Debug, PartialEq)]
pub struct BeShapeArc {
    /// The `BE_SHAPE_CIRCLE` base.
    pub circle: BeShapeCircle,
    /// `m_startAngle`
    pub start_angle: EdaAngle,
    /// `m_endAngle`
    pub end_angle: EdaAngle,
    /// `m_startPoint`
    pub start_point: Vector2I,
    /// `m_endPoint`
    pub end_point: Vector2I,
}

impl BeShapeArc {
    /// Port of `BE_SHAPE_ARC( VECTOR2I aPos, int aRadius, EDA_ANGLE aStartAngle,
    /// EDA_ANGLE aEndAngle, VECTOR2D aStartPoint, VECTOR2D aEndPoint )`.
    pub fn new(
        a_pos: Vector2I,
        a_radius: i32,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
        a_start_point: Vector2D,
        a_end_point: Vector2D,
    ) -> Self {
        let mut circle = BeShapeCircle::new(a_pos, a_radius);
        circle.base.type_ = CreepShapeType::Arc;

        BeShapeArc {
            circle,
            start_angle: a_start_angle,
            end_angle: a_end_angle,
            start_point: vi(a_start_point),
            end_point: vi(a_end_point),
        }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.circle.base.pos
    }

    /// `GetRadius()`.
    pub fn radius(&self) -> i32 {
        self.circle.radius
    }

    /// `GetStartAngle()`.
    pub fn start_angle(&self) -> EdaAngle {
        self.start_angle
    }

    /// `GetEndAngle()`.
    pub fn end_angle(&self) -> EdaAngle {
        self.end_angle
    }

    /// `GetStartPoint()`.
    pub fn start_point(&self) -> Vector2I {
        self.start_point
    }

    /// `GetEndPoint()`.
    pub fn end_point(&self) -> Vector2I {
        self.end_point
    }

    /// Port of `BE_SHAPE_ARC::AngleBetweenStartAndEnd()`.
    pub fn angle_between_start_and_end(&self, a_point: Vector2I) -> EdaAngle {
        angle_between_start_and_end(self.pos(), self.start_angle, self.end_angle, a_point)
    }

    /// Port of `BE_SHAPE_ARC::IsThereATangentPassingThroughPoint()`.
    pub fn is_there_a_tangent_passing_through_point(&self, a_point: &BeShapePoint) -> (bool, bool) {
        let r: f64 = self.radius() as f64;

        let new_point = a_point.pos() - self.pos();

        if (new_point.squared_euclidean_norm() as f64) <= r * r {
            // If the point is inside the arc
            return (false, false);
        }

        let test_angle = self.angle_between_start_and_end(a_point.pos());

        let start_angle = self.start_angle.as_radians();
        let end_angle = self.end_angle.as_radians();
        let point_angle = test_angle.as_radians();

        let greater_than_180 = (self.end_angle - self.start_angle) > EdaAngle::from_degrees(180.0);
        let nx = new_point.x as f64;
        let ny = new_point.y as f64;

        let mut connect_to_end_point =
            cmath::cos(start_angle) * nx + cmath::sin(start_angle) * ny >= r;

        if greater_than_180 {
            connect_to_end_point &= cmath::cos(end_angle) * nx + cmath::sin(end_angle) * ny <= r;
        }

        connect_to_end_point |= (cmath::cos(end_angle) * nx + cmath::sin(end_angle) * ny <= r)
            && (point_angle >= end_angle || point_angle <= start_angle);

        let first = !connect_to_end_point;

        connect_to_end_point = cmath::cos(end_angle) * nx + cmath::sin(end_angle) * ny >= r;

        if greater_than_180 {
            connect_to_end_point &=
                cmath::cos(start_angle) * nx + cmath::sin(start_angle) * ny <= r;
        }

        connect_to_end_point |= (cmath::cos(start_angle) * nx + cmath::sin(start_angle) * ny <= r)
            && (point_angle >= end_angle || point_angle <= start_angle);

        let second = !connect_to_end_point;
        (first, second)
    }
}

/// A `CREEP_SHAPE*` of the graph: one variant per concrete C++ class.
#[derive(Clone, Debug, PartialEq)]
pub enum CreepShape {
    /// `BE_SHAPE_POINT`
    BePoint(BeShapePoint),
    /// `BE_SHAPE_CIRCLE`
    BeCircle(BeShapeCircle),
    /// `BE_SHAPE_ARC`
    BeArc(BeShapeArc),
    /// `CU_SHAPE_SEGMENT`
    CuSegment(CuShapeSegment),
    /// `CU_SHAPE_CIRCLE`
    CuCircle(CuShapeCircle),
    /// `CU_SHAPE_ARC`
    CuArc(CuShapeArc),
}

impl CreepShape {
    /// The `CREEP_SHAPE` data members.
    pub fn base(&self) -> &CreepShapeBase {
        match self {
            CreepShape::BePoint(s) => &s.base,
            CreepShape::BeCircle(s) => &s.base,
            CreepShape::BeArc(s) => &s.circle.base,
            CreepShape::CuSegment(s) => &s.base,
            CreepShape::CuCircle(s) => &s.base,
            CreepShape::CuArc(s) => &s.circle.base,
        }
    }

    /// The `CREEP_SHAPE` data members, mutable.
    pub fn base_mut(&mut self) -> &mut CreepShapeBase {
        match self {
            CreepShape::BePoint(s) => &mut s.base,
            CreepShape::BeCircle(s) => &mut s.base,
            CreepShape::BeArc(s) => &mut s.circle.base,
            CreepShape::CuSegment(s) => &mut s.base,
            CreepShape::CuCircle(s) => &mut s.base,
            CreepShape::CuArc(s) => &mut s.circle.base,
        }
    }

    /// Port of the virtual `GetRadius()`.
    pub fn radius(&self) -> i32 {
        match self {
            CreepShape::BePoint(_) => 0,
            CreepShape::BeCircle(s) => s.radius(),
            CreepShape::BeArc(s) => s.radius(),
            CreepShape::CuSegment(s) => s.radius(),
            CreepShape::CuCircle(s) => s.radius(),
            CreepShape::CuArc(s) => s.radius(),
        }
    }

    /// Port of the virtual `GetStartAngle()`.
    pub fn start_angle(&self) -> EdaAngle {
        match self {
            CreepShape::BeArc(s) => s.start_angle(),
            CreepShape::CuArc(s) => s.start_angle(),
            _ => EdaAngle::from_degrees(0.0),
        }
    }

    /// Port of the virtual `GetEndAngle()`.
    pub fn end_angle(&self) -> EdaAngle {
        match self {
            CreepShape::BeArc(s) => s.end_angle(),
            CreepShape::CuArc(s) => s.end_angle(),
            _ => EdaAngle::from_degrees(0.0),
        }
    }

    /// Port of the virtual `GetStartPoint()`.
    pub fn start_point(&self) -> Vector2I {
        match self {
            CreepShape::BeArc(s) => s.start_point(),
            CreepShape::CuArc(s) => s.start_point(),
            _ => Vector2I::new(0, 0),
        }
    }

    /// Port of the virtual `GetEndPoint()`.
    pub fn end_point(&self) -> Vector2I {
        match self {
            CreepShape::BeArc(s) => s.end_point(),
            CreepShape::CuArc(s) => s.end_point(),
            _ => Vector2I::new(0, 0),
        }
    }

    /// `GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.base().pos
    }

    /// `GetType()`.
    pub fn type_(&self) -> CreepShapeType {
        self.base().type_
    }

    /// `GetParent()`.
    pub fn parent(&self) -> Option<CreepItem> {
        self.base().parent
    }

    /// `SetParent()`.
    pub fn set_parent(&mut self, a_parent: Option<CreepItem>) {
        self.base_mut().parent = a_parent;
    }

    /// `IsConductive()`.
    pub fn is_conductive(&self) -> bool {
        self.base().conductive
    }
}

/// Port of `compareShapes()`: the (not strictly weak) ordering `RemoveDuplicatedShapes` sorts
/// with. `None` is a null `CREEP_SHAPE*`.
pub fn compare_shapes(a: Option<&CreepShape>, b: Option<&CreepShape>) -> bool {
    let Some(a) = a else {
        return true;
    };

    let Some(b) = b else {
        return false;
    };

    if a.type_() != b.type_() {
        return a.type_() < b.type_();
    }

    if a.type_() == CreepShapeType::Undefined {
        return true;
    }

    if a.pos() != b.pos() {
        // VECTOR2::operator<: x first, then y
        let (pa, pb) = (a.pos(), b.pos());
        return pa.x < pb.x || (pa.x == pb.x && pa.y < pb.y);
    }

    if a.type_() == CreepShapeType::Circle {
        return a.radius() < b.radius();
    }

    false
}

/// Port of `areEquivalent()`.
pub fn are_equivalent(a: Option<&CreepShape>, b: Option<&CreepShape>) -> bool {
    let (a, b) = match (a, b) {
        (None, None) => return true,
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };

    if a.type_() != b.type_() {
        return false;
    }

    if a.type_() == CreepShapeType::PointType {
        return a.pos() == b.pos();
    }

    if a.type_() == CreepShapeType::Circle {
        return a.pos() == b.pos() && (a.radius() == b.radius());
    }

    false
}

/// Port of `segmentIntersectsArc()`: whether the segment `p1 p2` crosses the arc in its
/// interior (an intersection within 50 nm of either segment end is a touch, not a crossing).
/// The interior crossings are appended to `a_intersection_points` when given.
#[allow(clippy::too_many_arguments)]
pub fn segment_intersects_arc(
    p1: Vector2I,
    p2: Vector2I,
    center: Vector2I,
    radius: f64,
    start_angle: EdaAngle,
    end_angle: EdaAngle,
    a_intersection_points: Option<&mut Vec<Vector2I>>,
) -> bool {
    let segment = Seg::new(p1, p2);
    let start_point = Vector2I::new(
        to_int(radius * cmath::cos(start_angle.as_radians())),
        to_int(radius * cmath::sin(start_angle.as_radians())),
    );
    let arc =
        ShapeArc::from_center_start_angle(center, start_point + center, end_angle - start_angle, 0);

    let geom1 = IntersectableGeom::Seg(segment);
    let geom2 = IntersectableGeom::Arc(arc);

    let mut raw_points: Vec<Vector2I> = Vec::new();
    IntersectionVisitor::new(&geom2, &mut raw_points).visit(&geom1);

    // A path is allowed to end on the arc, so an intersection at either endpoint is a touch,
    // not a crossing. Only interior crossings count. Tolerance absorbs solver rounding.
    let tolerance: i64 = 50;
    let tolerance_sq: i64 = tolerance * tolerance;

    let coincident = |a: Vector2I, b: Vector2I| (a - b).squared_euclidean_norm() <= tolerance_sq;

    let filtered: Vec<Vector2I> = raw_points
        .iter()
        .copied()
        .filter(|&ip| !coincident(ip, p1) && !coincident(ip, p2))
        .collect();

    let any = !filtered.is_empty();

    if let Some(out) = a_intersection_points {
        out.extend(filtered);
    }

    any
}

/// Port of `segments_intersect()`: whether the segments `p1 q1` and `p2 q2` intersect,
/// excluding endpoint overlap. The intersections are appended to `a_intersection_points`.
pub fn segments_intersect(
    p1: Vector2I,
    q1: Vector2I,
    p2: Vector2I,
    q2: Vector2I,
    a_intersection_points: &mut Vec<Vector2I>,
) -> bool {
    if p1 == p2 || p1 == q2 || q1 == p2 || q1 == q2 {
        return false;
    }

    let geom1 = IntersectableGeom::Seg(Seg::new(p1, q1));
    let geom2 = IntersectableGeom::Seg(Seg::new(p2, q2));

    let start_count = a_intersection_points.len();

    IntersectionVisitor::new(&geom2, a_intersection_points).visit(&geom1);

    a_intersection_points.len() > start_count
}
