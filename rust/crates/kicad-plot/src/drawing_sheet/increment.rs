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

//! Port of `STRING_INCREMENTER`, `IndexFromAlphabetic` and `AlphabeticFromIndex`
//! (`common/increment.cpp`, `include/increment.h`): the label stepping of repeated drawing
//! sheet texts (`DS_DATA_ITEM_TEXT::IncrementLabel`).
//!
//! The only user in the port is the drawing sheet; the module lives here until a crate
//! further down needs it (see `PORT_STATUS.md`).

/// Port of `STRING_INCREMENTER::STRING_PART_TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StringPartType {
    Alphabetic,
    Integer,
    Skip,
}

/// Port of `class STRING_INCREMENTER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringIncrementer {
    /// `m_SkipIOSQXZ`
    pub skip_iosqxz: bool,
    /// `m_AlphabeticMaxIndex` (negative: no limit)
    pub alphabetic_max_index: i32,
}

impl Default for StringIncrementer {
    fn default() -> Self {
        StringIncrementer {
            skip_iosqxz: true,
            alphabetic_max_index: 50,
        }
    }
}

/// The byte length of the longest suffix of `s` whose bytes all satisfy `pred` (the
/// `std::regex_search` of a `[...]+$` pattern on the UTF-8 string: every pattern used here
/// only matches ASCII bytes).
fn suffix_len(s: &[u8], pred: impl Fn(u8) -> bool) -> usize {
    s.iter().rev().take_while(|&&c| pred(c)).count()
}

/// `wxString::ToLong( &number )` of a string of ASCII digits: false on overflow.
fn to_long(digits: &str) -> Option<i64> {
    digits.parse::<i64>().ok()
}

impl StringIncrementer {
    /// Port of `STRING_INCREMENTER::Increment( aStr, aDelta, aRightIndex )`: step the
    /// `a_right_index`-th steppable part (counted from the right) of `a_str` by `a_delta`.
    pub fn increment(&self, a_str: &str, a_delta: i32, a_right_index: usize) -> Option<String> {
        if a_str.is_empty() {
            return None;
        }

        let mut remaining: Vec<u8> = a_str.as_bytes().to_vec();
        let mut parts: Vec<(Vec<u8>, StringPartType)> = Vec::new();
        let mut good_parts: usize = 0;

        while good_parts <= a_right_index && !remaining.is_empty() {
            // integerRegex( R"(\d+$)" ), sameCaseAlphabetRegex( R"(([a-z]+|[A-Z]+)$)" ),
            // skipRegex( R"([^a-zA-Z0-9]+$)" )
            let n_int = suffix_len(&remaining, |c| c.is_ascii_digit());
            let n_lower = suffix_len(&remaining, |c| c.is_ascii_lowercase());
            let n_upper = suffix_len(&remaining, |c| c.is_ascii_uppercase());
            let n_skip = suffix_len(&remaining, |c| !c.is_ascii_alphanumeric());

            let (len, kind) = if n_int > 0 {
                (n_int, StringPartType::Integer)
            } else if n_lower > 0 {
                (n_lower, StringPartType::Alphabetic)
            } else if n_upper > 0 {
                (n_upper, StringPartType::Alphabetic)
            } else if n_skip > 0 {
                (n_skip, StringPartType::Skip)
            } else {
                break;
            };

            if kind != StringPartType::Skip {
                good_parts += 1;
            }

            let start = remaining.len() - len;
            parts.push((remaining[start..].to_vec(), kind));
            remaining.truncate(start);
        }

        if good_parts <= a_right_index {
            return None;
        }

        let (last_bytes, last_kind) = parts.last().cloned()?;
        // wxString::FromUTF8 of an ASCII-only part
        let mut part = String::from_utf8(last_bytes).ok()?;

        if !self.increment_part(&mut part, last_kind, a_delta) {
            return None;
        }

        if let Some(last) = parts.last_mut() {
            last.0 = part.into_bytes();
        }

        let mut result = remaining;

        for (bytes, _) in parts.iter().rev() {
            result.extend_from_slice(bytes);
        }

        // wxString::FromUTF8: an invalid sequence gives an empty string
        Some(String::from_utf8(result).unwrap_or_default())
    }

