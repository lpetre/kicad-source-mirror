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

//! Port of `libs/kimath/src/geometry/shape_nearest_points.cpp`: `SHAPE::NearestPoints()` for
//! every pair of shape types.
//!
//! The C++ overloads `NearestPoints( const X&, const Y&, VECTOR2I& aPtA, VECTOR2I& aPtB )` are
//! the private functions `np_<x>_<y>` below; their reference parameters are in/out (`&mut`),
//! because several of them leave the incoming points untouched when nothing is found.
//! [`nearest_points`] is the public entry point (`SHAPE::NearestPoints`, the static
//! `nearestPoints()`), [`nearest_points_single_shapes`] the dispatcher
//! (`nearestPointsSingleShapes()`).

use crate::geometry::seg::Seg;
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_circle::ShapeCircle;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_segment::ShapeSegment;
use crate::math::vector2::Vector2I;

/// `dynamic_cast<const SHAPE_LINE_CHAIN*>( &aChain )`.
fn as_chain(a_chain: &dyn ShapeLineChainBase) -> Option<&ShapeLineChain> {
    match a_chain.as_shape_ref() {
        ShapeRef::LineChain(c) => Some(c),
        _ => None,
    }
}

/// Port of `NearestPoints( const SHAPE_CIRCLE&, const SHAPE_CIRCLE&, ... )`: the nearest points
/// between two (solid) circles.  Always succeeds.
fn np_circle_circle(
    a_a: &ShapeCircle,
    a_b: &ShapeCircle,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let delta = a_b.get_center() - a_a.get_center();
    let dist: i32 = delta.euclidean_norm();

    if dist == 0 {
        // Circles are concentric - pick arbitrary points
        *a_pt_a = a_a.get_center() + Vector2I::new(a_a.get_radius(), 0);
        *a_pt_b = a_b.get_center() + Vector2I::new(a_b.get_radius(), 0);
    } else {
        // Points lie on line between centers
        *a_pt_a = a_a.get_center() + delta.resize(a_a.get_radius());
        *a_pt_b = a_b.get_center() - delta.resize(a_b.get_radius());
    }

    true
}

/// Port of `NearestPoints( const SHAPE_CIRCLE&, const SHAPE_RECT&, ... )`: the nearest points
/// between a (solid) circle and a (solid) rectangle.  Always succeeds.
fn np_circle_rect(
    a_circle: &ShapeCircle,
    a_rect: &ShapeRect,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let c = a_circle.get_center();
    let p0 = a_rect.get_position();
    let size = a_rect.get_size();

    // Clamp circle center to rectangle bounds to find nearest point on rect
    a_pt_b.x = p0.x.max(c.x.min(p0.x + size.x));
    a_pt_b.y = p0.y.max(c.y.min(p0.y + size.y));

    // Find nearest point on circle
    if *a_pt_b == c {
        // Center is inside rectangle - find nearest edge
        let dist_to_left: i32 = c.x - p0.x;
        let dist_to_right: i32 = p0.x + size.x - c.x;
        let dist_to_top: i32 = c.y - p0.y;
        let dist_to_bottom: i32 = p0.y + size.y - c.y;

        let min_dist: i32 = dist_to_left
            .min(dist_to_right)
            .min(dist_to_top)
            .min(dist_to_bottom);

        if min_dist == dist_to_left {
            *a_pt_b = Vector2I::new(p0.x, c.y);
            *a_pt_a = c - Vector2I::new(a_circle.get_radius(), 0);
        } else if min_dist == dist_to_right {
            *a_pt_b = Vector2I::new(p0.x + size.x, c.y);
            *a_pt_a = c + Vector2I::new(a_circle.get_radius(), 0);
        } else if min_dist == dist_to_top {
            *a_pt_b = Vector2I::new(c.x, p0.y);
            *a_pt_a = c - Vector2I::new(0, a_circle.get_radius());
        } else {
            *a_pt_b = Vector2I::new(c.x, p0.y + size.y);
            *a_pt_a = c + Vector2I::new(0, a_circle.get_radius());
        }
    } else {
        let dir = (*a_pt_b - c).resize(a_circle.get_radius());
        *a_pt_a = c + dir;
    }

    true
}

