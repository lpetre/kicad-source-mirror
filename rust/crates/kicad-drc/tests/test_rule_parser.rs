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

//! The parser-only cases of the C++ QA tests:
//!
//! * `qa/tests/pcbnew/test_libeval_compiler.cpp`: `SingleItemRulePreflight`,
//!   `PairConditionsInSingleItemRules`, `PairItemDependency`;
//! * `qa/tests/pcbnew/drc/test_drc_stub_length.cpp`: `StubLengthAcceptsTimeDomainUnits`,
//!   `ReturnPathDoesNotCarryTimeDomainOption`;
//! * `qa/tests/cli/test_pcb.py::test_pcb_drc_rejects_unavailable_second_item` (the parser part:
//!   the throwing parse the DRC engine does);
//!
//! plus unit tests of `drc_rule.cpp` and of the `DRC_ENGINE::loadRules` text preparation.
//!
//! Conditions are compiled with the stand-in binding of `tests/common` (the PCB binding is
//! package DRC-21); everything these tests check is decided by the parts it ports exactly.

mod common;

use common::{SyntaxBinder, parse_throwing, parse_with_reporter};
use kicad_core::{Lset, PcbLayerId, Severity};
use kicad_drc::constraint_types::drc_constraint_options;
use kicad_drc::engine::prepare_rules_text;
use kicad_drc::pcbexpr::evaluator::{PcbExprBinder, PcbExprUnitResolver, pcbexpr_compiler};
use kicad_drc::reporter::Reporter;
use kicad_drc::rule::{DrcConstraint, DrcConstraintExt, DrcRule};
use kicad_drc::rule_parser::{LayerChoices, default_layer_choices};
use kicad_drc::{DrcConstraintT, is_component_class_selector};
use kicad_libeval::{Context, Ucode};

/// `qa/tests/pcbnew/test_libeval_compiler.cpp`: `SingleItemRulePreflight`.
#[test]
fn single_item_rule_preflight() {
    for constraint in [
        "assertion \"A.Type == 'Graphic'\"",
        "hole_size (min 0.2mm)",
        "text_height (min 1mm)",
        "text_thickness (min 0.1mm)",
        "track_segment_length (min 1mm)",
        "annular_width (min 0.1mm)",
        "solder_mask_expansion (opt 0mm)",
        "solder_paste_abs_margin (opt 0mm)",
        "solder_paste_rel_margin (opt 0mm)",
        "disallow track",
        "via_diameter (min 0.5mm)",
        "length (max 100mm)",
        "net_chain_length (max 100mm)",
        "stub_length (max 1mm)",
        "return_path (layer 'B.Cu')",
        "skew (max 1mm)",
        "via_count (max 2)",
        "via_dangling (max 0)",
        "bridged_mask (min 0)",
        "microvia_stack_depth (max 2)",
        "hole_aspect_ratio (max 1)",
    ] {
        for condition_first in [true, false] {
            let context = format!("{constraint}, condition first: {condition_first}");
            let condition = "(condition \"B.Type == 'Graphic'\")";
            let body = format!("(constraint {constraint})");
            let source = format!(
                "(version 1)\n(rule test\n{}\n)",
                if condition_first {
                    format!("{condition}\n{body}")
                } else {
                    format!("{body}\n{condition}")
                }
            );

            let control_source = source.replace("B.Type", "A.Type");
            let (_, reporter, result) = parse_with_reporter(&control_source, "single item control");
            assert!(result.is_ok(), "{context}");
            assert!(
                !reporter.messages_text().contains("ERROR:"),
                "{context}: {}",
                reporter.messages_text()
            );

            let (_, reporter, result) = parse_with_reporter(&source, "single item preflight");
            assert!(result.is_ok(), "{context}");
            assert!(
                reporter.messages_text().contains("Item 'B'"),
                "{context}: {}",
                reporter.messages_text()
            );

            let (_, result) = parse_throwing(&source, "single item preflight");
            let error = result.expect_err(&context);
            let parse_error = error.as_parse_error().expect("PARSE_ERROR");
            assert!(
                error.what().contains("Item 'B' is not available"),
                "{context}"
            );
            assert_eq!(
                parse_error.line_number,
                if condition_first { 3 } else { 4 },
                "{context}"
            );
        }
    }
}

