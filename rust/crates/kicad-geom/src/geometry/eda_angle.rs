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

//! Port of `libs/kimath/include/geometry/eda_angle.h` and
//! `libs/kimath/src/geometry/eda_angle.cpp`.

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use crate::math::util::{equals, ki_round};
use crate::math::vector2::Vector2D;
use crate::ostream::format_double;

/// Port of `EDA_ANGLE_T`: the unit of the value an [`EdaAngle`] is constructed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdaAngleT {
    /// `TENTHS_OF_A_DEGREE_T`
    TenthsOfADegree,
    /// `DEGREES_T`
    Degrees,
    /// `RADIANS_T`
    Radians,
}

/// Port of the enumerator `TENTHS_OF_A_DEGREE_T`.
pub const TENTHS_OF_A_DEGREE_T: EdaAngleT = EdaAngleT::TenthsOfADegree;

/// Port of the enumerator `DEGREES_T`.
pub const DEGREES_T: EdaAngleT = EdaAngleT::Degrees;

/// Port of the enumerator `RADIANS_T`.
pub const RADIANS_T: EdaAngleT = EdaAngleT::Radians;

/// Port of `EDA_ANGLE`.
///
/// The angle is stored in degrees as a `double`.  Equality and ordering compare the stored
/// values exactly (no normalisation, no epsilon), as the C++ operators do.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct EdaAngle {
    /// value in degrees
    value: f64,
}

impl EdaAngle {
    /// Port of `EDA_ANGLE::DEGREES_TO_RADIANS`.
    pub const DEGREES_TO_RADIANS: f64 = PI / 180.0;

    /// Port of `EDA_ANGLE::EDA_ANGLE( double aValue, EDA_ANGLE_T aAngleType )`.
    ///
    /// Angles can be created in degrees, 1/10ths of a degree, or radians, and read as any of
    /// the angle types.
    ///
    /// Angle type must be explicitly specified at creation, because there is no other way of
    /// knowing what an int or a double represents.
    #[inline]
    pub fn new(a_value: f64, a_angle_type: EdaAngleT) -> Self {
        let value = match a_angle_type {
            EdaAngleT::Radians => a_value / Self::DEGREES_TO_RADIANS,
            EdaAngleT::TenthsOfADegree => a_value / 10.0,
            EdaAngleT::Degrees => a_value,
        };

        EdaAngle { value }
    }

    /// Port of the `constexpr explicit EDA_ANGLE( double aAngleInDegrees )` constructor.
    ///
    /// Construct an EDA_ANGLE in degrees.  This is really only for the statically-defined
    /// global angles.  All other code should use `EdaAngle::new( xxx, DEGREES_T )`.
    #[inline]
    pub const fn from_degrees(a_angle_in_degrees: f64) -> Self {
        EdaAngle { value: a_angle_in_degrees }
    }

    /// Port of `explicit EDA_ANGLE( const VECTOR2D& aVector )`: the direction of a vector,
    /// with exact results for the axes and diagonals.
    ///
    /// A `Vector2I` converts to `Vector2D` implicitly in C++ and via `Into` here.
    pub fn from_vector(a_vector: impl Into<Vector2D>) -> Self {
        let a_vector: Vector2D = a_vector.into();

        let value = if a_vector.x == 0.0 && a_vector.y == 0.0 {
            0.0
        } else if a_vector.y == 0.0 {
            if a_vector.x >= 0.0 { 0.0 } else { -180.0 }
        } else if a_vector.x == 0.0 {
            if a_vector.y >= 0.0 { 90.0 } else { -90.0 }
        } else if a_vector.x == a_vector.y {
            if a_vector.x >= 0.0 { 45.0 } else { -180.0 + 45.0 }
        } else if a_vector.x == -a_vector.y {
            if a_vector.x >= 0.0 { -45.0 } else { 180.0 - 45.0 }
        } else {
            return EdaAngle::new(a_vector.y.atan2(a_vector.x), EdaAngleT::Radians);
        };

        EdaAngle { value }
    }

    /// Port of `EDA_ANGLE::AsDegrees()`.
    #[inline]
    pub const fn as_degrees(self) -> f64 {
        self.value
    }

    /// Port of `EDA_ANGLE::AsTenthsOfADegree()`.
    #[inline]
    pub fn as_tenths_of_a_degree(self) -> i32 {
        ki_round(self.value * 10.0)
    }