/// Port of `NearestPoints( const SHAPE_CIRCLE&, const SEG&, ... )`: the nearest points between
/// a (solid) circle and a line segment.  Always succeeds.
fn np_circle_seg(
    a_circle: &ShapeCircle,
    a_seg: &Seg,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    *a_pt_b = a_seg.nearest_point(a_circle.get_center());

    if *a_pt_b == a_circle.get_center() {
        // Center is on segment - pick perpendicular direction
        let dir = (a_seg.b - a_seg.a)
            .perpendicular()
            .resize(a_circle.get_radius());
        *a_pt_a = a_circle.get_center() + dir;
    } else {
        let dir = (*a_pt_b - a_circle.get_center()).resize(a_circle.get_radius());
        *a_pt_a = a_circle.get_center() + dir;
    }

    true
}

/// Port of `NearestPoints( const SHAPE_CIRCLE&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn np_circle_lcb(
    a_circle: &ShapeCircle,
    a_chain: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let chain = as_chain(a_chain);
    let mut min_dist_sq: i64 = i64::MAX;

    for i in 0..a_chain.get_segment_count() {
        if chain.is_some_and(|c| c.is_arc_segment(i)) {
            continue;
        }

        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();

        if np_circle_seg(
            a_circle,
            &a_chain.get_segment(i as i32),
            &mut pt_a,
            &mut pt_b,
        ) {
            let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();

            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                *a_pt_a = pt_a;
                *a_pt_b = pt_b;
            }
        }
    }

    // Also handle arcs if this is a SHAPE_LINE_CHAIN
    if let Some(chain) = chain {
        for j in 0..chain.arc_count() {
            let arc = chain.arc(j);

            let mut pt_a = Vector2I::default();
            let mut pt_b = Vector2I::default();
            let mut dist_sq: i64 = 0;

            // Reverse the output points to match the arc_from_chain/circle order
            if arc.nearest_points_circle(a_circle, &mut pt_b, &mut pt_a, &mut dist_sq)
                && dist_sq < min_dist_sq
            {
                min_dist_sq = dist_sq;
                *a_pt_a = pt_a;
                *a_pt_b = pt_b;
            }
        }
    }

    min_dist_sq < i64::MAX
}

/// Port of `NearestPoints( const SHAPE_RECT&, const SHAPE_RECT&, ... )`.
fn np_rect_rect(
    a_a: &ShapeRect,
    a_b: &ShapeRect,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    // Convert rectangles to line chains and use that algorithm
    let outline_a = a_a.outline();
    let outline_b = a_b.outline();

    np_lcb_lcb(&outline_a, &outline_b, a_pt_a, a_pt_b)
}

/// Port of `NearestPoints( const SHAPE_RECT&, const SEG&, ... )`.
fn np_rect_seg(
    a_rect: &ShapeRect,
    a_seg: &Seg,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let outline = a_rect.outline();

    // Reverse the output points to match the seg/rect_outline order
    np_seg_lcb(a_seg, &outline, a_pt_b, a_pt_a)
}

/// Port of `NearestPoints( const SHAPE_RECT&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn np_rect_lcb(
    a_rect: &ShapeRect,
    a_chain: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let outline = a_rect.outline();

    np_lcb_lcb(&outline, a_chain, a_pt_a, a_pt_b)
}

/// Port of `NearestPoints( const SEG&, const SEG&, ... )`.  Always succeeds.
fn np_seg_seg(a_a: &Seg, a_b: &Seg, a_pt_a: &mut Vector2I, a_pt_b: &mut Vector2I) -> bool {
    *a_pt_a = a_a.nearest_point_seg(a_b);
    *a_pt_b = a_b.nearest_point(*a_pt_a);

    true
}

