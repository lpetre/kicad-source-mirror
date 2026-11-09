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

//! Port of `qa/tests/common/test_string_utils.cpp`.
//!
//! Not ported: the `ConvertPathToFileUri_*` cases (file system, not part of the port) and
//! `SplitGraphemeClustersKeepsCharactersWhole` (`SplitGraphemeClusters` needs the harfbuzz
//! Unicode tables, see `PORT_STATUS.md`).

use kicad_core::string_utils::*;

#[test]
fn generated_net_names_use_reserved_prefixes() {
    assert!(is_auto_generated_net_name("unconnected-(VD1A-A-Pad2)"));
    assert!(is_auto_generated_net_name("Net-(VD1-Pad2)"));
    assert!(!is_auto_generated_net_name("/Net-(A)/SIG"));
    assert!(!is_auto_generated_net_name("MyNet-(U1-Pad2)"));
    assert!(!is_auto_generated_net_name(""));
}

#[test]
fn replace_illegal_file_name_chars_std_string_no_change() {
    let mut name = String::from("valid_filename-123._ok");
    let original = name.clone();
    assert!(!replace_illegal_file_name_chars_std(&mut name, 0));
    assert_eq!(name, original);
}

fn check_percent_encoded(name: &str) {
    for enc in [
        "%5c", "%2f", "%3f", "%2a", "%7c", "%22", "%3c", "%3e", ".txt", "bad",
    ] {
        assert!(name.contains(enc), "{name} lacks {enc}");
    }
}

#[test]
fn replace_illegal_file_name_chars_std_string_percent_encoding() {
    let mut name = String::from(r#"bad\name/with?illegal*chars|"<>.txt"#);
    assert!(replace_illegal_file_name_chars_std(&mut name, 0));
    check_percent_encoded(&name);
}

#[test]
fn replace_illegal_file_name_chars_std_string_replacement_char() {
    let mut name = String::from("a*bad?name|file");
    assert!(replace_illegal_file_name_chars_std(&mut name, b'_'));
    assert_eq!(name, "a_bad_name_file");
}

#[test]
fn replace_illegal_file_name_chars_wx_string_no_change() {
    let mut name = String::from("valid_filename-123._ok");
    let original = name.clone();
    assert!(!replace_illegal_file_name_chars(&mut name, '\0'));
    assert_eq!(name, original);
}

#[test]
fn replace_illegal_file_name_chars_wx_string_percent_encoding() {
    let mut name = String::from("bad\\name/with?illegal*chars|\"<>.txt");
    assert!(replace_illegal_file_name_chars(&mut name, '\0'));
    check_percent_encoded(&name);
}

#[test]
fn replace_illegal_file_name_chars_wx_string_replacement_char() {
    let mut name = String::from("a*bad?name|file");
    assert!(replace_illegal_file_name_chars(&mut name, '_'));
    assert_eq!(name, "a_bad_name_file");
}

#[test]
fn stacked_pin_notation_comma_in_plain_number_is_single_pin() {
    let (expanded, valid) = expand_stacked_pin_notation("1,foo,bar,buz");
    assert!(valid);
    assert_eq!(expanded, vec!["1,foo,bar,buz"]);
    assert_eq!(count_stacked_pin_notation("1,foo,bar,buz"), (1, true));
}

#[test]
fn stacked_pin_notation_escaped_comma_round_trips() {
    let raw = "1,foo";
    let escaped = escape_stacked_pin_item(raw);
    assert_eq!(escaped, "1\\,foo");
    let notation = format!("[{escaped},2]");
    let (expanded, valid) = expand_stacked_pin_notation(&notation);
    assert!(valid);
    assert_eq!(expanded, vec![raw, "2"]);
    assert_eq!(count_stacked_pin_notation(&notation), (2, true));
}

#[test]
fn stacked_pin_notation_escaped_dash_is_literal() {
    let (expanded, valid) = expand_stacked_pin_notation("[A\\-B,C]");
    assert!(valid);
    assert_eq!(expanded, vec!["A-B", "C"]);

    // An unescaped dash still forms a range.
    let (expanded, valid) = expand_stacked_pin_notation("[1-3]");
    assert!(valid);
    assert_eq!(expanded.len(), 3);
    assert_eq!(expanded[0], "1");
    assert_eq!(expanded[2], "3");
}

#[test]
fn stacked_pin_notation_escape_stacked_pin_item() {
    assert_eq!(escape_stacked_pin_item("A1"), "A1");
    assert_eq!(escape_stacked_pin_item("a,b"), "a\\,b");
    assert_eq!(escape_stacked_pin_item("a-b"), "a\\-b");
    assert_eq!(escape_stacked_pin_item("[x]"), "\\[x\\]");
    assert_eq!(escape_stacked_pin_item("a\\b"), "a\\\\b");
}

#[test]
fn stacked_pin_notation_legacy_notation_unchanged() {
    let (expanded, valid) = expand_stacked_pin_notation("[6,7,9-11]");
    assert!(valid);
    assert_eq!(expanded, vec!["6", "7", "9", "10", "11"]);
    assert_eq!(count_stacked_pin_notation("[6,7,9-11]"), (5, true));
}

#[test]
fn stacked_pin_notation_display_split_preserves_ranges() {
    let items = split_stacked_pin_display_items("1-5,a\\,b,7");
    assert_eq!(items, vec!["1-5", "a,b", "7"]);
}

#[test]
fn stacked_pin_notation_literal_backslash_preserved() {
    let (expanded, valid) = expand_stacked_pin_notation("[A\\B,C]");
    assert!(valid);
    assert_eq!(expanded, vec!["A\\B", "C"]);
    assert_eq!(escape_stacked_pin_item("A\\B"), "A\\\\B");
}

#[test]
fn stacked_pin_notation_empty_is_single_pin() {
    assert_eq!(count_stacked_pin_notation(""), (1, true));
    let (expanded, valid) = expand_stacked_pin_notation("");
    assert!(valid);
    assert_eq!(expanded, vec![""]);
}

#[test]
fn string_escape() {
    let test_strings = [
        (EscapeContext::Netname, "", ""),
        (EscapeContext::Netname, "abc", "abc"),
        (EscapeContext::Netname, "a/,b", "a{slash},b"),
        (EscapeContext::Netname, "a{b}c", "a{b}c"),
        (EscapeContext::Netname, "a{bc", "a{bc"),
        (EscapeContext::Netname, "abc}", "abc}"),
    ];

    for (context, input, expected) in test_strings {
        let actual = escape_string(input, context);
        assert_eq!(actual, expected, "Testing EscapeString with input: {input}");
        // Test the round-trip.
        assert_eq!(unescape_string(&actual), input);
    }
}

#[test]
fn regex_escape_keeps_ordinary_characters() {
    assert_eq!(escape_string("DDR3_DQ0", EscapeContext::Regex), "DDR3_DQ0");
    assert_eq!(escape_string("+3.3V", EscapeContext::Regex), "\\+3\\.3V");
}
