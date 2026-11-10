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

//! Port of `include/eda_units.h` and `common/eda_units.cpp`: the `EDA_UNITS` /
//! `EDA_DATA_TYPE` enums and the `EDA_UNIT_UTILS` namespace (with its `UI` sub-namespace in
//! [`ui`]).
//!
//! `wxString` becomes `String` (see `src/wxstr.rs` for the character semantics) and
//! `wxString::Printf` / `fmt::format` become the printf-exact formatters of
//! [`crate::printf`].  Translated strings (`_( "min" )`) use the English source text.

use crate::base_units::{EdaIuScale, SCH_IU_PER_MM};
use crate::printf::{format_e, format_f, format_g};
use crate::wxstr;

/// Port of `enum class EDA_DATA_TYPE`: the kind of quantity a value measures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum EdaDataType {
    /// `DISTANCE` (the default of the `EDA_DATA_TYPE` parameters in the C++ API)
    #[default]
    Distance = 0,
    /// `AREA`
    Area = 1,
    /// `VOLUME`
    Volume = 2,
    /// `UNITLESS`
    Unitless = 3,
    /// `TIME`
    Time = 4,
    /// `LENGTH_DELAY`
    LengthDelay = 5,
    /// `ANGLE`: angles are unscaled, but not unitless.
    Angle = 6,
}

/// Port of `enum class EDA_UNITS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum EdaUnits {
    /// `INCH` (do not use `IN`: it conflicts with a Windows header)
    Inch = 0,
    /// `MM`
    Mm = 1,
    /// `UNSCALED` (also the [`Default`], as in `kicad-libeval`'s unit resolvers)
    #[default]
    Unscaled = 2,
    /// `DEGREES`
    Degrees = 3,
    /// `PERCENT`
    Percent = 4,
    /// `MILS`
    Mils = 5,
    /// `UM`
    Um = 6,
    /// `CM`
    Cm = 7,
    /// `FS`: femtoseconds
    Fs = 8,
    /// `PS`: picoseconds
    Ps = 9,
    /// `PS_PER_INCH`
    PsPerInch = 10,
    /// `PS_PER_CM`
    PsPerCm = 11,
    /// `PS_PER_MM`
    PsPerMm = 12,
}

impl EdaUnits {
    /// The numeric value of the C++ enumerator.
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// The enumerator with the given C++ numeric value.
    pub fn from_i32(value: i32) -> Option<EdaUnits> {
        usize::try_from(value)
            .ok()
            .and_then(|i| EdaUnits::ALL.get(i).copied())
    }

    /// All units, in enum order.
    pub const ALL: [EdaUnits; 13] = [
        EdaUnits::Inch,
        EdaUnits::Mm,
        EdaUnits::Unscaled,
        EdaUnits::Degrees,
        EdaUnits::Percent,
        EdaUnits::Mils,
        EdaUnits::Um,
        EdaUnits::Cm,
        EdaUnits::Fs,
        EdaUnits::Ps,
        EdaUnits::PsPerInch,
        EdaUnits::PsPerCm,
        EdaUnits::PsPerMm,
    ];
}

impl EdaDataType {
    /// All data types, in enum order.
    pub const ALL: [EdaDataType; 7] = [
        EdaDataType::Distance,
        EdaDataType::Area,
        EdaDataType::Volume,
        EdaDataType::Unitless,
        EdaDataType::Time,
        EdaDataType::LengthDelay,
        EdaDataType::Angle,
    ];
}

/// Port of the file-static `removeTrailingZeros`: removes trailing `0`s and then a trailing
/// decimal separator (`.` or `,`).  The first character is never removed.
fn remove_trailing_zeros(text: &mut String) {
    let chars: Vec<char> = text.chars().collect();
    let mut len = chars.len() as isize;
    let mut remove_last = 0usize;

    loop {
        len -= 1;

        if !(len > 0 && chars[len as usize] == '0') {
            break;
        }

        remove_last += 1;
    }

    if len >= 0 && (chars[len as usize] == '.' || chars[len as usize] == ',') {
        remove_last += 1;
    }

    let keep = chars.len() - remove_last;
    *text = chars[..keep].iter().collect();
}

