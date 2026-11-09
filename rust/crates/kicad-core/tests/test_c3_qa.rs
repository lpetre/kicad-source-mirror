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

//! Ported C++ QA tests of work package C3:
//! - `qa/tests/common/test_expand_text_vars.cpp` (suites `ExpandTextVarsTests` and
//!   `NormalizeFilePathForTextVarsTests`; the job, environment-variable and math-evaluator
//!   suites need code outside this crate),
//! - `qa/tests/common/test_nested_settings.cpp` (the in-memory cases; the on-disk cases need the
//!   settings manager and file I/O),
//! - the pattern matching part of `qa/tests/common/test_lib_tree_model_adapter.cpp`
//!   (`SingleLetterSearchMatchesDeviceNameOverKeywordCollision`, reduced to `ScoreTerms`),
//! plus unit tests of the JSON helpers and the net class matcher facts of
//! `rust/docs/analysis/settings.md` section 4.6.

use kicad_core::eda_pattern_match::*;
use kicad_core::json::{self, JsonPointer, ObjectOrder, Value, dump};
use kicad_core::json_settings::{JsonSettings, JsonSettingsInternals, NestedSettings};
use kicad_core::settings_field;
use kicad_core::settings_parameters::ParamT;
use kicad_core::text_vars::*;

// ---- test_expand_text_vars.cpp ----

/// `ExpandTextVarsFixture::resolver`: VAR->value, X->5, Y->2.
fn resolver(token: &mut String) -> bool {
    let v = match token.as_str() {
        "VAR" => "value",
        "X" => "5",
        "Y" => "2",
        _ => return false,
    };

    *token = v.to_string();
    true
}

fn expand(s: &str, ctx: ResolutionContext) -> String {
    expand_text_vars(s, Some(&resolver), ctx)
}

use ResolutionContext::*;

#[test]
fn assertions_remain_available_only_during_rule_checks() {
    for prefix in ["ERC_WARNING", "ERC_ERROR", "DRC_WARNING", "DRC_ERROR"] {
        let assertion = format!("${{{prefix} Check this item}}");
        assert_eq!(expand(&assertion, ForErcDrc), assertion);
        assert!(expand(&assertion, ForCanvas).is_empty());
        assert_eq!(expand(&assertion, RawValue), assertion);
    }
}

#[test]
fn simple_variable() {
    assert_eq!(expand("${VAR}", Internal), "value");
}

#[test]
fn multiple_variables() {
    assert_eq!(expand("${X}+${Y}", Internal), "5+2");
}

#[test]
fn escaped_variable() {
    assert!(expand("\\${VAR}", Internal).contains("<<<ESC_DOLLAR:"));
}

#[test]
fn escaped_then_regular_variable() {
    let r = expand("\\${literal}${VAR}", Internal);
    assert!(r.contains("<<<ESC_DOLLAR:"));
    assert!(r.contains("value"));
}

#[test]
fn regular_then_escaped_variable() {
    let r = expand("${VAR}\\${literal}", Internal);
    assert!(r.starts_with("value"));
    assert!(r.contains("<<<ESC_DOLLAR:"));
}

#[test]
fn escaped_inside_math_expression() {
    let r = expand("@{\\${X}+${Y}}", Internal);
    assert!(r.contains("<<<ESC_DOLLAR:"), "Expected escape marker in result");
    assert!(r.contains("+2"), "Expected '+2' (from ${{Y}} expansion) in result");
    assert_eq!(r, "@{<<<ESC_DOLLAR:X}+2}");
}

#[test]
fn escaped_inside_variable_reference() {
    assert!(expand("${prefix\\${suffix}}", Internal).contains("<<<ESC_DOLLAR:"));
}

#[test]
fn multiple_escaped_in_math_expression() {
    let r = expand("@{\\${A}+\\${B}+${Y}}", Internal);
    assert!(r.contains("+2"), "Expected '+2' (from ${{Y}} expansion) in: {r}");
    assert_eq!(r.matches("<<<ESC_DOLLAR:").count(), 2);
}

#[test]
fn escaped_at_in_expression() {
    let r = expand("${VAR}\\@{literal}", Internal);
    assert!(r.starts_with("value"));
    assert!(r.contains("<<<ESC_AT:"));
}

#[test]
fn consecutive_escaped() {
    assert_eq!(expand("\\${A}\\${B}", Internal).matches("<<<ESC_DOLLAR:").count(), 2);
}

#[test]
fn backslash_before_variable_is_escape() {
    assert!(expand("subdir\\${VAR}_file.txt", Internal).contains("<<<ESC_DOLLAR:"));
}

