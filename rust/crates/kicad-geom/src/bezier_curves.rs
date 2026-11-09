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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.
//
// Portions of this code are based draw2d
// Copyright (c) 2010, Laurent Le Goff
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:
//
// 	* Redistributions of source code must retain the above copyright notice,
// 		this list of conditions and the following disclaimer.
// 	* Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following
//  		disclaimer in the documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
// IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY,
// OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA,
// OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
// STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE,
// EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//
// Portions of this code are base on BezierKit
// Copyright (c) 2017 Holmes Futrell
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
// Portions of this code are based on the spline-research project
// Copyright 2018 Raph Levien
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Port of `libs/kimath/include/bezier_curves.h` and `libs/kimath/src/bezier_curves.cpp`:
//! `BEZIER_POLY` (Bezier to polyline conversion, based on "Fast, Precise Flattening of Cubic
//! Bezier segments offset Curves" by Hain, et. al.), the generic cubic `BEZIER<T>` and
//! `TransformEllipseToBeziers`.
//!
//! The templates are instantiated for `int` and `double` (trait [`NumericType`]); the C++
//! implicit conversions between `VECTOR2<int>` and `VECTOR2D` are spelled out by the trait.
//! The `wxLogTrace` calls (trace mask `"bezier"`) are not ported.

use core::fmt::Debug;

use crate::cmath;
use crate::geometry::eda_angle::{ANGLE_0, ANGLE_360, EdaAngle, RADIANS_T};
use crate::geometry::ellipse::Ellipse;
use crate::math::util::ki_round;
use crate::math::vector2::{Vector2, Vector2D, Vector2I};
use crate::trigo::rotate_point;

/// The coordinate types `BEZIER<T>` and `ELLIPSE<T>` are instantiated for (`int`, `double`),
/// with the conversions and the arithmetic the C++ templates perform on them.
pub trait NumericType: Copy + Default + Debug + PartialEq + 'static {
    /// `std::is_integral_v<NumericType>`
    const IS_INTEGRAL: bool;

    /// `static_cast<double>( v )`
    fn to_f64(self) -> f64;

    /// `NumericType( v )` of a `double`: truncation for `int` (the reference build gives
    /// `INT_MIN` out of range, see `PORTING.md` section 9).
    fn from_f64(v: f64) -> Self;

    /// The implicit `VECTOR2<T>( const VECTOR2D& )` conversion: for `int` clamped to the
    /// `int` range and truncated.
    fn vec_from_d(v: Vector2D) -> Vector2<Self>;

    /// The implicit `VECTOR2D( const VECTOR2<T>& )` conversion.
    fn vec_to_d(v: Vector2<Self>) -> Vector2D {
        Vector2D::new(v.x.to_f64(), v.y.to_f64())
    }

    /// `VECTOR2<T> - VECTOR2<T>` (wrapping for `int`, like the reference build).
    fn vec_sub(a: Vector2<Self>, b: Vector2<Self>) -> Vector2<Self>;

    /// `VECTOR2<T> + VECTOR2<T>` (wrapping for `int`, like the reference build).
    fn vec_add(a: Vector2<Self>, b: Vector2<Self>) -> Vector2<Self>;

    /// `VECTOR2<T>::EuclideanNorm()`.
    fn vec_norm(v: Vector2<Self>) -> Self;

    /// `2 * aRef - aValue` (`ELLIPSE::Mirror`).
    fn mirror(a_ref: Self, a_value: Self) -> Self;
}

impl NumericType for i32 {
    const IS_INTEGRAL: bool = true;

    fn to_f64(self) -> f64 {
        self as f64
    }

    fn from_f64(v: f64) -> Self {
        cmath::f64_to_i32_x86(v)
    }

    fn vec_from_d(v: Vector2D) -> Vector2I {
        Vector2I::from_vec(v)
    }

    fn vec_sub(a: Vector2I, b: Vector2I) -> Vector2I {
        Vector2I::new(a.x.wrapping_sub(b.x), a.y.wrapping_sub(b.y))
    }