/// Port of `EDA_UNIT_UTILS::IsImperialUnit`.
pub fn is_imperial_unit(unit: EdaUnits) -> bool {
    matches!(unit, EdaUnits::Inch | EdaUnits::Mils)
}

/// Port of `EDA_UNIT_UTILS::IsMetricUnit`.
pub fn is_metric_unit(unit: EdaUnits) -> bool {
    matches!(unit, EdaUnits::Um | EdaUnits::Mm | EdaUnits::Cm)
}

/// Port of `EDA_UNIT_UTILS::Mm2mils`: `KiROUND( aVal * 1000. / 25.4 )`.
pub fn mm2mils(val: f64) -> i32 {
    wxstr::ki_round_i32(val * 1000. / 25.4)
}

/// Port of `EDA_UNIT_UTILS::Mils2mm`: `KiROUND( aVal * 25.4 / 1000. )`.
pub fn mils2mm(val: f64) -> i32 {
    wxstr::ki_round_i32(val * 25.4 / 1000.)
}

/// Port of `EDA_UNIT_UTILS::Mils2IU`.
pub fn mils2iu(iu_scale: &EdaIuScale, mils: i32) -> i32 {
    let x = mils as f64 * iu_scale.iu_per_mils;

    (if x < 0.0 { x - 0.5 } else { x + 0.5 }) as i32
}

/// Port of `EDA_UNIT_UTILS::FetchUnitsFromString`: the units named after the number in
/// `text_value`, or `None` (C++ returns false and leaves `aUnits` untouched).
pub fn fetch_units_from_string(text_value: &str) -> Option<EdaUnits> {
    let buf: Vec<char> = wxstr::strip_both(text_value).chars().collect();
    let mut brk_point = 0;

    while brk_point < buf.len() {
        let c = buf[brk_point];

        if !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+') {
            break;
        }

        brk_point += 1;
    }

    let rest: String = buf[brk_point..].iter().collect();
    let rest = wxstr::trim_left(&rest);

    // Check the unit designator (2 ch significant)
    let unit = wxstr::lower(wxstr::left(rest, 2));

    // check for um, μm (µ is MICRO SIGN) and µm (µ is GREEK SMALL LETTER MU) for micrometre
    let units = if unit == "um" || unit == "\u{00B5}m" || unit == "\u{03BC}m" {
        EdaUnits::Um
    } else if unit == "mm" {
        EdaUnits::Mm
    } else if unit == "cm" {
        EdaUnits::Cm
    } else if unit == "mi" || unit == "th" {
        // "mils" or "thou"
        EdaUnits::Mils
    } else if unit == "in" || unit == "\"" {
        EdaUnits::Inch
    } else if unit == "de" || unit == "ra" {
        // "deg" or "rad"
        EdaUnits::Degrees
    } else if unit == "fs" {
        EdaUnits::Fs
    } else if unit == "ps" {
        let time_unit = wxstr::lower(wxstr::left(rest, 5));

        match time_unit.as_str() {
            "ps" => EdaUnits::Ps,
            "ps/in" => EdaUnits::PsPerInch,
            "ps/cm" => EdaUnits::PsPerCm,
            "ps/mm" => EdaUnits::PsPerMm,
            _ => return None,
        }
    } else {
        return None;
    };

    Some(units)
}

/// Port of `EDA_UNIT_UTILS::GetText`: the units suffix with its leading space (`" mm"`).
///
/// C++ logs `UNIMPLEMENTED_FOR` for `EDA_DATA_TYPE::UNITLESS` and returns the units text
/// unchanged; so does the port (without the log).
pub fn get_text(units: EdaUnits, type_: EdaDataType) -> String {
    let mut label = String::from(match units {
        EdaUnits::Um => " \u{00B5}m", // 00B5 for µ
        EdaUnits::Mm => " mm",
        EdaUnits::Cm => " cm",
        EdaUnits::Degrees => "\u{00B0}",
        EdaUnits::Mils => " mils",
        EdaUnits::Inch => " in",
        EdaUnits::Percent => "%",
        EdaUnits::Fs => " fs",
        EdaUnits::Ps => " ps",
        EdaUnits::PsPerInch => " ps/in",
        EdaUnits::PsPerCm => " ps/cm",
        EdaUnits::PsPerMm => " ps/mm",
        EdaUnits::Unscaled => "",
    });

    match type_ {
        EdaDataType::Volume => label.push('\u{00B3}'),
        EdaDataType::Area => label.push('\u{00B2}'),
        _ => {}
    }

    label
}

