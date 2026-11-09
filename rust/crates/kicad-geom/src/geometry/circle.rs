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

//! Port of `libs/kimath/include/geometry/circle.h` and `libs/kimath/src/geometry/circle.cpp`.

use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::Seg;
use crate::geometry::shape::MIN_PRECISION_IU;
use crate::math::util::ki_round;
use crate::math::vector2::{Vector2D, Vector2I, Vector2L};
use crate::trigo::{calc_arc_mid, rotate_point};

/// Port of `CIRCLE`: basic circle geometry with utility geometry functions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Circle {
    /// `Radius`, public to make access simpler
    pub radius: i32,
    /// `Center`, public to make access simpler
    pub center: Vector2I,
}

impl Circle {
    /// Port of `CIRCLE( const VECTOR2I& aCenter, int aRadius )`.
    pub const fn new(a_center: Vector2I, a_radius: i32) -> Self {
        Circle {
            radius: a_radius,
            center: a_center,
        }
    }

    /// Port of `CIRCLE::ConstructFromTanTanPt()`.
    ///
    /// Construct this circle such that it is tangent to the given segments and passes through
    /// the given point, generating the solution which can be used to fillet both segments.
    /// The caller is responsible for ensuring it is providing a solvable problem (the C++
    /// `wxCHECK_MSG`s return early, leaving the circle partially updated; so does the port).
    pub fn construct_from_tan_tan_pt(
        &mut self,
        a_line_a: &Seg,
        a_line_b: &Seg,
        a_p: Vector2I,
    ) -> &mut Self {
        //fixme: There might be more efficient / accurate solution than using geometrical constructs

        let mut anglebisector = Seg::default();
        let intersect_point: Vector2I;

        let furthest_from_intersect =
            |ip: Vector2I, a_pt1: Vector2I, a_pt2: Vector2I| -> Vector2I {
                if (a_pt1 - ip).euclidean_norm() > (a_pt2 - ip).euclidean_norm() {
                    a_pt1
                } else {
                    a_pt2
                }
            };

        let closest_to_intersect = |ip: Vector2I, a_pt1: Vector2I, a_pt2: Vector2I| -> Vector2I {
            if (a_pt1 - ip).euclidean_norm() <= (a_pt2 - ip).euclidean_norm() {
                a_pt1
            } else {
                a_pt2
            }
        };

        if a_line_a.approx_parallel(a_line_b) {
            // Special case, no intersection point between the two lines
            // The center will be in the line equidistant between the two given lines
            // The radius will be half the distance between the two lines
            // The possible centers can be found by intersection

            let perpendicular_a_to_b = Seg::new(a_line_a.a, a_line_b.line_project(a_line_a.a));
            let mid_pt = perpendicular_a_to_b.center();
            self.radius = (mid_pt - a_line_a.a).euclidean_norm();

            anglebisector = a_line_a.parallel_seg(mid_pt);

            self.center = a_p; // use this circle as a construction to find the actual centers
            let possible_centers = self.intersect_line(&anglebisector);

            if possible_centers.is_empty() {
                // wxCHECK_MSG: "No solutions exist!"
                return self;
            }

            intersect_point = a_line_a.a; // just for the purpose of deciding which solution to return

            // For the special case of the two segments being parallel, we will return the solution
            // whose center is closest to aLineA.A
            self.center = closest_to_intersect(
                intersect_point,
                possible_centers[0],
                *possible_centers.last().unwrap(),
            );
        } else {
            // General case, using homothety.
            // All circles inscribed in the same angle are homothetic with center at the intersection
            // In this code, the prefix "h" denotes "the homothetic image"
            let Some(intersect_calc) = a_line_a.intersect_lines(a_line_b) else {
                // wxCHECK_MSG: "Lines do not intersect but are not parallel?"
                return self;
            };
            intersect_point = intersect_calc;

            if a_p == intersect_point {
                //Special case: The point is at the intersection of the two lines
                self.center = a_p;
                self.radius = 0;
                return self;
            }

            // Calculate bisector
            let line_a_pt = furthest_from_intersect(intersect_point, a_line_a.a, a_line_a.b);
            let line_b_pt = furthest_from_intersect(intersect_point, a_line_b.a, a_line_b.b);
            let bisector_pt = calc_arc_mid(line_a_pt, line_b_pt, intersect_point, true);

            anglebisector.a = intersect_point;
            anglebisector.b = bisector_pt;

            // Create an arbitrary circle that is tangent to both lines
            let mut h_solution = Circle::default();
            h_solution.center = anglebisector.line_project(a_p);
            h_solution.radius = a_line_a.line_distance(h_solution.center);

            // Find the homothetic image of aP in the construction circle (hSolution)
            let throughap = Seg::new(intersect_point, a_p);
            let h_projections = h_solution.intersect_line(&throughap);

            if h_projections.is_empty() {
                // wxCHECK_MSG: "No solutions exist!"
                return self;
            }

            // We want to create a fillet, so the projection of homothetic projection of aP
            // should be the one closest to the intersection
            let h_selected = closest_to_intersect(
                intersect_point,
                h_projections[0],
                *h_projections.last().unwrap(),
            );

            let h_tan_line_a = a_line_a.line_project(h_solution.center);
            let h_tan_line_b = a_line_b.line_project(h_solution.center);

            // To minimise errors, use the furthest away tangent point from aP
            if (h_tan_line_a - a_p).squared_euclidean_norm()
                > (h_tan_line_b - a_p).squared_euclidean_norm()
            {
                // Find the tangent at line A by homothetic inversion
                let h_t = Seg::new(h_tan_line_a, h_selected);
                let Some(act_tan_a) = h_t.parallel_seg(a_p).intersect_lines(a_line_a) else {
                    return self;
                };

                // Find circle center by perpendicular intersection with the angle bisector
                let perpendicular_to_tan_a = a_line_a.perpendicular_seg(act_tan_a);
                let Some(act_center) = perpendicular_to_tan_a.intersect_lines(&anglebisector)
                else {
                    return self;
                };

                self.center = act_center;
                self.radius = a_line_a.line_distance(self.center);
            } else {
                // Find the tangent at line B by inversion
                let h_t = Seg::new(h_tan_line_b, h_selected);
                let Some(act_tan_b) = h_t.parallel_seg(a_p).intersect_lines(a_line_b) else {
                    return self;
                };

                // Find circle center by perpendicular intersection with the angle bisector
                let perpendicular_to_tan_b = a_line_b.perpendicular_seg(act_tan_b);
                let Some(act_center) = perpendicular_to_tan_b.intersect_lines(&anglebisector)
                else {
                    return self;
                };

                self.center = act_center;
                self.radius = a_line_b.line_distance(self.center);
            }
        }

        self
    }