    fn vec_add(a: Vector2I, b: Vector2I) -> Vector2I {
        Vector2I::new(a.x.wrapping_add(b.x), a.y.wrapping_add(b.y))
    }

    fn vec_norm(v: Vector2I) -> Self {
        v.euclidean_norm()
    }

    fn mirror(a_ref: Self, a_value: Self) -> Self {
        a_ref.wrapping_mul(2).wrapping_sub(a_value)
    }
}

impl NumericType for f64 {
    const IS_INTEGRAL: bool = false;

    fn to_f64(self) -> f64 {
        self
    }

    fn from_f64(v: f64) -> Self {
        v
    }

    fn vec_from_d(v: Vector2D) -> Vector2D {
        v
    }

    fn vec_sub(a: Vector2D, b: Vector2D) -> Vector2D {
        a - b
    }

    fn vec_add(a: Vector2D, b: Vector2D) -> Vector2D {
        a + b
    }

    fn vec_norm(v: Vector2D) -> Self {
        v.euclidean_norm()
    }

    fn mirror(a_ref: Self, a_value: Self) -> Self {
        2.0 * a_ref - a_value
    }
}

/// `std::min( std::max( t, 0.0 ), 1.0 )` with the libstdc++ comparisons.
#[inline]
fn clamp01(t: f64) -> f64 {
    let m = if t < 0.0 { 0.0 } else { t };
    if 1.0 < m { 1.0 } else { m }
}

/// Port of `BEZIER_POLY`: Bezier curves to polygon converter.
///
/// Only quadratic and cubic Bezier curves are handled.
#[derive(Clone, Debug, PartialEq)]
pub struct BezierPoly {
    /// `m_minSegLen` (stored, never read by the C++ either)
    min_seg_len: f64,

    /// Control points
    ctrl_pts: Vec<Vector2D>,
}

impl BezierPoly {
    /// `BEZIER_POLY( const VECTOR2I& aStart, const VECTOR2I& aCtrl1, const VECTOR2I& aCtrl2,
    /// const VECTOR2I& aEnd )`.
    pub fn new(a_start: Vector2I, a_ctrl1: Vector2I, a_ctrl2: Vector2I, a_end: Vector2I) -> Self {
        BezierPoly {
            min_seg_len: 0.0,
            ctrl_pts: vec![
                Vector2D::from(a_start),
                Vector2D::from(a_ctrl1),
                Vector2D::from(a_ctrl2),
                Vector2D::from(a_end),
            ],
        }
    }

    /// `BEZIER_POLY( const std::vector<VECTOR2I>& aControlPoints )`.
    pub fn from_points_i(a_control_points: &[Vector2I]) -> Self {
        BezierPoly {
            min_seg_len: 0.0,
            ctrl_pts: a_control_points.iter().map(|&p| Vector2D::from(p)).collect(),
        }
    }

    /// `BEZIER_POLY( const std::vector<VECTOR2D>& aControlPoints )`.
    pub fn from_points_d(a_control_points: Vec<Vector2D>) -> Self {
        BezierPoly {
            min_seg_len: 0.0,
            ctrl_pts: a_control_points,
        }
    }

    /// The control points.
    pub fn ctrl_pts(&self) -> &[Vector2D] {
        &self.ctrl_pts
    }

    /// `m_minSegLen`.
    pub fn min_seg_len(&self) -> f64 {
        self.min_seg_len
    }

    /// Port of `BEZIER_POLY::isNaN`.
    fn is_nan(&self) -> bool {
        self.ctrl_pts.iter().any(|pt| pt.x.is_nan() || pt.y.is_nan())
    }

