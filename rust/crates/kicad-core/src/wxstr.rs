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

//! Helpers reproducing the parts of `wxString`, the C library and `KiROUND` that the
//! modules ported from `common/eda_units.cpp`, `common/string_utils.cpp` and
//! `common/gal/color4d.cpp` rely on.  The `wxString` / C library ones are public so that the
//! other foundation crates (`kicad-libeval`) share them instead of keeping copies; the
//! `KiROUND` instantiations stay private (the public port is `kicad_geom::math::util`).
//!
//! A `wxString` on the reference platform (Linux, see `PORTING.md` section 9) is a sequence
//! of 32 bit `wchar_t`, i.e. of Unicode code points, so every index and length of the C++ code
//! is a count of `char`s here.  Locale dependent C functions are ported for a UTF-8 locale
//! whose `LC_NUMERIC` is "C": the decimal separator is `.` and `towupper` / `towlower` are the
//! Unicode simple case mappings.

/// `localeconv()->decimal_point[0]`: always the "C" locale's.
pub const LOCALE_DECIMAL_POINT: char = '.';

/// `wxSafeIsspace` as used by `wxString::Trim` / `Strip`: the ASCII white space characters.
pub fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// `wxIsdigit` (`iswdigit`): true for `0` .. `9` only, in every locale.
pub fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// `towlower` (`wxTolower`, `wxString::Lower`, `CmpNoCase`): the Unicode *simple* lower-case
/// mapping (one character in, one character out).
pub fn to_lower(c: char) -> char {
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

/// `towupper` (`wxToupper`, `wxString::MakeUpper`): the Unicode *simple* upper-case mapping.
pub fn to_upper(c: char) -> char {
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

/// `wxString::Lower()`.
pub fn lower(s: &str) -> String {
    s.chars().map(to_lower).collect()
}

/// `wxString::Upper()` / `MakeUpper()`.
pub fn upper(s: &str) -> String {
    s.chars().map(to_upper).collect()
}

/// `wxString::Capitalize()`: first character upper-cased, the others lower-cased.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = String::with_capacity(s.len());

    if let Some(first) = chars.next() {
        out.push(to_upper(first));
        out.extend(chars.map(to_lower));
    }

    out
}

/// Port of `wxString::CmpNoCase` (wxWidgets 3.2, `src/common/string.cpp`, the
/// `!wxUSE_UNICODE_UTF8` branch of the reference platform): `wxStricmp` (`wcscasecmp`: the
/// first difference of the lower-cased characters) on the NUL-delimited chunks of both
/// strings, embedded NULs compared explicitly (a NUL sorts before anything else, a string
/// that ends first is smaller); the result is normalised to -1, 0 or 1.
pub fn cmp_no_case(a: &str, b: &str) -> i32 {
    let a: Vec<u32> = a.chars().map(|c| to_lower(c) as u32).collect();
    let b: Vec<u32> = b.chars().map(|c| to_lower(c) as u32).collect();

    // `c_str()`: the terminating NUL is at `len()`
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);

    let mut this_cur = 0usize;
    let mut that_cur = 0usize;
    let mut rc: i32;

    loop {
        // Compare until the next NUL, if the strings differ this is the final result.
        // (wxStricmp( thisCur, thatCur ))
        let mut k = 0;

        rc = loop {
            let c1 = at(&a, this_cur + k);
            let c2 = at(&b, that_cur + k);

            if c1 == 0 || c1 != c2 {
                break c1 as i32 - c2 as i32;
            }

            k += 1;
        };

        if rc != 0 {
            break;
        }

        let len_chunk = a[this_cur..].iter().take_while(|&&c| c != 0).count();
        this_cur += len_chunk;
        that_cur += len_chunk;

        // Skip all the NULs as wxStricmp() doesn't handle them.
        while at(&a, this_cur) == 0 {
            // Check if we exhausted either of the strings.
            if this_cur == a.len() {
                // This one is exhausted, is the other one too?
                return if that_cur == b.len() { 0 } else { -1 };
            }

            if that_cur == b.len() {
                // Because of the check above we know that this one is not
                // exhausted yet so it's "greater".
                return 1;
            }

            if at(&b, that_cur) != 0 {
                // Anything non-NUL is greater than NUL.
                rc = -1;
                break;
            }

            this_cur += 1;
            that_cur += 1;
        }

        if rc != 0 {
            break;
        }
    }

    rc.signum()
}

/// `wxString::IsSameAs( other, false )`.
pub fn is_same_as_no_case(a: &str, b: &str) -> bool {
    cmp_no_case(a, b) == 0
}

