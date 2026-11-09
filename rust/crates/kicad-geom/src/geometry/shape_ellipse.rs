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

//! Port of `libs/kimath/include/geometry/shape_ellipse.h` and
//! `libs/kimath/src/geometry/shape_ellipse.cpp` (`SHAPE_ELLIPSE`), including the file-private
//! numeric helpers (`adaptiveSimpson`, `subdivideEllipseArc`, `quadraticRoots`, `cubicRoots`,
//! `quarticRoots`, `dedupePoints`).
//!
//! | C++ | Rust |
//! |---|---|
//! | `SHAPE_ELLIPSE( aCenter, aMajorRadius, aMinorRadius, aRotation )` | [`ShapeEllipse::new`] |
//! | `SHAPE_ELLIPSE( ..., aRotation, aStartAngle, aEndAngle )` | [`ShapeEllipse::new_arc`] |
//! | `SHAPE_ELLIPSE( aCenter, aMajorEndpoint, aRatio )` | [`ShapeEllipse::from_major_endpoint`] |
//! | `SHAPE_ELLIPSE( aCenter, aMajorEndpoint, aRatio, aStartAngle, aEndAngle )` | [`ShapeEllipse::from_major_endpoint_arc`] |
//! | `Intersect( SHAPE_ELLIPSE / CIRCLE / SHAPE_ARC / SEG, aTreatAsLine )` | `intersect_ellipse` / `intersect_circle` / `intersect_arc` / `intersect_seg` |
//!
//! The `static_cast<int>` / `static_cast<ecoord>` conversions of doubles use
//! `cmath::f64_to_i32_x86` / `f64_to_i64_x86` (PORTING.md section 9).

use std::f64::consts::PI;

use crate::cmath;
use crate::geometry::approximation::ErrorLoc;
use crate::geometry::circle::Circle;
use crate::geometry::eda_angle::{ANGLE_0, ANGLE_90, EdaAngle, RADIANS_T};
use crate::geometry::ellipse::Ellipse;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{MIN_PRECISION_IU, Shape, ShapeRef, ShapeType, format_shape_base};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::kicore::mirror::FlipDirection;
use crate::math::box2::Box2I;
use crate::math::util::{ki_round, std_clamp};
use crate::math::vector2::{Vector2D, Vector2I};
use crate::ostream::format_double;
use crate::stdsort::{std_sort, std_unique};
use crate::trigo::rotate_point_around;

/// Port of the file-private `adaptiveSimpsonRec()`.
#[allow(clippy::too_many_arguments)]
fn adaptive_simpson_rec<F: Fn(f64) -> f64>(
    f: &F,
    a: f64,
    b: f64,
    tol: f64,
    whole: f64,
    fa: f64,
    fb: f64,
    fm: f64,
    depth: i32,
) -> f64 {
    let m = 0.5 * (a + b);
    let lm = 0.5 * (a + m);
    let rm = 0.5 * (m + b);
    let flm = f(lm);
    let frm = f(rm);

    let left = (m - a) * (fa + 4.0 * flm + fm) / 6.0;
    let right = (b - m) * (fm + 4.0 * frm + fb) / 6.0;
    let diff = left + right - whole;

    if depth <= 0 || diff.abs() < 15.0 * tol {
        return left + right + diff / 15.0;
    }

    adaptive_simpson_rec(f, a, m, 0.5 * tol, left, fa, fm, flm, depth - 1)
        + adaptive_simpson_rec(f, m, b, 0.5 * tol, right, fm, fb, frm, depth - 1)
}

/// Port of the file-private `adaptiveSimpson()`: adaptive Simpson's method
/// (<https://en.wikipedia.org/wiki/Adaptive_Simpson%27s_method>) for `f` over `[a, b]` with
/// absolute tolerance `tol` and a recursion depth cap.
pub fn adaptive_simpson<F: Fn(f64) -> f64>(f: F, a: f64, b: f64, tol: f64, max_depth: i32) -> f64 {
    let fa = f(a);
    let fb = f(b);
    let fm = f(0.5 * (a + b));
    let whole = (b - a) * (fa + 4.0 * fm + fb) / 6.0;
    adaptive_simpson_rec(&f, a, b, tol, whole, fa, fb, fm, max_depth)
}

/// Port of the file-private `subdivideEllipseArc()`: split the curve into line segments.  If
/// the straight line between two points is too far from the actual ellipse curve at the
/// midpoint, split in half and try again; stop when the straight line is close enough.
#[allow(clippy::too_many_arguments)]
fn subdivide_ellipse_arc<E: Fn(f64) -> Vector2I>(
    t0: f64,
    p0: Vector2I,
    t1: f64,
    p1: Vector2I,
    a_max_err_sq: f64,
    a_depth: i32,
    a_eval: &E,
    a_out: &mut ShapeLineChain,
) {
    if a_depth <= 0 {
        a_out.append_point(p1, false);
        return;
    }

    let tm = 0.5 * (t0 + t1);
    let pm = a_eval(tm);

    let mx = 0.5 * (p0.x as f64 + p1.x as f64);
    let my = 0.5 * (p0.y as f64 + p1.y as f64);
    let ex = pm.x as f64 - mx;
    let ey = pm.y as f64 - my;

    if ex * ex + ey * ey <= a_max_err_sq {
        a_out.append_point(p1, false);
        return;
    }

    subdivide_ellipse_arc(t0, p0, tm, pm, a_max_err_sq, a_depth - 1, a_eval, a_out);
    subdivide_ellipse_arc(tm, pm, t1, p1, a_max_err_sq, a_depth - 1, a_eval, a_out);
}

/// `ROOT_EPSILON` of `shape_ellipse.cpp`.
const ROOT_EPSILON: f64 = 1e-12;