/// `qa/tests/pcbnew/test_libeval_compiler.cpp`: `PairConditionsInSingleItemRules`.
#[test]
fn pair_conditions_in_single_item_rules() {
    for body in [
        "(condition \"A.intersectsFrontCourtyard(B.Parent)\")\
         (constraint assertion \"A.Type == 'Graphic'\")",
        "(condition \"AB.isCoupledDiffPair()\")(constraint hole_size (min 0.2mm))",
        "(constraint assertion \"B.Type == 'Graphic'\")",
        "(constraint assertion \"A.intersectsFrontCourtyard(B.Parent.getField('Reference'))\")",
        "(condition \"B.Type == 'Graphic'\")(constraint clearance (min 0.2mm))\
         (constraint hole_size (min 0.2mm))",
        "(condition \"AB.isCoupledDiffPair() && B.Type == 'Track'\")\
         (constraint length (max 100mm))",
    ] {
        let (_, result) = parse_throwing(&format!("(version 1)(rule test {body})"), "preflight");
        let error = result.expect_err(body);
        assert!(
            error.what().contains("Item 'B' is not available"),
            "{body}: {}",
            error.what()
        );
    }

    for body in [
        "(condition \"A.Reference == 'B.Width'\")(constraint hole_size (min 0.2mm))",
        "(constraint assertion \"A.Type == 'Graphic'\")",
        "(condition \"A.intersectsFrontCourtyard(B.Parent)\")\
         (constraint physical_clearance (min 100mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint diff_pair_gap (min 0.2mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint track_width (min 0.2mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint diff_pair_uncoupled (max 1mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint length (max 100mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint net_chain_length (max 100mm))",
        "(condition \"AB.isCoupledDiffPair()\")(constraint skew (max 1mm))",
        "(condition \"B.Type == 'Track'\")(constraint track_width (min 0.2mm))",
        "(condition \"B.Type == 'Track'\")(constraint track_angle (min 45))",
        "(condition \"B.Type == 'Zone'\")(constraint thermal_relief_gap (min 0.2mm))",
    ] {
        let (_, reporter, result) =
            parse_with_reporter(&format!("(version 1)(rule test {body})"), "preflight");
        assert!(result.is_ok(), "{body}");
        assert!(
            !reporter.messages_text().contains("ERROR:"),
            "{body}: {}",
            reporter.messages_text()
        );
    }
}

/// `qa/tests/pcbnew/test_libeval_compiler.cpp`: `PairItemDependency`.
#[test]
fn pair_item_dependency() {
    let requires_pair_items = |expr: &str| {
        let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
        let mut ucode: Ucode<SyntaxBinder> = Ucode::new(SyntaxBinder::default());
        let mut preflight: Context<'_, SyntaxBinder> = Context::new(()); // PCBEXPR_CONTEXT preflight( NULL_CONSTRAINT, UNDEFINED_LAYER )

        assert!(compiler.compile(expr, &mut ucode, &mut preflight));
        ucode.binder().requires_pair_items()
    };

    assert!(!requires_pair_items("A.Width == 1mm"));
    assert!(requires_pair_items("B.Width == 1mm"));
    assert!(requires_pair_items("AB.isCoupledDiffPair()"));
    assert!(!requires_pair_items("A.Reference == 'B.Width'"));
}

