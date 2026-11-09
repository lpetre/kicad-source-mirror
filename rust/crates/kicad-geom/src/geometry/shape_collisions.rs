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

//! Partial port of `libs/kimath/src/geometry/shape_collisions.cpp` (the generic
//! `SHAPE::Collide( const SHAPE*, ... )` entry points and the collision matrix) and of the entry
//! point of `libs/kimath/src/geometry/shape_nearest_points.cpp`.  The full port of both files
//! is package G5.
//!
//! [`collide_shapes`] is the single dispatch function.  It serves every pair whose C++
//! implementation only needs the shapes ported so far; the remaining pairs end in
//! [`unsupported_g5`] (marked "G5" below), which returns false, just like the C++ fallback for
//! a pair the matrix does not know ("Unsupported collision").  `PORT_STATUS.md` lists them:
//!
//! - line chain / simple polygon / poly-set triangle against each other
//!   (`Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )`),
//! - rectangle against rectangle when a clearance, distance, location or MTV is requested,
//! - arc against rectangle unless the arc is effectively a line (needs `SHAPE::NearestPoints`),
//! - anything against a poly set (`SHAPE_POLY_SET::Collide( const SHAPE* )`, package G4/G5).
//!
//! Rectangles with rounded corners go through `SHAPE_RECT::Outline()`, which needs `ROUNDRECT`
//! (not ported yet; see `PORT_STATUS.md`).

use crate::geometry::seg::Ecoord;
use crate::geometry::seg::Seg;
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_circle::ShapeCircle;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_segment::ShapeSegment;
use crate::math::util::ki_round;
use crate::math::vector2::Vector2I;

/// Writes `v` through an optional out-parameter.
fn set<T>(a_out: &mut Option<&mut T>, v: T) {
    if let Some(o) = a_out.as_deref_mut() {
        *o = v;
    }
}

/// Fallback for the pairs of the collision matrix that are not ported yet (package G5).
/// Returns false, like the C++ fallback for an unsupported pair.
pub fn unsupported_g5(_a_a: ShapeRef<'_>, _a_b: ShapeRef<'_>) -> bool {
    false
}