/// `wxString::Trim( true )` / `Strip( wxString::trailing )`: remove trailing white space.
pub fn trim_right(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `wxString::Trim( false )` / `Strip( wxString::leading )`: remove leading white space.
pub fn trim_left(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// `wxString::Strip( wxString::both )` / `Trim( true ).Trim( false )`.
pub fn strip_both(s: &str) -> &str {
    trim_left(trim_right(s))
}

/// `wxString::Left( count )` in characters.
pub fn left(s: &str, count: usize) -> &str {
    match s.char_indices().nth(count) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// `wxString::Mid( start )` in characters.
pub fn mid(s: &str, start: usize) -> &str {
    match s.char_indices().nth(start) {
        Some((idx, _)) => &s[idx..],
        None => "",
    }
}

/// Result of [`strtod`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strtod {
    /// The converted value (0.0 if nothing could be converted).
    pub value: f64,
    /// Number of bytes of the input that were consumed (0 if nothing could be converted).
    pub consumed: usize,
    /// True when C would set `errno` to `ERANGE` (overflow or underflow).
    pub erange: bool,
}

/// C `strtod` / `wcstod` for the "C" locale, restricted to decimal notation with an optional
/// exponent.
///
/// Leading white space, an optional sign, digits with an optional decimal point and an
/// optional exponent are accepted, as are `inf`, `infinity`, `nan` (case-insensitive) and
/// hexadecimal floats (`0x1.8p3`; their mantissa is accumulated in a double, which is exact
/// up to 13 hex digits).  The decimal conversion is correctly rounded, like glibc's.
pub fn strtod(s: &str) -> Strtod {
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() && is_space(bytes[i] as char) {
        i += 1;
    }

    let start = i;
    let negative = i < bytes.len() && bytes[i] == b'-';

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    if let Some(r) = strtod_special(bytes, i, negative) {
        return r;
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

/// The `inf` / `nan` / hexadecimal forms of `strtod`, after the sign at `i`.
fn strtod_special(bytes: &[u8], i: usize, negative: bool) -> Option<Strtod> {
    let rest = &bytes[i..];
    let starts_ci = |w: &[u8]| rest.len() >= w.len() && rest[..w.len()].eq_ignore_ascii_case(w);
    let sign = if negative { -1.0 } else { 1.0 };

    if starts_ci(b"infinity") || starts_ci(b"inf") {
        let n = if starts_ci(b"infinity") { 8 } else { 3 };
        return Some(Strtod {
            value: sign * f64::INFINITY,
            consumed: i + n,
            erange: false,
        });
    }

    if starts_ci(b"nan") {
        let mut n = 3;

        // nan(n-char-sequence)
        if rest.get(3) == Some(&b'(') {
            let close = rest[4..]
                .iter()
                .position(|&c| !(c.is_ascii_alphanumeric() || c == b'_'));

            if let Some(k) = close {
                if rest[4 + k] == b')' {
                    n = 4 + k + 1;
                }
            }
        }

        let nan = if negative { -f64::NAN } else { f64::NAN };
        return Some(Strtod {
            value: nan,
            consumed: i + n,
            erange: false,
        });
    }

    let is_hex = rest.len() >= 3
        && rest[0] == b'0'
        && (rest[1] == b'x' || rest[1] == b'X')
        && (rest[2].is_ascii_hexdigit()
            || (rest[2] == b'.' && rest.get(3).is_some_and(|c| c.is_ascii_hexdigit())));

    if !is_hex {
        return None;
    }

    let mut j = 2;
    let mut mantissa = 0.0f64;
    let mut exp: i32 = 0;

    while j < rest.len() && rest[j].is_ascii_hexdigit() {
        mantissa = mantissa * 16.0 + (rest[j] as char).to_digit(16).unwrap_or(0) as f64;
        j += 1;
    }

    if j < rest.len() && rest[j] == b'.' {
        j += 1;

        while j < rest.len() && rest[j].is_ascii_hexdigit() {
            mantissa = mantissa * 16.0 + (rest[j] as char).to_digit(16).unwrap_or(0) as f64;
            exp -= 4;
            j += 1;
        }
    }

    if j < rest.len() && (rest[j] == b'p' || rest[j] == b'P') {
        let mut k = j + 1;
        let mut eneg = false;

        if k < rest.len() && (rest[k] == b'+' || rest[k] == b'-') {
            eneg = rest[k] == b'-';
            k += 1;
        }

        if k < rest.len() && rest[k].is_ascii_digit() {
            let mut e: i32 = 0;

            while k < rest.len() && rest[k].is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add((rest[k] - b'0') as i32);
                k += 1;
            }

            exp = exp.saturating_add(if eneg { -e } else { e });
            j = k;
        }
    }

    let value = sign * mantissa * 2f64.powi(exp);
    let erange = value.is_infinite() || (mantissa != 0.0 && (value == 0.0 || value.is_subnormal()));

    Some(Strtod {
        value,
        consumed: i + j,
        erange,
    })
}

/// Port of the semantics of `wxString::ToDouble` / `ToCDouble` (the `WX_STRING_TO_X_TYPE_*`
/// macros of wxWidgets' `string.cpp`).
///
/// Returns `(parsed, complete)`: `parsed` is the value C++ stores through the output pointer
/// (`None` when it leaves the output untouched: nothing parsed, or `ERANGE`), `complete` is
/// the function's boolean result (true only if the whole string was consumed).
pub fn to_double(s: &str) -> (Option<f64>, bool) {
    let r = strtod(s);

    if r.consumed == 0 || r.erange {
        return (None, false);
    }

    (Some(r.value), r.consumed == s.len())
}

/// Port of `KiROUND<double, int>( v )` (`libs/kimath/include/math/util.h`): round half away
/// from zero and clamp to the limits of `int`; NaN gives 0.
///
/// The public port of `KiROUND` lives in `kicad_geom::math::util`; this crate must not depend
/// on the geometry crate, so the two instantiations it needs are repeated here.
pub(crate) fn ki_round_i32(v: f64) -> i32 {
    if v.is_nan() {
        return 0;
    }

    if v >= i32::MAX as f64 + 0.5 {
        return i32::MAX;
    }

    // lowest - 0.5 can round to lowest itself, which llround still handles exactly
    if v - (i32::MIN as f64) <= -0.5 {
        return i32::MIN;
    }

    let rounded = v.round() as i64;

    rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Port of `KiROUND<double, long long int>( v )`.
pub(crate) fn ki_round_i64(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }

    if v >= i64::MAX as f64 + 0.5 {
        return i64::MAX;
    }

    if v - (i64::MIN as f64) <= -0.5 {
        return i64::MIN;
    }

    // `as` saturates, like the clamp of the C++ code.
    v.round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtod_semantics() {
        assert_eq!(strtod("1.5").value, 1.5);
        assert_eq!(strtod("1.2.3").consumed, 3);
        assert_eq!(strtod("-").consumed, 0);
        assert_eq!(strtod("+-1").consumed, 0);
        assert_eq!(strtod(".").consumed, 0);
        assert_eq!(strtod(".5").value, 0.5);
        assert_eq!(strtod("5.").consumed, 2);
        assert_eq!(strtod("1e5").value, 1e5);
        assert_eq!(strtod("1e").consumed, 1);
        assert!(strtod("1e999").erange);
        assert!(strtod("1e-999").erange);
        assert_eq!(to_double("12abc"), (Some(12.0), false));
        assert_eq!(to_double("abc"), (None, false));
        assert_eq!(to_double("7"), (Some(7.0), true));
    }

    #[test]
    fn ki_round() {
        assert_eq!(ki_round_i32(0.5), 1);
        assert_eq!(ki_round_i32(-0.5), -1);
        assert_eq!(ki_round_i32(2.4), 2);
        assert_eq!(ki_round_i32(f64::NAN), 0);
        assert_eq!(ki_round_i32(1e20), i32::MAX);
        assert_eq!(ki_round_i32(-1e20), i32::MIN);
        assert_eq!(ki_round_i32(-2147483648.4), i32::MIN);
        assert_eq!(ki_round_i64(1e30), i64::MAX);
        assert_eq!(ki_round_i64(-1e30), i64::MIN);
        assert_eq!(ki_round_i64(-2.5), -3);
    }

    #[test]
    fn case_mapping() {
        assert_eq!(capitalize("hELLO"), "Hello");
        assert_eq!(upper("stra\u{df}e"), "STRA\u{df}E");
        assert_eq!(cmp_no_case("abc", "ABC"), 0);
        assert!(cmp_no_case("abc", "abd") < 0);
        assert!(cmp_no_case("b", "") > 0);
        assert_eq!(left("h\u{e9}llo", 2), "h\u{e9}");
        assert_eq!(mid("h\u{e9}llo", 2), "llo");
        assert_eq!(mid("ab", 5), "");
    }
}
