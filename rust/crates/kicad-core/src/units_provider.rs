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

//! Port of `include/units_provider.h` (`UNITS_PROVIDER`): an internal-unit scale plus the
//! user's display units, with the value <-> text helpers of [`crate::eda_units::ui`].
//!
//! `GetOriginTransforms()` (identity unless a frame overrides it) is not ported: no in-scope
//! caller transforms coordinates for display. Angles are passed as degrees (`EDA_ANGLE` lives
//! in kicad-geom, which this crate must not depend on).

use crate::base_units::{EdaIuScale, UNITY_SCALE};
use crate::eda_units::{self, EdaDataType, EdaUnits, ui};
use crate::minoptmax::MinOptMax;
use crate::wxstr::ki_round_i32;

/// Port of `UNITS_PROVIDER::NullUiString`: the UI text of a null (absent) value.
pub const NULL_UI_STRING: &str = "";

/// Port of `class UNITS_PROVIDER`.
#[derive(Clone, Copy, Debug)]
pub struct UnitsProvider<'a> {
    /// `m_iuScale` (invariant).
    pub iu_scale: &'a EdaIuScale,
    /// `m_userUnits`.
    pub user_units: EdaUnits,
}

impl<'a> UnitsProvider<'a> {
    /// `UNITS_PROVIDER( aIuScale, aUnits )`.
    pub fn new(iu_scale: &'a EdaIuScale, units: EdaUnits) -> Self {
        Self {
            iu_scale,
            user_units: units,
        }
    }

    /// `GetUserUnits()`.
    pub fn user_units(&self) -> EdaUnits {
        self.user_units
    }

    /// `SetUserUnits()`.
    pub fn set_user_units(&mut self, units: EdaUnits) {
        self.user_units = units;
    }

    /// `GetIuScale()`.
    pub fn iu_scale(&self) -> &EdaIuScale {
        self.iu_scale
    }

    /// Port of the default `GetUnitPair`: (user units, mm for imperial / mils for metric).
    pub fn unit_pair(&self) -> (EdaUnits, EdaUnits) {
        let primary = self.user_units;
        let secondary = if eda_units::is_imperial_unit(primary) {
            EdaUnits::Mm
        } else {
            EdaUnits::Mils
        };
        (primary, secondary)
    }

    /// Port of `StringFromValue( double, bool, EDA_DATA_TYPE )`.
    pub fn string_from_value(
        &self,
        value: f64,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        ui::string_from_value(
            self.iu_scale,
            self.units_from_type(type_),
            value,
            add_unit_label,
            type_,
        )
    }

    /// Port of `StringFromOptionalValue`: [`NULL_UI_STRING`] for `None`.
    pub fn string_from_optional_value(
        &self,
        value: Option<i32>,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        match value {
            None => NULL_UI_STRING.to_string(),
            Some(v) => ui::string_from_value(
                self.iu_scale,
                self.units_from_type(type_),
                f64::from(v),
                add_unit_label,
                type_,
            ),
        }
    }

    /// Port of `StringFromValue( const EDA_ANGLE&, bool )` (angle in degrees).
    pub fn string_from_angle(&self, degrees: f64, add_unit_label: bool) -> String {
        ui::string_from_value(
            &UNITY_SCALE,
            EdaUnits::Degrees,
            degrees,
            add_unit_label,
            EdaDataType::Distance,
        )
    }

    /// Port of `MessageTextFromValue( double, bool, EDA_DATA_TYPE )`.
    pub fn message_text_from_value(
        &self,
        value: f64,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        ui::message_text_from_value(
            self.iu_scale,
            self.units_from_type(type_),
            value,
            add_unit_label,
            type_,
        )
    }

    /// Port of `MessageTextFromValue( const EDA_ANGLE&, bool )` (angle in degrees).
    pub fn message_text_from_angle(&self, degrees: f64, add_unit_label: bool) -> String {
        ui::message_text_from_value(
            &UNITY_SCALE,
            EdaUnits::Degrees,
            degrees,
            add_unit_label,
            EdaDataType::Distance,
        )
    }