    /// Port of `EDA_ANGLE::AsRadians()`.
    #[inline]
    pub fn as_radians(self) -> f64 {
        self.value * Self::DEGREES_TO_RADIANS
    }

    /// Port of `EDA_ANGLE::IsCardinal()`.
    ///
    /// Returns true if angle is one of the four cardinal directions (0/90/180/270 degrees),
    /// otherwise false.
    pub fn is_cardinal(self) -> bool {
        if !self.value.is_finite() {
            return false;
        }

        let mut test: f64 = Self::fold_to_two_periods(self.value, 90.0);

        while test < 0.0 {
            test += 90.0;
        }

        while test >= 90.0 {
            test -= 90.0;
        }

        test == 0.0
    }

    /// Port of `EDA_ANGLE::IsCardinal90()`.
    ///
    /// Returns true if angle is one of the two cardinal directions (90/270 degrees),
    /// otherwise false.
    pub fn is_cardinal90(self) -> bool {
        // return true if angle is one of the two cardinal directions (90/270 degrees),
        if !self.value.is_finite() {
            return false;
        }

        let mut test: f64 = Self::fold_to_two_periods(self.value.abs(), 180.0);

        while test >= 180.0 {
            test -= 180.0;
        }

        test == 90.0
    }

    /// Port of `EDA_ANGLE::IsZero()`.
    #[inline]
    pub fn is_zero(self) -> bool {
        self.value == 0.0
    }

    /// Port of `EDA_ANGLE::IsHorizontal()`.
    #[inline]
    pub fn is_horizontal(self) -> bool {
        self.value == 0.0 || self.value == 180.0
    }

    /// Port of `EDA_ANGLE::IsVertical()`.
    #[inline]
    pub fn is_vertical(self) -> bool {
        self.value == 90.0 || self.value == 270.0
    }

    /// Port of `EDA_ANGLE::IsParallelTo()`.
    pub fn is_parallel_to(self, a_angle: EdaAngle) -> bool {
        let mut a_angle = a_angle;
        let mut this_normalized = self;

        // Normalize90 is inclusive on both ends [-90, +90]
        // but we need it to be (-90, +90] for this test to work
        this_normalized.normalize90();

        if equals(this_normalized.as_degrees(), -90.0) {
            this_normalized = EdaAngle::new(90.0, EdaAngleT::Degrees);
        }

        a_angle.normalize90();

        if equals(a_angle.as_degrees(), -90.0) {
            a_angle = EdaAngle::new(90.0, EdaAngleT::Degrees);
        }

        equals(this_normalized.as_degrees(), a_angle.as_degrees())
    }

    /// Port of `EDA_ANGLE::Invert()`.
    #[inline]
    pub fn invert(self) -> EdaAngle {
        EdaAngle::new(-self.as_degrees(), EdaAngleT::Degrees)
    }

    /// Port of `EDA_ANGLE::Sin()`: exact values for multiples of 45 degrees.
    pub fn sin(self) -> f64 {
        let mut test = self;
        test.normalize();

        if test.value == 0.0 || test.value == 180.0 {
            0.0
        } else if test.value == 45.0 || test.value == 135.0 {
            FRAC_1_SQRT_2 // sqrt(2)/2
        } else if test.value == 225.0 || test.value == 315.0 {
            -FRAC_1_SQRT_2
        } else if test.value == 90.0 {
            1.0
        } else if test.value == 270.0 {
            -1.0
        } else {
            self.as_radians().sin()
        }
    }

    /// Port of `EDA_ANGLE::Cos()`: exact values for multiples of 45 degrees.
    pub fn cos(self) -> f64 {
        let mut test = self;
        test.normalize();

        if test.value == 0.0 {
            1.0
        } else if test.value == 180.0 {
            -1.0
        } else if test.value == 90.0 || test.value == 270.0 {
            0.0
        } else if test.value == 45.0 || test.value == 315.0 {
            FRAC_1_SQRT_2 // sqrt(2)/2
        } else if test.value == 135.0 || test.value == 225.0 {
            -FRAC_1_SQRT_2
        } else {
            self.as_radians().cos()
        }
    }

    /// Port of `EDA_ANGLE::Tan()`.
    #[inline]
    pub fn tan(self) -> f64 {
        self.as_radians().tan()
    }

