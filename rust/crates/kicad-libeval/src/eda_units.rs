// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012-2016 Jean-Pierre Charras, jp.charras at wanadoo.fr
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

//! Port of the part of `include/eda_units.h`, `include/base_units.h` and
//! `common/eda_units.cpp` that the expression evaluators and their unit resolvers need: the
//! `EDA_UNITS` enumeration attached to values, `EDA_IU_SCALE`, and the
//! `EDA_UNIT_UTILS::UI::DoubleValueFromString` / `FromUserUnit` conversions of numeric
//! literals.
//!
//! The complete units module belongs to `kicad-core`; this crate deliberately has no
//! dependencies, so these items are duplicated here (see `PORT_STATUS.md`).

use crate::crt;

/// Port of `enum class EDA_UNITS` (same discriminants as the C++ enumeration).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum EdaUnits {
    /// `EDA_UNITS::INCH`
    Inch = 0,
    /// `EDA_UNITS::MM`
    Mm = 1,
    /// `EDA_UNITS::UNSCALED`
    #[default]
    Unscaled = 2,
    /// `EDA_UNITS::DEGREES`
    Degrees = 3,
    /// `EDA_UNITS::PERCENT`
    Percent = 4,
    /// `EDA_UNITS::MILS`
    Mils = 5,
    /// `EDA_UNITS::UM`
    Um = 6,
    /// `EDA_UNITS::CM`
    Cm = 7,
    /// `EDA_UNITS::FS` (femtoseconds)
    Fs = 8,
    /// `EDA_UNITS::PS` (picoseconds)
    Ps = 9,
    /// `EDA_UNITS::PS_PER_INCH`
    PsPerInch = 10,
    /// `EDA_UNITS::PS_PER_CM`
    PsPerCm = 11,
    /// `EDA_UNITS::PS_PER_MM`
    PsPerMm = 12,
}

impl EdaUnits {
    /// The numeric value of the C++ enumerator.
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// The enumerator with the given C++ numeric value.
    pub fn from_i32(value: i32) -> Option<EdaUnits> {
        Some(match value {
            0 => EdaUnits::Inch,
            1 => EdaUnits::Mm,
            2 => EdaUnits::Unscaled,
            3 => EdaUnits::Degrees,
            4 => EdaUnits::Percent,
            5 => EdaUnits::Mils,
            6 => EdaUnits::Um,
            7 => EdaUnits::Cm,
            8 => EdaUnits::Fs,
            9 => EdaUnits::Ps,
            10 => EdaUnits::PsPerInch,
            11 => EdaUnits::PsPerCm,
            12 => EdaUnits::PsPerMm,
            _ => return None,
        })
    }
}

/// Port of `enum class EDA_DATA_TYPE`: the type of unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EdaDataType {
    /// `EDA_DATA_TYPE::DISTANCE`
    #[default]
    Distance = 0,
    /// `EDA_DATA_TYPE::AREA`
    Area = 1,
    /// `EDA_DATA_TYPE::VOLUME`
    Volume = 2,
    /// `EDA_DATA_TYPE::UNITLESS`
    Unitless = 3,
    /// `EDA_DATA_TYPE::TIME`
    Time = 4,
    /// `EDA_DATA_TYPE::LENGTH_DELAY`
    LengthDelay = 5,
    /// `EDA_DATA_TYPE::ANGLE`: angles are unscaled, but not unitless
    Angle = 6,
}

/// Port of `struct EDA_IU_SCALE` (`include/base_units.h`): the internal units scale of an
/// application.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdaIuScale {
    /// `IU_PER_MM`
    pub iu_per_mm: f64,
    /// `IU_PER_MILS`
    pub iu_per_mils: f64,
    /// `IU_PER_PS`: internal time units are attoseconds
    pub iu_per_ps: f64,
    /// `IU_PER_PS_PER_MM`: internal delay units are attoseconds/mm
    pub iu_per_ps_per_mm: f64,
    /// `MM_PER_IU`
    pub mm_per_iu: f64,
    /// `MILS_PER_IU`
    pub mils_per_iu: f64,
}

impl EdaIuScale {
    /// Port of `EDA_IU_SCALE::EDA_IU_SCALE( double aIUPerMM )`.
    pub const fn new(iu_per_mm: f64) -> Self {
        let iu_per_mils = iu_per_mm * 0.0254;

        EdaIuScale {
            iu_per_mm,
            iu_per_mils,
            iu_per_ps: 1e6,
            iu_per_ps_per_mm: 1e6,
            mm_per_iu: 1.0 / iu_per_mm,
            mils_per_iu: 1.0 / iu_per_mils,
        }
    }
}

/// Port of `pcbIUScale`: pcbnew's internal unit is 1 nanometer (`PCB_IU_PER_MM` = 1e6).
pub const PCB_IU_SCALE: EdaIuScale = EdaIuScale::new(1e6);

/// Port of `EDA_UNIT_UTILS::UI::FromUserUnit`: convert `value` given in `units` to internal
/// units.
pub fn from_user_unit(iu_scale: &EdaIuScale, units: EdaUnits, value: f64) -> f64 {
    match units {
        EdaUnits::Um => value / 1000.0 * iu_scale.iu_per_mm,
        EdaUnits::Mm => value * iu_scale.iu_per_mm,
        EdaUnits::Cm => value * 10.0 * iu_scale.iu_per_mm,
        EdaUnits::Mils => value * iu_scale.iu_per_mils,
        EdaUnits::Inch => value * iu_scale.iu_per_mils * 1000.0,
        EdaUnits::Fs => value / 1000.0 * iu_scale.iu_per_ps,
        EdaUnits::Ps => value * iu_scale.iu_per_ps,
        EdaUnits::PsPerInch => value / 25.4 * iu_scale.iu_per_ps_per_mm,
        EdaUnits::PsPerCm => value / 10.0 * iu_scale.iu_per_ps_per_mm,
        EdaUnits::PsPerMm => value * iu_scale.iu_per_ps_per_mm,
        EdaUnits::Degrees | EdaUnits::Unscaled | EdaUnits::Percent => value,
    }
}

