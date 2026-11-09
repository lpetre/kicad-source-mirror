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

//! Semantics of the microcode virtual machine (`LIBEVAL::UOP::Exec`, `LIBEVAL::UCODE::Run`,
//! `LIBEVAL::VALUE`, `LIBEVAL::CONTEXT`): what every operator does with numbers, strings,
//! undefined and null values, units, deferred values and specialised values.
//!
//! Every expectation on an expression in this file was produced by the C++ implementation
//! (the expressions are part of `tests/golden/libeval_cases.txt`, see
//! `tests/oracle_golden.rs`); they are spelled out here so the rules can be read in one
//! place.  The variables and functions are those of the test binding in `tests/common`:
//!
//! | expression | value |
//! |---|---|
//! | `A.Num`, `B.Num`, `L.Num` | 10, 20, 30 (+100 per `.Parent`) |
//! | `A.Frac` | 2.5 |
//! | `A.Str`, `B.Str` | `hello`, `World` (values built from a string, as properties are) |
//! | `A.NumStr` | the string `42` |
//! | `A.Wild` | the wildcard pattern `h*` |
//! | `A.Undef`, `A` | an undefined value |
//! | `A.Null`, `null` | a null value |
//! | `A.Mm` | 1e6 with units mm |
//! | `A.Code`, `B.Code`, `L.Code` | values comparing by code (7, 7, 9) named `seven`, `SEVEN-B`, `nine` |
//! | `A.Alias` | `power_in`, also equal to `Power input` |

mod common;

use std::cell::RefCell;

use common::{TestBinder, TestEnv, make_compiler};
use kicad_libeval::libeval_compiler::{
    TR_OP_ADD, TR_OP_BOOL_AND, TR_OP_JZ, TR_UOP_PUSH_VALUE, TR_UOP_PUSH_VAR,
};
use kicad_libeval::{
    Compiler, Context, DefaultBinder, EdaUnits, Ucode, Uop, UopValue, Value, VarType,
};

struct Eval {
    type_: VarType,
    num: f64,
    text: String,
    units: EdaUnits,
    /// Run-time errors (reported only because the context has an error callback).
    errors: Vec<String>,
    /// Side effects of deferred evaluations.
    log: Vec<String>,
    /// What the run left on the context's stack (0 for well-formed microcode).
    sp: i32,
}

fn eval_with(resolver: &str, expr: &str) -> Eval {
    let mut compiler = make_compiler(resolver);
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(
        compiler.compile(expr, &mut ucode, &mut preflight),
        "'{expr}' must compile"
    );

    let errors: RefCell<Vec<String>> = RefCell::new(Vec::new());

    let (type_, num, text, units, log, sp) = {
        let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());

        ctx.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
            assert_eq!(offset, -1, "run-time errors have no offset");
            errors.borrow_mut().push(message.to_string());
        })));

        let result = ucode.run(&mut ctx);
        let value = ctx.value(result);
        let type_ = value.type_();
        let num = value.as_double(&ctx);
        let text = value.as_string(&ctx).to_string();
        let units = value.units();
        let log = ctx.env().log.borrow().clone();

        (type_, num, text, units, log, ctx.sp())
    };

    Eval {
        type_,
        num,
        text,
        units,
        errors: errors.into_inner(),
        log,
        sp,
    }
}

fn eval(expr: &str) -> Eval {
    eval_with("none", expr)
}

/// The numeric result of an expression that reports no run-time error.
fn num(expr: &str) -> f64 {
    let e = eval(expr);

    assert_eq!(e.type_, VarType::Numeric, "{expr}");
    assert_eq!(e.units, EdaUnits::Unscaled, "{expr}");
    assert!(e.errors.is_empty(), "{expr}: {:?}", e.errors);
    e.num
}

/// What an expression's microcode leaves on the stack besides (or instead of) its result.
fn stack_left(expr: &str) -> i32 {
    eval(expr).sp
}

/// The numeric result and the single "Type mismatch" error of an expression.
fn mismatch(expr: &str) -> (f64, String) {
    let e = eval(expr);

    assert_eq!(e.type_, VarType::Numeric, "{expr}");
    assert_eq!(e.errors.len(), 1, "{expr}: {:?}", e.errors);
    (e.num, e.errors[0].clone())
}

/* --------------------------------------------------------------------------------------------
 * Numbers
 */

#[test]
fn arithmetic() {
    assert_eq!(num("1 + 2"), 3.0);
    assert_eq!(num("1 - 2"), -1.0);
    assert_eq!(num("3 * 4"), 12.0);
    assert_eq!(num("7 / 2"), 3.5);
    assert_eq!(num("-(1 + 2)"), -3.0);
    assert_eq!(num("0.1 + 0.2"), 0.1 + 0.2);
    assert_eq!(num("A.Num + B.Num * L.Num"), 610.0);
    assert_eq!(num("A.Frac * 2"), 5.0);
}