/// [`get_text`] with the default `EDA_DATA_TYPE::DISTANCE`.
pub fn get_text_distance(units: EdaUnits) -> String {
    get_text(units, EdaDataType::Distance)
}

/// Port of `EDA_UNIT_UTILS::GetLabel`: [`get_text`] without the leading space.
pub fn get_label(units: EdaUnits, type_: EdaDataType) -> String {
    wxstr::trim_left(&get_text(units, type_)).to_string()
}

/// Port of `EDA_UNIT_UTILS::FormatAngle`: `fmt::format( "{:.10g}", aAngle.AsDegrees() )`.
///
/// Takes the angle in degrees (`EDA_ANGLE::AsDegrees()`), so that this crate does not depend
/// on `kicad-geom`.
pub fn format_angle(degrees: f64) -> String {
    format_g(degrees, 10)
}

/// Port of `EDA_UNIT_UTILS::FormatInternalUnits( const EDA_IU_SCALE&, int, EDA_DATA_TYPE )`:
/// the value in millimetres (or ps, ps/mm, unscaled for the other data types) as written to
/// KiCad files.
///
/// When the divisor is exactly 1e6 (PCB distances, the board writer's hot path) the text is
/// produced by [`push_iu_1e6_exact`] straight from the integer; see there for why that is the
/// same text as the C++ double formatting.  Every other divisor takes the ported generic path
/// [`format_internal_units_generic`].
pub fn format_internal_units(iu_scale: &EdaIuScale, value: i32, data_type: EdaDataType) -> String {
    if get_scale_for_internal_unit_type(iu_scale, data_type) == 1e6 {
        let mut buf = String::with_capacity(16);
        push_iu_1e6_exact(&mut buf, value);
        buf
    } else {
        format_internal_units_generic(iu_scale, value, data_type)
    }
}

/// The statement-by-statement port of `FormatInternalUnits( const EDA_IU_SCALE&, int,
/// EDA_DATA_TYPE )` (double division, `{:.10f}` with trailing zeros stripped for tiny values,
/// `{:.10g}` otherwise).  Used for every divisor other than 1e6, and kept public (hidden) as
/// the reference of the exactness tests of [`push_iu_1e6_exact`].
#[doc(hidden)]
pub fn format_internal_units_generic(
    iu_scale: &EdaIuScale,
    value: i32,
    data_type: EdaDataType,
) -> String {
    let mut eng_units = value as f64;

    eng_units /= get_scale_for_internal_unit_type(iu_scale, data_type);

    if eng_units != 0.0 && eng_units.abs() <= 0.0001 {
        let mut buf = format_f(eng_units, 10);

        // remove trailing zeros
        while buf.ends_with('0') {
            buf.pop();
        }

        // if the value was really small
        // we may have just stripped all the zeros after the decimal
        if buf.ends_with('.') {
            buf.pop();
        }

        buf
    } else {
        format_g(eng_units, 10)
    }
}

