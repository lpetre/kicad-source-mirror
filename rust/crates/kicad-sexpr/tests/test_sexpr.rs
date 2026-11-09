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

//! Port of `qa/tests/libs/sexpr/test_sexpr.cpp`: test suite `Sexpr` for the SEXPR tree.

// 3.14 is the test data of the C++ tests, not an approximation of pi.
#![allow(clippy::approx_constant)]

mod sexpr_test_utils;

use kicad_sexpr::{Sexpr, SexprDouble, SexprInteger, SexprList, SexprString, SexprSymbol};
use sexpr_test_utils::{
    print_type, sexpr_converts_to_string, sexpr_is_double_with_value, sexpr_is_integer_with_value,
    sexpr_is_list_of_length, sexpr_is_string_with_value,
};

/// `BOOST_AUTO_TEST_CASE( BasicConstruction )`
#[test]
fn basic_construction() {
    let s_int: Sexpr = SexprInteger::new(1).into();
    assert!(
        sexpr_is_integer_with_value(&s_int, 1),
        "{}",
        print_type(&s_int)
    );

    let s_double: Sexpr = SexprDouble::new(3.14).into();
    assert!(
        sexpr_is_double_with_value(&s_double, 3.14),
        "{}",
        print_type(&s_double)
    );

    let s_string: Sexpr = SexprString::new("string").into();
    assert!(
        sexpr_is_string_with_value(&s_string, "string"),
        "{}",
        print_type(&s_string)
    );

    // (the C++ test also uses a SEXPR_STRING here)
    let s_symbol: Sexpr = SexprString::new("symbol").into();
    assert!(
        sexpr_is_string_with_value(&s_symbol, "symbol"),
        "{}",
        print_type(&s_symbol)
    );
}

/// `BOOST_AUTO_TEST_CASE( AsStringInt )`
#[test]
fn as_string_int() {
    let s: Sexpr = SexprInteger::new(1).into();
    assert!(sexpr_converts_to_string(&s, "1"));
}

/// `BOOST_AUTO_TEST_CASE( AsStringDouble )`
#[test]
fn as_string_double() {
    let s: Sexpr = SexprDouble::new(3.14).into();
    assert!(sexpr_converts_to_string(&s, "3.14"));
}

/// `BOOST_AUTO_TEST_CASE( AsStringSymbol )`
#[test]
fn as_string_symbol() {
    let s: Sexpr = SexprSymbol::new("symbol").into();
    assert!(sexpr_converts_to_string(&s, "symbol"));
}

/// `BOOST_AUTO_TEST_CASE( AsStringString )`
#[test]
fn as_string_string() {
    let s: Sexpr = SexprString::new("string").into();

    // strings get quotes
    assert!(sexpr_converts_to_string(&s, "\"string\""));
}

/// `BOOST_AUTO_TEST_CASE( AsStringList )`
#[test]
fn as_string_list() {
    let mut s_list = SexprList::new();

    s_list.add_child(SexprSymbol::new("symbol"));

    {
        let mut s_sub_list = SexprList::new();
        // *s_SubList << 2 << 42.42 << "substring";
        s_sub_list.push(2).push(42.42).push("substring");
        s_list.add_child(s_sub_list);
    }

    // s_list << 1 << 3.14 << "string";
    s_list.push(1).push(3.14).push("string");

    let s_list: Sexpr = s_list.into();

    assert!(
        sexpr_is_list_of_length(&s_list, 5),
        "{}",
        print_type(&s_list)
    );

    // REVIEW: should there be a space at the end of the "symbol"?
    assert!(sexpr_converts_to_string(
        &s_list,
        "(symbol \n  (2 42.42 \"substring\") 1 3.14 \"string\")"
    ));
}
