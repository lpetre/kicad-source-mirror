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

//! Port of `libs/kimath/include/geometry/shape_compound.h` and
//! `libs/kimath/src/geometry/shape_compound.cpp`.

use std::sync::Arc;

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::Seg;
use crate::geometry::shape::{Shape, ShapeRef, ShapeType};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of `SHAPE_COMPOUND`: a compound shape, consisting of multiple simple shapes.
///
/// The subshapes are owned (`std::vector<SHAPE*>` deleted in the C++ destructor).
#[derive(Clone, Debug)]
pub struct ShapeCompound {
    /// `m_cachedBBox` (never read, in C++ either)
    #[allow(dead_code)]
    cached_bbox: Box2I,
    /// `m_dirty` (never read, in C++ either)
    #[allow(dead_code)]
    dirty: bool,
    shapes: Vec<Box<dyn Shape>>,
}

impl Default for ShapeCompound {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeCompound {
    /// Port of `SHAPE_COMPOUND()`.
    pub fn new() -> Self {
        ShapeCompound {
            cached_bbox: Box2I::default(),
            dirty: true,
            shapes: Vec::new(),
        }
    }

    /// Port of `SHAPE_COMPOUND( const std::vector<SHAPE*>& aShapes )`: takes ownership of the
    /// shapes (nested compounds are kept as they are, as in C++).
    pub fn from_shapes(a_shapes: Vec<Box<dyn Shape>>) -> Self {
        ShapeCompound {
            cached_bbox: Box2I::default(),
            dirty: true,
            shapes: a_shapes,
        }
    }

    /// Port of `Shapes()`.
    pub fn shapes(&self) -> &[Box<dyn Shape>] {
        &self.shapes
    }

    /// Port of `AddShape( SHAPE* aShape )`: takes ownership of the shape.
    ///
    /// Don't make clients deal with nested SHAPE_COMPOUNDs: the subshapes of a compound are
    /// added instead of the compound itself.
    pub fn add_shape(&mut self, a_shape: Box<dyn Shape>) {
        if let ShapeRef::Compound(compound) = a_shape.as_shape_ref() {
            for subshape in compound.indexable_subshapes() {
                self.shapes.push(subshape.clone_shape());
            }
        } else {
            self.shapes.push(a_shape);
        }

        self.dirty = true;
    }

    /// Port of `AddShape( std::shared_ptr<SHAPE> aShape )`: adds a copy of the shape.
    pub fn add_shape_shared(&mut self, a_shape: &Arc<dyn Shape>) {
        // Don't make clients deal with nested SHAPE_COMPOUNDs
        if let ShapeRef::Compound(compound) = a_shape.as_shape_ref() {
            for subshape in compound.indexable_subshapes() {
                self.shapes.push(subshape.clone_shape());
            }
        } else {
            self.shapes.push(a_shape.clone_shape());
        }

        self.dirty = true;
    }

    /// Port of `Empty()`.
    pub fn empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Port of `Size()`.
    pub fn size(&self) -> i32 {
        self.shapes.len() as i32
    }

    /// Port of `UniqueSubshape()`: the only subshape, if there is exactly one.
    pub fn unique_subshape(&self) -> Option<&dyn Shape> {
        if self.shapes.len() != 1 {
            None
        } else {
            Some(self.shapes[0].as_ref())
        }
    }
}

impl Shape for ShapeCompound {
    fn type_(&self) -> ShapeType {
        ShapeType::Compound
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Compound(self)
    }

    /// Port of `SHAPE_COMPOUND::Clone()` (a deep copy).
    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(ShapeCompound::from_shapes(self.shapes.iter().map(|s| s.clone_shape()).collect()))
    }

    fn has_indexable_subshapes(&self) -> bool {
        true
    }

    fn indexable_subshape_count(&self) -> usize {
        self.shapes.len()
    }

    fn indexable_subshapes(&self) -> Vec<&dyn Shape> {
        self.shapes.iter().map(|s| s.as_ref()).collect()
    }

    /// Port of `SHAPE_COMPOUND::BBox()`.  Note that, as in C++, `a_clearance` is ignored.
    fn bbox(&self, _a_clearance: i32) -> Box2I {
        let mut bb = Box2I::default();

        if self.shapes.is_empty() {
            return bb;
        }

        bb = self.shapes[0].bbox(0);

        for shape in &self.shapes[1..] {
            bb.merge(&shape.bbox(0));
        }

        bb
    }

    /// Port of `SHAPE_COMPOUND::Collide( const SEG&, ... )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        let mut closest_dist = i32::MAX;
        let mut nearest = Vector2I::default();
        let want_actual = a_actual.is_some();
        let want_location = a_location.is_some();

        for item in &self.shapes {
            let mut actual: i32 = 0;
            let mut pn = Vector2I::default();

            if item.collide_seg(
                a_seg,
                a_clearance,
                if want_actual || want_location { Some(&mut actual) } else { None },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if actual < closest_dist {
                    nearest = pn;
                    closest_dist = actual;

                    if !want_location && !want_actual {
                        break;
                    }
                } else if want_location
                    && actual == closest_dist
                    && (pn - a_seg.a).squared_euclidean_norm() < (nearest - a_seg.a).squared_euclidean_norm()
                {
                    nearest = pn;
                }
            }
        }

        if closest_dist == 0 || closest_dist < a_clearance {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = closest_dist;
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_COMPOUND::TransformToPolygon()`.
    fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, a_error: i32, a_error_loc: ErrorLoc) {
        for item in &self.shapes {
            item.transform_to_polygon(a_buffer, a_error, a_error_loc);
        }
    }

    /// Port of `SHAPE_COMPOUND::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        for item in &mut self.shapes {
            item.rotate(a_angle, a_center);
        }
    }

    /// Port of `SHAPE_COMPOUND::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        for item in &mut self.shapes {
            item.move_(a_vector);
        }
    }

    /// Port of `SHAPE_COMPOUND::IsSolid()`.
    fn is_solid(&self) -> bool {
        true
    }

    /// Port of `SHAPE_COMPOUND::Format()` (the subshapes are formatted with their default
    /// `Format()` argument, `true`).
    fn format(&self, _a_cplusplus: bool) -> String {
        let mut ss = String::from("compound( ");

        for shape in &self.shapes {
            ss += &shape.format(true);
            ss += " ";
        }

        ss
    }
}