    /// Port of `EDA_ANGLE::Arccos()`.
    #[inline]
    pub fn arccos(x: f64) -> EdaAngle {
        EdaAngle::new(x.acos(), EdaAngleT::Radians)
    }

    /// Port of `EDA_ANGLE::Arcsin()`.
    #[inline]
    pub fn arcsin(x: f64) -> EdaAngle {
        EdaAngle::new(x.asin(), EdaAngleT::Radians)
    }

    /// Port of `EDA_ANGLE::Arctan()`.
    #[inline]
    pub fn arctan(x: f64) -> EdaAngle {
        EdaAngle::new(x.atan(), EdaAngleT::Radians)
    }

    /// Port of `EDA_ANGLE::Arctan2()`.
    #[inline]
    pub fn arctan2(y: f64, x: f64) -> EdaAngle {
        EdaAngle::new(y.atan2(x), EdaAngleT::Radians)
    }

    /// Port of `EDA_ANGLE::Normalize()`: normalize in place to [0, 360) and return the new
    /// value.
    #[inline]
    pub fn normalize(&mut self) -> EdaAngle {
        self.value = Self::fold_to_two_periods(self.value, 360.0);

        while self.value < -0.0 {
            self.value += 360.0;
        }

        while self.value >= 360.0 {
            self.value -= 360.0;
        }

        *self
    }

    /// Port of `EDA_ANGLE::Normalized()`: a copy normalized to [0, 360).
    #[inline]
    pub fn normalized(self) -> EdaAngle {
        let mut ret = self;

        ret.normalize()
    }

    /// Port of `EDA_ANGLE::NormalizeNegative()`: normalize in place to (-360, 0].
    #[inline]
    pub fn normalize_negative(&mut self) -> EdaAngle {
        self.value = Self::fold_to_two_periods(self.value, 360.0);

        while self.value <= -360.0 {
            self.value += 360.0;
        }

        while self.value > 0.0 {
            self.value -= 360.0;
        }

        *self
    }

    /// Port of `EDA_ANGLE::Normalize90()`: normalize in place to [-90, 90].
    #[inline]
    pub fn normalize90(&mut self) -> EdaAngle {
        self.value = Self::fold_to_two_periods(self.value, 180.0);

        while self.value < -90.0 {
            self.value += 180.0;
        }

        while self.value > 90.0 {
            self.value -= 180.0;
        }

        *self
    }

    /// Port of `EDA_ANGLE::Normalize180()`: normalize in place to (-180, 180].
    #[inline]
    pub fn normalize180(&mut self) -> EdaAngle {
        self.value = Self::fold_to_two_periods(self.value, 360.0);

        while self.value <= -180.0 {
            self.value += 360.0;
        }

        while self.value > 180.0 {
            self.value -= 360.0;
        }

        *self
    }

    /// Port of `EDA_ANGLE::Normalize720()`: normalize in place to [-360, 360).
    #[inline]
    pub fn normalize720(&mut self) -> EdaAngle {
        self.value = Self::fold_to_two_periods(self.value, 360.0);

        while self.value < -360.0 {
            self.value += 360.0;
        }

        while self.value >= 360.0 {
            self.value -= 360.0;
        }

        *self
    }

    /// Port of `EDA_ANGLE::Snapped()`.
    ///
    /// Snap to the nearest integer multiple of `a_step` (returns `self` if `a_step` <= 0).
    pub fn snapped(self, a_step: EdaAngle) -> EdaAngle {
        let step: f64 = a_step.as_degrees();

        if step <= 0.0 {
            return self;
        }

        EdaAngle::new(step * ki_round(self.as_degrees() / step) as f64, EdaAngleT::Degrees)
    }

    /// Port of `EDA_ANGLE::KeepUpright()`.
    pub fn keep_upright(self) -> EdaAngle {
        let mut in_angle = self;
        in_angle.normalize();

        let in_degrees: f64 = in_angle.as_degrees();

        let out_degrees: f64 =
            if in_degrees <= 45.0 || in_degrees >= 315.0 || (in_degrees > 135.0 && in_degrees <= 225.0) {
                0.0
            } else {
                90.0
            };

        EdaAngle::new(out_degrees, EdaAngleT::Degrees)
    }

