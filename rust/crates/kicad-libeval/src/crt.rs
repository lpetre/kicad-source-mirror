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

//! Emulation of the few C runtime / wxString primitives the ported code relies on:
//! `strtod`/`atof`/`wxString::ToDouble`, the `printf` conversions `%f` and `%g`, the `ctype`
//! classification functions and the per-character case mapping of `wxString::CmpNoCase` /
//! `MakeUpper`.
//!
//! The C++ code calls these with the process locale in effect.  This port has no locale
//! (see `rust/PORTING.md` section 5): the decimal separator is always `.`, `isalpha` and
//! friends classify ASCII only (the "C" locale; for values above 255 the C++ calls are
//! undefined behaviour anyway) and case mapping is the Unicode simple case mapping (what a
//! UTF-8 locale gives `towlower`/`towupper`).

/// The locale's decimal separator, `localeconv()->decimal_point[0]`.  Always the "C" locale's.
pub(crate) const LOCALE_DECIMAL_POINT: char = '.';

/// C `isdigit` in the "C" locale.
pub(crate) fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// C `isalpha` in the "C" locale.
pub(crate) fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// C `isalnum` in the "C" locale.
pub(crate) fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// C `isspace` in the "C" locale (also what wx's `wxSafeIsspace` accepts).
pub(crate) fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// `towlower` as used by `wxString::CmpNoCase` / `Lower()`: the Unicode *simple* lower-case
/// mapping (one character in, one character out).
pub(crate) fn to_lower(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }

    let mut it = c.to_lowercase();

    match (it.next(), it.next()) {
        (Some(l), None) => l,
        // U+0130 is the only character whose full lower-case mapping is longer than one
        // character; its simple mapping is 'i'.
        _ if c == '\u{130}' => 'i',
        _ => c,
    }
}

/// `towupper` as used by `wxString::MakeUpper()`: the Unicode *simple* upper-case mapping.
pub(crate) fn to_upper(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_uppercase();
    }

    let mut it = c.to_uppercase();

    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => {
            // The full mapping has several characters.  The simple mapping is the character
            // itself (e.g. U+00DF) except for the Greek letters with ypogegrammeni, which map
            // to their single-character "capital with prosgegrammeni" forms.
            match c as u32 {
                0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => {
                    char::from_u32(c as u32 + 8).unwrap_or(c)
                }
                0x1FB3 => '\u{1FBC}',
                0x1FC3 => '\u{1FCC}',
                0x1FF3 => '\u{1FFC}',
                _ => c,
            }
        }
    }
}

/// `wxString::IsSameAs( other, false )` / `CmpNoCase( other ) == 0`: character-wise
/// comparison of the lower-cased strings.
pub(crate) fn is_same_as_no_case(a: &str, b: &str) -> bool {
    let mut ia = a.chars();
    let mut ib = b.chars();

    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return true,
            (Some(ca), Some(cb)) => {
                if to_lower(ca) != to_lower(cb) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `wxString::Trim( true )` / `Strip( wxString::trailing )`: remove trailing white space.
pub(crate) fn trim_right(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `wxString::Trim( false )` / `Strip( wxString::leading )`: remove leading white space.
pub(crate) fn trim_left(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// `wxString::Strip( wxString::both )`.
pub(crate) fn strip_both(s: &str) -> &str {
    trim_left(trim_right(s))
}

/// Result of [`strtod`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Strtod {
    /// The converted value (0.0 if nothing could be converted).
    pub value: f64,
    /// Number of bytes of the input that were consumed (0 if nothing could be converted).
    pub consumed: usize,
    /// True when C would set `errno` to `ERANGE` (overflow or underflow).
    pub erange: bool,
}

/// C `strtod` for the "C" locale, restricted to decimal notation.
///
/// Leading white space, an optional sign, digits with an optional decimal point and an
/// optional exponent are accepted; hexadecimal floats, `inf` and `nan` are not, because no
/// caller can pass them: every call site filters its input down to digits, separators and
/// signs first.  The conversion itself is correctly rounded, like glibc's.
pub(crate) fn strtod(s: &str) -> Strtod {
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() && is_space(bytes[i] as char) {
        i += 1;
    }

    let start = i;

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    let mut digits = 0;
    let mut nonzero = false;

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        nonzero |= bytes[i] != b'0';
        digits += 1;
        i += 1;
    }

    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        let mut frac = 0;

        while j < bytes.len() && bytes[j].is_ascii_digit() {
            nonzero |= bytes[j] != b'0';
            frac += 1;
            j += 1;
        }

        // "1." and ".5" are numbers, "." is not.
        if digits + frac > 0 {
            digits += frac;
            i = j;
        }
    }

    if digits == 0 {
        return Strtod {
            value: 0.0,
            consumed: 0,
            erange: false,
        };
    }

    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;

        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }

        let exp_start = j;

        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }

        if j > exp_start {
            i = j;
        }
    }

    // The accepted text is a subset of what Rust's (correctly rounded) parser accepts.
    let value: f64 = s[start..i].parse().unwrap_or(0.0);

    // glibc reports ERANGE on overflow (HUGE_VAL is returned) and when the result underflows.
    let erange = value.is_infinite() || (nonzero && (value == 0.0 || value.is_subnormal()));

    Strtod {
        value,
        consumed: i,
        erange,
    }
}

