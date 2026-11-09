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

//! Port of `libs/kimath/include/geometry/shape_null.h`.

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::Seg;
use crate::geometry::shape::{Shape, ShapeRef, ShapeType};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of `SHAPE_NULL`: an empty shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeNull;

impl ShapeNull {
    /// Port of `SHAPE_NULL()`.
    pub const fn new() -> Self {
        ShapeNull
    }
}

impl Shape for ShapeNull {
    fn type_(&self) -> ShapeType {
        ShapeType::Null
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Null(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    /// Port of `SHAPE_NULL::BBox()`: an empty, uninitialised box.
    fn bbox(&self, _a_clearance: i32) -> Box2I {
        Box2I::default()
    }

    fn collide_seg(
        &self,
        _a_seg: &Seg,
        _a_clearance: i32,
        _a_actual: Option<&mut i32>,
        _a_location: Option<&mut Vector2I>,
    ) -> bool {
        false
    }

    fn transform_to_polygon(
        &self,
        _a_buffer: &mut ShapePolySet,
        _a_error: i32,
        _a_error_loc: ErrorLoc,
    ) {
    }

    fn rotate(&mut self, _a_angle: EdaAngle, _a_center: Vector2I) {}

    fn move_(&mut self, _a_vector: Vector2I) {}

    fn is_solid(&self) -> bool {
        false
    }
}
