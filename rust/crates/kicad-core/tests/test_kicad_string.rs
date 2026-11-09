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

//! Port of `qa/tests/common/test_kicad_string.cpp`.

use kicad_core::string_utils::*;

#[test]
fn trailing_int() {
    let cases = [
        ("", 0),
        ("foo", 0),      // no int
        ("0", 0),        // only int
        ("42", 42),      // only int
        ("1001", 1001),  // only int
        ("Foo42", 42),   // only the trailing
        ("12Foo42", 42), // only the trailing
        ("12Foo4.2", 2), // no dots
    ];

    for (s, n) in cases {
        assert_eq!(get_trailing_int(s), n);
    }
}

#[test]
fn natural_number_compare() {
    let cases: &[(&str, &str, i32, i32)] = &[
        ("a", "b", -1, -1),
        ("b", "a", 1, 1),
        ("a", "a", 0, 0),
        ("a", "A", 1, 0),
        ("A", "a", -1, 0),
        ("a", "", 1, 1),
        ("", "a", -1, -1),
        ("1", "", 1, 1),
        ("", "1", -1, -1),
        ("10", "2", 1, 1),
        ("2", "10", -1, -1),
        ("2", "2", 0, 0),
        ("10", "10", 0, 0),
        ("01", "1", 0, 0),
        ("01a", "1a", 0, 0),
        ("01a", "1b", -1, -1),
        ("01b", "1a", 1, 1),
        ("10 ten", "2 two", 1, 1),
        ("SYM1", "sym2", -1, -1),
        ("sym2", "SYM1", 1, 1),
        ("a10b20c30", "a10b20c31", -1, -1),
        ("a10b20c31", "a10b20c30", 1, 1),
        ("10UF", "10UF", 0, 0),
        ("10uF", "10uF", 0, 0),
        ("u10", "u10", 0, 0),
        ("U10", "U10", 0, 0),
        ("u10", "U10", 1, 0),
        ("U10", "u10", -1, 0),
        ("U10.1", "U10.10", -1, -1),
        ("U10-1", "U10-10", -1, -1),
        ("U10,1", "U10,10", -1, -1),
        ("U10.A", "U10.a", -1, 0),
        ("U10-A", "U10-a", -1, 0),
        ("U10,A", "U10,a", -1, 0),
    ];

    for &(a, b, cs, ci) in cases {
        assert_eq!(
            str_num_cmp(a, b, CmpT::CaseSensitive),
            cs,
            "{a} AND {b} failed for case sensitive"
        );
        assert_eq!(
            str_num_cmp(a, b, CmpT::IgnoreCase),
            ci,
            "{a} AND {b} failed for case insensitive"
        );
    }
}

#[test]
fn value_compare() {
    let cases = [
        ("100", "10", 1),
        ("10K", "1K", 1),
        ("10K", "1K5", 1),
        ("10K", "10,000", 0),
        ("1K5", "1.5K", 0),
        ("1K5", "1,5K", 0),
        ("K5", "1K", -1),
        ("1K5", "K55", 1),
        ("1R5", "1.5", 0),
        // Capacitor values using an SI prefix as the decimal separator.
        ("1u5", "1.5u", 0),
        ("1u5F", "1.5uF", 0),
        ("2u2F", "2.2uF", 0),
        ("1n5F", "1.5nF", 0),
        ("12n1F", "12.1nF", 0),
        ("5p6F", "5.6pF", 0),
        ("1\u{b5}5", "1u5", 0),
    ];

    for (a, b, expected) in cases {
        assert_eq!(value_string_compare(a, b), expected, "{a} AND {b} failed");
    }
}

#[test]
fn value_compare_alphanumeric_identifiers() {
    // Textual identifiers must not be parsed as numbers just because they contain SI/RKM
    // letters.  They sort after pure numbers and use normal lexical ordering relative to
    // other identifiers.
    assert!(value_string_compare("T9G", "1") > 0);
    assert!(value_string_compare("1182T117P", "9999") > 0);
    assert!(value_string_compare("G4W", "F1") > 0);
}

#[test]
fn double2_str() {
    // conceptually a little quirky because doubles do have all those pesky additional values
    let cases = [
        (0.0, "0"),
        (1.000, "1"),
        (1.050, "1.05"),               // single trailing zero
        (0.00001523, "0.00001523"),    // value less than the magic 0.0001 threshold
        (0.00000000000000001523, "0"), // really small decimal that gets cut off
        (623523.0, "623523"),          // large whole number
    ];

    for (v, s) in cases {
        // Test both of these functions that work the same but the innards are different
        assert_eq!(format_double2_str(v), s);
        assert_eq!(ui_double2_str(v), s);
    }
}

#[test]
fn html_escape() {
    let cases = [
        (
            "I will display \u{20ac} \u{20ac}",
            "I will display &#8364; &#x20AC;",
        ),
        ("&lt;", "&amp;lt;"),
        ("Don't \u{3a9}", "Don&apos;t \u{3a9}"),
    ];

    for (original, escaped_test) in cases {
        let escaped = escape_html(original);
        assert_eq!(unescape_html(&escaped), original);
        assert_eq!(unescape_html(escaped_test), original);
    }
}

#[test]
fn variant_name_sort() {
    // Verify default variant name is always sorted to the beginning of the list.
    let mut names = ["Variant1".to_string(), get_default_variant_name()];
    names.sort_by(|a, b| sort_variant_names(a, b).cmp(&0));
    assert_eq!(names[0], get_default_variant_name());
    assert_eq!(names[1], "Variant1");
}

#[test]
fn format_trimmed_decimal_keeps_integer_digits() {
    // Precision 0 has no decimal point and must not lose integer zeros
    assert_eq!(format_trimmed_decimal(100.0, 0), "100");
    assert_eq!(format_trimmed_decimal(1000.0, 0), "1000");
    // Byte-identical with the old exporters for precision >= 1
    assert_eq!(format_trimmed_decimal(1.5, 6), "1.50");
    assert_eq!(format_trimmed_decimal(1.0, 6), "1.0");
    assert_eq!(format_trimmed_decimal(100.0, 4), "100.0");
    assert_eq!(format_trimmed_decimal(0.123456, 6), "0.123456");
    assert_eq!(format_trimmed_decimal(-0.0001, 3), "0.0");
    assert_eq!(format_trimmed_decimal(-0.3, 0), "0");
}

/// Verbatim old ODB and IPC-2581 trim loop, compared only at precision >= 1 away from
/// negative zero
fn old_trim_loop(val: f64, precision: i32) -> String {
    let mut s = wx_from_c_double(val, precision);
    while s.ends_with("00") {
        s.pop();
    }
    s
}

#[test]
fn format_trimmed_decimal_matches_old_trim_loop() {
    for ii in -300..=300 {
        let value = ii as f64 * 0.137;
        for precision in 1..=8 {
            assert_eq!(
                format_trimmed_decimal(value, precision),
                old_trim_loop(value, precision)
            );
        }
    }

    // oldTrimLoop() reproduces the precision-0 bug this formatter fixes, so precision 0 is
    // checked against literal values here rather than against oldTrimLoop()
    assert_eq!(format_trimmed_decimal(100.0, 0), "100");
    assert_eq!(format_trimmed_decimal(1000.0, 0), "1000");
    assert_eq!(format_trimmed_decimal(2500.0, 0), "2500");
}