/// C `atof`: `strtod` without any error reporting.
pub(crate) fn atof(s: &str) -> f64 {
    strtod(s).value
}

/// Port of the semantics of `wxString::ToDouble` / `ToCDouble` (the `WX_STRING_TO_X_TYPE_*`
/// macros of wxWidgets' `string.cpp`).
///
/// Returns `(parsed, complete)`: `parsed` is the value C++ stores through the output pointer
/// (`None` when it leaves the output untouched: nothing parsed, or `ERANGE`), `complete` is
/// the function's boolean result (true only if the whole string was consumed).
pub(crate) fn to_double(s: &str) -> (Option<f64>, bool) {
    let r = strtod(s);

    if r.consumed == 0 || r.erange {
        return (None, false);
    }

    (Some(r.value), r.consumed == s.len())
}

/// `printf( "%.<precision>f" )` (also `%lf`, which is the same conversion).
pub(crate) fn format_f(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }

    if value.is_infinite() {
        return if value < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    format!("{value:.precision$}")
}

/// `printf( "%.<precision>g" )`.
pub(crate) fn format_g(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }

    if value.is_infinite() {
        return if value < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    // "Let P equal the precision if nonzero, 6 if the precision is omitted, or 1 if it is
    // zero.  Then, if a conversion with style E would have an exponent of X: if P > X >= -4,
    // the conversion is with style f and precision P - (X + 1).  Otherwise, the conversion is
    // with style e and precision P - 1.  Finally, unless the # flag is used, any trailing
    // zeros are removed from the fractional portion of the result and the decimal-point
    // character is removed if there is no fractional portion remaining."  (C99 7.19.6.1)
    let p = if precision == 0 { 1 } else { precision };
    let e_style = format!("{:.*e}", p - 1, value);
    let (mantissa, exponent) = e_style.split_once('e').unwrap_or((e_style.as_str(), "0"));
    let x: i32 = exponent.parse().unwrap_or(0);

    let strip = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text.to_string()
        }
    };

    if x >= -4 && x < p as i32 {
        strip(&format!("{:.*}", (p as i32 - 1 - x) as usize, value))
    } else {
        // The exponent always contains at least two digits.
        let sign = if x < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip(mantissa), sign, x.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtod_prefixes() {
        assert_eq!(
            strtod("1.5"),
            Strtod {
                value: 1.5,
                consumed: 3,
                erange: false
            }
        );
        assert_eq!(
            strtod("1.2.3"),
            Strtod {
                value: 1.2,
                consumed: 3,
                erange: false
            }
        );
        assert_eq!(
            strtod("1."),
            Strtod {
                value: 1.0,
                consumed: 2,
                erange: false
            }
        );
        assert_eq!(
            strtod(".5"),
            Strtod {
                value: 0.5,
                consumed: 2,
                erange: false
            }
        );
        assert_eq!(
            strtod("."),
            Strtod {
                value: 0.0,
                consumed: 0,
                erange: false
            }
        );
        assert_eq!(
            strtod("+-1"),
            Strtod {
                value: 0.0,
                consumed: 0,
                erange: false
            }
        );
        assert_eq!(
            strtod("-1+2"),
            Strtod {
                value: -1.0,
                consumed: 2,
                erange: false
            }
        );
        assert_eq!(
            strtod("  7x"),
            Strtod {
                value: 7.0,
                consumed: 3,
                erange: false
            }
        );
        assert_eq!(
            strtod("1e3"),
            Strtod {
                value: 1000.0,
                consumed: 3,
                erange: false
            }
        );
        assert_eq!(
            strtod("1e"),
            Strtod {
                value: 1.0,
                consumed: 1,
                erange: false
            }
        );
        assert_eq!(
            strtod(""),
            Strtod {
                value: 0.0,
                consumed: 0,
                erange: false
            }
        );
        assert!(strtod(&"9".repeat(400)).erange);
        assert!(strtod(&format!("0.{}1", "0".repeat(400))).erange);
        assert!(!strtod("0.000").erange);
    }

    #[test]
    fn to_double_follows_wx() {
        assert_eq!(to_double("1.5"), (Some(1.5), true));
        assert_eq!(to_double("1.5x"), (Some(1.5), false));
        assert_eq!(to_double("x"), (None, false));
        assert_eq!(to_double(&"9".repeat(400)), (None, false));
    }

    #[test]
    fn printf_g() {
        // Values checked against glibc printf( "%.10g" ).
        assert_eq!(format_g(0.0, 10), "0");
        assert_eq!(format_g(-0.0, 10), "-0");
        assert_eq!(format_g(1.0, 10), "1");
        assert_eq!(format_g(25.4, 10), "25.4");
        assert_eq!(format_g(1013.05, 10), "1013.05");
        assert_eq!(format_g(39.37007874015748, 10), "39.37007874");
        assert_eq!(format_g(1e10, 10), "1e+10");
        assert_eq!(format_g(9999999999.0, 10), "9999999999");
        assert_eq!(format_g(99999999999.0, 10), "1e+11");
        assert_eq!(format_g(123456789012.0, 10), "1.23456789e+11");
        assert_eq!(format_g(0.00011, 10), "0.00011");
        assert_eq!(format_g(1.0e-5, 10), "1e-05");
        assert_eq!(format_g(1.5e-7, 10), "1.5e-07");
        assert_eq!(format_g(1.0 / 3.0, 10), "0.3333333333");
        assert_eq!(format_g(-37.0, 10), "-37");
        assert_eq!(format_g(1e100, 10), "1e+100");
        assert_eq!(format_g(f64::INFINITY, 10), "inf");
        assert_eq!(format_g(f64::NEG_INFINITY, 10), "-inf");
        assert_eq!(format_g(100.0, 0), "1e+02");
    }

    #[test]
    fn printf_f() {
        assert_eq!(format_f(1.0, 6), "1.000000");
        assert_eq!(format_f(0.25, 1), "0.2");
        assert_eq!(format_f(f64::INFINITY, 6), "inf");
        assert_eq!(format_f(f64::NAN, 6), "nan");
    }

    #[test]
    fn simple_case_mapping() {
        assert_eq!(to_lower('A'), 'a');
        assert_eq!(to_lower('\u{130}'), 'i');
        assert_eq!(to_lower('\u{3A9}'), '\u{3C9}');
        assert_eq!(to_upper('\u{DF}'), '\u{DF}');
        assert_eq!(to_upper('\u{1F80}'), '\u{1F88}');
        assert_eq!(to_upper('\u{B5}'), '\u{39C}');
        assert!(is_same_as_no_case("HV_Line", "hv_line"));
        assert!(!is_same_as_no_case("HV_Line", "hv_lines"));
    }
}