/// Port of the file-private `quadraticRoots()`: real roots of `a x^2 + b x + c`, degenerating
/// to the linear case when `a` vanishes.  The second root is formed by division rather than
/// by subtraction, which keeps its precision when the discriminant nearly cancels the linear
/// term.
pub fn quadratic_roots(a_a: f64, a_b: f64, a_c: f64) -> Vec<f64> {
    let mut roots = Vec::new();

    if a_a.abs() < ROOT_EPSILON {
        if a_b.abs() >= ROOT_EPSILON {
            roots.push(-a_c / a_b);
        }

        return roots;
    }

    let disc = a_b * a_b - 4.0 * a_a * a_c;

    if disc < 0.0 {
        return roots;
    }

    let sq = disc.sqrt();
    let q = -0.5 * (a_b + if a_b >= 0.0 { sq } else { -sq });

    roots.push(q / a_a);

    if q.abs() >= ROOT_EPSILON {
        roots.push(a_c / q);
    }

    roots
}

/// Port of the file-private `cubicRoots()`: real roots of `a x^3 + b x^2 + c x + d`.  The
/// three root case uses the trigonometric form, which avoids taking cube roots of complex
/// numbers.
pub fn cubic_roots(a_a: f64, a_b: f64, a_c: f64, a_d: f64) -> Vec<f64> {
    if a_a.abs() < ROOT_EPSILON {
        return quadratic_roots(a_b, a_c, a_d);
    }

    let b = a_b / a_a;
    let c = a_c / a_a;
    let d = a_d / a_a;

    // Depress to y^3 + p y + q, where x is y minus the shift
    let shift = b / 3.0;
    let p = c - b * b / 3.0;
    let q = 2.0 * b * b * b / 27.0 - b * c / 3.0 + d;

    let mut roots = Vec::new();
    let disc = q * q / 4.0 + p * p * p / 27.0;

    if p.abs() < ROOT_EPSILON && q.abs() < ROOT_EPSILON {
        roots.push(-shift);
    } else if disc > 0.0 {
        let sq = disc.sqrt();
        roots.push(cmath::cbrt(-q / 2.0 + sq) + cmath::cbrt(-q / 2.0 - sq) - shift);
    } else {
        let r = 2.0 * (-p / 3.0).sqrt();
        let arg = std_clamp(3.0 * q / (p * r), -1.0, 1.0);
        let phi = cmath::acos(arg) / 3.0;

        for k in 0..3 {
            roots.push(r * cmath::cos(phi - 2.0 * PI * k as f64 / 3.0) - shift);
        }
    }

    roots
}

/// Port of the file-private `quarticRoots()`: real roots of `a x^4 + b x^3 + c x^2 + d x + e`
/// by Ferrari's method, which splits the depressed quartic into two quadratics using a root
/// of its resolvent cubic.
pub fn quartic_roots(a_a: f64, a_b: f64, a_c: f64, a_d: f64, a_e: f64) -> Vec<f64> {
    if a_a.abs() < ROOT_EPSILON {
        return cubic_roots(a_b, a_c, a_d, a_e);
    }

    let b = a_b / a_a;
    let c = a_c / a_a;
    let d = a_d / a_a;
    let e = a_e / a_a;

    // Depress to y^4 + p y^2 + q y + r, where x is y minus the shift
    let shift = b / 4.0;
    let p = c - 3.0 * b * b / 8.0;
    let q = d - b * c / 2.0 + b * b * b / 8.0;
    let r = e - b * d / 4.0 + b * b * c / 16.0 - 3.0 * b * b * b * b / 256.0;

    let mut roots = Vec::new();

    if q.abs() < ROOT_EPSILON {
        for y_sq in quadratic_roots(1.0, p, r) {
            if y_sq >= 0.0 {
                let y = y_sq.sqrt();
                roots.push(y - shift);
                roots.push(-y - shift);
            }
        }

        return roots;
    }

    let mut alpha_sq = 0.0;

    for z in cubic_roots(1.0, 2.0 * p, p * p - 4.0 * r, -q * q) {
        if z > alpha_sq {
            alpha_sq = z;
        }
    }

    if alpha_sq <= 0.0 {
        return roots;
    }

    let alpha = alpha_sq.sqrt();
    let beta = (p + alpha_sq - q / alpha) / 2.0;
    let gamma = (p + alpha_sq + q / alpha) / 2.0;

    for y in quadratic_roots(1.0, alpha, beta) {
        roots.push(y - shift);
    }

    for y in quadratic_roots(1.0, -alpha, gamma) {
        roots.push(y - shift);
    }

    roots
}

/// Port of the file-private `dedupePoints()`: sort by x then y, then drop duplicates.
pub fn dedupe_points(a_points: &mut Vec<Vector2I>) {
    std_sort(a_points, |a_left, a_right| {
        if a_left.x != a_right.x { a_left.x < a_right.x } else { a_left.y < a_right.y }
    });

    std_unique(a_points, |a, b| a == b);
}

/// `std::round( x )` converted to `int` with `static_cast<int>`.
fn round_to_i32(x: f64) -> i32 {
    cmath::f64_to_i32_x86(x.round())
}

/// Port of the private `SHAPE_ELLIPSE::CONIC`: a conic curve
/// `Axx x^2 + Axy xy + Ayy y^2 + Bx x + By y + C = 0`, written in an ellipse's local frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Conic {
    axx: f64,
    axy: f64,
    ayy: f64,
    bx: f64,
    by: f64,
    c: f64,
}

/// Port of `SHAPE_ELLIPSE`: an ellipse or elliptical arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeEllipse {
    /// `m_ellipse`: wrapped geometric data (from `geometry/ellipse.h`)
    ellipse: Ellipse<i32>,
    /// `m_isArc`: true if open elliptical arc, false if closed ellipse
    is_arc: bool,
    /// `m_sinRot`: sin(Rotation)
    sin_rot: f64,
    /// `m_cosRot`: cos(Rotation)
    cos_rot: f64,
    /// `m_invMajorRSq`: 1 / MajorRadius ^ 2
    inv_major_r_sq: f64,
    /// `m_invMinorRSq`: 1 / MinorRadius ^ 2
    inv_minor_r_sq: f64,
}

