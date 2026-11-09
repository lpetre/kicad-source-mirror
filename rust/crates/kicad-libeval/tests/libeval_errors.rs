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

//! Error reporting of the expression compiler: the exact messages, the source offsets
//! passed to the error callback, the return value of `compile` and the pending error status.
//!
//! Every expectation in this file was produced by the C++ implementation (the cases are
//! part of `tests/golden/libeval_cases.txt`, see `tests/oracle_golden.rs`); they are spelled
//! out here so the rules can be read in one place.
//!
//! Offsets count characters.  The source position of a token is the position at which the
//! tokenizer *started looking* for it, i.e. the white space before the token belongs to it.
//! Syntax and lexical errors are reported at the position of the offending token.  Code
//! generation errors compute their offset from the position stored in a tree node, which is
//! the position of the token *after* the node's text (see `tests/libeval_grammar.rs`).

mod common;

use std::cell::RefCell;

use common::{TestBinder, TestEnv, make_compiler};
use kicad_libeval::{CompilationStage, Context, Ucode};

struct Outcome {
    /// The return value of `compile`.
    ok: bool,
    /// Every invocation of the compiler's error callback, in order.
    errors: Vec<(i32, String)>,
    /// `( stage, srcPos, message )` of `Compiler::error()` if an error is pending.
    pending: Option<(CompilationStage, i32, String)>,
}

fn compile(resolver: &str, expr: &str) -> Outcome {
    let errors: RefCell<Vec<(i32, String)>> = RefCell::new(Vec::new());

    let (ok, pending) = {
        let mut compiler = make_compiler(resolver);
        let mut ucode = Ucode::new(TestBinder::default());
        let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

        compiler.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
            errors.borrow_mut().push((offset, message.to_string()));
        })));

        let ok = compiler.compile(expr, &mut ucode, &mut preflight);
        let status = compiler.error();
        let pending = compiler
            .is_error_pending()
            .then(|| (status.stage, status.src_pos, status.message.clone()));

        (ok, pending)
    };

    Outcome {
        ok,
        errors: errors.into_inner(),
        pending,
    }
}

/// Check a case with exactly the given callback invocations; the pending error must be the
/// last one reported.
fn check(resolver: &str, expr: &str, ok: bool, stage: CompilationStage, expected: &[(i32, &str)]) {
    let outcome = compile(resolver, expr);
    let expected: Vec<(i32, String)> = expected
        .iter()
        .map(|(offset, message)| (*offset, message.to_string()))
        .collect();

    assert_eq!(outcome.ok, ok, "return value for '{expr}'");
    assert_eq!(outcome.errors, expected, "error callbacks for '{expr}'");

    match expected.last() {
        Some((offset, message)) => {
            assert_eq!(
                outcome.pending,
                Some((stage, *offset, message.clone())),
                "'{expr}'"
            );
        }
        None => assert_eq!(outcome.pending, None, "'{expr}'"),
    }
}

fn parse_error(expr: &str, expected: &[(i32, &str)]) {
    check("none", expr, false, CompilationStage::Parse, expected);
}

fn codegen_error(expr: &str, expected: &[(i32, &str)]) {
    check("none", expr, true, CompilationStage::Codegen, expected);
}

const MISSING_UNITS: &str = "Missing units for '1'| (must be mm, in, mil, deg, fs, or ps)";

/* --------------------------------------------------------------------------------------------
 * CST_PARSE: the lexer
 */

#[test]
fn unrecognized_character() {
    parse_error("$", &[(0, "Unrecognized character '$'")]);
    parse_error("1 $ 2", &[(1, "Unrecognized character '$'")]);
    parse_error("1 = 1", &[(1, "Unrecognized character '='")]);
    parse_error("1 & 2", &[(1, "Unrecognized character '&'")]);
    parse_error("1 | 2", &[(1, "Unrecognized character '|'")]);
    parse_error("1 % 2", &[(1, "Unrecognized character '%'")]);
    parse_error("\"abc\"", &[(0, "Unrecognized character '\"'")]);

    // Only the space is white space.
    parse_error("1\t2", &[(1, "Unrecognized character '\t'")]);
    parse_error("1\n2", &[(1, "Unrecognized character '\n'")]);

    // "===" is not "==" followed by "=": the two-character operators must not be followed by
    // their last character again.
    parse_error("1 === 1", &[(1, "Unrecognized character '='")]);
    parse_error("1 &&& 2", &[(1, "Unrecognized character '&'")]);
}