/// Division follows IEEE 754: no error, infinities and NaN.
#[test]
fn division_by_zero() {
    assert_eq!(num("1 / 0"), f64::INFINITY);
    assert_eq!(num("-1 / 0"), f64::NEG_INFINITY);
    assert!(num("0 / 0").is_nan());

    assert_eq!(num("(1 / 0) == (1 / 0)"), 1.0);
    assert_eq!(num("(0 / 0) == (0 / 0)"), 0.0);
    assert_eq!(num("(0 / 0) != (0 / 0)"), 1.0);

    // NaN is "true": it is not equal to zero.
    assert_eq!(num("!(0 / 0)"), 0.0);
    assert_eq!(num("(0 / 0) && 1"), 1.0);
    assert_eq!(num("(0 / 0) || 0"), 1.0);
}

#[test]
fn comparisons() {
    assert_eq!(num("1 == 1"), 1.0);
    assert_eq!(num("1 == 2"), 0.0);
    assert_eq!(num("1 != 2"), 1.0);
    assert_eq!(num("1 < 2"), 1.0);
    assert_eq!(num("2 <= 2"), 1.0);
    assert_eq!(num("3 > 2"), 1.0);
    assert_eq!(num("2 >= 3"), 0.0);
}

/// Boolean operators treat any non-zero number as true and produce exactly 0 or 1.
#[test]
fn boolean_coercion() {
    assert_eq!(num("2 && 3"), 1.0);
    assert_eq!(num("2 || 3"), 1.0);
    assert_eq!(num("0.5 && 0.5"), 1.0);
    assert_eq!(num("!0.5"), 0.0);
    assert_eq!(num("!-1"), 0.0);
    assert_eq!(num("!0"), 1.0);

    // A short-circuited || must yield a normalized 1, not the raw (nonzero) left operand.
    assert_eq!(num("(2 || 0) == 1"), 1.0);
    assert_eq!(num("(7 || 0) + 5"), 6.0);
    assert_eq!(num("(0 || 7) + 5"), 6.0);
    assert_eq!(num("(7 && 3) + 5"), 6.0);
    assert_eq!(num("(0 && 3) + 5"), 5.0);
    assert_eq!(num("(3 && 0) + 5"), 5.0);
}

/// An empty expression (or one of white space only) is "true".
#[test]
fn empty_expression() {
    assert_eq!(num(""), 1.0);
    assert_eq!(num("   "), 1.0);
}

/* --------------------------------------------------------------------------------------------
 * Undefined and null values
 */

/// An undefined value (unknown item, property missing on the item, `VALUE()`) is 0 in
/// arithmetic and ordering, false in boolean context, and never equal *or* unequal to
/// anything: `EqualTo` and `NotEqualTo` both return false.
#[test]
fn undefined_values() {
    assert_eq!(eval("A.Undef").type_, VarType::Undefined);
    assert_eq!(eval("A").type_, VarType::Undefined);

    assert_eq!(num("A.Undef + 1"), 1.0);
    assert_eq!(num("A.Undef * 5"), 0.0);
    assert_eq!(num("1 - A.Undef"), 1.0);
    assert_eq!(num("A.Undef < 1"), 1.0);
    assert_eq!(num("A.Undef >= 0"), 1.0);
    assert_eq!(num("!A.Undef"), 1.0);
    assert_eq!(num("A.Undef && 1"), 0.0);
    assert_eq!(num("A.Undef || 1"), 1.0);

    assert_eq!(num("A.Undef == 0"), 0.0);
    assert_eq!(num("A.Undef != 0"), 0.0);
    assert_eq!(num("0 == A.Undef"), 0.0);
    assert_eq!(num("0 != A.Undef"), 0.0);
    assert_eq!(num("A.Undef == A.Undef"), 0.0);
    assert_eq!(num("A.Undef != A.Undef"), 0.0);
    assert_eq!(num("A.Undef == 'x'"), 0.0);
    assert_eq!(num("A.Undef == null"), 0.0);
    assert_eq!(num("null == A.Undef"), 0.0);
    assert_eq!(num("A == A"), 0.0);
}

/// A null value (an unset optional property, the `null` keyword of the binding) is 0 in
/// arithmetic, equal only to another null and unequal to everything defined.
#[test]
fn null_values() {
    assert_eq!(eval("A.Null").type_, VarType::Null);
    assert_eq!(eval("null").type_, VarType::Null);

    assert_eq!(num("A.Null + 1"), 1.0);
    assert_eq!(num("!A.Null"), 1.0);

    assert_eq!(num("A.Null == null"), 1.0);
    assert_eq!(num("null == A.Null"), 1.0);
    assert_eq!(num("A.Null != null"), 0.0);
    assert_eq!(num("null == null"), 1.0);
    assert_eq!(num("null != null"), 0.0);

    assert_eq!(num("A.Null == 0"), 0.0);
    assert_eq!(num("A.Null != 0"), 1.0);
    assert_eq!(num("0 == A.Null"), 0.0);
    assert_eq!(num("A.Null == 'x'"), 0.0);
    assert_eq!(num("A.Null != 'x'"), 1.0);
    assert_eq!(num("A.Num == null"), 0.0);
    assert_eq!(num("A.Num != null"), 1.0);
    assert_eq!(num("A.Str == null"), 0.0);
    assert_eq!(num("A.Str != null"), 1.0);

    // undefined wins over null
    assert_eq!(num("A.Null == A.Undef"), 0.0);
    assert_eq!(num("A.Null != A.Undef"), 0.0);
}