/// Port of `NearestPoints( const SEG&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn np_seg_lcb(
    a_seg: &Seg,
    a_chain: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let chain = as_chain(a_chain);
    let mut min_dist_sq: i64 = i64::MAX;

    for i in 0..a_chain.get_segment_count() {
        if chain.is_some_and(|c| c.is_arc_segment(i)) {
            continue;
        }

        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();

        if np_seg_seg(a_seg, &a_chain.get_segment(i as i32), &mut pt_a, &mut pt_b) {
            let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();

            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                *a_pt_a = pt_a;
                *a_pt_b = pt_b;
            }
        }
    }

    // Also handle arcs if this is a SHAPE_LINE_CHAIN
    if let Some(chain) = chain {
        for j in 0..chain.arc_count() {
            let arc = chain.arc(j);

            let mut pt_a = Vector2I::default();
            let mut pt_b = Vector2I::default();
            let mut dist_sq: i64 = 0;

            // Reverse the output points to match the arc_from_chain/seg order
            if arc.nearest_points_seg(a_seg, &mut pt_b, &mut pt_a, &mut dist_sq)
                && dist_sq < min_dist_sq
            {
                min_dist_sq = dist_sq;
                *a_pt_a = pt_a;
                *a_pt_b = pt_b;
            }
        }
    }

    min_dist_sq < i64::MAX
}

/// Port of `NearestPoints( const SHAPE_LINE_CHAIN_BASE&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn np_lcb_lcb(
    a_a: &dyn ShapeLineChainBase,
    a_b: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let chain_a = as_chain(a_a);
    let chain_b = as_chain(a_b);
    let mut min_dist_sq: i64 = i64::MAX;

    let mut take = |dist_sq: i64, pt_a: Vector2I, pt_b: Vector2I, min_dist_sq: &mut i64| {
        if dist_sq < *min_dist_sq {
            *min_dist_sq = dist_sq;
            *a_pt_a = pt_a;
            *a_pt_b = pt_b;
        }
    };

    // Check all segment pairs
    for i in 0..a_a.get_segment_count() {
        if chain_a.is_some_and(|c| c.is_arc_segment(i)) {
            continue;
        }

        for j in 0..a_b.get_segment_count() {
            if chain_b.is_some_and(|c| c.is_arc_segment(j)) {
                continue;
            }

            let mut pt_a = Vector2I::default();
            let mut pt_b = Vector2I::default();

            if np_seg_seg(
                &a_a.get_segment(i as i32),
                &a_b.get_segment(j as i32),
                &mut pt_a,
                &mut pt_b,
            ) {
                let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();
                take(dist_sq, pt_a, pt_b, &mut min_dist_sq);
            }
        }
    }

    // Also handle arcs if this is a SHAPE_LINE_CHAIN
    if let Some(chain_a) = chain_a {
        for i in 0..chain_a.arc_count() {
            let arc_a = chain_a.arc(i);

            if let Some(chain_b) = chain_b {
                // Arc to arc
                for j in 0..chain_b.arc_count() {
                    let mut pt_a = Vector2I::default();
                    let mut pt_b = Vector2I::default();
                    let mut dist_sq: i64 = 0;

                    if arc_a.nearest_points_arc(chain_b.arc(j), &mut pt_a, &mut pt_b, &mut dist_sq)
                    {
                        take(dist_sq, pt_a, pt_b, &mut min_dist_sq);
                    }
                }
            }

            // Arc to segments
            for j in 0..a_b.get_segment_count() {
                let mut pt_a = Vector2I::default();
                let mut pt_b = Vector2I::default();
                let mut dist_sq: i64 = 0;

                if arc_a.nearest_points_seg(
                    &a_b.get_segment(j as i32),
                    &mut pt_a,
                    &mut pt_b,
                    &mut dist_sq,
                ) {
                    take(dist_sq, pt_a, pt_b, &mut min_dist_sq);
                }
            }
        }
    }

    if let (Some(chain_b), None) = (chain_b, chain_a) {
        // Handle arcs in chainB vs segments in aA
        for j in 0..chain_b.arc_count() {
            let arc_b = chain_b.arc(j);

            for i in 0..a_a.get_segment_count() {
                let mut pt_a = Vector2I::default();
                let mut pt_b = Vector2I::default();
                let mut dist_sq: i64 = 0;

                if arc_b.nearest_points_seg(
                    &a_a.get_segment(i as i32),
                    &mut pt_b,
                    &mut pt_a,
                    &mut dist_sq,
                ) {
                    take(dist_sq, pt_a, pt_b, &mut min_dist_sq);
                }
            }
        }
    }

    min_dist_sq < i64::MAX
}

