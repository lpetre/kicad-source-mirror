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

//! Port of the functions of `common/string_utils.cpp` (`include/string_utils.h`) that LIBEVAL
//! and the numeric evaluator call: `WildCompareString`, `QuacksLikeADuck` (with its helpers
//! `UnescapeString`, `SplitString`, `ApplyModifier` and `convertSeparators`) and
//! `UIDouble2Str`.
//!
//! `string_utils` as a whole belongs to `kicad-core`; this crate deliberately has no
//! dependencies, so these functions are duplicated here (see `PORT_STATUS.md`).

use crate::crt;
use crate::numeric_evaluator::NumericEvaluator;

/// Port of `WildCompareString`: compare a string against a wild card (`*` and `?`) pattern
/// using the usual rules.
///
/// Returns true if `string_to_tst` matches `pattern`.
pub fn wild_compare_string(pattern: &str, string_to_tst: &str, case_sensitive: bool) -> bool {
    let (wild_buf, str_buf): (Vec<char>, Vec<char>) = if case_sensitive {
        (pattern.chars().collect(), string_to_tst.chars().collect())
    } else {
        (
            pattern.chars().map(crt::to_upper).collect(),
            string_to_tst.chars().map(crt::to_upper).collect(),
        )
    };

    // The C++ code walks NUL-terminated wxChar buffers.
    let w = |i: usize| -> char { wild_buf.get(i).copied().unwrap_or('\0') };
    let s = |i: usize| -> char { str_buf.get(i).copied().unwrap_or('\0') };

    let mut wild = 0usize;
    let mut str_ = 0usize;
    let mut cp = 0usize;
    let mut mp = 0usize;

    while s(str_) != '\0' && w(wild) != '*' {
        if w(wild) != s(str_) && w(wild) != '?' {
            return false;
        }

        wild += 1;
        str_ += 1;
    }

    while s(str_) != '\0' {
        if w(wild) == '*' {
            wild += 1;

            if w(wild) == '\0' {
                return true;
            }

            mp = wild;
            cp = str_ + 1;
        } else if w(wild) == s(str_) || w(wild) == '?' {
            wild += 1;
            str_ += 1;
        } else {
            wild = mp;
            str_ = cp;
            cp += 1;
        }
    }

    while w(wild) == '*' {
        wild += 1;
    }

    w(wild) == '\0'
}

/// Port of `UnescapeString`: replace the `{token}` escape sequences produced by
/// `EscapeString` (e.g. `{slash}`, `{dblquote}`) by the characters they stand for.
///
/// The C++ function calls itself for the text inside unknown or prefixed (`${...}`) braces.
/// The port runs the same algorithm with an explicit stack of activation records, so deeply
/// nested braces in (untrusted) text cannot overflow the call stack.
pub fn unescape_string(source: &str) -> String {
    /// The local state of one C++ call.
    struct Frame {
        src: Vec<char>,
        i: usize,
        ch: char,
        newbuf: String,
        /// Whether `}` follows the result of the nested call this frame is waiting for.
        close: bool,
    }

    let mut stack: Vec<Frame> = Vec::new();
    // The argument of a call about to be made, and the value a finished call returned.
    let mut call: Option<Vec<char>> = Some(source.chars().collect());
    let mut ret: Option<String> = None;

    loop {
        if let Some(src) = call.take() {
            // smallest escape string is three characters, shortcut everything else
            if src.len() <= 2 {
                ret = Some(src.into_iter().collect());
            } else {
                let newbuf = String::with_capacity(src.len());
                stack.push(Frame {
                    src,
                    i: 0,
                    ch: '\0',
                    newbuf,
                    close: false,
                });
            }
        }

        let Some(frame) = stack.last_mut() else {
            return ret.unwrap_or_default();
        };

        if let Some(nested) = ret.take() {
            frame.newbuf.push_str(&nested);

            if frame.close {
                frame.newbuf.push('}');
            }

            frame.i += 1;
        }

        let source_len = frame.src.len();

        while frame.i < source_len {
            let prev = frame.ch;
            frame.ch = frame.src[frame.i];

            if frame.ch == '{' {
                let mut token: Vec<char> = Vec::new();
                let mut depth = 1;
                let mut terminated = false;

                frame.i += 1;

                while frame.i < source_len {
                    frame.ch = frame.src[frame.i];

                    if frame.ch == '{' {
                        depth += 1;
                    } else if frame.ch == '}' {
                        depth -= 1;
                    }

                    if depth <= 0 {
                        terminated = true;
                        break;
                    } else {
                        token.push(frame.ch);
                    }

                    frame.i += 1;
                }

                // Unterminated braces and braces after one of the markup prefixes are kept.
                let keep_braces =
                    !terminated || prev == '$' || prev == '~' || prev == '^' || prev == '_';

                let replacement = if keep_braces {
                    None
                } else {
                    match token.iter().collect::<String>().as_str() {
                        "dblquote" => Some('"'),
                        "quote" => Some('\''),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "backslash" => Some('\\'),
                        "slash" => Some('/'),
                        "bar" => Some('|'),
                        "comma" => Some(','),
                        "colon" => Some(':'),
                        "space" => Some(' '),
                        "dollar" => Some('$'),
                        "tab" => Some('\t'),
                        "return" => Some('\n'),
                        "brace" => Some('{'),
                        _ => None,
                    }
                };

                match replacement {
                    Some(c) => frame.newbuf.push(c),
                    None => {
                        // newbuf << "{" << UnescapeString( token ) [<< "}"]
                        frame.newbuf.push('{');
                        frame.close = terminated;
                        call = Some(token);
                        break;
                    }
                }
            } else {
                frame.newbuf.push(frame.ch);
            }

            frame.i += 1;
        }

        if call.is_some() {
            continue;
        }

        if let Some(done) = stack.pop() {
            ret = Some(done.newbuf);
        }
    }
}

