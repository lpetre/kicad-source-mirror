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

//! Port of `libs/kimath/include/geometry/shape_rect.h` and
//! `libs/kimath/src/geometry/shape_rect.cpp`.

use crate::geometry::approximation::{ERROR_INSIDE, ErrorLoc};
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeRef, ShapeType};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;
use crate::trigo::rotate_point_around;

/// Port of `SHAPE_RECT`: an axis-aligned rectangle, optionally with rounded corners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeRect {
    /// Top-left corner
    p0: Vector2I,
    /// Width
    w: i32,
    /// Height
    h: i32,
    /// Corner radius
    radius: i32,
}

impl ShapeRect {
    /// Port of `SHAPE_RECT()`: an empty (0-sized) rectangle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `SHAPE_RECT( const BOX2I& aBox )`.
    pub fn from_box(a_box: &Box2I) -> Self {
        ShapeRect {
            p0: a_box.position(),
            // BOX2I::GetWidth() / GetHeight() return the 64 bit size, narrowed to int
            w: a_box.width() as i32,
            h: a_box.height() as i32,
            radius: 0,
        }
    }

    /// Port of `SHAPE_RECT( int aX0, int aY0, int aW, int aH )`.
    pub fn from_xywh(a_x0: i32, a_y0: i32, a_w: i32, a_h: i32) -> Self {
        ShapeRect {
            p0: Vector2I::new(a_x0, a_y0),
            w: a_w,
            h: a_h,
            radius: 0,
        }
    }

    /// Port of `SHAPE_RECT( const VECTOR2I& aP0, int aW, int aH )`.
    pub fn from_pos_size(a_p0: Vector2I, a_w: i32, a_h: i32) -> Self {
        ShapeRect {
            p0: a_p0,
            w: a_w,
            h: a_h,
            radius: 0,
        }
    }

    /// Port of `SHAPE_RECT( const VECTOR2I& aP0, const VECTOR2I& aP1 )`: create by two corners.
    pub fn from_corners(a_p0: Vector2I, a_p1: Vector2I) -> Self {
        ShapeRect {
            p0: a_p0,
            w: a_p1.x - a_p0.x,
            h: a_p1.y - a_p0.y,
            radius: 0,
        }
    }

    /// Port of `GetInflated()`: a rectangle that is larger by `a_offset` in all directions,
    /// but still centered on the original rectangle.
    pub fn get_inflated(&self, a_offset: i32) -> ShapeRect {
        let mut r = ShapeRect::from_pos_size(
            self.p0 - Vector2I::new(a_offset, a_offset),
            self.w + 2 * a_offset,
            self.h + 2 * a_offset,
        );
        r.set_radius(self.radius + a_offset);
        r
    }

    /// Port of `Diagonal()`: length of the diagonal of the rectangle.
    pub fn diagonal(&self) -> i32 {
        Vector2I::new(self.w, self.h).euclidean_norm()
    }

    /// Port of `MajorDimension()`.
    pub fn major_dimension(&self) -> i32 {
        self.w.max(self.h)
    }

    /// Port of `MinorDimension()`.
    pub fn minor_dimension(&self) -> i32 {
        self.w.min(self.h)
    }

    /// Port of `GetPosition()`: the top left corner of the rectangle.
    pub fn get_position(&self) -> Vector2I {
        self.p0
    }

    /// Port of `GetSize()`.
    pub fn get_size(&self) -> Vector2I {
        Vector2I::new(self.w, self.h)
    }

    /// Port of `GetHeight()`.
    pub fn get_height(&self) -> i32 {
        self.h
    }

    /// Port of `GetRadius()`: the corner radius of the rectangle.
    pub fn get_radius(&self) -> i32 {
        self.radius
    }

    /// Port of `SetRadius()`.
    pub fn set_radius(&mut self, a_radius: i32) {
        self.radius = a_radius;
    }

    /// Port of `SHAPE_RECT::Outline()`.
    ///
    /// The C++ comment: "TODO: we're DEPENDING on clients of this routine to use the actual arcs
    /// (if any) inserted into the SHAPE_LINE_CHAIN.  They must NOT use the approximated segments
    /// because we don't know what IUScale to generate them in."
    ///
    /// Where the C++ reads `Outline( 0 )` of an empty set (undefined behaviour: a degenerate
    /// rounded rectangle) the result is an empty chain.
    pub fn outline(&self) -> ShapeLineChain {
        let mut buffer = ShapePolySet::new();
        self.transform_to_polygon(
            &mut buffer,
            ShapeArc::default_accuracy_for_pcb(),
            ERROR_INSIDE,
        );

        if buffer.outline_count() == 0 {
            return ShapeLineChain::new();
        }

        std::mem::take(buffer.outline(0))
    }