/// Appends the exact decimal value of `value / 10^6` to `buf`: an optional `-`, the integer
/// part, and, if the remainder is non-zero, `.` and the six-digit zero-padded remainder with
/// its trailing zeros removed (`0`, `1`, `-0.000001`, `2147.483647`, `-2147.483648`).
///
/// Invariant (why this equals [`format_internal_units_generic`] for a divisor of exactly 1e6):
/// `value as f64 / 1e6` is the correctly rounded quotient, within 2^-53 relative of the exact
/// value, which has at most 10 significant and at most 6 fractional digits (|value| < 2^31).
/// For |q| <= 1e-4 the C++ prints `{:.10f}` (grid 1e-10); otherwise `{:.10g}`, which stays in
/// fixed notation because the decimal exponent is in [-4, 3] and so has a grid of at most 1e-6
/// that contains the exact value.  Half a grid step exceeds the rounding error by a factor of
/// at least 10^5, so both branches print the exact value, and the trailing zeros (and a bare
/// `.`) are then stripped, as here.  `value == 0` gives `{:.10g}` of 0.0 = `0`.  Checked for
/// all 2^32 `i32` values by `tests/format_internal_units_exact.rs` (ignored test).
pub fn push_iu_1e6_exact(buf: &mut String, value: i32) {
    // 64-bit magnitude so that i32::MIN does not overflow.
    let magnitude = (value as i64).unsigned_abs();
    let int_part = magnitude / 1_000_000;
    let mut frac = (magnitude % 1_000_000) as u32;

    if value < 0 {
        buf.push('-');
    }

    let mut digits = [0u8; 10];
    let mut n = 0;
    let mut rest = int_part;

    loop {
        digits[n] = b'0' + (rest % 10) as u8;
        n += 1;
        rest /= 10;

        if rest == 0 {
            break;
        }
    }

    for &d in digits[..n].iter().rev() {
        buf.push(d as char);
    }

    if frac != 0 {
        let mut width = 6;

        while frac % 10 == 0 {
            frac /= 10;
            width -= 1;
        }

        buf.push('.');

        let mut frac_digits = [b'0'; 6];

        for slot in frac_digits[..width].iter_mut().rev() {
            *slot = b'0' + (frac % 10) as u8;
            frac /= 10;
        }

        for &d in &frac_digits[..width] {
            buf.push(d as char);
        }
    }
}

/// [`format_internal_units`] with the default `EDA_DATA_TYPE::DISTANCE`.
pub fn format_internal_units_distance(iu_scale: &EdaIuScale, value: i32) -> String {
    format_internal_units(iu_scale, value, EdaDataType::Distance)
}

/// Port of `EDA_UNIT_UTILS::FormatInternalUnits( const EDA_IU_SCALE&, const VECTOR2I& )`:
/// `"x y"`.  The point is passed as its two coordinates.  For a 1e6 scale both coordinates are
/// written into one `String` by [`push_iu_1e6_exact`] (same text as the generic path).
pub fn format_internal_units_point(iu_scale: &EdaIuScale, x: i32, y: i32) -> String {
    if get_scale_for_internal_unit_type(iu_scale, EdaDataType::Distance) == 1e6 {
        let mut buf = String::with_capacity(32);
        push_iu_1e6_exact(&mut buf, x);
        buf.push(' ');
        push_iu_1e6_exact(&mut buf, y);
        buf
    } else {
        format_internal_units_distance(iu_scale, x) + " " + &format_internal_units_distance(iu_scale, y)
    }
}

/// Port of `EDA_UNIT_UTILS::GetScaleForInternalUnitType`.
pub fn get_scale_for_internal_unit_type(iu_scale: &EdaIuScale, data_type: EdaDataType) -> f64 {
    match data_type {
        EdaDataType::Time => iu_scale.iu_per_ps,
        EdaDataType::LengthDelay => iu_scale.iu_per_ps_per_mm,
        EdaDataType::Unitless => 1.0,
        EdaDataType::Angle => 1.0,
        _ => iu_scale.iu_per_mm,
    }
}

/// Port of `EDA_UNIT_UTILS::ParseInternalUnits( const std::string&, const EDA_IU_SCALE&,
/// int& )`.
///
/// The C++ function is compiled out (`#if 0`, no `std::from_chars` on macOS); the port keeps
/// its semantics: `std::from_chars` (here the `fast_float` port of `kicad-sexpr`, same syntax)
/// on a prefix of the input, then `aOut = value * IU_PER_MM` with the implicit truncating
/// double to int conversion (saturating instead of undefined behaviour out of range).
pub fn parse_internal_units(input: &str, iu_scale: &EdaIuScale) -> Option<i32> {
    let (value, _) = kicad_sexpr::lexer::c_numeric::from_chars_double(input.as_bytes()).ok()?;

    Some((value * iu_scale.iu_per_mm) as i32)
}

