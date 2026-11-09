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

//! Port of `qa/tests/pcbnew/test_libeval_compiler.cpp` (test suite `Libeval_Compiler`).
//!
//! The C++ suite drives LIBEVAL through pcbnew's binding (`PCBEXPR_COMPILER`,
//! `PCBEXPR_UCODE`, `PCBEXPR_CONTEXT`).  That binding is ported in `kicad-drc`, on top of
//! this crate, so only the cases that need no board items run here: `SimpleExpressions`,
//! with the same unit resolver behaviour pcbnew provides (`tests/common`:
//! `PcbExprUnitResolver`).  Every other case is kept with its complete data as an
//! `#[ignore = "needs PCB binding (kicad-drc)"]` test, to be moved to `kicad-drc` and
//! enabled once the binding exists; each of them documents the board set-up the C++ test
//! performs.
//!
//! The cases of `qa/tools/libeval_compiler/libeval_compiler_test.cpp` (a manual test tool)
//! are a subset of this suite's expressions with older property names; the only expression
//! unique to it is kept in `tool_from_to_expression`.

mod common;

use common::PcbExprUnitResolver;
use kicad_libeval::{Compiler, Context, DefaultBinder, Ucode, Value, VarType};

/// `LIBEVAL::VALUE` as used for expected results (`using VAL = LIBEVAL::VALUE;`).
#[derive(Clone, Copy, Debug)]
enum Val {
    /// `VAL( double )`
    Num(f64),
    /// `VAL( const char* )`
    Str(&'static str),
}

/// `struct EXPR_TO_TEST`
#[derive(Clone, Copy, Debug)]
struct ExprToTest {
    expression: &'static str,
    expect_error: bool,
    expected_result: Val,
}

const fn e(expression: &'static str, expect_error: bool, expected_result: Val) -> ExprToTest {
    ExprToTest {
        expression,
        expect_error,
        expected_result,
    }
}

/// `pcbIUScale.MilsToIU( mils )`
const fn mils_to_iu(mils: i32) -> f64 {
    (mils * 25400) as f64
}

/// `pcbIUScale.mmToIU( mm )` for whole millimetres
const fn mm_to_iu(mm: i32) -> f64 {
    (mm * 1_000_000) as f64
}

/// `simpleExpressions`
const SIMPLE_EXPRESSIONS: &[ExprToTest] = &[
    e("10mm + 20 mm", false, Val::Num(30e6)),
    e("3*(7+8)", false, Val::Num((3 * (7 + 8)) as f64)),
    e("3*7+8", false, Val::Num((3 * 7 + 8) as f64)),
    e("(3*7)+8", false, Val::Num((3 * 7 + 8) as f64)),
    e("10mm + 20)", true, Val::Num(0.0)),
    e("1", false, Val::Num(1.0)),
    e("1.5", false, Val::Num(1.5)),
    e("1,5", false, Val::Num(1.5)),
    e("1mm", false, Val::Num(1e6)),
    // Any White-space is OK
    e("   1 +     2    ", false, Val::Num(3.0)),
    // Decimals are OK in expressions
    e("1.5 + 0.2 + 0.1", false, Val::Num(1.8)),
    // Negatives are OK
    e("3 - 10", false, Val::Num(-7.0)),
    // Lots of operands
    e("1 + 2 + 10 + 1000.05", false, Val::Num(1013.05)),
    // Operator precedence
    e("1 + 2 - 4 * 20 / 2", false, Val::Num(-37.0)),
    // Parens
    e("(1)", false, Val::Num(1.0)),
    // Parens affect precedence
    e("-(1 + (2 - 4)) * 20.8 / 2", false, Val::Num(10.4)),
    // Unary addition is a sign, not a leading operator
    e("+2 - 1", false, Val::Num(1.0)),
    // A short-circuited || must yield a normalized 1, not the raw (nonzero) left operand, so a
    // boolean feeding a further operator behaves the same as the non-short-circuited path.
    e("(2 || 0) == 1", false, Val::Num(1.0)),
    e("(7 || 0) + 5", false, Val::Num(6.0)),
];

/// `introspectionExpressions`
const INTROSPECTION_EXPRESSIONS: &[ExprToTest] = &[
    e(
        "A.type == 'Pad' && B.type == 'Pad' && (A.existsOnLayer('F.Cu'))",
        false,
        Val::Num(0.0),
    ),
    e("A.Width > B.Width", false, Val::Num(0.0)),
    e(
        "A.Width + B.Width",
        false,
        Val::Num(mils_to_iu(10) + mils_to_iu(20)),
    ),
    e("A.Netclass", false, Val::Str("HV_LINE")),
    e("A.Net_Class == 'HV_LINE'", false, Val::Num(1.0)),
    e(
        "(A.Netclass == 'HV_LINE') && (B.netclass == 'otherClass') && (B.netclass != 'F.Cu')",
        false,
        Val::Num(1.0),
    ),
    e("A.Netclass + 1.0", false, Val::Num(1.0)),
    e("A.hasNetclass('HV_LINE')", false, Val::Num(1.0)),
    e("A.hasNetclass('HV_*')", false, Val::Num(1.0)),
    e("A.existsOnLayer(B.Layer)", false, Val::Num(1.0)),
    e(
        "A.type == 'Track' && B.type == 'Track' && A.layer == 'F.Cu'",
        false,
        Val::Num(1.0),
    ),
    e(
        "(A.type == 'Track') && (B.type == 'Track') && (A.layer == 'F.Cu')",
        false,
        Val::Num(1.0),
    ),
    e("A.type == 'Via' && A.isMicroVia()", false, Val::Num(0.0)),
];

/// Port of the static `testEvalExpr` for the cases without board items.
///
/// The C++ helper compiles with `PCBEXPR_COMPILER( new PCBEXPR_UNIT_RESOLVER() )` into a
/// `PCBEXPR_UCODE`; without items only the unit resolver matters, so the plain
/// `LIBEVAL::UCODE` behaviour ([`DefaultBinder`]) is used here.
fn test_eval_expr(expr: &str, expected_result: Val, expect_error: bool) -> bool {
    let mut compiler = Compiler::with_unit_resolver(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode = Ucode::new(DefaultBinder);
    let mut context = Context::new(());
    let mut preflight_context = Context::new(());

    let error = !compiler.compile(expr, &mut ucode, &mut preflight_context);

    assert_eq!(error, expect_error, "Expr: '{expr}'");

    if error {
        return true;
    }

    let expected: Value<'_, DefaultBinder> = match expected_result {
        Val::Num(v) => Value::from_double(v),
        Val::Str(s) => Value::from_string(s, false),
    };

    let result = ucode.run(&mut context);
    let ok = context.value(result).equal_to(&context, &expected);

    if expected.type_() == VarType::Numeric {
        assert_eq!(
            context.value(result).as_double(&context),
            expected.as_double(&context),
            "Expr: '{expr}'"
        );
    } else {
        assert_eq!(
            context.value(result).as_string(&context),
            expected.as_string(&context),
            "Expr: '{expr}'"
        );
    }

    ok
}

/// Stand-in for the C++ helpers that need pcbnew's binding (`testEvalExpr` with items,
/// `expectCompileError`, `expectCompileSuccess`).  The tests calling it are `#[ignore]`d.
fn needs_pcb_binding(what: &str) {
    panic!("needs PCB binding (kicad-drc): {what}");
}

/// `testEvalExpr( expr, expectedResult, expectError, itemA, itemB )`
fn test_eval_expr_with_items(expr: &str, _expected: Val, _expect_error: bool, items: &str) {
    needs_pcb_binding(&format!("testEvalExpr( \"{expr}\" ) with {items}"));
}

/// `expectCompileError( aExpr )`: compile with `PCBEXPR_COMPILER` / `PCBEXPR_UCODE` and check
/// `compiler.IsErrorPending()`.
fn expect_compile_error(expr: &str) {
    needs_pcb_binding(&format!("expectCompileError( \"{expr}\" )"));
}

/// `expectCompileSuccess( aExpr )`: compile with `PCBEXPR_COMPILER` / `PCBEXPR_UCODE` and
/// check `!compiler.IsErrorPending()`.
fn expect_compile_success(expr: &str) {
    needs_pcb_binding(&format!("expectCompileSuccess( \"{expr}\" )"));
}

/// `BOOST_DATA_TEST_CASE( SimpleExpressions, ... )`
#[test]
fn simple_expressions() {
    for expr in SIMPLE_EXPRESSIONS {
        test_eval_expr(expr.expression, expr.expected_result, expr.expect_error);
    }
}

/// The C++ data test case ignores the value `testEvalExpr` returns; this checks it too.
#[test]
fn simple_expressions_equal_to() {
    for expr in SIMPLE_EXPRESSIONS {
        assert!(
            test_eval_expr(expr.expression, expr.expected_result, expr.expect_error),
            "Expr: '{}'",
            expr.expression
        );
    }
}

/// `BOOST_AUTO_TEST_CASE( IntrospectedProperties )`
///
/// Board set-up: two nets `net1` (code 1, netclass `HV_LINE`) and `net2` (code 2, netclass
/// `otherClass`); `PCB_TRACK trackA` on `net1` with width 10 mil; `PCB_TRACK trackB` on
/// `net2`, layer `F_Cu`, width 20 mil.  Every expression is evaluated with A = trackA,
/// B = trackB.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn introspected_properties() {
    for expr in INTROSPECTION_EXPRESSIONS {
        test_eval_expr_with_items(
            expr.expression,
            expr.expected_result,
            expr.expect_error,
            "A = trackA, B = trackB",
        );
    }
}