/// Port of `ApplyModifier`: scale `value` by the SI/IEC 60062 modifier `string` starts with
/// (e.g. `k`, `M`, `u`, optionally followed by a unit such as `F`, `Hz`, `V`).
///
/// Returns false, leaving `value` untouched, if `string` is not a known modifier/unit
/// combination.
pub fn apply_modifier(value: &mut f64, string: &str) -> bool {
    // Although the two 'μ's look the same, they are U+03BC and U+00B5
    const MODIFIERS: &str = "afpnu\u{B5}\u{3BC}mLRFkKMGTPE";

    let Some(first) = string.chars().next() else {
        return false;
    };

    let modifier;
    let units: String;

    // wxString::Strip() without argument strips trailing white space only.
    if crt::trim_right(string) == "m" {
        // a single m is meters, not milli-undefineds
        return true;
    } else if crt::trim_right(string) == "mils" {
        *value *= 0.0000254;
        return true;
    } else if crt::trim_right(string) == "in" {
        *value *= 0.0254;
        return true;
    }

    if MODIFIERS.contains(first) {
        modifier = first;
        units = crt::trim_right(&string[first.len_utf8()..]).to_string();
    } else {
        modifier = ' ';
        units = crt::trim_right(string).to_string();
    }

    if !units.is_empty()
        && !crt::is_same_as_no_case(&units, "F")
        && !crt::is_same_as_no_case(&units, "hz")
        && !crt::is_same_as_no_case(&units, "\u{3A9}")
        && !crt::is_same_as_no_case(&units, "W")
        && !crt::is_same_as_no_case(&units, "V")
        && !crt::is_same_as_no_case(&units, "A")
        && !crt::is_same_as_no_case(&units, "H")
        && !crt::is_same_as_no_case(&units, "s")
        && !crt::is_same_as_no_case(&units, "m")
    {
        return false;
    }

    // Note: most of these are SI, but some (L, R, F) are IEC 60062.
    // (The if / else-if structure, including the two chains that are not joined by an
    // "else", is the C++ one.)
    if modifier == 'a' {
        *value *= 1.0e-18;
    } else if modifier == 'f' {
        *value *= 1.0e-15;
    }

    if modifier == 'p' {
        *value *= 1.0e-12;
    }

    if modifier == 'n' {
        *value *= 1.0e-9;
    } else if modifier == 'u' || modifier == '\u{B5}' || modifier == '\u{3BC}' {
        // Although the two 'μ's look the same, they are U+03BC and U+00B5
        *value *= 1.0e-6;
    } else if modifier == 'm' || modifier == 'L' {
        *value *= 1.0e-3;
    } else if modifier == 'R' || modifier == 'F' {
        // unity scalar
    } else if modifier == 'c' {
        // primarily for cm
        *value *= 1.0e2;
    } else if modifier == 'k' || modifier == 'K' {
        *value *= 1.0e3;
    } else if modifier == 'M' {
        *value *= 1.0e6;
    } else if modifier == 'G' {
        *value *= 1.0e9;
    } else if modifier == 'T' {
        *value *= 1.0e12;
    } else if modifier == 'P' {
        *value *= 1.0e15;
    } else if modifier == 'E' {
        *value *= 1.0e18;
    }

    true
}

