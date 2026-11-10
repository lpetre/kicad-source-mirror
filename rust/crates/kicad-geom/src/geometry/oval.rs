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

//! Port of `libs/kimath/include/geometry/oval.h` and `libs/kimath/src/geometry/oval.cpp`
//! (namespace `KIGEOM`): utility functions for ovals (oblongs / stadiums), which are
//! represented by `SHAPE_SEGMENT`.

use crate::geometry::eda_angle::{ANGLE_90, ANGLE_180};
use crate::geometry::point_types::{PT_CENTER, PT_END, PT_MID, PT_QUADRANT, TypedPoint2I};
use crate::geometry::shape::Shape;
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_segment::ShapeSegment;
use crate::math::vector2::Vector2I;
use crate::trigo::{get_rotated, rotate_point};

/// Port of `OVAL_KEY_POINTS::OVAL_CENTER`.
pub const OVAL_CENTER: OvalKeyPointFlags = 1 << 0;
/// Port of `OVAL_KEY_POINTS::OVAL_CAP_TIPS`.
pub const OVAL_CAP_TIPS: OvalKeyPointFlags = 1 << 1;
/// Port of `OVAL_KEY_POINTS::OVAL_CAP_CENTERS`.
pub const OVAL_CAP_CENTERS: OvalKeyPointFlags = 1 << 2;
/// Port of `OVAL_KEY_POINTS::OVAL_SIDE_MIDPOINTS`.
pub const OVAL_SIDE_MIDPOINTS: OvalKeyPointFlags = 1 << 3;
/// Port of `OVAL_KEY_POINTS::OVAL_SIDE_ENDS`.
pub const OVAL_SIDE_ENDS: OvalKeyPointFlags = 1 << 4;
/// Port of `OVAL_KEY_POINTS::OVAL_CARDINAL_EXTREMES`.
pub const OVAL_CARDINAL_EXTREMES: OvalKeyPointFlags = 1 << 5;
/// Port of `OVAL_KEY_POINTS::OVAL_ALL_KEY_POINTS`.
pub const OVAL_ALL_KEY_POINTS: OvalKeyPointFlags = 0xFF;

/// Port of `OVAL_KEY_POINT_FLAGS`.
pub type OvalKeyPointFlags = u32;

/// Port of `KIGEOM::ConvertToChain()`: the closed outline of an oval, with two 180 degree arcs.
pub fn convert_to_chain(a_oval: &ShapeSegment) -> ShapeLineChain {
    let seg = *a_oval.get_seg();
    let perp = get_rotated(seg.b - seg.a, ANGLE_90).resize(a_oval.get_width() / 2);

    let mut chain = ShapeLineChain::new();
    chain.append_point(seg.a - perp, false);
    chain.append_arc(&ShapeArc::from_center_start_angle(
        seg.a,
        seg.a - perp,
        ANGLE_180,
        0,
    ));
    chain.append_point(seg.b + perp, false);
    chain.append_arc(&ShapeArc::from_center_start_angle(
        seg.b,
        seg.b + perp,
        ANGLE_180,
        0,
    ));
    chain.set_closed(true);
    chain
}

