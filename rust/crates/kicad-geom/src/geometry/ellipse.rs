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

//! Port of `libs/kimath/include/geometry/ellipse.h` and `libs/kimath/src/geometry/ellipse.cpp`
//! (`ELLIPSE<NumericType>`, instantiated for `int` and `double`).

use crate::bezier_curves::NumericType;
use crate::cmath;
use crate::geometry::eda_angle::{ANGLE_0, ANGLE_180, EdaAngle, FULL_CIRCLE, RADIANS_T};
use crate::kicore::mirror::FlipDirection;
use crate::math::util::{ki_round, std_max};
use crate::math::vector2::{Vector2, Vector2D, Vector2I};

/// Port of `ELLIPSE<NumericType>`.
///
/// Plain ellipse / elliptical-arc data. Used by SHAPE_ELLIPSE, by EDA_SHAPE's
/// native ellipse storage, and by importers that need to round-trip ellipses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ellipse<T> {
    /// `Center`
    pub center: Vector2<T>,
    /// `MajorRadius`: the radius of the "x-axis" dimension of the ellipse
    pub major_radius: T,
    /// `MinorRadius`: the radius of the "y-axis" dimension of the ellipse
    pub minor_radius: T,
    /// `Rotation`: the angle of the ellipse "x-axis" relative to world x-axis
    pub rotation: EdaAngle,
    /// `StartAngle`
    pub start_angle: EdaAngle,
    /// `EndAngle`
    pub end_angle: EdaAngle,
}

impl<T: NumericType> Ellipse<T> {
    /// `ELLIPSE( const VECTOR2<NumericType>& aCenter, NumericType aMajorRadius,
    /// NumericType aMinorRadius, EDA_ANGLE aRotation, EDA_ANGLE aStartAngle = ANGLE_0,
    /// EDA_ANGLE aEndAngle = FULL_CIRCLE )`.
    ///
    /// The ellipse sweeps from `a_start_angle` to `a_end_angle` in a counter-clockwise
    /// direction.
    pub fn new(
        a_center: Vector2<T>,
        a_major_radius: T,
        a_minor_radius: T,
        a_rotation: EdaAngle,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
    ) -> Self {
        Ellipse {
            center: a_center,
            major_radius: a_major_radius,
            minor_radius: a_minor_radius,
            rotation: a_rotation,
            start_angle: a_start_angle,
            end_angle: a_end_angle,
        }
    }

    /// The radius constructor with the default angles (`ANGLE_0`, `FULL_CIRCLE`).
    pub fn new_full(
        a_center: Vector2<T>,
        a_major_radius: T,
        a_minor_radius: T,
        a_rotation: EdaAngle,
    ) -> Self {
        Self::new(
            a_center,
            a_major_radius,
            a_minor_radius,
            a_rotation,
            ANGLE_0,
            FULL_CIRCLE,
        )
    }

    /// `ELLIPSE( const VECTOR2<NumericType>& aCenter, const VECTOR2<NumericType>& aMajor,
    /// double aRatio, EDA_ANGLE aStartAngle = ANGLE_0, EDA_ANGLE aEndAngle = FULL_CIRCLE )`.
    ///
    /// Constructs a DXF-style ellipse or elliptical arc, where the major axis is given by a
    /// point (relative to the center) rather than a radius, and therefore defines not only the
    /// major radius but also the rotation of the ellipse.  `a_ratio` is the ratio of the minor
    /// axis length to the major axis length.
    pub fn from_major_axis(
        a_center: Vector2<T>,
        a_major: Vector2<T>,
        a_ratio: f64,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
    ) -> Self {
        let major_radius: T = T::vec_norm(a_major);
        let minor_radius: T = T::from_f64(major_radius.to_f64() * a_ratio);
        let rotation = EdaAngle::new(
            cmath::atan2(a_major.y.to_f64(), a_major.x.to_f64()),
            RADIANS_T,
        );

        Ellipse {
            center: a_center,
            major_radius,
            minor_radius,
            rotation,
            start_angle: a_start_angle,
            end_angle: a_end_angle,
        }
    }

    /// Port of `ELLIPSE::Mirror`: mirror the ellipse along a horizontal or vertical axis
    /// passing through `a_ref`.
    pub fn mirror(&mut self, a_ref: Vector2<T>, a_flip_direction: FlipDirection) {
        if a_flip_direction == FlipDirection::LeftRight {
            self.center.x = T::mirror(a_ref.x, self.center.x);
        } else {
            self.center.y = T::mirror(a_ref.y, self.center.y);
        }

        self.rotation = -self.rotation;

        let old_start = self.start_angle;
        let old_end = self.end_angle;

        if a_flip_direction == FlipDirection::LeftRight {
            // theta' = pi - theta  ->  [s, e]  ->  [pi - e, pi - s]
            self.start_angle = ANGLE_180 - old_end;
            self.end_angle = ANGLE_180 - old_start;
        } else {
            // theta' = -theta  ->  [s, e]  ->  [-e, -s]
            self.start_angle = -old_end;
            self.end_angle = -old_start;
        }
    }

    /// Port of `ELLIPSE::GetSubtendedAngle`.
    pub fn subtended_angle(&self) -> EdaAngle {
        let mut subtended = self.end_angle - self.start_angle;
        subtended.normalize()
    }

    /// Port of `ELLIPSE::GetPointAtAngle`.
    ///
    /// The angle is measured in the ellipse's local coordinate system, where the x-axis is the
    /// major axis and the y-axis is the minor axis.
    pub fn point_at_angle(&self, angle: EdaAngle) -> Vector2<T> {
        let t: f64 = angle.as_radians();
        let ex: f64 = self.major_radius.to_f64() * cmath::cos(t);
        let ey: f64 = self.minor_radius.to_f64() * cmath::sin(t);
        let cos_r: f64 = self.rotation.cos();
        let sin_r: f64 = self.rotation.sin();
        let rx: f64 = ex * cos_r - ey * sin_r;
        let ry: f64 = ex * sin_r + ey * cos_r;

        if T::IS_INTEGRAL {
            let offset = Vector2I::new(ki_round(rx), ki_round(ry));
            T::vec_add(self.center, T::vec_from_d(Vector2D::from(offset)))
        } else {
            T::vec_add(self.center, T::vec_from_d(Vector2D::new(rx, ry)))
        }
    }

    /// Port of `ELLIPSE::GetArcStartPoint`.
    pub fn arc_start_point(&self) -> Vector2<T> {
        self.point_at_angle(self.start_angle)
    }

    /// Port of `ELLIPSE::GetArcEndPoint`.
    pub fn arc_end_point(&self) -> Vector2<T> {
        self.point_at_angle(self.end_angle)
    }

    /// Port of `ELLIPSE::GetAngleAtPoint`: the parametric angle of a point on the ellipse.
    pub fn angle_at_point(&self, a_pt: Vector2<T>) -> EdaAngle {
        let a: f64 = std_max(1.0, self.major_radius.to_f64());
        let b: f64 = std_max(1.0, self.minor_radius.to_f64());
        let dx: f64 = a_pt.x.to_f64() - self.center.x.to_f64();
        let dy: f64 = a_pt.y.to_f64() - self.center.y.to_f64();
        let cos_rot: f64 = self.rotation.cos();
        let sin_rot: f64 = self.rotation.sin();
        let lx: f64 = dx * cos_rot + dy * sin_rot;
        let ly: f64 = -dx * sin_rot + dy * cos_rot;

        EdaAngle::new(cmath::atan2(ly / b, lx / a), RADIANS_T)
    }
}