/// Port of `EDA_UNIT_UTILS::ParseInternalUnits( const std::string&, const EDA_IU_SCALE&,
/// VECTOR2I& )`: two values separated by the first space; returns `(x, y)`.
pub fn parse_internal_units_point(input: &str, iu_scale: &EdaIuScale) -> Option<(i32, i32)> {
    let pos = input.find(' ')?;
    let x = parse_internal_units(&input[..pos], iu_scale)?;
    let y = parse_internal_units(&input[pos + 1..], iu_scale)?;

    Some((x, y))
}

/// Port of the `EDA_UNIT_UTILS::UI` namespace: conversions between internal units and the
/// user-visible text of dialogs and messages (also used by the DRC report writer).
pub mod ui {
    use super::*;

    /// Port of `EDA_UNIT_UTILS::UI::ToUserUnit`: internal units to the given user units.
    pub fn to_user_unit(iu_scale: &EdaIuScale, unit: EdaUnits, value: f64) -> f64 {
        // IU_TO_MM( x, scale ) ( x / scale.IU_PER_MM ), IU_TO_IN( x, scale )
        // ( x / scale.IU_PER_MILS / 1000 ), ... as macros in C++.
        match unit {
            EdaUnits::Um => value / iu_scale.iu_per_mm * 1000.0,
            EdaUnits::Mm => value / iu_scale.iu_per_mm,
            EdaUnits::Cm => value / iu_scale.iu_per_mm / 10.0,
            EdaUnits::Mils => value / iu_scale.iu_per_mils,
            EdaUnits::Inch => value / iu_scale.iu_per_mils / 1000.0,
            EdaUnits::Degrees => value,
            EdaUnits::Fs => value / iu_scale.iu_per_ps * 1000.0,
            EdaUnits::Ps => value / iu_scale.iu_per_ps,
            EdaUnits::PsPerInch => value / iu_scale.iu_per_ps_per_mm * 25.4,
            EdaUnits::PsPerCm => value / iu_scale.iu_per_ps_per_mm * 10.0,
            EdaUnits::PsPerMm => value / iu_scale.iu_per_ps_per_mm,
            _ => value,
        }
    }

    /// Port of `EDA_UNIT_UTILS::UI::FromUserUnit`: user units to internal units (as a
    /// double).
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

    /// The `switch( aType )` with fall-throughs shared by `StringFromValue` and
    /// `MessageTextFromValue`: volumes are converted three times, areas twice.
    fn convert_for_type(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: f64,
        type_: EdaDataType,
    ) -> f64 {
        let mut v = value;

        match type_ {
            EdaDataType::Volume => {
                v = to_user_unit(iu_scale, units, v);
                v = to_user_unit(iu_scale, units, v);
                v = to_user_unit(iu_scale, units, v);
            }
            EdaDataType::Area => {
                v = to_user_unit(iu_scale, units, v);
                v = to_user_unit(iu_scale, units, v);
            }
            EdaDataType::Distance | EdaDataType::Time | EdaDataType::LengthDelay => {
                v = to_user_unit(iu_scale, units, v);
            }
            EdaDataType::Unitless | EdaDataType::Angle => {}
        }

        v
    }