/// Port of `NearestPoints( const SHAPE_ARC&, const SHAPE_SEGMENT&, ... )`.
fn np_arc_segment(
    a_arc: &ShapeArc,
    a_seg: &ShapeSegment,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let mut dist_sq: i64 = 0;
    let ret_val = a_arc.nearest_points_seg(a_seg.get_seg(), a_pt_a, a_pt_b, &mut dist_sq);

    // Adjust point B by half the seg width towards point A
    let dir = (*a_pt_a - *a_pt_b).resize(a_seg.get_width() / 2);
    *a_pt_b += dir;

    ret_val
}

/// Port of `NearestPoints( const SHAPE_ARC&, const SHAPE_LINE_CHAIN_BASE&, ... )`.  Returns
/// true even when the chain has nothing to compare against (the points are then untouched).
fn np_arc_lcb(
    a_arc: &ShapeArc,
    a_chain: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let chain = as_chain(a_chain);
    let mut dist_sq: i64 = 0;
    let mut min_dist_sq: i64 = i64::MAX;
    let mut tmp_pt_a = Vector2I::default();
    let mut tmp_pt_b = Vector2I::default();

    for i in 0..a_chain.get_segment_count() {
        if chain.is_some_and(|c| c.is_arc_segment(i)) {
            continue;
        }

        if a_arc.nearest_points_seg(
            &a_chain.get_segment(i as i32),
            &mut tmp_pt_a,
            &mut tmp_pt_b,
            &mut dist_sq,
        ) && dist_sq < min_dist_sq
        {
            *a_pt_a = tmp_pt_a;
            *a_pt_b = tmp_pt_b;
            min_dist_sq = dist_sq;
        }
    }

    // Also handle arcs if this is a SHAPE_LINE_CHAIN
    if let Some(chain) = chain {
        for j in 0..chain.arc_count() {
            let arc = chain.arc(j);

            if a_arc.nearest_points_arc(arc, &mut tmp_pt_a, &mut tmp_pt_b, &mut dist_sq)
                && dist_sq < min_dist_sq
            {
                min_dist_sq = dist_sq;
                *a_pt_a = tmp_pt_a;
                *a_pt_b = tmp_pt_b;
            }
        }
    }

    true
}

/// Port of `NearestPoints( const SHAPE_SEGMENT&, const SHAPE_CIRCLE&, ... )`.
fn np_segment_circle(
    a_seg: &ShapeSegment,
    a_circle: &ShapeCircle,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    if np_circle_seg(a_circle, a_seg.get_seg(), a_pt_b, a_pt_a) {
        // Adjust point A by half the segment width towards point B
        let dir = (*a_pt_b - *a_pt_a).resize(a_seg.get_width() / 2);
        *a_pt_a += dir;
        return true;
    }

    false
}

/// Port of `NearestPoints( const SHAPE_SEGMENT&, const SHAPE_RECT&, ... )`.
fn np_segment_rect(
    a_seg: &ShapeSegment,
    a_rect: &ShapeRect,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    if np_rect_seg(a_rect, a_seg.get_seg(), a_pt_b, a_pt_a) {
        // Adjust point A by half the segment width towards point B
        let dir = (*a_pt_b - *a_pt_a).resize(a_seg.get_width() / 2);
        *a_pt_a += dir;
        return true;
    }

    false
}

/// Port of `NearestPoints( const SHAPE_SEGMENT&, const SHAPE_SEGMENT&, ... )`.
fn np_segment_segment(
    a_seg_a: &ShapeSegment,
    a_seg_b: &ShapeSegment,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    // Find nearest points between two segments
    if np_seg_seg(a_seg_a.get_seg(), a_seg_b.get_seg(), a_pt_a, a_pt_b) {
        // Adjust point A by half the segment width towards point B
        let mut dir = (*a_pt_b - *a_pt_a).resize(a_seg_a.get_width() / 2);
        *a_pt_a += dir;
        // Adjust point B by half the segment width towards point A
        dir = (*a_pt_a - *a_pt_b).resize(a_seg_b.get_width() / 2);
        *a_pt_b += dir;
        return true;
    }

    false
}

