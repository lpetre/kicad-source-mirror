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

//! Port of `libs/kimath/src/geometry/shape_collisions.cpp`: the collision matrix behind
//! `SHAPE::Collide( const SHAPE*, ... )` (every `Collide( A, B )` pair function, the dispatch
//! `collideSingleShapes()` and the compound iteration `collideShapes()`), and the entry point of
//! `libs/kimath/src/geometry/shape_nearest_points.cpp` ([`nearest_points`], implemented in
//! [`crate::geometry::shape_nearest_points`]).
//!
//! [`collide_shapes`] is the single dispatch function.  The C++ overload resolution of the
//! `CollCase` / `CollCaseReversed` templates is spelled out in [`collide_single_shapes`]: e.g.
//! `CollCase<SHAPE_RECT, SHAPE_LINE_CHAIN>` resolves to `Collide( const SHAPE_RECT&,
//! const SHAPE_LINE_CHAIN_BASE& )` while `CollCase<SHAPE_ARC, SHAPE_LINE_CHAIN>` has its own
//! overload.  Poly sets go through `SHAPE_POLY_SET::Collide( const SHAPE* )`
//! ([`crate::geometry::shape_poly_set::ShapePolySet::collide_shape`]) as in C++.

use crate::geometry::seg::Ecoord;
use crate::geometry::seg::Seg;
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef};
use crate::geometry::shape_arc::ShapeArc;
use crate::cmath::f64_to_i32_x86;
use crate::geometry::segment_index::SegmentIndex;
use crate::geometry::shape_circle::ShapeCircle;
use crate::geometry::shape_ellipse::ShapeEllipse;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_segment::ShapeSegment;
use crate::math::util::ki_round;
use crate::stdsort::std_sort;
use crate::math::vector2::Vector2I;

/// Writes `v` through an optional out-parameter.
fn set<T>(a_out: &mut Option<&mut T>, v: T) {
    if let Some(o) = a_out.as_deref_mut() {
        *o = v;
    }
}

/// The C++ fallback for a pair the collision matrix does not know ("Unsupported collision",
/// `wxFAIL_MSG`): returns false.  Since package G5 the matrix is complete and only compounds
/// nested in compounds end here.
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
        return collide_lcb_lcb(&outline, a_b, a_clearance, a_actual, a_location);
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
        // SHAPE_LINE_CHAIN_BASE ("MTV not implemented", wxASSERT_MSG)
        let _ = a_mtv;
        let outline_a = a_a.outline();
        let outline_b = a_b.outline();
        collide_lcb_lcb(&outline_a, &outline_b, a_clearance, a_actual, a_location)
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

    let mut pt_a = Vector2I::default();
    let mut pt_b = Vector2I::default();
    let mut dist_sq: i64 = i64::MAX;
    a_a.nearest_points_rect(a_b, &mut pt_a, &mut pt_b, &mut dist_sq);

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

/// `dynamic_cast<const SHAPE_LINE_CHAIN*>( &aChain )`.
fn as_line_chain(a_chain: &dyn ShapeLineChainBase) -> Option<&ShapeLineChain> {
    match a_chain.as_shape_ref() {
        ShapeRef::LineChain(c) => Some(c),
        _ => None,
    }
}

/// The `dense_candidates` lambda of `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )`:
/// whether the first query segment's bounds, inflated by the clearance, cover all indexed
/// segments (an index would then prune nothing).
fn dense_candidates(a_queries: &[Seg], a_indexed: &[Seg], a_clearance: i32) -> bool {
    let mut indexed_min_x = i32::MAX;
    let mut indexed_min_y = i32::MAX;
    let mut indexed_max_x = i32::MIN;
    let mut indexed_max_y = i32::MIN;

    for segment in a_indexed {
        indexed_min_x = indexed_min_x.min(segment.a.x).min(segment.b.x);
        indexed_min_y = indexed_min_y.min(segment.a.y).min(segment.b.y);
        indexed_max_x = indexed_max_x.max(segment.a.x).max(segment.b.x);
        indexed_max_y = indexed_max_y.max(segment.a.y).max(segment.b.y);
    }

    let query = &a_queries[0];
    let min_x: i64 = query.a.x.min(query.b.x) as i64 - a_clearance as i64;
    let min_y: i64 = query.a.y.min(query.b.y) as i64 - a_clearance as i64;
    let max_x: i64 = query.a.x.max(query.b.x) as i64 + a_clearance as i64;
    let max_y: i64 = query.a.y.max(query.b.y) as i64 + a_clearance as i64;

    min_x <= indexed_min_x as i64
        && min_y <= indexed_min_y as i64
        && max_x >= indexed_max_x as i64
        && max_y >= indexed_max_y as i64
}

