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

//! Port of `libs/kimath/include/geometry/shape_segment.h` and
//! `libs/kimath/src/geometry/shape_segment.cpp` (the `SHAPE_CIRCLE` methods of that file are in
//! [`crate::geometry::shape_circle`]).

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::{DEGREES_T, EdaAngle};
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeRef, ShapeType, format_shape_base};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;
use crate::trigo::{rotate_point, rotate_point_around};

/// Port of `SHAPE_SEGMENT`: a segment with a width (an oval / track).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeSegment {
    seg: Seg,
    width: i32,
}

impl ShapeSegment {
    /// Port of `SHAPE_SEGMENT( const VECTOR2I& aA, const VECTOR2I& aB, int aWidth )`.
    pub fn new(a_a: Vector2I, a_b: Vector2I, a_width: i32) -> Self {
        ShapeSegment {
            seg: Seg::new(a_a, a_b),
            width: a_width,
        }
    }

    /// Port of `SHAPE_SEGMENT( const SEG& aSeg, int aWidth )`.
    pub fn from_seg(a_seg: Seg, a_width: i32) -> Self {
        ShapeSegment {
            seg: a_seg,
            width: a_width,
        }
    }

    /// Port of `SHAPE_SEGMENT::BySizeAndCenter()`.
    pub fn by_size_and_center(
        a_overall_size: Vector2I,
        a_center: Vector2I,
        a_rotation: EdaAngle,
    ) -> ShapeSegment {
        let mut seg_vec = Vector2I::new(0, 0);
        let width: i32;

        // Find the major axis, without endcaps
        if a_overall_size.x > a_overall_size.y {
            width = a_overall_size.y;
            seg_vec.x = a_overall_size.x - width;
        } else {
            width = a_overall_size.x;
            seg_vec.y = a_overall_size.y - width;
        }

        rotate_point(&mut seg_vec, a_rotation);

        ShapeSegment::new(a_center - seg_vec / 2, a_center + seg_vec / 2, width)
    }

    /// Port of `SetSeg()`.
    pub fn set_seg(&mut self, a_seg: Seg) {
        self.seg = a_seg;
    }

    /// Port of `GetSeg()`.
    pub fn get_seg(&self) -> &Seg {
        &self.seg
    }

    /// Port of `GetTotalLength()`: the total length of the segment, from tip to tip.
    pub fn get_total_length(&self) -> i32 {
        self.seg.length() + self.width
    }

    /// Port of `GetCenter()`.
    pub fn get_center(&self) -> Vector2I {
        self.seg.center()
    }

    /// Port of `GetAngle()`.
    pub fn get_angle(&self) -> EdaAngle {
        EdaAngle::from_vector(self.seg.b - self.seg.a)
    }

    /// Port of `SHAPE_SEGMENT::Is45Degree()` with the default tolerance of 1 degree.
    pub fn is_45_degree(&self) -> bool {
        self.is_45_degree_with_tolerance(EdaAngle::new(1.0, DEGREES_T))
    }

    /// Port of `SHAPE_SEGMENT::Is45Degree( EDA_ANGLE aTollerance )`.
    pub fn is_45_degree_with_tolerance(&self, a_tollerance: EdaAngle) -> bool {
        let mag = EdaAngle::from_vector(self.seg.a - self.seg.b).normalize180();

        let f: f64 = mag.as_degrees() % 45.0;
        let d: f64 = a_tollerance.as_degrees();

        f >= 45.0 - d || f <= d
    }
}

impl Shape for ShapeSegment {
    fn type_(&self) -> ShapeType {
        ShapeType::Segment
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Segment(self)
    }

    /// Port of `SHAPE_SEGMENT::Clone()` (copies the segment and the width).
    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(ShapeSegment::from_seg(self.seg, self.width))
    }

    /// Port of `SHAPE_SEGMENT::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        *Box2I::new(self.seg.a, self.seg.b - self.seg.a).inflate(a_clearance + (self.width + 1) / 2)
    }

    /// Port of `SHAPE_SEGMENT::Collide( const SEG&, ... )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if a_seg.a == a_seg.b {
            return self.collide_point(a_seg.a, a_clearance, a_actual, a_location);
        }

        let min_dist: i32 = (self.width + 1) / 2 + a_clearance;
        let dist_sq: Ecoord = self.seg.squared_distance_seg(a_seg);

        if dist_sq == 0 || dist_sq < Seg::square(min_dist) {
            if let Some(l) = a_location {
                *l = self.seg.nearest_point_seg(a_seg);
            }

            if let Some(a) = a_actual {
                *a = 0.max((dist_sq as f64).sqrt() as i32 - (self.width + 1) / 2);
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_SEGMENT::Collide( const VECTOR2I&, ... )`.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        let min_dist: i32 = (self.width + 1) / 2 + a_clearance;
        let dist_sq: Ecoord = self.seg.squared_distance(a_p);

        if dist_sq == 0 || dist_sq < Seg::square(min_dist) {
            if let Some(l) = a_location {
                *l = self.seg.nearest_point(a_p);
            }

            if let Some(a) = a_actual {
                *a = 0.max((dist_sq as f64).sqrt() as i32 - (self.width + 1) / 2);
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_SEGMENT::TransformToPolygon()`: `TransformOvalToPolygon()`.
    fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, a_error: i32, a_error_loc: ErrorLoc) {
        crate::convert_basic_shapes_to_polygon::transform_oval_to_polygon(
            a_buffer,
            self.seg.a,
            self.seg.b,
            self.width,
            a_error,
            a_error_loc,
            0,
        );
    }

    /// Port of `SHAPE_SEGMENT::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        rotate_point_around(&mut self.seg.a, a_center, a_angle);
        rotate_point_around(&mut self.seg.b, a_center, a_angle);
    }

    fn get_start(&self) -> Vector2I {
        self.seg.a
    }

    fn get_end(&self) -> Vector2I {
        self.seg.b
    }

    fn get_width(&self) -> i32 {
        self.width
    }

    fn set_width(&mut self, a_width: i32) {
        self.width = a_width;
    }

    /// Port of `SHAPE_SEGMENT::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.seg.a += a_vector;
        self.seg.b += a_vector;
    }

    fn is_solid(&self) -> bool {
        true
    }

    /// Port of `SHAPE_SEGMENT::Format()`.
    fn format(&self, a_cplusplus: bool) -> String {
        if a_cplusplus {
            format!(
                "SHAPE_SEGMENT( VECTOR2I( {}, {}), VECTOR2I( {}, {}), {}); ",
                self.seg.a.x, self.seg.a.y, self.seg.b.x, self.seg.b.y, self.width
            )
        } else {
            format!(
                "{} {} {} {} {} {}",
                format_shape_base(self.type_()),
                self.seg.a.x,
                self.seg.a.y,
                self.seg.b.x,
                self.seg.b.y,
                self.width
            )
        }
    }
}
