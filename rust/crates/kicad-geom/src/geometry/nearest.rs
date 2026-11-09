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

//! Port of `libs/kimath/include/geometry/nearest.h` and `libs/kimath/src/geometry/nearest.cpp`:
//! the `NEARABLE_GEOM` variant and the nearest point on one or several geometries.

use crate::geometry::circle::Circle;
use crate::geometry::half_line::HalfLine;
use crate::geometry::line::Line;
use crate::geometry::seg::Seg;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_ellipse::ShapeEllipse;
use crate::geometry::shape_utils::box_to_segs;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of `NEARABLE_GEOM` (`std::variant<LINE, HALF_LINE, SEG, CIRCLE, SHAPE_ARC,
/// SHAPE_ELLIPSE, BOX2I, VECTOR2I>`).
#[derive(Clone, Debug)]
pub enum NearableGeom {
    /// `LINE`
    Line(Line),
    /// `HALF_LINE`
    HalfLine(HalfLine),
    /// `SEG`
    Seg(Seg),
    /// `CIRCLE`
    Circle(Circle),
    /// `SHAPE_ARC`
    Arc(ShapeArc),
    /// `SHAPE_ELLIPSE`
    Ellipse(ShapeEllipse),
    /// `BOX2I`
    Box(Box2I),
    /// `VECTOR2I`
    Point(Vector2I),
}

/// `VECTOR2I::Distance()` converted to `int` (truncation, as the C++ assignment to `int`).
fn int_distance(a_a: Vector2I, a_b: Vector2I) -> i32 {
    crate::cmath::f64_to_i32_x86(a_a.distance(a_b))
}

/// Port of the file-local `NearestPoint( const BOX2I&, const VECTOR2I& )`: the nearest point
/// on the outline of a box (the last of equally near sides wins).
fn nearest_point_box(a_box: &Box2I, a_pt: Vector2I) -> Vector2I {
    let mut nearest = Vector2I::new(0, 0);
    let mut best_distance = i32::MAX;

    for seg in &box_to_segs(a_box) {
        let nearest_seg_pt = seg.nearest_point(a_pt);
        let this_distance = int_distance(nearest_seg_pt, a_pt);

        if this_distance <= best_distance {
            nearest = nearest_seg_pt;
            best_distance = this_distance;
        }
    }

    nearest
}

/// Port of `GetNearestPoint( const NEARABLE_GEOM& aGeom, const VECTOR2I& aPt )`.
pub fn get_nearest_point(a_geom: &NearableGeom, a_pt: Vector2I) -> Vector2I {
    match a_geom {
        // Same signatures for all these types
        // But they're not in the same polymorphic hierarchy
        NearableGeom::Line(g) => g.nearest_point(a_pt),
        NearableGeom::HalfLine(g) => g.nearest_point(a_pt),
        NearableGeom::Seg(g) => g.nearest_point(a_pt),
        NearableGeom::Circle(g) => g.nearest_point(a_pt),
        NearableGeom::Arc(g) => g.nearest_point(a_pt),
        NearableGeom::Ellipse(g) => g.nearest_point(a_pt),
        // Defer to the utils function
        NearableGeom::Box(g) => nearest_point_box(g, a_pt),
        NearableGeom::Point(g) => *g,
    }
}

/// Port of `GetNearestPoint( const std::vector<NEARABLE_GEOM>& aGeoms, const VECTOR2I& aPt )`:
/// the nearest point on any of the geometries (the first of equally near ones), or `None` if
/// there are none.
pub fn get_nearest_point_any(a_geoms: &[NearableGeom], a_pt: Vector2I) -> Option<Vector2I> {
    let mut nearest_point_on_any: Option<Vector2I> = None;
    let mut best_distance = i32::MAX;

    for geom in a_geoms {
        let this_nearest = get_nearest_point(geom, a_pt);
        let this_distance = int_distance(this_nearest, a_pt);

        if nearest_point_on_any.is_none() || this_distance < best_distance {
            nearest_point_on_any = Some(this_nearest);
            best_distance = this_distance;
        }
    }

    nearest_point_on_any
}