impl Default for ShapeEllipse {
    /// Port of `SHAPE_ELLIPSE()`.
    fn default() -> Self {
        ShapeEllipse {
            ellipse: Ellipse::default(),
            is_arc: false,
            sin_rot: 0.0,
            cos_rot: 1.0,
            inv_major_r_sq: 0.0,
            inv_minor_r_sq: 0.0,
        }
    }
}

impl ShapeEllipse {
    fn from_parts(a_ellipse: Ellipse<i32>, a_is_arc: bool) -> Self {
        let mut s = ShapeEllipse { ellipse: a_ellipse, is_arc: a_is_arc, ..ShapeEllipse::default() };
        s.normalize();
        s
    }

    /// Port of `SHAPE_ELLIPSE( aCenter, aMajorRadius, aMinorRadius, aRotation )`: a closed
    /// ellipse.
    pub fn new(a_center: Vector2I, a_major_radius: i32, a_minor_radius: i32, a_rotation: EdaAngle) -> Self {
        Self::from_parts(Ellipse::new_full(a_center, a_major_radius, a_minor_radius, a_rotation), false)
    }

    /// Port of `SHAPE_ELLIPSE( aCenter, aMajorRadius, aMinorRadius, aRotation, aStartAngle,
    /// aEndAngle )`: an elliptical arc.
    pub fn new_arc(
        a_center: Vector2I,
        a_major_radius: i32,
        a_minor_radius: i32,
        a_rotation: EdaAngle,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
    ) -> Self {
        Self::from_parts(
            Ellipse::new(a_center, a_major_radius, a_minor_radius, a_rotation, a_start_angle, a_end_angle),
            true,
        )
    }

    /// Port of `SHAPE_ELLIPSE( aCenter, aMajorEndpoint, aRatio )`: a closed ellipse whose major
    /// axis ends at `a_major_endpoint` (relative to the centre).
    pub fn from_major_endpoint(a_center: Vector2I, a_major_endpoint: Vector2I, a_ratio: f64) -> Self {
        Self::from_parts(
            Ellipse::from_major_axis(a_center, a_major_endpoint, a_ratio, ANGLE_0, crate::geometry::eda_angle::FULL_CIRCLE),
            false,
        )
    }

    /// Port of `SHAPE_ELLIPSE( aCenter, aMajorEndpoint, aRatio, aStartAngle, aEndAngle )`.
    pub fn from_major_endpoint_arc(
        a_center: Vector2I,
        a_major_endpoint: Vector2I,
        a_ratio: f64,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
    ) -> Self {
        Self::from_parts(Ellipse::from_major_axis(a_center, a_major_endpoint, a_ratio, a_start_angle, a_end_angle), true)
    }

    /// Port of `GetCenter()`.
    pub fn get_center(&self) -> Vector2I {
        self.ellipse.center
    }

    /// Port of `GetMajorRadius()`.
    pub fn get_major_radius(&self) -> i32 {
        self.ellipse.major_radius
    }

    /// Port of `GetMinorRadius()`.
    pub fn get_minor_radius(&self) -> i32 {
        self.ellipse.minor_radius
    }

    /// Port of `GetRotation()`.
    pub fn get_rotation(&self) -> EdaAngle {
        self.ellipse.rotation
    }

    /// Port of `GetStartAngle()`.
    pub fn get_start_angle(&self) -> EdaAngle {
        self.ellipse.start_angle
    }

    /// Port of `GetEndAngle()`.
    pub fn get_end_angle(&self) -> EdaAngle {
        self.ellipse.end_angle
    }

    /// Port of `IsArc()`.
    pub fn is_arc(&self) -> bool {
        self.is_arc
    }

    /// Port of `operator==`: the cached trigonometry is derived from the other fields, so it is
    /// not compared.
    pub fn equals(&self, a_other: &ShapeEllipse) -> bool {
        self.is_arc == a_other.is_arc
            && self.ellipse.center == a_other.ellipse.center
            && self.ellipse.major_radius == a_other.ellipse.major_radius
            && self.ellipse.minor_radius == a_other.ellipse.minor_radius
            && self.ellipse.rotation == a_other.ellipse.rotation
            && self.ellipse.start_angle == a_other.ellipse.start_angle
            && self.ellipse.end_angle == a_other.ellipse.end_angle
    }

    /// Port of `SetCenter()`.
    pub fn set_center(&mut self, a_center: Vector2I) {
        self.ellipse.center = a_center;
    }

    /// Port of `SetMajorRadius()`.
    pub fn set_major_radius(&mut self, a_radius: i32) {
        self.ellipse.major_radius = a_radius;
        self.normalize();
    }

    /// Port of `SetMinorRadius()`.
    pub fn set_minor_radius(&mut self, a_radius: i32) {
        self.ellipse.minor_radius = a_radius;
        self.normalize();
    }

    /// Port of `SetRotation()`.
    pub fn set_rotation(&mut self, a_angle: EdaAngle) {
        self.ellipse.rotation = a_angle;
        self.update_cache();
    }

    /// Port of `SetStartAngle()`.
    pub fn set_start_angle(&mut self, a_angle: EdaAngle) {
        self.ellipse.start_angle = a_angle;
    }

    /// Port of `SetEndAngle()`.
    pub fn set_end_angle(&mut self, a_angle: EdaAngle) {
        self.ellipse.end_angle = a_angle;
    }

