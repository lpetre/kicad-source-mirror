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

//! Number parsing with the exact accepted syntax of the C/C++ library functions the KiCad
//! s-expression code uses.
//!
//! - [`from_chars_double`] reproduces `fast_float::from_chars` (the bundled
//!   `thirdparty/fast_float`, version 8.2) as called by `DSNLEXER::parseDouble` with
//!   `chars_format::skip_white_space | chars_format::general`.
//! - [`strtol`] / [`strtoll`] reproduce the C library functions (LP64: `long` is 64 bit) as
//!   used by the s-expression parsers (`strtol( CurText(), nullptr, 10 )`) and by
//!   `SEXPR::PARSER` (`strtoll( tmp.c_str(), nullptr, 0 )`).
//! - [`strtod_prefix`] reproduces what `strtod` consumes and returns for plain decimal input.
//!
//! The decimal to binary conversion itself is done by Rust's `str::parse::<f64>`, which is
//! correctly rounded (round to nearest, ties to even) exactly like `fast_float` and glibc's
//! `strtod`, so only the accepted syntax and the error cases need porting.

/// Why [`from_chars_double`] failed; the two error codes `fast_float::from_chars` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromCharsError {
    /// `std::errc::invalid_argument`: the text does not start with a number.
    InvalidArgument,
    /// `std::errc::result_out_of_range`: the number overflows to infinity or underflows to zero.
    ResultOutOfRange,
}

/// Port of `fast_float::is_space` (`space_lut`): `'\t'`, `'\n'`, `'\v'`, `'\f'`, `'\r'` and
/// `' '`.  This is also the set of the C library's `isspace` in the "C" locale.
pub fn is_c_space(c: u8) -> bool {
    matches!(c, 9..=13 | b' ')
}

/// Compare the first `pattern.len()` bytes of `text` with the lower case ASCII `pattern`,
/// ignoring ASCII case (port of `fastfloat_strncasecmp`).  The caller guarantees the length.
fn starts_with_ignore_case(text: &[u8], pattern: &[u8]) -> bool {
    text.len() >= pattern.len()
        && text.iter().zip(pattern).all(|(t, p)| t.to_ascii_lowercase() == *p)
}

/// Port of `fast_float::detail::parse_infnan` (without `allow_leading_plus`): the special
/// cases `inf`, `-inf`, `nan`, `infinity`, `-infinity` and `nan(n-char-seq-opt)`, in any
/// letter case.
fn parse_infnan(first: &[u8]) -> Result<(f64, usize), FromCharsError> {
    // assume first < last, so dereference without checks;
    let minus_sign = first[0] == b'-';
    let mut pos = 0;

    // C++17 20.19.3.(7.1) explicitly forbids '+' sign here
    if minus_sign {
        pos += 1;
    }

    let rest = &first[pos..];

    if rest.len() >= 3 {
        if starts_with_ignore_case(rest, b"nan") {
            pos += 3;
            let mut end = pos;
            let value = if minus_sign { -f64::NAN } else { f64::NAN };

            // Check for possible nan(n-char-seq-opt), C++17 20.19.3.7,
            // C11 7.20.1.3.3. At least MSVC produces nan(ind) and nan(snan).
            if pos != first.len() && first[pos] == b'(' {
                for (i, &c) in first.iter().enumerate().skip(pos + 1) {
                    if c == b')' {
                        end = i + 1; // valid nan(n-char-seq-opt)
                        break;
                    } else if !(c.is_ascii_alphanumeric() || c == b'_') {
                        break; // forbidden char, not nan(n-char-seq-opt)
                    }
                }
            }

            return Ok((value, end));
        }

        if starts_with_ignore_case(rest, b"inf") {
            let end = if rest.len() >= 8 && starts_with_ignore_case(&rest[3..], b"inity") {
                pos + 8
            } else {
                pos + 3
            };

            let value = if minus_sign { f64::NEG_INFINITY } else { f64::INFINITY };
            return Ok((value, end));
        }
    }

    Err(FromCharsError::InvalidArgument)
}