    /// Port of `EDA_UNIT_UTILS::UI::StringFromValue`: the value in user units with enough
    /// precision to be parsed back (dialog fields), trailing zeros removed.
    pub fn string_from_value(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: f64,
        add_units_text: bool,
        type_: EdaDataType,
    ) -> String {
        let is_eeschema = iu_scale.iu_per_mm == SCH_IU_PER_MM;
        let value_to_print = convert_for_type(iu_scale, units, value, type_);

        let precision = match units {
            EdaUnits::Mils => {
                if is_eeschema {
                    3
                } else {
                    5
                }
            }
            EdaUnits::Inch => {
                if is_eeschema {
                    6
                } else {
                    8
                }
            }
            EdaUnits::Degrees => 4,
            EdaUnits::PsPerInch => 4,
            EdaUnits::PsPerCm => 3,
            EdaUnits::PsPerMm => 3,
            _ => 10,
        };

        let mut text = format_f(value_to_print, precision);
        remove_trailing_zeros(&mut text);

        if value_to_print != 0.0 && (text == "0" || text == "-0") {
            text = format_f(value_to_print, 10);
            remove_trailing_zeros(&mut text);
        }

        if add_units_text {
            text.push_str(&get_text(units, type_));
        }

        text
    }

    /// Port of `EDA_UNIT_UTILS::UI::MessageTextFromValue( const EDA_IU_SCALE&, EDA_UNITS,
    /// double, bool, EDA_DATA_TYPE )`: a lower-precision (for readability) version of
    /// [`string_from_value`].
    pub fn message_text_from_value(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: f64,
        add_units_text: bool,
        type_: EdaDataType,
    ) -> String {
        let short_form = iu_scale.iu_per_mm == SCH_IU_PER_MM
            || (type_ == EdaDataType::Volume || type_ == EdaDataType::Area);

        let value = convert_for_type(iu_scale, units, value, type_);

        let precision = match units {
            EdaUnits::Um => {
                if short_form {
                    0
                } else {
                    1
                }
            }
            EdaUnits::Mm => {
                if short_form {
                    3
                } else {
                    4
                }
            }
            EdaUnits::Cm => {
                if short_form {
                    3
                } else {
                    5
                }
            }
            EdaUnits::Mils => {
                if short_form {
                    0
                } else {
                    2
                }
            }
            EdaUnits::Inch => {
                if short_form {
                    3
                } else {
                    4
                }
            }
            EdaUnits::Degrees => 3,
            EdaUnits::Unscaled => 0,
            EdaUnits::Fs => 4,
            EdaUnits::Ps => 2,
            EdaUnits::PsPerInch => 2,
            EdaUnits::PsPerCm => 2,
            EdaUnits::PsPerMm => 2,
            // `default:` shares the `UM` case
            EdaUnits::Percent => {
                if short_form {
                    0
                } else {
                    1
                }
            }
        };

        let mut text = format_f(value, precision);

        // Check if the formatted value shows only zeros but the actual value is non-zero
        // If so, use scientific notation instead
        if value != 0.0 {
            let shows_only_zeros = !text.chars().any(|ch| ('1'..='9').contains(&ch));

            if shows_only_zeros {
                text = format_e(value, 3);
            }
        }

        // Trim to 2-1/2 digits after the decimal place for short-form mm
        if short_form && units == EdaUnits::Mm {
            let chars: Vec<char> = text.chars().collect();
            let length = chars.len();

            if length > 4
                && chars[length - 4] == wxstr::LOCALE_DECIMAL_POINT
                && chars[length - 1] == '0'
            {
                text = chars[..length - 1].iter().collect();
            }
        }

        if add_units_text {
            text.push_str(&get_text(units, type_));
        }

        text
    }

    /// Port of `EDA_UNIT_UTILS::UI::MessageTextFromValue( ..., int aValue, ... )`.
    pub fn message_text_from_value_int(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: i32,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        message_text_from_value(iu_scale, units, value as f64, add_unit_label, type_)
    }

    /// Port of `EDA_UNIT_UTILS::UI::MessageTextFromValue( ..., long long int aValue, ... )`.
    pub fn message_text_from_value_i64(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: i64,
        add_unit_label: bool,
        type_: EdaDataType,
    ) -> String {
        message_text_from_value(iu_scale, units, value as f64, add_unit_label, type_)
    }

    /// Port of `EDA_UNIT_UTILS::UI::MessageTextFromValue( EDA_ANGLE, bool )`: `"%.1f°"`.
    /// Takes the angle in degrees.
    pub fn message_text_from_angle(degrees: f64, add_unit_label: bool) -> String {
        let mut text = format_f(degrees, 1);

        if add_unit_label {
            text.push('\u{00B0}');
        }

        text
    }