/// `BOOST_AUTO_TEST_CASE( IntrospectedExtendedNumericProperties )`
///
/// Board set-up: `ZONE zone` on `F_Cu` with assigned priority 7, hatch orientation 45
/// degrees and minimum island area 123456789; `PCB_SHAPE ellipse` (`SHAPE_T::ELLIPSE`) with
/// ellipse rotation 30 degrees; `FOOTPRINT footprint` with local solder paste margin ratio
/// 0.125, then `std::nullopt` for the last expression.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn introspected_extended_numeric_properties() {
    let one = Val::Num(1.0);

    test_eval_expr_with_items("A.Priority == 7", one, false, "A = zone");
    test_eval_expr_with_items("A.Hatch_Orientation == 45deg", one, false, "A = zone");
    test_eval_expr_with_items("A.Minimum_Island_Area == 123456789", one, false, "A = zone");

    test_eval_expr_with_items("A.Ellipse_Rotation == 30deg", one, false, "A = ellipse");

    test_eval_expr_with_items(
        "A.Solderpaste_Margin_Ratio_Override == 0.125",
        one,
        false,
        "A = footprint (ratio 0.125)",
    );

    test_eval_expr_with_items(
        "A.Solderpaste_Margin_Ratio_Override == null",
        one,
        false,
        "A = footprint (ratio unset)",
    );
}