/// Port of `KIGEOM::GetOvalKeyPoints()`: interesting points on an oval (rectangle with
/// semicircular end caps) and their geometrical types, as selected by `a_flags`: the middles
/// of the sides, the tips of the end caps, the extreme cardinal points of the whole oval (if
/// rotated non-cardinally), ...
pub fn get_oval_key_points(a_oval: &ShapeSegment, a_flags: OvalKeyPointFlags) -> Vec<TypedPoint2I> {
    let half_width = a_oval.get_width() / 2;
    let half_len = a_oval.get_total_length() / 2;
    let rotation = a_oval.get_angle() - ANGLE_90;

    // Points on a non-rotated pad at the origin, long-axis is y
    // (so for now, width is left/right, len is up/down)
    let mut pts: Vec<TypedPoint2I> = Vec::new();
    let mut add = |pt: Vector2I, ty| pts.push(TypedPoint2I::new(pt, ty));

    if a_flags & OVAL_CENTER != 0 {
        // Centre is easy
        add(Vector2I::new(0, 0), PT_CENTER);
    }

    if a_flags & OVAL_SIDE_MIDPOINTS != 0 {
        // Side midpoints
        add(Vector2I::new(half_width, 0), PT_MID);
        add(Vector2I::new(-half_width, 0), PT_MID);
    }

    if a_flags & OVAL_CAP_TIPS != 0 {
        // If the oval is square-on, the tips are quadrants
        let pt_type = if rotation.is_cardinal() {
            PT_QUADRANT
        } else {
            PT_END
        };

        // Cap ends
        add(Vector2I::new(0, half_len), pt_type);
        add(Vector2I::new(0, -half_len), pt_type);
    }

    // Distance from centre to cap centres
    let d_centre_to_cap_centre = half_len - half_width;

    if a_flags & OVAL_CAP_CENTERS != 0 {
        // Cap centres
        add(Vector2I::new(0, d_centre_to_cap_centre), PT_CENTER);
        add(Vector2I::new(0, -d_centre_to_cap_centre), PT_CENTER);
    }

    if a_flags & OVAL_SIDE_ENDS != 0 {
        // End points of flat sides (always vertical)
        add(Vector2I::new(half_width, d_centre_to_cap_centre), PT_END);
        add(Vector2I::new(half_width, -d_centre_to_cap_centre), PT_END);
        add(Vector2I::new(-half_width, d_centre_to_cap_centre), PT_END);
        add(Vector2I::new(-half_width, -d_centre_to_cap_centre), PT_END);
    }

    // Add the quadrant points to the caps only if rotated
    // (otherwise they're just the tips)
    if (a_flags & OVAL_CARDINAL_EXTREMES) != 0 && !rotation.is_cardinal() {
        // We need to find two perpendicular lines from the centres
        // of each cap to the cap edge, which will hit the points
        // where the cap is tangent to H/V lines when rotated into place.
        //
        // Because we know the oval is always vertical, this means the
        // two lines are formed between _|, through \/ to |_
        // where the apex is the cap centre.

        // The vector from a cap centre to the tip (i.e. vertical)
        let cap_radial = Vector2I::new(0, half_width);

        // Rotate in the opposite direction to the oval's rotation
        // (that will be unwound later)
        let mut radial_line_rotation = rotation;

        radial_line_rotation.normalize90();

        let mut cap_radial_to_x_axis = cap_radial;
        rotate_point(&mut cap_radial_to_x_axis, radial_line_rotation);

        // Find the other line - it's 90 degrees away, but re-normalise
        // as it could be to the left or right
        radial_line_rotation -= ANGLE_90;
        radial_line_rotation.normalize90();

        let mut cap_radial_to_y_axis = cap_radial;
        rotate_point(&mut cap_radial_to_y_axis, radial_line_rotation);

        // The quadrant points are then the relevant offsets from each cap centre
        add(
            Vector2I::new(0, d_centre_to_cap_centre) + cap_radial_to_y_axis,
            PT_QUADRANT,
        );
        add(
            Vector2I::new(0, d_centre_to_cap_centre) + cap_radial_to_x_axis,
            PT_QUADRANT,
        );
        // The opposite cap offsets go from the other cap centre, the other way
        add(
            Vector2I::new(0, -d_centre_to_cap_centre) - cap_radial_to_y_axis,
            PT_QUADRANT,
        );
        add(
            Vector2I::new(0, -d_centre_to_cap_centre) - cap_radial_to_x_axis,
            PT_QUADRANT,
        );
    }

    for pt in &mut pts {
        // Transform to the actual orientation
        rotate_point(&mut pt.point, -rotation);

        // Translate to the actual position
        pt.point += a_oval.get_center();
    }

    pts
}