/// Port of `NearestPoints( const SHAPE_SEGMENT&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn np_segment_lcb(
    a_seg: &ShapeSegment,
    a_chain: &dyn ShapeLineChainBase,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    if np_seg_lcb(a_seg.get_seg(), a_chain, a_pt_a, a_pt_b) {
        // Adjust point A by half the segment width towards point B
        let dir = (*a_pt_b - *a_pt_a).resize(a_seg.get_width() / 2);
        *a_pt_a += dir;
        return true;
    }

    false
}

/// Port of `NearestPoints( const SHAPE_ARC&, const SHAPE_CIRCLE / SHAPE_RECT / SHAPE_ARC&,
/// ... )`: the arc's own `NearestPoints()`.
fn np_arc_circle(a: &ShapeArc, b: &ShapeCircle, pa: &mut Vector2I, pb: &mut Vector2I) -> bool {
    let mut dist_sq: i64 = 0;
    a.nearest_points_circle(b, pa, pb, &mut dist_sq)
}

fn np_arc_rect(a: &ShapeArc, b: &ShapeRect, pa: &mut Vector2I, pb: &mut Vector2I) -> bool {
    let mut dist_sq: i64 = 0;
    a.nearest_points_rect(b, pa, pb, &mut dist_sq)
}

fn np_arc_arc(a: &ShapeArc, b: &ShapeArc, pa: &mut Vector2I, pb: &mut Vector2I) -> bool {
    let mut dist_sq: i64 = 0;
    a.nearest_points_arc(b, pa, pb, &mut dist_sq)
}

/// The SH_LINE_CHAIN / SH_ARC case of `nearestPointsSingleShapes()` ("Special handling for
/// arc"): all segments of the chain (arc segments included) and its arcs.
fn np_chain_arc(
    chain: &ShapeLineChain,
    arc: &ShapeArc,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let mut min_dist_sq: i64 = i64::MAX;

    // Check segments
    for i in 0..chain.segment_count() {
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();
        let mut dist_sq: i64 = 0;

        // Reverse the output points to match the arc/segment_from_rect order
        if arc.nearest_points_seg(&chain.c_segment(i), &mut pt_b, &mut pt_a, &mut dist_sq)
            && dist_sq < min_dist_sq
        {
            min_dist_sq = dist_sq;
            *a_pt_a = pt_a;
            *a_pt_b = pt_b;
        }
    }

    // Check arcs
    for i in 0..chain.arc_count() {
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();
        let mut dist_sq: i64 = 0;

        if chain
            .arc(i)
            .nearest_points_arc(arc, &mut pt_a, &mut pt_b, &mut dist_sq)
            && dist_sq < min_dist_sq
        {
            min_dist_sq = dist_sq;
            *a_pt_a = pt_a;
            *a_pt_b = pt_b;
        }
    }

    min_dist_sq < i64::MAX
}

/// The SH_SIMPLE / SH_POLY_SET_TRIANGLE against SH_ARC case of `nearestPointsSingleShapes()`
/// (segments only).
fn np_lcb_arc(
    chain: &dyn ShapeLineChainBase,
    arc: &ShapeArc,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let mut min_dist_sq: i64 = i64::MAX;

    for i in 0..chain.get_segment_count() {
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();
        let mut dist_sq: i64 = 0;

        // Reverse the output points to match the arc/segment_from_line_chain order
        if arc.nearest_points_seg(
            &chain.get_segment(i as i32),
            &mut pt_b,
            &mut pt_a,
            &mut dist_sq,
        ) && dist_sq < min_dist_sq
        {
            min_dist_sq = dist_sq;
            *a_pt_a = pt_a;
            *a_pt_b = pt_b;
        }
    }

    min_dist_sq < i64::MAX
}