    /// Port of `BEZIER_POLY::isFlat`.
    fn is_flat(&self, a_max_error: f64) -> bool {
        let p = &self.ctrl_pts;

        if p.len() == 3 {
            let d21: Vector2D = p[1] - p[0];
            let d31: Vector2D = p[2] - p[0];

            let t: f64 = d21.dot(d31) / d31.squared_euclidean_norm();
            let u: f64 = clamp01(t);
            let pp: Vector2D = p[0] + u * d31;
            let delta: Vector2D = p[1] - pp;

            return 0.5 * delta.euclidean_norm() <= a_max_error;
        } else if p.len() == 4 {
            let delta: Vector2D = p[3] - p[0];

            let d21: Vector2D = p[1] - p[0];
            let d31: Vector2D = p[2] - p[0];

            let cross1: f64 = delta.cross(d21);
            let cross2: f64 = delta.cross(d31);

            let inv_delta_sq: f64 = 1.0 / delta.squared_euclidean_norm();
            let d1: f64 = (cross1 * cross1) * inv_delta_sq;
            let d2: f64 = (cross2 * cross2) * inv_delta_sq;

            let factor: f64 = if cross1 * cross2 > 0.0 {
                3.0 / 4.0
            } else {
                4.0 / 9.0
            };
            let f2: f64 = factor * factor;
            let tol: f64 = a_max_error * a_max_error;

            return d1 * f2 <= tol && d2 * f2 <= tol;
        }

        debug_assert!(false, "BEZIER_POLY::isFlat: not a quadratic or cubic curve");
        false
    }

    /// Port of `BEZIER_POLY::GetPoly( std::vector<VECTOR2I>& aOutput, int aMaxError = 10 )`.
    ///
    /// Convert a Bezier curve to a polygon.  `a_output` is cleared first; `a_max_error` is
    /// the maximum error in IU between the curve and the polygon.
    pub fn get_poly_i(&self, a_output: &mut Vec<Vector2I>, a_max_error: i32) {
        a_output.clear();
        let mut buffer: Vec<Vector2D> = Vec::new();
        self.get_poly_d(&mut buffer, a_max_error as f64);

        a_output.reserve(buffer.len());

        for pt in &buffer {
            a_output.push(Vector2I::new(ki_round(pt.x), ki_round(pt.y)));
        }
    }

    /// Port of `BEZIER_POLY::GetPoly( std::vector<VECTOR2D>& aOutput, double aMaxError = 10.0 )`.
    ///
    /// Unlike the `int` overload this one appends to `a_output` without clearing it, as in
    /// C++.  A non-positive `a_max_error` means 10.
    pub fn get_poly_d(&self, a_output: &mut Vec<Vector2D>, a_max_error: f64) {
        let mut max_error = a_max_error;

        if max_error <= 0.0 {
            max_error = 10.0;
        }

        if self.ctrl_pts.len() == 3 {
            self.get_quad_poly(a_output, max_error);
        } else if self.ctrl_pts.len() == 4 {
            self.get_cubic_poly(a_output, max_error);
        } else {
            debug_assert!(false, "BEZIER_POLY::GetPoly: not a quadratic or cubic curve");
        }
    }

    /// Convenience: [`BezierPoly::get_poly_i`] into a new vector.
    pub fn poly_i(&self, a_max_error: i32) -> Vec<Vector2I> {
        let mut out = Vec::new();
        self.get_poly_i(&mut out, a_max_error);
        out
    }

    /// Port of `BEZIER_POLY::eval`.
    fn eval(&self, t: f64) -> Vector2D {
        let p = &self.ctrl_pts;
        let omt: f64 = 1.0 - t;
        let omt2: f64 = omt * omt;

        if p.len() == 3 {
            omt2 * p[0] + 2.0 * omt * t * p[1] + t * t * p[2]
        } else if p.len() == 4 {
            let omt3: f64 = omt * omt2;
            let t2: f64 = t * t;
            let t3: f64 = t * t2;
            omt3 * p[0] + 3.0 * t * omt2 * p[1] + 3.0 * t2 * omt * p[2] + t3 * p[3]
        } else {
            debug_assert!(false, "BEZIER_POLY::eval: not a quadratic or cubic curve");
            Vector2D::new(0.0, 0.0)
        }
    }