    /// Port of `normalize()`: if major < minor, swap them and add 90 degrees to the rotation.
    /// Clamps non positive radii to 1.
    fn normalize(&mut self) {
        self.ellipse.major_radius = 1.max(self.ellipse.major_radius);
        self.ellipse.minor_radius = 1.max(self.ellipse.minor_radius);

        if self.ellipse.major_radius < self.ellipse.minor_radius {
            std::mem::swap(&mut self.ellipse.major_radius, &mut self.ellipse.minor_radius);
            self.ellipse.rotation += ANGLE_90;

            if self.is_arc {
                self.ellipse.start_angle -= ANGLE_90;
                self.ellipse.end_angle -= ANGLE_90;
            }
        }

        self.update_cache();
    }

    /// Port of `updateCache()`: recompute the cached sin / cos and inverse-radius-squared
    /// values.
    fn update_cache(&mut self) {
        let rot_rad = self.ellipse.rotation.as_radians();
        self.sin_rot = cmath::sin(rot_rad);
        self.cos_rot = cmath::cos(rot_rad);

        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;
        self.inv_major_r_sq = 1.0 / (a * a);
        self.inv_minor_r_sq = 1.0 / (b * b);
    }

    /// Port of `GetLength()`: Ramanujan's second approximation for a closed ellipse, adaptive
    /// Simpson integration of the sweep for an arc.
    pub fn get_length(&self) -> f64 {
        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;

        if !self.is_arc {
            // Ramanujan's second approximation
            // See https://en.wikipedia.org/wiki/Perimeter_of_an_ellipse
            let h = (a - b) / (a + b);
            let h2 = h * h;
            return PI * (a + b) * (1.0 + 3.0 * h2 / (10.0 + (4.0 - 3.0 * h2).sqrt()));
        }

        let integrand = |theta: f64| -> f64 {
            let s = cmath::sin(theta);
            let c = cmath::cos(theta);
            (a * a * s * s + b * b * c * c).sqrt()
        };

        let (t0, t1) = self.sweep_range();

        adaptive_simpson(integrand, t0, t1, 1e-9, 20)
    }

    /// Port of `Mirror()`: mirror the ellipse across a horizontal or vertical axis passing
    /// through `a_ref`.
    pub fn mirror(&mut self, a_ref: Vector2I, a_flip_direction: FlipDirection) {
        self.ellipse.mirror(a_ref, a_flip_direction);
        self.update_cache();
    }

    /// Port of `ConvertToPolyline()`: a polyline approximation of the ellipse or arc, within
    /// `a_max_error` IU of the true curve.
    pub fn convert_to_polyline(&self, a_max_error: i32) -> ShapeLineChain {
        let a_max_error = if a_max_error < 1 { 1 } else { a_max_error };

        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;
        let cx = self.ellipse.center.x as f64;
        let cy = self.ellipse.center.y as f64;
        let sin_rot = self.sin_rot;
        let cos_rot = self.cos_rot;

        let eval = move |theta: f64| -> Vector2I {
            let ct = cmath::cos(theta);
            let st = cmath::sin(theta);
            let lx = a * ct;
            let ly = b * st;
            let wx = lx * cos_rot - ly * sin_rot;
            let wy = lx * sin_rot + ly * cos_rot;
            Vector2I::new(ki_round(cx + wx), ki_round(cy + wy))
        };

        let (t_start, t_end) = self.sweep_range();

        let max_err_sq = a_max_error as f64 * a_max_error as f64;

        let mut out = ShapeLineChain::new();
        let p_start = eval(t_start);
        let p_end = eval(t_end);

        out.append_point(p_start, false);
        subdivide_ellipse_arc(t_start, p_start, t_end, p_end, max_err_sq, 20, &eval, &mut out);

        if !self.is_arc {
            if out.point_count() > 1 && out.c_point(0) == out.c_point(-1) {
                out.remove(out.point_count() - 1);
            }

            out.set_closed(true);
        }

        out
    }

    /// Port of `sweepRange()`: the canonical counter-clockwise sweep in radians
    /// (`end >= start`), used by all sweep-aware paths.
    fn sweep_range(&self) -> (f64, f64) {
        let two_pi = 2.0 * PI;

        if !self.is_arc {
            return (0.0, two_pi);
        }

        let a_start = self.ellipse.start_angle.as_radians();
        let mut a_end = self.ellipse.end_angle.as_radians();

        let sweep = a_end - a_start;

        if sweep >= two_pi || sweep <= -two_pi {
            a_end = a_start + two_pi;
        } else if a_end < a_start {
            a_end += two_pi;
        }

        (a_start, a_end)
    }

    /// Port of `isAngleInSweep()`: whether a parameter angle falls between the start and end
    /// angles (counter-clockwise sweep).  Only valid for arcs.
    fn is_angle_in_sweep(&self, a_angle_rad: f64) -> bool {
        let two_pi = 2.0 * PI;
        let (t_start, t_end) = self.sweep_range();

        // Reduce aAngleRad into [tStart, tStart + 2*pi).
        let mut t = a_angle_rad;

        while t < t_start {
            t += two_pi;
        }

        while t >= t_start + two_pi {
            t -= two_pi;
        }

        t <= t_end
    }

    /// Port of `toLocal()`.
    fn to_local(&self, a_p: Vector2I) -> Vector2D {
        let dx = a_p.x as f64 - self.ellipse.center.x as f64;
        let dy = a_p.y as f64 - self.ellipse.center.y as f64;

        Vector2D::new(dx * self.cos_rot + dy * self.sin_rot, -dx * self.sin_rot + dy * self.cos_rot)
    }

    /// Port of `toWorld()`.
    fn to_world(&self, a_p: Vector2D) -> Vector2I {
        let wx = a_p.x * self.cos_rot - a_p.y * self.sin_rot;
        let wy = a_p.x * self.sin_rot + a_p.y * self.cos_rot;

        Vector2I::new(
            round_to_i32(self.ellipse.center.x as f64 + wx),
            round_to_i32(self.ellipse.center.y as f64 + wy),
        )
    }

