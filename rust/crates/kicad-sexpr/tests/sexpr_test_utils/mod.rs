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

//! Port of `qa/tests/libs/sexpr/sexpr_test_utils.h` (namespace `KI_TEST`): predicates on
//! s-expression nodes for the SEXPR tests.
//!
//! Where the C++ predicates log through `BOOST_TEST_MESSAGE` / `BOOST_TEST_INFO`, these print
//! to stderr, which the test harness shows for a failing test.

#![allow(dead_code)]

use kicad_sexpr::{Sexpr, SexprType};

/// Port of `KI_TEST::getType`: get the type of the s-expression.
pub fn get_type(sexpr: &Sexpr) -> SexprType {
    if sexpr.is_list() {
        SexprType::List
    } else if sexpr.is_symbol() {
        SexprType::AtomSymbol
    } else if sexpr.is_string() {
        SexprType::AtomString
    } else if sexpr.is_double() {
        SexprType::AtomDouble
    } else if sexpr.is_integer() {
        SexprType::AtomInteger
    } else {
        panic!("<Unknown S-Expression Type!>");
    }
}

/// Port of `KI_TEST::getDebugType`: get a debug-friendly string for a given s-expr type.
pub fn get_debug_type(type_: SexprType) -> &'static str {
    match type_ {
        SexprType::List => "List",
        SexprType::AtomSymbol => "Symbol",
        SexprType::AtomString => "String",
        SexprType::AtomDouble => "Double",
        SexprType::AtomInteger => "Integer",
    }
}

/// Port of `KI_TEST::GetSexprDebugType`.
pub fn get_sexpr_debug_type(sexpr: &Sexpr) -> &'static str {
    get_debug_type(get_type(sexpr))
}

/// Port of `KI_TEST::IsSexprOfType`.
pub fn is_sexpr_of_type(sexpr: &Sexpr, type_: SexprType) -> bool {
    if get_type(sexpr) != type_ {
        eprintln!("Sexpr is not of type: {}", get_debug_type(type_));
        return false;
    }

    true
}

/// Port of `KI_TEST::IsSexprValueEqual`: predicate to check two s-expr values (of the same
/// type) are equal.
///
/// Returns false if not equal (and output logging).
pub fn is_sexpr_value_equal<T: PartialEq + std::fmt::Debug>(got: &T, expected: &T) -> bool {
    if got != expected {
        eprintln!("Sexpr value not equal: got {got:?}, expected {expected:?}");
        return false;
    }

    true
}

/// Port of `KI_TEST::SexprIsSymbol`: test predicate: is the s-expression a symbol?
pub fn sexpr_is_symbol(sexpr: &Sexpr) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomSymbol)
}

/// Port of `KI_TEST::SexprIsSymbolWithValue`: test predicate: is the s-expression a symbol with
/// the given value?
pub fn sexpr_is_symbol_with_value(sexpr: &Sexpr, val: &str) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomSymbol)
        && is_sexpr_value_equal(&sexpr.symbol().expect("a symbol"), &val)
}

/// Port of `KI_TEST::SexprIsString`: test predicate: is the s-expression a string?
pub fn sexpr_is_string(sexpr: &Sexpr) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomString)
}

/// Port of `KI_TEST::SexprIsStringWithValue`: test predicate: is the s-expression a string with
/// the given value?
pub fn sexpr_is_string_with_value(sexpr: &Sexpr, val: &str) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomString)
        && is_sexpr_value_equal(&sexpr.string().expect("a string"), &val)
}

/// Port of `KI_TEST::SexprIsInteger`: test predicate: is the s-expression an integer?
pub fn sexpr_is_integer(sexpr: &Sexpr) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomInteger)
}

/// Port of `KI_TEST::SexprIsIntegerWithValue`: test predicate: is the s-expression an integer
/// with the given value?
pub fn sexpr_is_integer_with_value(sexpr: &Sexpr, val: i64) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomInteger)
        && is_sexpr_value_equal(&sexpr.long_integer().expect("an integer"), &val)
}

/// Port of `KI_TEST::SexprIsDouble`: test predicate: is the s-expression a double?
pub fn sexpr_is_double(sexpr: &Sexpr) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomDouble)
}

/// Port of `KI_TEST::SexprIsDoubleWithValue`: test predicate: is the s-expression a double with
/// the given value?
pub fn sexpr_is_double_with_value(sexpr: &Sexpr, val: f64) -> bool {
    is_sexpr_of_type(sexpr, SexprType::AtomDouble)
        && is_sexpr_value_equal(&sexpr.double().expect("a double"), &val)
}

/// Port of `KI_TEST::SexprIsList`: test predicate: is the s-expression a list?
pub fn sexpr_is_list(sexpr: &Sexpr) -> bool {
    is_sexpr_of_type(sexpr, SexprType::List)
}

/// Port of `KI_TEST::SexprIsListOfLength`: test predicate: is the s-expression a list with the
/// given length?
pub fn sexpr_is_list_of_length(sexpr: &Sexpr, expected_length: usize) -> bool {
    if !is_sexpr_of_type(sexpr, SexprType::List) {
        return false;
    }

    let length = sexpr.number_of_children().expect("a list");

    if length != expected_length {
        eprintln!("List is wrong length: got {length}, expected {expected_length}");
        return false;
    }

    true
}

/// Port of `KI_TEST::SexprConvertsToString`: predicate to check an SEXPR object converts to the
/// expected string.
///
/// Returns true if match.
pub fn sexpr_converts_to_string(sexpr: &Sexpr, exp_str: &str) -> bool {
    let converted = sexpr.as_string(0);

    let mut ok = true;

    if converted != exp_str {
        eprintln!("Sexpr string conversion mismatch: got '{converted}', expected '{exp_str}'");
        ok = false;
    }

    ok
}

/// Port of `SEXPR::boost_test_print_type`: the text Boost prints for an SEXPR object.
pub fn print_type(sexpr: &Sexpr) -> String {
    format!(
        "SEXPR [ {} ]\n    {}",
        get_sexpr_debug_type(sexpr),
        sexpr.as_string(0)
    )
}
