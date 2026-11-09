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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_ellipse.cpp` (suite `ShapeEllipse`).
//!
//! The randomised tests use the libstdc++ engine / distribution replicas of `test_utils` and
//! draw in GCC's argument evaluation order (right to left for constructor calls), so they run
//! on the reference build's data.  The tests of `SHAPE::Collide( SHAPE* )` against other shapes
//! use the `SH_ELLIPSE` entries of the collision matrix (package G5).

use std::f64::consts::PI;

use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::eda_angle::{ANGLE_0, ANGLE_90, DEGREES_T, EdaAngle};
use kicad_geom::geometry::intersection::{IntersectableGeom, IntersectionVisitor};
use kicad_geom::geometry::seg::{Ecoord, Seg};
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::test_utils::{
    Mt19937, is_close_pct, uniform_int_libstdcxx, uniform_real_libstdcxx,
};

fn pt(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
}

fn ell(c: Vector2I, a: i32, b: i32, r: f64) -> ShapeEllipse {
    ShapeEllipse::new(c, a, b, deg(r))
}

fn ell_arc(c: Vector2I, a: i32, b: i32, r: f64, s: f64, e: f64) -> ShapeEllipse {
    ShapeEllipse::new_arc(c, a, b, deg(r), deg(s), deg(e))
}

fn uni(rng: &mut Mt19937, a: i32, b: i32) -> i32 {
    uniform_int_libstdcxx(rng, a as i64, b as i64) as i32
}

/// `VECTOR2I( dist( rng ), dist( rng ) )`: GCC evaluates the arguments right to left.
fn rand_pt(rng: &mut Mt19937, a: i32, b: i32) -> Vector2I {
    let y = uni(rng, a, b);
    let x = uni(rng, a, b);
    pt(x, y)
}

/// Port of `bruteForceEllipseBBox()`: a bounding box from sampled points (ground truth).
#[allow(clippy::too_many_arguments)]
fn brute_force_ellipse_bbox(
    a_center: Vector2I,
    a_major_r: i32,
    a_minor_r: i32,
    a_rotation: EdaAngle,
    a_start_angle: EdaAngle,
    a_end_angle: EdaAngle,
    a_is_arc: bool,
    a_n_samples: i32,
) -> Box2I {
    let a = a_major_r as f64;
    let b = a_minor_r as f64;
    let cp = a_rotation.as_radians().cos();
    let sp = a_rotation.as_radians().sin();

    let t_start = if a_is_arc {
        a_start_angle.as_radians()
    } else {
        0.0
    };
    let mut t_end = if a_is_arc {
        a_end_angle.as_radians()
    } else {
        2.0 * PI
    };

    if t_end < t_start {
        t_end += 2.0 * PI;
    }

    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    for i in 0..=a_n_samples {
        let t = t_start + (t_end - t_start) * i as f64 / a_n_samples as f64;
        let ct = t.cos();
        let st = t.sin();
        let x = a_center.x as f64 + a * ct * cp - b * st * sp;
        let y = a_center.y as f64 + a * ct * sp + b * st * cp;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    let ix = min_x.floor() as i32;
    let iy = min_y.floor() as i32;
    let ex = max_x.ceil() as i32;
    let ey = max_y.ceil() as i32;

    Box2I::new(pt(ix, iy), pt(ex - ix, ey - iy))
}

fn boxes_within(a: &Box2I, b: &Box2I, tol: i32) -> bool {
    (a.left() - b.left()).abs() <= tol
        && (a.right() - b.right()).abs() <= tol
        && (a.top() - b.top()).abs() <= tol
        && (a.bottom() - b.bottom()).abs() <= tol
}

#[test]
fn constructor_swaps_major_minor() {
    // Pass major=100, minor=300 (wrong order). Constructor should swap them
    // to major=300, minor=100 and add 90 degrees to rotation
    let e = ell(pt(0, 0), 100, 300, 0.0);

    assert_eq!(e.get_major_radius(), 300);
    assert_eq!(e.get_minor_radius(), 100);
    assert!(is_close_pct(e.get_rotation().as_degrees(), 90.0, 1e-6));
}

#[test]
fn constructor_clamps_degenerate() {
    // Zero/negative radii are clamped to 1 IU
    for (a, b) in [(0, 100), (100, 0), (-10, 100)] {
        let e = ell(pt(0, 0), a, b, 0.0);
        assert!(e.get_major_radius() >= 1);
        assert!(e.get_minor_radius() >= 1);
    }
}

#[test]
fn axis_aligned_ellipse_bbox() {
    // Axis-aligned ellipse at (1000, 2000) with major=500 along X, minor=300 along Y.
    // No rotation, so the bbox is center.x +/- 500 and center.y +/- 300.
    let e = ell(pt(1000, 2000), 500, 300, 0.0);
    let bbox = e.bbox(0);

    assert_eq!(bbox.left(), 500);
    assert_eq!(bbox.right(), 1500);
    assert_eq!(bbox.top(), 1700);
    assert_eq!(bbox.bottom(), 2300);
}

#[test]
fn rotated_ellipse_bbox_at_45_degrees() {
    // At 45 degree rotation, the ellipse extends equally in both directions. The
    // half extent on each axis is sqrt((a^2 + b^2) / 2).
    let a = 500;
    let b = 300;
    let e = ell(pt(0, 0), a, b, 45.0);
    let bbox = e.bbox(0);

    let expected = ((a as f64 * a as f64 + b as f64 * b as f64) / 2.0).sqrt();

    assert!((bbox.width() as f64 / 2.0 - expected).abs() <= 2.0);
    assert!((bbox.height() as f64 / 2.0 - expected).abs() <= 2.0);
}

#[test]
fn random_closed_ellipse_bbox_vs_brute_force() {
    // Generate 1000 random closed ellipses with varying centers, radii, and
    // rotations. Compare the analytical BBox against the brute-force sampled
    // bbox. They must agree within +/- 2 IU (integer rounding tolerance).
    let mut rng = Mt19937::new(42);

    for _ in 0..1000 {
        let center = rand_pt(&mut rng, -10000, 10000);
        let mut r1 = uni(&mut rng, 50, 2000);
        let mut r2 = uni(&mut rng, 50, 2000);

        if r1 < r2 {
            std::mem::swap(&mut r1, &mut r2);
        }

        let rot = deg(uniform_real_libstdcxx(&mut rng, 0.0, 360.0));

        let e = ShapeEllipse::new(center, r1, r2, rot);
        let analytic = e.bbox(0);
        let brute =
            brute_force_ellipse_bbox(center, r1, r2, rot, deg(0.0), deg(360.0), false, 10000);

        // Allow +/- 2 IU for integer rounding on either side.
        assert!(boxes_within(&analytic, &brute, 2));
    }
}

#[test]
fn random_elliptical_arc_bbox_vs_brute_force() {
    // 1000 random elliptical arcs with varying start angles and sweep lengths.
    // The arc bbox logic has to check whether each axis extremum falls inside
    // the angular range
    let mut rng = Mt19937::new(1337);
    let mut failures = 0;

    for _ in 0..1000 {
        let center = rand_pt(&mut rng, -10000, 10000);
        let mut r1 = uni(&mut rng, 50, 2000);
        let mut r2 = uni(&mut rng, 50, 2000);

        if r1 < r2 {
            std::mem::swap(&mut r1, &mut r2);
        }

        let rot = deg(uniform_real_libstdcxx(&mut rng, 0.0, 360.0));
        let start = deg(uniform_real_libstdcxx(&mut rng, 0.0, 360.0));
        let end = deg(start.as_degrees() + uniform_real_libstdcxx(&mut rng, 10.0, 350.0));

        let e = ShapeEllipse::new_arc(center, r1, r2, rot, start, end);
        let analytic = e.bbox(0);
        let brute = brute_force_ellipse_bbox(center, r1, r2, rot, start, end, true, 10000);

        if !boxes_within(&analytic, &brute, 2) {
            failures += 1;
        }
    }

    assert_eq!(failures, 0);
}

#[test]
fn circle_perimeter_via_ramanujan() {
    // When both radii are equal it's a circle. Ramanujan's formula should
    // give exactly 2 pi r in that case.
    let r = 1000;
    let e = ell(pt(0, 0), r, r, 0.0);
    assert!(is_close_pct(e.get_length(), 2.0 * PI * r as f64, 1e-9));
}

#[test]
fn ellipse_perimeter_matches_high_res_integration() {
    // Cross-check Ramanujan's approximation against a high-resolution numerical
    // integration (200,000 steps) for a 2:1 ellipse. They should agree to
    // within 10 parts per million
    let a = 1000.0f64;
    let b = 500.0f64;
    let n = 200000;
    let mut sum = 0.0;
    let h = 2.0 * PI / n as f64;

    let f = |t: f64| {
        let (s, c) = (t.sin(), t.cos());
        (a * a * s * s + b * b * c * c).sqrt()
    };

    for i in 0..n {
        let t0 = i as f64 * h;
        let t1 = (i + 1) as f64 * h;
        sum += (t1 - t0) * (f(t0) + 4.0 * f(0.5 * (t0 + t1)) + f(t1)) / 6.0;
    }

    let e = ell(pt(0, 0), a as i32, b as i32, 30.0);
    assert!(is_close_pct(e.get_length(), sum, 1e-4));
}

#[test]
fn semi_circle_arc_length() {
    // Semicircular arc of radius 1000. Length should be 1000 * pi.
    let r = 1000;
    let arc = ell_arc(pt(0, 0), r, r, 0.0, 0.0, 180.0);
    assert!(is_close_pct(arc.get_length(), PI * r as f64, 1e-6));
}

#[test]
fn quarter_circle_arc_length() {
    // Quarter of a circle (equal radii, 0 degrees to 90 degrees). Arc length must be pi * r/2.
    let r = 1000;
    let arc = ell_arc(pt(0, 0), r, r, 0.0, 0.0, 90.0);
    assert!(is_close_pct(arc.get_length(), 0.5 * PI * r as f64, 1e-6));
}

#[test]
fn point_inside_closed_axis_aligned() {
    // Check points inside, outside, and on the boundary of an unrotated ellipse.
    let e = ell(pt(0, 0), 500, 300, 0.0);

    assert!(e.point_inside(pt(0, 0), 0, false)); // center
    assert!(e.point_inside(pt(400, 0), 0, false)); // inside on major
    assert!(e.point_inside(pt(0, 200), 0, false)); // inside on minor
    assert!(!e.point_inside(pt(501, 0), 0, false)); // just outside
    assert!(!e.point_inside(pt(0, 301), 0, false)); // just outside
    assert!(!e.point_inside(pt(500, 300), 0, false)); // bbox corner, outside
}

#[test]
fn point_inside_rotated_ellipse() {
    // Ellipse rotated 90 degrees. Major axis runs along Y.
    let e = ell(pt(0, 0), 500, 300, 90.0);

    assert!(e.point_inside(pt(0, 499), 0, false)); // now tall on Y
    assert!(e.point_inside(pt(299, 0), 0, false)); // now short on X
    assert!(!e.point_inside(pt(0, 501), 0, false));
    assert!(!e.point_inside(pt(301, 0), 0, false));
}

#[test]
fn point_inside_arc_always_false() {
    // Arcs are open curves with no interior. PointInside must always return false.
    let arc = ell_arc(pt(0, 0), 500, 300, 0.0, 0.0, 180.0);

    assert!(!arc.point_inside(pt(0, 0), 0, false));
    assert!(!arc.point_inside(pt(100, 50), 0, false));
}

#[test]
fn squared_distance_circle_agrees_with_radial_distance() {
    // For a circle (equal radii), distance from a point to the boundary is
    // |distance_to_center - radius|. Verify SquaredDistance agrees.
    let r = 1000;
    let c = ell(pt(0, 0), r, r, 0.0);
    let p = pt(2000, 0);
    let expected = 1000.0;
    let got = (c.squared_distance(p, true) as f64).sqrt();

    assert!((got - expected).abs() <= 1.0);
}

#[test]
fn squared_distance_zero_inside_closed_ellipse() {
    // Point inside the center of a closed ellipse means SquaredDistance should be 0
    let e = ell(pt(0, 0), 500, 300, 30.0);
    assert_eq!(e.squared_distance(pt(0, 0), false), 0);
}

#[test]
fn squared_distance_outline_only_returns_boundary_dist() {
    // From the center, outline-only distance should be the minor radius (the
    // closest point on the boundary).
    let (a, b) = (500, 300);
    let e = ell(pt(0, 0), a, b, 0.0);
    let d = (e.squared_distance(pt(0, 0), true) as f64).sqrt();

    assert!((d - b as f64).abs() <= 2.0);
}

#[test]
fn convert_to_polyline_closed_ellipse_is_closed() {
    // A closed ellipse's polyline must be marked closed and have enough points
    // to approximate the curve.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let chain = e.convert_to_polyline(5);

    assert!(chain.is_closed());
    assert!(chain.point_count() > 8); // at least 8 segments
}

#[test]
fn convert_to_polyline_arc_is_open_with_correct_endpoints() {
    // An arc's polyline must be open. First point should be at
    // the start angle (1000, 0) and last point at the end angle (-1000, 0).
    let arc = ell_arc(pt(0, 0), 1000, 500, 0.0, 0.0, 180.0);
    let chain = arc.convert_to_polyline(5);

    assert!(!chain.is_closed());
    assert!((chain.c_point(0) - pt(1000, 0)).euclidean_norm() <= 2);
    assert!((chain.c_point(-1) - pt(-1000, 0)).euclidean_norm() <= 2);
}

#[test]
fn convert_to_polyline_all_points_within_max_error() {
    // Every tessellated point must lie on the true ellipse.
    // Verify no point deviates more than maxError + rounding tolerance.
    let max_err = 10;
    let e = ell(pt(0, 0), 2000, 800, 37.5);
    let chain = e.convert_to_polyline(max_err);

    let mut max_observed = 0;

    for i in 0..chain.point_count() {
        let d = (e.squared_distance(chain.c_point(i), true) as f64).sqrt();
        max_observed = max_observed.max(d.ceil() as i32);
    }

    // Rounding adds up to ~1 IU per axis, so permit maxErr + 2.
    assert!(max_observed <= max_err + 2);
}

#[test]
fn convert_to_polyline_tighter_error_yields_more_points() {
    // Tighter error tolerance must produce more points.
    let e = ell(pt(0, 0), 1000, 600, 0.0);
    let coarse = e.convert_to_polyline(50).point_count();
    let fine = e.convert_to_polyline(5).point_count();

    assert!(fine > coarse);
}

#[test]
fn collide_segment_through_closed_ellipse() {
    // Segment passes straight through the ellipse along the major axis.
    // Collision with distance 0 (the segment crosses the boundary).
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let s = Seg::new(pt(-1000, 0), pt(1000, 0));

    let mut actual = -1;
    assert!(e.collide_seg(&s, 0, Some(&mut actual), None));
    assert_eq!(actual, 0);
}

#[test]
fn collide_segment_endpoint_inside_closed_ellipse() {
    // One endpoint is at the center (inside the ellipse). Collision with
    // distance 0 expected
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let s = Seg::new(pt(0, 0), pt(2000, 0));

    let mut actual = -1;
    assert!(e.collide_seg(&s, 0, Some(&mut actual), None));
    assert_eq!(actual, 0);
}

#[test]
fn collide_segment_outside_far() {
    // Segment is far away from the ellipse. No collision even with 100 IU clearance.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let s = Seg::new(pt(10000, 10000), pt(20000, 20000));

    assert!(!e.collide_seg(&s, 100, None, None));
}

#[test]
fn collide_segment_near_miss_within_clearance() {
    // Segment runs 50 IU above the ellipse's top. With clearance 40 it's a miss,
    // with clearance 60 it's a hit. Reported distance should be close to 50.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let s = Seg::new(pt(-1000, 350), pt(1000, 350));

    assert!(!e.collide_seg(&s, 40, None, None)); // 50 > 40

    let mut actual = -1;
    assert!(e.collide_seg(&s, 60, Some(&mut actual), None)); // 50 < 60
    assert!((actual - 50).abs() <= 2);
}

#[test]
fn collide_degenerate_segment_at_center() {
    // Zero length segment at the center is treated as a point. Inside the
    // ellipse, so collision with distance 0.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    assert!(e.collide_seg(&Seg::new(pt(0, 0), pt(0, 0)), 0, None, None));
}

#[test]
fn collide_degenerate_segment_far_away() {
    // Zero length segment far away. No collision.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    assert!(!e.collide_seg(&Seg::new(pt(5000, 5000), pt(5000, 5000)), 100, None, None));
}

#[test]
fn collide_segment_vs_rotated_ellipse() {
    // Ellipse rotated 90 degrees. Major axis now runs along Y. A vertical
    // segment through center and a horizontal segment at y=350
    // should collide
    let e = ell(pt(0, 0), 500, 300, 90.0);

    assert!(e.collide_seg(&Seg::new(pt(0, -1000), pt(0, 1000)), 0, None, None));
    assert!(e.collide_seg(&Seg::new(pt(-1000, 350), pt(1000, 350)), 0, None, None));
}

#[test]
fn collide_segment_vs_arc_upper_half() {
    // Upper-half arc (0 to 180 degrees). A vertical segment crossing the arc
    // should collide. A horizontal segment in the lower half should miss the
    // arc even though it would hit the full ellipse
    let arc = ell_arc(pt(0, 0), 500, 300, 0.0, 0.0, 180.0);

    // Vertical segment at x=300 crosses the arc around (300, 240).
    assert!(arc.collide_seg(&Seg::new(pt(300, -500), pt(300, 500)), 0, None, None));

    // Horizontal segment at y=-100 stays in the lower half.
    // The full ellipse would intersect it, but the arc sweep does not.
    let lower = Seg::new(pt(-600, -100), pt(600, -100));
    assert!(!arc.collide_seg(&lower, 50, None, None)); // 100 > 50
    assert!(arc.collide_seg(&lower, 150, None, None)); // 100 < 150
}

#[test]
fn collide_circle_degenerate_agrees_with_shape_circle() {
    // Equal radii ellipse is a circle. Collide results must match
    let r = 1000;
    let e = ell(pt(0, 0), r, r, 0.0);
    let c = ShapeCircle::new(pt(0, 0), r);

    let segments = [
        Seg::new(pt(2000, 0), pt(2500, 0)),       // far right
        Seg::new(pt(-2000, 0), pt(2000, 0)),      // through center
        Seg::new(pt(1100, 1100), pt(2000, 2000)), // outside diagonal
        Seg::new(pt(500, 500), pt(800, 800)),     // inside
    ];

    for s in &segments {
        assert_eq!(
            e.collide_seg(s, 0, None, None),
            c.collide_seg(s, 0, None, None)
        );
        assert_eq!(
            e.collide_seg(s, 100, None, None),
            c.collide_seg(s, 100, None, None)
        );
    }
}

const NEEDS_G5: &str = "needs the SH_ELLIPSE entries of the collision matrix (package G5)";

#[test]
fn collide_ellipse_vs_circle_overlap() {
    // Ellipse and circle overlap. They should collide.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let c = ShapeCircle::new(pt(400, 0), 200);
    assert!(e.collide(&c, 0, None, None), "{NEEDS_G5}");
}

#[test]
fn collide_ellipse_vs_circle_disjoint() {
    // Ellipse and circle far apart. There is no collision.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let c = ShapeCircle::new(pt(2000, 2000), 100);
    assert!(!e.collide(&c, 0, None, None));
}

#[test]
fn collide_ellipse_vs_rect_contains_ellipse() {
    // Large rectangle entirely contains the ellipse. Collision detected
    // because the ellipse center is inside the rect.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let r = ShapeRect::from_pos_size(pt(-2000, -2000), 4000, 4000);
    assert!(e.collide(&r, 0, None, None));
}

#[test]
fn collide_ellipse_vs_rect_disjoint() {
    // Small rectangle far from the ellipse. No collision.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let r = ShapeRect::from_pos_size(pt(2000, 2000), 100, 100);
    assert!(!e.collide(&r, 0, None, None));
}

#[test]
fn collide_ellipse_vs_rect_edge_intersects() {
    // Rectangle straddles the ellipse's right edge. We have partial overlap.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let r = ShapeRect::from_pos_size(pt(400, -100), 400, 200);
    assert!(e.collide(&r, 0, None, None));
}

#[test]
fn collide_ellipse_vs_line_chain_intersects() {
    // Open polyline passes through the ellipse. It should collide.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let mut chain = ShapeLineChain::new();
    chain.append_point(pt(-1000, 100), false);
    chain.append_point(pt(1000, 100), false); // horizontal line piercing the ellipse
    chain.append_point(pt(1000, 500), false);
    assert!(e.collide(&chain, 0, None, None));
}

#[test]
fn collide_ellipse_inside_closed_chain() {
    // Closed square chain entirely contains the ellipse. Collision detected
    let mut chain = ShapeLineChain::new();
    chain.append_point(pt(-2000, -2000), false);
    chain.append_point(pt(2000, -2000), false);
    chain.append_point(pt(2000, 2000), false);
    chain.append_point(pt(-2000, 2000), false);
    chain.set_closed(true);

    let e = ell(pt(0, 0), 500, 300, 0.0);
    assert!(e.collide(&chain, 0, None, None));
}

#[test]
fn collide_ellipse_vs_arc_overlap() {
    // Circular arc overlaps the ellipse. It should collide.
    let e = ell(pt(0, 0), 500, 300, 0.0);
    let arc = ShapeArc::from_center_start_angle(pt(0, 0), pt(400, 0), deg(180.0), 0);
    assert!(e.collide(&arc, 0, None, None));
}

#[test]
fn collide_ellipse_vs_ellipse_overlap() {
    // Two overlapping ellipses. They should collide.
    let a = ell(pt(0, 0), 500, 300, 0.0);
    let b = ell(pt(400, 0), 400, 200, 0.0);
    assert!(a.collide(&b, 0, None, None));
}

#[test]
fn collide_ellipse_vs_ellipse_disjoint() {
    // Two ellipses far apart. There is no collision.
    let a = ell(pt(0, 0), 500, 300, 0.0);
    let b = ell(pt(3000, 3000), 400, 200, 0.0);
    assert!(!a.collide(&b, 0, None, None));
}

#[test]
fn collide_ellipse_inside_another_ellipse() {
    // Small ellipse inside a larger one. Collision should be detected.
    let big = ell(pt(0, 0), 2000, 1500, 0.0);
    let small = ell(pt(0, 0), 200, 100, 0.0);
    assert!(big.collide(&small, 0, None, None));
}

#[test]
fn collide_ellipse_vs_rect() {
    let e = ShapeEllipse::new(pt(0, 0), 500, 300, ANGLE_0);
    let r = ShapeRect::from_pos_size(pt(400, 0), 200, 200); // straddles the ellipse boundary
    let mut actual = -1;
    assert!(e.collide(&r, 0, Some(&mut actual), None));
    assert_eq!(actual, 0);
}

#[test]
fn rotate_by_full_turn_preserves_bbox() {
    // Rotating 360 degrees about any point should leave the bbox unchanged
    let mut e = ell(pt(100, 200), 500, 300, 30.0);
    let before = e.bbox(0);
    e.rotate(deg(360.0), pt(0, 0));
    let after = e.bbox(0);

    // Full 360 degree rotation: bbox position and size should match (integer rounding <= 2 IU).
    assert!(boxes_within(&before, &after, 2));
}

#[test]
fn rotate_circle_about_center_is_bbox_invariant() {
    // A circle's bbox doesn't change under any rotation about its center.
    let mut c = ell(pt(0, 0), 1000, 1000, 0.0);
    let before = c.bbox(0);
    c.rotate(deg(47.5), pt(0, 0));
    let after = c.bbox(0);

    assert!(boxes_within(&before, &after, 2));
}

#[test]
fn rotate_about_non_center_translates_and_rotates() {
    // Rotating 180 degrees about (1000, 0) moves the center from (0, 0) to
    // (2000, 0) and subtracts 180 from the ellipse's internal rotation.
    let mut e = ell(pt(0, 0), 500, 300, 0.0);
    e.rotate(deg(180.0), pt(1000, 0));

    assert!((e.get_center().x - 2000).abs() <= 1);
    assert!(e.get_center().y.abs() <= 1);
    assert!(is_close_pct(e.get_rotation().as_degrees(), -180.0, 1e-6));
}

#[test]
fn mirror_left_right_flips_center() {
    // Left-right mirror across x=1000: center.x flips from 100 to 1900,
    // rotation negates from 30 to -30 degrees.
    let mut e = ell(pt(100, 200), 500, 300, 30.0);
    e.mirror(pt(1000, 0), FlipDirection::LeftRight);

    assert_eq!(e.get_center().x, 1900);
    assert_eq!(e.get_center().y, 200);
    assert!(is_close_pct(e.get_rotation().as_degrees(), -30.0, 1e-6));
}

#[test]
fn mirror_top_bottom_flips_center() {
    // Top-bottom mirror across y=500: center.y flips from 200 to 800,
    // rotation negates
    let mut e = ell(pt(100, 200), 500, 300, 30.0);
    e.mirror(pt(0, 500), FlipDirection::TopBottom);

    assert_eq!(e.get_center().x, 100);
    assert_eq!(e.get_center().y, 800);
    assert!(is_close_pct(e.get_rotation().as_degrees(), -30.0, 1e-6));
}

#[test]
fn mirror_twice_is_identity() {
    // MirrorLeftRightTwiceIsIdentity and MirrorTopBottomTwiceIsIdentity: mirroring twice with
    // the same axis returns to the original center and rotation.
    for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
        let mut e = ell(pt(100, 200), 500, 300, 45.0);
        let orig_center = e.get_center();
        let orig_rotation = e.get_rotation().as_degrees();

        e.mirror(pt(1234, 5678), dir);
        e.mirror(pt(1234, 5678), dir);

        assert_eq!(e.get_center().x, orig_center.x);
        assert_eq!(e.get_center().y, orig_center.y);
        assert!(is_close_pct(
            e.get_rotation().as_degrees(),
            orig_rotation,
            1e-6
        ));
    }
}

#[test]
fn mirror_arc_swaps_and_reflects_sweep() {
    // Symmetric arc [30, 150] mirrored left-right. The sweep [30, 150] is
    // symmetric about 90 degrees, so start and end angles stay the same.
    let mut arc = ell_arc(pt(0, 0), 500, 300, 0.0, 30.0, 150.0);
    arc.mirror(pt(0, 0), FlipDirection::LeftRight);

    assert!(is_close_pct(arc.get_start_angle().as_degrees(), 30.0, 1e-6));
    assert!(is_close_pct(arc.get_end_angle().as_degrees(), 150.0, 1e-6));
}

#[test]
fn mirror_arc_asymmetric_swaps_sweep() {
    // Asymmetric arc [20, 80] mirrored left-right. Start becomes 180-80=100,
    // end becomes 180-20=160.
    let mut arc = ell_arc(pt(0, 0), 500, 300, 0.0, 20.0, 80.0);
    arc.mirror(pt(0, 0), FlipDirection::LeftRight);

    assert!(is_close_pct(
        arc.get_start_angle().as_degrees(),
        100.0,
        1e-6
    )); // 180 - 80
    assert!(is_close_pct(arc.get_end_angle().as_degrees(), 160.0, 1e-6)); // 180 - 20
}

#[test]
fn format_contains_expected_fields() {
    let e = ell(pt(100, 200), 500, 300, 30.0);
    let cpp = e.format(true);

    // output mentions all the key numeric values.
    for s in ["SHAPE_ELLIPSE", "100", "200", "500", "300"] {
        assert!(cpp.contains(s));
    }

    let plain = e.format(false);
    assert!(plain.contains("500"));
    assert!(plain.contains("300"));

    // Exact texts (not in the C++ test): the stream formatting of the reference build
    assert_eq!(
        cpp,
        "SHAPE_ELLIPSE( VECTOR2I( 100, 200 ), 500, 300, EDA_ANGLE( 30, DEGREES_T ) );"
    );
    assert_eq!(plain, "shape 10 100 200 500 300 30 0");
}

#[test]
fn fuzz_random_ellipses_invariants_and_determinism() {
    // 10,000 random ellipses. Check that major >= minor after construction
    // bbox is non-degenerate, SquaredDistance gives the same result when
    // called multiple times and PointInside is consistent
    // with SquaredDistance returning 0 for interior points.
    let mut rng = Mt19937::new(12345);
    let mut determinism_failures = 0;

    for _ in 0..10000 {
        let center = rand_pt(&mut rng, -10000, 10000);
        let r1 = uni(&mut rng, 50, 2000);
        let r2 = uni(&mut rng, 50, 2000);
        let rot = deg(uniform_real_libstdcxx(&mut rng, 0.0, 360.0));

        let e = ShapeEllipse::new(center, r1, r2, rot);

        // major >= minor > 0 after normalize
        assert!(e.get_major_radius() >= e.get_minor_radius());
        assert!(e.get_minor_radius() > 0);

        // BBox is non-degenerate
        let bbox = e.bbox(0);
        assert!(bbox.width() > 0);
        assert!(bbox.height() > 0);

        // SquaredDistance and PointInside are functions of their inputs
        let test_pt = rand_pt(&mut rng, -15000, 15000);
        let d1: Ecoord = e.squared_distance(test_pt, false);
        let d2: Ecoord = e.squared_distance(test_pt, false);
        let d3: Ecoord = e.squared_distance(test_pt, false);

        if d1 != d2 || d2 != d3 {
            determinism_failures += 1;
        }

        // If PointInside is true for a closed ellipse, non-outline
        // SquaredDistance must be 0.
        if !e.is_arc() && e.point_inside(test_pt, 0, false) {
            assert_eq!(e.squared_distance(test_pt, false), 0);
        }
    }

    assert_eq!(determinism_failures, 0);
}

#[test]
fn cross_check_segment_collide_against_tessellation() {
    let mut rng = Mt19937::new(9999);
    let n = 5000;
    let clearance = 50;
    let mut mismatches = 0;

    for _ in 0..n {
        let ec = rand_pt(&mut rng, -5000, 5000);
        let r1 = uni(&mut rng, 100, 1500);
        let r2 = uni(&mut rng, 100, 1500);
        let rot = deg(uniform_real_libstdcxx(&mut rng, 0.0, 360.0));

        let e = ShapeEllipse::new(ec, r1, r2, rot);

        // SEG s( VECTOR2I( d, d ), VECTOR2I( d, d ) ): right to left
        let b = rand_pt(&mut rng, -8000, 8000);
        let a = rand_pt(&mut rng, -8000, 8000);
        let s = Seg::new(a, b);

        let analytic_collide = e.collide_seg(&s, clearance, None, None);

        // Brute force: very fine tessellation + SHAPE_LINE_CHAIN::Collide.
        // Tessellation error 2 IU << clearance 50 IU, so boundary ambiguity is small.
        let chain = e.convert_to_polyline(2);
        let mut brute_collide = chain.collide_seg(&s, clearance, None, None);

        // The tessellated chain's Collide does not model the closed ellipse interior.
        // Compensate: for a closed ellipse, a segment endpoint inside the interior is
        // a collision too.
        if !brute_collide
            && !e.is_arc()
            && (e.point_inside(s.a, 0, false) || e.point_inside(s.b, 0, false))
        {
            brute_collide = true;
        }

        if analytic_collide != brute_collide {
            mismatches += 1;
        }
    }

    // Near-threshold cases may disagree within the 2 IU tessellation budget.
    // Allow up to 1% boundary mismatches.
    assert!(
        mismatches <= n / 100,
        "cross-check mismatches: {mismatches} / {n}"
    );
}

/// When minor > major, radii swap and rotation += 90.  For arcs, angles must shift by -90 to
/// keep the same physical arc.
#[test]
fn normalize_arc_angles_on_swap() {
    // Major=20, Minor=50 -> after normalize: Major=50, Minor=20, Rotation += 90
    let e = ell_arc(pt(0, 0), 20, 50, 0.0, 30.0, 120.0);

    assert_eq!(e.get_major_radius(), 50);
    assert_eq!(e.get_minor_radius(), 20);
    assert!(is_close_pct(e.get_rotation().as_degrees(), 90.0, 1e-6));

    // Angles should shift by -90: 30-90=-60, 120-90=30
    assert!(is_close_pct(e.get_start_angle().as_degrees(), -60.0, 1e-6));
    assert!(is_close_pct(e.get_end_angle().as_degrees(), 30.0, 1e-6));

    // Sweep should be preserved: was 90, still 90
    let sweep = e.get_end_angle().as_degrees() - e.get_start_angle().as_degrees();
    assert!(is_close_pct(sweep, 90.0, 1e-6));
}

/// normalize() does NOT adjust angles for closed ellipses (only arcs).
#[test]
fn normalize_closed_ellipse_no_angle_shift() {
    let e = ell(pt(0, 0), 20, 50, 0.0);

    assert_eq!(e.get_major_radius(), 50);
    assert_eq!(e.get_minor_radius(), 20);
    assert!(is_close_pct(e.get_rotation().as_degrees(), 90.0, 1e-6));
    assert!(!e.is_arc());
}

/// isAngleInSweep handles full-sweep (start=0, end=360) correctly.
#[test]
fn full_sweep_angle_in_sweep() {
    let e = ell_arc(pt(0, 0), 100, 50, 0.0, 0.0, 360.0);

    // Every angle should be in the sweep for a full 360 arc.
    for p in [pt(100, 0), pt(0, 50), pt(-100, 0), pt(0, -50)] {
        assert!(e.collide_seg(&Seg::new(p, p), 1, None, None));
    }
}

/// The cache is consistent after SetRotation.
#[test]
fn cache_consistency_after_set_rotation() {
    let mut e = ell(pt(0, 0), 100, 50, 0.0);
    let box0 = e.bbox(0);

    // Rotate 90 degrees so major and minor visual extents swap.
    e.set_rotation(deg(90.0));
    let box90 = e.bbox(0);

    assert_eq!(box0.width(), box90.height());
    assert_eq!(box0.height(), box90.width());
}

/// The cache is consistent after SetMajorRadius.
#[test]
fn cache_consistency_after_set_major_radius() {
    let mut e = ell(pt(0, 0), 100, 50, 0.0);
    let box1 = e.bbox(0);
    e.set_major_radius(200);
    let box2 = e.bbox(0);

    // Width should double (major axis is along X at rotation=0).
    assert_eq!(box2.width(), box1.width() * 2);
    // Height should stay the same (minor unchanged).
    assert_eq!(box2.height(), box1.height());
}

/// PointInside after rotation change (stale cache test).
#[test]
fn point_inside_after_rotation_change() {
    let mut e = ell(pt(0, 0), 100, 30, 0.0);

    assert!(e.point_inside(pt(80, 0), 0, false));
    assert!(!e.point_inside(pt(0, 80), 0, false));

    e.set_rotation(deg(90.0));

    assert!(!e.point_inside(pt(80, 0), 0, false));
    assert!(e.point_inside(pt(0, 80), 0, false));
}

fn contains_point_near(a_points: &[Vector2I], a_expected: Vector2I) -> bool {
    a_points.iter().any(|p| p.distance(a_expected) <= 2.0)
}

#[test]
fn intersect_ellipse_with_ellipse() {
    let first = ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_0);
    let second = ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_90);

    let points = first.intersect_ellipse(&second);
    assert_eq!(points.len(), 4);

    let offset: i32 = ki_round(2000000.0 / 5.0f64.sqrt());

    for sx in [-1, 1] {
        for sy in [-1, 1] {
            assert!(
                contains_point_near(&points, pt(sx * offset, sy * offset)),
                "no crossing near {}, {}",
                sx * offset,
                sy * offset
            );
        }
    }
}

#[test]
fn intersect_ellipse_with_offset_circle() {
    let ellipse = ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_0);
    let circle = Circle::new(pt(0, 300000), 1500000);

    let points = ellipse.intersect_circle(&circle);
    assert_eq!(points.len(), 4);

    for p in &points {
        assert!(
            ellipse.squared_distance(*p, true) <= 4,
            "point {p:?} is off the ellipse"
        );

        let radial = (circle.center.distance(*p) - circle.radius as f64).abs();
        assert!(radial <= 2.0, "point {p:?} is off the circle");
    }
}

#[test]
fn intersect_ellipse_with_segment() {
    let ellipse = ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_0);
    let seg = Seg::new(pt(-3000000, 0), pt(3000000, 0));

    let points = ellipse.intersect_seg(&seg, false);
    assert_eq!(points.len(), 2);
    assert!(contains_point_near(&points, pt(-2000000, 0)));
    assert!(contains_point_near(&points, pt(2000000, 0)));
}

#[test]
fn intersect_ellipse_with_elliptical_arc() {
    let ellipse = ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_0);
    let arc = ShapeEllipse::new_arc(pt(0, 0), 2000000, 1000000, ANGLE_90, ANGLE_0, ANGLE_90);

    let points = ellipse.intersect_ellipse(&arc);
    let offset: i32 = ki_round(2000000.0 / 5.0f64.sqrt());

    assert_eq!(points.len(), 1);
    assert!(contains_point_near(&points, pt(-offset, offset)));
}

#[test]
fn intersection_visitor_handles_ellipses() {
    let first = IntersectableGeom::Ellipse(ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_0));
    let second =
        IntersectableGeom::Ellipse(ShapeEllipse::new(pt(0, 0), 2000000, 1000000, ANGLE_90));
    let circle = IntersectableGeom::Circle(Circle::new(pt(0, 300000), 1500000));

    let mut ellipse_to_ellipse = Vec::new();
    IntersectionVisitor::new(&first, &mut ellipse_to_ellipse).visit(&second);
    assert_eq!(ellipse_to_ellipse.len(), 4);

    let mut visited_circle = Vec::new();
    IntersectionVisitor::new(&first, &mut visited_circle).visit(&circle);
    assert_eq!(visited_circle.len(), 4);

    let mut visited_ellipse = Vec::new();
    IntersectionVisitor::new(&circle, &mut visited_ellipse).visit(&first);
    assert_eq!(visited_ellipse.len(), 4);
}
