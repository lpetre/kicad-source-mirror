// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2016 Mark Roszko <mark.roszko@gmail.com>
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

//! Port of `libs/sexpr/include/sexpr/sexpr_parser.h` and `libs/sexpr/sexpr_parser.cpp`:
//! `SEXPR::PARSER`, which reads text into a [`Sexpr`] tree.
//!
//! This parser is independent of [`DsnLexer`](crate::lexer::dsnlexer::DsnLexer) and has its
//! own, simpler rules: strings keep their escape sequences, an atom is a number if it consists
//! of digits and dots (optionally after a minus sign), and an atom must be followed by white
//! space or a parenthesis.
//!
//! `PARSER::ParseFromFile` and `PARSER::GetFileContents` read a file and are not ported (no
//! file system in library crates): read the file and call [`Parser::parse`].

use crate::lexer::c_numeric::{strtod_prefix, strtoll};
use crate::tree::sexpr::{Sexpr, SexprDouble, SexprInteger, SexprList, SexprString, SexprSymbol};
use crate::tree::sexpr_exception::{ParseException, SexprException};

/// Port of `PARSER::whitespaceCharacters`: `" \t\n\r\b\f\v"`.
const WHITESPACE_CHARACTERS: &[u8] = b" \t\n\r\x08\x0c\x0b";

fn is_whitespace(c: u8) -> bool {
    WHITESPACE_CHARACTERS.contains(&c)
}

/// Port of `SEXPR::PARSER`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parser {
    /// C++ `m_lineNumber`.  It is not reset between calls of [`parse`](Self::parse).
    line_number: i32,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Port of `PARSER::PARSER`.
    pub fn new() -> Self {
        Self { line_number: 1 }
    }

    /// Port of `PARSER::Parse`: parse the first s-expression in `string`.
    ///
    /// Returns `Ok(None)` (C++ `nullptr`) if `string` is empty, only white space, or starts
    /// with a closing parenthesis.  Text behind the first expression is ignored.  A list that
    /// is still open at the end of the text is returned as it is, without an error.
    ///
    /// # Errors
    ///
    /// `PARSE_EXCEPTION( "missing closing quote" )` for an unterminated string and
    /// `PARSE_EXCEPTION( "format error" )` for an atom that runs up to the end of the text
    /// (which includes every text that is just a number or a symbol).
    pub fn parse(&mut self, string: &str) -> Result<Option<Sexpr>, SexprException> {
        // PARSER::parseString is recursive in C++ (one level per nested list).  The port keeps
        // the open lists on an explicit stack instead, so that deeply nested input cannot
        // overflow the call stack; the order of all operations is the same.
        let text = string.as_bytes();
        let end = text.len();
        let mut it = 0;

        // The top level call of parseString: skip white space up to the first item.
        loop {
            if it == end {
                return Ok(None);
            }

            if text[it] == b'\n' {
                self.line_number += 1;
            }

            if is_whitespace(text[it]) {
                it += 1;
                continue;
            }

            break;
        }

        if text[it] == b')' {
            return Ok(None);
        }

        let mut open_lists: Vec<SexprList> = Vec::new();

        loop {
            // One call of parseString with `it` at a character that is neither white space nor
            // a closing parenthesis.
            let mut item: Option<Sexpr> = None;

            if text[it] == b'(' {
                it += 1;

                open_lists.push(SexprList::with_line_number(self.line_number));
            } else if text[it] == b'"' {
                it += 1;

                let starting_it = it;

                while it != end {
                    let ch = text[it];

                    if ch == b'\\' {
                        // Skip the next escaped character
                        it += 1;

                        if it == end {
                            break;
                        }

                        it += 1;
                        continue;
                    }

                    if ch == b'"' {
                        break;
                    }

                    it += 1;
                }

                if it == end {
                    return Err(ParseException::new("missing closing quote").into());
                }

                item = Some(
                    SexprString::with_line_number(
                        String::from_utf8_lossy(&text[starting_it..it]),
                        self.line_number,
                    )
                    .into(),
                );

                it += 1;
            } else {
                let start_pos = it;
                let closing_pos = text[start_pos..]
                    .iter()
                    .position(|&c| is_whitespace(c) || c == b'(' || c == b')')
                    .map(|offset| start_pos + offset);

                let Some(closing_pos) = closing_pos else {
                    return Err(ParseException::new("format error").into());
                };

                let tmp = &text[start_pos..closing_pos];
                let is_digits_or_dots =
                    |bytes: &[u8]| bytes.iter().all(|&c| c.is_ascii_digit() || c == b'.');

                if is_digits_or_dots(tmp)
                    || (tmp.len() > 1 && tmp[0] == b'-' && is_digits_or_dots(&tmp[1..]))
                {
                    if tmp.contains(&b'.') {
                        // floating point type. Use only a "C" string to double conversion
                        //
                        // wxString::ToCDouble() stores what strtod() converts from the start of
                        // the text, even if not all of it is a number ("1.2.3" gives 1.2).  If
                        // nothing can be converted (".", "-.", "..1") or the value is out of
                        // range, C++ leaves `fnumb` uninitialised; the port uses 0 resp. the
                        // saturated value there.
                        let fnumb = strtod_prefix(tmp).map_or(0.0, |(value, _)| value);

                        item = Some(SexprDouble::with_line_number(fnumb, self.line_number).into());
                    } else {
                        item = Some(
                            SexprInteger::with_line_number(strtoll(tmp, 0).0, self.line_number)
                                .into(),
                        );
                    }
                } else {
                    item = Some(
                        SexprSymbol::with_line_number(
                            String::from_utf8_lossy(tmp),
                            self.line_number,
                        )
                        .into(),
                    );
                }

                it = closing_pos;
            }

            // Hand a finished item to the list it belongs to, then continue the loop over the
            // children of the innermost open list until the next child starts.
            loop {
                match (item.take(), open_lists.last_mut()) {
                    (Some(finished), Some(list)) => list.add_child(finished),
                    // The outermost call of parseString returns its item.
                    (finished, None) => return Ok(finished),
                    (None, Some(_)) => {}
                }

                // while( it != aString.end() && *it != ')' )
                while it != end && text[it] != b')' {
                    //there may be newlines in between atoms of a list, so detect these here
                    if text[it] == b'\n' {
                        self.line_number += 1;
                    }

                    if is_whitespace(text[it]) {
                        it += 1;
                        continue;
                    }

                    break;
                }

                if it != end && text[it] != b')' {
                    break; // the next child starts here
                }

                if it != end {
                    it += 1;
                }

                // return list;
                item = open_lists.pop().map(Sexpr::from);
            }
        }
    }
}