    /// Port of `pointAtParam()`: the point on the full ellipse at a parameter angle, in the local
    /// frame.
    fn point_at_param(&self, a_theta: f64) -> Vector2D {
        Vector2D::new(
            self.ellipse.major_radius as f64 * cmath::cos(a_theta),
            self.ellipse.minor_radius as f64 * cmath::sin(a_theta),
        )
    }

    /// Port of `closestLocalPoint()`: the point of the curve closest to `a_local`, both in the
    /// local frame (Eberly's bisection; "Distance from a Point to an Ellipse, an Ellipsoid, or a
    /// Hyperellipsoid", David Eberly, Geometric Tools,
    /// <https://www.geometrictools.com/Documentation/DistancePointEllipseEllipsoid.pdf>).
    fn closest_local_point(&self, a_local: Vector2D) -> Vector2D {
        let lx = a_local.x;
        let ly = a_local.y;

        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;

        let y0 = lx.abs();
        let y1 = ly.abs();

        let x0_local;
        let x1_local;

        if y1 > 0.0 {
            if y0 > 0.0 {
                let z0 = y0 / a;
                let z1 = y1 / b;
                let g = z0 * z0 + z1 * z1 - 1.0;

                if g != 0.0 {
                    let r0 = (a / b) * (a / b);
                    let n0 = r0 * z0;

                    let mut s0 = z1 - 1.0;
                    let mut s1 = if g < 0.0 { 0.0 } else { (n0 * n0 + z1 * z1).sqrt() - 1.0 };
                    let mut s = 0.0;

                    for _iter in 0..64 {
                        s = 0.5 * (s0 + s1);

                        if s == s0 || s == s1 {
                            break;
                        }

                        let ratio0 = n0 / (s + r0);
                        let ratio1 = z1 / (s + 1.0);
                        let gs = ratio0 * ratio0 + ratio1 * ratio1 - 1.0;

                        if gs > 0.0 {
                            s0 = s;
                        } else if gs < 0.0 {
                            s1 = s;
                        } else {
                            break;
                        }
                    }

                    x0_local = r0 * y0 / (s + r0);
                    x1_local = y1 / (s + 1.0);
                } else {
                    // Point is on the ellipse.
                    x0_local = y0;
                    x1_local = y1;
                }
            } else {
                // y0 == 0 point lies on the minor axis.
                x0_local = 0.0;
                x1_local = b;
            }
        } else {
            // y1 == 0 point lies on the major axis.
            let numer0 = a * y0;
            let denom0 = a * a - b * b;

            if numer0 < denom0 {
                let xde0 = numer0 / denom0;
                x0_local = a * xde0;
                x1_local = b * crate::math::util::std_max(0.0, 1.0 - xde0 * xde0).sqrt();
            } else {
                x0_local = a;
                x1_local = 0.0;
            }
        }

        let closest = Vector2D::new(
            if lx < 0.0 { -x0_local } else { x0_local },
            if ly < 0.0 { -x1_local } else { x1_local },
        );

        // An arc that does not reach round to the closest point is nearest at one of its ends
        if self.is_arc {
            let closest_theta = cmath::atan2(closest.y / b, closest.x / a);

            if !self.is_angle_in_sweep(closest_theta) {
                let start = self.point_at_param(self.ellipse.start_angle.as_radians());
                let end = self.point_at_param(self.ellipse.end_angle.as_radians());

                return if (a_local - start).squared_euclidean_norm() <= (a_local - end).squared_euclidean_norm() {
                    start
                } else {
                    end
                };
            }
        }

        closest
    }

    /// Port of `NearestPoint()`: the point on the curve closest to `a_p` (for an arc limited to
    /// the drawn sweep).
    pub fn nearest_point(&self, a_p: Vector2I) -> Vector2I {
        self.to_world(self.closest_local_point(self.to_local(a_p)))
    }

    /// Port of `conicOf()`: an ellipse with the given world placement as a conic in this local
    /// frame.
    fn conic_of(&self, a_center: Vector2I, a_major_r: f64, a_minor_r: f64, a_rotation: EdaAngle) -> Conic {
        // Take a point of this local frame across to the other curve's local frame, then
        // square out the other curve's own equation to get the coefficients.
        let delta = self.ellipse.rotation.as_radians() - a_rotation.as_radians();
        let cd = cmath::cos(delta);
        let sd = cmath::sin(delta);

        let cr = cmath::cos(a_rotation.as_radians());
        let sr = cmath::sin(a_rotation.as_radians());

        let wx = (self.ellipse.center.x - a_center.x) as f64;
        let wy = (self.ellipse.center.y - a_center.y) as f64;

        let dx = wx * cr + wy * sr;
        let dy = -wx * sr + wy * cr;

        let p = 1.0 / (a_major_r * a_major_r);
        let q = 1.0 / (a_minor_r * a_minor_r);

        Conic {
            axx: p * cd * cd + q * sd * sd,
            axy: 2.0 * sd * cd * (q - p),
            ayy: p * sd * sd + q * cd * cd,
            bx: 2.0 * (p * dx * cd + q * dy * sd),
            by: 2.0 * (q * dy * cd - p * dx * sd),
            c: p * dx * dx + q * dy * dy - 1.0,
        }
    }