/// `qa/tests/pcbnew/drc/test_drc_stub_length.cpp`: `StubLengthAcceptsTimeDomainUnits`.
#[test]
fn stub_length_accepts_time_domain_units() {
    // Regression for H-5: stub_length used to be missing from the allowsTimeDomain allow-list
    // in DRC_RULES_PARSER, so a (max 100ps) value would be rejected with "Time based units not
    // allowed for constraint type." The rule must parse cleanly and the resulting constraint
    // must be flagged as TIME_DOMAIN.
    let dru_time = "(version 1)\n\
                    (rule \"StubBudgetTime\"\n    \
                    (condition \"A.NetClass == 'Default'\")\n    \
                    (constraint stub_length (max 100ps))\n\
                    )\n";

    let (rules, reporter, result) = parse_with_reporter(dru_time, "stub_length_time_test");
    assert!(result.is_ok());
    assert!(
        !reporter.has_message_of_severity(Severity::Error as i32),
        "stub_length with ps units must parse without error, got: {}",
        reporter.messages_text()
    );
    assert_eq!(rules.len(), 1);

    let stub = rules[0]
        .find_constraint(DrcConstraintT::NetChainStubLengthConstraint)
        .expect("stub_length constraint");
    assert!(stub.option(drc_constraint_options::TIME_DOMAIN));
    assert!(!stub.option(drc_constraint_options::SPACE_DOMAIN));
    assert!(stub.value.has_max());
    // 1 ps == 1e6 IU (internal time unit is the attosecond).
    assert_eq!(stub.value.max(), 100 * 1000 * 1000);
}

/// `qa/tests/pcbnew/drc/test_drc_stub_length.cpp`: `ReturnPathDoesNotCarryTimeDomainOption`.
#[test]
fn return_path_does_not_carry_time_domain_option() {
    // return_path takes a layer name, not a delay, so the TIME_DOMAIN option must never be set
    // on a return_path constraint regardless of stray (max ...) subtrees.
    let dru_layer = "(version 1)\n\
                     (rule \"GoodReturnPath\"\n    \
                     (condition \"A.NetClass == 'Default'\")\n    \
                     (constraint return_path (layer \"B.Cu\"))\n\
                     )\n";

    let (rules, reporter, result) = parse_with_reporter(dru_layer, "return_path_layer_test");
    assert!(result.is_ok());
    assert!(
        !reporter.has_message_of_severity(Severity::Error as i32),
        "return_path with a layer must parse without error, got: {}",
        reporter.messages_text()
    );
    assert_eq!(rules.len(), 1);

    let rp = rules[0]
        .find_constraint(DrcConstraintT::NetChainReturnPathConstraint)
        .expect("return_path constraint");
    assert!(!rp.option(drc_constraint_options::TIME_DOMAIN));
    assert_eq!(rp.reference_layer, "B.Cu");
}

/// `qa/tests/cli/test_pcb.py::test_pcb_drc_rejects_unavailable_second_item`: the rule file is
/// rejected by the throwing parse of the DRC engine (kicad-cli then exits with code 3).
#[test]
fn rejects_unavailable_second_item() {
    for rule in [
        "(rule test (condition \"A.intersectsFrontCourtyard(B.Parent)\") \
         (constraint assertion \"A.Type == 'Graphic'\"))",
        "(rule test (constraint assertion \"A.intersectsFrontCourtyard(B.Parent)\"))",
    ] {
        let (_, result) = parse_throwing(&format!("(version 1)\n{rule}"), "basic_test.kicad_dru");
        let error = result.expect_err(rule);
        assert!(error.what().contains("Item 'B' is not available"), "{rule}");
    }
}

/// `DRC_ENGINE::loadRules` hands the parser every line followed by an extra `'\n'`, so line
/// numbers double. Reference: kicad-cli 10.99.0 `pcb drc` on a board whose rule file has
/// `(min 1)` on line 4 printed "ERROR: Missing units for '1' (must be mm, in, mil, deg, fs, or
/// ps) in '<path>', line 7, offset 62.".
#[test]
fn load_rules_text_doubles_line_numbers() {
    let file = "(version 1)\n\n(rule a\n  (constraint clearance (min 1)))\n";
    let no_resolve = |_: &mut String| false;
    let text = prepare_rules_text(file, |line| line.to_string(), &no_resolve);

    assert_eq!(
        text,
        "(version 1)\n\n\n\n(rule a\n\n  (constraint clearance (min 1)))\n\n"
    );

    let (_, result) = parse_throwing(&text, "b.kicad_dru");
    assert_eq!(
        result.unwrap_err().what(),
        "ERROR: Missing units for '1' (must be mm, in, mil, deg, fs, or ps) in 'b.kicad_dru', \
         line 7, offset 62."
    );
}