    /// Port of `EDA_ANGLE::Reflect()`.
    ///
    /// Reflection, over a line with direction `a_axis_angle`, of the turn this angle stores.
    ///
    /// Assuming a y-down coordinate system, the turn is measured anti-clockwise from the
    /// positive x-axis (i.e. the same turn convention as RotatePoint()).
    ///
    /// ```text
    ///       /45   |     \ 135
    ///      /___   |      \____
    ///        a    |        a'
    ///             |
    ///             |<- reflection axis, 90 or 270 degrees
    /// ```
    ///
    /// In this case, an angle a = 45 degrees is reflected over the y-axis to become
    /// a' = 135 degrees.
    ///
    /// Reflecting over aAxisAngle gives the same result as reflecting over
    /// aAxisAngle + 180 (or any value modulo 180), so the direction of the axis
    /// does not matter.
    #[must_use]
    pub fn reflect(self, a_axis_angle: EdaAngle) -> EdaAngle {
        EdaAngle::new(2.0 * a_axis_angle.as_degrees() - self.as_degrees(), EdaAngleT::Degrees)
    }

    /// Port of `EDA_ANGLE::Round( int digits )`.
    #[must_use]
    pub fn round(self, digits: i32) -> EdaAngle {
        let rounded: f64 =
            ki_round(self.as_degrees() * 10.0f64.powf(digits as f64)) as f64 / 10.0f64.powf(digits as f64);

        EdaAngle::new(rounded, EdaAngleT::Degrees)
    }

    /// Port of `std::abs( const EDA_ANGLE& )`.
    #[inline]
    pub fn abs(self) -> EdaAngle {
        EdaAngle::new(self.as_degrees().abs(), EdaAngleT::Degrees)
    }

    /// Port of `EDA_ANGLE::foldToTwoPeriods`.
    ///
    /// Exactly remove whole periods from values beyond two periods, keeping the sign, so the
    /// normalization loops run at most twice.  Repeated subtraction alone never terminates
    /// once the spacing between doubles exceeds the period.  Non-finite values become 0.
    #[inline]
    fn fold_to_two_periods(a_value: f64, a_period: f64) -> f64 {
        if !a_value.is_finite() {
            return 0.0;
        }

        if a_value.abs() < 2.0 * a_period {
            return a_value;
        }

        // Exact because |aValue| >= 2 * aPeriod keeps the remainder on a grid no finer than the
        // sum.  (The `%` operator on f64 is std::fmod.)
        (a_value % a_period) + a_period.copysign(a_value)
    }
}

impl Neg for EdaAngle {
    type Output = EdaAngle;

    #[inline]
    fn neg(self) -> EdaAngle {
        self.invert()
    }
}

impl Sub for EdaAngle {
    type Output = EdaAngle;

    #[inline]
    fn sub(self, rhs: EdaAngle) -> EdaAngle {
        EdaAngle::new(self.as_degrees() - rhs.as_degrees(), EdaAngleT::Degrees)
    }
}

impl Add for EdaAngle {
    type Output = EdaAngle;

    #[inline]
    fn add(self, rhs: EdaAngle) -> EdaAngle {
        EdaAngle::new(self.as_degrees() + rhs.as_degrees(), EdaAngleT::Degrees)
    }
}

impl Mul<f64> for EdaAngle {
    type Output = EdaAngle;

    #[inline]
    fn mul(self, a_operator: f64) -> EdaAngle {
        EdaAngle::new(self.as_degrees() * a_operator, EdaAngleT::Degrees)
    }
}

impl Mul<i32> for EdaAngle {
    type Output = EdaAngle;

    /// `operator*( const EDA_ANGLE&, double )` called with an `int`, which C++ converts
    /// implicitly.
    #[inline]
    fn mul(self, a_operator: i32) -> EdaAngle {
        self * a_operator as f64
    }
}

impl Div<f64> for EdaAngle {
    type Output = EdaAngle;

    #[inline]
    fn div(self, a_operator: f64) -> EdaAngle {
        EdaAngle::new(self.as_degrees() / a_operator, EdaAngleT::Degrees)
    }
}

impl Div<i32> for EdaAngle {
    type Output = EdaAngle;

    /// `operator/( const EDA_ANGLE&, double )` called with an `int`, which C++ converts
    /// implicitly.
    #[inline]
    fn div(self, a_operator: i32) -> EdaAngle {
        self / a_operator as f64
    }
}

impl Div<EdaAngle> for EdaAngle {
    type Output = f64;

    #[inline]
    fn div(self, a_operator: EdaAngle) -> f64 {
        self.as_degrees() / a_operator.as_degrees()
    }
}