    /// Port of `BEZIER_POLY::getQuadPoly`.
    fn get_quad_poly(&self, a_output: &mut Vec<Vector2D>, a_max_error: f64) {
        let p = &self.ctrl_pts;
        let ddx: f64 = 2.0 * p[1].x - p[0].x - p[2].x;
        let ddy: f64 = 2.0 * p[1].y - p[0].y - p[2].y;
        let u0: f64 = (p[1].x - p[0].x) * ddx + (p[1].y - p[0].y) * ddy;
        let u2: f64 = (p[2].x - p[1].x) * ddx + (p[2].y - p[1].y) * ddy;
        let cross: f64 = (p[2].x - p[0].x) * ddy - (p[2].y - p[0].y) * ddx;
        let x0: f64 = u0 / cross;
        let x2: f64 = u2 / cross;
        let scale: f64 = cross.abs() / (cmath::hypot(ddx, ddy) * (x2 - x0).abs());

        let a0: f64 = approx_int(x0);
        let a2: f64 = approx_int(x2);

        // `int n = std::ceil( ... )`: NaN / infinity (collinear control points) give INT_MIN on
        // the reference build, so no intermediate point is generated
        let n: i32 = cmath::f64_to_i32_x86(
            (0.5 * (a2 - a0).abs() * (scale / a_max_error).sqrt()).ceil(),
        );

        let v0: f64 = approx_inv_int(a0);
        let v2: f64 = approx_inv_int(a2);

        a_output.push(p[0]);

        for ii in 0..n.max(0) {
            let u: f64 = approx_inv_int(a0 + (a2 - a0) * ii as f64 / n as f64);
            let t: f64 = (u - v0) / (v2 - v0);
            a_output.push(self.eval(t));
        }

        a_output.push(p[2]);
    }

    /// Port of `BEZIER_POLY::numberOfInflectionPoints`.
    fn number_of_inflection_points(&self) -> i32 {
        let p = &self.ctrl_pts;
        let d21: Vector2D = p[1] - p[0];
        let d32: Vector2D = p[2] - p[1];
        let d43: Vector2D = p[3] - p[2];

        let cross1: f64 = d21.cross(d32) * d32.cross(d43);
        let cross2: f64 = d21.cross(d32) * d21.cross(d43);

        if cross1 < 0.0 {
            return 1;
        } else if cross2 > 0.0 {
            return 0;
        }

        let b1: bool = d21.dot(d32) > 0.0;
        let b2: bool = d32.dot(d43) > 0.0;

        if b1 ^ b2 {
            return 0;
        }

        // These are rare cases where there are potentially 2 or 0 inflection points.
        -1
    }

    /// Port of `BEZIER_POLY::thirdControlPointDeviation`.
    fn third_control_point_deviation(&self) -> f64 {
        let p = &self.ctrl_pts;
        let delta: Vector2D = p[1] - p[0];
        let len_sq: f64 = delta.squared_euclidean_norm();

        if len_sq < 1e-6 {
            return 0.0;
        }

        let len: f64 = len_sq.sqrt();
        let r: f64 = (p[1].y - p[0].y) / len;
        let s: f64 = (p[0].x - p[1].x) / len;
        let u: f64 = (p[1].x * p[0].y - p[0].x * p[1].y) / len;

        (r * p[2].x + s * p[2].y + u).abs()
    }

    /// Port of `BEZIER_POLY::subdivide` (cubic curves; `aLeft` / `aRight` are 4-point curves
    /// in every C++ call).
    fn subdivide(&self, a_t: f64) -> (BezierPoly, BezierPoly) {
        let p = &self.ctrl_pts;
        let mut left = BezierPoly::from_points_d(vec![Vector2D::default(); 4]);
        let mut right = BezierPoly::from_points_d(vec![Vector2D::default(); 4]);

        if p.len() == 3 {
            left.ctrl_pts[0] = p[0];
            left.ctrl_pts[1] = p[0] + a_t * (p[1] - p[0]);
            left.ctrl_pts[2] = self.eval(a_t);

            right.ctrl_pts[2] = p[2];
            right.ctrl_pts[1] = p[1] + a_t * (p[2] - p[1]);
            right.ctrl_pts[0] = left.ctrl_pts[2];
        } else if p.len() == 4 {
            let left_ctrl1: Vector2D = p[0] + a_t * (p[1] - p[0]);
            let tmp: Vector2D = p[1] + a_t * (p[2] - p[1]);
            let left_ctrl2: Vector2D = left_ctrl1 + a_t * (tmp - left_ctrl1);
            let right_ctrl2: Vector2D = p[2] + a_t * (p[3] - p[2]);
            let right_ctrl1: Vector2D = tmp + a_t * (right_ctrl2 - tmp);
            let shared: Vector2D = left_ctrl2 + a_t * (right_ctrl1 - left_ctrl2);

            left.ctrl_pts[0] = p[0];
            left.ctrl_pts[1] = left_ctrl1;
            left.ctrl_pts[2] = left_ctrl2;
            left.ctrl_pts[3] = shared;

            right.ctrl_pts[3] = p[3];
            right.ctrl_pts[2] = right_ctrl2;
            right.ctrl_pts[1] = right_ctrl1;
            right.ctrl_pts[0] = shared;
        } else {
            debug_assert!(false, "BEZIER_POLY::subdivide: not a quadratic or cubic curve");
        }

        (left, right)
    }