/* --------------------------------------------------------------------------------------------
 * Strings
 */

/// String comparison is case insensitive; a `?` or `*` in a string *literal on the right-hand
/// side* makes it a wildcard pattern.
#[test]
fn string_comparison() {
    let e = eval("A.Str");
    assert_eq!(
        (e.type_, e.text.as_str(), e.num),
        (VarType::String, "hello", 0.0)
    );

    assert_eq!(num("A.Str == 'hello'"), 1.0);
    assert_eq!(num("A.Str == 'HELLO'"), 1.0);
    assert_eq!(num("A.Str == 'hell'"), 0.0);
    assert_eq!(num("A.Str != 'HELLO'"), 0.0);
    assert_eq!(num("'abc' == 'ABC'"), 1.0);
    assert_eq!(num("'abc' != 'abd'"), 1.0);

    assert_eq!(num("A.Str == 'h*'"), 1.0);
    assert_eq!(num("A.Str == 'H?LLO'"), 1.0);
    assert_eq!(num("'abc' == '????'"), 0.0);
    assert_eq!(num("'' == '*'"), 1.0);
    assert_eq!(num("'' == '?'"), 0.0);

    // the pattern must be the right operand
    assert_eq!(num("'h*' == A.Str"), 0.0);
    assert_eq!(num("'a*' == 'abc'"), 0.0);

    // a value flagged as wildcard by the binding behaves the same
    assert_eq!(num("A.Wild == 'hello'"), 0.0);
    assert_eq!(num("'hello' == A.Wild"), 1.0);
    assert_eq!(num("A.Str == A.Wild"), 1.0);
    assert_eq!(num("B.Str == A.Wild"), 0.0);
    assert_eq!(num("A.Str != A.Wild"), 0.0);
    assert_eq!(num("B.Str != A.Wild"), 1.0);

    // Case folding is per character and not limited to ASCII (UTF-8 locale behaviour).
    assert_eq!(
        num("'\u{fc}n\u{ef}c\u{f6}d\u{e9}' == '\u{dc}N\u{cf}C\u{d6}D\u{c9}'"),
        1.0
    );
    assert_eq!(num("'\u{fc}n\u{ef}c\u{f6}d\u{e9}' == '\u{dc}*'"), 1.0);
    assert_eq!(num("'stra\u{df}e' == 'STRASSE'"), 0.0);
    assert_eq!(num("'\u{130}' == 'i'"), 1.0);
}

/// There is no string arithmetic, ordering or truthiness: in every operator except `==` and
/// `!=` a string counts as 0.
#[test]
fn strings_are_zero_in_arithmetic() {
    assert_eq!(num("A.Str + A.Str"), 0.0);
    assert_eq!(num("'5' * '5'"), 0.0);
    assert_eq!(num("'5' + '5'"), 0.0);
    assert_eq!(num("'a' < 'b'"), 0.0);
    assert_eq!(num("'abc' && 1"), 0.0);
    assert_eq!(num("!'abc'"), 1.0);
    assert_eq!(num("!''"), 1.0);
    assert_eq!(num("!A.Str"), 1.0);
    assert_eq!(num("A.Str && 1"), 0.0);
}

