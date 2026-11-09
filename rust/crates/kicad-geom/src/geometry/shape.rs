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

//! Port of `libs/kimath/include/geometry/shape.h` and `libs/kimath/src/geometry/shape.cpp`,
//! plus the `SHAPE_LINE_CHAIN_BASE` section of `libs/kimath/src/geometry/shape_line_chain.cpp`.
//!
//! The C++ class hierarchy `SHAPE_BASE` / `SHAPE` / `SHAPE_LINE_CHAIN_BASE` becomes:
//!
//! - [`Shape`]: an object safe trait with the virtual interface of `SHAPE_BASE` and `SHAPE`,
//!   implemented by every concrete shape struct.
//! - [`ShapeRef`]: a borrowed view of a shape as an enum (returned by
//!   [`Shape::as_shape_ref`]); the collision code dispatches on it with an exhaustive `match`
//!   instead of `Type()` plus `static_cast`.
//! - [`ShapeLineChainBase`]: the abstract `SHAPE_LINE_CHAIN_BASE` (shared by
//!   `SHAPE_LINE_CHAIN`, `SHAPE_SIMPLE` and the poly-set triangle).  Rust has no way for a sub
//!   trait to override a super trait method, so the shared implementations of the
//!   `SHAPE_LINE_CHAIN_BASE` overrides are default methods with an `lcb_` prefix
//!   ([`ShapeLineChainBase::lcb_collide_point`], ...), and every implementor forwards the
//!   corresponding [`Shape`] method to them (or to its own override, as in C++).
//!
//! Out-parameters: the C++ collision functions take nullable `int* aActual` /
//! `VECTOR2I* aLocation` pointers and behave differently depending on whether they were passed
//! (for example "any collision will do" when no distance is requested).  The port keeps that
//! contract with `Option<&mut i32>` / `Option<&mut Vector2I>` parameters.
//!
//! See `rust/docs/design/geom-shapes.md`.

use std::fmt::Debug;

use crate::geometry::approximation::{ERROR_INSIDE, ErrorLoc};
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_circle::ShapeCircle;
use crate::geometry::shape_collisions;
use crate::geometry::shape_compound::ShapeCompound;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_null::ShapeNull;
use crate::geometry::shape_poly_set::{ShapePolySet, Tri};
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_segment::ShapeSegment;
use crate::geometry::shape_simple::ShapeSimple;
use crate::math::box2::Box2I;
use crate::math::util::rescale;
use crate::math::vector2::Vector2I;

/// Port of `SHAPE_TYPE`: lists all supported shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum ShapeType {
    /// `SH_RECT`: axis-aligned rectangle
    Rect = 0,
    /// `SH_SEGMENT`: line segment
    Segment,
    /// `SH_LINE_CHAIN`: line chain (polyline)
    LineChain,
    /// `SH_CIRCLE`: circle
    Circle,
    /// `SH_SIMPLE`: simple polygon
    Simple,
    /// `SH_POLY_SET`: set of polygons (with holes, etc.)
    PolySet,
    /// `SH_COMPOUND`: compound shape, consisting of multiple simple shapes
    Compound,
    /// `SH_ARC`: circular arc
    Arc,
    /// `SH_NULL`: empty shape (no shape...),
    Null,
    /// `SH_POLY_SET_TRIANGLE`: a single triangle belonging to a POLY_SET triangulation
    PolySetTriangle,
    /// `SH_ELLIPSE`: ellipse or elliptical arc (the shape itself is not ported yet)
    Ellipse,
}

/// Port of `SHAPE_TYPE_asString`.
pub fn shape_type_as_string(a: ShapeType) -> &'static str {
    match a {
        ShapeType::Rect => "SH_RECT",
        ShapeType::Segment => "SH_SEGMENT",
        ShapeType::LineChain => "SH_LINE_CHAIN",
        ShapeType::Circle => "SH_CIRCLE",
        ShapeType::Simple => "SH_SIMPLE",
        ShapeType::PolySet => "SH_POLY_SET",
        ShapeType::Compound => "SH_COMPOUND",
        ShapeType::Arc => "SH_ARC",
        ShapeType::Null => "SH_NULL",
        ShapeType::PolySetTriangle => "SH_POLY_SET_TRIANGLE",
        ShapeType::Ellipse => "SH_ELLIPSE",
    }
}

/// Port of `SHAPE::MIN_PRECISION_IU`: the minimum precision for all the points in a shape.
pub const MIN_PRECISION_IU: i32 = 4;