impl AddAssign for EdaAngle {
    #[inline]
    fn add_assign(&mut self, a_angle: EdaAngle) {
        *self = EdaAngle::new(self.as_degrees() + a_angle.as_degrees(), EdaAngleT::Degrees);
    }
}

impl SubAssign for EdaAngle {
    #[inline]
    fn sub_assign(&mut self, a_angle: EdaAngle) {
        *self = EdaAngle::new(self.as_degrees() - a_angle.as_degrees(), EdaAngleT::Degrees);
    }
}

impl fmt::Display for EdaAngle {
    /// Port of `operator<<( std::ostream&, const EDA_ANGLE& )`: the angle in degrees.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_double(self.as_degrees()))
    }
}

/// Port of `ANGLE_HORIZONTAL`.
pub const ANGLE_HORIZONTAL: EdaAngle = EdaAngle::from_degrees(0.0);
/// Port of `ANGLE_VERTICAL`.
pub const ANGLE_VERTICAL: EdaAngle = EdaAngle::from_degrees(90.0);
/// Port of `FULL_CIRCLE`.
pub const FULL_CIRCLE: EdaAngle = EdaAngle::from_degrees(360.0);

/// Port of `ANGLE_0`.
pub const ANGLE_0: EdaAngle = EdaAngle::from_degrees(0.0);
/// Port of `ANGLE_45`.
pub const ANGLE_45: EdaAngle = EdaAngle::from_degrees(45.0);
/// Port of `ANGLE_90`.
pub const ANGLE_90: EdaAngle = EdaAngle::from_degrees(90.0);
/// Port of `ANGLE_135`.
pub const ANGLE_135: EdaAngle = EdaAngle::from_degrees(135.0);
/// Port of `ANGLE_180`.
pub const ANGLE_180: EdaAngle = EdaAngle::from_degrees(180.0);
/// Port of `ANGLE_270`.
pub const ANGLE_270: EdaAngle = EdaAngle::from_degrees(270.0);
/// Port of `ANGLE_360`.
pub const ANGLE_360: EdaAngle = EdaAngle::from_degrees(360.0);

/// Port of `EDA_ORIENTATION`.
///
/// An "orientation", held as an angle constrained to the range [0, 360) degrees.
///
/// This makes sure that orientations are always in their normalized form, which
/// keeps equality, serialisation and display stable.
///
/// Adding or subtracting EDA_ANGLEs and negation wrap around the [0, 360) range.
///
/// There is no meaningful multiplication: e.g. 45 and 405 are the same orientation, yet their
/// halves are 22.5 and 202.5. In the same way that there's no such thing as "3 x East".
///
/// Do not use this class for angle magnitudes such as an arc extent or an angular spacing,
/// because those are not constrained to [0, 360). In particular, arc extent has a meaningful
/// value of 360 degrees that is distinct from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdaOrientation {
    angle: EdaAngle,
}

impl EdaOrientation {
    /// Port of `explicit EDA_ORIENTATION( const EDA_ANGLE& aAngle )`.
    #[inline]
    pub fn new(a_angle: EdaAngle) -> Self {
        let mut orientation = EdaOrientation::default();
        orientation.set_angle(a_angle);
        orientation
    }

    /// Port of `EDA_ORIENTATION::GetAngle()`.
    #[inline]
    pub fn angle(self) -> EdaAngle {
        self.angle
    }

    /// Port of `EDA_ORIENTATION::SetAngle()` (and of `operator=( const EDA_ANGLE& )`):
    /// assign a plain angle, normalising it.
    pub fn set_angle(&mut self, a_angle: EdaAngle) {
        self.angle = a_angle;
        self.angle.normalize();

        // Normalize -0 to 0 to avoid -0 in display and serialization.
        if self.angle == ANGLE_0 {
            self.angle = ANGLE_0;
        }
    }

    /// Port of `EDA_ORIENTATION::Reflect()`.
    ///
    /// This orientation reflected over a line with direction `a_axis_angle`.
    ///
    /// The orientation counterpart of [`EdaAngle::reflect`] with the normalized
    /// result.
    #[must_use]
    pub fn reflect(self, a_axis_angle: EdaAngle) -> EdaOrientation {
        EdaOrientation::new(self.angle.reflect(a_axis_angle))
    }
}

