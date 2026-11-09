// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007-2010 Wayne Stambaugh <stambaughw@gmail.com>
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

//! Port of `libs/kimath/include/transform.h` and `libs/kimath/src/transform.cpp` (`TRANSFORM`,
//! the symbol orientation matrix, and its `std::hash` specialisation).

use crate::geometry::eda_angle::{ANGLE_0, ANGLE_360, DEGREES_T, EdaAngle};
use crate::hash::{StdHash, hash_combine};
use crate::math::box2::Box2I;
use crate::math::vector2::{Vector2D, Vector2I};

/// Port of `TRANSFORM`: a 2x2 integer matrix for transforming drawing coordinates (mirror /
/// rotation of symbols).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transform {
    /// `x1`
    pub x1: i32,
    /// `y1`
    pub y1: i32,
    /// `x2`
    pub x2: i32,
    /// `y2`
    pub y2: i32,
}

/// Port of the global `DefaultTransform` (`TRANSFORM( 1, 0, 0, 1 )`).
pub const DEFAULT_TRANSFORM: Transform = Transform::new(1, 0, 0, 1);

impl Default for Transform {
    /// `TRANSFORM()`: the identity, i.e. the normal orientation.
    fn default() -> Self {
        Transform::new(1, 0, 0, 1)
    }
}

impl Transform {
    /// `TRANSFORM( int ax1, int ay1, int ax2, int ay2 )`.
    pub const fn new(ax1: i32, ay1: i32, ax2: i32, ay2: i32) -> Self {
        Transform {
            x1: ax1,
            y1: ay1,
            x2: ax2,
            y2: ay2,
        }
    }

    /// Port of `TRANSFORM::TransformCoordinate( const VECTOR2I& )`.
    ///
    /// Calculate a new coordinate according to the mirror/rotation transform.  The `int`
    /// arithmetic wraps on overflow as the reference build does.
    pub fn transform_coordinate(&self, a_point: Vector2I) -> Vector2I {
        Vector2I::new(
            self.x1
                .wrapping_mul(a_point.x)
                .wrapping_add(self.y1.wrapping_mul(a_point.y)),
            self.x2
                .wrapping_mul(a_point.x)
                .wrapping_add(self.y2.wrapping_mul(a_point.y)),
        )
    }

    /// Port of `TRANSFORM::TransformCoordinate( const BOX2I& )`.
    pub fn transform_coordinate_box(&self, a_rect: &Box2I) -> Box2I {
        let mut rect = Box2I::default();
        rect.set_origin(self.transform_coordinate(a_rect.origin()));
        rect.set_end(self.transform_coordinate(a_rect.end()));
        rect
    }

    /// Port of `TRANSFORM::InverseTransform`.
    ///
    /// Calculates the inverse matrix coeffs: for a matrix m{x1, x2, y1, y2} the inverse matrix
    /// is 1/(x1*y2 -x2*y1) m{y2,-x2,-y1,x1).  The determinant "is never null, because the
    /// inverse matrix exists"; for a singular matrix (division by zero in C++) the port returns
    /// the matrix unchanged instead of panicking.
    pub fn inverse_transform(&self) -> Transform {
        let det: i32 = self
            .x1
            .wrapping_mul(self.y2)
            .wrapping_sub(self.x2.wrapping_mul(self.y1));

        if det == 0 {
            return *self;
        }

        let invx1 = self.y2.wrapping_div(det);
        let invx2 = self.x2.wrapping_neg().wrapping_div(det);
        let invy1 = self.y1.wrapping_neg().wrapping_div(det);
        let invy2 = self.x1.wrapping_div(det);

        Transform::new(invx1, invy1, invx2, invy2)
    }

    /// Port of `TRANSFORM::MapAngles`.
    ///
    /// Calculate new angles according to the transform.  Returns true if the angles were
    /// swapped during the transform.
    pub fn map_angles(&self, a_angle1: &mut EdaAngle, a_angle2: &mut EdaAngle) -> bool {
        let epsilon = EdaAngle::new(0.1, DEGREES_T);

        let mut swap = false;

        let delta: EdaAngle = *a_angle2 - *a_angle1;

        let x = a_angle1.cos();
        let y = a_angle1.sin();
        let v = Vector2D::new(
            x * self.x1 as f64 + y * self.y1 as f64,
            x * self.x2 as f64 + y * self.y2 as f64,
        );
        *a_angle1 = EdaAngle::from_vector(v);

        let x = a_angle2.cos();
        let y = a_angle2.sin();
        let v = Vector2D::new(
            x * self.x1 as f64 + y * self.y1 as f64,
            x * self.x2 as f64 + y * self.y2 as f64,
        );
        *a_angle2 = EdaAngle::from_vector(v);

        let delta_transformed: EdaAngle = *a_angle2 - *a_angle1;
        let mut residual_error: EdaAngle = delta_transformed - delta;
        residual_error.normalize();

        if residual_error > epsilon || residual_error < epsilon.invert().normalize() {
            core::mem::swap(a_angle1, a_angle2);
            swap = true;
        }

        if *a_angle2 < *a_angle1 {
            if *a_angle2 < ANGLE_0 {
                a_angle2.normalize();
            } else {
                *a_angle1 = a_angle1.normalize() - ANGLE_360;
            }
        }

        swap
    }

    /// Port of `std::hash<TRANSFORM>::operator()`.
    pub fn std_hash_value(&self) -> u64 {
        let mut seed: u64 = self.x1.std_hash();
        hash_combine(&mut seed, &self.y1);
        hash_combine(&mut seed, &self.x2);
        hash_combine(&mut seed, &self.y2);
        seed
    }
}

impl StdHash for Transform {
    /// `std::hash<TRANSFORM>`.
    fn std_hash(&self) -> u64 {
        self.std_hash_value()
    }
}
