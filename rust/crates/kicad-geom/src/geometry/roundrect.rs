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

//! Port of `libs/kimath/include/geometry/roundrect.h` and
//! `libs/kimath/src/geometry/roundrect.cpp` (`ROUNDRECT`).

use crate::geometry::direction45::Directions;
use crate::geometry::eda_angle::ANGLE_360;
use crate::geometry::shape::Shape;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_utils::{get_point, make_arc_cw90, make_arc_cw180};
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of the file-local `MakeCornerArcCw90()`.
fn make_corner_arc_cw90(a_rect: &ShapeRect, a_radius: i32, a_dir: Directions) -> ShapeArc {
    let center = get_point(a_rect, a_dir, 0);
    make_arc_cw90(center, a_radius, a_dir)
}

/// Port of the file-local `MakeSideArcCw180()`.
fn make_side_arc_cw180(a_rect: &ShapeRect, a_radius: i32, a_dir: Directions) -> ShapeArc {
    let center = get_point(a_rect, a_dir, 0);
    make_arc_cw180(center, a_radius, a_dir)
}

/// Port of `ROUNDRECT`: a round rectangle shape, based on a rectangle and a radius.
///
/// The C++ comment: "For now, not an inheritor of SHAPE as that means implementing a lot of
/// Collision logic."
#[derive(Clone, Debug, Default)]
pub struct Roundrect {
    /// `m_rect`
    rect: ShapeRect,
    /// `m_radius`
    radius: i32,
}

impl Roundrect {
    /// Port of `ROUNDRECT( SHAPE_RECT aRect, int aRadius, bool aNormalizeOnCreate )`: the radius
    /// is clamped to `[0, |minor dimension| / 2]`; with `a_normalize_on_create` the rectangle is
    /// normalised first (positive width and height, which can move its origin).
    pub fn new(a_rect: ShapeRect, a_radius: i32, a_normalize_on_create: bool) -> Self {
        let mut rr = Roundrect { rect: a_rect, radius: a_radius };

        if a_normalize_on_create {
            rr.rect.normalize();
        }

        // Ensure radius is compatible with rectangle size:
        let min_radius = rr.rect.minor_dimension().wrapping_abs() / 2;

        if rr.radius > min_radius {
            rr.radius = min_radius;
        }

        if rr.radius < 0 {
            rr.radius = 0;
        }

        rr
    }

    /// Port of `ROUNDRECT::OutsetFrom()`: the rectangle inflated by `a_outset`, with that radius.
    pub fn outset_from(a_rect: &ShapeRect, a_outset: i32) -> Roundrect {
        Roundrect::new(a_rect.get_inflated(a_outset), a_outset, false)
    }

    /// Port of `GetRoundRadius()`.
    pub fn get_round_radius(&self) -> i32 {
        self.radius
    }

    /// Port of `GetRect()`: the basis rectangle (without the rounded corners).
    pub fn get_rect(&self) -> &ShapeRect {
        &self.rect
    }

    /// Port of `GetWidth()`.
    pub fn get_width(&self) -> i32 {
        self.rect.get_width()
    }

    /// Port of `GetHeight()`.
    pub fn get_height(&self) -> i32 {
        self.rect.get_height()
    }

    /// Port of `GetPosition()`.
    pub fn get_position(&self) -> Vector2I {
        self.rect.get_position()
    }

    /// Port of `BBox()`: always the basis rectangle's bounding box.
    pub fn bbox(&self) -> Box2I {
        self.rect.bbox(0)
    }

    /// Port of `GetInflated()`: the roundrect grown by `a_outset` in all directions (the radius
    /// grows by `a_outset` as well).
    pub fn get_inflated(&self, a_outset: i32) -> Roundrect {
        Roundrect::new(self.rect.get_inflated(a_outset), self.radius + a_outset, false)
    }