    /// Port of `conicRoots()`: the parameter angles of this curve where the conic is zero,
    /// already limited to its sweep.
    fn conic_roots(&self, a_conic: &Conic) -> Vec<f64> {
        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;

        // The conic seen along this ellipse, as a function of the parameter angle
        let c_a = a_conic.axx * a * a;
        let c_b = a_conic.axy * a * b;
        let c_c = a_conic.ayy * b * b;
        let c_d = a_conic.bx * a;
        let c_e = a_conic.by * b;
        let c_f = a_conic.c;

        let value = |a_theta: f64| -> f64 {
            let ct = cmath::cos(a_theta);
            let st = cmath::sin(a_theta);
            c_a * ct * ct + c_b * ct * st + c_c * st * st + c_d * ct + c_e * st + c_f
        };

        let slope = |a_theta: f64| -> f64 {
            let ct = cmath::cos(a_theta);
            let st = cmath::sin(a_theta);
            2.0 * (c_c - c_a) * ct * st + c_b * (ct * ct - st * st) - c_d * st + c_e * ct
        };

        // The tangent of the half angle turns that into a quartic, at the cost of never
        // reaching pi.  That one angle is offered below as a candidate like any other.
        let k4 = c_a - c_d + c_f;
        let k3 = 2.0 * (c_e - c_b);
        let k2 = 2.0 * (c_f - c_a) + 4.0 * c_c;
        let k1 = 2.0 * (c_b + c_e);
        let k0 = c_a + c_d + c_f;

        // std::max( { ... } ): the first of the largest values
        let mut scale = k4.abs();

        for k in [k3.abs(), k2.abs(), k1.abs(), k0.abs()] {
            if scale < k {
                scale = k;
            }
        }

        // The curves lie on top of each other, so there is no crossing to report
        if scale == 0.0 {
            return Vec::new();
        }

        let mut params = Vec::new();

        for u in quartic_roots(k4 / scale, k3 / scale, k2 / scale, k1 / scale, k0 / scale) {
            params.push(2.0 * cmath::atan(u));
        }

        params.push(PI);

        let mut result = Vec::new();

        for mut theta in params {
            // The quartic loses digits when its roots run large, so finish on the exact equation
            for _iter in 0..8 {
                let f = value(theta);
                let df = slope(theta);

                if df == 0.0 {
                    break;
                }

                let step = f / df;
                theta -= step;

                if step.abs() < 1e-15 {
                    break;
                }
            }

            if value(theta).abs() > scale * 1e-9 {
                continue;
            }

            if self.is_arc && !self.is_angle_in_sweep(theta) {
                continue;
            }

            result.push(theta);
        }

        result
    }

    /// Port of `Intersect( const SHAPE_ELLIPSE& )`: the points where this curve crosses another
    /// ellipse; crossings outside either drawn sweep are dropped.
    pub fn intersect_ellipse(&self, a_other: &ShapeEllipse) -> Vec<Vector2I> {
        let conic = self.conic_of(
            a_other.ellipse.center,
            a_other.ellipse.major_radius as f64,
            a_other.ellipse.minor_radius as f64,
            a_other.ellipse.rotation,
        );

        let mut points = Vec::new();

        for theta in self.conic_roots(&conic) {
            let world = self.ellipse.point_at_angle(EdaAngle::new(theta, RADIANS_T));

            if a_other.is_arc && !a_other.is_angle_in_sweep(a_other.ellipse.angle_at_point(world).as_radians()) {
                continue;
            }

            points.push(world);
        }

        dedupe_points(&mut points);
        points
    }

    /// Port of `intersectCircle()`: the points where this curve crosses a full circle.
    fn intersect_full_circle(&self, a_center: Vector2I, a_radius: f64) -> Vec<Vector2I> {
        let mut points = Vec::new();

        if a_radius <= 0.0 {
            return points;
        }

        for theta in self.conic_roots(&self.conic_of(a_center, a_radius, a_radius, ANGLE_0)) {
            points.push(self.ellipse.point_at_angle(EdaAngle::new(theta, RADIANS_T)));
        }

        dedupe_points(&mut points);
        points
    }

    /// Port of `Intersect( const CIRCLE& )`.
    pub fn intersect_circle(&self, a_circle: &Circle) -> Vec<Vector2I> {
        self.intersect_full_circle(a_circle.center, a_circle.radius as f64)
    }

    /// Port of `Intersect( const SHAPE_ARC& )`.
    pub fn intersect_arc(&self, a_arc: &ShapeArc) -> Vec<Vector2I> {
        // The radius is taken as a double, so the arc is not rounded to a CIRCLE first
        let mut points = self.intersect_full_circle(a_arc.get_center(), a_arc.get_radius());

        // Crossings of the full circle that miss the drawn part are pulled back to an end
        // of the arc, so they fail this test
        points.retain(|a_point| a_arc.nearest_point(*a_point).distance(*a_point) <= MIN_PRECISION_IU as f64);

        points
    }

    /// Port of `Intersect( const SEG& aSeg, bool aTreatAsLine )`: the points where this curve
    /// crosses the segment (or, with `a_treat_as_line`, its infinite line).
    pub fn intersect_seg(&self, a_seg: &Seg, a_treat_as_line: bool) -> Vec<Vector2I> {
        let start = self.to_local(a_seg.a);
        let dir = self.to_local(a_seg.b) - start;

        if dir.x == 0.0 && dir.y == 0.0 {
            return Vec::new();
        }

        // A straight line is a conic with no squared terms, so the quartic falls away by itself
        let conic = Conic {
            axx: 0.0,
            axy: 0.0,
            ayy: 0.0,
            bx: dir.y,
            by: -dir.x,
            c: start.y * dir.x - start.x * dir.y,
        };

        let mut points = Vec::new();

        for theta in self.conic_roots(&conic) {
            let world = self.ellipse.point_at_angle(EdaAngle::new(theta, RADIANS_T));

            if a_treat_as_line || a_seg.contains(world) {
                points.push(world);
            }
        }

        dedupe_points(&mut points);
        points
    }
}