impl From<EdaAngle> for EdaOrientation {
    #[inline]
    fn from(a_angle: EdaAngle) -> Self {
        EdaOrientation::new(a_angle)
    }
}

impl Add<EdaAngle> for EdaOrientation {
    type Output = EdaOrientation;

    /// Add an EDA_ANGLE to this orientation, wrapping around the [0, 360) range.
    #[inline]
    fn add(self, a_angle: EdaAngle) -> EdaOrientation {
        EdaOrientation::new(self.angle + a_angle)
    }
}

impl Sub<EdaAngle> for EdaOrientation {
    type Output = EdaOrientation;

    /// Subtract an EDA_ANGLE from this orientation, wrapping around the [0, 360) range.
    #[inline]
    fn sub(self, a_angle: EdaAngle) -> EdaOrientation {
        EdaOrientation::new(self.angle - a_angle)
    }
}

impl Neg for EdaOrientation {
    type Output = EdaOrientation;

    #[inline]
    fn neg(self) -> EdaOrientation {
        EdaOrientation::new(-self.angle)
    }
}

impl Sub<EdaOrientation> for EdaOrientation {
    type Output = EdaAngle;

    /// The difference of two orientations is a signed rotation amount, not an orientation.
    ///
    /// The sign of the result indicates the direction of the difference of the two
    /// orientations.
    #[inline]
    fn sub(self, a_other: EdaOrientation) -> EdaAngle {
        self.angle - a_other.angle
    }
}

impl AddAssign<EdaAngle> for EdaOrientation {
    #[inline]
    fn add_assign(&mut self, a_angle: EdaAngle) {
        self.set_angle(self.angle + a_angle);
    }
}

impl SubAssign<EdaAngle> for EdaOrientation {
    #[inline]
    fn sub_assign(&mut self, a_angle: EdaAngle) {
        self.set_angle(self.angle - a_angle);
    }
}

// Free forms with the angle on the left, so the mirror-style formulas read as-written
// (180 - orientation, -orientation).
impl Add<EdaOrientation> for EdaAngle {
    type Output = EdaOrientation;

    #[inline]
    fn add(self, a_orientation: EdaOrientation) -> EdaOrientation {
        a_orientation + self
    }
}

impl Sub<EdaOrientation> for EdaAngle {
    type Output = EdaOrientation;

    #[inline]
    fn sub(self, a_orientation: EdaOrientation) -> EdaOrientation {
        EdaOrientation::new(self - a_orientation.angle())
    }
}

impl fmt::Display for EdaOrientation {
    /// Port of `operator<<( std::ostream&, const EDA_ORIENTATION& )`: follow EDA_ANGLE's
    /// convention of outputting angles in degrees.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_double(self.angle().as_degrees()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vector2::Vector2I;

    #[test]
    fn construction_units() {
        assert_eq!(EdaAngle::new(900.0, TENTHS_OF_A_DEGREE_T).as_degrees(), 90.0);
        assert_eq!(EdaAngle::new(90.0, DEGREES_T).as_degrees(), 90.0);
        assert_eq!(EdaAngle::new(PI, RADIANS_T).as_degrees(), 180.0);
        assert_eq!(EdaAngle::new(90.0, DEGREES_T).as_tenths_of_a_degree(), 900);
        assert_eq!(EdaAngle::new(180.0, DEGREES_T).as_radians(), PI);
        assert_eq!(EdaAngle::default().as_degrees(), 0.0);
        assert_eq!(EdaAngle::new(12.34, DEGREES_T).as_tenths_of_a_degree(), 123);
        assert_eq!(EdaAngle::new(-12.35, DEGREES_T).as_tenths_of_a_degree(), -124);
    }

    #[test]
    fn from_vector_cardinals_and_diagonals() {
        let deg = |x: i32, y: i32| EdaAngle::from_vector(Vector2I::new(x, y)).as_degrees();

        assert_eq!(deg(0, 0), 0.0);
        assert_eq!(deg(5, 0), 0.0);
        assert_eq!(deg(-5, 0), -180.0);
        assert_eq!(deg(0, 5), 90.0);
        assert_eq!(deg(0, -5), -90.0);
        assert_eq!(deg(5, 5), 45.0);
        assert_eq!(deg(-5, -5), -135.0);
        assert_eq!(deg(5, -5), -45.0);
        assert_eq!(deg(-5, 5), 135.0);
        assert_eq!(deg(1, 2), 2.0f64.atan2(1.0) / EdaAngle::DEGREES_TO_RADIANS);
        assert_eq!(EdaAngle::from_vector(Vector2D::new(-0.5, -0.5)).as_degrees(), -135.0);
    }

