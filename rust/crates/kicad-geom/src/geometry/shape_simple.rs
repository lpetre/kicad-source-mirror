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

//! Port of `libs/kimath/include/geometry/shape_simple.h` (and
//! `SHAPE_SIMPLE::TransformToPolygon()` from `libs/kimath/src/geometry/shape.cpp`).

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::{Vector2D, Vector2I};

/// Port of `SHAPE_SIMPLE`: a simple polygon consisting of a zero-thickness closed chain of
/// connected line segments.
///
/// Internally the vertices are held in a SHAPE_LINE_CHAIN, please note that
/// there is a "virtual" line segment between the last and first vertex.
#[derive(Clone, Debug)]
pub struct ShapeSimple {
    /// vertices
    points: ShapeLineChain,
}

impl Default for ShapeSimple {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeSimple {
    /// Port of `SHAPE_SIMPLE()`: an empty polygon.
    pub fn new() -> Self {
        let mut points = ShapeLineChain::new();
        points.set_closed(true);
        ShapeSimple { points }
    }

    /// Port of `SHAPE_SIMPLE( const SHAPE_LINE_CHAIN& aPoly )`.
    pub fn from_line_chain(a_poly: &ShapeLineChain) -> Self {
        let mut points = a_poly.clone();
        points.set_closed(true);
        ShapeSimple { points }
    }

    /// Port of `Clear()`: remove all points from the polygon.
    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// Port of `PointCount()`: the number of points (vertices) in this polygon.
    pub fn point_count(&self) -> i32 {
        self.points.point_count()
    }

    /// Port of `CPoint()`: a given point of the polygon.  Negative indices count from the end of
    /// the point list, e.g. -1 means "last point", -2 means "second to last point" and so on.
    pub fn c_point(&self, a_index: i32) -> Vector2I {
        self.points.c_point(a_index)
    }

    /// Port of `CDPoint()`: a given point as a vector with elements of type double.
    pub fn cd_point(&self, a_index: i32) -> Vector2D {
        let v = self.c_point(a_index);
        Vector2D::new(v.x as f64, v.y as f64)
    }

    /// Port of `Vertices()`: the list of vertices defining this simple polygon.
    pub fn vertices(&self) -> &ShapeLineChain {
        &self.points
    }

    /// Port of `Append( int aX, int aY )`: append a new point at the end of the polygon.
    pub fn append_xy(&mut self, a_x: i32, a_y: i32) {
        let v = Vector2I::new(a_x, a_y);
        self.append(v);
    }

    /// Port of `Append( const VECTOR2I& aP )`: append a new point at the end of the polygon.
    pub fn append(&mut self, a_p: Vector2I) {
        self.points.append_point(a_p, false);
    }
}

impl Shape for ShapeSimple {
    fn type_(&self) -> ShapeType {
        ShapeType::Simple
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Simple(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    /// Port of `SHAPE_SIMPLE::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        self.points.bbox(a_clearance)
    }

    /// `SHAPE_LINE_CHAIN_BASE::Collide( const VECTOR2I&, ... )`.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.lcb_collide_point(a_p, a_clearance, a_actual, a_location)
    }

    /// Port of `SHAPE_SIMPLE::Collide( const SEG&, ... )`: the collision of the vertex chain.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.points.collide_seg(a_seg, a_clearance, a_actual, a_location)
    }

    /// `SHAPE_LINE_CHAIN_BASE::SquaredDistance()`.
    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        self.lcb_squared_distance(a_p, a_outline_only)
    }

    /// `SHAPE_LINE_CHAIN_BASE::PointInside()`.
    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        self.lcb_point_inside(a_pt, a_accuracy, a_use_bbox_cache)
    }

    /// Port of `SHAPE_SIMPLE::TransformToPolygon()`.
    fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, _a_error: i32, _a_error_loc: ErrorLoc) {
        a_buffer.add_outline(&self.points);
    }

    /// Port of `SHAPE_SIMPLE::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        self.points.rotate(a_angle, a_center);
    }

    /// Port of `SHAPE_SIMPLE::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.points.move_(a_vector);
    }

    fn is_solid(&self) -> bool {
        true
    }
}

impl ShapeLineChainBase for ShapeSimple {
    fn get_point(&self, a_index: i32) -> Vector2I {
        self.points.c_point(a_index)
    }

    fn get_segment(&self, a_index: i32) -> Seg {
        self.points.c_segment(a_index)
    }

    fn get_point_count(&self) -> usize {
        self.points.point_count() as usize
    }

    fn get_segment_count(&self) -> usize {
        self.points.segment_count() as usize
    }

    fn is_closed(&self) -> bool {
        true
    }
}