/// `BOOST_AUTO_TEST_CASE( RenamedProperties )`
///
/// Board set-up: `PCB_TRACK track` from (1 mm, 2 mm) to (3 mm, 4 mm).
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn renamed_properties() {
    let _start = (mm_to_iu(1), mm_to_iu(2));
    let _end = (mm_to_iu(3), mm_to_iu(4));
    let one = Val::Num(1.0);

    expect_compile_success("A.Origin_X == 1mm");
    expect_compile_success("A.Origin_Y == 2mm");

    test_eval_expr_with_items("A.Origin_X == 1mm", one, false, "A = track");
    test_eval_expr_with_items("A.Origin_Y == 2mm", one, false, "A = track");
    test_eval_expr_with_items("A.Origin_X != A.End_X", one, false, "A = track");
    test_eval_expr_with_items("A.origin_y != A.End_Y", one, false, "A = track");

    test_eval_expr_with_items("A.Start_X == 1mm", one, false, "A = track");
    test_eval_expr_with_items("A.Start_Y == 2mm", one, false, "A = track");
}

/// `BOOST_AUTO_TEST_CASE( IntrospectedColorProperties )`
///
/// Board set-up: `PCB_TABLE table`; for each colour below `table.SetBorderColor( colour )`,
/// `COLOR4D::ToCSSString()` must give the listed text, and
/// `A.Border_Color == '<css>'` must be 1 with A = table.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn introspected_color_properties() {
    // ( COLOR4D( r, g, b, a ), expected CSS string )
    let colors: [((f64, f64, f64, f64), &str); 2] = [
        ((0.25, 0.5, 0.75, 0.5), "rgba(64, 128, 191, 0.502)"),
        ((1.0, 0.0, 0.0, 1.0), "rgb(255, 0, 0)"),
    ];

    for (_color, expected_css) in colors {
        let expression = format!("A.Border_Color == '{expected_css}'");
        test_eval_expr_with_items(&expression, Val::Num(1.0), false, "A = table");
    }
}

