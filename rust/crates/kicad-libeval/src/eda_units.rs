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

//! Port of the small part of `include/eda_units.h` and `common/eda_units.cpp` that LIBEVAL
//! itself needs: the `EDA_UNITS` enumeration attached to values and
//! `EDA_UNIT_UTILS::UI::DoubleValueFromString( const wxString& )`, which converts unit-less
//! numeric literals.
//!
//! The complete units module belongs to `kicad-core`; this crate deliberately has no
//! dependencies, so the two items are duplicated here (see `PORT_STATUS.md`).

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

/// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const wxString& aTextValue )` (the
/// overload without scale and units).
///
/// The text is stripped, both `.` and `,` are taken as the decimal separator, and the
/// leading run of digits, separators and signs is converted with `strtod` semantics
/// (so `"1,5"` is 1.5, `"1.2.3"` is 1.2 and anything unparsable is 0).
pub fn double_value_from_string(text_value: &str) -> f64 {
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
    fn enum_values_match_cpp() {
        for v in 0..13 {
            assert_eq!(EdaUnits::from_i32(v).map(EdaUnits::as_i32), Some(v));
        }

        assert_eq!(EdaUnits::from_i32(13), None);
        assert_eq!(EdaUnits::default(), EdaUnits::Unscaled);
    }
}