/// A string meeting a number is reported as "Type mismatch" (if the context has an error
/// callback) and counts as 0 -- unless it is the *left* operand, is not flagged as literal and
/// looks like a number (`QuacksLikeADuck`), in which case it is converted.
///
/// "Flagged as literal" means "built with the `VALUE( wxString )` constructor", which is how
/// bindings create property values.  String literals of the expression are pushed as copies
/// that lose the flag, so they are the ones that get converted.
#[test]
fn string_number_mismatch() {
    assert_eq!(
        mismatch("A.Str + 1"),
        (
            1.0,
            "Type mismatch between 'hello' and 1.000000".to_string()
        )
    );
    assert_eq!(
        mismatch("1 + A.Str"),
        (
            1.0,
            "Type mismatch between 1.000000 and 'hello'".to_string()
        )
    );
    assert_eq!(mismatch("A.Str * 2").0, 0.0);
    assert_eq!(mismatch("A.Str < 1").0, 1.0);
    assert_eq!(mismatch("A.Str == 1").0, 0.0);
    assert_eq!(mismatch("A.Str != 1").0, 1.0);
    assert_eq!(mismatch("1 == A.Str").0, 0.0);
    assert_eq!(mismatch("1 != A.Str").0, 1.0);

    // || evaluates its right side here, so the OR op sees the string and the number.
    assert_eq!(
        mismatch("A.Str || 0"),
        (
            0.0,
            "Type mismatch between 'hello' and 0.000000".to_string()
        )
    );

    // A property value that looks like a number is still a mismatch ...
    assert_eq!(
        mismatch("A.NumStr + 1"),
        (1.0, "Type mismatch between '42' and 1.000000".to_string())
    );
    assert_eq!(mismatch("A.NumStr == 42").0, 0.0);
    assert_eq!(
        mismatch("42 == A.NumStr"),
        (0.0, "Type mismatch between 42.000000 and '42'".to_string())
    );
    assert_eq!(num("A.NumStr == '42'"), 1.0);

    // ... while a literal on the left is converted ...
    assert_eq!(num("'5' + 1"), 6.0);
    assert_eq!(num("'5' == 5"), 1.0);
    assert_eq!(num("'5' != 5"), 0.0);
    assert_eq!(num("'4k7' + 0"), 4700.0);
    assert_eq!(num("'1.5mm' + 0"), 0.0015);
    assert_eq!(num("'R5' + 0"), 0.5);
    assert_eq!(num("'1,000.5' + 0"), 1000.5);
    assert_eq!(
        mismatch("'abc' + 1"),
        (1.0, "Type mismatch between 'abc' and 1.000000".to_string())
    );
    assert_eq!(
        mismatch("'-5' + 0"),
        (0.0, "Type mismatch between '-5' and 0.000000".to_string())
    );

    // ... and on the right it never is.
    assert_eq!(
        mismatch("1 + '5'"),
        (1.0, "Type mismatch between 1.000000 and '5'".to_string())
    );
    assert_eq!(mismatch("5 == '5'").0, 0.0);
    assert_eq!(mismatch("5 != '5'").0, 1.0);
}

/// Without error callback nothing is reported, and the results are the same.
#[test]
fn mismatch_without_callback() {
    let mut compiler = Compiler::new();
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("A.Str + 1", &mut ucode, &mut preflight));

    let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());
    assert!(!ctx.has_error_callback());

    let result = ucode.run(&mut ctx);
    assert_eq!(ctx.value(result).as_double(&ctx), 1.0);
}

/* --------------------------------------------------------------------------------------------
 * Units
 */

/// Literals carry the unit they were written with; `+ - * /` and the unary operators pass
/// units on (the right operand's, unless only the left one has any); comparisons and
/// boolean operators produce unit-less results.
#[test]
fn units() {
    let u = |expr: &str| {
        let e = eval_with("pcb", expr);
        assert_eq!(e.type_, VarType::Numeric, "{expr}");
        (e.num, e.units)
    };

    assert_eq!(u("1mm"), (1e6, EdaUnits::Mm));
    assert_eq!(u("1mil"), (25400.0, EdaUnits::Mils));
    assert_eq!(u("1in"), (25.4e6, EdaUnits::Inch));
    assert_eq!(u("1deg"), (1.0, EdaUnits::Degrees));
    assert_eq!(u("1fs"), (1000.0, EdaUnits::Fs));
    assert_eq!(u("1ps"), (1e6, EdaUnits::Ps));
    assert_eq!(u("0.2mm + 1mil"), (225400.0, EdaUnits::Mils));

    assert_eq!(u("1mm + 1"), (1000001.0, EdaUnits::Mm));
    assert_eq!(u("1 + 1mm"), (1000001.0, EdaUnits::Mm));
    assert_eq!(u("1mm + 1mil"), (1025400.0, EdaUnits::Mils));
    assert_eq!(u("1mil + 1mm"), (1025400.0, EdaUnits::Mm));
    assert_eq!(u("1mm * 2"), (2e6, EdaUnits::Mm));
    assert_eq!(u("2 / 1mm"), (2e-6, EdaUnits::Mm));
    assert_eq!(u("1mm - 1in"), (-24.4e6, EdaUnits::Inch));
    assert_eq!(u("1deg + 1mm"), (1000001.0, EdaUnits::Mm));
    assert_eq!(u("-1mm"), (-1e6, EdaUnits::Mm));
    assert_eq!(u("!1mm"), (0.0, EdaUnits::Mm));

    assert_eq!(u("1mm == 1mm"), (1.0, EdaUnits::Unscaled));
    assert_eq!(u("1mm < 2mm"), (1.0, EdaUnits::Unscaled));
    assert_eq!(u("1mm && 1mm"), (1.0, EdaUnits::Unscaled));
    assert_eq!(u("1mm || 0"), (1.0, EdaUnits::Unscaled));
    assert_eq!(u("0 || 1mm"), (1.0, EdaUnits::Unscaled));
    assert_eq!(u("(1mm || 0) + 1mm"), (1000001.0, EdaUnits::Mm));

    // values produced by the binding can carry units too
    assert_eq!(u("A.Mm"), (1e6, EdaUnits::Mm));
    assert_eq!(u("A.Mm + 1"), (1000001.0, EdaUnits::Mm));
}