/// The C++ code formats the character with `%c` after a cast to `char`: only the low byte of
/// the code point survives, and a byte above 0x7F is printed as `?`.
#[test]
fn unrecognized_non_ascii_character() {
    parse_error("1 \u{e9} 2", &[(1, "Unrecognized character '?'")]);
    parse_error("1 \u{141} 2", &[(1, "Unrecognized character 'A'")]);
    parse_error("1 \u{20ac} 2", &[(1, "Unrecognized character '?'")]);

    // Identifiers are ASCII only, so the error is at the first non-ASCII letter.
    parse_error("Gr\u{f6}\u{df}e", &[(2, "Unrecognized character '?'")]);
}

#[test]
fn unresolved_text_variable() {
    // reported two characters past the token's source position
    parse_error("${x}", &[(2, "Unresolved text variable reference")]);
    parse_error("1 ${", &[(3, "Unresolved text variable reference")]);
    parse_error("${x} == 1", &[(2, "Unresolved text variable reference")]);
    parse_error("$ {x}", &[(0, "Unrecognized character '$'")]);
    parse_error("$$", &[(0, "Unrecognized character '$'")]);

    // Inside a string literal it is just text.
    check(
        "none",
        "A.Str == '${x}'",
        true,
        CompilationStage::Parse,
        &[],
    );
}

/// After a lexical error the lexer still hands an end-of-input token to the parser.  If the
/// input so far is not a complete expression, the parser reports a syntax error on top, at
/// the same source position as the bad character's token, and that is the pending error.
#[test]
fn lexical_error_followed_by_syntax_error() {
    parse_error(
        "1 + ${x}",
        &[
            (5, "Unresolved text variable reference"),
            (3, "Syntax error"),
        ],
    );
    parse_error(
        "1 + $",
        &[(3, "Unrecognized character '$'"), (3, "Syntax error")],
    );
    parse_error(
        "(1 $",
        &[(2, "Unrecognized character '$'"), (2, "Syntax error")],
    );

    // "1" is complete, so there is no second error here.
    parse_error("1 $", &[(1, "Unrecognized character '$'")]);
}

/* --------------------------------------------------------------------------------------------
 * CST_PARSE: the parser
 */

#[test]
fn syntax_errors() {
    // The offset is the source position of the first token that cannot continue the input;
    // for the end of the input that is the position after the last token.
    parse_error("1 +", &[(3, "Syntax error")]);
    parse_error("1 +)", &[(3, "Syntax error")]);
    parse_error("1 +  )", &[(3, "Syntax error")]);
    parse_error("1 2", &[(1, "Syntax error")]);
    parse_error("1   2 ", &[(1, "Syntax error")]);
    parse_error(")(", &[(0, "Syntax error")]);
    parse_error("(", &[(1, "Syntax error")]);
    parse_error("(1", &[(2, "Syntax error")]);
    parse_error("1)", &[(1, "Syntax error")]);
    parse_error("()", &[(1, "Syntax error")]);
    parse_error("*", &[(0, "Syntax error")]);
    parse_error("1 * * 2", &[(3, "Syntax error")]);
    parse_error("a b", &[(1, "Syntax error")]);
    parse_error("'abc' 'def'", &[(5, "Syntax error")]);
    parse_error("A.one(1", &[(7, "Syntax error")]);
    parse_error("A.", &[(2, "Syntax error")]);
    parse_error(".Num", &[(0, "Syntax error")]);
    parse_error("A..Num", &[(2, "Syntax error")]);

    // tokens that no grammar rule uses
    parse_error("1 ; 2", &[(1, "Syntax error")]);
    parse_error(";", &[(0, "Syntax error")]);

    // comparisons are not associative
    parse_error("1 < 2 < 3", &[(5, "Syntax error")]);
    parse_error("1 == 1 == 1", &[(6, "Syntax error")]);

    // without a unit resolver "mm" is an identifier, and two operands in a row are an error
    parse_error("1mm", &[(1, "Syntax error")]);

    // with one, a unit is only allowed directly after a number
    check(
        "pcb",
        "mm",
        false,
        CompilationStage::Parse,
        &[(0, "Syntax error")],
    );
    check(
        "pcb",
        "1mm mm",
        false,
        CompilationStage::Parse,
        &[(3, "Syntax error")],
    );

    // "1, 2" is the number "1," followed by the number "2"
    parse_error("A.add(1, 2)", &[(8, "Syntax error")]);
}

/// Only the first syntax error is reported: `compile` stops at the first error.
#[test]
fn only_the_first_syntax_error_is_reported() {
    parse_error("1 + ) * ( 2 3 4", &[(3, "Syntax error")]);
    parse_error(") ) )", &[(0, "Syntax error")]);
}