/// `BOOST_AUTO_TEST_CASE( ExpressionPropertyTypesAreSupported )`
///
/// For every property of every class derived from `BOARD_ITEM`,
/// `PCBEXPR_VAR_REF::ClassifyProperty` must not be `UNSUPPORTED`, and all properties sharing a
/// name must map to one expression type -- except `Shape`, which is numeric on `PCB_TARGET`
/// and an enum on `EDA_SHAPE` (exactly 2 types), so that `A.Shape` is a compile error.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn expression_property_types_are_supported() {
    expect_compile_error("A.Shape");
}

/// `BOOST_AUTO_TEST_CASE( IntrospectedPropertyAvailability )`
///
/// Board set-up: `ZONE zone` on `F_Cu` with assigned priority 7; first as a rule area
/// (`SetIsRuleArea( true )`: the property is not available, the comparison is 0), then as a
/// normal zone (1).
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn introspected_property_availability() {
    test_eval_expr_with_items(
        "A.Priority == 7",
        Val::Num(0.0),
        false,
        "A = zone (rule area)",
    );
    test_eval_expr_with_items(
        "A.Priority == 7",
        Val::Num(1.0),
        false,
        "A = zone (copper zone)",
    );
}

/// `BOOST_AUTO_TEST_CASE( InNetChainClassWildcard )`
///
/// Board set-up: net settings map net chain `ChainHS` to net chain class `HighSpeed`; nets
/// `netA` (code 1, chain `ChainOrphan`), `netB` (code 2, chain `ChainHS`), `netC` (code 3, no
/// chain); one `PCB_TRACK` on each: trackUnclassified, trackClassified, trackNoChain.  A and B
/// are both the named track.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn in_net_chain_class_wildcard() {
    let zero = Val::Num(0.0);
    let one = Val::Num(1.0);

    // A chain with no class assignment must not match any inNetChainClass() pattern,
    // including the '*' wildcard.
    test_eval_expr_with_items("A.inNetChainClass('*')", zero, false, "trackUnclassified");
    test_eval_expr_with_items(
        "A.inNetChainClass('HighSpeed')",
        zero,
        false,
        "trackUnclassified",
    );

    // Net with no chain at all must not match either.
    test_eval_expr_with_items("A.inNetChainClass('*')", zero, false, "trackNoChain");

    // Properly classified chain must match both wildcard and exact patterns.
    test_eval_expr_with_items("A.inNetChainClass('*')", one, false, "trackClassified");
    test_eval_expr_with_items(
        "A.inNetChainClass('HighSpeed')",
        one,
        false,
        "trackClassified",
    );
    test_eval_expr_with_items("A.inNetChainClass('High*')", one, false, "trackClassified");
    test_eval_expr_with_items(
        "A.inNetChainClass('LowSpeed')",
        zero,
        false,
        "trackClassified",
    );
}