    /// Port of `STRING_INCREMENTER::incrementPart`.
    fn increment_part(&self, a_part: &mut String, a_type: StringPartType, a_delta: i32) -> bool {
        match a_type {
            StringPartType::Integer => {
                let zero_padded = a_part.starts_with('0');
                let old_len = a_part.chars().count();

                if let Some(mut number) = to_long(a_part) {
                    if a_delta > 0 && number > i64::MAX - i64::from(a_delta) {
                        return false;
                    }

                    number += i64::from(a_delta);

                    if number < 0 {
                        return false;
                    }

                    *a_part = format!("{number}");

                    let len = a_part.chars().count();

                    if zero_padded && len < old_len {
                        *a_part = format!("{}{}", "0".repeat(old_len - len), a_part);
                    }

                    return true;
                }

                false
            }

            StringPartType::Alphabetic => {
                let upper = a_part.to_uppercase();
                let was_upper = *a_part == upper;

                const ALPHABET_FULL: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
                const ALPHA_NO_IOSQXZ: &str = "ABCDEFGHJKLMNPRTUVWY";

                let contains_iosqxz = a_part.chars().any(|c| "IOSQXZ".contains(c));
                let alpha = if self.skip_iosqxz & !contains_iosqxz {
                    ALPHA_NO_IOSQXZ
                } else {
                    ALPHABET_FULL
                };

                let index = index_from_alphabetic(&upper, alpha);

                if index == -1 {
                    return false;
                }

                if index > self.alphabetic_max_index && self.alphabetic_max_index >= 0 {
                    return false;
                }

                let next_index = i64::from(index) + i64::from(a_delta);

                if next_index < 0 || next_index > i64::from(i32::MAX) {
                    return false;
                }

                let mut new_str = alphabetic_from_index(next_index as usize, alpha, true);

                if !was_upper {
                    new_str = new_str.to_lowercase();
                }

                *a_part = new_str;
                true
            }

            StringPartType::Skip => false,
        }
    }
}

/// Port of `IndexFromAlphabetic`: the index of `a_str` in the bijective numbering of
/// `a_alphabet` (`A` = 0, `Z` = 25, `AA` = 26 ...), -1 if a letter is not in the alphabet.
pub fn index_from_alphabetic(a_str: &str, a_alphabet: &str) -> i32 {
    let mut index: i32 = 0;
    let alphabet: Vec<char> = a_alphabet.chars().collect();
    let radix = alphabet.len() as i32;
    let chars: Vec<char> = a_str.chars().collect();
    let len = chars.len();

    for (i, c) in chars.iter().enumerate() {
        let Some(found) = alphabet.iter().position(|a| a == c) else {
            return -1;
        };

        let mut alpha_index = found as i32;

        if i != len - 1 {
            alpha_index += 1;
        }

        // index += alphaIndex * pow( radix, aStr.Len() - 1 - i ): computed in double, then
        // converted back to int
        let sum = index as f64 + alpha_index as f64 * (radix as f64).powi((len - 1 - i) as i32);
        index = kicad_geom::cmath::f64_to_i32_x86(sum);
    }

    index
}

/// Port of `AlphabeticFromIndex( aN, aAlphabet, aZeroBasedNonUnitCols )`.
pub fn alphabetic_from_index(
    mut a_n: usize,
    a_alphabet: &str,
    a_zero_based_non_unit_cols: bool,
) -> String {
    let alphabet: Vec<char> = a_alphabet.chars().collect();
    let radix = alphabet.len();
    let mut item_num: Vec<char> = Vec::new();
    let mut first_round = true;

    loop {
        let mut mod_n = (a_n % radix) as i64;

        if a_zero_based_non_unit_cols && !first_round {
            mod_n -= 1; // Start the "tens/hundreds/etc column" at "Ax", not "Bx"
        }

        // aAlphabet[modN]: modN is never negative here (the column above a unit column is at
        // least 1)
        item_num.insert(0, alphabet[mod_n.max(0) as usize]);
        a_n /= radix;
        first_round = false;

        if a_n == 0 {
            break;
        }
    }

    item_num.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds_incrementer() -> StringIncrementer {
        StringIncrementer {
            skip_iosqxz: false,
            alphabetic_max_index: -1,
        }
    }

    #[test]
    fn drawing_sheet_labels() {
        let inc = ds_incrementer();
        assert_eq!(inc.increment("1", 1, 0).as_deref(), Some("2"));
        assert_eq!(inc.increment("A", 1, 0).as_deref(), Some("B"));
        assert_eq!(inc.increment("Z", 1, 0).as_deref(), Some("AA"));
        assert_eq!(inc.increment("z", 2, 0).as_deref(), Some("ab"));
        assert_eq!(inc.increment("09", 1, 0).as_deref(), Some("10"));
        assert_eq!(inc.increment("007", 1, 0).as_deref(), Some("008"));
        assert_eq!(inc.increment("R1-", 3, 0).as_deref(), Some("R4-"));
        assert_eq!(inc.increment("-", 1, 0), None);
        assert_eq!(inc.increment("", 1, 0), None);
        assert_eq!(inc.increment("1", -2, 0), None);
    }
}