    /// Port of `CIRCLE::Contains( const VECTOR2I& aP ) const`: true if `a_p` is on the
    /// circumference of this circle, within [`MIN_PRECISION_IU`].
    ///
    /// Note that C++ also has a non-const overload with a different meaning, see
    /// [`Circle::contains_inside`].
    pub fn contains(&self, a_p: Vector2I) -> bool {
        let distance: i64 = (Vector2L::from(a_p) - self.center).euclidean_norm();

        distance <= (self.radius as i64 + MIN_PRECISION_IU as i64)
            && distance >= (self.radius as i64 - MIN_PRECISION_IU as i64)
    }

    /// Port of the non-const overload `CIRCLE::Contains( const VECTOR2I& aP )`: check whether
    /// point `a_p` is strictly inside this circle.
    pub fn contains_inside(&self, a_p: Vector2I) -> bool {
        (a_p - self.center).euclidean_norm() < self.radius
    }

    /// Port of `CIRCLE::NearestPoint( const VECTOR2I& )`: the point on the circumference that is
    /// closest to `a_p`.
    pub fn nearest_point(&self, a_p: Vector2I) -> Vector2I {
        let mut vec = a_p - self.center;

        // Handle special case where aP is equal to this circle's center
        if vec.x == 0 && vec.y == 0 {
            vec.x = 1; // Arbitrary, to ensure the return value is always on the circumference
        }

        vec.resize(self.radius) + self.center
    }

    /// Port of `CIRCLE::NearestPoint( const VECTOR2D& )` (floating point version).
    pub fn nearest_point_d(&self, a_p: Vector2D) -> Vector2D {
        let mut vec: Vector2D = a_p - self.center;

        // Handle special case where aP is equal to this circle's center
        if vec.x == 0.0 && vec.y == 0.0 {
            vec.x = 1.0; // Arbitrary, to ensure the return value is always on the circumference
        }

        vec.resize(self.radius as f64) + self.center
    }

    /// Port of `CIRCLE::FurthestPoint( const VECTOR2I& )`.
    pub fn furthest_point(&self, a_p: Vector2I) -> Vector2I {
        let mut vec = self.center - a_p;

        if vec.x == 0 && vec.y == 0 {
            vec.x = 1;
        }

        vec.resize(self.radius) + self.center
    }

    /// Port of `CIRCLE::FurthestPoint( const VECTOR2D& )`.
    pub fn furthest_point_d(&self, a_p: Vector2D) -> Vector2D {
        let mut vec: Vector2D = self.center - a_p;

        if vec.x == 0.0 && vec.y == 0.0 {
            vec.x = 1.0;
        }

        vec.resize(self.radius as f64) + self.center
    }

