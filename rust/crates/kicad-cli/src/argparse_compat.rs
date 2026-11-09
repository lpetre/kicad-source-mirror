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

//! The parts of `thirdparty/argparse/include/argparse/argparse.hpp` (p-ranav argparse, MIT)
//! whose behaviour reaches the user: the number scanners behind `scan<'i', int>()` and
//! `scan<'g', double>()`, and the texts of the parse errors `PGM_KICAD::OnPgmRun` prints.
//!
//! `clap` does the parsing; this module makes its failures look like argparse's: the same
//! accepted number syntax (including argparse's `0x` / `0b` / leading-`0` octal detection for
//! integers) and the same one-line messages. The help layout itself is clap's.

use clap::error::{ContextKind, ContextValue, ErrorKind};

/// Result of `std::from_chars` for an integer.
enum FromCharsError {
    /// `std::errc::invalid_argument`: no digits.
    Invalid,
    /// `std::errc::result_out_of_range`.
    Range,
}

/// `std::from_chars( first, last, int&, base )`: optional `-`, then digits of `radix`.
/// Returns the value (or error) and the number of bytes consumed.
fn from_chars_i32(s: &str, radix: u32) -> (Result<i32, FromCharsError>, usize) {
    let bytes = s.as_bytes();
    let mut idx = 0;
    let negative = bytes.first() == Some(&b'-');

    if negative {
        idx = 1;
    }

    let start = idx;
    let mut value: i128 = 0;
    let mut overflow = false;

    while idx < bytes.len() {
        let Some(d) = (bytes[idx] as char).to_digit(radix) else {
            break;
        };

        if !overflow {
            value = value * radix as i128 + d as i128;

            if value > i64::MAX as i128 {
                overflow = true;
            }
        }

        idx += 1;
    }

    if idx == start {
        return (Err(FromCharsError::Invalid), 0);
    }

    let signed = if negative { -value } else { value };

    if overflow || signed < i32::MIN as i128 || signed > i32::MAX as i128 {
        return (Err(FromCharsError::Range), idx);
    }

    (Ok(signed as i32), idx)
}

/// argparse `do_from_chars<int, radix>( s )`.
fn do_from_chars(s: &str, radix: u32) -> Result<i32, String> {
    match from_chars_i32(s, radix) {
        (Ok(x), consumed) if consumed == s.len() => Ok(x),
        (Ok(_), _) => Err(format!("pattern '{s}' does not match to the end")),
        (Err(FromCharsError::Invalid), _) => Err(format!("pattern '{s}' not found")),
        (Err(FromCharsError::Range), _) => Err(format!("'{s}' not representable")),
    }
}

/// argparse `parse_number<int>` (the `scan<'i', int>()` scanner): `0x`/`0X` hexadecimal,
/// `0b`/`0B` binary, a leading `0` octal, else decimal. The error text is what argparse
/// throws (and `kicad-cli` prints).
pub fn scan_int(s: &str) -> Result<i32, String> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return do_from_chars(rest, 16)
            .map_err(|e| format!("Failed to parse '{s}' as hexadecimal: {e}"));
    }

    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return do_from_chars(rest, 2).map_err(|e| format!("Failed to parse '{s}' as binary: {e}"));
    }

    if s.starts_with('0') {
        return do_from_chars(s, 8).map_err(|e| format!("Failed to parse '{s}' as octal: {e}"));
    }

    do_from_chars(s, 10).map_err(|e| format!("Failed to parse '{s}' as decimal integer: {e}"))
}

/// Length of the longest prefix of `s` that glibc `strtod` converts (decimal and
/// `inf`/`infinity`/`nan[(...)]` forms; hexadecimal floats are not supported, see
/// `PORT_STATUS.md`). 0 when nothing converts.
fn strtod_prefix_len(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;

    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }

    let lower: String = s[i..].chars().take(8).collect::<String>().to_ascii_lowercase();

    if lower.starts_with("infinity") {
        return i + 8;
    }

    if lower.starts_with("inf") {
        return i + 3;
    }

    if lower.starts_with("nan") {
        let mut j = i + 3;

        if j < b.len() && b[j] == b'(' {
            let mut k = j + 1;

            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }

            if k < b.len() && b[k] == b')' {
                j = k + 1;
            }
        }

        return j;
    }

    let mut digits = 0;

    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }

    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        let mut frac = 0;

        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            frac += 1;
        }

        if digits + frac > 0 {
            i = j;
            digits += frac;
        }
    }

    if digits == 0 {
        return 0;
    }

    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;

        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }

        let exp_start = j;

        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }

        if j > exp_start {
            i = j;
        }
    }

    i
}