/// Port of the static `nearestPointsSingleShapes()`: the dispatcher for two non-compound
/// shapes.  Pairs the C++ does not handle (null, ellipse, compound members, ...) return false
/// and leave the points untouched.
pub fn nearest_points_single_shapes(
    a_a: ShapeRef<'_>,
    a_b: ShapeRef<'_>,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    use ShapeRef as S;

    let (pa, pb) = (a_pt_a, a_pt_b);

    // NearestPointsCase<A, B>( a, b, pa, pb ) is np_a_b( a, b, pa, pb );
    // NearestPointsCaseReversed<A, B>( a, b, pa, pb ) is np_b_a( b, a, pb, pa ).
    match (a_a, a_b) {
        (S::Rect(a), S::Rect(b)) => return np_rect_rect(a, b, pa, pb),
        (S::Rect(a), S::Circle(b)) => return np_circle_rect(b, a, pb, pa),
        (S::Rect(a), S::LineChain(b)) => return np_rect_lcb(a, b, pa, pb),
        (S::Rect(a), S::Segment(b)) => return np_segment_rect(b, a, pb, pa),
        (S::Rect(a), S::Simple(b)) => return np_rect_lcb(a, b, pa, pb),
        (S::Rect(a), S::PolySetTriangle(b)) => return np_rect_lcb(a, b, pa, pb),
        (S::Rect(a), S::Arc(b)) => return np_arc_rect(b, a, pb, pa),

        (S::Circle(a), S::Rect(b)) => return np_circle_rect(a, b, pa, pb),
        (S::Circle(a), S::Circle(b)) => return np_circle_circle(a, b, pa, pb),
        (S::Circle(a), S::LineChain(b)) => return np_circle_lcb(a, b, pa, pb),
        (S::Circle(a), S::Segment(b)) => return np_segment_circle(b, a, pb, pa),
        (S::Circle(a), S::Simple(b)) => return np_circle_lcb(a, b, pa, pb),
        (S::Circle(a), S::PolySetTriangle(b)) => return np_circle_lcb(a, b, pa, pb),
        (S::Circle(a), S::Arc(b)) => return np_arc_circle(b, a, pb, pa),

        (S::LineChain(a), S::Rect(b)) => return np_rect_lcb(b, a, pb, pa),
        (S::LineChain(a), S::Circle(b)) => return np_circle_lcb(b, a, pb, pa),
        (S::LineChain(a), S::LineChain(b)) => return np_lcb_lcb(a, b, pa, pb),
        (S::LineChain(a), S::Segment(b)) => return np_segment_lcb(b, a, pb, pa),
        (S::LineChain(a), S::Simple(b)) => return np_lcb_lcb(a, b, pa, pb),
        (S::LineChain(a), S::PolySetTriangle(b)) => return np_lcb_lcb(a, b, pa, pb),
        (S::LineChain(a), S::Arc(b)) => return np_chain_arc(a, b, pa, pb),

        (S::Segment(a), S::Rect(b)) => return np_segment_rect(a, b, pa, pb),
        (S::Segment(a), S::Circle(b)) => return np_segment_circle(a, b, pa, pb),
        (S::Segment(a), S::LineChain(b)) => return np_segment_lcb(a, b, pa, pb),
        (S::Segment(a), S::Segment(b)) => return np_segment_segment(a, b, pa, pb),
        (S::Segment(a), S::Simple(b)) => return np_segment_lcb(a, b, pa, pb),
        (S::Segment(a), S::PolySetTriangle(b)) => return np_segment_lcb(a, b, pa, pb),
        (S::Segment(a), S::Arc(b)) => return np_arc_segment(b, a, pb, pa),

        (S::Simple(_) | S::PolySetTriangle(_), _)
            if !matches!(a_b, S::PolySet(_) | S::Compound(_) | S::Null(_) | S::Ellipse(_)) =>
        {
            let a: &dyn ShapeLineChainBase = a_a.as_line_chain_base().expect("line chain base");

            return match a_b {
                S::Rect(b) => np_rect_lcb(b, a, pb, pa),
                S::Circle(b) => np_circle_lcb(b, a, pb, pa),
                S::LineChain(b) => np_lcb_lcb(b, a, pb, pa),
                S::Segment(b) => np_segment_lcb(b, a, pb, pa),
                S::Simple(b) => np_lcb_lcb(a, b, pa, pb),
                S::PolySetTriangle(b) => np_lcb_lcb(a, b, pa, pb),
                S::Arc(b) => np_lcb_arc(a, b, pa, pb),
                _ => false,
            };
        }

        (S::Arc(a), S::Rect(b)) => return np_arc_rect(a, b, pa, pb),
        (S::Arc(a), S::Circle(b)) => return np_arc_circle(a, b, pa, pb),
        (S::Arc(a), S::LineChain(b)) => return np_arc_lcb(a, b, pa, pb),
        (S::Arc(a), S::Segment(b)) => return np_arc_segment(a, b, pa, pb),
        (S::Arc(a), S::Simple(b)) => return np_arc_lcb(a, b, pa, pb),
        (S::Arc(a), S::PolySetTriangle(b)) => return np_arc_lcb(a, b, pa, pb),
        (S::Arc(a), S::Arc(b)) => return np_arc_arc(a, b, pa, pb),

        (S::PolySet(poly_set), _) => {
            // For polygon sets, find nearest points to all edges
            let mut min_dist_sq: i64 = i64::MAX;
            let mut it = poly_set.c_iterate_segments_with_holes();

            while it.is_valid() {
                let seg = ShapeSegment::from_seg(it.get(), 0);
                let mut pt_a = Vector2I::default();
                let mut pt_b = Vector2I::default();

                if nearest_points_single_shapes(S::Segment(&seg), a_b, &mut pt_a, &mut pt_b) {
                    let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();

                    if dist_sq < min_dist_sq {
                        min_dist_sq = dist_sq;
                        *pa = pt_a;
                        *pb = pt_b;
                    }
                }

                it.advance();
            }

            return min_dist_sq < i64::MAX;
        }

        _ => {}
    }

    // Handle SHAPE_POLY_SET as second shape
    if let S::PolySet(poly_set) = a_b {
        let mut min_dist_sq: i64 = i64::MAX;
        let mut it = poly_set.c_iterate_segments();

        while it.is_valid() {
            let seg = ShapeSegment::from_seg(it.get(), 0);
            let mut pt_a = Vector2I::default();
            let mut pt_b = Vector2I::default();

            if nearest_points_single_shapes(a_a, S::Segment(&seg), &mut pt_a, &mut pt_b) {
                let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();

                if dist_sq < min_dist_sq {
                    min_dist_sq = dist_sq;
                    *pa = pt_a;
                    *pb = pt_b;
                }
            }

            it.advance();
        }

        return min_dist_sq < i64::MAX;
    }

    false
}