/* --------------------------------------------------------------------------------------------
 * Functions and deferred evaluation
 */

#[test]
fn function_calls() {
    assert_eq!(num("A.one() + A.one()"), 2.0);
    assert_eq!(num("A.isFoo('foo')"), 1.0);
    assert_eq!(num("A.isFoo('bar')"), 0.0);
    assert_eq!(num("A.add(1 , 2)"), 3.0);

    // the receiver is passed to the function
    assert_eq!(num("A.self()"), 1.0);
    assert_eq!(num("B.self()"), 11.0);
    assert_eq!(num("A.Parent.self()"), 2.0);
    assert_eq!(num("A.Parent.Parent.Num"), 210.0);
    // ... as "no receiver" if the item is unknown (a compile-time error was reported)
    assert_eq!(num("X.self()"), 0.0);

    // At run time the arguments are on the stack in reverse order: the first argument is
    // popped first.  cat() concatenates "second popped" + "first popped".
    assert_eq!(eval("A.cat('a' , 'b')").text, "ba");
    assert_eq!(eval("A.cat(A.Str , B.Str)").text, "Worldhello");
    assert_eq!(num("A.add(10 , 2) - A.add(2 , 10)"), 0.0);

    // a function reporting an error at run time
    let e = eval("A.isFoo('')");
    assert_eq!(
        (e.num, e.errors),
        (0.0, vec!["Missing argument to isFoo().".to_string()])
    );
}

/// Microcode that leaves anything but exactly one value on the stack evaluates to 0.
#[test]
fn malformed_microcode() {
    // a function that pushes two results
    assert_eq!(num("A.push2()"), 0.0);
    // a function that pushes none
    assert_eq!(num("A.nopush(1)"), 0.0);
    // an unknown function is not called at all
    assert_eq!(num("A.unknown()"), 0.0);
    // the comma operator outside of a call
    assert_eq!(num("1 , 2"), 0.0);
    assert_eq!(num("(1 , 2) + 3"), 0.0);

    // Popping an empty stack reports "Malformed expression" and yields an undefined value.
    let e = eval("A.unknown() + 1");
    assert_eq!(
        (e.num, e.errors),
        (1.0, vec!["Malformed expression".to_string()])
    );

    // As in C++, Run() does not tidy up: surplus values stay on the context's stack until the
    // context is reset.
    assert_eq!(stack_left("1 + 2"), 0);
    assert_eq!(stack_left("A.push2()"), 2);
    assert_eq!(stack_left("1 , 2"), 2);
    assert_eq!(stack_left("A.nopush(1)"), 0);
}

/// A deferred value is computed when it is first read, and not at all if nothing reads it:
/// `&&` and `||` skip their right operand when the left one decides (short-circuit jumps).
#[test]
fn deferred_evaluation() {
    let lazy = |expr: &str| {
        let e = eval(expr);
        (e.num, e.log.len(), e.errors.len())
    };

    // ( result, number of evaluations of lazy(), errors it reported )
    assert_eq!(lazy("A.lazy()"), (5.0, 1, 1));
    assert_eq!(lazy("A.lazy() + 1"), (6.0, 1, 1));
    assert_eq!(lazy("A.lazy() == 5"), (1.0, 1, 1));
    assert_eq!(lazy("A.lazy() * 0"), (0.0, 1, 1));
    assert_eq!(lazy("A.lazy() + A.lazy()"), (10.0, 2, 2));

    assert_eq!(lazy("0 && A.lazy()"), (0.0, 0, 0));
    assert_eq!(lazy("1 || A.lazy()"), (1.0, 0, 0));
    assert_eq!(lazy("1 && A.lazy()"), (1.0, 1, 1));
    assert_eq!(lazy("0 || A.lazy()"), (1.0, 1, 1));
    assert_eq!(lazy("A.lazy() && 0"), (0.0, 1, 1));
    assert_eq!(lazy("A.lazy() || 1"), (1.0, 1, 1));
    assert_eq!(lazy("A.lazy() && A.lazy()"), (1.0, 2, 2));
    assert_eq!(lazy("A.lazy() || A.lazy()"), (1.0, 1, 1));
    assert_eq!(lazy("A.isFoo('foo') || A.lazy()"), (1.0, 0, 0));
    assert_eq!(lazy("A.isFoo('bar') && A.lazy()"), (0.0, 0, 0));

    // deferred strings
    let e = eval("A.lazyStr('x')");
    assert_eq!(
        (e.type_, e.text.as_str(), e.log.len()),
        (VarType::String, "<x>", 1)
    );
    assert_eq!(lazy("A.lazyStr('x') == '<x>'"), (1.0, 1, 0));
    assert_eq!(lazy("0 && A.lazyStr('x')"), (0.0, 0, 0));
    assert_eq!(eval("A.lazyStr(A.lazyStr('x'))").text, "<<x>>");
    assert_eq!(
        mismatch("A.lazyStr('x') + 1"),
        (1.0, "Type mismatch between '<x>' and 1.000000".to_string())
    );
}