#[cfg(test)]
// 3.14 is test data here (as in the C++ tests), not an approximation of pi.
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Option<Sexpr>, SexprException> {
        Parser::new().parse(text)
    }

    fn parse_ok(text: &str) -> Sexpr {
        parse(text).unwrap().unwrap()
    }

    #[test]
    fn nothing_to_parse() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse(" \t\r\n\x08\x0c\x0b").unwrap(), None);
        assert_eq!(parse(")").unwrap(), None);
        assert_eq!(parse("  ) (a)").unwrap(), None);
    }

    #[test]
    fn atoms_need_a_delimiter() {
        for text in ["symbol", "1", "3.14", ",", "(symbol", "(a (b", "(1"] {
            assert_eq!(parse(text).unwrap_err().what(), "format error", "{text:?}");
        }

        // With a delimiter behind it a lone atom is fine, and the rest is ignored.
        assert_eq!(parse_ok("symbol ").symbol().unwrap(), "symbol");
        assert_eq!(parse_ok("12 13").long_integer().unwrap(), 12);
        assert_eq!(parse_ok("1.5(").double().unwrap(), 1.5);
        assert_eq!(parse_ok("a)").symbol().unwrap(), "a");
    }

    #[test]
    fn unterminated_lists_are_accepted() {
        assert_eq!(parse_ok("(").as_string(0), "()");
        assert_eq!(parse_ok("(a ").as_string(0), "(a)");
        assert_eq!(parse_ok("(a (b ").as_string(0), "(a \n  (b))");
        assert_eq!(parse_ok("(a \"s\"").as_string(0), "(a \"s\")");
    }

    #[test]
    fn strings_keep_their_escapes() {
        assert_eq!(parse_ok("\"string\"").string().unwrap(), "string");
        assert_eq!(parse_ok("\"\"").string().unwrap(), "");
        assert_eq!(
            parse_ok("\"a \\\"b\\\" \\\\ (c)\"").string().unwrap(),
            "a \\\"b\\\" \\\\ (c)"
        );
        assert_eq!(
            parse_ok("(\"a\nb\" x)").child(0).unwrap().string().unwrap(),
            "a\nb"
        );

        for text in ["\"abc", "\"abc\\", "\"abc\\\"", "(a \"b"] {
            assert_eq!(
                parse(text).unwrap_err().what(),
                "missing closing quote",
                "{text:?}"
            );
        }
    }

    #[test]
    fn number_classification() {
        let list = parse_ok("(42 -42 3.14 -3.14 .5 5. -.5 1.2.3 007 08 -012 - -a 1e5 +1 1,5 a1)");
        let children = list.children().unwrap();

        assert_eq!(children[0].long_integer().unwrap(), 42);
        assert_eq!(children[1].long_integer().unwrap(), -42);
        assert_eq!(children[2].double().unwrap(), 3.14);
        assert!(children[2].is_double());
        assert_eq!(children[3].double().unwrap(), -3.14);
        assert_eq!(children[4].double().unwrap(), 0.5);
        assert_eq!(children[5].double().unwrap(), 5.0);
        assert!(children[5].is_double());
        assert_eq!(children[6].double().unwrap(), -0.5);
        // strtod() stops at the second dot
        assert_eq!(children[7].double().unwrap(), 1.2);
        // strtoll( ..., 0 ): a leading zero means octal
        assert_eq!(children[8].long_integer().unwrap(), 7);
        assert_eq!(children[9].long_integer().unwrap(), 0);
        assert_eq!(children[10].long_integer().unwrap(), -10);

        for (index, symbol) in [
            (11, "-"),
            (12, "-a"),
            (13, "1e5"),
            (14, "+1"),
            (15, "1,5"),
            (16, "a1"),
        ] {
            assert_eq!(children[index].symbol().unwrap(), symbol);
        }
    }

    #[test]
    fn degenerate_numbers() {
        let list = parse_ok("(. -. .. 99999999999999999999 -99999999999999999999 )");
        let children = list.children().unwrap();

        // Nothing strtod() can convert: indeterminate in C++, 0 here.
        assert_eq!(children[0].double().unwrap(), 0.0);
        assert_eq!(children[1].double().unwrap(), 0.0);
        assert_eq!(children[2].double().unwrap(), 0.0);
        // strtoll() clamps
        assert_eq!(children[3].long_integer().unwrap(), i64::MAX);
        assert_eq!(children[4].long_integer().unwrap(), i64::MIN);
    }

    #[test]
    fn line_numbers() {
        let mut parser = Parser::new();
        let list = parser
            .parse("\n(a\n  (b \"x\ny\"\n c)\n\n 5)\n")
            .unwrap()
            .unwrap();

        assert_eq!(list.line_number(), 2);
        assert_eq!(list.child(0).unwrap().line_number(), 2);

        let inner = list.child(1).unwrap();
        assert_eq!(inner.line_number(), 3);
        assert_eq!(inner.child(0).unwrap().line_number(), 3);
        assert_eq!(inner.child(1).unwrap().line_number(), 3);
        // the line break inside the string is not counted
        assert_eq!(inner.child(2).unwrap().line_number(), 4);
        assert_eq!(list.child(2).unwrap().line_number(), 6);

        // The parser keeps counting across calls.
        let next = parser.parse("(z)").unwrap().unwrap();
        assert_eq!(next.line_number(), 6);
    }

    #[test]
    fn nested_lists_round_trip() {
        let text = "(a \n  (b \n    (c 1 2.5 \"s\") \n    ()) d)";

        assert_eq!(parse_ok(text).as_string(0), text);
    }

    #[test]
    fn non_ascii_text() {
        let list = parse_ok("(µm \"Ω é\" ü)");

        assert_eq!(list.child(0).unwrap().symbol().unwrap(), "µm");
        assert_eq!(list.child(1).unwrap().string().unwrap(), "Ω é");
        assert_eq!(list.child(2).unwrap().symbol().unwrap(), "ü");
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let depth = 300_000;
        let mut text = "(".repeat(depth);
        text.push_str(&")".repeat(depth));

        let mut node = &parse_ok(&text);
        let mut levels = 1;

        while let Ok(child) = node.child(0) {
            node = child;
            levels += 1;
        }

        assert_eq!(levels, depth);
    }
}