/// `BOOST_AUTO_TEST_CASE( StackedMicroviaExpression )`
///
/// Board set-up: 4 copper layers.  Microvias (diameter 0.25 mm, drill 0.1 mm) `upperHop`
/// at the origin `F_Cu`-`In1_Cu`, `lowerHop` at the origin `In1_Cu`-`In2_Cu`, `lone` at
/// (10 mm, 0) `F_Cu`-`In1_Cu`; a through via `through` at the origin `F_Cu`-`B_Cu` (diameter
/// 0.6 mm, drill 0.3 mm).  For the last two checks a microvia `landing` at (10 mm, 0)
/// `In1_Cu`-`In2_Cu` is added and `BOARD::IncrementTimeStamp()` is called.  A and B are both
/// the named via.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn stacked_microvia_expression() {
    let zero = Val::Num(0.0);
    let one = Val::Num(1.0);

    // Both hops of a stack are in it, not just the one on top.
    test_eval_expr_with_items("A.isStackedVia()", one, false, "upperHop");
    test_eval_expr_with_items("A.isStackedVia()", one, false, "lowerHop");

    // A microvia landing on nothing is not a stack.
    test_eval_expr_with_items("A.isStackedVia()", zero, false, "lone");

    // A through via sharing the position of a stack is not part of it.
    test_eval_expr_with_items("A.isStackedVia()", zero, false, "through");

    // The predicate composes with the rest of the language.
    test_eval_expr_with_items("A.isMicroVia() && !A.isStackedVia()", one, false, "lone");

    // The relation is cached for the whole board, so an edit has to drop it.
    test_eval_expr_with_items(
        "A.isStackedVia()",
        one,
        false,
        "lone (after adding landing)",
    );
    test_eval_expr_with_items("A.isStackedVia()", one, false, "landing");
}

/// `BOOST_AUTO_TEST_CASE( ParentNavigation )`
///
/// Board set-up: `FOOTPRINT fp` with reference `J1` containing a `PCB_TEXT text` with text
/// `J1`.  A and B are both `text`.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn parent_navigation() {
    let zero = Val::Num(0.0);
    let one = Val::Num(1.0);

    // The headline capability: getField() only returns a value when its receiver is a
    // footprint, so this passes solely because navigation steps from the text to its parent.
    test_eval_expr_with_items("A.Parent.getField('Reference') == 'J1'", one, false, "text");

    // The exact pattern from the issue (parent field compared to the text's own value).
    test_eval_expr_with_items(
        "A.Parent.getField('Reference') == A.Text",
        one,
        false,
        "text",
    );

    // The parent resolves to a footprint object that can be type-queried.
    test_eval_expr_with_items("A.Parent.Type == 'Footprint'", one, false, "text");

    // Regression: the terminal "Parent" string still yields the parent footprint reference,
    // i.e. the object and the string refer to the same parent.
    test_eval_expr_with_items("A.Parent == 'J1'", one, false, "text");

    // Without navigation getField() has a non-footprint receiver and returns "", so this must
    // NOT match - it guards against the navigation silently applying to the base item.
    test_eval_expr_with_items("A.getField('Reference') == 'J1'", zero, false, "text");

    // Error cases.  These are code-generation errors (not parse errors), which the shared
    // testEvalExpr does not observe through Compile()'s return value, so check the pending-error
    // status directly.  None of these expressions contain a bare number, so the only error that
    // can be raised is the unrecognized item/property we are testing for.
    // An unknown property on the parent, an unknown navigation step, and navigation on the
    // layer pseudo-item (which has no parent).
    expect_compile_error("A.Parent.bogusProperty");
    expect_compile_error("A.bogus.Reference == 'J1'");
    expect_compile_error("L.Parent.Type == 'Footprint'");
}

/// `BOOST_AUTO_TEST_CASE( PropertyParentFallback )`: a property missing on a child item
/// resolves against its parent footprint.
///
/// Board set-up: `FOOTPRINT fp` with reference `J1` containing a `PCB_TEXT text`.  A and B
/// are both `text`.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn property_parent_fallback() {
    test_eval_expr_with_items("A.Reference == 'J1'", Val::Num(1.0), false, "text");
    test_eval_expr_with_items("A.Reference == 'J2'", Val::Num(0.0), false, "text");
}