    /// Port of `EDA_UNIT_UTILS::UI::MessageTextFromMinOptMax`:
    /// `"min <v>; opt <v>; max <v>"` (each part only when present; `min` only when > 0).
    pub fn message_text_from_min_opt_max(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        value: &crate::minoptmax::MinOptMax<i32>,
    ) -> String {
        let mut msg = String::new();
        let fmt =
            |v: i32| message_text_from_value_int(iu_scale, units, v, true, EdaDataType::Distance);

        if value.has_min() && value.min() > 0 {
            msg += &("min".to_string() + " " + &fmt(value.min()));
        }

        if value.has_opt() {
            if !msg.is_empty() {
                msg += "; ";
            }

            msg += &("opt".to_string() + " " + &fmt(value.opt()));
        }

        if value.has_max() {
            if !msg.is_empty() {
                msg += "; ";
            }

            msg += &("max".to_string() + " " + &fmt(value.max()));
        }

        msg
    }

    /// The numeric prefix scan shared by the `DoubleValueFromString` overloads: strip, map
    /// `.` and `,` to the decimal point, find the end of the numeric part.  Returns the
    /// characters and the break point.
    fn numeric_prefix(text_value: &str) -> (Vec<char>, usize) {
        let decimal_point = wxstr::LOCALE_DECIMAL_POINT;

        // Convert any entered decimal point separators to the 'right' one
        let buf: Vec<char> = wxstr::strip_both(text_value)
            .chars()
            .map(|c| {
                if c == '.' || c == ',' {
                    decimal_point
                } else {
                    c
                }
            })
            .collect();

        // Find the end of the numeric part
        let mut brk_point = 0;

        while brk_point < buf.len() {
            let ch = buf[brk_point];

            if !(ch.is_ascii_digit() || ch == decimal_point || ch == '-' || ch == '+') {
                break;
            }

            brk_point += 1;
        }

        (buf, brk_point)
    }

    /// `buf.Left( brk_point ).ToDouble( &dtmp )` with `dtmp` initialised to 0.
    fn to_double_or_zero(buf: &[char], brk_point: usize) -> f64 {
        let numeric: String = buf[..brk_point].iter().collect();

        wxstr::to_double(&numeric).0.unwrap_or(0.0)
    }

    /// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const wxString& )`: the number at
    /// the start of the text, units ignored.
    pub fn double_value_from_string_unscaled(text_value: &str) -> f64 {
        let (buf, brk_point) = numeric_prefix(text_value);

        to_double_or_zero(&buf, brk_point)
    }

    /// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const EDA_IU_SCALE&, EDA_UNITS,
    /// const wxString&, EDA_DATA_TYPE )`: parse a user-entered value (with optional units
    /// designator) into internal units.
    pub fn double_value_from_string(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        text_value: &str,
        type_: EdaDataType,
    ) -> f64 {
        let mut units = units;
        let (buf, brk_point) = numeric_prefix(text_value);

        // Extract the numeric part
        let mut dtmp = to_double_or_zero(&buf, brk_point);

        // Check the optional unit designator (2 ch significant)
        let rest: String = buf[brk_point..].iter().collect();
        let rest = wxstr::trim_left(&rest);
        let unit = wxstr::lower(wxstr::left(rest, 2));

        if units == EdaUnits::Um
            || units == EdaUnits::Mm
            || units == EdaUnits::Cm
            || units == EdaUnits::Mils
            || units == EdaUnits::Inch
        {
            // check for um, μm (µ is MICRO SIGN) and µm (µ is GREEK SMALL LETTER MU) for
            // micrometre
            if unit == "um" || unit == "\u{00B5}m" || unit == "\u{03BC}m" {
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
                // Radians (`180.0f / M_PI`: the float literal is exact)
                dtmp *= 180.0 / std::f64::consts::PI;
            }
        } else if units == EdaUnits::Fs
            || units == EdaUnits::Ps
            || units == EdaUnits::PsPerInch
            || units == EdaUnits::PsPerCm
            || units == EdaUnits::PsPerMm
        {
            let time_unit = wxstr::lower(wxstr::left(rest, 5));

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
            EdaDataType::Distance | EdaDataType::Time | EdaDataType::LengthDelay => {
                dtmp = from_user_unit(iu_scale, units, dtmp);
            }
            EdaDataType::Unitless | EdaDataType::Angle => {}
        }

        dtmp
    }