/// A borrowed view of a concrete shape, used to dispatch on the shape kind with an exhaustive
/// `match` where the C++ uses `Type()` and `static_cast`.
#[derive(Clone, Copy, Debug)]
pub enum ShapeRef<'a> {
    /// `SH_RECT`
    Rect(&'a ShapeRect),
    /// `SH_SEGMENT`
    Segment(&'a ShapeSegment),
    /// `SH_LINE_CHAIN`
    LineChain(&'a ShapeLineChain),
    /// `SH_CIRCLE`
    Circle(&'a ShapeCircle),
    /// `SH_SIMPLE`
    Simple(&'a ShapeSimple),
    /// `SH_POLY_SET`
    PolySet(&'a ShapePolySet),
    /// `SH_COMPOUND`
    Compound(&'a ShapeCompound),
    /// `SH_ARC`
    Arc(&'a ShapeArc),
    /// `SH_NULL`
    Null(&'a ShapeNull),
    /// `SH_POLY_SET_TRIANGLE`
    PolySetTriangle(&'a Tri),
}

impl<'a> ShapeRef<'a> {
    /// The shape as a trait object.
    pub fn as_dyn(self) -> &'a dyn Shape {
        match self {
            ShapeRef::Rect(s) => s,
            ShapeRef::Segment(s) => s,
            ShapeRef::LineChain(s) => s,
            ShapeRef::Circle(s) => s,
            ShapeRef::Simple(s) => s,
            ShapeRef::PolySet(s) => s,
            ShapeRef::Compound(s) => s,
            ShapeRef::Arc(s) => s,
            ShapeRef::Null(s) => s,
            ShapeRef::PolySetTriangle(s) => s,
        }
    }

    /// The shape type (`SHAPE_BASE::Type()`).
    pub fn type_(self) -> ShapeType {
        self.as_dyn().type_()
    }

    /// The shape as a `SHAPE_LINE_CHAIN_BASE`, for the three kinds deriving from it in C++.
    pub fn as_line_chain_base(self) -> Option<&'a dyn ShapeLineChainBase> {
        match self {
            ShapeRef::LineChain(s) => Some(s),
            ShapeRef::Simple(s) => Some(s),
            ShapeRef::PolySetTriangle(s) => Some(s),
            _ => None,
        }
    }
}

/// Port of `SHAPE_BASE` and `SHAPE`: an abstract shape on the 2D plane.
///
/// Polymorphic shapes are held as `Box<dyn Shape>` where C++ owns a `SHAPE*` (or a
/// `std::unique_ptr<SHAPE>`), as `std::sync::Arc<dyn Shape>` where it uses
/// `std::shared_ptr<SHAPE>`, and borrowed as `&dyn Shape` where it passes a `const SHAPE*`.
pub trait Shape: Debug + Send + Sync {
    /// Port of `SHAPE_BASE::Type()`.
    fn type_(&self) -> ShapeType;

    /// Port of `SHAPE_BASE::TypeName()`.
    fn type_name(&self) -> &'static str {
        shape_type_as_string(self.type_())
    }

    /// Port of `SHAPE_BASE::HasIndexableSubshapes()`.
    fn has_indexable_subshapes(&self) -> bool {
        false
    }

    /// Port of `SHAPE_BASE::GetIndexableSubshapeCount()`.
    fn indexable_subshape_count(&self) -> usize {
        0
    }

    /// Port of `SHAPE_BASE::GetIndexableSubshapes()`: the C++ appends to (for compounds:
    /// replaces) a vector; here the subshapes are returned.
    fn indexable_subshapes(&self) -> Vec<&dyn Shape> {
        Vec::new()
    }

    /// A borrowed enum view of this shape (no C++ counterpart; replaces RTTI).
    fn as_shape_ref(&self) -> ShapeRef<'_>;

    /// Port of `SHAPE::Clone()`: a heap-allocated copy of the shape.
    fn clone_shape(&self) -> Box<dyn Shape>;

    /// Port of `SHAPE::IsNull()`.
    fn is_null(&self) -> bool {
        self.type_() == ShapeType::Null
    }

    /// Port of `SHAPE::Collide( const VECTOR2I& aP, int aClearance, int* aActual,
    /// VECTOR2I* aLocation )`: check if the boundary of this shape lies closer to the point
    /// `a_p` than `a_clearance`.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.collide_seg(&Seg::new(a_p, a_p), a_clearance, a_actual, a_location)
    }

    /// Port of `SHAPE::Collide( const SEG& aSeg, int aClearance, int* aActual,
    /// VECTOR2I* aLocation )`: check if the boundary of this shape lies closer to the segment
    /// `a_seg` than `a_clearance`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool;

    /// Port of `SHAPE::Collide( const SHAPE* aShape, int aClearance, int* aActual,
    /// VECTOR2I* aLocation )`, implemented in `shape_collisions.cpp`
    /// ([`shape_collisions::collide_shapes`]).
    fn collide(
        &self,
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        shape_collisions::collide_shapes(
            self.as_shape_ref(),
            a_shape.as_shape_ref(),
            a_clearance,
            a_actual,
            a_location,
            None,
        )
    }

    /// Port of `SHAPE::Collide( const SHAPE* aShape, int aClearance, VECTOR2I* aMTV )`
    /// (minimum translation vector variant), implemented in `shape_collisions.cpp`.
    fn collide_mtv(
        &self,
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_mtv: Option<&mut Vector2I>,
    ) -> bool {
        shape_collisions::collide_shapes(
            self.as_shape_ref(),
            a_shape.as_shape_ref(),
            a_clearance,
            None,
            None,
            a_mtv,
        )
    }

    /// Port of `SHAPE::GetClearance()`: the actual minimum distance between two shapes.
    fn get_clearance(&self, a_other: &dyn Shape) -> i32 {
        get_clearance(self.as_shape_ref().as_dyn(), a_other)
    }

    /// Port of `SHAPE::BBox()`: a bounding box of the shape, with a margin of `a_clearance`.
    fn bbox(&self, a_clearance: i32) -> Box2I;

    /// Port of `SHAPE::Centre()`: the centre of mass of the shape (the bounding box centre if
    /// nothing better is available).
    fn centre(&self) -> Vector2I {
        self.bbox(0).centre()
    }

    /// Port of `SHAPE::Distance()`: the minimum distance from a point to this shape.
    fn distance(&self, a_p: Vector2I) -> i32 {
        // C++: `return sqrt( SquaredDistance( aP, false ) );` (double truncated to int)
        // (INT_MIN when nothing was measured: `ECOORD_MAX`, e.g. an empty outline)
        crate::cmath::f64_to_i32_x86((self.squared_distance(a_p, false) as f64).sqrt())
    }

    /// Port of `SHAPE::SquaredDistance()`.
    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        let mut buffer = ShapePolySet::new();
        self.transform_to_polygon(&mut buffer, 0, ERROR_INSIDE);

        if buffer.outline_count() < 1 {
            return Vector2I::ECOORD_MAX;
        }

        buffer.c_outline(0).squared_distance(a_p, a_outline_only)
    }

    /// Port of `SHAPE::PointInside()`: check if point `a_pt` lies inside a closed shape.
    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        let mut buffer = ShapePolySet::new();
        self.transform_to_polygon(&mut buffer, a_accuracy, ERROR_INSIDE);

        if buffer.outline_count() < 1 {
            return false;
        }

        buffer
            .c_outline(0)
            .point_inside(a_pt, a_accuracy, a_use_bbox_cache)
    }

    /// Port of `SHAPE::TransformToPolygon()`: fill `a_buffer` with a polygon representation of
    /// this shape.
    fn transform_to_polygon(
        &self,
        a_buffer: &mut ShapePolySet,
        a_error: i32,
        a_error_loc: ErrorLoc,
    );

    /// Port of `SHAPE::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I);

    /// Port of `SHAPE::GetStart()`.
    fn get_start(&self) -> Vector2I {
        Vector2I::new(0, 0)
    }

    /// Port of `SHAPE::GetEnd()`.
    fn get_end(&self) -> Vector2I {
        Vector2I::new(0, 0)
    }

    /// Port of `SHAPE::GetWidth()`.
    fn get_width(&self) -> i32 {
        0
    }

    /// Port of `SHAPE::SetWidth()`.
    fn set_width(&mut self, _a_width: i32) {}

    /// Port of `SHAPE::Move()`.
    fn move_(&mut self, a_vector: Vector2I);

    /// Port of `SHAPE::IsSolid()`.
    fn is_solid(&self) -> bool;

    /// Port of `SHAPE::Format()`.
    fn format(&self, _a_cplusplus: bool) -> String {
        format_shape_base(self.type_())
    }
}

impl Clone for Box<dyn Shape> {
    fn clone(&self) -> Self {
        self.clone_shape()
    }
}

/// The text of the base class `SHAPE::Format()`: `shape <type number>`.
pub fn format_shape_base(a_type: ShapeType) -> String {
    format!("shape {}", a_type as i32)
}

/// Port of `SHAPE::GetClearance( const SHAPE* aOther )` with `this` as `a_this`.
pub fn get_clearance(a_this: &dyn Shape, a_other: &dyn Shape) -> i32 {
    let mut actual_clearance = i32::MAX;
    let mut a_shapes: Vec<&dyn Shape> = Vec::new();
    let mut b_shapes: Vec<&dyn Shape> = Vec::new();

    // POLY_SETs contain a bunch of polygons that are triangulated.
    // But there are way more triangles than necessary for collision detection.
    // Triangles check three vertices each but for the outline, we only need one.
    // These are also fractured, so we don't need to worry about holes
    if let ShapeRef::PolySet(poly_set) = a_this.as_shape_ref() {
        if poly_set.outline_count() > 0 {
            a_shapes.push(poly_set.c_outline(0));
        }
    } else {
        a_shapes.extend(a_this.indexable_subshapes());
    }

    if let ShapeRef::PolySet(poly_set) = a_other.as_shape_ref() {
        if poly_set.outline_count() > 0 {
            b_shapes.push(poly_set.c_outline(0));
        }
    } else {
        b_shapes.extend(a_other.indexable_subshapes());
    }

    if a_this.indexable_subshape_count() == 0 {
        a_shapes.push(a_this);
    }

    if a_other.indexable_subshape_count() == 0 {
        b_shapes.push(a_other);
    }

    for a in &a_shapes {
        for b in &b_shapes {
            let mut temp_dist = i32::MAX;
            a.collide(*b, i32::MAX / 2, Some(&mut temp_dist), None);

            if temp_dist < actual_clearance {
                actual_clearance = temp_dist;
            }
        }
    }

    actual_clearance
}

/// Port of `SHAPE_LINE_CHAIN_BASE`: the shared base of polyline-like shapes (line chain,
/// simple polygon, poly-set triangle).
///
/// The `lcb_*` default methods are the `SHAPE_LINE_CHAIN_BASE` implementations of the
/// `SHAPE` virtual methods; implementors forward the corresponding [`Shape`] methods to them
/// unless they override them in C++.
pub trait ShapeLineChainBase: Shape {
    /// Port of `SHAPE_LINE_CHAIN_BASE::GetPoint()`.
    fn get_point(&self, a_index: i32) -> Vector2I;

    /// Port of `SHAPE_LINE_CHAIN_BASE::GetSegment()`.
    fn get_segment(&self, a_index: i32) -> Seg;

    /// Port of `SHAPE_LINE_CHAIN_BASE::GetPointCount()`.
    fn get_point_count(&self) -> usize;

    /// Port of `SHAPE_LINE_CHAIN_BASE::GetSegmentCount()`.
    fn get_segment_count(&self) -> usize;

    /// Port of `SHAPE_LINE_CHAIN_BASE::IsClosed()`.
    fn is_closed(&self) -> bool;

    /// Port of `SHAPE_LINE_CHAIN_BASE::GetCachedBBox()` (a copy instead of a pointer).
    fn get_cached_bbox(&self) -> Option<Box2I> {
        None
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::Collide( const VECTOR2I& aP, ... )`: check if point
    /// `a_p` lies closer to us than `a_clearance`.
    fn lcb_collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.is_closed() && self.point_inside(a_p, a_clearance, false) {
            if let Some(l) = a_location {
                *l = a_p;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        let mut closest_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
        let clearance_sq: Ecoord = Seg::square(a_clearance);
        let mut nearest = Vector2I::default();

        for i in 0..self.get_segment_count() {
            let s = self.get_segment(i as i32);
            let pn = s.nearest_point(a_p);
            let dist_sq: Ecoord = (pn - a_p).squared_euclidean_norm();

            if dist_sq < closest_dist_sq {
                nearest = pn;
                closest_dist_sq = dist_sq;

                if closest_dist_sq == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if closest_dist_sq < clearance_sq && a_actual.is_none() {
                    break;
                }
            }
        }

        if closest_dist_sq == 0 || closest_dist_sq < clearance_sq {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = (closest_dist_sq as f64).sqrt() as i32;
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::Collide( const SEG& aSeg, ... )`: check if segment
    /// `a_seg` lies closer to us than `a_clearance`.
    fn lcb_collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.is_closed() && self.point_inside(a_seg.a, 0, false) {
            if let Some(l) = a_location {
                *l = a_seg.a;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        let mut closest_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
        let clearance_sq: Ecoord = Seg::square(a_clearance);
        let mut nearest = Vector2I::default();

        for i in 0..self.get_segment_count() {
            let s = self.get_segment(i as i32);
            let dist_sq: Ecoord = s.squared_distance_seg(a_seg);

            if dist_sq < closest_dist_sq {
                if a_location.is_some() {
                    nearest = s.nearest_point_seg(a_seg);
                }

                closest_dist_sq = dist_sq;

                if closest_dist_sq == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if closest_dist_sq < clearance_sq && a_actual.is_none() {
                    break;
                }
            }
        }

        if closest_dist_sq == 0 || closest_dist_sq < clearance_sq {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = (closest_dist_sq as f64).sqrt() as i32;
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::SquaredDistance()`.
    fn lcb_squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        let mut d: Ecoord = Vector2I::ECOORD_MAX;

        if self.is_closed() && self.point_inside(a_p, 0, false) && !a_outline_only {
            return 0;
        }

        for s in 0..self.get_segment_count() {
            d = d.min(self.get_segment(s as i32).squared_distance(a_p));
        }

        d
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::PointInside()`.
    fn lcb_point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        // Don't check the bounding box unless it's cached.  Building it is about the same speed
        // as the rigorous test below and so just slows things down by doing potentially two
        // tests.
        if a_use_bbox_cache {
            if let Some(bbox) = self.get_cached_bbox() {
                if !bbox.contains(a_pt) {
                    return false;
                }
            }
        }

        if !self.is_closed() || self.get_point_count() < 3 {
            return false;
        }

        // To check for interior points, we draw a line in the positive x direction from
        // the point.  If it intersects an even number of segments, the point is outside the
        // line chain (it had to first enter and then exit).  Otherwise, it is inside the chain.
        //
        // Note: slope might be denormal here in the case of a horizontal line but we require our
        // y to move from above to below the point (or vice versa)
        let point_count = self.get_point_count() as i32;
        let mut inside = false;
        let mut i: i32 = 0;

        while i < point_count {
            let p1 = self.get_point(i);
            i += 1;
            let p2 = self.get_point(if i == point_count { 0 } else { i });
            let diff = p2 - p1;

            if diff.y == 0 {
                continue;
            }

            let d: i32 = rescale(diff.x, a_pt.y - p1.y, diff.y);

            if ((p1.y >= a_pt.y) != (p2.y >= a_pt.y)) && (a_pt.x - p1.x < d) {
                inside = !inside;
            }
        }

        // If accuracy is <= 1 (nm) then we skip the accuracy test for performance.  Otherwise
        // we use "OnEdge(accuracy)" as a proxy for "Inside(accuracy)".
        if a_accuracy <= 1 {
            inside
        } else {
            inside || self.point_on_edge(a_pt, a_accuracy)
        }
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::PointOnEdge()`: check if point `a_pt` lies on an edge or
    /// vertex of the line chain.
    fn point_on_edge(&self, a_pt: Vector2I, a_accuracy: i32) -> bool {
        self.edge_containing_point(a_pt, a_accuracy) >= 0
    }

    /// Port of `SHAPE_LINE_CHAIN_BASE::EdgeContainingPoint()`: index of the first edge
    /// containing the point, otherwise negative.
    fn edge_containing_point(&self, a_pt: Vector2I, a_accuracy: i32) -> i32 {
        let threshold: i32 = a_accuracy + 1;
        let threshold_sq: i64 = threshold as i64 * threshold as i64;
        let point_count = self.get_point_count();

        if point_count == 0 {
            return -1;
        } else if point_count == 1 {
            let dist_sq: Ecoord = self.get_point(0).squared_distance(a_pt);
            return if dist_sq <= threshold_sq { 0 } else { -1 };
        }

        let seg_count = self.get_segment_count();

        for i in 0..seg_count {
            let s = self.get_segment(i as i32);

            if s.a == a_pt || s.b == a_pt {
                return i as i32;
            }

            if s.squared_distance(a_pt) <= threshold_sq {
                return i as i32;
            }
        }

        -1
    }
}