    /// Port of `BEZIER_POLY::recursiveSegmentation` (iterative, with an explicit stack).
    fn recursive_segmentation(&self, a_output: &mut Vec<Vector2D>, a_threshhold: f64) {
        let mut stack: Vec<BezierPoly> = vec![self.clone()];

        while let Some(bezier) = stack.last_mut() {
            if bezier.ctrl_pts[3] == bezier.ctrl_pts[0] {
                // dropping zero length segment
                stack.pop();
            } else if bezier.is_flat(a_threshhold) {
                a_output.push(bezier.ctrl_pts[3]);
                stack.pop();
            } else {
                let (left, right) = bezier.subdivide(0.5);
                *bezier = right;
                stack.push(left);
            }
        }
    }

    /// Port of `BEZIER_POLY::findInflectionPoints`: returns the count and `aT1`, `aT2`.
    fn find_inflection_points(&self) -> (i32, f64, f64) {
        let p = &self.ctrl_pts;
        let aa = Vector2D::new(
            -p[0].x + 3.0 * p[1].x - 3.0 * p[2].x + p[3].x,
            -p[0].y + 3.0 * p[1].y - 3.0 * p[2].y + p[3].y,
        );
        let bb = Vector2D::new(
            3.0 * p[0].x - 6.0 * p[1].x + 3.0 * p[2].x,
            3.0 * p[0].y - 6.0 * p[1].y + 3.0 * p[2].y,
        );
        let cc = Vector2D::new(-3.0 * p[0].x + 3.0 * p[1].x, -3.0 * p[0].y + 3.0 * p[1].y);

        let a: f64 = 3.0 * aa.cross(bb);
        let b: f64 = 3.0 * aa.cross(cc);
        let c: f64 = bb.cross(cc);

        // Solve the quadratic equation a*t^2 + b*t + c = 0
        let r2: f64 = b * b - 4.0 * a * c;

        if r2 >= 0.0 && a != 0.0 {
            let r: f64 = r2.sqrt();

            let mut t1: f64 = (-b + r) / (2.0 * a);
            let mut t2: f64 = (-b - r) / (2.0 * a);

            if (t1 > 0.0 && t1 < 1.0) && (t2 > 0.0 && t2 < 1.0) {
                if t1 > t2 {
                    core::mem::swap(&mut t1, &mut t2);
                }

                if t2 - t1 > 0.00001 {
                    return (2, t1, t2);
                } else {
                    return (1, t1, t2);
                }
            } else if t1 > 0.0 && t1 < 1.0 {
                return (1, t1, 0.0);
            } else if t2 > 0.0 && t2 < 1.0 {
                return (1, t2, 0.0);
            }

            return (0, 0.0, 0.0);
        }

        (0, 0.0, 0.0)
    }

