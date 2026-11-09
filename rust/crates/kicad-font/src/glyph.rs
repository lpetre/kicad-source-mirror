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

//! Port of `include/font/glyph.h`, `include/font/glyph_types.h` and `common/font/glyph.cpp`:
//! the `GLYPH` hierarchy as the [`Glyph`] enum with [`StrokeGlyph`] and [`OutlineGlyph`].

use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::{Box2D, EdaAngle, Vector2D, Vector2I, rotate_point_around};

/// Port of `KIFONT::GLYPH` and its subclasses `STROKE_GLYPH` / `OUTLINE_GLYPH`.
#[derive(Clone, Debug)]
pub enum Glyph {
    /// `STROKE_GLYPH`
    Stroke(StrokeGlyph),
    /// `OUTLINE_GLYPH`
    Outline(OutlineGlyph),
}

impl Glyph {
    /// Port of `GLYPH::IsStroke()`.
    pub fn is_stroke(&self) -> bool {
        matches!(self, Glyph::Stroke(_))
    }

    /// Port of `GLYPH::IsOutline()`.
    pub fn is_outline(&self) -> bool {
        matches!(self, Glyph::Outline(_))
    }

    /// Port of `GLYPH::BoundingBox()`.
    pub fn bounding_box(&self) -> Box2D {
        match self {
            Glyph::Stroke(g) => g.bounding_box(),
            Glyph::Outline(g) => g.bounding_box(),
        }
    }

    /// Port of `GLYPH::IsHover()`.
    pub fn is_hover(&self) -> bool {
        match self {
            Glyph::Stroke(g) => g.is_hover,
            Glyph::Outline(g) => g.is_hover,
        }
    }

    /// Port of `GLYPH::SetIsHover()`.
    pub fn set_is_hover(&mut self, is_hover: bool) {
        match self {
            Glyph::Stroke(g) => g.is_hover = is_hover,
            Glyph::Outline(g) => g.is_hover = is_hover,
        }
    }

    /// The stroke glyph, if this is one.
    pub fn as_stroke(&self) -> Option<&StrokeGlyph> {
        match self {
            Glyph::Stroke(g) => Some(g),
            Glyph::Outline(_) => None,
        }
    }

    /// The outline glyph, if this is one.
    pub fn as_outline(&self) -> Option<&OutlineGlyph> {
        match self {
            Glyph::Stroke(_) => None,
            Glyph::Outline(g) => Some(g),
        }
    }
}

/// Port of `KIFONT::STROKE_GLYPH` (a `std::vector<std::vector<VECTOR2D>>`): one point list per
/// pen-down polyline, plus a bounding box.
///
/// Glyphs of the font table are in font units (cap height about 1.0, y down, baseline at
/// y = 0); [`StrokeGlyph::transform`] produces board coordinates.
#[derive(Debug, Default)]
pub struct StrokeGlyph {
    /// The polylines (the `std::vector` base class).
    pub strokes: Vec<Vec<Vector2D>>,
    /// `m_penIsDown` (build state only)
    pen_is_down: bool,
    /// `m_boundingBox`
    bounding_box: Box2D,
    /// `GLYPH::m_isHover`
    pub is_hover: bool,
}

impl Clone for StrokeGlyph {
    /// Port of the copy constructor `STROKE_GLYPH( const STROKE_GLYPH& )`: the strokes and the
    /// bounding box are copied; the pen state and the hover flag (base class `GLYPH`
    /// default-constructed) are not.
    fn clone(&self) -> Self {
        StrokeGlyph {
            strokes: self.strokes.clone(),
            pen_is_down: false,
            bounding_box: self.bounding_box,
            is_hover: false,
        }
    }
}

impl StrokeGlyph {
    /// Port of `STROKE_GLYPH()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `STROKE_GLYPH::AddPoint`: a pen-up state starts a new polyline.
    pub fn add_point(&mut self, a_point: Vector2D) {
        if !self.pen_is_down {
            // This handles all but 359 strokes (out of over 328,000)
            self.strokes.push(Vec::with_capacity(16));
            self.pen_is_down = true;
        }

        if let Some(back) = self.strokes.last_mut() {
            back.push(a_point);
        }
    }

