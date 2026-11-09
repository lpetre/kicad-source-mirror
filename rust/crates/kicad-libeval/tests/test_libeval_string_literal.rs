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

//! Port of `qa/tests/common/libeval/test_libeval_string_literal.cpp` (test suite
//! `LibevalStringLiteral`), plus a few more string literal cases.

use kicad_libeval::{Compiler, Context, DefaultBinder, Ucode, VarType, escape_string_literal};

/// Compile `'<literal>'` and return the string it evaluates to.
fn eval_string_literal(literal: &str) -> String {
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut context = Context::new(());

    assert!(
        compiler.compile(&format!("'{literal}'"), &mut code, &mut context),
        "{literal}"
    );

    let result = code.run(&mut context);
    assert_eq!(context.value(result).type_(), VarType::String);
    context.value(result).as_string(&context).to_string()
}

/// `BOOST_AUTO_TEST_CASE( EscapedTextCompilesBackUnchanged )`
#[test]
fn escaped_text_compiles_back_unchanged() {
    for text in ["it's", "a\\'b", "a\\b"] {
        let literal = escape_string_literal(text);
        assert!(literal.is_some(), "{text}");

        assert_eq!(eval_string_literal(&literal.unwrap()), text);
    }

    assert!(escape_string_literal("BPECL2\\").is_none());
}

/* --------------------------------------------------------------------------------------------
 * Additional tests (not in the C++ suite)
 */

#[test]
fn escape_string_literal_only_escapes_quotes() {
    assert_eq!(escape_string_literal("").as_deref(), Some(""));
    assert_eq!(escape_string_literal("plain").as_deref(), Some("plain"));
    assert_eq!(escape_string_literal("it's").as_deref(), Some("it\\'s"));
    assert_eq!(escape_string_literal("''").as_deref(), Some("\\'\\'"));
    assert_eq!(escape_string_literal("a\\b").as_deref(), Some("a\\b"));
    assert_eq!(escape_string_literal("\\").as_deref(), None);
    assert_eq!(escape_string_literal("a\\\\").as_deref(), None);
}

/// The tokenizer's only escape is `\'`; every other backslash is literal.
#[test]
fn string_literal_escapes() {
    assert_eq!(eval_string_literal(""), "");
    assert_eq!(eval_string_literal("abc"), "abc");
    assert_eq!(eval_string_literal("a b  c"), "a b  c");
    assert_eq!(eval_string_literal("\\'"), "'");
    assert_eq!(eval_string_literal("a\\nb"), "a\\nb");
    assert_eq!(eval_string_literal("a\\\\b"), "a\\\\b");
    assert_eq!(eval_string_literal("\\'\\'"), "''");
    assert_eq!(eval_string_literal("1 + 2"), "1 + 2");
    assert_eq!(eval_string_literal("${VAR}"), "${VAR}");
    assert_eq!(
        eval_string_literal("\u{fc}n\u{ef}c\u{f6}d\u{e9}"),
        "\u{fc}n\u{ef}c\u{f6}d\u{e9}"
    );
}

/// A missing closing quote ends the literal at the end of the input.
#[test]
fn unterminated_string_literal() {
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut context = Context::new(());

    assert!(compiler.compile("'abc", &mut code, &mut context));
    assert!(!compiler.is_error_pending());

    let result = code.run(&mut context);
    assert_eq!(context.value(result).as_string(&context), "abc");
}

/// `?` and `*` make a literal a wildcard pattern; a literal pushed on the stack is not
/// flagged as "literal" (see `Uop::exec`).
#[test]
fn string_literal_flags() {
    for (expr, wildcard) in [
        ("'abc'", false),
        ("'a*'", true),
        ("'a?c'", true),
        ("'*'", true),
    ] {
        let mut compiler = Compiler::new();
        let mut code = Ucode::new(DefaultBinder);
        let mut context = Context::new(());

        assert!(compiler.compile(expr, &mut code, &mut context));

        let result = code.run(&mut context);
        let value = context.value(result);
        assert_eq!(value.string_is_wildcard(), wildcard, "{expr}");
        assert!(!value.string_is_literal(), "{expr}");
    }
}