/// Port of the accepted syntax and error reporting of
/// `fast_float::from_chars( first, last, value, chars_format::skip_white_space |
/// chars_format::general )` for `double`.
///
/// On success returns the value and the number of bytes consumed (C++ `result.ptr - first`),
/// which may be less than `text.len()`: like the C++ function this parses a *prefix*.
///
/// The grammar is: optional white space, an optional `'-'` (a leading `'+'` is not accepted),
/// decimal digits with an optional `'.'` (at least one digit before or after it), and an
/// optional exponent `e`/`E`, optional sign, digits.  An `e` that is not followed by a valid
/// exponent is left unconsumed.  If that does not match, `nan`, `inf` and `infinity` (any
/// case, optionally negative) are accepted.
///
/// # Errors
///
/// [`FromCharsError::InvalidArgument`] if no number is found;
/// [`FromCharsError::ResultOutOfRange`] if the value overflows to infinity, or is non-zero and
/// underflows to zero.
pub fn from_chars_double(text: &[u8]) -> Result<(f64, usize), FromCharsError> {
    let mut first = 0;

    // skip_white_space
    while first != text.len() && is_c_space(text[first]) {
        first += 1;
    }

    if first == text.len() {
        return Err(FromCharsError::InvalidArgument);
    }

    // parse_number_string(); any failure in there falls through to parse_infnan().
    let number = (|| {
        let mut p = first;

        // C++17 20.19.3.(7.1) explicitly forbids '+' sign here
        if text[p] == b'-' {
            p += 1;

            if p == text.len() {
                return None; // missing_integer_or_dot_after_sign
            }

            // a sign must be followed by an integer or the dot
            if !text[p].is_ascii_digit() && text[p] != b'.' {
                return None; // missing_integer_or_dot_after_sign
            }
        }

        let start_digits = p;
        let mut nonzero_mantissa = false;

        while p != text.len() && text[p].is_ascii_digit() {
            nonzero_mantissa |= text[p] != b'0';
            p += 1;
        }

        let mut digit_count = p - start_digits;

        if p != text.len() && text[p] == b'.' {
            p += 1;
            let before = p;

            while p != text.len() && text[p].is_ascii_digit() {
                nonzero_mantissa |= text[p] != b'0';
                p += 1;
            }

            digit_count += p - before;
        }

        if digit_count == 0 {
            return None; // no_digits_in_mantissa: we must have encountered at least one integer!
        }

        if p != text.len() && (text[p] == b'e' || text[p] == b'E') {
            let location_of_e = p;
            p += 1;

            if p != text.len() && (text[p] == b'-' || text[p] == b'+') {
                // '+' on exponent is allowed by C++17 20.19.3.(7.1)
                p += 1;
            }

            if p == text.len() || !text[p].is_ascii_digit() {
                // The format includes chars_format::fixed, so the exponential part is not
                // an error: we will be ignoring the 'e'.
                p = location_of_e;
            } else {
                while p != text.len() && text[p].is_ascii_digit() {
                    p += 1;
                }
            }
        }

        Some((p, nonzero_mantissa))
    })();

    let Some((lastmatch, nonzero_mantissa)) = number else {
        return parse_infnan(&text[first..]).map(|(value, used)| (value, first + used));
    };

    // The matched text is plain ASCII and a syntax Rust's parser accepts as well (it is a
    // superset of the above for everything but the leading white space).
    let value = std::str::from_utf8(&text[first..lastmatch])
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or(FromCharsError::InvalidArgument)?;

    // Test for over/underflow:
    // (pns.mantissa != 0 && am.mantissa == 0 && am.power2 == 0) || am.power2 == infinite_power
    if value.is_infinite() || (nonzero_mantissa && value == 0.0) {
        return Err(FromCharsError::ResultOutOfRange);
    }

    Ok((value, lastmatch))
}

/// Value of an ASCII digit or letter in bases up to 36, as `strtol` sees it.
fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

