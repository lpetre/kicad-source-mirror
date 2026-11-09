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

//! Floating point conversions with the exact output of C `printf` (`%f`, `%e`, `%g`) and of
//! the `{fmt}` library (`{:.Nf}`, `{:.Ne}`, `{:.Ng}`), which agree for these conversions.
//!
//! `OUTPUTFORMATTER::Print` is a `printf` style function and a few s-expression writers pass
//! doubles straight to it (`Print( "(scale %g)", ... )`, `Print( "(opacity %0.4f)", ... )`),
//! and `SEXPR::AsString` uses `fmt::format( "{:.10g}", ... )`.  Rust's `{}`/`{:e}` formatting
//! differs from both (shortest round-trip digits, `e2` instead of `e+02`, `NaN` instead of
//! `nan`), so those call sites use the functions below instead.
//!
//! The digits come from Rust's `{:.N}` / `{:.Ne}` formatting, which like glibc and `{fmt}`
//! rounds the exact binary value correctly (ties to even), so the results are identical.

/// The text C prints for a non-finite value: `nan`, `-nan`, `inf`, `-inf`.
fn non_finite(value: f64) -> Option<String> {
    let sign = if value.is_sign_negative() { "-" } else { "" };

    if value.is_nan() {
        Some(format!("{sign}nan"))
    } else if value.is_infinite() {
        Some(format!("{sign}inf"))
    } else {
        None
    }
}

/// Split Rust's `{:e}` output (`-1.5e-7`) into mantissa text and exponent value.
fn split_exponent(formatted: &str) -> (&str, i32) {
    match formatted.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse().unwrap_or(0)),
        None => (formatted, 0),
    }
}

/// Append a C style exponent: sign and at least two digits.
fn push_exponent(out: &mut String, exponent: i32) {
    out.push('e');
    out.push(if exponent < 0 { '-' } else { '+' });

    let magnitude = exponent.unsigned_abs();

    if magnitude < 10 {
        out.push('0');
    }

    out.push_str(&magnitude.to_string());
}

/// Remove trailing zeros after a decimal point, and the point itself if nothing is left
/// behind it (what `%g` does without the `#` flag).
fn strip_trailing_zeros(text: &mut String) {
    if text.contains('.') {
        let trimmed = text.trim_end_matches('0').trim_end_matches('.').len();
        text.truncate(trimmed);
    }
}

/// `printf( "%.<precision>f", value )` / `fmt::format( "{:.<precision>f}", value )`.
pub fn format_f(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    format!("{value:.precision$}")
}

/// `printf( "%.<precision>e", value )` / `fmt::format( "{:.<precision>e}", value )`.
pub fn format_e(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    let formatted = format!("{value:.precision$e}");
    let (mantissa, exponent) = split_exponent(&formatted);
    let mut out = String::with_capacity(formatted.len() + 2);

    out.push_str(mantissa);
    push_exponent(&mut out, exponent);
    out
}

