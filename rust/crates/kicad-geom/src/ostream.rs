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

//! Support code without a C++ source file of its own: the default formatting of floating
//! point numbers by `std::ostream` (`operator<<( double )`), which is what
//! `VECTOR2<T>::Format()`, `BOX2<Vec>::Format()` and the stream operators of `VECTOR2`,
//! `EDA_ANGLE` and `SEG` produce for `double` coordinates.
//!
//! A default-constructed `std::ostream` formats floating point values like
//! `printf( "%g" )`, i.e. with a precision of 6 significant digits, switching to exponent
//! notation when the decimal exponent is below -4 or not below the precision, and with
//! trailing zeros removed.

/// Format `value` like C `printf( "%.{precision}g", value )`.
///
/// This follows the C standard's definition of the `g` conversion: let `P` be the precision
/// (1 if it is 0) and `X` the decimal exponent the `e` conversion would use; if
/// `P > X >= -4` the fixed notation with `P - 1 - X` decimals is used, otherwise the
/// exponent notation with `P - 1` decimals; trailing zeros (and a trailing decimal point)
/// are then removed.  The exponent is written with a sign and at least two digits.
pub fn format_g(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() };
    }

    if value.is_infinite() {
        return if value < 0.0 { "-inf".to_string() } else { "inf".to_string() };
    }

    let p = if precision == 0 { 1 } else { precision };

    // The exponent of the value after rounding to P significant digits
    let exp_repr = format!("{:.*e}", p - 1, value);
    let (mantissa, exponent) = exp_repr.split_once('e').unwrap_or((exp_repr.as_str(), "0"));
    let x: i32 = exponent.parse().unwrap_or(0);

    if x >= -4 && x < p as i32 {
        let fixed = format!("{:.*}", (p as i32 - 1 - x) as usize, value);

        strip_trailing_zeros(&fixed).to_string()
    } else {
        let sign = if x < 0 { '-' } else { '+' };

        format!("{}e{}{:02}", strip_trailing_zeros(mantissa), sign, x.unsigned_abs())
    }
}

/// Format `value` the way `std::ostream << double` does with default stream settings
/// (`%g`, precision 6).
pub fn format_double(value: f64) -> String {
    format_g(value, 6)
}

/// Remove trailing zeros of the fractional part, and the decimal point if nothing remains.
fn strip_trailing_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_double_matches_printf_g() {
        // Expected strings produced by glibc printf( "%g" )
        assert_eq!(format_double(0.0), "0");
        assert_eq!(format_double(-0.0), "-0");
        assert_eq!(format_double(1.0), "1");
        assert_eq!(format_double(-1.0), "-1");
        assert_eq!(format_double(1.5), "1.5");
        assert_eq!(format_double(-2.5e-7), "-2.5e-07");
        assert_eq!(format_double(1234567.0), "1.23457e+06");
        assert_eq!(format_double(0.1 + 0.2), "0.3");
        assert_eq!(format_double(100000.0), "100000");
        assert_eq!(format_double(999999.0), "999999");
        assert_eq!(format_double(999999.5), "1e+06");
        assert_eq!(format_double(1000000.0), "1e+06");
        assert_eq!(format_double(0.0001), "0.0001");
        assert_eq!(format_double(0.00001), "1e-05");
        assert_eq!(format_double(0.000123456789), "0.000123457");
        assert_eq!(format_double(123.456), "123.456");
        assert_eq!(format_double(1e100), "1e+100");
        assert_eq!(format_double(1.5e-300), "1.5e-300");
        assert_eq!(format_double(f64::INFINITY), "inf");
        assert_eq!(format_double(f64::NEG_INFINITY), "-inf");
        assert_eq!(format_double(f64::NAN), "nan");
        assert_eq!(format_double(360.0), "360");
        assert_eq!(format_double(45.5), "45.5");
        assert_eq!(format_double(1e19), "1e+19");
        assert_eq!(format_double(2147483647.0), "2.14748e+09");

        // Exact ties round to even, as glibc does in the default rounding mode
        assert_eq!(format_double(524288.5), "524288");
        assert_eq!(format_double(524289.5), "524290");
        assert_eq!(format_double(0.5), "0.5");
    }

    #[test]
    fn format_g_other_precisions() {
        assert_eq!(format_g(1234567.0, 10), "1234567");
        assert_eq!(format_g(0.1, 17), "0.10000000000000001");
        assert_eq!(format_g(2.5, 1), "2");
        assert_eq!(format_g(3.5, 0), "4");
        assert_eq!(format_g(123456.0, 3), "1.23e+05");
    }
}