/// Port of the C library's `strtoll( nptr, &endptr, base )` for `base` 0 or 2..=36.
///
/// `text` is treated as a C string: parsing stops at the first NUL byte.  Returns the value
/// and the number of bytes consumed (`endptr - nptr`; 0 if no conversion was performed, in
/// which case the value is 0).  Out of range values are clamped to `i64::MIN`/`i64::MAX`, as
/// the C function does (it also sets `errno` to `ERANGE`, which no KiCad caller inspects).
///
/// The syntax is: optional white space (`isspace`), an optional `'+'` or `'-'`, an optional
/// `0x`/`0X` prefix if the base is 16 or 0, then digits.  With base 0 the base is 16 after a
/// `0x` prefix, 8 after a leading `0`, and 10 otherwise.
pub fn strtoll(text: &[u8], base: u32) -> (i64, usize) {
    // C string semantics: a NUL terminates the text.
    let text = match text.iter().position(|&b| b == 0) {
        Some(nul) => &text[..nul],
        None => text,
    };

    if base == 1 || base > 36 {
        // EINVAL
        return (0, 0);
    }

    let mut p = 0;

    while p < text.len() && is_c_space(text[p]) {
        p += 1;
    }

    let mut negative = false;

    if p < text.len() && (text[p] == b'-' || text[p] == b'+') {
        negative = text[p] == b'-';
        p += 1;
    }

    let mut base = base;

    // A "0x" prefix only counts when a hexadecimal digit follows it; otherwise the "0" alone
    // is the number.
    let has_hex_prefix = p + 2 < text.len()
        && text[p] == b'0'
        && (text[p + 1] == b'x' || text[p + 1] == b'X')
        && digit_value(text[p + 2]).is_some_and(|d| d < 16);

    if base == 0 {
        if has_hex_prefix {
            base = 16;
            p += 2;
        } else if p < text.len() && text[p] == b'0' {
            base = 8;
        } else {
            base = 10;
        }
    } else if base == 16 && has_hex_prefix {
        p += 2;
    }

    let digits_start = p;

    // Accumulate the magnitude; the negative range is one larger than the positive one.
    let limit: u64 = if negative { 1u64 << 63 } else { i64::MAX as u64 };
    let mut value: u64 = 0;
    let mut overflow = false;

    while p < text.len() {
        let Some(digit) = digit_value(text[p]).filter(|&d| d < base) else {
            break;
        };

        if !overflow {
            match value.checked_mul(u64::from(base)).and_then(|v| v.checked_add(u64::from(digit)))
            {
                Some(v) if v <= limit => value = v,
                _ => overflow = true,
            }
        }

        p += 1;
    }

    if p == digits_start {
        // No digits: no conversion is performed.
        return (0, 0);
    }

    let result = if overflow {
        if negative { i64::MIN } else { i64::MAX }
    } else if negative {
        // value <= 2^63, so the wrapping negation is exact (and yields i64::MIN for 2^63).
        (value as i64).wrapping_neg()
    } else {
        value as i64
    };

    (result, p)
}

/// Port of the C library's `strtol( nptr, &endptr, base )` on an LP64 platform (Linux, macOS),
/// where `long` is 64 bit wide and the function is therefore identical to [`strtoll`].
///
/// Note that on LLP64 platforms (64 bit Windows) `long` is 32 bit and the C function clamps to
/// the 32 bit range there; the port follows the Linux behaviour.
pub fn strtol(text: &[u8], base: u32) -> (i64, usize) {
    strtoll(text, base)
}