#[test]
fn forward_slash_before_variable_expands() {
    assert_eq!(expand("subdir/${VAR}_file.txt", Internal), "subdir/value_file.txt");
}

#[test]
fn normalize_backslash_separator_before_var_expands() {
    let path = normalize_file_path_for_text_vars("Output\\BoM\\${VAR}_file.csv");
    let r = expand(&path, Internal);
    assert!(!r.contains("<<<ESC_DOLLAR:"), "Variable after backslash separator should expand, not escape. Got: {r}");
    assert_eq!(r, "Output\\BoM/value_file.csv");
}

#[test]
fn normalize_multiple_vars_after_backslash_separators() {
    let path = normalize_file_path_for_text_vars("Output\\BoM\\${X}_V${Y}.csv");
    assert_eq!(expand(&path, Internal), "Output\\BoM/5_V2.csv");
}

#[test]
fn normalize_forward_slash_path_unchanged() {
    let path = normalize_file_path_for_text_vars("Output/BoM/${VAR}.csv");
    assert_eq!(expand(&path, Internal), "Output/BoM/value.csv");
}

#[test]
fn normalize_non_variable_backslashes_are_preserved() {
    assert_eq!(normalize_file_path_for_text_vars("Output\\BoM\\literal.csv"), "Output\\BoM\\literal.csv");
}

#[test]
fn finalize_restores_escapes_per_context() {
    let mut a = expand("\\${VAR} ${VAR}", Internal);
    let mut b = a.clone();
    finalize_text_var_expansion(&mut a, ForCanvas);
    finalize_text_var_expansion(&mut b, Internal);
    assert_eq!(a, "${VAR} value");
    assert_eq!(b, "\\${VAR} value");
}

#[test]
fn resolve_text_vars_uses_the_evaluator_when_given() {
    let eval = |s: &str| s.replace("@{5-1}", "4");
    let opts = TextVarOptions { max_depth: 6, evaluator: Some(&eval) };
    let mut depth = 0;
    assert_eq!(resolve_text_vars("@{${X}-1}", Some(&resolver), &mut depth, &opts), "4");
    assert_eq!(depth, 1);
}

// ---- test_nested_settings.cpp ----

/// `TEST_NESTED_SETTINGS`: `test_value` (default 0) and `extra_value` (default 42).
struct NestedValues {
    test_value: i32,
    extra_value: i32,
}

fn test_nested(parent: &JsonSettingsInternals, reset: bool) -> (NestedSettings<NestedValues>, NestedValues) {
    let mut n = NestedSettings::new(0, "test_nested");
    n.settings.reset_params_if_missing = reset;
    n.settings.add_param(Box::new(ParamT::new("test_value", settings_field!(NestedValues, test_value), 0)));
    n.settings.add_param(Box::new(ParamT::new("extra_value", settings_field!(NestedValues, extra_value), 42)));
    let mut v = NestedValues { test_value: 0, extra_value: 42 };
    n.load_from_parent(Some(parent), &mut v);
    (n, v)
}

fn parent_with(nested: &str) -> JsonSettingsInternals {
    let mut p = JsonSettingsInternals::new(Value::Object(json::Map::new()));
    *JsonPointer::parse("/test_nested").unwrap().get_unchecked(&mut p.doc).unwrap() =
        json::parse(nested, ObjectOrder::Sorted).unwrap();
    p
}

