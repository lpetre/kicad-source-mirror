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

//! Partial port of `libs/kimath/include/geometry/shape_utils.h` and
//! `libs/kimath/src/geometry/shape_utils.cpp` (namespace `KIGEOM`): only `BoxToSegs`,
//! `ClipHalfLineToBox` (declared in `half_line.h`) and `ClipLineToBox`.  The rest of the file
//! (`DIRECTION_45` helpers, `MakeArcCw90`, ...) belongs to a later package.

use crate::geometry::half_line::HalfLine;
use crate::geometry::line::Line;
use crate::geometry::seg::Seg;
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of `KIGEOM::BoxToSegs()`: the four edges of a box, clockwise from the top left
/// (in KiCad's y-down coordinates).
pub fn box_to_segs(a_box: &Box2I) -> [Seg; 4] {
    let corners: [Vector2I; 4] = [
        Vector2I::new(a_box.left(), a_box.top()),
        Vector2I::new(a_box.right(), a_box.top()),
        Vector2I::new(a_box.right(), a_box.bottom()),
        Vector2I::new(a_box.left(), a_box.bottom()),
    ];

    [
        Seg::new(corners[0], corners[1]),
        Seg::new(corners[1], corners[2]),
        Seg::new(corners[2], corners[3]),
        Seg::new(corners[3], corners[0]),
    ]
}

/// Port of `KIGEOM::ClipHalfLineToBox()`: the part of the ray inside the box, if any.
pub fn clip_half_line_to_box(a_ray: &HalfLine, a_box: &Box2I) -> Option<Seg> {
    // Do the naive implementation - if this really is done in a tight loop,
    // the Cohen-Sutherland implementation in ClipLine could be faster, but
    // needs to be adapted to work with half-lines.
    let box_segs = box_to_segs(a_box);

    let mut pt_a: Option<Vector2I> = None;
    let mut pt_b: Option<Vector2I> = None;

    for box_seg in &box_segs {
        let Some(intersection) = a_ray.intersect_seg(box_seg) else {
            continue;
        };

        // Init the first point or eat it if it's the same
        if pt_a.is_none() || Some(intersection) == pt_a {
            pt_a = Some(intersection);
        } else {
            pt_b = Some(intersection);
        }
    }

    // If we have exactly two intersections, the ray crossed twice
    // so take the segment between the two points
    if let (Some(a), Some(b)) = (pt_a, pt_b) {
        return Some(Seg::new(a, b));
    }

    // It only crosses once, so the start is in the box. Take the segment from
    // the start point to the intersection
    if let Some(a) = pt_a {
        if a != a_ray.start() {
            return Some(Seg::new(a_ray.start(), a));
        }
    }

    // It didn't cross at all
    None
}

/// Port of `KIGEOM::ClipLineToBox()`: the part of the infinite line inside the box, if any.
pub fn clip_line_to_box(a_line: &Line, a_box: &Box2I) -> Option<Seg> {
    // As above, maybe can be optimised?
    let box_segs = box_to_segs(a_box);

    let mut pt_a: Option<Vector2I> = None;
    let mut pt_b: Option<Vector2I> = None;

    for box_seg in &box_segs {
        let intersection = a_line.intersect_seg(box_seg);

        // Reject intersections that are not on the actual box boundary
        if let Some(intersection) = intersection {
            if box_seg.contains(intersection) {
                // Init the first point or eat it if it's the same
                if pt_a.is_none() || Some(intersection) == pt_a {
                    pt_a = Some(intersection);
                } else {
                    pt_b = Some(intersection);
                }
            }
        }
    }

    // If we have exactly two intersections, we have a segment
    // (zero is no intersection, and one is a just crossing a corner exactly)
    if let (Some(a), Some(b)) = (pt_a, pt_b) {
        return Some(Seg::new(a, b));
    }

    None
}