/// Port of what the C library's `strtod( nptr, &endptr )` does with *plain decimal* input:
/// optional white space, an optional sign, digits with an optional `'.'` (at least one digit),
/// and an optional exponent.  Hexadecimal floats, `inf` and `nan` are not handled (the callers
/// only pass text made of digits, dots and a leading minus).
///
/// Returns `None` if no conversion can be performed (C++: `endptr == nptr`), otherwise the
/// value and the number of bytes consumed.  Overflow yields an infinity and underflow a
/// denormal or zero, as in C (which additionally sets `errno` to `ERANGE`).
pub fn strtod_prefix(text: &[u8]) -> Option<(f64, usize)> {
    let mut first = 0;

    while first < text.len() && is_c_space(text[first]) {
        first += 1;
    }

    let mut p = first;

    if p < text.len() && (text[p] == b'-' || text[p] == b'+') {
        p += 1;
    }

    let start_digits = p;

    while p < text.len() && text[p].is_ascii_digit() {
        p += 1;
    }

    let mut digit_count = p - start_digits;

    if p < text.len() && text[p] == b'.' {
        let after_dot = p + 1;
        let mut q = after_dot;

        while q < text.len() && text[q].is_ascii_digit() {
            q += 1;
        }

        digit_count += q - after_dot;

        // A lone "." (no digit on either side) is not consumed.
        if digit_count != 0 {
            p = q;
        }
    }

    if digit_count == 0 {
        return None;
    }

    if p < text.len() && (text[p] == b'e' || text[p] == b'E') {
        let mut q = p + 1;

        if q < text.len() && (text[q] == b'-' || text[q] == b'+') {
            q += 1;
        }

        if q < text.len() && text[q].is_ascii_digit() {
            while q < text.len() && text[q].is_ascii_digit() {
                q += 1;
            }

            p = q;
        }
    }

    let value = std::str::from_utf8(&text[first..p]).ok()?.parse::<f64>().ok()?;

    Some((value, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> (f64, usize) {
        from_chars_double(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
    }

    fn err(text: &str) -> FromCharsError {
        from_chars_double(text.as_bytes()).expect_err(text)
    }

    #[test]
    fn from_chars_plain_numbers() {
        assert_eq!(ok("0"), (0.0, 1));
        assert_eq!(ok("1"), (1.0, 1));
        assert_eq!(ok("-1"), (-1.0, 2));
        assert_eq!(ok("3.14"), (3.14, 4));
        assert_eq!(ok("-3.14"), (-3.14, 5));
        assert_eq!(ok("5."), (5.0, 2));
        assert_eq!(ok(".5"), (0.5, 2));
        assert_eq!(ok("-.5"), (-0.5, 3));
        assert_eq!(ok("-5."), (-5.0, 3));
        assert_eq!(ok("1e3"), (1000.0, 3));
        assert_eq!(ok("1E3"), (1000.0, 3));
        assert_eq!(ok("1.e3"), (1000.0, 4));
        assert_eq!(ok(".1e1"), (1.0, 4));
        assert_eq!(ok("1e+3"), (1000.0, 4));
        assert_eq!(ok("1.5e-3"), (0.0015, 6));
        assert_eq!(ok("0.1"), (0.1, 3));
        assert_eq!(ok("127.5"), (127.5, 5));
        assert_eq!(ok("0001.5000"), (1.5, 9));
    }

    #[test]
    fn from_chars_negative_zero_keeps_its_sign() {
        let (v, used) = ok("-0");
        assert_eq!(used, 2);
        assert!(v == 0.0 && v.is_sign_negative());

        let (v, _) = ok("-0.000");
        assert!(v == 0.0 && v.is_sign_negative());
    }

    #[test]
    fn from_chars_skips_leading_white_space() {
        assert_eq!(ok("  \t\n\r\x0b\x0c1.5"), (1.5, 10));
        assert_eq!(err("   "), FromCharsError::InvalidArgument);
        assert_eq!(err(""), FromCharsError::InvalidArgument);
    }

    #[test]
    fn from_chars_parses_a_prefix() {
        assert_eq!(ok("1.5mm"), (1.5, 3));
        assert_eq!(ok("1.5 2.5"), (1.5, 3));
        assert_eq!(ok("1.2.3"), (1.2, 3));
        assert_eq!(ok("12abc"), (12.0, 2));
        // An 'e' without a valid exponent is left alone.
        assert_eq!(ok("1e"), (1.0, 1));
        assert_eq!(ok("1e+"), (1.0, 1));
        assert_eq!(ok("1e-x"), (1.0, 1));
        assert_eq!(ok("1.5E"), (1.5, 3));
        assert_eq!(ok("1e5e5"), (1e5, 3));
    }

    #[test]
    fn from_chars_rejects_non_numbers() {
        for text in ["+1", "+.5", "-", "-x", ".", "-.", "e5", ".e5", "abc", "--1", "-+1", "(", "x1"]
        {
            assert_eq!(err(text), FromCharsError::InvalidArgument, "{text:?}");
        }
    }

    #[test]
    fn from_chars_inf_nan() {
        assert_eq!(ok("inf"), (f64::INFINITY, 3));
        assert_eq!(ok("INF"), (f64::INFINITY, 3));
        assert_eq!(ok("-inf"), (f64::NEG_INFINITY, 4));
        assert_eq!(ok("infinity"), (f64::INFINITY, 8));
        assert_eq!(ok("-InFiNiTy"), (f64::NEG_INFINITY, 9));
        assert_eq!(ok("infinit"), (f64::INFINITY, 3));
        assert_eq!(ok("  inf"), (f64::INFINITY, 5));

        let (v, used) = ok("nan");
        assert!(v.is_nan() && v.is_sign_positive());
        assert_eq!(used, 3);

        let (v, used) = ok("-NaN");
        assert!(v.is_nan() && v.is_sign_negative());
        assert_eq!(used, 4);

        assert_eq!(ok("nan(ind)").1, 8);
        assert_eq!(ok("nan(snan_1)x").1, 11);
        assert_eq!(ok("nan(").1, 3);
        assert_eq!(ok("nan(a-b)").1, 3);
        assert_eq!(ok("nanx").1, 3);

        assert_eq!(err("+inf"), FromCharsError::InvalidArgument);
        assert_eq!(err("in"), FromCharsError::InvalidArgument);
        assert_eq!(err("na"), FromCharsError::InvalidArgument);
        assert_eq!(err("-na"), FromCharsError::InvalidArgument);
    }

    #[test]
    fn from_chars_range_errors() {
        assert_eq!(err("1e999"), FromCharsError::ResultOutOfRange);
        assert_eq!(err("-1e999"), FromCharsError::ResultOutOfRange);
        assert_eq!(err("1e-999"), FromCharsError::ResultOutOfRange);
        assert_eq!(err("1e99999999999999999999"), FromCharsError::ResultOutOfRange);
        assert_eq!(err("1.8e308"), FromCharsError::ResultOutOfRange);

        // Zero with any exponent is fine, and so are subnormals and the extremes.
        assert_eq!(ok("0e999"), (0.0, 5));
        assert_eq!(ok("0.0e-99999999999999999999"), (0.0, 25));
        assert_eq!(ok("5e-324"), (5e-324, 6));
        assert_eq!(ok("1.7976931348623157e308"), (f64::MAX, 22));
        assert_eq!(ok("2.2250738585072014e-308"), (f64::MIN_POSITIVE, 23));
    }

    #[test]
    fn from_chars_is_correctly_rounded() {
        // More than 19 significant digits and classic hard cases.
        assert_eq!(ok("0.1000000000000000055511151231257827").0, 0.1);
        assert_eq!(ok("9007199254740993").0, 9007199254740992.0);
        assert_eq!(ok("9007199254740995").0, 9007199254740996.0);
        assert_eq!(ok("2.2250738585072011e-308").0, 2.2250738585072011e-308);
        assert_eq!(ok("123456789012345678901234567890").0, 1.2345678901234568e29);
    }

    #[test]
    fn strtol_base_10() {
        assert_eq!(strtol(b"42", 10), (42, 2));
        assert_eq!(strtol(b"-42", 10), (-42, 3));
        assert_eq!(strtol(b"+42", 10), (42, 3));
        assert_eq!(strtol(b"  \t42abc", 10), (42, 5));
        assert_eq!(strtol(b"1.5", 10), (1, 1));
        assert_eq!(strtol(b"abc", 10), (0, 0));
        assert_eq!(strtol(b"", 10), (0, 0));
        assert_eq!(strtol(b"-", 10), (0, 0));
        assert_eq!(strtol(b"0x10", 10), (0, 1));
        assert_eq!(strtol(b"010", 10), (10, 3));
        assert_eq!(strtol(b"12\x0034", 10), (12, 2));
    }

    #[test]
    fn strtol_clamps() {
        assert_eq!(strtol(b"9223372036854775807", 10), (i64::MAX, 19));
        assert_eq!(strtol(b"9223372036854775808", 10), (i64::MAX, 19));
        assert_eq!(strtol(b"-9223372036854775808", 10), (i64::MIN, 20));
        assert_eq!(strtol(b"-9223372036854775809", 10), (i64::MIN, 20));
        assert_eq!(strtol(b"99999999999999999999999999", 10), (i64::MAX, 26));
    }

    #[test]
    fn strtol_base_16() {
        assert_eq!(strtol(b"ff", 16), (255, 2));
        assert_eq!(strtol(b"FF", 16), (255, 2));
        assert_eq!(strtol(b"0xff", 16), (255, 4));
        assert_eq!(strtol(b"0XfF", 16), (255, 4));
        assert_eq!(strtol(b"-0x10", 16), (-16, 5));
        assert_eq!(strtol(b"0x", 16), (0, 1));
        assert_eq!(strtol(b"0xg", 16), (0, 1));
        assert_eq!(strtol(b"00000FFF_", 16), (4095, 8));
        assert_eq!(strtol(b"7fffffffffffffff", 16), (i64::MAX, 16));
        assert_eq!(strtol(b"ffffffffffffffff", 16), (i64::MAX, 16));
    }

    #[test]
    fn strtoll_base_0() {
        assert_eq!(strtoll(b"42", 0), (42, 2));
        assert_eq!(strtoll(b"-42", 0), (-42, 3));
        assert_eq!(strtoll(b"0", 0), (0, 1));
        assert_eq!(strtoll(b"010", 0), (8, 3));
        assert_eq!(strtoll(b"-012", 0), (-10, 4));
        assert_eq!(strtoll(b"08", 0), (0, 1));
        assert_eq!(strtoll(b"0129", 0), (10, 3));
        assert_eq!(strtoll(b"0x1f", 0), (31, 4));
        assert_eq!(strtoll(b"0x", 0), (0, 1));
        assert_eq!(strtoll(b"00", 0), (0, 2));
    }

    #[test]
    fn strtol_invalid_base() {
        assert_eq!(strtol(b"11", 1), (0, 0));
        assert_eq!(strtol(b"11", 37), (0, 0));
        assert_eq!(strtol(b"zz", 36), (35 * 36 + 35, 2));
        assert_eq!(strtol(b"102", 2), (2, 2));
    }

    #[test]
    fn strtod_prefixes() {
        assert_eq!(strtod_prefix(b"1.2.3"), Some((1.2, 3)));
        assert_eq!(strtod_prefix(b"-.5"), Some((-0.5, 3)));
        assert_eq!(strtod_prefix(b"5."), Some((5.0, 2)));
        assert_eq!(strtod_prefix(b"5..1"), Some((5.0, 2)));
        assert_eq!(strtod_prefix(b"3.14"), Some((3.14, 4)));
        assert_eq!(strtod_prefix(b"+2e2x"), Some((200.0, 4)));
        assert_eq!(strtod_prefix(b"2e"), Some((2.0, 1)));
        assert_eq!(strtod_prefix(b"."), None);
        assert_eq!(strtod_prefix(b"-."), None);
        assert_eq!(strtod_prefix(b"..5"), None);
        assert_eq!(strtod_prefix(b"-"), None);
        assert_eq!(strtod_prefix(b""), None);
    }
}