/// `BOOST_AUTO_TEST_CASE( BoolPropertyEvaluation )`: a bool property must be readable in a
/// rule.
///
/// Board set-up: `FOOTPRINT fp`; the second check runs after `fp.SetDNP( true )`.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn bool_property_evaluation() {
    test_eval_expr_with_items("A.Do_not_Populate == 0", Val::Num(1.0), false, "fp");
    test_eval_expr_with_items("A.Do_not_Populate == 1", Val::Num(1.0), false, "fp (DNP)");
}

/// `BOOST_AUTO_TEST_CASE( VariantAwareEvaluation )`: queries read flags and fields through
/// the current assembly variant.
///
/// Board set-up: `FOOTPRINT fp` with value `BaseVal` and a variant `V1` (DNP, field `Value`
/// = `VariantVal`); the board has variant `V1`, which is current for the first two checks and
/// unset (`SetCurrentVariant( wxEmptyString )`) for the last two.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn variant_aware_evaluation() {
    let one = Val::Num(1.0);

    test_eval_expr_with_items("A.Do_not_Populate == 1", one, false, "fp, variant V1");
    test_eval_expr_with_items(
        "A.getField('Value') == 'VariantVal'",
        one,
        false,
        "fp, variant V1",
    );

    // Without a current variant the base values apply.
    test_eval_expr_with_items("A.Do_not_Populate == 0", one, false, "fp, no variant");
    test_eval_expr_with_items(
        "A.getField('Value') == 'BaseVal'",
        one,
        false,
        "fp, no variant",
    );
}

/// `BOOST_AUTO_TEST_CASE( FieldAliasNormalization )`
///
/// Reference resolves as a property with the fallback above, so the query normalizer
/// must leave it alone. getField() has no such fallback.
///
/// This tests `normalizeQueryFieldAliases` of `pcbnew/dialogs/dialog_find_by_properties.cpp`
/// (a GUI dialog helper, outside the scope of this port); the data is kept for reference.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn field_alias_normalization() {
    // ( query, custom field rows, expected normalized query )
    let cases: [(&str, &[&str], &str); 3] = [
        ("A.Reference == 'R1'", &[], "A.Reference == 'R1'"),
        // A field with no property behind it needs the rewrite to resolve at all.
        ("A.Value == 'X'", &[], "A.getField('Value') == 'X'"),
        // PROPERTY_ROW_DATA{ propertyName = "My Field", property = nullptr, MATCHING }
        (
            "A.My_Field == 'X'",
            &["My Field"],
            "A.getField('My Field') == 'X'",
        ),
    ];

    for (query, _rows, _expected) in cases {
        needs_pcb_binding(&format!("normalizeQueryFieldAliases( \"{query}\" )"));
    }
}

/// `BOOST_AUTO_TEST_CASE( ReceiverValidation )`
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn receiver_validation() {
    expect_compile_success("L == 'F.Cu'");
    expect_compile_success("AB.isCoupledDiffPair()");
    expect_compile_success("A.Parent.Reference == 'J1'");
    expect_compile_success("A.Width == 1mm");

    expect_compile_error("L.Width == 1mm");
    expect_compile_error("AB.Width == A.Width");
    expect_compile_error("AB.Parent.Reference == 'J1'");
}

/// `BOOST_AUTO_TEST_CASE( LayerReceiverLayerField )`
///
/// `L.Layer == '*.Paste'` must compile without error; with a `PCB_TRACK` as both items it
/// evaluates to 1.0 in a `PCBEXPR_CONTEXT( NULL_CONSTRAINT, F_Paste )` and to 0.0 in a
/// `PCBEXPR_CONTEXT( NULL_CONSTRAINT, F_Cu )`.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn layer_receiver_layer_field() {
    let expression = "L.Layer == '*.Paste'";
    let _expected: [(&str, f64); 2] = [("F_Paste", 1.0), ("F_Cu", 0.0)];

    needs_pcb_binding(&format!("Run( \"{expression}\" ) with layer contexts"));
}