/// Port of `SHAPE::NearestPoints( const SHAPE* aOther, VECTOR2I& aPtThis, VECTOR2I& aPtOther )`
/// (the static `nearestPoints()`): compound shapes are resolved into all sub-shape pairs, the
/// closest result wins.  Returns false (points untouched) when no pair yields points.
pub fn nearest_points(
    a_a: ShapeRef<'_>,
    a_b: ShapeRef<'_>,
    a_pt_a: &mut Vector2I,
    a_pt_b: &mut Vector2I,
) -> bool {
    let mut min_dist_sq: i64 = i64::MAX;
    let mut found = false;

    let mut check_nearest_points = |shape_a: ShapeRef<'_>, shape_b: ShapeRef<'_>| -> bool {
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();

        if nearest_points_single_shapes(shape_a, shape_b, &mut pt_a, &mut pt_b) {
            let dist_sq: i64 = (pt_b - pt_a).squared_euclidean_norm();

            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                *a_pt_a = pt_a;
                *a_pt_b = pt_b;
                found = true;
            }

            return true;
        }

        false
    };

    match (a_a, a_b) {
        (ShapeRef::Compound(cmp_a), ShapeRef::Compound(cmp_b)) => {
            for elem_a in cmp_a.shapes() {
                for elem_b in cmp_b.shapes() {
                    check_nearest_points(elem_a.as_shape_ref(), elem_b.as_shape_ref());
                }
            }
        }
        (ShapeRef::Compound(cmp_a), _) => {
            for elem_a in cmp_a.shapes() {
                check_nearest_points(elem_a.as_shape_ref(), a_b);
            }
        }
        (_, ShapeRef::Compound(cmp_b)) => {
            for elem_b in cmp_b.shapes() {
                check_nearest_points(a_a, elem_b.as_shape_ref());
            }
        }
        _ => {
            return nearest_points_single_shapes(a_a, a_b, a_pt_a, a_pt_b);
        }
    }

    found
}