/// Port of `Collide( const SHAPE_CIRCLE&, const SHAPE_CIRCLE&, ... )`.
fn collide_circle_circle(
    a_a: &ShapeCircle,
    a_b: &ShapeCircle,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    // int additions, widened afterwards (as in C++)
    let min_dist: Ecoord = (a_clearance + a_a.get_radius() + a_b.get_radius()) as Ecoord;
    let min_dist_sq: Ecoord = min_dist * min_dist;

    let delta = a_b.get_center() - a_a.get_center();
    let dist_sq: Ecoord = delta.squared_euclidean_norm();

    if dist_sq == 0 || dist_sq < min_dist_sq {
        set(
            &mut a_actual,
            0.max((dist_sq as f64).sqrt() as i32 - a_a.get_radius() - a_b.get_radius()),
        );
        set(&mut a_location, (a_a.get_center() + a_b.get_center()) / 2);
        // fixme: apparent rounding error
        set(
            &mut a_mtv,
            delta.resize((min_dist as f64 - (dist_sq as f64).sqrt() + 3.0) as i32),
        );

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_RECT&, const SHAPE_CIRCLE&, ... )`.
fn collide_rect_circle(
    a_a: &ShapeRect,
    a_b: &ShapeCircle,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_a.get_radius() > 0 {
        // "MTV not implemented for SHAPE_RECT to SHAPE_CIRCLE collisions when rect has rounded
        // corners" (wxASSERT_MSG)
        let outline = a_a.outline();
        return collide_shapes(
            ShapeRef::LineChain(&outline),
            ShapeRef::Circle(a_b),
            a_clearance,
            a_actual,
            a_location,
            None,
        );
    }

    let c = a_b.get_center();
    let p0 = a_a.get_position();
    let size = a_a.get_size();
    let r: i32 = a_b.get_radius();
    let min_dist: i32 = a_clearance + r;
    let min_dist_sq: Ecoord = Seg::square(min_dist);

    let vts: [Vector2I; 5] = [
        Vector2I::new(p0.x, p0.y),
        Vector2I::new(p0.x, p0.y + size.y),
        Vector2I::new(p0.x + size.x, p0.y + size.y),
        Vector2I::new(p0.x + size.x, p0.y),
        Vector2I::new(p0.x, p0.y),
    ];

    let mut nearest_side_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
    let mut nearest = Vector2I::default();

    let inside = c.x >= p0.x && c.x <= (p0.x + size.x) && c.y >= p0.y && c.y <= (p0.y + size.y);

    // If we're not looking for MTV or actual, short-circuit once we find a hard collision
    if inside && a_actual.is_none() && a_location.is_none() && a_mtv.is_none() {
        return true;
    }

    for i in 0..4 {
        let side = Seg::new(vts[i], vts[i + 1]);

        let pn = side.nearest_point(c);
        let side_dist_sq: Ecoord = (pn - c).squared_euclidean_norm();

        if side_dist_sq < nearest_side_dist_sq {
            nearest = pn;
            nearest_side_dist_sq = side_dist_sq;

            if a_mtv.is_some() {
                continue;
            }

            if nearest_side_dist_sq == 0 {
                break;
            }

            // If we're not looking for aActual then any collision will do
            if nearest_side_dist_sq < min_dist_sq && a_actual.is_none() {
                break;
            }
        }
    }

    if inside || nearest_side_dist_sq == 0 || nearest_side_dist_sq < min_dist_sq {
        set(&mut a_location, nearest);
        set(
            &mut a_actual,
            0.max((nearest_side_dist_sq as f64).sqrt() as i32 - r),
        );

        if let Some(mtv) = a_mtv {
            let delta = c - nearest;
            let nearest_dist: f64 = (nearest_side_dist_sq as f64).sqrt();

            if inside {
                *mtv = -delta.resize(((min_dist as f64 + 1.0 + nearest_dist).abs() + 1.0) as i32);
            } else {
                *mtv = delta.resize(((min_dist as f64 + 1.0 - nearest_dist).abs() + 1.0) as i32);
            }
        }

        return true;
    }

    false
}

/// Port of the static `pushoutForce()`.
fn pushout_force(a_a: &ShapeCircle, a_b: &Seg, a_clearance: i32) -> Vector2I {
    let mut f = Vector2I::new(0, 0);

    let c = a_a.get_center();
    let nearest = a_b.nearest_point(c);

    let r: i32 = a_a.get_radius();

    let dist: i32 = (nearest - c).euclidean_norm();
    let min_dist: i32 = a_clearance + r;

    if dist < min_dist {
        for corr in 0..5 {
            f = (a_a.get_center() - nearest).resize(min_dist - dist + corr);

            if a_b.distance(c + f) >= min_dist {
                break;
            }
        }
    }

    f
}

/// Port of `Collide( const SHAPE_CIRCLE&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn collide_circle_lcb(
    a_a: &ShapeCircle,
    a_b: &dyn ShapeLineChainBase,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    let mut closest_dist: i32 = i32::MAX;
    let mut closest_mtv_dist: i32 = i32::MAX;
    let mut nearest = Vector2I::default();
    let mut closest_mtv_seg: i32 = -1;

    if a_b.is_closed() && a_b.point_inside(a_a.get_center(), 0, false) {
        nearest = a_a.get_center();
        closest_dist = 0;

        if a_mtv.is_some() {
            for s in 0..a_b.get_segment_count() {
                let dist: i32 = a_b.get_segment(s as i32).distance(a_a.get_center());

                if dist < closest_mtv_dist {
                    closest_mtv_dist = dist;
                    closest_mtv_seg = s as i32;
                }
            }
        }
    } else {
        let want_dist = a_actual.is_some() || a_location.is_some();
        let want_location = a_location.is_some();

        for s in 0..a_b.get_segment_count() {
            let mut collision_dist: i32 = 0;
            let mut pn = Vector2I::default();

            if a_a.collide_seg(
                &a_b.get_segment(s as i32),
                a_clearance,
                if want_dist {
                    Some(&mut collision_dist)
                } else {
                    None
                },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if collision_dist < closest_dist {
                    nearest = pn;
                    closest_dist = collision_dist;
                }

                if closest_dist == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if a_actual.is_none() {
                    break;
                }
            }
        }
    }

    if closest_dist == 0 || closest_dist < a_clearance {
        set(&mut a_location, nearest);
        set(&mut a_actual, closest_dist);

        if let Some(mtv) = a_mtv {
            let mut cmoved = *a_a;
            let mut f_total = Vector2I::new(0, 0);

            let mut f = Vector2I::default();

            if closest_mtv_seg >= 0 {
                let cs = a_b.get_segment(closest_mtv_seg);
                let np = cs.nearest_point(a_a.get_center());
                f = (np - a_a.get_center()) + (np - a_a.get_center()).resize(a_a.get_radius());
            }

            cmoved.set_center(cmoved.get_center() + f);
            f_total += f;

            for s in 0..a_b.get_segment_count() {
                f = pushout_force(&cmoved, &a_b.get_segment(s as i32), a_clearance);
                cmoved.set_center(cmoved.get_center() + f);
                f_total += f;
            }

            *mtv = f_total;
        }

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_CIRCLE&, const SHAPE_SEGMENT&, ... )`.
fn collide_circle_segment(
    a_a: &ShapeCircle,
    a_seg: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_a.collide_seg(
        a_seg.get_seg(),
        a_clearance + a_seg.get_width() / 2,
        a_actual.as_deref_mut(),
        a_location,
    ) {
        set(
            &mut a_mtv,
            -pushout_force(a_a, a_seg.get_seg(), a_clearance + a_seg.get_width() / 2),
        );

        if let Some(a) = a_actual {
            *a = 0.max(*a - a_seg.get_width() / 2);
        }

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_RECT&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn collide_rect_lcb(
    a_a: &ShapeRect,
    a_b: &dyn ShapeLineChainBase,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_a.get_radius() > 0 {
        // Collide( aA.Outline(), aB, ... ): SHAPE_LINE_CHAIN_BASE against SHAPE_LINE_CHAIN_BASE
        let outline = a_a.outline();
        return unsupported_g5(ShapeRef::LineChain(&outline), a_b.as_shape_ref());
    }

    // "MTV not implemented for %s : %s collisions" (wxASSERT_MSG)
    let _ = a_mtv;

    let mut closest_dist: i32 = i32::MAX;
    let mut nearest = Vector2I::default();

    if a_b.is_closed() && a_b.point_inside(a_a.centre(), 0, false) {
        nearest = a_a.centre();
        closest_dist = 0;
    } else {
        let want_dist = a_actual.is_some() || a_location.is_some();
        let want_location = a_location.is_some();

        for s in 0..a_b.get_segment_count() {
            let mut collision_dist: i32 = 0;
            let mut pn = Vector2I::default();

            if a_a.collide_seg(
                &a_b.get_segment(s as i32),
                a_clearance,
                if want_dist {
                    Some(&mut collision_dist)
                } else {
                    None
                },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if collision_dist < closest_dist {
                    nearest = pn;
                    closest_dist = collision_dist;
                }

                if closest_dist == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if a_actual.is_none() {
                    break;
                }
            }
        }
    }

    if closest_dist == 0 || closest_dist < a_clearance {
        set(&mut a_location, nearest);
        set(&mut a_actual, closest_dist);

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_SEGMENT&, const SHAPE_SEGMENT&, ... )`.
fn collide_segment_segment(
    a_a: &ShapeSegment,
    a_b: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    let rv = a_a.collide_seg(
        a_b.get_seg(),
        a_clearance + a_b.get_width() / 2,
        a_actual.as_deref_mut(),
        a_location,
    );

    if rv {
        if let Some(a) = a_actual {
            *a = 0.max(*a - a_b.get_width() / 2);
        }
    }

    rv
}

/// Port of `Collide( const SHAPE_LINE_CHAIN_BASE&, const SHAPE_SEGMENT&, ... )`.
fn collide_lcb_segment(
    a_a: &dyn ShapeLineChainBase,
    a_b: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    let rv = a_a.collide_seg(
        a_b.get_seg(),
        a_clearance + a_b.get_width() / 2,
        a_actual.as_deref_mut(),
        a_location,
    );

    if rv {
        if let Some(a) = a_actual {
            *a = 0.max(*a - a_b.get_width() / 2);
        }
    }

    rv
}

/// Port of `Collide( const SHAPE_RECT&, const SHAPE_SEGMENT&, ... )`.
fn collide_rect_segment(
    a_a: &ShapeRect,
    a_b: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    if a_a.get_radius() > 0 {
        let outline = a_a.outline();
        return collide_lcb_segment(&outline, a_b, a_clearance, a_actual, a_location);
    }

    let rv = a_a.collide_seg(
        a_b.get_seg(),
        a_clearance + a_b.get_width() / 2,
        a_actual.as_deref_mut(),
        a_location,
    );

    if rv {
        if let Some(a) = a_actual {
            *a = 0.max(*a - a_b.get_width() / 2);
        }
    }

    rv
}

/// Port of `Collide( const SHAPE_RECT&, const SHAPE_RECT&, ... )`.
fn collide_rect_rect(
    a_a: &ShapeRect,
    a_b: &ShapeRect,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_clearance != 0
        || a_actual.is_some()
        || a_location.is_some()
        || a_mtv.is_some()
        || a_a.get_radius() > 0
        || a_b.get_radius() > 0
    {
        // Collide( aA.Outline(), aB.Outline(), ... ): SHAPE_LINE_CHAIN_BASE against
        // SHAPE_LINE_CHAIN_BASE
        unsupported_g5(ShapeRef::Rect(a_a), ShapeRef::Rect(a_b))
    } else {
        let bboxa = a_a.bbox(0);
        let bboxb = a_b.bbox(0);

        bboxa.intersects(&bboxb)
    }
}

/// The common tail of the arc-against-X collisions that use `SHAPE_ARC::NearestPoints()`.
fn arc_nearest_points_result(
    pt_a: Vector2I,
    pt_b: Vector2I,
    dist_sq: i64,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    if dist_sq == 0 || dist_sq < Seg::square(a_clearance) {
        set(&mut a_location, (pt_a + pt_b) / 2);
        set(&mut a_actual, 0.max(ki_round((dist_sq as f64).sqrt())));

        if let Some(mtv) = a_mtv {
            let delta = pt_b - pt_a;
            *mtv = delta.resize((a_clearance as f64 - (dist_sq as f64).sqrt() + 3.0) as i32);
        }

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_CIRCLE&, ... )`.
fn collide_arc_circle(
    a_a: &ShapeArc,
    a_b: &ShapeCircle,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_a.is_effective_line() {
        let tmp = ShapeSegment::new(a_a.get_p0(), a_a.get_p1(), a_a.get_width());
        let retval = collide_circle_segment(
            a_b,
            &tmp,
            a_clearance,
            a_actual,
            a_location,
            a_mtv.as_deref_mut(),
        );

        if retval {
            if let Some(m) = a_mtv {
                *m = -*m;
            }
        }

        return retval;
    }

    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq: i64 = i64::MAX;
    a_a.nearest_points_circle(a_b, &mut pt_a, &mut pt_b, &mut dist_sq);

    arc_nearest_points_result(
        pt_a,
        pt_b,
        dist_sq,
        a_clearance,
        a_actual,
        a_location,
        a_mtv,
    )
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_LINE_CHAIN&, ... )`.
fn collide_arc_line_chain(
    a_a: &ShapeArc,
    a_b: &ShapeLineChain,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for %s : %s collisions" (wxASSERT_MSG)

    let mut closest_dist: i32 = i32::MAX;
    let mut nearest = Vector2I::default();

    if a_b.is_closed() && a_b.point_inside(a_a.get_p0(), 0, false) {
        closest_dist = 0;
        nearest = a_a.get_p0();
    } else {
        let mut collision_dist: i32 = 0;
        let mut pn = Vector2I::default();
        let want_dist = a_actual.is_some() || a_location.is_some();
        let want_location = a_location.is_some();

        for i in 0..a_b.segment_count() as usize {
            // ignore arcs - we will collide these separately
            if a_b.is_arc_segment(i) {
                continue;
            }

            if a_a.collide_seg(
                &a_b.segment(i as i32),
                a_clearance,
                if want_dist {
                    Some(&mut collision_dist)
                } else {
                    None
                },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if collision_dist < closest_dist {
                    nearest = pn;
                    closest_dist = collision_dist;
                }

                if closest_dist == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if a_actual.is_none() {
                    break;
                }
            }
        }

        for i in 0..a_b.arc_count() {
            let arc = a_b.arc(i);

            // The arcs in the chain should have zero width
            debug_assert!(arc.get_width() == 0, "Invalid arc width - should be zero");

            if a_a.collide(
                arc,
                a_clearance,
                if want_dist {
                    Some(&mut collision_dist)
                } else {
                    None
                },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if collision_dist < closest_dist {
                    nearest = pn;
                    closest_dist = collision_dist;
                }

                if closest_dist == 0 {
                    break;
                }

                if a_actual.is_none() {
                    break;
                }
            }
        }
    }

    if closest_dist == 0 || closest_dist < a_clearance {
        set(&mut a_location, nearest);
        set(&mut a_actual, closest_dist);

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_RECT&, ... )`.
fn collide_arc_rect(
    a_a: &ShapeArc,
    a_b: &ShapeRect,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_b.get_radius() > 0 {
        let outline = a_b.outline();
        return collide_arc_line_chain(a_a, &outline, a_clearance, a_actual, a_location);
    }

    if a_a.is_effective_line() {
        let tmp = ShapeSegment::new(a_a.get_p0(), a_a.get_p1(), a_a.get_width());
        // Collide( SHAPE_RECT, SHAPE_SEGMENT ) never writes the MTV, but the C++ negates
        // whatever the caller passed in
        let retval = collide_rect_segment(a_b, &tmp, a_clearance, a_actual, a_location);
        return reversed(retval, a_mtv);
    }

    // aA.NearestPoints( aB, ptA, ptB, dist_sq ) defers to SHAPE::NearestPoints() (G5)
    unsupported_g5(ShapeRef::Arc(a_a), ShapeRef::Rect(a_b))
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_SEGMENT&, ... )`.
fn collide_arc_segment(
    a_a: &ShapeArc,
    a_b: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for %s : %s collisions" (wxASSERT_MSG)

    // If the arc radius is too large, it is effectively a line segment
    if a_a.is_effective_line() {
        let tmp = ShapeSegment::new(a_a.get_p0(), a_a.get_p1(), a_a.get_width());
        return collide_segment_segment(&tmp, a_b, a_clearance, a_actual, a_location);
    }

    let rv = a_a.collide_seg(
        a_b.get_seg(),
        a_clearance + a_b.get_width() / 2,
        a_actual.as_deref_mut(),
        a_location,
    );

    if rv {
        if let Some(a) = a_actual {
            *a = 0.max(*a - a_b.get_width() / 2);
        }
    }

    rv
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn collide_arc_lcb(
    a_a: &ShapeArc,
    a_b: &dyn ShapeLineChainBase,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    // If the arc radius is too large, it is effectively a line segment
    if a_a.is_effective_line() {
        let tmp = ShapeSegment::new(a_a.get_p0(), a_a.get_p1(), a_a.get_width());
        return collide_lcb_segment(a_b, &tmp, a_clearance, a_actual, a_location);
    }

    // "MTV not implemented for %s : %s collisions" (wxASSERT_MSG)

    let mut closest_dist: i32 = i32::MAX;
    let mut nearest = Vector2I::default();

    if a_b.is_closed() && a_b.point_inside(a_a.get_p0(), 0, false) {
        closest_dist = 0;
        nearest = a_a.get_p0();
    } else {
        let arc_bbox = a_a.bbox(a_clearance);
        let near_full_circle = (a_a.get_p0() - a_a.get_p1()).squared_euclidean_norm()
            < Seg::square(a_clearance)
            && a_a.get_central_angle().as_degrees() > 180.0;
        let want_dist = a_actual.is_some() || a_location.is_some();
        let want_location = a_location.is_some();

        for i in 0..a_b.get_segment_count() {
            let segment = a_b.get_segment(i as i32);

            // SHAPE_ARC::Collide( SEG ) uses the full circle for near-full arcs
            if !near_full_circle
                && (segment.a.x.max(segment.b.x) < arc_bbox.left()
                    || segment.a.x.min(segment.b.x) > arc_bbox.right()
                    || segment.a.y.max(segment.b.y) < arc_bbox.top()
                    || segment.a.y.min(segment.b.y) > arc_bbox.bottom())
            {
                continue;
            }

            let mut collision_dist: i32 = 0;
            let mut pn = Vector2I::default();

            if a_a.collide_seg(
                &segment,
                a_clearance,
                if want_dist {
                    Some(&mut collision_dist)
                } else {
                    None
                },
                if want_location { Some(&mut pn) } else { None },
            ) {
                if collision_dist < closest_dist {
                    nearest = pn;
                    closest_dist = collision_dist;
                }

                if closest_dist == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if a_actual.is_none() {
                    break;
                }
            }
        }
    }

    if closest_dist == 0 || closest_dist < a_clearance {
        set(&mut a_location, nearest);
        set(&mut a_actual, closest_dist);

        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ARC&, const SHAPE_ARC&, ... )`.
fn collide_arc_arc(
    a_a: &ShapeArc,
    a_b: &ShapeArc,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    if a_a.is_effective_line() {
        let tmp = ShapeSegment::new(a_a.get_p0(), a_a.get_p1(), a_a.get_width());
        // Collide( SHAPE_ARC, SHAPE_SEGMENT ) never writes the MTV, but the C++ negates
        // whatever the caller passed in
        let retval = collide_arc_segment(a_b, &tmp, a_clearance, a_actual, a_location);

        if retval {
            if let Some(m) = a_mtv.as_deref_mut() {
                *m = -*m;
            }
        }

        return retval;
    }

    if a_b.is_effective_line() {
        let tmp = ShapeSegment::new(a_b.get_p0(), a_b.get_p1(), a_b.get_width());
        return collide_arc_segment(a_a, &tmp, a_clearance, a_actual, a_location);
    }

    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq: i64 = i64::MAX;
    a_a.nearest_points_arc(a_b, &mut pt_a, &mut pt_b, &mut dist_sq);

    arc_nearest_points_result(
        pt_a,
        pt_b,
        dist_sq,
        a_clearance,
        a_actual,
        a_location,
        a_mtv,
    )
}

/// Negates the MTV of a reversed collision (`CollCaseReversed`).
fn reversed(rv: bool, a_mtv: Option<&mut Vector2I>) -> bool {
    if rv {
        if let Some(m) = a_mtv {
            *m = -*m;
        }
    }

    rv
}

/// Port of the static `collideSingleShapes()`: the collision matrix for two non-compound
/// shapes.
fn collide_single_shapes(
    a_a: ShapeRef<'_>,
    a_b: ShapeRef<'_>,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    use ShapeRef as S;

    if let S::PolySet(_) = a_a {
        // polySetA->Collide( aB, aClearance, aActual, aLocation ) (G4/G5)
        return unsupported_g5(a_a, a_b);
    } else if let S::PolySet(_) = a_b {
        // polySetB->Collide( aA, aClearance, aActual, aLocation ) (G4/G5)
        return unsupported_g5(a_a, a_b);
    }

    let (cl, act, loc) = (a_clearance, a_actual, a_location);

    match (a_a, a_b) {
        (S::Null(_), _) => false,
        (_, S::Null(_)) => false,

        (S::Rect(a), S::Rect(b)) => collide_rect_rect(a, b, cl, act, loc, a_mtv),
        (S::Rect(a), S::Circle(b)) => collide_rect_circle(a, b, cl, act, loc, a_mtv),
        (S::Rect(a), S::LineChain(b)) => collide_rect_lcb(a, b, cl, act, loc, a_mtv),
        (S::Rect(a), S::Segment(b)) => collide_rect_segment(a, b, cl, act, loc),
        (S::Rect(a), S::Simple(b)) => collide_rect_lcb(a, b, cl, act, loc, a_mtv),
        (S::Rect(a), S::PolySetTriangle(b)) => collide_rect_lcb(a, b, cl, act, loc, a_mtv),
        (S::Rect(a), S::Arc(b)) => reversed(
            collide_arc_rect(b, a, cl, act, loc, a_mtv.as_deref_mut()),
            a_mtv,
        ),

        (S::Circle(a), S::Rect(b)) => reversed(
            collide_rect_circle(b, a, cl, act, loc, a_mtv.as_deref_mut()),
            a_mtv,
        ),
        (S::Circle(a), S::Circle(b)) => collide_circle_circle(a, b, cl, act, loc, a_mtv),
        (S::Circle(a), S::LineChain(b)) => collide_circle_lcb(a, b, cl, act, loc, a_mtv),
        (S::Circle(a), S::Segment(b)) => collide_circle_segment(a, b, cl, act, loc, a_mtv),
        (S::Circle(a), S::Simple(b)) => collide_circle_lcb(a, b, cl, act, loc, a_mtv),
        (S::Circle(a), S::PolySetTriangle(b)) => collide_circle_lcb(a, b, cl, act, loc, a_mtv),
        (S::Circle(a), S::Arc(b)) => reversed(
            collide_arc_circle(b, a, cl, act, loc, a_mtv.as_deref_mut()),
            a_mtv,
        ),

        (S::LineChain(a), S::Rect(b)) => collide_rect_lcb(b, a, cl, act, loc, a_mtv),
        (S::LineChain(a), S::Circle(b)) => collide_circle_lcb(b, a, cl, act, loc, a_mtv),
        (S::LineChain(a), S::Segment(b)) => collide_lcb_segment(a, b, cl, act, loc),
        (S::LineChain(a), S::Arc(b)) => collide_arc_line_chain(b, a, cl, act, loc),

        (S::Segment(a), S::Rect(b)) => collide_rect_segment(b, a, cl, act, loc),
        (S::Segment(a), S::Circle(b)) => reversed(
            collide_circle_segment(b, a, cl, act, loc, a_mtv.as_deref_mut()),
            a_mtv,
        ),
        (S::Segment(a), S::LineChain(b)) => collide_lcb_segment(b, a, cl, act, loc),
        (S::Segment(a), S::Segment(b)) => collide_segment_segment(a, b, cl, act, loc),
        (S::Segment(a), S::Simple(b)) => collide_lcb_segment(b, a, cl, act, loc),
        (S::Segment(a), S::PolySetTriangle(b)) => collide_lcb_segment(b, a, cl, act, loc),
        (S::Segment(a), S::Arc(b)) => collide_arc_segment(b, a, cl, act, loc),

        (S::Simple(a), S::Rect(b)) => collide_rect_lcb(b, a, cl, act, loc, a_mtv),
        (S::PolySetTriangle(a), S::Rect(b)) => collide_rect_lcb(b, a, cl, act, loc, a_mtv),
        (S::Simple(a), S::Circle(b)) => collide_circle_lcb(b, a, cl, act, loc, a_mtv),
        (S::PolySetTriangle(a), S::Circle(b)) => collide_circle_lcb(b, a, cl, act, loc, a_mtv),
        (S::Simple(a), S::Segment(b)) => collide_lcb_segment(a, b, cl, act, loc),
        (S::PolySetTriangle(a), S::Segment(b)) => collide_lcb_segment(a, b, cl, act, loc),
        (S::Simple(a), S::Arc(b)) => collide_arc_lcb(b, a, cl, act, loc),
        (S::PolySetTriangle(a), S::Arc(b)) => collide_arc_lcb(b, a, cl, act, loc),

        (S::Arc(a), S::Rect(b)) => collide_arc_rect(a, b, cl, act, loc, a_mtv),
        (S::Arc(a), S::Circle(b)) => collide_arc_circle(a, b, cl, act, loc, a_mtv),
        (S::Arc(a), S::LineChain(b)) => collide_arc_line_chain(a, b, cl, act, loc),
        (S::Arc(a), S::Segment(b)) => collide_arc_segment(a, b, cl, act, loc),
        (S::Arc(a), S::Simple(b)) => collide_arc_lcb(a, b, cl, act, loc),
        (S::Arc(a), S::PolySetTriangle(b)) => collide_arc_lcb(a, b, cl, act, loc),
        (S::Arc(a), S::Arc(b)) => collide_arc_arc(a, b, cl, act, loc, a_mtv),

        // Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE ) (G5)
        (
            S::LineChain(_) | S::Simple(_) | S::PolySetTriangle(_),
            S::LineChain(_) | S::Simple(_) | S::PolySetTriangle(_),
        ) => unsupported_g5(a_a, a_b),

        // Compounds within compounds and the poly sets handled above: the C++ matrix has no
        // case for them ("Unsupported collision", wxFAIL_MSG) and returns false
        (S::Compound(_), _) | (_, S::Compound(_)) | (S::PolySet(_), _) | (_, S::PolySet(_)) => {
            false
        }
    }
}

/// Port of the static `collideShapes()`: collisions of compound shapes are resolved here, the
/// rest goes to the collision matrix.
pub fn collide_shapes(
    a_a: ShapeRef<'_>,
    a_b: ShapeRef<'_>,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
    mut a_mtv: Option<&mut Vector2I>,
) -> bool {
    let mut current_actual: i32 = i32::MAX;
    let mut current_location = Vector2I::default();
    let mut current_mtv = Vector2I::new(0, 0);
    let mut colliding = false;

    let want_actual = a_actual.is_some();
    let want_location = a_location.is_some();
    let want_mtv = a_mtv.is_some();

    let can_exit = |colliding: bool, current_actual: i32| -> bool {
        if !colliding {
            return false;
        }

        if want_actual && current_actual > 0 {
            return false;
        }

        if want_mtv {
            return false;
        }

        true
    };

    let collide_compound_subshapes = |elem_a: &dyn Shape,
                                      elem_b: &dyn Shape,
                                      clearance: i32,
                                      current_actual: &mut i32,
                                      current_location: &mut Vector2I,
                                      current_mtv: &mut Vector2I|
     -> bool {
        let mut actual: i32 = 0;
        let mut location = Vector2I::default();
        let mut mtv = Vector2I::default();

        if collide_single_shapes(
            elem_a.as_shape_ref(),
            elem_b.as_shape_ref(),
            clearance,
            if want_actual || want_location {
                Some(&mut actual)
            } else {
                None
            },
            if want_location {
                Some(&mut location)
            } else {
                None
            },
            if want_mtv { Some(&mut mtv) } else { None },
        ) {
            if actual < *current_actual {
                *current_actual = actual;
                *current_location = location;
            }

            if want_mtv && mtv.squared_euclidean_norm() > current_mtv.squared_euclidean_norm() {
                *current_mtv = mtv;
            }

            return true;
        }

        false
    };

    match (a_a, a_b) {
        (ShapeRef::Compound(cmp_a), ShapeRef::Compound(cmp_b)) => {
            for elem_a in cmp_a.shapes() {
                for elem_b in cmp_b.shapes() {
                    if collide_compound_subshapes(
                        elem_a.as_ref(),
                        elem_b.as_ref(),
                        a_clearance,
                        &mut current_actual,
                        &mut current_location,
                        &mut current_mtv,
                    ) {
                        colliding = true;

                        if can_exit(colliding, current_actual) {
                            break;
                        }
                    }
                }

                if can_exit(colliding, current_actual) {
                    break;
                }
            }
        }
        (ShapeRef::Compound(cmp_a), _) => {
            for elem_a in cmp_a.shapes() {
                if collide_compound_subshapes(
                    elem_a.as_ref(),
                    a_b.as_dyn(),
                    a_clearance,
                    &mut current_actual,
                    &mut current_location,
                    &mut current_mtv,
                ) {
                    colliding = true;

                    if can_exit(colliding, current_actual) {
                        break;
                    }
                }
            }
        }
        (_, ShapeRef::Compound(cmp_b)) => {
            for elem_b in cmp_b.shapes() {
                if collide_compound_subshapes(
                    a_a.as_dyn(),
                    elem_b.as_ref(),
                    a_clearance,
                    &mut current_actual,
                    &mut current_location,
                    &mut current_mtv,
                ) {
                    colliding = true;

                    if can_exit(colliding, current_actual) {
                        break;
                    }
                }
            }
        }
        _ => {
            return collide_single_shapes(a_a, a_b, a_clearance, a_actual, a_location, a_mtv);
        }
    }

    if colliding {
        set(&mut a_location, current_location);
        set(&mut a_actual, current_actual);
        set(&mut a_mtv, current_mtv);
    }

    colliding
}

/// Entry point of `SHAPE::NearestPoints( const SHAPE* aOther, VECTOR2I& aPtThis,
/// VECTOR2I& aPtOther )` (`shape_nearest_points.cpp`).
///
/// Package G5: not ported yet.  Returns false ("the points were not found") and leaves the
/// points unchanged.
pub fn nearest_points(
    _a_a: ShapeRef<'_>,
    _a_b: ShapeRef<'_>,
    _a_pt_this: &mut Vector2I,
    _a_pt_other: &mut Vector2I,
) -> bool {
    false
}