    /// Port of `BEZIER_POLY::cubicParabolicApprox`.
    ///
    /// The C++ ping-pongs between two pairs of preallocated curves so that the curve being
    /// split is never overwritten by its own halves; owned values do the same here.
    fn cubic_parabolic_approx(&self, a_output: &mut Vec<Vector2D>, a_max_error: f64) {
        let mut c: BezierPoly = self.clone();

        loop {
            if c.is_nan() {
                break;
            }

            if c.is_flat(a_max_error) {
                // If the subsegment deviation satisfies the flatness criterion, store the last
                // point and stop
                a_output.push(c.ctrl_pts[3]);
                break;
            }

            // Find the third control point deviation and the t values for subdivision
            let d: f64 = c.third_control_point_deviation();

            // A vanishing deviation yields no valid split parameter, so subdivide recursively
            // rather than dividing by zero
            if d == 0.0 {
                c.recursive_segmentation(a_output, a_max_error);
                break;
            }

            let t: f64 = 2.0 * (a_max_error / (3.0 * d)).sqrt(); // Forumla 2 in Hain et al.

            if t > 1.0 {
                // Case where the t value calculated is invalid, so use recursive subdivision
                c.recursive_segmentation(a_output, a_max_error);
                break;
            }

            // Valid t value to subdivide at that calculated value
            let (b1, b2) = c.subdivide(t);

            // First subsegment should have its deviation equal to flatness
            if b1.is_flat(a_max_error) {
                a_output.push(b1.ctrl_pts[3]);
            } else {
                // if not then use segment to handle any mathematical errors
                b1.recursive_segmentation(a_output, a_max_error);
            }

            // Repeat the process for the left over subsegment
            c = b2;
        }
    }

    /// Port of `BEZIER_POLY::getCubicPoly`.
    fn get_cubic_poly(&self, a_output: &mut Vec<Vector2D>, a_max_error: f64) {
        a_output.push(self.ctrl_pts[0]);

        if self.number_of_inflection_points() == 0 {
            // If no inflection points then apply PA on the full Bezier segment.
            self.cubic_parabolic_approx(a_output, a_max_error);
            return;
        }

        // If one or more inflection points then we will have to subdivide the curve
        let (num_of_ifp, t1, _t2) = self.find_inflection_points();

        if num_of_ifp == 2 {
            // Case when 2 inflection points then divide at the smallest one first
            let (sub1, tmp1) = self.subdivide(t1);

            // Use PA for first subsegment
            sub1.cubic_parabolic_approx(a_output, a_max_error);

            // Now find the second inflection point in the second curve and subdivide
            let (num_of_ifp, t1, _t2) = tmp1.find_inflection_points();

            if num_of_ifp != 2 && num_of_ifp != 1 {
                // 2nd inflection point not found
                a_output.push(tmp1.ctrl_pts[3]);
                return;
            }

            let (sub2, sub3) = tmp1.subdivide(t1);

            // Use Segment for the second (middle) subsegment
            sub2.recursive_segmentation(a_output, a_max_error);

            // Use PA for the third curve
            sub3.cubic_parabolic_approx(a_output, a_max_error);
        } else if num_of_ifp == 1 {
            // Case where there is one inflection point, subdivide once and use PA on both
            // subsegments
            let (sub1, sub2) = self.subdivide(t1);
            sub1.cubic_parabolic_approx(a_output, a_max_error);
            sub2.cubic_parabolic_approx(a_output, a_max_error);
        } else {
            // Case where there is no inflection, use PA directly
            self.cubic_parabolic_approx(a_output, a_max_error);
        }
    }
}

/// `approx_int`
fn approx_int(x: f64) -> f64 {
    const D: f64 = 0.6744897501960817;
    let d4: f64 = D * D * D * D;
    x / (1.0 - D + cmath::pow(d4 + x * x * 0.25, 0.25))
}

/// `approx_inv_int`
fn approx_inv_int(x: f64) -> f64 {
    const P: f64 = 0.39538816;
    x * (1.0 - P + (P * P + 0.25 * x * x).sqrt())
}

/// Port of `BEZIER<NumericType>`: generic cubic Bezier representation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bezier<T> {
    /// `Start`
    pub start: Vector2<T>,
    /// `C1`
    pub c1: Vector2<T>,
    /// `C2`
    pub c2: Vector2<T>,
    /// `End`
    pub end: Vector2<T>,
}