/// The resolver of `loadRules` leaves `${Class:...}` selectors alone and expands others.
#[test]
fn load_rules_text_resolves_variables() {
    let resolve = |token: &mut String| {
        if token == "MIN" {
            *token = "0.3mm".to_string();
            true
        } else {
            false
        }
    };
    let text = prepare_rules_text(
        "(rule a (condition \"A.memberOfFootprint('${Class:Power}')\") (min ${MIN}))",
        |line| line.to_string(),
        &resolve,
    );

    assert_eq!(
        text,
        "(rule a (condition \"A.memberOfFootprint('${Class:Power}')\") (min 0.3mm))\n"
    );
}

/// Layer names are matched against the layer table, including board names when the table
/// holds them (BOARD_LOADER).
#[test]
fn layer_names_from_the_layer_table() {
    let text = "(version 1)\n(rule a (layer \"top_*\") (constraint clearance (min 1mm)))\n";

    let (rules, _, _) = parse_with_reporter(text, "t");
    assert!(rules[0].layer_condition.contains(PcbLayerId::Rescue));

    let mut choices: LayerChoices = default_layer_choices();
    choices.push(("top_layer".to_string(), PcbLayerId::FCu));

    let mut rules = Vec::new();
    let mut parser = common::TestParser::new(text, "t");
    parser.set_layer_choices(choices);
    parser.parse(&mut rules, None).unwrap();

    assert_eq!(rules[0].layer_condition, Lset::from([PcbLayerId::FCu]));
    assert_eq!(rules[0].layer_source, "top_*");
}

/// `drc_rule.cpp`: `IsComponentClassSelector`, `DRC_CONSTRAINT::IsUnary`, `GetName`, `Merge`.
#[test]
fn drc_rule_basics() {
    assert!(is_component_class_selector("Class:Power"));
    assert!(is_component_class_selector("CLASS:"));
    assert!(is_component_class_selector("class:x"));
    assert!(!is_component_class_selector("Classes"));

    let mut unary = DrcConstraint::new(DrcConstraintT::HoleSizeConstraint, "");
    assert!(unary.is_unary());
    unary.type_ = DrcConstraintT::ClearanceConstraint;
    assert!(!unary.is_unary());

    let mut rule = DrcRule::with_name("r1");
    rule.severity = Severity::Warning;
    let mut c = DrcConstraint::new(DrcConstraintT::ClearanceConstraint, "board minimum");
    assert_eq!(c.name(), "board minimum");
    c.value.set_min(100);
    rule.add_constraint(&mut c, None);
    assert_eq!(rule.constraints[0].name(), "rule 'r1'");
    assert_eq!(rule.constraints[0].severity(), Severity::Warning);
    assert!(rule.constraints[0].min_rule.is_some());
    assert!(rule.constraints[0].max_rule.is_none());

    let mut implicit = DrcRule::with_name("netclass 'Default'");
    implicit.set_implicit_source(kicad_drc::DrcImplicitSource::NetClass);
    let mut other = DrcConstraint::new(DrcConstraintT::ClearanceConstraint, "");
    other.value.set_min(200);
    other.value.set_max(5000);
    other.value.set_opt(300);
    implicit.add_constraint(&mut other, None);

    let mut merged = rule.constraints[0].clone();
    merged.merge(&implicit.constraints[0]);
    assert_eq!(merged.value.min(), 200);
    assert_eq!(merged.value.max(), 5000);
    assert_eq!(merged.value.opt(), 300);
    assert_eq!(merged.min_rule.as_ref().unwrap().name, "netclass 'Default'");
    assert_eq!(merged.name(), "netclass 'Default'");

    // A smaller minimum does not override.
    let mut merged2 = implicit.constraints[0].clone();
    merged2.merge(&rule.constraints[0]);
    assert_eq!(merged2.value.min(), 200);
    assert_eq!(
        merged2.min_rule.as_ref().unwrap().name,
        "netclass 'Default'"
    );

    // The C++ constraint sees the rule's final severity.
    let (rules, _, _) = parse_with_reporter(
        "(version 1)(rule a (constraint clearance (min 1mm)) (severity error))",
        "t",
    );
    assert_eq!(rules[0].constraints[0].severity(), Severity::Error);
}