/// The result of a run may itself be deferred: it is computed when the caller reads it, and
/// only once.
#[test]
fn deferred_result_is_cached() {
    let mut compiler = Compiler::new();
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("A.lazy()", &mut ucode, &mut preflight));
    // the pre-flight call does not read the value
    assert!(preflight.env().log.borrow().is_empty());

    let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());
    let result = ucode.run(&mut ctx);

    assert!(ctx.env().log.borrow().is_empty());
    assert_eq!(ctx.value(result).as_double(&ctx), 5.0);
    assert_eq!(ctx.value(result).as_double(&ctx), 5.0);
    assert_eq!(ctx.env().log.borrow().len(), 1);
}

/* --------------------------------------------------------------------------------------------
 * Specialised values (C++: subclasses of VALUE)
 */

/// `EqualTo` / `NotEqualTo` are dispatched on the *left* operand (on the right one only if
/// the right one is undefined), so an overriding value must be on the left to have a say.
#[test]
fn custom_values() {
    // A.Code and B.Code have the same code but different names; L.Code differs.
    assert_eq!(num("A.Code == B.Code"), 1.0);
    assert_eq!(num("A.Code != B.Code"), 0.0);
    assert_eq!(num("A.Code == L.Code"), 0.0);
    assert_eq!(num("A.Code != L.Code"), 1.0);

    // Against a plain string the (lazily computed) name is compared.
    assert_eq!(eval("A.Code").text, "seven");
    assert_eq!(eval("B.Code").text, "SEVEN-B");
    assert_eq!(num("A.Code == 'seven'"), 1.0);
    assert_eq!(num("A.Code == 'SEVEN'"), 1.0);
    assert_eq!(num("A.Code != 'seven'"), 0.0);
    assert_eq!(num("B.Code == 'seven'"), 0.0);
    assert_eq!(num("'seven' == B.Code"), 0.0);
    assert_eq!(num("'seven-b' == B.Code"), 1.0);
    assert_eq!(num("A.Code == 's*'"), 1.0);

    // A.Alias overrides only EqualTo; NotEqualTo is derived from it.
    assert_eq!(num("A.Alias == 'power_in'"), 1.0);
    assert_eq!(num("A.Alias == 'Power input'"), 1.0);
    assert_eq!(num("A.Alias != 'Power input'"), 0.0);
    assert_eq!(num("A.Alias == 'passive'"), 0.0);
    assert_eq!(num("A.Alias != 'passive'"), 1.0);
    // ... and has no say as the right operand
    assert_eq!(num("'Power input' == A.Alias"), 0.0);
    assert_eq!(num("'power_in' == A.Alias"), 1.0);

    // undefined still wins
    assert_eq!(num("A.Code == A.Undef"), 0.0);
    assert_eq!(num("A.Undef == A.Code"), 0.0);
    assert_eq!(num("A.Alias == A.Undef"), 0.0);
    assert_eq!(num("A.Undef == A.Alias"), 0.0);
    assert_eq!(num("A.Alias != A.Undef"), 0.0);

    // custom values as function arguments
    assert_eq!(eval("A.lazyStr(A.Code)").text, "<seven>");
    assert_eq!(eval("A.cat(A.Code , A.Alias)").text, "power_inseven");
}

/* --------------------------------------------------------------------------------------------
 * The API below the compiler: Value, Context, Uop, Ucode
 */

type Plain<'e> = Value<'e, DefaultBinder>;