/// Port of `Collide( const SHAPE_LINE_CHAIN_BASE&, const SHAPE_LINE_CHAIN_BASE&, ... )`
/// ("MTV not implemented", `wxASSERT_MSG`).
///
/// The straight segments of both shapes are compared pairwise; above the measured crossovers
/// of the C++ a `SEGMENT_INDEX` prunes the candidates (same visit order and the same tie
/// handling as the C++, so the reported location is identical).  The arcs of line chains are
/// then collided separately through `SHAPE_ARC::Collide( const SHAPE* )`.
pub fn collide_lcb_lcb(
    a_a: &dyn ShapeLineChainBase,
    a_b: &dyn ShapeLineChainBase,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    let mut closest_dist: i32 = i32::MAX;
    let mut nearest = Vector2I::default();

    if a_b.is_closed() && a_a.get_point_count() > 0 && a_b.point_inside(a_a.get_point(0), 0, false)
    {
        closest_dist = 0;
        nearest = a_a.get_point(0);
    } else if a_a.is_closed()
        && a_b.get_point_count() > 0
        && a_a.point_inside(a_b.get_point(0), 0, false)
    {
        closest_dist = 0;
        nearest = a_b.get_point(0);
    } else {
        let mut a_segs: Vec<Seg> = Vec::new();
        let mut b_segs: Vec<Seg> = Vec::new();

        let chain_a = as_line_chain(a_a);
        let chain_b = as_line_chain(a_b);

        for ii in 0..a_a.get_segment_count() {
            if !chain_a.is_some_and(|c| c.is_arc_segment(ii)) {
                a_segs.push(a_a.get_segment(ii as i32));
            }
        }

        for ii in 0..a_b.get_segment_count() {
            if !chain_b.is_some_and(|c| c.is_arc_segment(ii)) {
                b_segs.push(a_b.get_segment(ii as i32));
            }
        }

        // Index gates below are measured crossovers rather than correctness requirements, and
        // the dense checks bound index build cost where nearly every pair is a candidate anyway
        let need_output = a_actual.is_some() || a_location.is_some();
        let pair_count: i64 = a_segs.len() as i64 * b_segs.len() as i64;

        if !need_output {
            let mut use_index = a_clearance >= 0
                && a_segs.len().min(b_segs.len()) >= 64
                && pair_count >= 4096;

            if use_index {
                let (queries, indexed) = if b_segs.len() >= a_segs.len() {
                    (&a_segs, &b_segs)
                } else {
                    (&b_segs, &a_segs)
                };
                use_index = !dense_candidates(queries, indexed, a_clearance);
            }

            if use_index {
                // Bounded probe answers the common early hit without paying for index
                // construction
                let probe_a_count = a_segs.len().min(4);
                let probe_b_count = b_segs.len().min(8);
                let mut early_collision = false;

                let mut i = 0;

                while i < probe_a_count && !early_collision {
                    for j in 0..probe_b_count {
                        if a_segs[i].collide(&b_segs[j], a_clearance) {
                            early_collision = true;
                            break;
                        }
                    }

                    i += 1;
                }

                if early_collision {
                    return true;
                } else if b_segs.len() >= a_segs.len() {
                    let index = SegmentIndex::new(std::mem::take(&mut b_segs));

                    for a_seg in &a_segs {
                        let mut found = false;
                        index.visit_candidates(a_seg, a_clearance, |a_index| {
                            if a_seg.collide(index.segment(a_index), a_clearance) {
                                found = true;
                                return false;
                            }

                            true
                        });

                        if found {
                            return true;
                        }
                    }
                } else {
                    let index = SegmentIndex::new(std::mem::take(&mut a_segs));

                    for b_seg in &b_segs {
                        let mut found = false;
                        index.visit_candidates(b_seg, a_clearance, |a_index| {
                            if index.segment(a_index).collide(b_seg, a_clearance) {
                                found = true;
                                return false;
                            }

                            true
                        });

                        if found {
                            return true;
                        }
                    }
                }
            } else if a_clearance >= 0 {
                for a_seg in &a_segs {
                    for b_seg in &b_segs {
                        if a_seg.collide(b_seg, a_clearance) {
                            return true;
                        }
                    }
                }
            }
        } else {
            let seg_sort = |a: &Seg, b: &Seg| a.a.x < b.a.x || (a.a.x == b.a.x && a.a.y < b.a.y);

            std_sort(&mut a_segs, seg_sort);
            std_sort(&mut b_segs, seg_sort);

            let use_index = a_clearance >= 0
                && a_segs.len() >= 32
                && b_segs.len() >= 64
                && pair_count >= 4096
                && !dense_candidates(&a_segs, &b_segs, a_clearance);

            if use_index {
                let index = SegmentIndex::new(std::mem::take(&mut b_segs));
                let mut candidates: Vec<i32> = Vec::new();
                let mut scan_direct = false;

                #[derive(PartialEq)]
                enum CollisionResult {
                    None,
                    Found,
                    Exact,
                }

                let collide_pair = |a_a_seg: &Seg,
                                        a_b_seg: &Seg,
                                        closest_dist: &mut i32,
                                        nearest: &mut Vector2I|
                 -> CollisionResult {
                    let (collides, dist) = a_a_seg.collide_actual(a_b_seg, a_clearance);

                    if !collides {
                        return CollisionResult::None;
                    }

                    if dist < *closest_dist {
                        *nearest = a_a_seg.nearest_point_seg(a_b_seg);
                        *closest_dist = dist;
                    }

                    if *closest_dist == 0 {
                        CollisionResult::Exact
                    } else {
                        CollisionResult::Found
                    }
                };

                for a_seg in &a_segs {
                    if !scan_direct {
                        candidates.clear();
                        index.visit_candidates(a_seg, a_clearance, |a_index| {
                            candidates.push(a_index);
                            true
                        });
                        // Candidates pruning less than half the index cost more than a direct
                        // scan
                        scan_direct = candidates.len() * 2 >= index.size();
                    }

                    // Candidate IDs restore the legacy order that determines tied witness
                    // locations
                    if !scan_direct {
                        candidates.sort_unstable();
                    }

                    let candidate_count = if scan_direct {
                        index.size()
                    } else {
                        candidates.len()
                    };

                    for i in 0..candidate_count {
                        let b_index: i32 = if scan_direct { i as i32 } else { candidates[i] };
                        let result = collide_pair(
                            a_seg,
                            index.segment(b_index),
                            &mut closest_dist,
                            &mut nearest,
                        );

                        if result == CollisionResult::Exact {
                            set(&mut a_location, nearest);
                            set(&mut a_actual, closest_dist);

                            return true;
                        }

                        if result == CollisionResult::Found && a_actual.is_none() {
                            break;
                        }
                    }
                }
            } else if a_clearance >= 0 {
                for a_seg in &a_segs {
                    for b_seg in &b_segs {
                        let (collides, dist) = a_seg.collide_actual(b_seg, a_clearance);

                        if collides {
                            if dist < closest_dist {
                                nearest = a_seg.nearest_point_seg(b_seg);
                                closest_dist = dist;
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
            }
        }
    }

    if (a_actual.is_none() && a_location.is_none()) || closest_dist > 0 {
        let chains = [as_line_chain(a_a), as_line_chain(a_b)];
        let shapes: [&dyn ShapeLineChainBase; 2] = [a_a, a_b];

        for ii in 0..2 {
            let Some(chain) = chains[ii] else {
                continue;
            };
            let other = shapes[(ii + 1) % 2];

            for jj in 0..chain.arc_count() {
                let arc = chain.arc(jj);

                if arc.collide(
                    other.as_shape_ref().as_dyn(),
                    a_clearance,
                    a_actual.as_deref_mut(),
                    a_location.as_deref_mut(),
                ) {
                    return true;
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

/// Port of the template `collideEllipseVsSegments()`: an ellipse against the segments of a
/// segment source (`aSegSource.GetSegmentCount()` / `GetSegment()`), with a containment test
/// as the fallback.
fn collide_ellipse_vs_segments(
    a_a: &ShapeEllipse,
    a_seg_source: &dyn ShapeLineChainBase,
    a_clearance: i32,
    a_half_width: i32,
    a_containment: impl FnOnce() -> bool,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    let eff_clear: i32 = a_clearance + a_half_width;

    let mut found = false;
    let mut best_actual: i32 = i32::MAX;
    let mut best_location = Vector2I::default();

    let n_segs = a_seg_source.get_segment_count();

    for i in 0..n_segs {
        let seg = a_seg_source.get_segment(i as i32);
        let mut local_actual: i32 = 0;
        let mut local_loc = Vector2I::default();

        if a_a.collide_seg(
            &seg,
            eff_clear,
            if a_actual.is_some() {
                Some(&mut local_actual)
            } else {
                None
            },
            if a_location.is_some() {
                Some(&mut local_loc)
            } else {
                None
            },
        ) {
            if a_actual.is_none() && a_location.is_none() {
                return true;
            }

            if !found || local_actual < best_actual {
                best_actual = local_actual;
                best_location = local_loc;
                found = true;
            }
        }
    }

    if found {
        set(&mut a_actual, 0.max(best_actual - a_half_width));
        set(&mut a_location, best_location);
        return true;
    }

    if a_containment() {
        set(&mut a_actual, 0);
        set(&mut a_location, a_a.get_center());
        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_SEGMENT&, ... )`.
fn collide_ellipse_segment(
    a_a: &ShapeEllipse,
    a_seg: &ShapeSegment,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    let half_width: i32 = a_seg.get_width() / 2;
    let eff_clear: i32 = a_clearance + half_width;

    let mut local_actual: i32 = 0;
    let mut local_loc = Vector2I::default();

    let want_actual = a_actual.is_some();
    let want_location = a_location.is_some();

    if a_a.collide_seg(
        a_seg.get_seg(),
        eff_clear,
        if want_actual {
            Some(&mut local_actual)
        } else {
            None
        },
        if want_location {
            Some(&mut local_loc)
        } else {
            None
        },
    ) {
        set(&mut a_actual, 0.max(local_actual - half_width));
        set(&mut a_location, local_loc);
        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_CIRCLE&, ... )`.
fn collide_ellipse_circle(
    a_a: &ShapeEllipse,
    a_b: &ShapeCircle,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    let c = a_b.get_center();
    let r: i32 = a_b.get_radius();
    let eff_clr: i32 = a_clearance + r;
    let d_sq: Ecoord = a_a.squared_distance(c, false);
    let eff_sq: Ecoord = eff_clr as Ecoord * eff_clr as Ecoord;

    if d_sq == 0 || d_sq < eff_sq {
        if let Some(a) = a_actual.as_deref_mut() {
            let d: i32 = f64_to_i32_x86((d_sq as f64).sqrt().round());
            *a = 0.max(d - r);
        }

        set(&mut a_location, c);
        return true;
    }

    false
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_RECT&, ... )`.
fn collide_ellipse_rect(
    a_a: &ShapeEllipse,
    a_b: &ShapeRect,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    let outline = a_b.outline();

    collide_ellipse_vs_segments(
        a_a,
        &outline,
        a_clearance,
        0,
        || a_b.bbox(0).contains(a_a.get_center()),
        a_actual,
        a_location,
    )
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_LINE_CHAIN_BASE&, ... )`.
fn collide_ellipse_lcb(
    a_a: &ShapeEllipse,
    a_b: &dyn ShapeLineChainBase,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    collide_ellipse_vs_segments(
        a_a,
        a_b,
        a_clearance,
        0,
        || {
            a_b.is_closed()
                && a_b.get_segment_count() > 0
                && a_b.point_inside(a_a.get_center(), 0, false)
        },
        a_actual,
        a_location,
    )
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_ARC&, ... )`.
fn collide_ellipse_arc(
    a_a: &ShapeEllipse,
    a_b: &ShapeArc,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    let half_width: i32 = a_b.get_width() / 2;
    let eff_clear: i32 = a_clearance + half_width;
    let tess_error: i32 = 1.max(eff_clear / 4);
    let chain = a_b.convert_to_polyline(tess_error);

    collide_ellipse_vs_segments(
        a_a,
        &chain,
        a_clearance,
        half_width,
        || false,
        a_actual,
        a_location,
    )
}

/// Port of `Collide( const SHAPE_ELLIPSE&, const SHAPE_ELLIPSE&, ... )`.
fn collide_ellipse_ellipse(
    a_a: &ShapeEllipse,
    a_b: &ShapeEllipse,
    a_clearance: i32,
    mut a_actual: Option<&mut i32>,
    mut a_location: Option<&mut Vector2I>,
) -> bool {
    // "MTV not implemented for SHAPE_ELLIPSE collisions" (wxASSERT_MSG)
    if !a_a.is_arc() && a_a.point_inside(a_b.get_center(), 0, false) {
        set(&mut a_actual, 0);
        set(&mut a_location, a_b.get_center());
        return true;
    }

    if !a_b.is_arc() && a_b.point_inside(a_a.get_center(), 0, false) {
        set(&mut a_actual, 0);
        set(&mut a_location, a_a.get_center());
        return true;
    }

    let tess_error: i32 = 1.max(a_clearance / 4);
    let chain_b = a_b.convert_to_polyline(tess_error);

    collide_ellipse_lcb(a_a, &chain_b, a_clearance, a_actual, a_location)
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

    if let S::PolySet(poly_set_a) = a_a {
        // wxASSERT( !aMTV )
        return poly_set_a.collide_shape(a_b.as_dyn(), a_clearance, a_actual, a_location);
    } else if let S::PolySet(poly_set_b) = a_b {
        // wxASSERT( !aMTV )
        return poly_set_b.collide_shape(a_a.as_dyn(), a_clearance, a_actual, a_location);
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

        // CollCase<SHAPE_LINE_CHAIN, SHAPE_LINE_CHAIN( _BASE )>( aA, aB )
        (S::LineChain(a), S::LineChain(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        (S::LineChain(a), S::Simple(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        (S::LineChain(a), S::PolySetTriangle(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        // CollCase<SHAPE_LINE_CHAIN, SHAPE_LINE_CHAIN_BASE>( aB, aA )
        (S::Simple(a), S::LineChain(b)) => collide_lcb_lcb(b, a, cl, act, loc),
        (S::PolySetTriangle(a), S::LineChain(b)) => collide_lcb_lcb(b, a, cl, act, loc),
        // CollCase<SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE>( aA, aB )
        (S::Simple(a), S::Simple(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        (S::Simple(a), S::PolySetTriangle(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        (S::PolySetTriangle(a), S::Simple(b)) => collide_lcb_lcb(a, b, cl, act, loc),
        (S::PolySetTriangle(a), S::PolySetTriangle(b)) => collide_lcb_lcb(a, b, cl, act, loc),

        // SH_ELLIPSE pairs: CollCase<SHAPE_ELLIPSE, X> and CollCaseReversed<X, SHAPE_ELLIPSE>
        (S::Ellipse(a), S::Rect(b)) => collide_ellipse_rect(a, b, cl, act, loc),
        (S::Ellipse(a), S::Circle(b)) => collide_ellipse_circle(a, b, cl, act, loc),
        (S::Ellipse(a), S::LineChain(b)) => collide_ellipse_lcb(a, b, cl, act, loc),
        (S::Ellipse(a), S::Segment(b)) => collide_ellipse_segment(a, b, cl, act, loc),
        (S::Ellipse(a), S::Simple(b)) => collide_ellipse_lcb(a, b, cl, act, loc),
        (S::Ellipse(a), S::PolySetTriangle(b)) => collide_ellipse_lcb(a, b, cl, act, loc),
        (S::Ellipse(a), S::Arc(b)) => collide_ellipse_arc(a, b, cl, act, loc),
        (S::Ellipse(a), S::Ellipse(b)) => collide_ellipse_ellipse(a, b, cl, act, loc),
        (S::Rect(a), S::Ellipse(b)) => reversed(collide_ellipse_rect(b, a, cl, act, loc), a_mtv),
        (S::Circle(a), S::Ellipse(b)) => {
            reversed(collide_ellipse_circle(b, a, cl, act, loc), a_mtv)
        }
        (S::LineChain(a), S::Ellipse(b)) => {
            reversed(collide_ellipse_lcb(b, a, cl, act, loc), a_mtv)
        }
        (S::Segment(a), S::Ellipse(b)) => {
            reversed(collide_ellipse_segment(b, a, cl, act, loc), a_mtv)
        }
        (S::Simple(a), S::Ellipse(b)) => reversed(collide_ellipse_lcb(b, a, cl, act, loc), a_mtv),
        (S::PolySetTriangle(a), S::Ellipse(b)) => {
            reversed(collide_ellipse_lcb(b, a, cl, act, loc), a_mtv)
        }
        (S::Arc(a), S::Ellipse(b)) => reversed(collide_ellipse_arc(b, a, cl, act, loc), a_mtv),

        // Compounds within compounds and the poly sets handled above: the C++ matrix has no
        // case for them ("Unsupported collision", wxFAIL_MSG) and returns false
        (S::Compound(_), _) | (_, S::Compound(_)) | (S::PolySet(_), _) | (_, S::PolySet(_)) => {
            unsupported_g5(a_a, a_b)
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

/// Port of `SHAPE::NearestPoints( const SHAPE* aOther, VECTOR2I& aPtThis,
/// VECTOR2I& aPtOther )` (`shape_nearest_points.cpp`): the nearest points between two shapes,
/// see [`crate::geometry::shape_nearest_points::nearest_points`].  Returns false (the points
/// are then untouched) when the pair has no nearest points.
pub fn nearest_points(
    a_a: ShapeRef<'_>,
    a_b: ShapeRef<'_>,
    a_pt_this: &mut Vector2I,
    a_pt_other: &mut Vector2I,
) -> bool {
    crate::geometry::shape_nearest_points::nearest_points(a_a, a_b, a_pt_this, a_pt_other)
}
