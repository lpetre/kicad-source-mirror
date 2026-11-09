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

//! Port of `libs/kimath/include/geometry/transform_trs.h` and
//! `libs/kimath/src/geometry/transform_trs.cpp` (`TRANSFORM_TRS`, the footprint child
//! transform of board files since format 20260616).

use crate::geometry::eda_angle::{ANGLE_0, EdaAngle};
use crate::math::util::{ki_round, std_max};
use crate::math::vector2::{Vector2D, Vector2I};
use crate::trigo::rotate_point;

/// `sameScale`: composing and decomposing a rotation can leave a scale one unit in the last
/// place off.
fn same_scale(a_a: f64, a_b: f64) -> bool {
    (a_a - a_b).abs() <= 1e-9 * std_max(a_a.abs(), a_b.abs())
}

/// Port of `TRANSFORM_TRS`: translate / rotate / scale transform for footprint placement.
///
/// Apply order is fixed: scale, then rotate, then translate.  Independent X/Y scale is
/// allowed.  Shear is not representable.
#[derive(Clone, Copy, Debug)]
pub struct TransformTrs {
    translate: Vector2I,
    rotate: EdaAngle,
    scale_x: f64,
    scale_y: f64,
}

impl Default for TransformTrs {
    /// `TRANSFORM_TRS()`: the identity.
    fn default() -> Self {
        TransformTrs {
            translate: Vector2I::new(0, 0),
            rotate: ANGLE_0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

impl PartialEq for TransformTrs {
    /// Port of `TRANSFORM_TRS::operator==`: exact comparison of all members.
    fn eq(&self, a_other: &Self) -> bool {
        self.translate == a_other.translate
            && self.rotate == a_other.rotate
            && self.scale_x == a_other.scale_x
            && self.scale_y == a_other.scale_y
    }
}

impl TransformTrs {
    /// `TRANSFORM_TRS( const VECTOR2I& aTranslate, const EDA_ANGLE& aRotate, double aScaleX,
    /// double aScaleY )`.
    pub fn new(a_translate: Vector2I, a_rotate: EdaAngle, a_scale_x: f64, a_scale_y: f64) -> Self {
        TransformTrs {
            translate: a_translate,
            rotate: a_rotate,
            scale_x: a_scale_x,
            scale_y: a_scale_y,
        }
    }

    /// Port of `TRANSFORM_TRS::Apply( const VECTOR2D& )`.
    pub fn apply_d(&self, a_point: Vector2D) -> Vector2D {
        let mut scaled = Vector2D::new(a_point.x * self.scale_x, a_point.y * self.scale_y);
        rotate_point(&mut scaled, self.rotate);
        scaled + Vector2D::from(self.translate)
    }

    /// Port of `TRANSFORM_TRS::Apply( const VECTOR2I& )`.
    pub fn apply(&self, a_point: Vector2I) -> Vector2I {
        let r = self.apply_d(Vector2D::from(a_point));
        Vector2I::new(ki_round(r.x), ki_round(r.y))
    }

    /// Port of `TRANSFORM_TRS::InverseApply( const VECTOR2D& )`.
    pub fn inverse_apply_d(&self, a_point: Vector2D) -> Vector2D {
        let mut shifted = a_point - Vector2D::from(self.translate);
        rotate_point(&mut shifted, -self.rotate);
        Vector2D::new(shifted.x / self.scale_x, shifted.y / self.scale_y)
    }

    /// Port of `TRANSFORM_TRS::InverseApply( const VECTOR2I& )`.
    pub fn inverse_apply(&self, a_point: Vector2I) -> Vector2I {
        let r = self.inverse_apply_d(Vector2D::from(a_point));
        Vector2I::new(ki_round(r.x), ki_round(r.y))
    }

    /// Port of `TRANSFORM_TRS::RescaleAround`.
    pub fn rescale_around(&self, a_fixed_point: Vector2I, a_sx: f64, a_sy: f64) -> TransformTrs {
        let mut result = *self;

        // aSx/aSy are scale multipliers in the footprint's own frame, so the local
        // scale factors scale directly.
        result.scale_x = self.scale_x * a_sx;
        result.scale_y = self.scale_y * a_sy;

        // The offset from the fixed point is in board axes. Rotate it into the
        // footprint frame, scale it there, then rotate back (R D R^-1) so the position
        // move matches the local scaling when the footprint is rotated.
        // (The differences are `int` arithmetic in C++; they wrap like the reference build.)
        let mut offset = Vector2D::new(
            self.translate.x.wrapping_sub(a_fixed_point.x) as f64,
            self.translate.y.wrapping_sub(a_fixed_point.y) as f64,
        );
        rotate_point(&mut offset, -self.rotate);
        offset.x *= a_sx;
        offset.y *= a_sy;
        rotate_point(&mut offset, self.rotate);

        result.translate = Vector2I::new(
            ki_round(a_fixed_point.x as f64 + offset.x),
            ki_round(a_fixed_point.y as f64 + offset.y),
        );
        result
    }

    /// Port of `TRANSFORM_TRS::IsIdentity`.
    pub fn is_identity(&self) -> bool {
        self.translate == Vector2I::new(0, 0) && self.rotate.is_zero() && self.has_unit_scale()
    }

    /// Port of `TRANSFORM_TRS::IsUniformScale`.
    ///
    /// Scales that differ only by rounding count as equal, so a pure rotation stays uniform.
    pub fn is_uniform_scale(&self) -> bool {
        same_scale(self.scale_x, self.scale_y)
    }

    /// Port of `TRANSFORM_TRS::HasUnitScale`: true when both scales are 1, ignoring rounding.
    pub fn has_unit_scale(&self) -> bool {
        same_scale(self.scale_x, 1.0) && same_scale(self.scale_y, 1.0)
    }

    /// Port of `TRANSFORM_TRS::ApplyLinearScale`.
    pub fn apply_linear_scale(&self, a_length: f64) -> f64 {
        let scale = if self.is_uniform_scale() {
            self.scale_x
        } else {
            (self.scale_x + self.scale_y) / 2.0
        };

        a_length * scale
    }

    /// Port of `TRANSFORM_TRS::InverseApplyLinearScale`.
    pub fn inverse_apply_linear_scale(&self, a_length: f64) -> f64 {
        let scale = if self.is_uniform_scale() {
            self.scale_x
        } else {
            (self.scale_x + self.scale_y) / 2.0
        };

        a_length / scale
    }

    /// Port of `TRANSFORM_TRS::GetTranslate`.
    pub fn translate(&self) -> Vector2I {
        self.translate
    }

    /// Port of `TRANSFORM_TRS::GetRotate`.
    pub fn rotate(&self) -> EdaAngle {
        self.rotate
    }

    /// Port of `TRANSFORM_TRS::GetScaleX`.
    pub fn scale_x(&self) -> f64 {
        self.scale_x
    }

    /// Port of `TRANSFORM_TRS::GetScaleY`.
    pub fn scale_y(&self) -> f64 {
        self.scale_y
    }

    /// Port of `TRANSFORM_TRS::SetTranslate`.
    pub fn set_translate(&mut self, a_t: Vector2I) {
        self.translate = a_t;
    }

    /// Port of `TRANSFORM_TRS::SetRotate`.
    pub fn set_rotate(&mut self, a_r: EdaAngle) {
        self.rotate = a_r;
    }

    /// Port of `TRANSFORM_TRS::SetScale`.
    pub fn set_scale(&mut self, a_sx: f64, a_sy: f64) {
        self.scale_x = a_sx;
        self.scale_y = a_sy;
    }
}