/* --------------------------------------------------------------------------------------------
 * CST_CODEGEN
 */

#[test]
fn unrecognized_item() {
    // offset = (position after the name) - (length of the name) = start of the name
    codegen_error("X", &[(0, "Unrecognized item 'X'")]);
    codegen_error("  X", &[(2, "Unrecognized item 'X'")]);
    codegen_error("1 + X", &[(4, "Unrecognized item 'X'")]);
    codegen_error("X.Num", &[(0, "Unrecognized item 'X'")]);
    codegen_error("X  .Num", &[(0, "Unrecognized item 'X'")]);
    codegen_error(
        "X == Y",
        &[(0, "Unrecognized item 'X'"), (5, "Unrecognized item 'Y'")],
    );
    codegen_error("X.one()", &[(0, "Unrecognized item 'X'")]);

    // A navigation chain the binder rejects is reported with the whole dotted name, at
    // (position after the chain) - (length of the dotted name): the start of the chain as
    // long as it contains no white space.
    codegen_error("A.Bogus.Num", &[(0, "Unrecognized item 'A.Bogus'")]);
    codegen_error("L.Parent.Num", &[(0, "Unrecognized item 'L.Parent'")]);
    codegen_error("A.Bogus.self()", &[(0, "Unrecognized item 'A.Bogus'")]);
}

#[test]
fn unrecognized_property() {
    // offset = (position after the property name) - (length of the name)
    codegen_error("A.Bogus", &[(2, "Unrecognized property 'Bogus'")]);
    codegen_error("A.  Bogus", &[(4, "Unrecognized property 'Bogus'")]);
    codegen_error("1 + A.Bogus + 1", &[(6, "Unrecognized property 'Bogus'")]);
    codegen_error("A.Parent.Bogus", &[(9, "Unrecognized property 'Bogus'")]);
    codegen_error(
        "A.Bogus == X.Num",
        &[
            (2, "Unrecognized property 'Bogus'"),
            (11, "Unrecognized item 'X'"),
        ],
    );

    // The right side of "." is neither a name nor a call ("malformed syntax" in the C++
    // code): offset = (position after the item) + 1.
    codegen_error("A.1", &[(2, "Unrecognized property '1'")]);
    codegen_error("A.'x'", &[(2, "Unrecognized property 'x'")]);

    // Parentheses are transparent.
    check("none", "A.(Num)", true, CompilationStage::Codegen, &[]);
    check("none", "(A).Num", true, CompilationStage::Codegen, &[]);
}

#[test]
fn unrecognized_function() {
    // offset = (position after the item) + 1, wherever the function name really starts
    codegen_error("A.unknown()", &[(2, "Unrecognized function 'unknown'")]);
    codegen_error("A.  unknown()", &[(2, "Unrecognized function 'unknown'")]);
    codegen_error("A.Num(1)", &[(2, "Unrecognized function 'Num'")]);
    codegen_error(
        "X.foo(1 , 2)",
        &[
            (0, "Unrecognized item 'X'"),
            (2, "Unrecognized function 'foo'"),
        ],
    );
}

#[test]
fn unknown_parent() {
    // A call without receiver; offset = position after the closing parenthesis.
    codegen_error("foo()", &[(5, "Unknown parent of function parameters")]);
    codegen_error("foo(1)", &[(6, "Unknown parent of function parameters")]);
    codegen_error("1 + foo()", &[(9, "Unknown parent of function parameters")]);
    codegen_error(
        "one(1 , 2)",
        &[(10, "Unknown parent of function parameters")],
    );

    // The receiver of "." is not a chain of names; offset = position after the receiver,
    // minus the length of its text if it is a single token.
    codegen_error("(1+2).Num", &[(5, "Unknown parent of property")]);
    codegen_error("( 1 + 2 ) . Num", &[(9, "Unknown parent of property")]);
    codegen_error("1 . 5", &[(0, "Unknown parent of property")]);
    codegen_error("A.one().one()", &[(7, "Unknown parent of property")]);
    codegen_error("A.self().Num", &[(8, "Unknown parent of property")]);
}