/// Port of the file-static `convertSeparators`: turn a number written with thousands and
/// decimal separators into "C" locale notation.
///
/// Returns false (after having removed the spaces only) when the separators are
/// inconsistent.
pub fn convert_separators(value: &mut String) -> bool {
    // Note: fetching the decimal separator from the current locale isn't a silver bullet because
    // it assumes the current computer's locale is the same as the locale the schematic was
    // authored in -- something that isn't true, for instance, when sharing designs through
    // DIYAudio.com.
    //
    // Some values are self-describing: multiple instances of a single separator character must be
    // thousands separators; a single instance of each character must be a thousands separator
    // followed by a decimal separator; etc.
    //
    // Only when presented with an ambiguous value do we fall back on the current locale.

    *value = value.replace(' ', "");

    let chars: Vec<char> = value.chars().collect();

    let mut ambiguous_separator = '?';
    let mut thousands_separator = '?';
    let mut thousands_separator_found = false;
    let mut decimal_separator = '?';
    let mut decimal_separator_found = false;
    let mut digits = 0;

    for ii in (0..chars.len()).rev() {
        let c = chars[ii];

        if c.is_ascii_digit() {
            digits += 1;
        } else if c == '.' || c == ',' {
            if decimal_separator != '?' || thousands_separator != '?' {
                // We've previously found a non-ambiguous separator...

                if c == decimal_separator {
                    if thousands_separator_found {
                        return false; // decimal before thousands
                    } else if decimal_separator_found {
                        return false; // more than one decimal
                    } else {
                        decimal_separator_found = true;
                    }
                } else if c == thousands_separator {
                    if digits != 3 {
                        return false; // thousands not followed by 3 digits
                    } else {
                        thousands_separator_found = true;
                    }
                }
            } else if ambiguous_separator != '?' {
                // We've previously found a separator, but we don't know for sure which...

                if c == ambiguous_separator {
                    // They both must be thousands separators
                    thousands_separator = ambiguous_separator;
                    thousands_separator_found = true;
                    decimal_separator = if c == '.' { ',' } else { '.' };
                } else {
                    // The first must have been a decimal, and this must be a thousands.
                    decimal_separator = ambiguous_separator;
                    decimal_separator_found = true;
                    thousands_separator = c;
                    thousands_separator_found = true;
                }
            } else {
                // This is the first separator...

                // If it's preceded by a '0' (only), or if it's followed by some number of
                // digits not equal to 3, then it -must- be a decimal separator.
                //
                // In all other cases we don't really know what it is yet.

                if (ii == 1 && chars[0] == '0') || digits != 3 {
                    decimal_separator = c;
                    decimal_separator_found = true;
                    thousands_separator = if c == '.' { ',' } else { '.' };
                } else {
                    ambiguous_separator = c;
                }
            }

            digits = 0;
        } else {
            digits = 0;
        }
    }

    // If we found nothing definitive then we have to look at the current locale
    if decimal_separator == '?' && thousands_separator == '?' {
        decimal_separator = crt::LOCALE_DECIMAL_POINT;
        thousands_separator = if decimal_separator == '.' { ',' } else { '.' };
    }

    // Convert to C-locale
    *value = value.replace(thousands_separator, "");
    *value = value.replace(decimal_separator, ".");

    true
}