#[test]
fn value_constructors_and_setters() {
    let ctx: Context<'_, DefaultBinder> = Context::new(());

    let v = Plain::new();
    assert_eq!(v.type_(), VarType::Undefined);
    assert_eq!(v.units(), EdaUnits::Unscaled);
    assert_eq!(v.as_double(&ctx), 0.0);
    assert_eq!(v.as_string(&ctx), "");
    assert!(!v.string_is_wildcard() && !v.string_is_literal());

    let v = Plain::from_double(2.5);
    assert_eq!(
        (v.type_(), v.as_double(&ctx), v.as_string(&ctx)),
        (VarType::Numeric, 2.5, "")
    );

    let v = Plain::from_string("abc", true);
    assert_eq!(
        (v.type_(), v.as_double(&ctx), v.as_string(&ctx)),
        (VarType::String, 0.0, "abc")
    );
    assert!(v.string_is_wildcard() && v.string_is_literal());

    assert_eq!(Plain::make_null_value().type_(), VarType::Null);

    // Set( double ) and Set( const wxString& ) change the type but keep the other member.
    let mut v = Plain::from_string("abc", false);
    v.set_double(7.0);
    assert_eq!(
        (v.type_(), v.as_double(&ctx), v.as_string(&ctx)),
        (VarType::Numeric, 7.0, "abc")
    );
    v.set_string("xyz");
    assert_eq!(
        (v.type_(), v.as_double(&ctx), v.as_string(&ctx)),
        (VarType::String, 7.0, "xyz")
    );
    assert!(v.string_is_literal());

    // Set( const VALUE& ) copies type, number, wildcard flag, units and string -- but not the
    // "literal" flag.
    let mut source = Plain::from_string("pattern*", true);
    source.set_units(EdaUnits::Mm);
    let mut copy = Plain::new();
    copy.set_value(&source);
    assert_eq!(
        (copy.type_(), copy.as_string(&ctx)),
        (VarType::String, "pattern*")
    );
    assert!(copy.string_is_wildcard());
    assert!(!copy.string_is_literal());
    assert_eq!(copy.units(), EdaUnits::Mm);

    let mut copy = Plain::from_string("old", false);
    copy.set_value(&Plain::from_double(3.0));
    assert_eq!(
        (copy.type_(), copy.as_double(&ctx), copy.as_string(&ctx)),
        (VarType::Numeric, 3.0, "old")
    );
}

#[test]
fn value_equality() {
    // ( EqualTo, NotEqualTo ); values and context share the lifetime parameter.
    fn eq<'e>(a: &Plain<'e>, b: &Plain<'e>) -> (bool, bool) {
        let ctx: Context<'e, DefaultBinder> = Context::new(());
        (a.equal_to(&ctx, b), a.not_equal_to(&ctx, b))
    }

    let undefined = Plain::new();
    let null = Plain::make_null_value();
    let one = Plain::from_double(1.0);
    let two = Plain::from_double(2.0);
    let abc = Plain::from_string("abc", false);
    let upper = Plain::from_string("ABC", false);
    let pattern = Plain::from_string("a*", true);

    assert_eq!(eq(&one, &one), (true, false));
    assert_eq!(eq(&one, &two), (false, true));
    assert_eq!(eq(&abc, &upper), (true, false));
    assert_eq!(eq(&abc, &pattern), (true, false));
    assert_eq!(eq(&pattern, &abc), (false, true));
    assert_eq!(eq(&null, &null), (true, false));
    assert_eq!(eq(&null, &one), (false, true));
    assert_eq!(eq(&one, &abc), (false, true));
    assert_eq!(eq(&abc, &one), (false, true));

    // NB: NotEqualTo is not an inverse of EqualTo as they both return false for undefined
    // values.
    assert_eq!(eq(&undefined, &undefined), (false, false));
    assert_eq!(eq(&undefined, &one), (false, false));
    assert_eq!(eq(&one, &undefined), (false, false));
    assert_eq!(eq(&null, &undefined), (false, false));
}

#[test]
fn value_deferred_evaluation() {
    // Whatever a deferred evaluation borrows must outlive the context type's lifetime
    // parameter, so it is declared first.
    let calls = std::cell::Cell::new(0);
    let ctx: Context<'_, DefaultBinder> = Context::new(());

    let mut v = Plain::new();
    v.set_double(1.0);
    v.set_deferred_eval_double(|_ctx| {
        calls.set(calls.get() + 1);
        42.0
    });

    assert_eq!(v.type_(), VarType::Numeric);
    assert_eq!(calls.get(), 0);
    assert_eq!(v.as_double(&ctx), 42.0);
    assert_eq!(v.as_double(&ctx), 42.0);
    assert_eq!(calls.get(), 1);

    // Set() after the evaluation sticks ...
    v.set_double(3.0);
    assert_eq!(v.as_double(&ctx), 3.0);

    // ... while Set() before it is overwritten by the deferred result.
    let mut v = Plain::new();
    v.set_deferred_eval_string(|_ctx| "late".to_string());
    v.set_string("early");
    assert_eq!(v.type_(), VarType::String);
    assert_eq!(v.as_string(&ctx), "late");
    v.set_string("after");
    assert_eq!(v.as_string(&ctx), "after");
}

