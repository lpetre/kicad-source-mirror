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

//! The C runtime / wxString primitives the ported code relies on: `strtod`/`atof`/
//! `wxString::ToDouble`, the `printf` conversions `%f` and `%g`, the `ctype` classification
//! functions and the per-character case mapping of `wxString::CmpNoCase` / `MakeUpper`.
//!
//! They are the shared ports of `kicad-core` (`kicad_core::wxstr`, `kicad_core::printf`);
//! only the two `ctype` classifications that `kicad-core` does not need are defined here.
//! The C++ code calls these with the process locale in effect.  This port has no locale
//! (see `rust/PORTING.md` section 5): the decimal separator is always `.`, `isalpha` and
//! friends classify ASCII only (the "C" locale; for values above 255 the C++ calls are
//! undefined behaviour anyway) and case mapping is the Unicode simple case mapping (what a
//! UTF-8 locale gives `towlower`/`towupper`).

pub(crate) use kicad_core::printf::format_f;
pub(crate) use kicad_core::wxstr::{LOCALE_DECIMAL_POINT, is_digit, is_same_as_no_case, strtod};

/// C `isalpha` in the "C" locale.
pub(crate) fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// C `isalnum` in the "C" locale.
pub(crate) fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// C `atof`: `strtod` without any error reporting.
pub(crate) fn atof(s: &str) -> f64 {
    strtod(s).value
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_core::printf::format_g;
    use kicad_core::wxstr::{to_double, to_lower, to_upper};

    #[test]
    fn to_double_follows_wx() {
        assert_eq!(to_double("1.5"), (Some(1.5), true));
        assert_eq!(to_double("1.5x"), (Some(1.5), false));
        assert_eq!(to_double("x"), (None, false));
        assert_eq!(to_double(&"9".repeat(400)), (None, false));
        assert_eq!(atof("-1+2"), -1.0);
        assert_eq!(atof("."), 0.0);
    }

    #[test]
    fn printf_g() {
        // Values checked against glibc printf( "%.10g" ).
        assert_eq!(format_g(0.0, 10), "0");
        assert_eq!(format_g(-0.0, 10), "-0");
        assert_eq!(format_g(25.4, 10), "25.4");
        assert_eq!(format_g(39.37007874015748, 10), "39.37007874");
        assert_eq!(format_g(1e10, 10), "1e+10");
        assert_eq!(format_g(123456789012.0, 10), "1.23456789e+11");
        assert_eq!(format_g(1.0e-5, 10), "1e-05");
        assert_eq!(format_g(f64::NEG_INFINITY, 10), "-inf");
        assert_eq!(format_g(100.0, 0), "1e+02");
        assert_eq!(format_f(0.25, 1), "0.2");
        assert_eq!(format_f(f64::NAN, 6), "nan");
    }

    #[test]
    fn simple_case_mapping() {
        assert_eq!(to_lower('\u{130}'), 'i');
        assert_eq!(to_upper('\u{1F80}'), '\u{1F88}');
        assert!(is_same_as_no_case("HV_Line", "hv_line"));
        assert!(!is_same_as_no_case("HV_Line", "hv_lines"));
        // wxString::CmpNoCase compares past embedded NULs
        assert!(!is_same_as_no_case("a\0b", "a\0c"));
        assert!(!is_same_as_no_case("a", "a\0"));
    }
}
