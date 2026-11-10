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

//! The part of `include/eda_units.h`, `include/base_units.h` and `common/eda_units.cpp` that
//! the expression evaluators and their unit resolvers need: the `EDA_UNITS` enumeration
//! attached to values, `EDA_IU_SCALE`, and the `EDA_UNIT_UTILS::UI::DoubleValueFromString` /
//! `FromUserUnit` conversions of numeric literals.
//!
//! These are the `kicad-core` ports (`kicad_core::eda_units`, `kicad_core::base_units`),
//! re-exported under the names this crate has always used; the two `DoubleValueFromString`
//! overloads keep this crate's function names (in `kicad-core` they are
//! `ui::double_value_from_string_unscaled` and `ui::double_value_from_string`).

pub use kicad_core::base_units::{EdaIuScale, PCB_IU_SCALE};
pub use kicad_core::eda_units::ui::from_user_unit;
pub use kicad_core::eda_units::{EdaDataType, EdaUnits};

/// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const wxString& aTextValue )` (the
/// overload without scale and units): `kicad_core::eda_units::ui::double_value_from_string_unscaled`.
///
/// The text is stripped, both `.` and `,` are taken as the decimal separator, and the
/// leading run of digits, separators and signs is converted with `strtod` semantics
/// (so `"1,5"` is 1.5, `"1.2.3"` is 1.2 and anything unparsable is 0).
pub fn double_value_from_string(text_value: &str) -> f64 {
    kicad_core::eda_units::ui::double_value_from_string_unscaled(text_value)
}

/// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const EDA_IU_SCALE& aIuScale,
/// EDA_UNITS aUnits, const wxString& aTextValue, EDA_DATA_TYPE aType )`: convert a text given
/// in `units` (or in the unit named by an optional suffix of the text) to internal units
/// (`kicad_core::eda_units::ui::double_value_from_string`).
pub fn double_value_from_string_with_units(
    iu_scale: &EdaIuScale,
    units: EdaUnits,
    text_value: &str,
    type_: EdaDataType,
) -> f64 {
    kicad_core::eda_units::ui::double_value_from_string(iu_scale, units, text_value, type_)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unitless_values() {
        assert_eq!(double_value_from_string("1"), 1.0);
        assert_eq!(double_value_from_string("1.5"), 1.5);
        assert_eq!(double_value_from_string("1,5"), 1.5);
        assert_eq!(double_value_from_string(" 1000.05 "), 1000.05);
        assert_eq!(double_value_from_string("1."), 1.0);
        assert_eq!(double_value_from_string("1.5mm"), 1.5);
        assert_eq!(double_value_from_string("-3"), -3.0);
        assert_eq!(double_value_from_string("abc"), 0.0);
        assert_eq!(double_value_from_string(""), 0.0);
        assert_eq!(double_value_from_string("1.2.3"), 1.2);
    }

    #[test]
    fn values_with_units() {
        let mm = |t: &str| {
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Mm,
                t,
                EdaDataType::Distance,
            )
        };
        let mils = |t: &str| {
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Mils,
                t,
                EdaDataType::Distance,
            )
        };
        let inch = |t: &str| {
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Inch,
                t,
                EdaDataType::Distance,
            )
        };

        assert_eq!(mm("1"), 1e6);
        assert_eq!(mm("10"), 10e6);
        assert_eq!(mm("0.2"), 200000.0);
        assert_eq!(mils("1"), 25400.0);
        assert_eq!(inch("1"), 25400000.0);
        assert_eq!(mm("1in"), 25400000.0);
        assert_eq!(mm("1 mil"), 25400.0);
        assert_eq!(mm("1um"), 1000.0);
        assert_eq!(mm("1cm"), 10e6);
        assert_eq!(mm("2 thou"), 50800.0);
        assert_eq!(mm("1\""), 25400000.0);
        assert_eq!(
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Ps,
                "2",
                EdaDataType::Distance
            ),
            2e6
        );
        assert_eq!(
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Fs,
                "2",
                EdaDataType::Distance
            ),
            2000.0
        );
        assert_eq!(
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Mm,
                "2",
                EdaDataType::Area
            ),
            2e12
        );
        assert_eq!(
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                EdaUnits::Degrees,
                "45",
                EdaDataType::Distance
            ),
            45.0
        );
        assert_eq!(PCB_IU_SCALE.iu_per_mils, 25400.0);
    }

    #[test]
    fn enum_values_match_cpp() {
        for v in 0..13 {
            assert_eq!(EdaUnits::from_i32(v).map(EdaUnits::as_i32), Some(v));
        }

        assert_eq!(EdaUnits::from_i32(13), None);
        assert_eq!(EdaUnits::default(), EdaUnits::Unscaled);
    }
}