/// Port of `SplitString`: break a string into three parts: the alphabetic preamble, the
/// numeric part, and any alphabetic ending.  For example `C10A` is split into `C`, `10`, `A`.
///
/// Returns `(beginning, digits, end)`.
pub fn split_string(str_to_split: &str) -> (String, String, String) {
    const SEPARATORS: &str = ".,";

    let chars: Vec<char> = str_to_split.chars().collect();
    let substr =
        |from: usize, len: usize| -> String { chars.iter().skip(from).take(len).collect() };

    let mut infix: Option<char> = None;

    // Clear all the return strings
    let mut str_beginning = String::new();
    let mut str_digits = String::new();
    let mut str_end = String::new();

    // There no need to do anything if the string is empty
    if chars.is_empty() {
        return (str_beginning, str_digits, str_end);
    }

    // Starting at the end of the string look for the first digit
    let mut ii = chars.len() as isize - 1;

    while ii >= 0 {
        if chars[ii as usize].is_ascii_digit() {
            break;
        }

        ii -= 1;
    }

    // If there were no digits then just set the single string
    if ii < 0 {
        str_beginning = str_to_split.to_string();
    } else {
        // Since there is at least one digit this is the trailing string
        str_end = substr(ii as usize + 1, usize::MAX);

        // Go to the end of the digits
        let position = ii + 1;

        while ii >= 0 {
            let c = chars[ii as usize];

            if c.is_ascii_digit() {
                ii -= 1;
                continue;
            }

            // This can be tricky to get to parse things like 4K7 (or just K7)
            // but not G4W
            let is_infix = {
                // Only allow one infix character e.g. 4K7, not 4KK7
                infix.is_none()
                // Allowed only it isn't the first character, or nothing follows it,
                // e.g. K7 but not K7X
                && (ii > 0 || str_end.is_empty())
                // Also make sure its a valid SI separator, e.g. 4K7 but not 4X7
                // Finally make that the combo makes sense e.g. T9G fails because TG is not a
                // valid combo, but unfortunately cursed constructions like 1u5F are indeed
                // found in the wild
                && match NumericEvaluator::is_old_school_decimal_separator(c) {
                    Some(mut scale) => apply_modifier(&mut scale, &format!("{c}{str_end}")),
                    None => false,
                }
            };

            if is_infix {
                infix = Some(c);
                ii -= 1;
                continue;
            } else if SEPARATORS.contains(c) {
                ii -= 1;
                continue;
            } else {
                break;
            }
        }

        // If all that was left was digits, then just set the digits string
        if ii < 0 {
            str_digits = substr(0, position as usize);
        }
        // Otherwise everything else is part of the preamble
        else {
            str_digits = substr(ii as usize + 1, (position - ii - 1) as usize);
            str_beginning = substr(0, ii as usize + 1);
        }

        if let Some(infix) = infix {
            str_digits = str_digits.replace(infix, ".");
            str_end = format!("{infix}{str_end}");
        }
    }

    (str_beginning, str_digits, str_end)
}

/// Port of `QuacksLikeADuck`: does the string look like a number (possibly with thousands
/// separators, an SI modifier and a unit, e.g. `4k7` or `1,000.5 uF`)?
///
/// Returns the value if it does (C++: `true` and `*aValue`), `None` otherwise (C++: `false`,
/// `*aValue` is NaN).
pub fn quacks_like_a_duck(text: &str) -> Option<f64> {
    let s = unescape_string(text);

    let (before_digits, mut digits, after_digits) = split_string(&s);

    if !crt::strip_both(&before_digits).is_empty() {
        return None;
    }

    // The result of convertSeparators() is ignored by the C++ code as well.
    convert_separators(&mut digits);

    // wxString::ToCDouble() must consume the whole string.
    let mut number = match crt::to_double(&digits) {
        (Some(value), true) => value,
        _ => return None,
    };

    if !after_digits.is_empty() && !apply_modifier(&mut number, &after_digits) {
        return None;
    }

    Some(number)
}