    /// Port of `SHAPE_RECT::Normalize()`: ensure that the height and width are positive.
    pub fn normalize(&mut self) {
        // Ensure that the height and width are positive.
        if self.w < 0 {
            self.w = -self.w;
            self.p0.x -= self.w;
        }

        if self.h < 0 {
            self.h = -self.h;
            self.p0.y -= self.h;
        }
    }
}

impl Shape for ShapeRect {
    fn type_(&self) -> ShapeType {
        ShapeType::Rect
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Rect(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    /// Port of `SHAPE_RECT::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        Box2I::new(
            Vector2I::new(self.p0.x - a_clearance, self.p0.y - a_clearance),
            Vector2I::new(self.w + 2 * a_clearance, self.h + 2 * a_clearance),
        )
    }

    /// Port of `SHAPE_RECT::Collide( const SEG&, ... )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.radius > 0 {
            let line_chain = self.outline();
            return line_chain.collide_seg(a_seg, a_clearance, a_actual, a_location);
        }

        let bbox = self.bbox(0);

        if bbox.contains(a_seg.a) {
            if let Some(l) = a_location {
                *l = a_seg.a;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        if bbox.contains(a_seg.b) {
            if let Some(l) = a_location {
                *l = a_seg.b;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        let corners: [Vector2I; 5] = [
            Vector2I::new(self.p0.x, self.p0.y),
            Vector2I::new(self.p0.x, self.p0.y + self.h),
            Vector2I::new(self.p0.x + self.w, self.p0.y + self.h),
            Vector2I::new(self.p0.x + self.w, self.p0.y),
            Vector2I::new(self.p0.x, self.p0.y),
        ];

        let mut closest_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
        let mut nearest = Vector2I::default();

        for i in 0..4 {
            let side = Seg::new(corners[i], corners[i + 1]);
            let dist_sq: Ecoord = side.squared_distance_seg(a_seg);

            if dist_sq < closest_dist_sq {
                if a_location.is_some() {
                    nearest = side.nearest_point_seg(a_seg);
                }

                closest_dist_sq = dist_sq;
            } else if a_location.is_some() && dist_sq == closest_dist_sq {
                let near = side.nearest_point_seg(a_seg);

                if (near - a_seg.a).squared_euclidean_norm()
                    < (nearest - a_seg.a).squared_euclidean_norm()
                {
                    nearest = near;
                }
            }
        }

        if closest_dist_sq == 0 || closest_dist_sq < Seg::square(a_clearance) {
            if let Some(a) = a_actual {
                *a = (closest_dist_sq as f64).sqrt() as i32;
            }

            if let Some(l) = a_location {
                *l = nearest;
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_RECT::TransformToPolygon()`.  A rectangle with rounded corners goes
    /// through `ROUNDRECT` (which replaces the buffer).
    fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, a_error: i32, _a_error_loc: ErrorLoc) {
        if self.radius > 0 {
            let rr = crate::geometry::roundrect::Roundrect::new(self.clone(), self.radius, false);
            rr.transform_to_polygon(a_buffer, a_error);
            return;
        }

        let idx = a_buffer.new_outline();
        let outline = a_buffer.outline(idx);

        outline.append_point(self.p0, false);
        outline.append_point(Vector2I::new(self.p0.x + self.w, self.p0.y), false);
        outline.append_point(Vector2I::new(self.p0.x + self.w, self.p0.y + self.h), false);
        outline.append_point(Vector2I::new(self.p0.x, self.p0.y + self.h), false);
        outline.set_closed(true);
    }

    /// Port of `SHAPE_RECT::Rotate()`.
    ///
    /// This function has limited utility for SHAPE_RECT as non-cartesian rotations will distort
    /// the rectangle.  If you might need to handle non-90 degree rotations then the SHAPE_RECT
    /// should first be converted to a SHAPE_SIMPLE which can then be free-rotated.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        let mut c1 = self.p0;
        let mut c2 = self.p0 + Vector2I::new(self.w, self.h);
        rotate_point_around(&mut c1, a_center, a_angle);
        rotate_point_around(&mut c2, a_center, a_angle);
        self.p0 = Vector2I::new(c1.x.min(c2.x), c1.y.min(c2.y));
        self.w = (c2.x - c1.x).abs();
        self.h = (c2.y - c1.y).abs();
    }

    /// Port of `SHAPE_RECT::GetWidth()`: the width of the rectangle.
    fn get_width(&self) -> i32 {
        self.w
    }

    /// Port of `SHAPE_RECT::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.p0 += a_vector;
    }

    fn is_solid(&self) -> bool {
        true
    }

    /// Port of `SHAPE_RECT::Format()`.
    fn format(&self, _a_cplusplus: bool) -> String {
        format!(
            "SHAPE_RECT( {}, {}, {}, {}, {});",
            self.p0.x, self.p0.y, self.w, self.h, self.radius
        )
    }
}