    /// Port of `ROUNDRECT::TransformToPolygon()`: replace `a_buffer` by the polygonal
    /// representation of the roundrect.  Degenerate roundrects (an edge shorter than the
    /// diameter, zero size) leave the buffer unchanged.
    ///
    /// Zero radius emits corners in order TL TR BR BL and constraint VERTEX bindings rely on
    /// that order.
    pub fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, a_max_error: i32) {
        // Roundrects won't have a gazillion points, so we use a higher definition than the
        // typical maxError.
        let max_error = a_max_error / 5;

        let mut tmp = ShapePolySet::new();
        let idx = tmp.new_outline();

        let w = self.rect.get_width();
        let h = self.rect.get_height();

        // Handle non normalized rect (i.e. w or h < 0 )
        if w < 0 || h < 0 {
            let norm_rr = Roundrect::new(self.rect.clone(), self.radius, true); // build a normalized ROUNDRECT (w,h >= 0)
            norm_rr.transform_to_polygon(a_buffer, a_max_error);
            return;
        }

        // This code works fine only with normalized rect (i.e. w or h >= 0 )
        let x_edge = self.rect.get_width() - 2 * self.radius;
        let y_edge = self.rect.get_height() - 2 * self.radius;

        // Handle degenerate cases where dimensions are invalid
        // This can happen with negative inflate values or zero-size rectangles
        if x_edge < 0 || y_edge < 0 || self.radius < 0 || w <= 0 || h <= 0 {
            return;
        }

        let m_p0 = self.rect.get_position();
        let r = self.radius;

        {
            let outline = tmp.outline(idx);

            if r == 0 {
                // Just a rectangle with order TL TR BR BL load bearing for constraint VERTEX bindings
                outline.append_point(m_p0, false);
                outline.append_point(m_p0 + Vector2I::new(w, 0), false);
                outline.append_point(m_p0 + Vector2I::new(w, h), false);
                outline.append_point(m_p0 + Vector2I::new(0, h), false);
            } else if x_edge == 0 && y_edge == 0 {
                // It's a circle
                outline.append_arc_with_error(
                    &ShapeArc::from_center_start_angle(
                        m_p0 + Vector2I::new(r, r),
                        m_p0 + Vector2I::new(0, r),
                        ANGLE_360,
                        0,
                    ),
                    max_error,
                );
            } else {
                let inner_rect = ShapeRect::from_pos_size(m_p0 + Vector2I::new(r, r), x_edge, y_edge);

                if x_edge > 0 {
                    // Either a normal roundrect or an oval with x_edge > 0

                    // Start to the right of the top left radius
                    outline.append_point(m_p0 + Vector2I::new(r, 0), false);

                    // Top side
                    outline.append_point(m_p0 + Vector2I::new(r + x_edge, 0), false);

                    if y_edge > 0 {
                        outline.append_arc_with_error(&make_corner_arc_cw90(&inner_rect, r, Directions::NE), max_error);
                        outline.append_point(m_p0 + Vector2I::new(w, r + y_edge), false);
                        outline.append_arc_with_error(&make_corner_arc_cw90(&inner_rect, r, Directions::SE), max_error);
                    } else {
                        outline.append_arc_with_error(&make_side_arc_cw180(&inner_rect, r, Directions::E), max_error);
                    }

                    // Bottom side
                    outline.append_point(m_p0 + Vector2I::new(r, h), false);

                    if y_edge > 0 {
                        outline.append_arc_with_error(&make_corner_arc_cw90(&inner_rect, r, Directions::SW), max_error);
                        outline.append_point(m_p0 + Vector2I::new(0, r), false);
                        outline.append_arc_with_error(&make_corner_arc_cw90(&inner_rect, r, Directions::NW), max_error);
                    } else {
                        outline.append_arc_with_error(&make_side_arc_cw180(&inner_rect, r, Directions::W), max_error);
                    }
                } else {
                    // x_edge is 0 but y_edge is not, so it's an oval the other way up
                    outline.append_point(m_p0 + Vector2I::new(0, r), false);
                    outline.append_arc_with_error(&make_side_arc_cw180(&inner_rect, r, Directions::N), max_error);
                    outline.append_point(m_p0 + Vector2I::new(w, r + y_edge), false);
                    outline.append_arc_with_error(&make_side_arc_cw180(&inner_rect, r, Directions::S), max_error);
                }
            }

            outline.set_closed(true);
        }

        *a_buffer = tmp;
    }
}