/// Port of `UIDouble2Str`: print a double the way the user interface shows it (no exponent
/// for small values, at most 10 significant digits otherwise).
pub fn ui_double2_str(value: f64) -> String {
    if value != 0.0 && value.abs() <= 0.0001 {
        // For these small values, %f works fine,
        // and %g gives an exponent
        let mut buf = crt::format_f(value, 16).into_bytes();
        let mut len = buf.len() as isize;

        // while( --len > 0 && buf[len] == '0' ) buf[len] = '\0';
        loop {
            len -= 1;

            if len > 0 && buf[len as usize] == b'0' {
                buf.truncate(len as usize);
            } else {
                break;
            }
        }

        if len >= 0 && (buf[len as usize] == b'.' || buf[len as usize] == b',') {
            buf.truncate(len as usize);
        } else {
            len += 1;
            buf.truncate(len.max(0) as usize);
        }

        String::from_utf8_lossy(&buf).into_owned()
    } else {
        // For these values, %g works fine, and sometimes %f
        // gives a bad value (try aValue = 1.222222222222, with %.16f format!)
        crt::format_g(value, 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wild_compare() {
        assert!(wild_compare_string("HV_*", "HV_LINE", false));
        assert!(wild_compare_string("hv_*", "HV_LINE", false));
        assert!(!wild_compare_string("hv_*", "HV_LINE", true));
        assert!(wild_compare_string("*", "", false));
        assert!(wild_compare_string("*", "anything", false));
        assert!(wild_compare_string("a?c", "abc", true));
        assert!(!wild_compare_string("a?c", "ac", true));
        assert!(wild_compare_string("*.Cu", "F.Cu", true));
        assert!(wild_compare_string("a*b*c", "aXXbYYc", true));
        assert!(!wild_compare_string("a*b*c", "aXXbYY", true));
        assert!(wild_compare_string("", "", true));
        assert!(!wild_compare_string("", "a", true));
        assert!(!wild_compare_string("a", "", true));
        assert!(wild_compare_string("a**", "a", true));
        assert!(wild_compare_string("*a", "bababa", true));
        assert!(!wild_compare_string("?", "", true));
    }

    #[test]
    fn unescape() {
        assert_eq!(unescape_string("a{slash}b"), "a/b");
        assert_eq!(unescape_string("{dblquote}x{dblquote}"), "\"x\"");
        assert_eq!(unescape_string("${VAR}"), "${VAR}");
        assert_eq!(unescape_string("{unknown}"), "{unknown}");
        assert_eq!(unescape_string("{open"), "{open");
        assert_eq!(unescape_string("{}"), "{}");
        assert_eq!(unescape_string("{{slash}}"), "{/}");
        assert_eq!(unescape_string("a{b{c"), "a{b{c");
        assert_eq!(unescape_string("x{y{slash}z}w"), "x{y/z}w");
        assert_eq!(unescape_string("_{a{lt}b}{gt}"), "_{a<b}>");
        assert_eq!(unescape_string("{a}"), "{a}");
        assert_eq!(unescape_string("{ab}{"), "{ab}{");

        // Deep nesting must not exhaust the stack.
        let deep = "{".repeat(4000);
        assert_eq!(unescape_string(&deep), deep);
    }

    #[test]
    fn split() {
        let t = |s: &str| {
            let (a, b, c) = split_string(s);
            format!("{a}|{b}|{c}")
        };

        assert_eq!(t("C10A"), "C|10|A");
        assert_eq!(t("42"), "|42|");
        assert_eq!(t("4k7"), "|4.7|k");
        assert_eq!(t("abc"), "abc||");
        assert_eq!(t("1.5mm"), "|1.5|mm");
        assert_eq!(t(""), "||");
        assert_eq!(t("1,000.5uF"), "|1,000.5|uF");
    }

    #[test]
    fn quacks() {
        assert_eq!(quacks_like_a_duck("42"), Some(42.0));
        assert_eq!(quacks_like_a_duck("5"), Some(5.0));
        assert_eq!(quacks_like_a_duck("4k7"), Some(4700.0));
        assert_eq!(quacks_like_a_duck("1,000.5"), Some(1000.5));
        assert_eq!(quacks_like_a_duck("hello"), None);
        assert_eq!(quacks_like_a_duck(""), None);
        assert_eq!(quacks_like_a_duck("-5"), None);
        // Values confirmed with the C++ implementation (tests/oracle).
        assert_eq!(quacks_like_a_duck("R5"), Some(0.5));
        assert_eq!(quacks_like_a_duck("1.5mm"), Some(0.0015));
        assert_eq!(quacks_like_a_duck("10k"), Some(10000.0));
        assert_eq!(quacks_like_a_duck("1.2.3"), None);
    }

    #[test]
    fn ui_double() {
        assert_eq!(ui_double2_str(0.0), "0");
        assert_eq!(ui_double2_str(1.0), "1");
        assert_eq!(ui_double2_str(25.4), "25.4");
        assert_eq!(ui_double2_str(0.0001), "0.0001");
        assert_eq!(ui_double2_str(0.00001), "0.00001");
        assert_eq!(ui_double2_str(-0.00001), "-0.00001");
        assert_eq!(ui_double2_str(1e-20), "0");
        assert_eq!(ui_double2_str(1013.05), "1013.05");
        assert_eq!(ui_double2_str(87.2), "87.2");
    }
}