    /// Port of `CIRCLE::Intersect( const CIRCLE& )`: 0, 1 (tangent) or 2 intersection points.
    pub fn intersect(&self, a_circle: &Circle) -> Vec<Vector2I> {
        // From https://mathworld.wolfram.com/Circle-CircleIntersection.html
        //
        // Simplify the problem:
        // Let this circle be centered at (0,0), with radius r1
        // Let aCircle be centered at (d, 0), with radius r2
        // (i.e. d is the distance between the two circle centers)
        //
        // The equations of the two circles are
        // (1)   x^2 + y^2 = r1^2
        // (2)   (x - d)^2 + y^2 = r2^2
        //
        // Combining (1) into (2):
        //       (x - d)^2 + r1^2 - x^2 = r2^2
        // Expanding:
        //       x^2 - 2*d*x + d^2 + r1^2 - x^2 = r2^2
        // Rearranging for x:
        // (3)   x = (d^2 + r1^2 - r2^2) / (2 * d)
        //
        // Rearranging (1) gives:
        // (4)   y = sqrt(r1^2 - x^2)

        let mut retval = Vec::new();

        let vec_c_to_c: Vector2L = Vector2L::from(a_circle.center) - self.center;
        let d: i64 = vec_c_to_c.euclidean_norm();
        let r1: i64 = self.radius as i64;
        let r2: i64 = a_circle.radius as i64;

        if d > (r1 + r2) || (d < (r1 - r2).abs()) {
            return retval; //circles do not intersect
        }

        if d == 0 {
            return retval; // circles are co-centered. Don't return intersection points
        }

        // Equation (3)
        let x: i64 = ((d * d) + (r1 * r1) - (r2 * r2)) / (2i64 * d);
        let r1sq_minus_xsq: i64 = (r1 * r1) - (x * x);

        if r1sq_minus_xsq < 0 {
            return retval; //circles do not intersect
        }

        // Equation (4)
        let y: i64 = ki_round((r1sq_minus_xsq as f64).sqrt()) as i64;

        // Now correct back to original coordinates
        let rot_angle = EdaAngle::from_vector(Vector2D::from_vec(vec_c_to_c));
        // VECTOR2I( int64_t, int64_t ): narrowing conversion of the coordinates
        let mut solution1 = Vector2I::new(x as i32, y as i32);
        rotate_point(&mut solution1, -rot_angle);
        solution1 += self.center;
        retval.push(solution1);

        if y != 0 {
            let mut solution2 = Vector2I::new(x as i32, -y as i32);
            rotate_point(&mut solution2, -rot_angle);
            solution2 += self.center;
            retval.push(solution2);
        }

        retval
    }

    /// Port of `CIRCLE::Intersect( const SEG& )`: intersection points between this circle and
    /// `a_seg` (only those on the segment).
    pub fn intersect_seg(&self, a_seg: &Seg) -> Vec<Vector2I> {
        let mut retval = Vec::new();

        for intersection in self.intersect_line(a_seg) {
            if a_seg.contains(intersection) {
                retval.push(intersection);
            }
        }

        retval
    }

    /// Port of `CIRCLE::IntersectLine()`: intersection points between this circle and the
    /// infinite line through `a_line` (0, 1 if tangent, or 2).
    pub fn intersect_line(&self, a_line: &Seg) -> Vec<Vector2I> {
        let mut retval = Vec::new();

        //
        //           .   *   .
        //         *           *
        //  -----1-------m-------2----
        //      *                 *
        //     *         O         *
        //     *                   *
        //      *                 *
        //       *               *
        //         *           *
        //           '   *   '
        // Let O be the center of this circle, 1 and 2 the intersection points of the line
        // and M be the center of the chord connecting points 1 and 2
        //
        // M will be O projected perpendicularly to the line since a chord is always perpendicular
        // to the radius.
        //
        // The distance M1 = M2 can be computed by pythagoras since O1 = O2 = Radius
        //
        // M1= M2 = sqrt( Radius^2 - OM^2)
        //

        let m = a_line.line_project(self.center); // O projected perpendicularly to the line
        let om_dist: i64 = (Vector2L::from(m) - self.center).euclidean_norm();

        if om_dist > (self.radius as i64 + MIN_PRECISION_IU as i64) {
            return retval; // does not intersect
        } else if om_dist <= (self.radius as i64 + MIN_PRECISION_IU as i64)
            && om_dist >= (self.radius as i64 - MIN_PRECISION_IU as i64)
        {
            retval.push(m);
            return retval; //tangent
        }

        let radius_squared: i64 = self.radius as i64 * self.radius as i64;
        let om_dist_squared: i64 = om_dist * om_dist;

        let m_to1dist: i32 = ((radius_squared - om_dist_squared) as f64).sqrt() as i32;

        let m_to1vec = (a_line.b - a_line.a).resize(m_to1dist);
        let m_to2vec = -m_to1vec;

        retval.push(m_to1vec + m);
        retval.push(m_to2vec + m);

        retval
    }
}