/// `BOOST_AUTO_TEST_CASE( DynamicCourtyardArgument )`
///
/// Board set-up: footprint `U1` (`target`) with a 4 mm x 4 mm rectangle (line width 0.05 mm)
/// on `F_CrtYd` centred on the origin; footprint `U2` (`other`); `PCB_SHAPE graphic`
/// (segment from (0, 0) to (1 mm, 0), width 0.05 mm, `F_Fab`) owned by `other`;
/// `PCB_TEXT targetChild` owned by `target` and `PCB_TEXT otherChild` owned by `other`.
/// Each expression must compile without error with `RequiresPairItems()`, and with layer
/// `F_Fab` evaluate to 1.0 for (graphic, targetChild), 0.0 for (graphic, otherChild) and 1.0
/// again for (graphic, targetChild).
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn dynamic_courtyard_argument() {
    let expressions = [
        "A.intersectsFrontCourtyard(B.Parent)",
        "A.intersectsCourtyard(B.Parent.Reference)",
        "A.intersectsFrontCourtyard(B.Parent.getField('Reference'))",
    ];

    for expression in expressions {
        needs_pcb_binding(&format!("Run( \"{expression}\" ) with courtyard items"));
    }
}

/// `BOOST_AUTO_TEST_CASE( FunctionArgumentValidation )`
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn function_argument_validation() {
    expect_compile_error("A.intersectsFrontCourtyard()");
    expect_compile_error("A.intersectsFrontCourtyard('')");
    expect_compile_error("A.memberOfFootprint('')");
    expect_compile_error("A.fromTo('U1-1')");
    expect_compile_error("A.intersectsFrontCourtyard(B.UnknownProperty)");
    expect_compile_error("A.intersectsFrontCourtyard(B.Parent.unknownFunction())");
}

/// `BOOST_AUTO_TEST_CASE( SingleItemRulePreflight )`
///
/// For each constraint below and for both orders of condition and constraint, the rule file
///
/// ```text
/// (version 1)
/// (rule test
/// (condition "B.Type == 'Graphic'")
/// (constraint <constraint>)
/// )
/// ```
///
/// must parse without "ERROR:" when `B.Type` is replaced by `A.Type` (control), must report a
/// message containing "Item 'B'" through a reporter, and without reporter must throw a
/// `PARSE_ERROR` containing "Item 'B' is not available" at line 3 (condition first) or 4.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn single_item_rule_preflight() {
    let constraints = [
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
    ];

    for constraint in constraints {
        for condition_first in [true, false] {
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

            needs_pcb_binding(&format!("DRC_RULES_PARSER( {source:?} )"));
        }
    }
}

/// `BOOST_AUTO_TEST_CASE( PairConditionsInSingleItemRules )`
///
/// Every body of the first list, wrapped as `(version 1)(rule test <body>)`, must make
/// `DRC_RULES_PARSER::Parse` throw a `PARSE_ERROR` containing "Item 'B' is not available";
/// every body of the second list must parse without "ERROR:".
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn pair_conditions_in_single_item_rules() {
    let rejected = [
        "(condition \"A.intersectsFrontCourtyard(B.Parent)\")\
         (constraint assertion \"A.Type == 'Graphic'\")",
        "(condition \"AB.isCoupledDiffPair()\")(constraint hole_size (min 0.2mm))",
        "(constraint assertion \"B.Type == 'Graphic'\")",
        "(constraint assertion \"A.intersectsFrontCourtyard(B.Parent.getField('Reference'))\")",
        "(condition \"B.Type == 'Graphic'\")(constraint clearance (min 0.2mm))\
         (constraint hole_size (min 0.2mm))",
        "(condition \"AB.isCoupledDiffPair() && B.Type == 'Track'\")\
         (constraint length (max 100mm))",
    ];

    let accepted = [
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
    ];

    for body in rejected.iter().chain(accepted.iter()) {
        needs_pcb_binding(&format!(
            "DRC_RULES_PARSER( \"(version 1)(rule test {body})\" )"
        ));
    }
}