/// The common first part of the `DoubleValueFromString` overloads: strip the text, accept
/// both `.` and `,` as decimal separator, convert the leading run of digits, separators and
/// signs with `wxString::ToDouble` semantics.  Returns the value and the rest of the text.
fn numeric_part(text_value: &str) -> (f64, String) {
    let mut dtmp = 0.0;

    // Acquire the 'right' decimal point separator
    let decimal_point = crt::LOCALE_DECIMAL_POINT;
    let mut buf = crt::strip_both(text_value).to_string();

    // Convert any entered decimal point separators to the 'right' one
    buf = buf.replace('.', &decimal_point.to_string());
    buf = buf.replace(',', &decimal_point.to_string());

    // Find the end of the numeric part
    let mut brk_point = 0;

    for ch in buf.chars() {
        if !(ch.is_ascii_digit() || ch == decimal_point || ch == '-' || ch == '+') {
            break;
        }

        // Only ASCII characters are accepted, so the char count is also the byte offset.
        brk_point += 1;
    }

    // Extract the numeric part
    if let (Some(value), _) = crt::to_double(&buf[..brk_point]) {
        dtmp = value;
    }

    (dtmp, buf[brk_point..].to_string())
}

/// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const wxString& aTextValue )` (the
/// overload without scale and units).
///
/// The text is stripped, both `.` and `,` are taken as the decimal separator, and the
/// leading run of digits, separators and signs is converted with `strtod` semantics
/// (so `"1,5"` is 1.5, `"1.2.3"` is 1.2 and anything unparsable is 0).
pub fn double_value_from_string(text_value: &str) -> f64 {
    numeric_part(text_value).0
}

/// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const EDA_IU_SCALE& aIuScale,
/// EDA_UNITS aUnits, const wxString& aTextValue, EDA_DATA_TYPE aType )`: convert a text given
/// in `units` (or in the unit named by an optional suffix of the text) to internal units.
pub fn double_value_from_string_with_units(
    iu_scale: &EdaIuScale,
    mut units: EdaUnits,
    text_value: &str,
    type_: EdaDataType,
) -> f64 {
    let (mut dtmp, rest) = numeric_part(text_value);

    // Check the optional unit designator (2 ch significant)
    let rest = crt::trim_left(&rest);
    let unit: String = rest.chars().take(2).map(crt::to_lower).collect();

    if units == EdaUnits::Um
        || units == EdaUnits::Mm
        || units == EdaUnits::Cm
        || units == EdaUnits::Mils
        || units == EdaUnits::Inch
    {
        //check for um, μm (µ is MICRO SIGN) and µm (µ is GREEK SMALL LETTER MU) for micrometre
        if unit == "um" || unit == "\u{B5}m" || unit == "\u{3BC}m" {
            units = EdaUnits::Um;
        } else if unit == "mm" {
            units = EdaUnits::Mm;
        } else if unit == "cm" {
            units = EdaUnits::Cm;
        } else if unit == "mi" || unit == "th" {
            units = EdaUnits::Mils;
        } else if unit == "in" || unit == "\"" {
            units = EdaUnits::Inch;
        } else if unit == "oz" {
            // 1 oz = 1.37 mils
            units = EdaUnits::Mils;
            dtmp *= 1.37;
        }
    } else if units == EdaUnits::Degrees {
        if unit == "ra" {
            // Radians  (C++: 180.0f / M_PI)
            dtmp *= 180.0 / std::f64::consts::PI;
        }
    } else if units == EdaUnits::Fs
        || units == EdaUnits::Ps
        || units == EdaUnits::PsPerInch
        || units == EdaUnits::PsPerCm
        || units == EdaUnits::PsPerMm
    {
        let time_unit: String = rest.chars().take(5).map(crt::to_lower).collect();

        if time_unit == "fs" {
            units = EdaUnits::Fs;
        }

        if time_unit == "ps" {
            units = EdaUnits::Ps;
        } else if time_unit == "ps/in" {
            units = EdaUnits::PsPerInch;
        } else if time_unit == "ps/cm" {
            units = EdaUnits::PsPerCm;
        } else if time_unit == "ps/mm" {
            units = EdaUnits::PsPerMm;
        }
    }

    match type_ {
        EdaDataType::Volume => {
            dtmp = from_user_unit(iu_scale, units, dtmp);
            dtmp = from_user_unit(iu_scale, units, dtmp);
            dtmp = from_user_unit(iu_scale, units, dtmp);
        }
        EdaDataType::Area => {
            dtmp = from_user_unit(iu_scale, units, dtmp);
            dtmp = from_user_unit(iu_scale, units, dtmp);
        }
        EdaDataType::Distance => {
            dtmp = from_user_unit(iu_scale, units, dtmp);
        }
        EdaDataType::Unitless | EdaDataType::Angle => {}
        EdaDataType::Time => {
            dtmp = from_user_unit(iu_scale, units, dtmp);
        }
        EdaDataType::LengthDelay => {
            dtmp = from_user_unit(iu_scale, units, dtmp);
        }
    }

    dtmp
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
