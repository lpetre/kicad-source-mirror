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

//! Port of `qa/tests/common/test_lib_id.cpp` (Boost test suite `LibId`).

use kicad_core::lib_id::{LibId, get_lib_filename_forbidden_chars, get_lib_id_forbidden_chars};

#[test]
fn parse_fully_qualified() {
    let mut id = LibId::new();
    assert!(id.parse("Package_SO:DGG56", false) == -1);
    assert_eq!(id.lib_nickname(), "Package_SO");
    assert_eq!(id.lib_item_name(), "DGG56");
    assert!(id.is_valid());
    assert!(!id.is_legacy());
    assert!(!id.is_empty());
}

#[test]
fn parse_legacy() {
    let mut id = LibId::new();
    assert!(id.parse("DGG56", false) == -1);
    assert!(id.lib_nickname().is_empty());
    assert_eq!(id.lib_item_name(), "DGG56");
    assert!(!id.is_valid());
    assert!(id.is_legacy());
    assert!(!id.is_empty());
}

#[test]
fn equality_fully_qualified() {
    let a = LibId::from_parts("Package_SO", "DGG56");
    let b = LibId::from_parts("Package_SO", "DGG56");
    let c = LibId::from_parts("OtherLib", "DGG56");

    assert!(a == b);
    assert!(a != c);
}

#[test]
fn equality_legacy_vs_fully_qualified() {
    let mut legacy = LibId::new();
    legacy.parse("DGG56", false);

    let qualified = LibId::from_parts("Package_SO", "DGG56");

    // Standard equality requires both library and item name to match
    assert!(legacy != qualified);

    // Legacy matching (item name only) should match when we explicitly compare just item names
    assert!(legacy.is_legacy());
    assert_eq!(legacy.lib_item_name(), qualified.lib_item_name());
}

#[test]
fn format_round_trip() {
    let qualified = LibId::from_parts("Package_SO", "DGG56");
    assert_eq!(qualified.format(), "Package_SO:DGG56");

    let mut legacy = LibId::new();
    legacy.parse("DGG56", false);
    assert_eq!(legacy.format(), "DGG56");

    // Re-parse the formatted strings
    let mut reparsed = LibId::new();
    reparsed.parse(&qualified.format(), false);
    assert!(reparsed == qualified);

    let mut reparsed_legacy = LibId::new();
    reparsed_legacy.parse(&legacy.format(), false);
    assert!(reparsed_legacy.is_legacy());
    assert_eq!(reparsed_legacy.lib_item_name(), legacy.lib_item_name());
}

#[test]
fn empty_id() {
    let empty = LibId::new();
    assert!(empty.is_empty());
    assert!(!empty.is_valid());
    assert!(!empty.is_legacy());
}

/// The shared LIB_ID forbidden-character list must actually drive LIB_ID validation.
/// Emptying or corrupting the list breaks these checks (revert guard for the dedup).
#[test]
fn shared_forbidden_list_drives_lib_id_validation() {
    let lib_id_bad = get_lib_id_forbidden_chars();

    assert!(!lib_id_bad.is_empty());

    // Pin the exact sets so a silently dropped character is caught; the membership loops below
    // only prove that whatever is in the list is rejected, not that the list stayed complete.
    assert!(lib_id_bad == "<>\"\\:\t\n\r");
    assert!(get_lib_filename_forbidden_chars() == "<>\"\\:\t\n\r%$/");

    // Known members that must remain forbidden regardless of refactoring.
    assert!(lib_id_bad.contains(':'));
    assert!(lib_id_bad.contains('"'));

    for ch in lib_id_bad.chars() {
        let mut name = String::from("abc");
        name.push(ch);

        assert!(
            LibId::has_illegal_chars(&name) >= 0,
            "LIB_ID accepted a shared-forbidden char 0x{:x}",
            u32::from(ch)
        );
    }
}

/// Filename rules are a strict superset of LIB_ID rules. This pins the deliberate
/// asymmetry the old TODOs warned about: '%', '$' and '/' are legal in a LIB_ID item
/// name but illegal in a library filename.
#[test]
fn filename_rules_superset_of_lib_id() {
    let lib_id = get_lib_id_forbidden_chars();
    let fname = get_lib_filename_forbidden_chars();

    for ch in lib_id.chars() {
        assert!(fname.contains(ch));
    }

    for ch in ['%', '$', '/'] {
        assert!(!lib_id.contains(ch));
        assert!(fname.contains(ch));

        let mut name = String::from("abc");
        name.push(ch);

        assert_eq!(LibId::has_illegal_chars(&name), -1);
    }
}