/// `BOOST_AUTO_TEST_CASE( PairItemDependency )`: `PCBEXPR_UCODE::RequiresPairItems()` after
/// compiling each expression (the compile itself must succeed).
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn pair_item_dependency() {
    // ( expression, RequiresPairItems() )
    let cases = [
        ("A.Width == 1mm", false),
        ("B.Width == 1mm", true),
        ("AB.isCoupledDiffPair()", true),
        ("A.Reference == 'B.Width'", false),
    ];

    for (expression, _requires_pair_items) in cases {
        needs_pcb_binding(&format!("RequiresPairItems( \"{expression}\" )"));
    }
}

/// The one expression of `qa/tools/libeval_compiler/libeval_compiler_test.cpp` that is not
/// part of the suite above: with trackA (net1, netclass `HV`) and trackB (net2, netclass
/// `otherClass`) it must evaluate to 0.
#[test]
#[ignore = "needs PCB binding (kicad-drc)"]
fn tool_from_to_expression() {
    test_eval_expr_with_items(
        "A.fromTo('U1', 'U3') && A.NetClass == 'DDR3_A' ",
        Val::Num(0.0),
        false,
        "A = trackA, B = trackB",
    );
}

/// What this crate *can* check about the board-dependent expressions above: they all get
/// through the parser, i.e. any error pending afterwards is a code generation error raised by
/// the (here missing) binding.
#[test]
fn board_expressions_are_syntactically_valid() {
    let expressions = INTROSPECTION_EXPRESSIONS
        .iter()
        .map(|expr| expr.expression)
        .chain([
            "A.Priority == 7",
            "A.Hatch_Orientation == 45deg",
            "A.Minimum_Island_Area == 123456789",
            "A.Ellipse_Rotation == 30deg",
            "A.Solderpaste_Margin_Ratio_Override == 0.125",
            "A.Solderpaste_Margin_Ratio_Override == null",
            "A.Origin_X == 1mm",
            "A.Origin_X != A.End_X",
            "A.Border_Color == 'rgba(64, 128, 191, 0.502)'",
            "A.inNetChainClass('High*')",
            "A.isMicroVia() && !A.isStackedVia()",
            "A.Parent.getField('Reference') == 'J1'",
            "A.Parent.getField('Reference') == A.Text",
            "A.Parent.Type == 'Footprint'",
            "A.Parent == 'J1'",
            "A.Parent.bogusProperty",
            "A.bogus.Reference == 'J1'",
            "L.Parent.Type == 'Footprint'",
            "A.Do_not_Populate == 0",
            "A.getField('Value') == 'VariantVal'",
            "L == 'F.Cu'",
            "AB.isCoupledDiffPair()",
            "AB.Parent.Reference == 'J1'",
            "L.Layer == '*.Paste'",
            "A.intersectsFrontCourtyard(B.Parent)",
            "A.intersectsCourtyard(B.Parent.Reference)",
            "A.intersectsFrontCourtyard(B.Parent.getField('Reference'))",
            "A.intersectsFrontCourtyard()",
            "A.intersectsFrontCourtyard('')",
            "A.fromTo('U1-1')",
            "A.intersectsFrontCourtyard(B.Parent.unknownFunction())",
            "A.Reference == 'B.Width'",
            "AB.isCoupledDiffPair() && B.Type == 'Track'",
            "A.fromTo('U1', 'U3') && A.NetClass == 'DDR3_A' ",
        ]);

    for expression in expressions {
        let mut compiler = Compiler::with_unit_resolver(Box::new(PcbExprUnitResolver::new(false)));
        let mut ucode = Ucode::new(DefaultBinder);
        let mut preflight = Context::new(());

        assert!(
            compiler.compile(expression, &mut ucode, &mut preflight),
            "{expression}"
        );
        assert!(compiler.tree().is_some(), "{expression}");

        if compiler.is_error_pending() {
            assert_eq!(
                compiler.error().stage,
                kicad_libeval::CompilationStage::Codegen,
                "{expression}"
            );
        }
    }
}
