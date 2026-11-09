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

//! Port of `libs/kimath/include/geometry/shape_circle.h` (and `SHAPE_CIRCLE::Format()` /
//! `SHAPE_CIRCLE::TransformToPolygon()` from `libs/kimath/src/geometry/shape_segment.cpp`).

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::circle::Circle;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeRef, ShapeType, format_shape_base};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;
use crate::trigo::rotate_point_around;

/// Port of `SHAPE_CIRCLE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeCircle {
    circle: Circle,
}

impl ShapeCircle {
    /// Port of `SHAPE_CIRCLE( const VECTOR2I& aCenter, int aRadius )`.
    pub const fn new(a_center: Vector2I, a_radius: i32) -> Self {
        ShapeCircle {
            circle: Circle::new(a_center, a_radius),
        }
    }

    /// Port of `SHAPE_CIRCLE( const CIRCLE& aCircle )`.
    pub const fn from_circle(a_circle: Circle) -> Self {
        ShapeCircle { circle: a_circle }
    }

    /// Port of `SetRadius()`.
    pub fn set_radius(&mut self, a_radius: i32) {
        self.circle.radius = a_radius;
    }

    /// Port of `SetCenter()`.
    pub fn set_center(&mut self, a_center: Vector2I) {
        self.circle.center = a_center;
    }

    /// Port of `GetRadius()`.
    pub fn get_radius(&self) -> i32 {
        self.circle.radius
    }

    /// Port of `GetCenter()`.
    pub fn get_center(&self) -> Vector2I {
        self.circle.center
    }

    /// Port of `GetCircle()`.
    pub fn get_circle(&self) -> Circle {
        self.circle
    }
}

impl Shape for ShapeCircle {
    fn type_(&self) -> ShapeType {
        ShapeType::Circle
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Circle(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    /// Port of `SHAPE_CIRCLE::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let rc = Vector2I::new(self.circle.radius + a_clearance, self.circle.radius + a_clearance);

        Box2I::new(self.circle.center - rc, rc * 2)
    }

    /// Port of `SHAPE_CIRCLE::Collide( const SEG&, ... )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        let min_dist: i32 = a_clearance + self.circle.radius;
        let pn = a_seg.nearest_point(self.circle.center);
        let dist_sq: Ecoord = (pn - self.circle.center).squared_euclidean_norm();

        if dist_sq == 0 || dist_sq < Seg::square(min_dist) {
            if let Some(l) = a_location {
                let pts = self.circle.intersect_seg(a_seg);

                if !pts.is_empty() && dist_sq == 0 {
                    *l = self.circle.intersect_seg(a_seg)[0];
                } else {
                    *l = pn;
                }
            }

            if let Some(a) = a_actual {
                *a = 0.max((dist_sq as f64).sqrt() as i32 - self.circle.radius);
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_CIRCLE::TransformToPolygon()`.
    ///
    /// The C++ calls `TransformCircleToPolygon()` (`convert_basic_shapes_to_polygon.cpp`), which
    /// is not ported yet: the buffer is left unchanged (see `PORT_STATUS.md`).
    fn transform_to_polygon(&self, _a_buffer: &mut ShapePolySet, _a_error: i32, _a_error_loc: ErrorLoc) {}

    /// Port of `SHAPE_CIRCLE::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        rotate_point_around(&mut self.circle.center, a_center, a_angle);
    }

    /// Port of `SHAPE_CIRCLE::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.circle.center += a_vector;
    }

    fn is_solid(&self) -> bool {
        true
    }

    /// Port of `SHAPE_CIRCLE::Format()`.
    fn format(&self, a_cplusplus: bool) -> String {
        if a_cplusplus {
            format!(
                "SHAPE_CIRCLE( VECTOR2I( {}, {}), {}); ",
                self.circle.center.x, self.circle.center.y, self.circle.radius
            )
        } else {
            format!(
                "{} {} {} {}",
                format_shape_base(self.type_()),
                self.circle.center.x,
                self.circle.center.y,
                self.circle.radius
            )
        }
    }
}