/// Errors a function reports during the compile-time pre-flight call (made when all its
/// arguments are literals) become code generation errors at
/// (position stored in the argument node) - 1.
#[test]
fn preflight_errors() {
    codegen_error("A.isFoo('')", &[(9, "Missing argument to isFoo().")]);
    codegen_error("A.isFoo(  ''  )", &[(11, "Missing argument to isFoo().")]);
    codegen_error("1 + A.isFoo('')", &[(13, "Missing argument to isFoo().")]);

    // No argument at all: the function pops an empty stack first.
    codegen_error(
        "A.isFoo()",
        &[
            (8, "Malformed expression"),
            (8, "Missing argument to isFoo()."),
        ],
    );

    // A function that does not push a result: the compiler's own pop of the result fails.
    codegen_error("A.nopush(1)", &[(9, "Malformed expression")]);

    // "1,2" is one number, so add() misses an argument.
    codegen_error("A.add(1,2)", &[(8, "Malformed expression")]);

    // Each call is checked on its own.
    codegen_error(
        "A.isFoo() == A.isFoo('')",
        &[
            (8, "Malformed expression"),
            (8, "Missing argument to isFoo()."),
            (22, "Missing argument to isFoo()."),
        ],
    );

    // No pre-flight call when an argument is not a literal ...
    check(
        "none",
        "A.isFoo(A.Empty)",
        true,
        CompilationStage::Codegen,
        &[],
    );
    check("none", "A.isFoo(-1)", true, CompilationStage::Codegen, &[]);
    // ... or when the function is unknown.
    codegen_error("A.unknown('')", &[(2, "Unrecognized function 'unknown'")]);
}

/// The pre-flight context is left without error callback and with the values the calls
/// allocated.
#[test]
fn preflight_context_after_compile() {
    let mut compiler = make_compiler("none");
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    preflight.set_error_callback(Some(Box::new(|_message: &str, _offset: i32| {
        panic!("the caller's pre-flight callback is replaced during compile");
    })));

    assert!(compiler.compile("A.isFoo('')", &mut ucode, &mut preflight));
    assert!(compiler.is_error_pending());
    assert!(!preflight.has_error_callback());
    assert_eq!(preflight.sp(), 0);
}

#[test]
fn missing_units() {
    // A single number without unit is reported if the resolver has a units message;
    // offset = position after the number.
    check(
        "pcb",
        "1",
        true,
        CompilationStage::Codegen,
        &[(1, MISSING_UNITS)],
    );
    check(
        "pcb",
        "  1",
        true,
        CompilationStage::Codegen,
        &[(3, MISSING_UNITS)],
    );
    check(
        "pcb",
        "(1)",
        true,
        CompilationStage::Codegen,
        &[(3, MISSING_UNITS)],
    );
    check(
        "pcb",
        "A.Num == 1",
        true,
        CompilationStage::Codegen,
        &[(10, MISSING_UNITS)],
    );

    // Not reported with more than one number (the implicit zero of a unary minus counts) ...
    check("pcb", "1 + 1", true, CompilationStage::Codegen, &[]);
    check("pcb", "1 == 1", true, CompilationStage::Codegen, &[]);
    check("pcb", "-1", true, CompilationStage::Codegen, &[]);
    check("pcb", "1 mm + 2", true, CompilationStage::Codegen, &[]);
    // ... with a unit ...
    check("pcb", "1mm", true, CompilationStage::Codegen, &[]);
    // ... or when the resolver has no message (implied degrees, or no units at all).
    check("pcbdeg", "1", true, CompilationStage::Codegen, &[]);
    check("none", "1", true, CompilationStage::Codegen, &[]);
}

/// Code generation errors do not stop the compilation: microcode with safe placeholders is
/// still produced and `compile` returns true.
#[test]
fn codegen_errors_still_generate_code() {
    let mut compiler = make_compiler("none");
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("1 + X + foo() + A.Bogus", &mut ucode, &mut preflight));
    assert!(compiler.is_error_pending());
    assert_eq!(compiler.error().stage, CompilationStage::Codegen);

    assert_eq!(
        common::normalize_dump(&ucode.dump()),
        "PUSH NUM [1.0000000000]\nPUSH VAR [null]\nADD 515\nPUSH NUM [0.0000000000]\n\
         ADD 515\nPUSH VAR [ref]\nADD 515\n"
    );

    let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());
    let result = ucode.run(&mut ctx);
    assert_eq!(ctx.value(result).as_double(&ctx), 1.0);
}

/* --------------------------------------------------------------------------------------------
 * Reusing a compiler
 */

/// A compiler without errors can be reused.
#[test]
fn compiler_reuse_without_errors() {
    let mut compiler = make_compiler("none");

    for (expr, expected) in [("1 + 2", 3.0), ("3 * 4", 12.0), ("", 1.0), ("5", 5.0)] {
        let mut ucode = Ucode::new(TestBinder::default());
        let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

        assert!(compiler.compile(expr, &mut ucode, &mut preflight), "{expr}");
        assert!(!compiler.is_error_pending());

        let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());
        let result = ucode.run(&mut ctx);
        assert_eq!(ctx.value(result).as_double(&ctx), expected, "{expr}");
    }
}