/// argparse `do_strtod<double>` + `parse_number<double, general>` (the `scan<'g', double>()`
/// scanner) on top of glibc `strtod` semantics.
pub fn scan_double(s: &str) -> Result<f64, String> {
    let wrap = |e: String| format!("Failed to parse '{s}' as number: {e}");

    if s.starts_with("0x") || s.starts_with("0X") {
        return Err("chars_format::general does not parse hexfloat".to_string());
    }

    if s.starts_with("0b") || s.starts_with("0B") {
        return Err("chars_format::general does not parse binfloat".to_string());
    }

    // `s[0]` of an empty std::string is the terminating NUL: neither space nor '+'.
    if let Some(&c) = s.as_bytes().first() {
        if c.is_ascii_whitespace() || c == b'\x0b' || c == b'+' {
            return Err(wrap(format!("pattern '{s}' not found")));
        }
    }

    let len = strtod_prefix_len(s);
    let prefix = &s[..len];

    let value: f64 = if len == 0 {
        0.0
    } else {
        let lower = prefix.to_ascii_lowercase();
        let unsigned = lower.trim_start_matches(['-', '+']);
        let negative = lower.starts_with('-');

        if unsigned.starts_with("inf") {
            if negative { f64::NEG_INFINITY } else { f64::INFINITY }
        } else if unsigned.starts_with("nan") {
            f64::NAN
        } else {
            let v: f64 = prefix.parse().unwrap_or(0.0);

            // glibc sets ERANGE on overflow and on underflow (a non-zero decimal that rounds
            // below DBL_MIN).
            let nonzero_mantissa = prefix
                .split(['e', 'E'])
                .next()
                .unwrap_or("")
                .bytes()
                .any(|c| (b'1'..=b'9').contains(&c));

            if v.is_infinite() || (nonzero_mantissa && v.abs() < f64::MIN_POSITIVE) {
                return Err(wrap(format!("'{s}' not representable")));
            }

            v
        }
    };

    if len == s.len() {
        Ok(value)
    } else {
        Err(wrap(format!("pattern '{s}' does not match to the end")))
    }
}

/// `details::get_levenshtein_distance`.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();

    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];

        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j - 1].min(prev[j]).min(cur[j - 1])
            };
        }

        prev = cur;
    }

    prev[b.len()]
}

/// `details::get_most_similar_string( m_subparser_map, input )`: the first name (in sorted
/// order, `std::map`) with the smallest edit distance.
pub fn most_similar<'a>(names: &[&'a str], input: &str) -> &'a str {
    let mut sorted: Vec<&str> = names.to_vec();
    sorted.sort();
    let mut best = "";
    let mut min_distance = usize::MAX;

    for name in sorted {
        let distance = levenshtein(name, input);

        if distance < min_distance {
            min_distance = distance;
            best = name;
        }
    }

    best
}

