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

//! Differential test of [`kicad_core::string_utils::wx_string_matches`] against
//! `wxString::Matches` of libwx_baseu 3.2: replays the 51,200 pseudo-random (text, mask) cases
//! of `oracle/wx_matches_oracle.cpp` and compares the results with
//! `golden/wx_matches_oracle.txt` (regenerate with `oracle/build_wx_matches_oracle.sh`).

use kicad_core::string_utils::wx_string_matches;

const CASES: usize = 64 * 800;

/// splitmix64, as `RNG` in the driver.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

const ALPHABET: [char; 9] = ['a', 'b', 'c', '.', '*', '?', '\u{e9}', '\u{1f600}', '\0'];

/// `randomString` of the driver.
fn random_string(rng: &mut Rng, mask: bool) -> String {
    let len = if rng.below(4) == 0 {
        rng.below(16)
    } else {
        rng.below(7)
    };
    let mut s = String::new();

    for _ in 0..len {
        let r = rng.below(100);
        let c = if r < 2 {
            '\0'
        } else if r < 6 {
            ALPHABET[6 + rng.below(2) as usize]
        } else if (mask && r < 40) || (!mask && r < 9) {
            ALPHABET[4 + rng.below(2) as usize]
        } else {
            ALPHABET[rng.below(4) as usize]
        };
        s.push(c);
    }

    s
}

#[test]
fn wx_string_matches_agrees_with_wx() {
    let golden = include_str!("golden/wx_matches_oracle.txt");
    let mut expected = golden.lines();
    let mut rng = Rng(77031);
    let mut bits = 0u64;
    let mut matched = 0;

    for c in 0..CASES {
        let text = random_string(&mut rng, false);
        let mask = random_string(&mut rng, true);

        if wx_string_matches(&text, &mask) {
            bits |= 1 << (c % 64);
            matched += 1;
        }

        if c % 64 == 63 {
            assert_eq!(
                Some(format!("m|{}|{bits:016x}", c - 63).as_str()),
                expected.next(),
                "cases {}..={c}",
                c - 63
            );
            bits = 0;
        }
    }

    assert_eq!(expected.next(), None);
    assert!(matched > 1000, "{matched}");
}

/// Hand-picked cases (the `wxString::Matches` rules `NETCLASS::ContainsNetclassWithName`
/// relies on).
#[test]
fn wx_string_matches_rules() {
    assert!(wx_string_matches("abc", "a*c"));
    assert!(wx_string_matches("abc", "*"));
    assert!(wx_string_matches("", "*"));
    assert!(!wx_string_matches("", "?"));
    assert!(wx_string_matches("abcbc", "a*bc"));
    assert!(wx_string_matches("aXbYb", "a*b"));
    assert!(!wx_string_matches("abd", "a*c"));
    // wx skips '?' directly after '*': "a*?" matches "a" (WildCompareString would not)
    assert!(wx_string_matches("a", "a*?"));
    assert!(wx_string_matches("a.b.c", "*.c"));
    assert!(!wx_string_matches("ab", "a"));
    // the C string ends at an embedded NUL
    assert!(wx_string_matches("ab\0cd", "ab"));
}

/// `wxString::CmpNoCase` / `IsSameAs( .., false )` values printed by libwx_baseu 3.2 (the
/// reference build's wx) for the same strings.
#[test]
fn cmp_no_case_matches_wx() {
    use kicad_core::wxstr::{cmp_no_case, is_same_as_no_case};

    let cases: [(&str, &str, i32); 10] = [
        ("a", "c", -2),
        ("C", "a", 2),
        ("ab", "a", 98),
        ("a", "ab", -98),
        ("a\0b", "a\0c", -1),
        ("a", "a\0", -1),
        ("a\0", "a", 1),
        ("a\0\0x", "a\0y", -1),
        ("x\0", "x\0", 0),
        ("\u{e9}", "E", 132),
    ];

    for (a, b, expected) in cases {
        assert_eq!(cmp_no_case(a, b), expected, "{a:?} {b:?}");
        assert_eq!(is_same_as_no_case(a, b), expected == 0, "{a:?} {b:?}");
    }
}