    /// Port of `EDA_UNIT_UTILS::UI::DoubleValueFromString( const EDA_IU_SCALE&, const
    /// wxString&, double& )`: the units designator is mandatory; returns `None` (C++: false,
    /// output untouched) without a number or with unknown units.
    pub fn double_value_from_string_with_units(
        iu_scale: &EdaIuScale,
        text_value: &str,
    ) -> Option<f64> {
        let (buf, brk_point) = numeric_prefix(text_value);

        if brk_point == 0 {
            return None;
        }

        // Extract the numeric part
        let mut dtmp = to_double_or_zero(&buf, brk_point);

        // Check the unit designator
        let rest: String = buf[brk_point..].iter().collect();
        let unit = wxstr::lower(wxstr::strip_both(&rest));
        let mut units = EdaUnits::Mm; // Make gcc quiet

        // check for um, μm (µ is MICRO SIGN) and µm (µ is GREEK SMALL LETTER MU) for micrometre
        if unit == "um" || unit == "\u{00B5}m" || unit == "\u{03BC}m" {
            units = EdaUnits::Um;
        } else if unit == "mm" {
            units = EdaUnits::Mm;
        } else if unit == "cm" {
            units = EdaUnits::Cm;
        } else if unit == "mil" || unit == "mils" || unit == "thou" {
            units = EdaUnits::Mils;
        } else if unit == "in" || unit == "\"" {
            units = EdaUnits::Inch;
        } else if unit == "oz" {
            // 1 oz = 1.37 mils
            units = EdaUnits::Mils;
            dtmp *= 1.37;
        } else if unit == "ra" {
            // Radians
            dtmp *= 180.0 / std::f64::consts::PI;
        } else if unit == "fs" {
            units = EdaUnits::Fs;
        } else if unit == "ps" {
            units = EdaUnits::Ps;
        } else if unit == "ps/in" {
            units = EdaUnits::PsPerInch;
        } else if unit == "ps/cm" {
            units = EdaUnits::PsPerCm;
        } else if unit == "ps/mm" {
            units = EdaUnits::PsPerMm;
        } else {
            return None;
        }

        Some(from_user_unit(iu_scale, units, dtmp))
    }

    /// Port of `EDA_UNIT_UTILS::UI::ValueFromString( const EDA_IU_SCALE&, EDA_UNITS, const
    /// wxString&, EDA_DATA_TYPE )`: [`double_value_from_string`] rounded with
    /// `KiROUND<double, long long>`.
    pub fn value_from_string(
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        text_value: &str,
        type_: EdaDataType,
    ) -> i64 {
        let value = double_value_from_string(iu_scale, units, text_value, type_);

        wxstr::ki_round_i64(value)
    }

    /// Port of `EDA_UNIT_UTILS::UI::ValueFromString( const wxString& )`.
    pub fn value_from_string_unscaled(text_value: &str) -> i64 {
        let value = double_value_from_string_unscaled(text_value);

        wxstr::ki_round_i64(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_zeros() {
        let mut s = String::from("1.2500");
        remove_trailing_zeros(&mut s);
        assert_eq!(s, "1.25");
        let mut s = String::from("0.000");
        remove_trailing_zeros(&mut s);
        assert_eq!(s, "0");
        let mut s = String::from("0");
        remove_trailing_zeros(&mut s);
        assert_eq!(s, "0");
        let mut s = String::new();
        remove_trailing_zeros(&mut s);
        assert_eq!(s, "");
        let mut s = String::from("100");
        remove_trailing_zeros(&mut s);
        assert_eq!(s, "1");
    }
}