    #[test]
    fn sin_cos_exact_at_multiples_of_45() {
        let cases: [(f64, f64, f64); 9] = [
            (0.0, 0.0, 1.0),
            (45.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
            (90.0, 1.0, 0.0),
            (135.0, FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
            (180.0, 0.0, -1.0),
            (225.0, -FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
            (270.0, -1.0, 0.0),
            (315.0, -FRAC_1_SQRT_2, FRAC_1_SQRT_2),
            (360.0, 0.0, 1.0),
        ];

        for (deg, s, c) in cases {
            for turns in [-2.0, -1.0, 0.0, 1.0, 2.0] {
                let a = EdaAngle::new(deg + 360.0 * turns, DEGREES_T);
                assert_eq!(a.sin(), s, "sin {deg} + {turns} turns");
                assert_eq!(a.cos(), c, "cos {deg} + {turns} turns");
            }
        }

        // Other angles go through libm with the *unnormalized* value
        let a = EdaAngle::new(30.0, DEGREES_T);
        assert_eq!(a.sin(), (30.0 * EdaAngle::DEGREES_TO_RADIANS).sin());
        assert_eq!(a.cos(), (30.0 * EdaAngle::DEGREES_TO_RADIANS).cos());
        let b = EdaAngle::new(-330.0, DEGREES_T);
        assert_eq!(b.sin(), (-330.0 * EdaAngle::DEGREES_TO_RADIANS).sin());
        assert_eq!(b.tan(), (-330.0 * EdaAngle::DEGREES_TO_RADIANS).tan());
    }

    #[test]
    fn normalization_ranges_at_boundaries() {
        let n = |d: f64| EdaAngle::new(d, DEGREES_T).normalize().as_degrees();
        let nn = |d: f64| EdaAngle::new(d, DEGREES_T).normalize_negative().as_degrees();
        let n90 = |d: f64| EdaAngle::new(d, DEGREES_T).normalize90().as_degrees();
        let n180 = |d: f64| EdaAngle::new(d, DEGREES_T).normalize180().as_degrees();
        let n720 = |d: f64| EdaAngle::new(d, DEGREES_T).normalize720().as_degrees();

        // [0, 360)
        assert_eq!(n(0.0), 0.0);
        assert_eq!(n(360.0), 0.0);
        assert_eq!(n(-0.0), 0.0);
        assert!(n(-0.0).is_sign_negative()); // -0.0 < -0.0 is false: the sign survives
        assert_eq!(n(-1.0), 359.0);
        assert_eq!(n(1080.5), 0.5);
        // (-360, 0]
        assert_eq!(nn(0.0), 0.0);
        assert_eq!(nn(360.0), 0.0);
        assert_eq!(nn(-360.0), 0.0);
        assert_eq!(nn(1.0), -359.0);
        // [-90, 90]
        assert_eq!(n90(90.0), 90.0);
        assert_eq!(n90(-90.0), -90.0);
        assert_eq!(n90(91.0), -89.0);
        assert_eq!(n90(270.0), 90.0);
        // (-180, 180]
        assert_eq!(n180(180.0), 180.0);
        assert_eq!(n180(-180.0), 180.0);
        assert_eq!(n180(181.0), -179.0);
        // [-360, 360)
        assert_eq!(n720(-360.0), -360.0);
        assert_eq!(n720(360.0), 0.0);
        assert_eq!(n720(-361.0), -1.0);
        assert_eq!(n720(725.0), 5.0);

        // normalize() mutates and returns the same value; normalized() leaves self alone
        let mut a = EdaAngle::new(450.0, DEGREES_T);
        let b = a.normalized();
        assert_eq!(a.as_degrees(), 450.0);
        assert_eq!(b.as_degrees(), 90.0);
        let c = a.normalize();
        assert_eq!(a.as_degrees(), 90.0);
        assert_eq!(c.as_degrees(), 90.0);
    }

    #[test]
    fn cardinal_tests() {
        for d in [0.0, 90.0, 180.0, 270.0, 360.0, -90.0, -450.0, 900.0] {
            assert!(EdaAngle::new(d, DEGREES_T).is_cardinal(), "{d}");
        }

        for d in [45.0, 0.1, 89.999, -45.0, f64::NAN] {
            assert!(!EdaAngle::new(d, DEGREES_T).is_cardinal(), "{d}");
        }

        for d in [90.0, 270.0, -90.0, -270.0, 450.0] {
            assert!(EdaAngle::new(d, DEGREES_T).is_cardinal90(), "{d}");
        }

        for d in [0.0, 180.0, 360.0, 45.0, f64::NAN] {
            assert!(!EdaAngle::new(d, DEGREES_T).is_cardinal90(), "{d}");
        }

        assert!(ANGLE_0.is_zero());
        assert!(ANGLE_0.is_horizontal() && ANGLE_180.is_horizontal() && !ANGLE_360.is_horizontal());
        assert!(ANGLE_90.is_vertical() && ANGLE_270.is_vertical() && !ANGLE_0.is_vertical());
    }

    #[test]
    fn parallel_and_upright() {
        assert!(ANGLE_0.is_parallel_to(ANGLE_180));
        assert!(ANGLE_90.is_parallel_to(ANGLE_270));
        assert!(ANGLE_90.is_parallel_to(-ANGLE_90));
        assert!(!ANGLE_0.is_parallel_to(ANGLE_90));
        assert!(ANGLE_45.is_parallel_to(EdaAngle::new(225.0, DEGREES_T)));
        assert!(!ANGLE_45.is_parallel_to(ANGLE_135));

        let up = |d: f64| EdaAngle::new(d, DEGREES_T).keep_upright().as_degrees();
        assert_eq!(up(0.0), 0.0);
        assert_eq!(up(45.0), 0.0);
        assert_eq!(up(46.0), 90.0);
        assert_eq!(up(90.0), 90.0);
        assert_eq!(up(135.0), 90.0);
        assert_eq!(up(136.0), 0.0);
        assert_eq!(up(180.0), 0.0);
        assert_eq!(up(225.0), 0.0);
        assert_eq!(up(226.0), 90.0);
        assert_eq!(up(270.0), 90.0);
        assert_eq!(up(314.0), 90.0);
        assert_eq!(up(315.0), 0.0);
        assert_eq!(up(-45.0), 0.0);
    }

    #[test]
    fn operators_and_display() {
        let a = EdaAngle::new(30.0, DEGREES_T);
        let b = EdaAngle::new(45.0, DEGREES_T);

        assert_eq!((a + b).as_degrees(), 75.0);
        assert_eq!((a - b).as_degrees(), -15.0);
        assert_eq!((-a).as_degrees(), -30.0);
        assert_eq!((a * 2.0).as_degrees(), 60.0);
        assert_eq!((a * 2).as_degrees(), 60.0);
        assert_eq!((a / 2.0).as_degrees(), 15.0);
        assert_eq!((a / 2).as_degrees(), 15.0);
        assert_eq!(b / a, 1.5);
        assert!(a < b && b > a && a <= a && a >= a && a != b);
        assert_eq!((a - b).abs().as_degrees(), 15.0);
        assert_eq!(a.round(0).as_degrees(), 30.0);
        assert_eq!(EdaAngle::new(12.3456, DEGREES_T).round(2).as_degrees(), 12.35);
        assert_eq!(a.to_string(), "30");
        assert_eq!(EdaAngle::new(22.5, DEGREES_T).to_string(), "22.5");
        assert_eq!(EdaOrientation::new(EdaAngle::new(-90.0, DEGREES_T)).to_string(), "270");

        let mut c = a;
        c += b;
        assert_eq!(c.as_degrees(), 75.0);
        c -= a;
        assert_eq!(c.as_degrees(), 45.0);

        assert_eq!(EdaAngle::arccos(0.0).as_degrees(), 0.0f64.acos() / EdaAngle::DEGREES_TO_RADIANS);
        assert_eq!(EdaAngle::arcsin(1.0).as_degrees(), 1.0f64.asin() / EdaAngle::DEGREES_TO_RADIANS);
        assert_eq!(EdaAngle::arctan(1.0).as_degrees(), 1.0f64.atan() / EdaAngle::DEGREES_TO_RADIANS);
        assert_eq!(EdaAngle::arctan2(1.0, -1.0).as_degrees(), 1.0f64.atan2(-1.0) / EdaAngle::DEGREES_TO_RADIANS);
    }
}