    /// Port of `MessageTextFromUnscaledValue`.
    pub fn message_text_from_unscaled_value(
        &self,
        value: f64,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        ui::message_text_from_value(
            &UNITY_SCALE,
            EdaUnits::Unscaled,
            value,
            add_unit_label,
            type_,
        )
    }

    /// Port of `MessageTextFromMinOptMax`.
    pub fn message_text_from_min_opt_max(
        &self,
        value: &MinOptMax<i32>,
        type_: EdaDataType,
    ) -> String {
        ui::message_text_from_min_opt_max(self.iu_scale, self.units_from_type(type_), value)
    }

    /// Port of `ValueFromString`: `KiROUND<double, int>( DoubleValueFromString( ... ) )`.
    pub fn value_from_string(&self, text: &str, type_: EdaDataType) -> i32 {
        let value =
            ui::double_value_from_string(self.iu_scale, self.units_from_type(type_), text, type_);
        ki_round_i32(value)
    }

    /// Port of `OptionalValueFromString`: `None` for [`NULL_UI_STRING`].
    pub fn optional_value_from_string(&self, text: &str, type_: EdaDataType) -> Option<i32> {
        if text == NULL_UI_STRING {
            return None;
        }

        Some(self.value_from_string(text, type_))
    }

    /// Port of `AngleValueFromString` (degrees).
    pub fn angle_value_from_string(&self, text: &str) -> f64 {
        ui::double_value_from_string(
            self.iu_scale,
            EdaUnits::Degrees,
            text,
            EdaDataType::Distance,
        )
    }

    /// Port of `GetUnitsFromType`.
    pub fn units_from_type(&self, type_: EdaDataType) -> EdaUnits {
        if type_ == EdaDataType::Time {
            EdaUnits::Ps
        } else if type_ == EdaDataType::LengthDelay {
            if eda_units::is_metric_unit(self.user_units) {
                EdaUnits::PsPerCm
            } else {
                EdaUnits::PsPerInch
            }
        } else {
            self.user_units
        }
    }

    /// Port of `GetTypeFromUnits`.
    pub fn type_from_units(units: EdaUnits) -> EdaDataType {
        match units {
            EdaUnits::Inch | EdaUnits::Mm | EdaUnits::Um | EdaUnits::Cm | EdaUnits::Mils => {
                EdaDataType::Distance
            }
            EdaUnits::Degrees => EdaDataType::Angle,
            EdaUnits::Percent | EdaUnits::Unscaled => EdaDataType::Unitless,
            EdaUnits::Fs | EdaUnits::Ps => EdaDataType::Time,
            EdaUnits::PsPerInch | EdaUnits::PsPerCm | EdaUnits::PsPerMm => EdaDataType::LengthDelay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_units::PCB_IU_SCALE;

    #[test]
    fn units_from_type_and_pairs() {
        let mut p = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm);
        assert_eq!(p.unit_pair(), (EdaUnits::Mm, EdaUnits::Mils));
        assert_eq!(
            p.units_from_type(EdaDataType::LengthDelay),
            EdaUnits::PsPerCm
        );
        assert_eq!(p.units_from_type(EdaDataType::Time), EdaUnits::Ps);
        p.set_user_units(EdaUnits::Inch);
        assert_eq!(p.unit_pair(), (EdaUnits::Inch, EdaUnits::Mm));
        assert_eq!(
            p.units_from_type(EdaDataType::LengthDelay),
            EdaUnits::PsPerInch
        );
        assert_eq!(
            UnitsProvider::type_from_units(EdaUnits::PsPerMm),
            EdaDataType::LengthDelay
        );
    }

    #[test]
    fn value_round_trip() {
        let p = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm);
        assert_eq!(p.value_from_string("1.5", EdaDataType::Distance), 1_500_000);
        assert_eq!(
            p.optional_value_from_string("", EdaDataType::Distance),
            None
        );
        assert_eq!(
            p.string_from_optional_value(None, true, EdaDataType::Distance),
            ""
        );
        assert_eq!(
            p.string_from_value(1_500_000.0, true, EdaDataType::Distance),
            "1.5 mm"
        );
    }
}