    /// Port of `STROKE_GLYPH::RaisePen`.
    pub fn raise_pen(&mut self) {
        self.pen_is_down = false;
    }

    /// Port of `STROKE_GLYPH::Finalize`: nothing to do (the C++ shrink-to-fit is disabled,
    /// "it's not worth it for the performance hit of doing more than 328,000 reallocs").
    pub fn finalize(&mut self) {}

    /// Port of `STROKE_GLYPH::BoundingBox()`.
    pub fn bounding_box(&self) -> Box2D {
        self.bounding_box
    }

    /// Port of `STROKE_GLYPH::SetBoundingBox()`.
    pub fn set_bounding_box(&mut self, bbox: Box2D) {
        self.bounding_box = bbox;
    }

    /// Port of `STROKE_GLYPH::Transform`: a copy of this glyph scaled by `a_glyph_size`,
    /// sheared by `a_tilt` (italic), moved by `a_offset`, mirrored about `a_origin.x` and
    /// rotated by `a_angle` about `a_origin`, in that order.
    pub fn transform(
        &self,
        a_glyph_size: Vector2D,
        a_offset: Vector2I,
        a_tilt: f64,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
    ) -> StrokeGlyph {
        let mut glyph = self.clone();

        let mut end: Vector2D = glyph.bounding_box.end();

        end.x *= a_glyph_size.x;
        end.y *= a_glyph_size.y;

        if a_tilt != 0.0 {
            end.x -= end.y * a_tilt;
        }

        let offset_d: Vector2D = a_offset.cast();
        let origin_d: Vector2D = a_origin.cast();

        glyph.bounding_box.set_end(end);
        glyph.bounding_box.offset(offset_d);

        for point_list in glyph.strokes.iter_mut() {
            for point in point_list.iter_mut() {
                point.x *= a_glyph_size.x;
                point.y *= a_glyph_size.y;

                if a_tilt != 0.0 {
                    point.x -= point.y * a_tilt;
                }

                point.x += offset_d.x;
                point.y += offset_d.y;

                if a_mirror {
                    point.x = origin_d.x - (point.x - origin_d.x);
                }

                if !a_angle.is_zero() {
                    rotate_point_around(point, origin_d, a_angle);
                }
            }
        }

        glyph
    }

    /// Port of `STROKE_GLYPH::Move`.
    pub fn move_(&mut self, a_offset: Vector2I) {
        let offset_d: Vector2D = a_offset.cast();

        self.bounding_box.offset(offset_d);

        for point_list in self.strokes.iter_mut() {
            for point in point_list.iter_mut() {
                point.x += offset_d.x;
                point.y += offset_d.y;
            }
        }
    }
}

/// Port of `KIFONT::OUTLINE_GLYPH` (a `SHAPE_POLY_SET`): the polygons of one outline-font
/// glyph, in board coordinates.
///
/// Only the data and the bounding box are ported; triangulation (`Triangulate`,
/// `CacheTriangulation`, `GetTriangulationData`) is left to the consumers until the
/// `kicad-geom` triangulation is available (see `PORT_STATUS.md`).
#[derive(Clone, Debug, Default)]
pub struct OutlineGlyph {
    /// The polygon set (the `SHAPE_POLY_SET` base class).
    pub poly: ShapePolySet,
    /// `GLYPH::m_isHover`
    pub is_hover: bool,
}

impl OutlineGlyph {
    /// Port of `OUTLINE_GLYPH( const SHAPE_POLY_SET& aPoly )`.
    pub fn new(poly: ShapePolySet) -> Self {
        OutlineGlyph { poly, is_hover: false }
    }

    /// Port of `OUTLINE_GLYPH::BoundingBox()`.
    pub fn bounding_box(&self) -> Box2D {
        let bbox = self.poly.bbox(0);
        Box2D::new(bbox.origin().cast(), bbox.size().cast::<f64>())
    }
}