impl Shape for ShapeEllipse {
    fn type_(&self) -> ShapeType {
        ShapeType::Ellipse
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::Ellipse(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    /// Port of `SHAPE_ELLIPSE::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;

        if !self.is_arc {
            let cos2 = self.cos_rot * self.cos_rot;
            let sin2 = self.sin_rot * self.sin_rot;

            let dx = (a * a * cos2 + b * b * sin2).sqrt();
            let dy = (a * a * sin2 + b * b * cos2).sqrt();

            let idx = cmath::f64_to_i32_x86(dx.ceil()) + a_clearance;
            let idy = cmath::f64_to_i32_x86(dy.ceil()) + a_clearance;

            return Box2I::new(
                Vector2I::new(self.ellipse.center.x - idx, self.ellipse.center.y - idy),
                Vector2I::new(2 * idx, 2 * idy),
            );
        }

        let eval = |theta: f64| -> Vector2D {
            let ct = cmath::cos(theta);
            let st = cmath::sin(theta);
            Vector2D::new(
                a * ct * self.cos_rot - b * st * self.sin_rot,
                a * ct * self.sin_rot + b * st * self.cos_rot,
            )
        };

        let p0 = eval(self.ellipse.start_angle.as_radians());
        let p1 = eval(self.ellipse.end_angle.as_radians());

        use crate::math::util::{std_max, std_min};

        let mut min_x = std_min(p0.x, p1.x);
        let mut max_x = std_max(p0.x, p1.x);
        let mut min_y = std_min(p0.y, p1.y);
        let mut max_y = std_max(p0.y, p1.y);

        let theta_x = cmath::atan2(-b * self.sin_rot, a * self.cos_rot);
        let theta_y = cmath::atan2(b * self.cos_rot, a * self.sin_rot);

        let candidates = [theta_x, theta_x + PI, theta_y, theta_y + PI];

        for c in candidates {
            if !self.is_angle_in_sweep(c) {
                continue;
            }

            let p = eval(c);
            min_x = std_min(min_x, p.x);
            max_x = std_max(max_x, p.x);
            min_y = std_min(min_y, p.y);
            max_y = std_max(max_y, p.y);
        }

        let i_min_x = cmath::f64_to_i32_x86(min_x.floor()) - a_clearance;
        let i_max_x = cmath::f64_to_i32_x86(max_x.ceil()) + a_clearance;
        let i_min_y = cmath::f64_to_i32_x86(min_y.floor()) - a_clearance;
        let i_max_y = cmath::f64_to_i32_x86(max_y.ceil()) + a_clearance;

        Box2I::new(
            Vector2I::new(self.ellipse.center.x + i_min_x, self.ellipse.center.y + i_min_y),
            Vector2I::new(i_max_x - i_min_x, i_max_y - i_min_y),
        )
    }

    /// Port of `SHAPE_ELLIPSE::Collide( const SEG& aSeg, int aClearance, int* aActual,
    /// VECTOR2I* aLocation )`.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if a_seg.a == a_seg.b {
            let d_sq = self.squared_distance(a_seg.a, false);
            let clear_sq = a_clearance as Ecoord * a_clearance as Ecoord;

            if d_sq == 0 || d_sq < clear_sq {
                if let Some(a) = a_actual {
                    *a = round_to_i32((d_sq as f64).sqrt());
                }

                if let Some(l) = a_location {
                    *l = a_seg.a;
                }

                return true;
            }

            return false;
        }

        let aloc = self.to_local(a_seg.a);
        let bloc = self.to_local(a_seg.b);
        let d = Vector2D::new(bloc.x - aloc.x, bloc.y - aloc.y);

        let a = self.ellipse.major_radius as f64;
        let b = self.ellipse.minor_radius as f64;
        let a_sq = a * a;
        let b_sq = b * b;

        let alpha = d.x * d.x / a_sq + d.y * d.y / b_sq;
        let beta = 2.0 * (aloc.x * d.x / a_sq + aloc.y * d.y / b_sq);
        let gamma = aloc.x * aloc.x / a_sq + aloc.y * aloc.y / b_sq - 1.0;

        // A is inside the closed ellipse if gamma < 0, B is inside if alpha + beta + gamma < 0
        let val_b = alpha + beta + gamma;

        if !self.is_arc {
            if gamma <= 0.0 {
                if let Some(a) = a_actual {
                    *a = 0;
                }

                if let Some(l) = a_location {
                    *l = a_seg.a;
                }

                return true;
            }

            if val_b <= 0.0 {
                if let Some(a) = a_actual {
                    *a = 0;
                }

                if let Some(l) = a_location {
                    *l = a_seg.b;
                }

                return true;
            }
        }

        // disc < 0 means the line misses the ellipse
        let disc = beta * beta - 4.0 * alpha * gamma;