/// `printf( "%.<precision>g", value )` / `fmt::format( "{:.<precision>g}", value )`; plain
/// `%g` is `format_g( value, 6 )`.
///
/// The value is rounded to `precision` significant digits (a precision of 0 is treated as 1)
/// and written in fixed notation if its decimal exponent `X` satisfies `-4 <= X < precision`,
/// in exponential notation otherwise; trailing zeros are removed either way.
pub fn format_g(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    let precision = precision.max(1);

    // The exponent X of the value in style e, after rounding to the precision.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = split_exponent(&scientific);

    if exponent < -4 || exponent >= precision as i32 {
        // Style e with precision P - 1.
        let mut out = mantissa.to_string();

        strip_trailing_zeros(&mut out);
        push_exponent(&mut out, exponent);
        out
    } else {
        // Style f with precision P - 1 - X.
        let decimals = (precision as i32 - 1 - exponent) as usize;
        let mut out = format!("{value:.decimals$}");

        strip_trailing_zeros(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected texts were produced with the C library (`printf`), see the oracle harness
    // in tests/oracle.

    #[test]
    fn g_default_precision() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (3.14, "3.14"),
            (100000.0, "100000"),
            (999999.0, "999999"),
            (999999.5, "1e+06"),
            (1000000.0, "1e+06"),
            (1234567.0, "1.23457e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.000123456789, "0.000123457"),
            (0.0000123456789, "1.23457e-05"),
            (4.5, "4.5"),
            (4.35, "4.35"),
            (1.0e100, "1e+100"),
            (1.5e-100, "1.5e-100"),
            (2.5e-5, "2.5e-05"),
            (123456.7, "123457"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.333333"),
            (2.0 / 3.0, "0.666667"),
            (9.9999995, "10"),
            (0.99999949, "0.999999"),
            (0.9999995, "1"),
            (f64::MAX, "1.79769e+308"),
            (f64::MIN_POSITIVE, "2.22507e-308"),
            (5e-324, "4.94066e-324"),
        ];

        for &(value, expected) in cases {
            assert_eq!(format_g(value, 6), expected, "%g of {value:?}");
        }
    }

    #[test]
    fn g_precision_10() {
        let cases: &[(f64, &str)] = &[
            (3.14, "3.14"),
            (42.42, "42.42"),
            (0.1, "0.1"),
            (1.0 / 3.0, "0.3333333333"),
            (1e10, "1e+10"),
            (9999999999.0, "9999999999"),
            (99999999999.0, "1e+11"),
            (99999999994.0, "9.999999999e+10"),
            (12345678912.0, "1.234567891e+10"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (123456789.123456789, "123456789.1"),
            (-2.5e-7, "-2.5e-07"),
            (1e20, "1e+20"),
            (0.30000000000000004, "0.3"),
            (1.0000000006, "1.000000001"),
            (1.0000000004, "1"),
        ];

        for &(value, expected) in cases {
            assert_eq!(format_g(value, 10), expected, "%.10g of {value:?}");
        }
    }

    #[test]
    fn g_other_precisions() {
        assert_eq!(format_g(1234.5678, 0), "1e+03");
        assert_eq!(format_g(1234.5678, 1), "1e+03");
        assert_eq!(format_g(1234.5678, 2), "1.2e+03");
        assert_eq!(format_g(1234.5678, 4), "1235");
        assert_eq!(format_g(1234.5678, 5), "1234.6");
        assert_eq!(format_g(0.5, 1), "0.5");
        assert_eq!(format_g(2.5, 1), "2");
        assert_eq!(format_g(3.5, 1), "4");
        assert_eq!(format_g(0.1, 17), "0.10000000000000001");
        assert_eq!(format_g(0.1, 20), "0.10000000000000000555");
        assert_eq!(format_g(1e23, 17), "9.9999999999999992e+22");
    }

    #[test]
    fn f_conversion() {
        assert_eq!(format_f(0.0, 4), "0.0000");
        assert_eq!(format_f(-0.0, 4), "-0.0000");
        assert_eq!(format_f(1.0, 4), "1.0000");
        assert_eq!(format_f(0.5, 0), "0");
        assert_eq!(format_f(1.5, 0), "2");
        assert_eq!(format_f(2.5, 0), "2");
        assert_eq!(format_f(0.125, 2), "0.12");
        assert_eq!(format_f(0.375, 2), "0.38");
        assert_eq!(format_f(1.005, 2), "1.00");
        assert_eq!(format_f(-0.00001, 4), "-0.0000");
        assert_eq!(format_f(0.99994, 4), "0.9999");
        assert_eq!(format_f(0.99995, 4), "1.0000");
        assert_eq!(format_f(0.99996, 4), "1.0000");
        assert_eq!(format_f(1e20, 10), "100000000000000000000.0000000000");
        assert_eq!(format_f(1e-20, 10), "0.0000000000");
        assert_eq!(format_f(1e22, 2), "10000000000000000000000.00");
        assert_eq!(format_f(1e23, 1), "99999999999999991611392.0");
    }

    #[test]
    fn e_conversion() {
        assert_eq!(format_e(0.0, 6), "0.000000e+00");
        assert_eq!(format_e(-0.0, 2), "-0.00e+00");
        assert_eq!(format_e(1.0, 6), "1.000000e+00");
        assert_eq!(format_e(150.0, 6), "1.500000e+02");
        assert_eq!(format_e(1.5e-7, 2), "1.50e-07");
        assert_eq!(format_e(1e100, 0), "1e+100");
        assert_eq!(format_e(-1.25e-200, 3), "-1.250e-200");
        assert_eq!(format_e(9.995, 2), "9.99e+00");
        assert_eq!(format_e(9.996, 2), "1.00e+01");
    }

    #[test]
    fn non_finite_values() {
        assert_eq!(format_g(f64::NAN, 6), "nan");
        assert_eq!(format_g(-f64::NAN, 6), "-nan");
        assert_eq!(format_g(f64::INFINITY, 10), "inf");
        assert_eq!(format_g(f64::NEG_INFINITY, 10), "-inf");
        assert_eq!(format_f(f64::INFINITY, 2), "inf");
        assert_eq!(format_f(f64::NAN, 2), "nan");
        assert_eq!(format_e(f64::NEG_INFINITY, 2), "-inf");
    }
}