#[test]
fn release_does_not_mark_parent_modified() {
    // https://gitlab.com/kicad/code/kicad/-/issues/22972, #24402
    let mut parent = parent_with(r#"{"meta":{"version":0},"test_value":5}"#);
    let (mut nested, v) = test_nested(&parent, true);

    assert_eq!(v.test_value, 5);
    assert_eq!(v.extra_value, 42);

    // ReleaseNestedSettings: m_modified |= nested->SaveToFile()
    let parent_modified = nested.save_to_parent(&mut parent, &v, false);
    assert!(
        !parent_modified,
        "Parent should not be marked modified after releasing nested settings when no intentional changes were made"
    );
}

#[test]
fn explicit_save_detects_modifications() {
    let mut parent = parent_with(r#"{"meta":{"version":0},"test_value":5,"extra_value":42}"#);
    let (mut nested, mut v) = test_nested(&parent, true);
    assert_eq!(v.test_value, 5);

    v.test_value = 99;
    assert!(nested.save_to_parent(&mut parent, &v, false));
    assert_eq!(parent.get::<i32>("test_nested.test_value").unwrap(), Some(99));
}

#[test]
fn release_flushes_data_to_parent() {
    let mut parent = parent_with(r#"{"meta":{"version":0},"test_value":5}"#);
    let (mut nested, mut v) = test_nested(&parent, true);
    v.test_value = 77;
    nested.save_to_parent(&mut parent, &v, false);
    assert_eq!(parent.get::<i32>("test_nested.test_value").unwrap(), Some(77));
}

#[test]
fn absent_default_is_modified_when_not_reset_if_missing() {
    let mut parent = parent_with(r#"{"meta":{"version":0},"test_value":5}"#);
    let (mut nested, v) = test_nested(&parent, false);

    assert_eq!(v.extra_value, 42);
    // The absent default must be reported as a change so it is persisted.
    assert!(nested.save_to_parent(&mut parent, &v, false));
    assert_eq!(parent.get::<i32>("test_nested.extra_value").unwrap(), Some(42));
}

#[test]
fn migration_flushes_to_parent() {
    let mut parent = parent_with(r#"{"meta":{"version":0}}"#);
    let mut nested: NestedSettings<()> = NestedSettings::new(1, "test_nested");
    nested.settings.register_migration(
        0,
        1,
        Box::new(|int: &mut JsonSettingsInternals| {
            int.set("migrated_field", &7)?;
            Ok(true)
        }),
    );
    nested.load_from_parent(Some(&parent), &mut ());

    assert!(nested.save_to_parent(&mut parent, &(), false));
    assert_eq!(parent.get::<i32>("test_nested.meta.version").unwrap(), Some(1));
    assert_eq!(parent.get::<i32>("test_nested.migrated_field").unwrap(), Some(7));
}

#[test]
fn settings_load_store_round_trip() {
    struct S {
        a: i32,
        name: String,
    }

    let mut js: JsonSettings<S> = JsonSettings::new(1, Some("x.json"));
    js.add_param(Box::new(ParamT::with_range("a.b", settings_field!(S, a), 3, 0, 10)));
    js.add_param(Box::new(ParamT::new("name", settings_field!(S, name), "n".to_string())));
    let mut s = S { a: 0, name: String::new() };

    assert!(js.load_from_buffer(b"{\"meta\":{\"version\":1},\"a\":{\"b\":11},\"name\":\"x\"}", &mut s));
    assert_eq!((s.a, s.name.as_str()), (3, "x"));
    s.a = 4;
    assert_eq!(
        js.format_as_string(&s),
        "{\n  \"a\": {\n    \"b\": 4\n  },\n  \"meta\": {\n    \"version\": 1\n  },\n  \"name\": \"x\"\n}\n"
    );
}

// ---- JSON helpers ----

/// Grisu2 is not always shortest: nlohmann prints 1e23 as 9.999999999999999e+22.
#[test]
fn nlohmann_dump_forms() {
    let v = json::parse(r#"{"b":[],"a":{},"c":[1,2.0,-0.0,1e23,"\u0001\"\\/é"],"d":null}"#, ObjectOrder::Insertion).unwrap();
    assert_eq!(dump(&v, None, ObjectOrder::Sorted), r#"{"a":{},"b":[],"c":[1,2.0,-0.0,9.999999999999999e+22,"\u0001\"\\/é"],"d":null}"#);
    assert_eq!(dump(&v, None, ObjectOrder::Insertion), r#"{"b":[],"a":{},"c":[1,2.0,-0.0,9.999999999999999e+22,"\u0001\"\\/é"],"d":null}"#);
    assert_eq!(dump(&json::parse("[1,[]]", ObjectOrder::Sorted).unwrap(), Some(2), ObjectOrder::Sorted), "[\n  1,\n  []\n]");
    assert_eq!(json::dump_float(1e-5), "1e-05");
    assert_eq!(json::dump_float(f64::NAN), "null");
}

#[test]
fn nlohmann_parse_rules() {
    // "-0" is the integer 0 (dumped as 0), "-0.0" stays a float.
    assert_eq!(dump(&json::parse("[-0,-0.0]", ObjectOrder::Sorted).unwrap(), None, ObjectOrder::Sorted), "[0,-0.0]");
    // Comments and a BOM are accepted (ignore_comments), trailing garbage is not.
    assert!(json::parse_bytes(b"\xEF\xBB\xBF/* x */ {\"a\":1} // y", ObjectOrder::Sorted, true).is_ok());
    assert!(json::parse_bytes(b"{\"a\":1} // y", ObjectOrder::Sorted, false).is_err());
    assert!(json::parse("{\"a\":1} x", ObjectOrder::Sorted).is_err());
    assert!(json::parse("1e400", ObjectOrder::Sorted).is_err());
    // Duplicate keys: the last value wins.
    assert_eq!(dump(&json::parse(r#"{"a":1,"a":2}"#, ObjectOrder::Sorted).unwrap(), None, ObjectOrder::Sorted), r#"{"a":2}"#);
    assert!(json::parse_bytes(b"\"\xC0\xAF\"", ObjectOrder::Sorted, true).is_err());
    assert!(json::parse("\"\\ud800\"", ObjectOrder::Sorted).is_err());
}

// ---- net class pattern facts (rust/docs/analysis/settings.md 4.6) ----

fn netclass(pattern: &str) -> EdaCombinedMatcher {
    EdaCombinedMatcher::new(pattern, CombinedMatcherContext::Netclass)
}

#[test]
fn netclass_pattern_facts() {
    // "+3V3" is not a valid PCRE2 regex: only the wildcard matcher exists, matching literally.
    let m = netclass("+3V3");
    assert_eq!(m.matcher_count(), 1);
    assert!(m.starts_with("+3V3"));
    assert!(!m.starts_with("3V3"));

    // As a regex, Net-(D1-A) is a group and also matches Net-D1-A.
    let m = netclass("Net-(D1-A)");
    assert!(m.starts_with("Net-(D1-A)"));
    assert!(m.starts_with("Net-D1-A"));

    // Only the outer alternatives are anchored.
    let m = netclass("/MOSI|/MISO|/CLK");
    assert!(m.starts_with("/MISO_2"));
    assert!(!m.starts_with("x/MISO"));

    // Braces that are not a quantifier are literal.
    assert!(netclass("/SIG_50_{OPEN}").starts_with("/SIG_50_{OPEN}"));

    // `$` also matches before a final newline (PCRE2), the wildcard matcher too.
    assert!(netclass("GND").starts_with("GND\n"));

    // Wildcards.
    let m = netclass("/bus*");
    assert!(m.starts_with("/bus0"));
    assert!(!m.starts_with("x/bus0"));
    assert!(netclass("USB_D?").starts_with("USB_DP"));
}

#[test]
fn relational_matcher() {
    let mut m = EdaPatternMatchRelational::default();
    assert!(m.set_pattern("r<10k"));
    assert_eq!(m.find("R1 r=4.7k tol=5%").start, 3);
    assert!(!m.find("r=22k").valid());
    assert!(!m.set_pattern("r<10q"));
}

// ---- test_lib_tree_model_adapter.cpp (reduced) ----

#[test]
fn single_letter_search_matches_device_name_over_keyword_collision() {
    // (library, name, keywords, description) as in the C++ test.
    let items = [
        ("Device", "R", "R res resistor", "Resistor"),
        ("Device", "L", "inductor choke coil reactor magnetic", "Inductor"),
        ("Device", "C", "cap capacitor", "Unpolarized capacitor"),
        ("Device", "D", "diode", "Diode"),
        ("Potentiometer_Digital", "AD5293", "R POT", "Digital potentiometer 1024 pos (SPI), TSSOP-14"),
        (
            "Interface_USB",
            "AP33771",
            "USB Type C PD Sink Converter Controller CMOS Clock capacitor charger coupled",
            "USB Type-C PD Sink Controller, QFN-24",
        ),
        ("74xGxx", "74AUC1G74", "Single D Flip-Flop D CMOS", "Single D Flip-Flop, Low-Voltage CMOS"),
    ];

    for letter in ["r", "l", "c", "d"] {
        let matcher = EdaCombinedMatcher::new(letter, CombinedMatcherContext::LibItem);

        for (lib, name, keywords, desc) in items {
            // TEST_LIB_TREE_ITEM::GetSearchTerms
            let mut terms = vec![
                SearchTerm::new(lib, 4, false),
                SearchTerm::new(name, 8, true),
                SearchTerm::new(&format!("{lib}:{name}"), 16, true),
            ];
            terms.extend(keywords.split([' ', '\t', '\r', '\n']).filter(|t| !t.is_empty()).map(|t| SearchTerm::new(t, 4, false)));
            terms.push(SearchTerm::new(keywords, 1, false));
            terms.push(SearchTerm::new(desc, 1, false));

            let mut exact = false;
            let score = matcher.score_terms(&mut terms, Some(&mut exact));
            let expected = lib == "Device" && name == letter.to_uppercase();

            assert!(score > 0 || !expected);
            assert_eq!(exact, expected, "search letter '{letter}', item {lib}:{name}");
        }
    }
}
