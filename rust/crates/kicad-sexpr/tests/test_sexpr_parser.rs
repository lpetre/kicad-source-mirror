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

//! Port of `qa/tests/libs/sexpr/test_sexpr_parser.cpp`: test suite `SexprParser` for
//! `SEXPR::PARSER`.

// 3.14 is the test data of the C++ tests, not an approximation of pi.
#![allow(clippy::approx_constant)]

mod sexpr_test_utils;

use kicad_sexpr::{Parser, Sexpr, SexprException};
use sexpr_test_utils::{
    print_type, sexpr_converts_to_string, sexpr_is_double_with_value, sexpr_is_integer_with_value,
    sexpr_is_list_of_length, sexpr_is_string_with_value, sexpr_is_symbol_with_value,
};

/// Port of `TEST_SEXPR_PARSER_FIXTURE`.
struct TestSexprParserFixture {
    parser: Parser,
}

impl TestSexprParserFixture {
    fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    /// Wrap the parser function (C++: with a unique_ptr).
    fn parse(&mut self, input: &str) -> Result<Option<Sexpr>, SexprException> {
        self.parser.parse(input)
    }
}

/// Port of `TEST_SEXPR_CASE`: collection of test cases for use when multiple cases can be
/// handled in the same test case.
struct TestSexprCase {
    case_name: &'static str,
    sexpr_data: &'static str,
}

/// `BOOST_AUTO_TEST_CASE( Empty )`: cases that result in no s-expr object.
#[test]
fn empty() {
    let mut fixture = TestSexprParserFixture::new();

    let cases = [
        TestSexprCase {
            case_name: "Empty string",
            sexpr_data: "",
        },
        TestSexprCase {
            case_name: "whitespace",
            sexpr_data: "  ",
        },
    ];

    for c in &cases {
        let data = fixture.parse(c.sexpr_data).expect(c.case_name);
        assert!(data.is_none(), "{}", c.case_name);
    }
}

/// `BOOST_AUTO_TEST_CASE( Words )`: this comes out as a single symbol.
#[test]
fn words() {
    let mut fixture = TestSexprParserFixture::new();

    let content = "this is just writing";
    let sexp = fixture.parse(content).unwrap();

    let sexp = sexp.expect("BOOST_REQUIRE_NE( sexp.get(), nullptr )");
    assert!(
        sexpr_is_symbol_with_value(&sexp, "this"),
        "{}",
        print_type(&sexp)
    );
}

/// `BOOST_AUTO_TEST_CASE( ParseExceptions )`
#[test]
fn parse_exceptions() {
    let mut fixture = TestSexprParserFixture::new();

    let cases = [
        TestSexprCase {
            case_name: "Unclosed (symbol",
            sexpr_data: "(symbol",
        },
        TestSexprCase {
            case_name: "Comma",
            sexpr_data: ",",
        },
        TestSexprCase {
            case_name: "Int only",
            sexpr_data: "1",
        },
        TestSexprCase {
            case_name: "Double only",
            sexpr_data: "3.14",
        },
        TestSexprCase {
            case_name: "Symbol only",
            sexpr_data: "symbol",
        },
        // { // this is OK for some reason
        //     "String only",
        //     "\"string\"",
        // },
    ];

    for c in &cases {
        // BOOST_CHECK_THROW( Parse( c.m_sexpr_data ), SEXPR::PARSE_EXCEPTION );
        assert!(
            matches!(fixture.parse(c.sexpr_data), Err(SexprException::Parse(_))),
            "{}",
            c.case_name
        );
    }
}

/// `BOOST_AUTO_TEST_CASE( EmptyParens )`: this comes out as an empty list.
#[test]
fn empty_parens() {
    let mut fixture = TestSexprParserFixture::new();

    let content = "()";
    let sexp = fixture.parse(content).unwrap();

    let sexp = sexp.expect("BOOST_REQUIRE_NE( sexp.get(), nullptr )");
    assert!(sexpr_is_list_of_length(&sexp, 0), "{}", print_type(&sexp));
}

/// `BOOST_AUTO_TEST_CASE( SimpleSymbol )`: single symbol in parens.
#[test]
fn simple_symbol() {
    let mut fixture = TestSexprParserFixture::new();

    let content = "(symbol)";
    let sexp = fixture.parse(content).unwrap();

    let sexp = sexp.expect("BOOST_REQUIRE_NE( sexp.get(), nullptr )");
    assert!(sexpr_is_list_of_length(&sexp, 1), "{}", print_type(&sexp));

    let child = sexp.child(0).unwrap();
    assert!(
        sexpr_is_symbol_with_value(child, "symbol"),
        "{}",
        print_type(child)
    );
}

/// `BOOST_AUTO_TEST_CASE( SymbolString )`: test several atoms in a list, including nested
/// lists.
#[test]
fn symbol_string() {
    let mut fixture = TestSexprParserFixture::new();

    let content = "(symbol \"string\" 42 3.14 (nested 4 ()))";
    let sexp = fixture.parse(content).unwrap();

    let sexp = sexp.expect("BOOST_REQUIRE_NE( sexp.get(), nullptr )");
    assert!(sexpr_is_list_of_length(&sexp, 5), "{}", print_type(&sexp));

    assert!(sexpr_is_symbol_with_value(sexp.child(0).unwrap(), "symbol"));
    assert!(sexpr_is_string_with_value(sexp.child(1).unwrap(), "string"));
    assert!(sexpr_is_integer_with_value(sexp.child(2).unwrap(), 42));
    assert!(sexpr_is_double_with_value(sexp.child(3).unwrap(), 3.14));

    // and child list
    let sublist = sexp.child(4).unwrap();
    assert!(
        sexpr_is_list_of_length(sublist, 3),
        "{}",
        print_type(sublist)
    );
    assert!(sexpr_is_symbol_with_value(
        sublist.child(0).unwrap(),
        "nested"
    ));
    assert!(sexpr_is_integer_with_value(sublist.child(1).unwrap(), 4));
    assert!(sexpr_is_list_of_length(sublist.child(2).unwrap(), 0));
}

/// Port of `TEST_SEXPR_ROUNDTRIPPING`: test for roundtripping (valid) s-expression back to
/// strings.
///
/// Note: the whitespace has to be the same in this test.
struct TestSexprRoundtripping {
    case_name: &'static str,
    input: &'static str,
}

/// `BOOST_AUTO_TEST_CASE( StringRoundtrip )`
#[test]
fn string_roundtrip() {
    let mut fixture = TestSexprParserFixture::new();

    let cases = [
        TestSexprRoundtripping {
            case_name: "empty list",
            input: "()",
        },
        TestSexprRoundtripping {
            case_name: "simple list",
            input: "(42 3.14 \"string\")",
        },
        TestSexprRoundtripping {
            case_name: "nested list", // REVIEW space after 42?
            input: "(42 \n  (1 2))",
        },
    ];

    for c in &cases {
        let sexp = fixture
            .parse(c.input)
            .expect(c.case_name)
            .expect(c.case_name);

        assert!(sexpr_converts_to_string(&sexp, c.input), "{}", c.case_name);
    }
}