impl<T: NumericType> Bezier<T> {
    /// `BEZIER( aStart, aC1, aC2, aEnd )`.
    pub const fn new(a_start: Vector2<T>, a_c1: Vector2<T>, a_c2: Vector2<T>, a_end: Vector2<T>) -> Self {
        Bezier {
            start: a_start,
            c1: a_c1,
            c2: a_c2,
            end: a_end,
        }
    }

    /// `BEZIER<T>( { VECTOR2D, VECTOR2D, VECTOR2D, VECTOR2D } )`: construction from double
    /// points with the implicit `VECTOR2<T>` conversion.
    pub fn from_d(a_start: Vector2D, a_c1: Vector2D, a_c2: Vector2D, a_end: Vector2D) -> Self {
        Bezier::new(
            T::vec_from_d(a_start),
            T::vec_from_d(a_c1),
            T::vec_from_d(a_c2),
            T::vec_from_d(a_end),
        )
    }

    /// Port of `BEZIER::PointAt`.
    ///
    /// Evaluate the Bezier curve at a given t value.  `a_t` doesn't have to be in the range
    /// [0, 1], but if it's not, the point will not be on the curve.
    pub fn point_at(&self, a_t: f64) -> Vector2<T> {
        let t2: f64 = a_t * a_t;
        let t3: f64 = t2 * a_t;
        let t_m1: f64 = 1.0 - a_t;
        let t_m1_2: f64 = t_m1 * t_m1;
        let t_m1_3: f64 = t_m1_2 * t_m1;

        let r: Vector2D = (t_m1_3 * T::vec_to_d(self.start))
            + (3.0 * a_t * t_m1_2 * T::vec_to_d(self.c1))
            + (3.0 * t2 * t_m1 * T::vec_to_d(self.c2))
            + (t3 * T::vec_to_d(self.end));

        T::vec_from_d(r)
    }

    /// `a + aT * ( b - a )` with the C++ conversions: the difference in `T`, the rest in
    /// `double`, converted back to `T`.
    fn lerp(a: Vector2<T>, b: Vector2<T>, a_t: f64) -> Vector2<T> {
        let diff: Vector2D = a_t * T::vec_to_d(T::vec_sub(b, a));
        T::vec_from_d(T::vec_to_d(a) + diff)
    }

    /// Port of `BEZIER::Split`.
    ///
    /// Split the Bezier curve into two curves at `a_t` using de Casteljau subdivision; returns
    /// the curve from 0 to `a_t` and the curve from `a_t` to 1.
    pub fn split(&self, a_t: f64) -> (Bezier<T>, Bezier<T>) {
        let start_c1 = Self::lerp(self.start, self.c1, a_t);
        let c1_c2 = Self::lerp(self.c1, self.c2, a_t);
        let c2_end = Self::lerp(self.c2, self.end, a_t);
        let left_c2 = Self::lerp(start_c1, c1_c2, a_t);
        let right_c1 = Self::lerp(c1_c2, c2_end, a_t);
        let shared = Self::lerp(left_c2, right_c1, a_t);

        (
            Bezier::new(self.start, start_c1, left_c2, shared),
            Bezier::new(shared, right_c1, c2_end, self.end),
        )
    }

    /// Port of `BEZIER::SubCurve`.
    ///
    /// Extract a sub-curve from `a_t0` to `a_t1` using de Casteljau subdivision.
    pub fn sub_curve(&self, a_t0: f64, a_t1: f64) -> Bezier<T> {
        let mut t0 = a_t0;
        let mut t1 = a_t1;

        if t0 < 0.0 {
            t0 = 0.0;
        } else if t0 > 1.0 {
            t0 = 1.0;
        }

        if t1 < 0.0 {
            t1 = 0.0;
        } else if t1 > 1.0 {
            t1 = 1.0;
        }

        if t1 <= t0 {
            let point = self.point_at(t0);
            return Bezier::new(point, point, point, point);
        }

        if t0 == 0.0 && t1 == 1.0 {
            return *self;
        }

        if t0 == 0.0 {
            return self.split(t1).0;
        }

        if t1 == 1.0 {
            return self.split(t0).1;
        }

        let (left, _right) = self.split(t1);
        let (_unused, sub_curve) = left.split(t0 / t1);

        sub_curve
    }
}