#[test]
fn context_stack_and_values() {
    let errors: RefCell<Vec<(String, i32)>> = RefCell::new(Vec::new());
    let mut ctx: Context<'_, DefaultBinder> = Context::new(());

    assert_eq!(ctx.sp(), 0);
    assert!(!ctx.has_error_callback());

    // Without callback errors vanish.
    ctx.report_error("nobody listens");

    ctx.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
        errors.borrow_mut().push((message.to_string(), offset));
    })));
    assert!(ctx.has_error_callback());

    let a = ctx.alloc_value();
    let b = ctx.store_value(Value::from_double(2.0));
    assert_eq!(ctx.value(a).type_(), VarType::Undefined);
    ctx.value_mut(a).set_double(1.0);

    ctx.push(a);
    ctx.push(b);
    assert_eq!(ctx.sp(), 2);
    assert_eq!(ctx.top(), b);
    assert_eq!(ctx.sp(), 2);
    assert_eq!(ctx.pop(), b);
    assert_eq!(ctx.pop(), a);
    assert_eq!(ctx.sp(), 0);
    assert!(errors.borrow().is_empty());

    // Pop() and Top() on an empty stack report an error and hand out a fresh undefined value.
    let fresh = ctx.pop();
    assert_eq!(ctx.value(fresh).type_(), VarType::Undefined);
    let fresh = ctx.top();
    assert_eq!(ctx.value(fresh).type_(), VarType::Undefined);
    assert_eq!(ctx.sp(), 0);
    assert_eq!(
        *errors.borrow(),
        vec![
            ("Malformed expression".to_string(), -1),
            ("Malformed expression".to_string(), -1)
        ]
    );

    // Reset() drops values, stack and callback.
    ctx.push(a);
    ctx.reset();
    assert_eq!(ctx.sp(), 0);
    assert!(!ctx.has_error_callback());
    let first = ctx.alloc_value();
    assert_eq!(ctx.value(first).type_(), VarType::Undefined);
}

/// A context can be reused for many runs; values accumulate until `reset`.
#[test]
fn context_reuse() {
    let mut compiler = Compiler::new();
    let mut ucode = Ucode::new(DefaultBinder);
    let mut ctx = Context::new(());

    assert!(compiler.compile("(1 + 2) * 3 == 9 && 'a' == 'A'", &mut ucode, &mut ctx));

    for _ in 0..1000 {
        ctx.reset();
        let result = ucode.run(&mut ctx);
        assert_eq!(ctx.value(result).as_double(&ctx), 1.0);
        assert_eq!(ctx.sp(), 0);
    }

    // Without reset the earlier results stay readable.
    let first = ucode.run(&mut ctx);
    let second = ucode.run(&mut ctx);
    assert_ne!(first, second);
    assert_eq!(ctx.value(first).as_double(&ctx), 1.0);
    assert_eq!(ctx.value(second).as_double(&ctx), 1.0);
}

/// Microcode can also be assembled by hand.
#[test]
fn hand_written_microcode() {
    let number = |value: f64| -> Uop<DefaultBinder> {
        Uop::with_value(
            TR_UOP_PUSH_VALUE,
            Some(UopValue::Numeric {
                value,
                units: EdaUnits::Unscaled,
            }),
        )
    };

    let mut code = Ucode::new(DefaultBinder);
    code.add_op(number(2.0));
    code.add_op(number(3.0));
    code.add_op(Uop::new(TR_OP_ADD));
    assert_eq!(code.size(), 3);
    assert_eq!(
        code.dump(),
        "PUSH NUM [2.0000000000]\nPUSH NUM [3.0000000000]\nADD 515\n"
    );

    let mut ctx = Context::new(());
    let result = code.run(&mut ctx);
    assert_eq!(ctx.value(result).as_double(&ctx), 5.0);

    // 0 && <anything>: JZ jumps over the right operand and the AND.
    let mut code = Ucode::new(DefaultBinder);
    code.add_op(number(0.0));
    code.add_op(Uop::new(TR_OP_JZ));
    code.mark_has_jumps();
    code.add_op(Uop::with_ref(TR_UOP_PUSH_VAR, None));
    code.add_op(Uop::new(TR_OP_BOOL_AND));
    code.set_jump_target(1, code.size());
    assert_eq!(
        code.dump(),
        "PUSH NUM [0.0000000000]\nJZ -> 4\nPUSH VAR [(nil)]\nAND 523\n"
    );
    assert_eq!(code.ops()[1].jump_target(), 4);
    assert_eq!(code.ops()[1].op(), TR_OP_JZ);

    let mut ctx = Context::new(());
    let result = code.run(&mut ctx);
    assert_eq!(ctx.value(result).as_double(&ctx), 0.0);

    // Ops outside every category do nothing; other dumps.
    let mut code: Ucode<DefaultBinder> = Ucode::default();
    code.add_op(Uop::with_value(TR_UOP_PUSH_VALUE, None));
    code.add_op(Uop::with_value(
        TR_UOP_PUSH_VALUE,
        Some(UopValue::String {
            value: "s".to_string(),
            is_wildcard: false,
        }),
    ));
    code.add_op(Uop::new(7));
    code.add_op(Uop::with_func(
        kicad_libeval::libeval_compiler::TR_OP_METHOD_CALL,
        None,
        None,
    ));
    code.add_op(Uop::new(kicad_libeval::libeval_compiler::TR_OP_FUNC_CALL));
    code.add_op(Uop::new(kicad_libeval::libeval_compiler::TR_OP_JNZ));
    assert_eq!(
        code.dump(),
        "PUSH nullptr\nPUSH STR [s]\n??? 7\nMCALL\nFCALL\nJNZ -> -1\n"
    );
}