        if disc >= 0.0 && alpha > 0.0 {
            let sqrt_disc = disc.sqrt();
            let two_alpha = 2.0 * alpha;
            let t0 = (-beta - sqrt_disc) / two_alpha;
            let t1 = (-beta + sqrt_disc) / two_alpha;

            for t in [t0, t1] {
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }

                let hit = Vector2D::new(aloc.x + t * d.x, aloc.y + t * d.y);

                // For arcs, the intersection must be within the angular sweep.
                if self.is_arc {
                    let angle = cmath::atan2(hit.y / b, hit.x / a);

                    if !self.is_angle_in_sweep(angle) {
                        continue;
                    }
                }

                if let Some(a) = a_actual {
                    *a = 0;
                }

                if let Some(l) = a_location {
                    *l = self.to_world(hit);
                }

                return true;
            }
        }

        let mut min_dist_sq = f64::MAX;
        let mut best_on_segment = Vector2D::new(0.0, 0.0);

        {
            let d_a = self.squared_distance(a_seg.a, true) as f64;
            let d_b = self.squared_distance(a_seg.b, true) as f64;

            if d_a < min_dist_sq {
                min_dist_sq = d_a;
                best_on_segment = aloc;
            }

            if d_b < min_dist_sq {
                min_dist_sq = d_b;
                best_on_segment = bloc;
            }
        }

        let d_dot_d = d.x * d.x + d.y * d.y;

        let mut project = |ex: f64, ey: f64| {
            // Orthogonal projection of (ex, ey) onto the segment
            let p_dot_d = (ex - aloc.x) * d.x + (ey - aloc.y) * d.y;
            let t = std_clamp(p_dot_d / d_dot_d, 0.0, 1.0);
            let qx = aloc.x + t * d.x;
            let qy = aloc.y + t * d.y;

            let dist_sq = (ex - qx) * (ex - qx) + (ey - qy) * (ey - qy);

            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                best_on_segment = Vector2D::new(qx, qy);
            }
        };

        // Check where ellipse outline runs parallel to the segment
        if d_dot_d > 0.0 {
            let theta0 = cmath::atan2(-b * d.x, a * d.y);

            for theta in [theta0, theta0 + PI] {
                if self.is_arc && !self.is_angle_in_sweep(theta) {
                    continue;
                }

                project(a * cmath::cos(theta), b * cmath::sin(theta));
            }
        }

        // Arc endpoints projected onto segment.
        if self.is_arc && d_dot_d > 0.0 {
            for end_angle in [self.ellipse.start_angle, self.ellipse.end_angle] {
                let angle_rad = end_angle.as_radians();
                project(a * cmath::cos(angle_rad), b * cmath::sin(angle_rad));
            }
        }

        // Clearance comparison
        let threshold_sq = a_clearance as f64 * a_clearance as f64;

        if min_dist_sq > 0.0 && min_dist_sq >= threshold_sq {
            return false;
        }

        if let Some(a) = a_actual {
            *a = round_to_i32(min_dist_sq.sqrt());
        }

        if let Some(l) = a_location {
            *l = self.to_world(best_on_segment);
        }

        true
    }

    /// Port of `SHAPE_ELLIPSE::TransformToPolygon()`: the tessellation points sit on the true
    /// ellipse curve (the error location is unused); an elliptical arc adds nothing.
    fn transform_to_polygon(&self, a_buffer: &mut ShapePolySet, a_error: i32, _a_error_loc: ErrorLoc) {
        if self.is_arc {
            return;
        }

        let mut chain = self.convert_to_polyline(a_error);
        chain.set_closed(true);
        a_buffer.add_outline(&chain);
    }

    /// Port of `SHAPE_ELLIPSE::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        rotate_point_around(&mut self.ellipse.center, a_center, a_angle);
        self.ellipse.rotation -= a_angle;
        self.update_cache();
    }

    /// Port of `SHAPE_ELLIPSE::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        self.ellipse.center += a_vector;
    }

    /// Port of `SHAPE_ELLIPSE::IsSolid()`.
    fn is_solid(&self) -> bool {
        !self.is_arc
    }

    /// Port of `SHAPE_ELLIPSE::PointInside()`: no interior for elliptical arcs.
    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, _a_use_bbox_cache: bool) -> bool {
        // No interior for elliptical arcs. Open curve
        if self.is_arc {
            return false;
        }

        let dx = a_pt.x as f64 - self.ellipse.center.x as f64;
        let dy = a_pt.y as f64 - self.ellipse.center.y as f64;
        let lx = dx * self.cos_rot + dy * self.sin_rot;
        let ly = -dx * self.sin_rot + dy * self.cos_rot;

        if a_accuracy > 0 {
            // Increase both radii by aAccuracy.
            let a = self.ellipse.major_radius as f64 + a_accuracy as f64;
            let b = self.ellipse.minor_radius as f64 + a_accuracy as f64;
            return (lx * lx) / (a * a) + (ly * ly) / (b * b) < 1.0;
        }

        lx * lx * self.inv_major_r_sq + ly * ly * self.inv_minor_r_sq < 1.0
    }

    /// Port of `SHAPE_ELLIPSE::SquaredDistance()`.
    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        let local = self.to_local(a_p);

        // Interior of a closed ellipse if val < 1
        if !self.is_arc && !a_outline_only {
            let val = local.x * local.x * self.inv_major_r_sq + local.y * local.y * self.inv_minor_r_sq;

            if val <= 1.0 {
                return 0;
            }
        }

        let closest = self.closest_local_point(local);
        let dx_e = closest.x - local.x;
        let dy_e = closest.y - local.y;

        cmath::f64_to_i64_x86(dx_e * dx_e + dy_e * dy_e)
    }

    /// Port of `SHAPE_ELLIPSE::Format()`.
    fn format(&self, a_cplusplus: bool) -> String {
        let e = &self.ellipse;
        let mut ss = String::new();

        if a_cplusplus {
            ss += &format!(
                "SHAPE_ELLIPSE( VECTOR2I( {}, {} ), {}, {}, EDA_ANGLE( {}, DEGREES_T )",
                e.center.x,
                e.center.y,
                e.major_radius,
                e.minor_radius,
                format_double(e.rotation.as_degrees())
            );

            if self.is_arc {
                ss += &format!(
                    ", EDA_ANGLE( {}, DEGREES_T ), EDA_ANGLE( {}, DEGREES_T )",
                    format_double(e.start_angle.as_degrees()),
                    format_double(e.end_angle.as_degrees())
                );
            }

            ss += " );";
        } else {
            ss += &format!(
                "{} {} {} {} {} {} {}",
                format_shape_base(self.type_()),
                e.center.x,
                e.center.y,
                e.major_radius,
                e.minor_radius,
                format_double(e.rotation.as_degrees()),
                if self.is_arc { 1 } else { 0 }
            );

            if self.is_arc {
                ss += &format!(
                    " {} {}",
                    format_double(e.start_angle.as_degrees()),
                    format_double(e.end_angle.as_degrees())
                );
            }
        }

        ss
    }
}
