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

//! The functions of `common/string_utils.cpp` (`include/string_utils.h`) that LIBEVAL and the
//! numeric evaluator call: `WildCompareString`, `QuacksLikeADuck` (with its helpers
//! `UnescapeString`, `SplitString`, `ApplyModifier` and `convertSeparators`) and
//! `UIDouble2Str`.
//!
//! These are the `kicad-core` ports (`kicad_core::string_utils`), re-exported under this
//! crate's path.

pub use kicad_core::string_utils::{
    apply_modifier, convert_separators, quacks_like_a_duck, split_string, ui_double2_str,
    unescape_string, wild_compare_string,
};

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