/// The argparse message for a `clap` error, as `PGM_KICAD::OnPgmRun` prints it (`err.what()`).
///
/// `subcommands` are the names of the subcommands of the deepest command that was reached,
/// `takes_input` tells whether that command has the positional `input` argument, and
/// `first_value_option` is the usage text of its first option taking a value (for argparse's
/// "Zero positional arguments expected, did you mean ..." hint).
pub fn argparse_message(
    err: &clap::Error,
    subcommands: &[&str],
    takes_input: bool,
    first_value_option: Option<&str>,
) -> String {
    let invalid_arg = match err.get(ContextKind::InvalidArg) {
        Some(ContextValue::String(s)) => s.clone(),
        Some(ContextValue::Strings(v)) => v.first().cloned().unwrap_or_default(),
        _ => String::new(),
    };

    // "--format <FORMAT>" -> "--format"
    let option_name = invalid_arg
        .split([' ', '='])
        .next()
        .unwrap_or("")
        .to_string();

    match err.kind() {
        ErrorKind::ValueValidation => {
            use std::error::Error;

            match err.source() {
                Some(source) => source.to_string(),
                None => err.to_string(),
            }
        }
        ErrorKind::MissingRequiredArgument => "input: 1 argument(s) expected. 0 provided.".to_string(),
        ErrorKind::ArgumentConflict => format!("Duplicate argument {option_name}"),
        ErrorKind::InvalidSubcommand => {
            format!(
                "Failed to parse '{invalid_arg}', did you mean '{}'",
                most_similar(subcommands, &invalid_arg)
            )
        }
        ErrorKind::UnknownArgument => {
            if invalid_arg.starts_with('-') && invalid_arg.len() > 1 {
                format!("Unknown argument: {invalid_arg}")
            } else if !subcommands.is_empty() {
                format!(
                    "Failed to parse '{invalid_arg}', did you mean '{}'",
                    most_similar(subcommands, &invalid_arg)
                )
            } else if takes_input {
                format!(
                    "Maximum number of positional arguments exceeded, failed to parse \
                     '{invalid_arg}'"
                )
            } else if let Some(opt) = first_value_option {
                format!("Zero positional arguments expected, did you mean {opt}")
            } else {
                "Zero positional arguments expected".to_string()
            }
        }
        _ => {
            // Anything else: clap's own text without its "error: " prefix and usage tail.
            let text = err.to_string();
            let first = text.lines().next().unwrap_or("").to_string();
            first.strip_prefix("error: ").unwrap_or(&first).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_like_argparse() {
        assert_eq!(scan_int("6"), Ok(6));
        assert_eq!(scan_int("-3"), Ok(-3));
        assert_eq!(scan_int("0x1A"), Ok(26));
        assert_eq!(scan_int("0b101"), Ok(5));
        assert_eq!(scan_int("010"), Ok(8));
        assert_eq!(scan_int("0"), Ok(0));
        assert_eq!(
            scan_int("abc").unwrap_err(),
            "Failed to parse 'abc' as decimal integer: pattern 'abc' not found"
        );
        assert_eq!(
            scan_int("1.5").unwrap_err(),
            "Failed to parse '1.5' as decimal integer: pattern '1.5' does not match to the end"
        );
        assert_eq!(
            scan_int("08").unwrap_err(),
            "Failed to parse '08' as octal: pattern '08' does not match to the end"
        );
        assert_eq!(
            scan_int("99999999999").unwrap_err(),
            "Failed to parse '99999999999' as decimal integer: '99999999999' not representable"
        );
        assert_eq!(
            scan_int("+5").unwrap_err(),
            "Failed to parse '+5' as decimal integer: pattern '+5' not found"
        );
    }

    #[test]
    fn doubles_like_argparse() {
        assert_eq!(scan_double("1"), Ok(1.0));
        assert_eq!(scan_double("0.5"), Ok(0.5));
        assert_eq!(scan_double("-2e3"), Ok(-2000.0));
        assert_eq!(scan_double(".5"), Ok(0.5));
        assert_eq!(scan_double("5."), Ok(5.0));
        assert_eq!(scan_double(""), Ok(0.0));
        assert_eq!(
            scan_double("abc").unwrap_err(),
            "Failed to parse 'abc' as number: pattern 'abc' does not match to the end"
        );
        assert_eq!(
            scan_double("+1").unwrap_err(),
            "Failed to parse '+1' as number: pattern '+1' not found"
        );
        assert_eq!(
            scan_double("1e999").unwrap_err(),
            "Failed to parse '1e999' as number: '1e999' not representable"
        );
        assert_eq!(
            scan_double("1.5x").unwrap_err(),
            "Failed to parse '1.5x' as number: pattern '1.5x' does not match to the end"
        );
        assert!(scan_double("inf").unwrap().is_infinite());
    }

    #[test]
    fn similar_names() {
        let names = ["diff", "downgrade", "drc", "export", "import", "render", "upgrade"];
        assert_eq!(most_similar(&names, "foo"), "drc");
        let top = [
            "api-server", "fp", "gerber", "import", "jobset", "mergetool", "pcb", "project",
            "sch", "sym", "version",
        ];
        assert_eq!(most_similar(&top, "bogus"), "fp");
    }
}