/// `pendingError` is never reset (as in C++): after any error every further non-empty
/// expression is rejected after its first token, without a new callback, and the error
/// status keeps describing the first failure.  An empty expression still "compiles".
#[test]
fn errors_are_sticky() {
    let errors: RefCell<Vec<(i32, String)>> = RefCell::new(Vec::new());
    let mut compiler = make_compiler("none");

    compiler.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
        errors.borrow_mut().push((offset, message.to_string()));
    })));

    let mut compile = |expr: &str| {
        let mut ucode = Ucode::new(TestBinder::default());
        let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());
        let ok = compiler.compile(expr, &mut ucode, &mut preflight);
        (
            ok,
            compiler.is_error_pending(),
            compiler.error().clone(),
            ucode.dump(),
        )
    };

    let (ok, pending, status, _) = compile("1 +)");
    assert!(!ok && pending);
    assert_eq!(
        (status.src_pos, status.message.as_str()),
        (3, "Syntax error")
    );

    let (ok, pending, status, dump) = compile("2");
    assert!(!ok && pending);
    assert_eq!(
        (status.src_pos, status.message.as_str()),
        (3, "Syntax error")
    );
    assert_eq!(dump, "");

    let (ok, pending, _, dump) = compile("");
    assert!(ok && pending);
    assert_eq!(dump, "PUSH NUM [1.0000000000]\n");

    assert_eq!(*errors.borrow(), vec![(3, "Syntax error".to_string())]);
}

/// A code generation error is sticky as well, although the compile that raised it succeeded.
#[test]
fn codegen_errors_are_sticky() {
    let mut compiler = make_compiler("none");
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("X", &mut ucode, &mut preflight));
    assert!(compiler.is_error_pending());

    let mut ucode = Ucode::new(TestBinder::default());
    assert!(!compiler.compile("1", &mut ucode, &mut preflight));
    assert_eq!(compiler.error().message, "Unrecognized item 'X'");
}

/// Sequences on a failed compiler that are undefined behaviour in C++ (the Lemon parser's
/// stack keeps pointers to tree nodes and strings of the previous expression, which
/// `COMPILER::Clear()` has freed; the reference implementation crashes on some of them).
/// The port must simply keep rejecting input without panicking.
#[test]
fn reuse_after_error_never_panics() {
    let sequences: [&[&str]; 8] = [
        &["(1 $", ")", "3"],
        &["(1 $", ")", "+ 3", "", "4"],
        &["(", "(", "(", "(", "1", ")", ")", ")", ")"],
        &["1 +)", "2", "3"],
        &["1 $ 2", "1"],
        &["A.Str $", "B", ".", "Num", ")", "'x'", "+", "1"],
        &["(((((", "A", ".one(", ")", ")", ")", ")", ")", ")", ")"],
        &["1 + (2 *", ") + 3", "4 5 6 7 8"],
    ];

    for sequence in sequences {
        let mut compiler = make_compiler("pcb");
        let mut first = true;

        for expr in sequence {
            let mut ucode = Ucode::new(TestBinder::default());
            let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());
            let ok = compiler.compile(expr, &mut ucode, &mut preflight);

            assert!(compiler.is_error_pending(), "{sequence:?}");

            if !first {
                assert_eq!(ok, expr.is_empty(), "'{expr}' in {sequence:?}");
            }

            first = false;
        }

        // Repeating until the parser stack has overflowed several times must be fine too.
        for _ in 0..500 {
            let mut ucode = Ucode::new(TestBinder::default());
            let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

            assert!(!compiler.compile("( A", &mut ucode, &mut preflight));
            assert!(!compiler.compile(")", &mut ucode, &mut preflight));
            assert!(!compiler.compile("+", &mut ucode, &mut preflight));
        }
    }
}

/// `Compiler::clear` drops the tree but, as in C++, not the pending error.
#[test]
fn clear_keeps_the_error() {
    let mut compiler = make_compiler("none");
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("A.Num + 1", &mut ucode, &mut preflight));
    assert!(compiler.tree().is_some());

    compiler.clear();
    assert!(compiler.tree().is_none());
    assert_eq!(compiler.format_tree(), "-");

    assert!(!compiler.compile("1 +", &mut ucode, &mut preflight));
    compiler.clear();
    assert!(compiler.is_error_pending());
}