/// Port of `TransformEllipseToBeziers<T>`.
///
/// Transforms an ellipse or elliptical arc into a set of Bezier curves that approximate it,
/// appended to `a_beziers`.
pub fn transform_ellipse_to_beziers<T: NumericType>(
    a_ellipse: &Ellipse<T>,
    a_beziers: &mut Vec<Bezier<T>>,
) {
    let mut arc_angle: EdaAngle = -(a_ellipse.end_angle - a_ellipse.start_angle);

    if arc_angle >= ANGLE_0 {
        arc_angle -= ANGLE_360;
    }

    // KiCad does not natively support ellipses or elliptical arcs.  So, we convert them to
    // Bezier splines as these are the nearest thing we have that represents them in a way
    // that is both editable and preserves their curvature accurately (enough).
    //
    // Credit to Kliment for developing and documenting this method.

    // Minimum number of Beziers to use for a full circle to keep error manageable.
    const MIN_BEZIERS_PER_CIRCLE: i32 = 4;

    // The number of Beziers needed for the given arc
    let num_beziers: i32 = cmath::f64_to_i32_x86(
        (arc_angle.as_radians() / (2.0 * core::f64::consts::PI / MIN_BEZIERS_PER_CIRCLE as f64))
            .abs()
            .ceil(),
    );

    // Angle occupied by each Bezier
    let angle_increment: f64 = arc_angle.as_radians() / num_beziers as f64;

    // Now, let's assume a circle of radius 1, centered on origin, with angle startangle
    // x-axis-aligned. We'll move, scale, and rotate it later. We're creating Bezier curves
    // that hug this circle as closely as possible, with the angles that will be used on the
    // final ellipse too.
    //
    // Thanks to the beautiful and excellent https://pomax.github.io/bezierinfo we know how to
    // define a curve that hugs a circle as closely as possible.
    //
    // We need the value k, which is the optimal distance from the endpoint to the control
    // point to make the curve match the circle for a given circle arc angle.
    //
    // k = 4/3 * tan(theta/4), where theta is the angle of the arc. In our case,
    // theta=angleIncrement
    let theta: f64 = angle_increment;
    let k: f64 = (4. / 3.) * cmath::tan(theta / 4.0);

    // Define our Bezier:
    // - Start point is on the circle at the x-axis
    // - First control point just uses k as the y-value
    // - Second control point is offset from the end point
    // - End point is defined by the angle of the arc segment
    // Note that we use double here no matter what the template param is; round at the end
    // only.
    let first: Bezier<f64> = Bezier::new(
        Vector2D::new(1.0, 0.0),
        Vector2D::new(1.0, k),
        Vector2D::new(
            cmath::cos(theta) + k * cmath::sin(theta),
            cmath::sin(theta) - k * cmath::cos(theta),
        ),
        Vector2D::new(cmath::cos(theta), cmath::sin(theta)),
    );

    // Now construct the actual segments by transforming/rotating the first one
    let transform_point = |a_point: Vector2D, a_angle: f64| -> Vector2D {
        let mut pt = a_point;

        // Bring to the actual starting angle
        rotate_point(
            &mut pt,
            -EdaAngle::new(a_angle - a_ellipse.start_angle.as_radians(), RADIANS_T),
        );

        // Then scale to the major and minor radiuses of the ellipse
        pt *= Vector2D::new(
            a_ellipse.major_radius.to_f64(),
            a_ellipse.minor_radius.to_f64(),
        );

        // Now rotate to the ellipse coordinate system
        rotate_point(&mut pt, -a_ellipse.rotation);

        // And finally offset to the center location of the ellipse
        pt += T::vec_to_d(a_ellipse.center);

        pt
    };

    for i in 0..num_beziers.max(0) {
        let angle = i as f64 * angle_increment;
        a_beziers.push(Bezier::from_d(
            transform_point(first.start, angle),
            transform_point(first.c1, angle),
            transform_point(first.c2, angle),
            transform_point(first.end, angle),
        ));
    }
}
