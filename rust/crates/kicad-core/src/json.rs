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
//
// Parts of this file port the bundled nlohmann/json 3.11.3
// (`thirdparty/nlohmann_json/nlohmann/json.hpp`):
// SPDX-FileCopyrightText: 2013-2023 Niels Lohmann <https://nlohmann.me>
// SPDX-License-Identifier: MIT

//! nlohmann/json compatible reading and writing on top of `serde_json::Value`.
//!
//! KiCad reads and writes `.kicad_pro`, `.kicad_prl`, DRC JSON reports and `.gbrjob` files with
//! the bundled nlohmann/json 3.11.3 (`include/json_common.h`). This module reproduces the parts
//! whose behaviour is visible in files or in settings values:
//!
//! - [`parse`]: port of nlohmann's lexer/parser as KiCad calls it
//!   (`nlohmann::json::parse( text, nullptr, ALLOW_EXCEPTIONS, IGNORE_COMMENTS )`): UTF-8 BOM,
//!   `//` and `/* */` comments, strict UTF-8 validation, nlohmann's number classification
//!   (`-0` is the integer 0, integers that overflow `int64`/`uint64` become doubles), duplicate
//!   keys (the last value wins, at the position of the first).
//! - [`dump`]: port of `basic_json::dump( indent )` / `operator<<` with `std::setw`, including
//!   nlohmann's string escaping (`ensure_ascii = false`) and its Grisu2 double printing
//!   ([`to_chars`]).
//! - [`ObjectOrder`]: `nlohmann::json` (objects are `std::map`, sorted by key bytes) versus
//!   `nlohmann::ordered_json` (insertion order). `serde_json` is built with `preserve_order`, so
//!   a [`Value`] always keeps insertion order and sorting is applied where C++ uses
//!   `nlohmann::json`.
//! - [`JsonPointer`]: `nlohmann::json_pointer` (`contains`, `at`, `operator[]`).
//! - [`FromJson`] / [`ToJson`]: the `get<T>()` conversions of nlohmann for the types KiCad's
//!   settings parameters use, with the same accept/throw rules.
//! - [`json_eq`], [`update`]: `operator==` and `update( ..., merge_objects )`.

mod to_chars;

pub use serde_json::{Map, Number, Value};

use std::collections::BTreeMap;
use std::fmt;

/// `nlohmann::detail::to_chars` (Grisu2 shortest-ish double printing) as used by `dump()`.
///
/// Appends the text of the finite `value` to `out` (`0.0`, `-0.0`, `1.5`, `1e+23`, ...).
pub fn to_chars(out: &mut Vec<u8>, value: f64) {
    to_chars::to_chars(out, value);
}

/// The text `dump()` writes for a double: [`to_chars`] for finite values, `null` otherwise
/// (`serializer::dump_float`).
pub fn dump_float(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }

    let mut out = Vec::with_capacity(32);
    to_chars::to_chars(&mut out, value);
    String::from_utf8(out).unwrap_or_default()
}

// ------------------------------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------------------------------

/// Kind of a nlohmann exception (`json.exception.<kind>.<id>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonErrorKind {
    /// `parse_error` (ids 101-115).
    ParseError,
    /// `type_error` (ids 301-318).
    TypeError,
    /// `out_of_range` (ids 401-410).
    OutOfRange,
}

/// A nlohmann exception: KiCad catches these (`catch( ... )` or `catch( parse_error& )`), so
/// the port returns them as errors instead of throwing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// Exception class.
    pub kind: JsonErrorKind,
    /// nlohmann error id (`error.id`, e.g. 101 for a syntax error, 302 for a type mismatch).
    pub id: i32,
    /// For parse errors: number of bytes read when the error was detected (`error.byte`).
    pub byte: usize,
    /// Message without the `[json.exception...]` prefix.
    pub message: String,
}

impl JsonError {
    fn new(kind: JsonErrorKind, id: i32, message: String) -> Self {
        Self {
            kind,
            id,
            byte: 0,
            message,
        }
    }

    /// `type_error::create( 302, "type must be <what>, but is <type>" )`.
    pub fn type_must_be(what: &str, value: &Value) -> Self {
        Self::new(
            JsonErrorKind::TypeError,
            302,
            format!("type must be {what}, but is {}", type_name(value)),
        )
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            JsonErrorKind::ParseError => "parse_error",
            JsonErrorKind::TypeError => "type_error",
            JsonErrorKind::OutOfRange => "out_of_range",
        };

        write!(f, "[json.exception.{kind}.{}] {}", self.id, self.message)
    }
}

impl std::error::Error for JsonError {}

/// Port of `basic_json::type_name()`.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
    }
}

// ------------------------------------------------------------------------------------------
// Number classification
// ------------------------------------------------------------------------------------------

/// nlohmann's three number types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JsonNumber {
    /// `number_integer` (`int64_t`).
    Integer(i64),
    /// `number_unsigned` (`uint64_t`).
    Unsigned(u64),
    /// `number_float` (`double`).
    Float(f64),
}

/// Classifies a `serde_json` number the way nlohmann stores a parsed number: non-negative
/// integers are `number_unsigned`, negative ones `number_integer`, everything else
/// `number_float`.
///
/// nlohmann also has non-negative `number_integer` values (created from C++ `int`s); `serde_json`
/// cannot represent the difference. It is invisible in the output (both dump as decimal
/// integers) and in comparisons; see `PORT_STATUS.md`.
pub fn classify_number(n: &Number) -> JsonNumber {
    if let Some(u) = n.as_u64() {
        JsonNumber::Unsigned(u)
    } else if let Some(i) = n.as_i64() {
        JsonNumber::Integer(i)
    } else {
        JsonNumber::Float(n.as_f64().unwrap_or(0.0))
    }
}

/// `is_number_integer()`: true for both integer kinds (nlohmann's definition).
pub fn is_number_integer(v: &Value) -> bool {
    matches!(v, Value::Number(n) if !n.is_f64())
}

/// `is_number_unsigned()`.
pub fn is_number_unsigned(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_u64())
}

/// `is_number_float()`.
pub fn is_number_float(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_f64())
}

/// A JSON value from a double the way nlohmann stores it (`number_float`). nlohmann keeps
/// NaN/infinity and dumps them as `null`; `serde_json` cannot hold them, so they become `null`
/// directly (identical `dump()` output).
pub fn float_value(v: f64) -> Value {
    Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

// ------------------------------------------------------------------------------------------
// Parser: port of nlohmann::detail::lexer / parser (strict, ignore_comments)
// ------------------------------------------------------------------------------------------

/// Object key order of the document type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectOrder {
    /// `nlohmann::json`: objects are `std::map<std::string, json>`, iterated in byte order of
    /// the UTF-8 keys.
    Sorted,
    /// `nlohmann::ordered_json`: objects keep insertion order.
    Insertion,
}

/// Maximum nesting depth accepted by [`parse`]. nlohmann has no limit; the port limits the
/// depth so that dropping a pathological document cannot overflow the stack (`PORT_STATUS.md`).
pub const MAX_PARSE_DEPTH: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    True,
    False,
    Null,
    Str,
    Num,
    BeginArray,
    BeginObject,
    EndArray,
    EndObject,
    NameSeparator,
    ValueSeparator,
    EndOfInput,
}

impl Token {
    /// `lexer::token_type_name`.
    fn name(self) -> &'static str {
        match self {
            Token::True => "'true'",
            Token::False => "'false'",
            Token::Null => "'null'",
            Token::Str => "string literal",
            Token::Num => "number literal",
            Token::BeginArray => "'['",
            Token::BeginObject => "'{'",
            Token::EndArray => "']'",
            Token::EndObject => "'}'",
            Token::NameSeparator => "':'",
            Token::ValueSeparator => "','",
            Token::EndOfInput => "end of input",
        }
    }
}

struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    ignore_comments: bool,
    // Value of the last string / number token.
    string: String,
    number: Value,
    // Byte position (1-based "chars read") of the last token start, for error reports.
    token_start: usize,
    line: usize,
    line_start: usize,
}

type LexResult = Result<Token, String>;

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;

        if c == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }

        Some(c)
    }

    /// Port of `lexer::skip_whitespace`.
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    /// Port of `lexer::scan_comment` (current character `/` already consumed).
    fn scan_comment(&mut self) -> Result<(), String> {
        match self.bump() {
            Some(b'/') => loop {
                match self.bump() {
                    Some(b'\n' | b'\r') | None => return Ok(()),
                    _ => {}
                }
            },
            Some(b'*') => loop {
                match self.bump() {
                    None => return Err("invalid comment; missing closing '*/'".into()),
                    Some(b'*') if self.peek() == Some(b'/') => {
                        self.bump();
                        return Ok(());
                    }
                    _ => {}
                }
            },
            _ => Err("invalid comment; expecting '/' or '*' after '/'".into()),
        }
    }

    /// Port of `lexer::scan`.
    fn scan(&mut self) -> LexResult {
        self.skip_whitespace();

        while self.ignore_comments && self.peek() == Some(b'/') {
            self.bump();
            self.scan_comment()?;
            self.skip_whitespace();
        }

        self.token_start = self.pos;

        let Some(c) = self.peek() else {
            return Ok(Token::EndOfInput);
        };

        match c {
            b'[' => {
                self.bump();
                Ok(Token::BeginArray)
            }
            b']' => {
                self.bump();
                Ok(Token::EndArray)
            }
            b'{' => {
                self.bump();
                Ok(Token::BeginObject)
            }
            b'}' => {
                self.bump();
                Ok(Token::EndObject)
            }
            b':' => {
                self.bump();
                Ok(Token::NameSeparator)
            }
            b',' => {
                self.bump();
                Ok(Token::ValueSeparator)
            }
            b't' => self.scan_literal(b"true", Token::True),
            b'f' => self.scan_literal(b"false", Token::False),
            b'n' => self.scan_literal(b"null", Token::Null),
            b'"' => {
                self.bump();
                self.scan_string()
            }
            b'-' | b'0'..=b'9' => self.scan_number(),
            0 => {
                self.bump();
                Err("invalid literal".into())
            }
            _ => {
                self.bump();
                Err("invalid literal".into())
            }
        }
    }

    fn scan_literal(&mut self, lit: &[u8], tok: Token) -> LexResult {
        for &expected in lit {
            if self.bump() != Some(expected) {
                return Err("invalid literal".into());
            }
        }

        Ok(tok)
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut cp = 0u32;

        for _ in 0..4 {
            let d = match self.bump()? {
                c @ b'0'..=b'9' => c - b'0',
                c @ b'a'..=b'f' => c - b'a' + 10,
                c @ b'A'..=b'F' => c - b'A' + 10,
                _ => return None,
            };
            cp = (cp << 4) | u32::from(d);
        }

        Some(cp)
    }

    /// Port of `lexer::scan_string` (opening quote consumed). Validates UTF-8 exactly like
    /// nlohmann (RFC 3629 byte ranges) and decodes escapes including surrogate pairs.
    fn scan_string(&mut self) -> LexResult {
        let mut buf: Vec<u8> = Vec::new();

        loop {
            let Some(c) = self.bump() else {
                return Err("invalid string: missing closing quote".into());
            };

            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.bump();
                    match e {
                        Some(b'"') => buf.push(b'"'),
                        Some(b'\\') => buf.push(b'\\'),
                        Some(b'/') => buf.push(b'/'),
                        Some(b'b') => buf.push(0x08),
                        Some(b'f') => buf.push(0x0C),
                        Some(b'n') => buf.push(b'\n'),
                        Some(b'r') => buf.push(b'\r'),
                        Some(b't') => buf.push(b'\t'),
                        Some(b'u') => {
                            let Some(cp1) = self.hex4() else {
                                return Err(
                                    "invalid string: '\\u' must be followed by 4 hex digits".into(),
                                );
                            };

                            let mut cp = cp1;

                            if (0xD800..=0xDBFF).contains(&cp1) {
                                if self.bump() == Some(b'\\') && self.bump() == Some(b'u') {
                                    let Some(cp2) = self.hex4() else {
                                        return Err("invalid string: '\\u' must be followed by 4 \
                                                    hex digits"
                                            .into());
                                    };

                                    if (0xDC00..=0xDFFF).contains(&cp2) {
                                        cp = ((cp1 << 10) + cp2) - 0x35F_DC00;
                                    } else {
                                        return Err("invalid string: surrogate U+D800..U+DBFF \
                                                    must be followed by U+DC00..U+DFFF"
                                            .into());
                                    }
                                } else {
                                    return Err("invalid string: surrogate U+D800..U+DBFF must \
                                                be followed by U+DC00..U+DFFF"
                                        .into());
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp1) {
                                return Err(
                                    "invalid string: surrogate U+DC00..U+DFFF must follow \
                                            U+D800..U+DBFF"
                                        .into(),
                                );
                            }

                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut tmp = [0u8; 4];
                            buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                        }
                        _ => {
                            return Err(
                                "invalid string: forbidden character after backslash".into()
                            );
                        }
                    }
                }
                0x00..=0x1F => {
                    return Err(format!(
                        "invalid string: control character U+{c:04X} must be escaped to \\u{c:04X}"
                    ));
                }
                0x20..=0x7F => buf.push(c),
                _ => {
                    // Multi-byte UTF-8 sequence: the ranges of lexer::scan_string.
                    let len = match c {
                        0xC2..=0xDF => 1,
                        0xE0..=0xEF => 2,
                        0xF0..=0xF4 => 3,
                        _ => return Err("invalid string: ill-formed UTF-8 byte".into()),
                    };
                    let (lo2, hi2) = match c {
                        0xE0 => (0xA0, 0xBF),
                        0xED => (0x80, 0x9F),
                        0xF0 => (0x90, 0xBF),
                        0xF4 => (0x80, 0x8F),
                        _ => (0x80, 0xBF),
                    };

                    buf.push(c);

                    for k in 0..len {
                        let b = self.bump();
                        let (lo, hi) = if k == 0 { (lo2, hi2) } else { (0x80, 0xBF) };

                        match b {
                            Some(b) if (lo..=hi).contains(&b) => buf.push(b),
                            _ => return Err("invalid string: ill-formed UTF-8 byte".into()),
                        }
                    }
                }
            }
        }

        // Validated above, so this cannot fail.
        self.string = String::from_utf8(buf).map_err(|_| "invalid string".to_string())?;
        Ok(Token::Str)
    }

    /// Port of `lexer::scan_number`: the JSON number grammar, then `strtoull` / `strtoll` for
    /// integers (falling back to `strtod` on overflow) and `strtod` for everything else.
    fn scan_number(&mut self) -> LexResult {
        let start = self.pos;
        let mut is_float = false;
        let negative = self.peek() == Some(b'-');

        if negative {
            self.bump();
        }

        match self.peek() {
            Some(b'0') => {
                self.bump();
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
            }
            _ => {
                self.bump();
                return Err("invalid number; expected digit after '-'".into());
            }
        }

        if self.peek() == Some(b'.') {
            is_float = true;
            self.bump();

            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
                return Err("invalid number; expected digit after '.'".into());
            }

            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }

        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.bump();

            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }

            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
                return Err("invalid number; expected digit after exponent sign".into());
            }

            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }

        // ASCII only, by construction.
        let text = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("0");

        if !is_float {
            if negative {
                if let Ok(i) = text.parse::<i64>() {
                    // "-0" is number_integer 0.
                    self.number = Value::Number(Number::from(i));
                    return Ok(Token::Num);
                }
            } else if let Ok(u) = text.parse::<u64>() {
                self.number = Value::Number(Number::from(u));
                return Ok(Token::Num);
            }
        }

        // strtod is correctly rounded on glibc, as is Rust's parser.
        let f: f64 = text.parse().unwrap_or(f64::NAN);
        self.number = float_value(f);

        if !f.is_finite() {
            // parser::sax_parse_internal: "number overflow parsing '<text>'" (out_of_range 406).
            return Err(format!("\u{0}406number overflow parsing '{text}'"));
        }

        Ok(Token::Num)
    }
}

/// Port of `nlohmann::basic_json::parse( input, nullptr, allow_exceptions = true,
/// ignore_comments )` for the document type with the given object order.
///
/// `input` is the raw file content (bytes, so that invalid UTF-8 is reported like nlohmann
/// does). KiCad always passes `ignore_comments = true`.
pub fn parse_bytes(
    input: &[u8],
    order: ObjectOrder,
    ignore_comments: bool,
) -> Result<Value, JsonError> {
    let mut lexer = Lexer {
        input,
        pos: 0,
        ignore_comments,
        string: String::new(),
        number: Value::Null,
        token_start: 0,
        line: 1,
        line_start: 0,
    };

    // lexer::skip_bom: only at the very start.
    if input.first() == Some(&0xEF) {
        if input.get(1) == Some(&0xBB) && input.get(2) == Some(&0xBF) {
            lexer.pos = 3;
            lexer.line_start = 3;
        } else {
            return Err(parse_error(
                &lexer,
                "invalid BOM; must be 0xEF 0xBB 0xBF if given".into(),
                None,
            ));
        }
    }

    let value = parse_value_iterative(&mut lexer)?;

    let tok = lexer.scan().map_err(|m| parse_error(&lexer, m, None))?;

    if tok != Token::EndOfInput {
        return Err(parse_error(
            &lexer,
            format!(
                "syntax error while parsing value - unexpected {}; expected end of input",
                tok.name()
            ),
            Some(tok),
        ));
    }

    let mut value = value;

    if order == ObjectOrder::Sorted {
        sort_objects(&mut value);
    }

    Ok(value)
}

/// [`parse_bytes`] with `ignore_comments = true` on a string (`IGNORE_COMMENTS` in KiCad).
pub fn parse(text: &str, order: ObjectOrder) -> Result<Value, JsonError> {
    parse_bytes(text.as_bytes(), order, true)
}

fn parse_error(lexer: &Lexer<'_>, message: String, _tok: Option<Token>) -> JsonError {
    if let Some(rest) = message.strip_prefix("\u{0}406") {
        return JsonError {
            kind: JsonErrorKind::OutOfRange,
            id: 406,
            byte: lexer.pos,
            message: rest.to_string(),
        };
    }

    let column = lexer.pos.saturating_sub(lexer.line_start);
    let message = if message.starts_with("syntax error") {
        message
    } else {
        format!("syntax error while parsing value - {message}")
    };

    JsonError {
        kind: JsonErrorKind::ParseError,
        id: 101,
        byte: lexer.pos,
        message: format!(
            "parse error at line {}, column {column}: {message}",
            lexer.line
        ),
    }
}

enum Frame {
    Array(Vec<Value>),
    Object(Map<String, Value>, String),
}

/// Port of `parser::sax_parse_internal` (iterative, with the same accepted grammar) building a
/// DOM like `json_sax_dom_parser`.
fn parse_value_iterative(lexer: &mut Lexer<'_>) -> Result<Value, JsonError> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut tok = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

    loop {
        // Parse one value starting at `tok`.
        let value = match tok {
            Token::BeginObject => {
                let next = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

                if next == Token::EndObject {
                    Value::Object(Map::new())
                } else {
                    if next != Token::Str {
                        return Err(parse_error(
                            lexer,
                            format!(
                                "syntax error while parsing object key - unexpected {}; expected \
                                 string literal",
                                next.name()
                            ),
                            Some(next),
                        ));
                    }

                    let key = std::mem::take(&mut lexer.string);
                    expect_name_separator(lexer)?;

                    if stack.len() >= MAX_PARSE_DEPTH {
                        return Err(parse_error(lexer, "nesting too deep".into(), None));
                    }

                    stack.push(Frame::Object(Map::new(), key));
                    tok = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;
                    continue;
                }
            }
            Token::BeginArray => {
                let next = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

                if next == Token::EndArray {
                    Value::Array(Vec::new())
                } else {
                    if stack.len() >= MAX_PARSE_DEPTH {
                        return Err(parse_error(lexer, "nesting too deep".into(), None));
                    }

                    stack.push(Frame::Array(Vec::new()));
                    tok = next;
                    continue;
                }
            }
            Token::Null => Value::Null,
            Token::True => Value::Bool(true),
            Token::False => Value::Bool(false),
            Token::Str => Value::String(std::mem::take(&mut lexer.string)),
            Token::Num => std::mem::replace(&mut lexer.number, Value::Null),
            other => {
                return Err(parse_error(
                    lexer,
                    format!(
                        "syntax error while parsing value - unexpected {}; expected '[', '{{', or a literal",
                        other.name()
                    ),
                    Some(other),
                ));
            }
        };

        // Attach the value to its parent, closing containers as needed.
        let mut value = value;

        loop {
            match stack.last_mut() {
                None => return Ok(value),
                Some(Frame::Array(items)) => {
                    items.push(value);
                    let next = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

                    match next {
                        Token::ValueSeparator => {
                            tok = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;
                            break;
                        }
                        Token::EndArray => {
                            let Some(Frame::Array(items)) = stack.pop() else {
                                unreachable!()
                            };
                            value = Value::Array(items);
                        }
                        other => {
                            return Err(parse_error(
                                lexer,
                                format!(
                                    "syntax error while parsing array - unexpected {}; expected ']'",
                                    other.name()
                                ),
                                Some(other),
                            ));
                        }
                    }
                }
                Some(Frame::Object(map, key)) => {
                    // json_sax_dom_parser::key: object[key] (last value wins, first position
                    // kept for ordered_json, map semantics for json).
                    map.insert(std::mem::take(key), value);
                    let next = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

                    match next {
                        Token::ValueSeparator => {
                            let k = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

                            if k != Token::Str {
                                return Err(parse_error(
                                    lexer,
                                    format!(
                                        "syntax error while parsing object key - unexpected {}; \
                                         expected string literal",
                                        k.name()
                                    ),
                                    Some(k),
                                ));
                            }

                            *key = std::mem::take(&mut lexer.string);
                            expect_name_separator(lexer)?;
                            tok = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;
                            break;
                        }
                        Token::EndObject => {
                            let Some(Frame::Object(map, _)) = stack.pop() else {
                                unreachable!()
                            };
                            value = Value::Object(map);
                        }
                        other => {
                            return Err(parse_error(
                                lexer,
                                format!(
                                    "syntax error while parsing object - unexpected {}; expected '}}'",
                                    other.name()
                                ),
                                Some(other),
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn expect_name_separator(lexer: &mut Lexer<'_>) -> Result<(), JsonError> {
    let sep = lexer.scan().map_err(|m| parse_error(lexer, m, None))?;

    if sep != Token::NameSeparator {
        return Err(parse_error(
            lexer,
            format!(
                "syntax error while parsing object separator - unexpected {}; expected ':'",
                sep.name()
            ),
            Some(sep),
        ));
    }

    Ok(())
}

/// Sorts every object of `value` by key bytes (`std::map<std::string, json>` order).
pub fn sort_objects(value: &mut Value) {
    value.sort_all_objects();
}

// ------------------------------------------------------------------------------------------
// Serializer: port of nlohmann::detail::serializer::dump
// ------------------------------------------------------------------------------------------

/// Port of `basic_json::dump( indent )` with `indent_char = ' '`, `ensure_ascii = false` and
/// the strict error handler.
///
/// `indent = None` is the compact form (`dump()`, `dump( -1 )`, and `operator<<` without
/// `std::setw` or with `setw( 0 )`); `Some( n )` pretty-prints with `n` spaces per level
/// (`dump( n )`, `o << std::setw( n ) << j` for `n > 0`). `order` selects `nlohmann::json`
/// (sorted keys) or `nlohmann::ordered_json` (insertion order) semantics.
///
/// Strings in a [`Value`] are always valid UTF-8, so nlohmann's `type_error.316` (invalid UTF-8
/// with the strict handler) cannot occur.
pub fn dump(value: &Value, indent: Option<usize>, order: ObjectOrder) -> String {
    let mut out = Vec::new();
    dump_value(&mut out, value, indent, order, 0);
    String::from_utf8(out).unwrap_or_default()
}

/// `o << std::setw( width ) << j << std::endl` as KiCad writes DRC reports (width 4) and Gerber
/// job files (width 2): `operator<<` pretty-prints only for `width > 0`.
pub fn dump_stream(value: &Value, width: usize, order: ObjectOrder) -> String {
    let indent = if width > 0 { Some(width) } else { None };
    let mut s = dump(value, indent, order);
    s.push('\n');
    s
}

fn push_indent(out: &mut Vec<u8>, n: usize) {
    out.extend(std::iter::repeat_n(b' ', n));
}

fn sorted_entries(map: &Map<String, Value>, order: ObjectOrder) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();

    if order == ObjectOrder::Sorted {
        entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    }

    entries
}

fn dump_value(
    out: &mut Vec<u8>,
    value: &Value,
    indent: Option<usize>,
    order: ObjectOrder,
    current: usize,
) {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                out.extend_from_slice(b"{}");
                return;
            }

            let entries = sorted_entries(map, order);

            if let Some(step) = indent {
                out.extend_from_slice(b"{\n");
                let new_indent = current + step;

                for (i, (k, v)) in entries.iter().enumerate() {
                    push_indent(out, new_indent);
                    out.push(b'"');
                    dump_escaped(out, k);
                    out.extend_from_slice(b"\": ");
                    dump_value(out, v, indent, order, new_indent);

                    if i + 1 < entries.len() {
                        out.extend_from_slice(b",\n");
                    }
                }

                out.push(b'\n');
                push_indent(out, current);
                out.push(b'}');
            } else {
                out.push(b'{');

                for (i, (k, v)) in entries.iter().enumerate() {
                    out.push(b'"');
                    dump_escaped(out, k);
                    out.extend_from_slice(b"\":");
                    dump_value(out, v, indent, order, current);

                    if i + 1 < entries.len() {
                        out.push(b',');
                    }
                }

                out.push(b'}');
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                out.extend_from_slice(b"[]");
                return;
            }

            if let Some(step) = indent {
                out.extend_from_slice(b"[\n");
                let new_indent = current + step;

                for (i, v) in items.iter().enumerate() {
                    push_indent(out, new_indent);
                    dump_value(out, v, indent, order, new_indent);

                    if i + 1 < items.len() {
                        out.extend_from_slice(b",\n");
                    }
                }

                out.push(b'\n');
                push_indent(out, current);
                out.push(b']');
            } else {
                out.push(b'[');

                for (i, v) in items.iter().enumerate() {
                    dump_value(out, v, indent, order, current);

                    if i + 1 < items.len() {
                        out.push(b',');
                    }
                }

                out.push(b']');
            }
        }
        Value::String(s) => {
            out.push(b'"');
            dump_escaped(out, s);
            out.push(b'"');
        }
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Null => out.extend_from_slice(b"null"),
        Value::Number(n) => match classify_number(n) {
            JsonNumber::Unsigned(u) => out.extend_from_slice(u.to_string().as_bytes()),
            JsonNumber::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
            JsonNumber::Float(f) => {
                if f.is_finite() {
                    to_chars::to_chars(out, f);
                } else {
                    out.extend_from_slice(b"null");
                }
            }
        },
    }
}

/// Port of `serializer::dump_escaped( s, ensure_ascii = false )`.
fn dump_escaped(out: &mut Vec<u8>, s: &str) {
    for ch in s.chars() {
        match ch {
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{0C}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            c if (c as u32) <= 0x1F => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut tmp = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// Comparison and update
// ------------------------------------------------------------------------------------------

/// Port of nlohmann's `operator==` for `nlohmann::json` (objects compare as maps, numbers of
/// different kinds compare numerically: integer vs float through `double`, integer vs unsigned
/// through `int64_t`).
pub fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(v, w)| json_eq(v, w))
        }
        (Value::Number(x), Value::Number(y)) => number_eq(classify_number(x), classify_number(y)),
        _ => a == b,
    }
}

fn number_eq(a: JsonNumber, b: JsonNumber) -> bool {
    use JsonNumber::*;

    match (a, b) {
        (Integer(x), Integer(y)) => x == y,
        (Unsigned(x), Unsigned(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Integer(x), Float(y)) => (x as f64) == y,
        (Float(x), Integer(y)) => x == (y as f64),
        (Unsigned(x), Float(y)) => (x as f64) == y,
        (Float(x), Unsigned(y)) => x == (y as f64),
        (Integer(x), Unsigned(y)) => x == (y as i64),
        (Unsigned(x), Integer(y)) => (x as i64) == y,
    }
}

/// Port of `basic_json::update( const_reference j, bool merge_objects )`.
///
/// `target` null becomes an empty object first; a non-object `target` or `source` is
/// `type_error.312`.
pub fn update(target: &mut Value, source: &Value, merge_objects: bool) -> Result<(), JsonError> {
    if target.is_null() {
        *target = Value::Object(Map::new());
    }

    let Value::Object(tmap) = target else {
        return Err(JsonError::new(
            JsonErrorKind::TypeError,
            312,
            format!("cannot use update() with {}", type_name(target)),
        ));
    };

    let Value::Object(smap) = source else {
        return Err(JsonError::new(
            JsonErrorKind::TypeError,
            312,
            format!("cannot use update() with {}", type_name(source)),
        ));
    };

    for (k, v) in smap {
        if merge_objects && v.is_object() {
            if let Some(existing) = tmap.get_mut(k) {
                // Recurses whatever the existing value is (a non-object throws type_error.312,
                // keeping the members merged so far).
                update(existing, v, true)?;
                continue;
            }
        }

        tmap.insert(k.clone(), v.clone());
    }

    Ok(())
}

// ------------------------------------------------------------------------------------------
// JSON pointer
// ------------------------------------------------------------------------------------------

/// Port of `nlohmann::json_pointer` (RFC 6901 with nlohmann's error rules).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JsonPointer {
    /// Unescaped reference tokens.
    pub tokens: Vec<String>,
}

impl JsonPointer {
    /// Port of `json_pointer( const string_t& )` / `json_pointer::split`.
    pub fn parse(s: &str) -> Result<Self, JsonError> {
        if s.is_empty() {
            return Ok(Self::default());
        }

        if !s.starts_with('/') {
            return Err(JsonError::new(
                JsonErrorKind::ParseError,
                107,
                format!("JSON pointer must be empty or begin with '/' - was: '{s}'"),
            ));
        }

        let mut tokens = Vec::new();

        for token in s[1..].split('/') {
            let bytes = token.as_bytes();

            for (pos, &b) in bytes.iter().enumerate() {
                if b == b'~'
                    && (pos + 1 == bytes.len()
                        || (bytes[pos + 1] != b'0' && bytes[pos + 1] != b'1'))
                {
                    return Err(JsonError::new(
                        JsonErrorKind::ParseError,
                        108,
                        "escape character '~' must be followed with '0' or '1'".into(),
                    ));
                }
            }

            // detail::unescape: "~1" -> "/" first, then "~0" -> "~".
            tokens.push(token.replace("~1", "/").replace("~0", "~"));
        }

        Ok(Self { tokens })
    }

    /// Port of `JSON_SETTINGS_INTERNALS::PointerFromString`: `a.b.c` becomes `/a/b/c`. An
    /// invalid result (a `~` not followed by `0`/`1`) is caught in C++ (with a debug assertion)
    /// and the default, empty pointer - the whole document - is returned.
    pub fn from_dotted(path: &str) -> Self {
        let p = format!("/{}", path.replace('.', "/"));
        Self::parse(&p).unwrap_or_default()
    }

    /// Port of `json_pointer::contains( const BasicJsonType* )`. Returns an error where
    /// nlohmann throws (`array_index` on an empty or out-of-range token).
    pub fn contains(&self, root: &Value) -> Result<bool, JsonError> {
        let mut ptr = root;

        for token in &self.tokens {
            match ptr {
                Value::Object(map) => match map.get(token) {
                    Some(v) => ptr = v,
                    None => return Ok(false),
                },
                Value::Array(items) => {
                    let b = token.as_bytes();

                    if token == "-" {
                        return Ok(false);
                    }

                    if b.len() == 1 && !b[0].is_ascii_digit() {
                        return Ok(false);
                    }

                    if b.len() > 1 {
                        if !(b'1'..=b'9').contains(&b[0]) {
                            return Ok(false);
                        }

                        if !b[1..].iter().all(u8::is_ascii_digit) {
                            return Ok(false);
                        }
                    }

                    let idx = array_index(token)?;

                    match items.get(idx) {
                        Some(v) => ptr = v,
                        None => return Ok(false),
                    }
                }
                _ => return Ok(false),
            }
        }

        Ok(true)
    }

    /// Port of `json_pointer::get_checked` (`basic_json::at( json_pointer )`).
    pub fn at<'v>(&self, root: &'v Value) -> Result<&'v Value, JsonError> {
        let mut ptr = root;

        for token in &self.tokens {
            match ptr {
                Value::Object(map) => match map.get(token) {
                    Some(v) => ptr = v,
                    None => {
                        return Err(JsonError::new(
                            JsonErrorKind::OutOfRange,
                            403,
                            format!("key '{token}' not found"),
                        ));
                    }
                },
                Value::Array(items) => {
                    if token == "-" {
                        return Err(JsonError::new(
                            JsonErrorKind::OutOfRange,
                            402,
                            format!("array index '-' ({}) is out of range", items.len()),
                        ));
                    }

                    let idx = array_index(token)?;

                    match items.get(idx) {
                        Some(v) => ptr = v,
                        None => {
                            return Err(JsonError::new(
                                JsonErrorKind::OutOfRange,
                                401,
                                format!("array index {idx} is out of range"),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(JsonError::new(
                        JsonErrorKind::OutOfRange,
                        404,
                        format!("unresolved reference token '{token}'"),
                    ));
                }
            }
        }

        Ok(ptr)
    }

    /// Mutable [`at`](Self::at).
    pub fn at_mut<'v>(&self, root: &'v mut Value) -> Result<&'v mut Value, JsonError> {
        // Validate first with the shared version, then walk mutably.
        self.at(root)?;
        let mut ptr = root;

        for token in &self.tokens {
            ptr = match ptr {
                Value::Object(map) => map.get_mut(token).ok_or_else(|| {
                    JsonError::new(
                        JsonErrorKind::OutOfRange,
                        403,
                        format!("key '{token}' not found"),
                    )
                })?,
                Value::Array(items) => {
                    let idx = array_index(token)?;
                    items.get_mut(idx).ok_or_else(|| {
                        JsonError::new(
                            JsonErrorKind::OutOfRange,
                            401,
                            format!("array index {idx} is out of range"),
                        )
                    })?
                }
                _ => {
                    return Err(JsonError::new(
                        JsonErrorKind::OutOfRange,
                        404,
                        format!("unresolved reference token '{token}'"),
                    ));
                }
            };
        }

        Ok(ptr)
    }

    /// Port of `json_pointer::get_unchecked` (`basic_json::operator[]( json_pointer )`, which
    /// `JSON_SETTINGS_INTERNALS::operator[]( path )` uses): a null value becomes an array when
    /// the token is all digits (including the empty token) or `-`, an object otherwise; missing
    /// keys are created, arrays grow with nulls, `-` appends.
    pub fn get_unchecked<'v>(&self, root: &'v mut Value) -> Result<&'v mut Value, JsonError> {
        let mut ptr = root;

        for token in &self.tokens {
            if ptr.is_null() {
                let nums = token.bytes().all(|b| b.is_ascii_digit());

                *ptr = if nums || token == "-" {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                };
            }

            ptr = match ptr {
                Value::Object(map) => map.entry(token.clone()).or_insert(Value::Null),
                Value::Array(items) => {
                    let idx = if token == "-" {
                        items.len()
                    } else {
                        array_index(token)?
                    };

                    if idx >= items.len() {
                        items.resize(idx + 1, Value::Null);
                    }

                    &mut items[idx]
                }
                _ => {
                    return Err(JsonError::new(
                        JsonErrorKind::OutOfRange,
                        404,
                        format!("unresolved reference token '{token}'"),
                    ));
                }
            };
        }

        Ok(ptr)
    }
}

/// Port of `json_pointer::array_index`.
fn array_index(s: &str) -> Result<usize, JsonError> {
    let b = s.as_bytes();

    if b.len() > 1 && b[0] == b'0' {
        return Err(JsonError::new(
            JsonErrorKind::ParseError,
            106,
            format!("array index '{s}' must not begin with '0'"),
        ));
    }

    if b.len() > 1 && !(b'1'..=b'9').contains(&b[0]) {
        return Err(JsonError::new(
            JsonErrorKind::ParseError,
            109,
            format!("array index '{s}' is not a number"),
        ));
    }

    // strtoull with full-length and range checks.
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return Err(JsonError::new(
            JsonErrorKind::OutOfRange,
            404,
            format!("unresolved reference token '{s}'"),
        ));
    }

    match s.parse::<u64>() {
        Ok(v) if v < u64::MAX => usize::try_from(v).map_err(|_| {
            JsonError::new(
                JsonErrorKind::OutOfRange,
                410,
                format!("array index {s} exceeds size_type"),
            )
        }),
        Ok(_) => Err(JsonError::new(
            JsonErrorKind::OutOfRange,
            410,
            format!("array index {s} exceeds size_type"),
        )),
        Err(_) => Err(JsonError::new(
            JsonErrorKind::OutOfRange,
            404,
            format!("unresolved reference token '{s}'"),
        )),
    }
}

// ------------------------------------------------------------------------------------------
// get<T>() / to_json conversions
// ------------------------------------------------------------------------------------------

/// x86-64 `cvttsd2si` (32 bit destination) semantics of a C++ `static_cast<int>( double )`: out
/// of range and NaN give `INT_MIN` (PORTING.md section 9).
fn f64_to_i32_x86(v: f64) -> i32 {
    if v.is_nan() || v >= 2_147_483_648.0 || v <= -2_147_483_649.0 {
        i32::MIN
    } else {
        v as i32
    }
}

/// x86-64 `cvttsd2si` (64 bit destination).
fn f64_to_i64_x86(v: f64) -> i64 {
    if v.is_nan() || v >= 9_223_372_036_854_775_808.0 || v < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        v as i64
    }
}

/// g++ x86-64 `static_cast<unsigned long>( double )`: values below 2^63 go through `cvttsd2si`
/// (negative values wrap), larger ones are reduced by 2^63 first.
fn f64_to_u64_x86(v: f64) -> u64 {
    if v < 9_223_372_036_854_775_808.0 {
        f64_to_i64_x86(v) as u64
    } else {
        (f64_to_i64_x86(v - 9_223_372_036_854_775_808.0) as u64) ^ (1u64 << 63)
    }
}

/// A type that nlohmann's `get<T>()` can produce (`from_json`).
pub trait FromJson: Sized {
    /// Port of `j.get<T>()`: the conversion or the `type_error` nlohmann throws.
    fn from_json(value: &Value) -> Result<Self, JsonError>;
}

/// A type that can be assigned to a nlohmann value (`to_json`).
pub trait ToJson {
    /// The JSON value nlohmann's `to_json` creates.
    fn to_json(&self) -> Value;
}

/// `get_arithmetic_value` / generic arithmetic `from_json`: the stored number cast with
/// `static_cast`, booleans accepted only by the generic overload (`allow_bool`).
fn arithmetic<T>(
    value: &Value,
    allow_bool: bool,
    from_u: fn(u64) -> T,
    from_i: fn(i64) -> T,
    from_f: fn(f64) -> T,
) -> Result<T, JsonError> {
    match value {
        Value::Number(n) => Ok(match classify_number(n) {
            JsonNumber::Unsigned(u) => from_u(u),
            JsonNumber::Integer(i) => from_i(i),
            JsonNumber::Float(f) => from_f(f),
        }),
        Value::Bool(b) if allow_bool => Ok(from_u(u64::from(*b))),
        _ => Err(JsonError::type_must_be("number", value)),
    }
}

impl FromJson for bool {
    /// `from_json( j, boolean_t& )`: only booleans.
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        match value {
            Value::Bool(b) => Ok(*b),
            _ => Err(JsonError::type_must_be("boolean", value)),
        }
    }
}

impl FromJson for i32 {
    /// Generic arithmetic `from_json` for `int`: numbers (floats truncated, out of range as
    /// on x86-64) and booleans.
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        arithmetic(value, true, |u| u as i32, |i| i as i32, f64_to_i32_x86)
    }
}

impl FromJson for u32 {
    /// Generic arithmetic `from_json` for `unsigned int` (accepts booleans).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        // g++ converts through a 64 bit cvttsd2si and keeps the low 32 bits.
        arithmetic(
            value,
            true,
            |u| u as u32,
            |i| i as u32,
            |f| f64_to_i64_x86(f) as u32,
        )
    }
}

impl FromJson for i64 {
    /// `from_json( j, number_integer_t& )` (`get_arithmetic_value`, no booleans).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        arithmetic(value, false, |u| u as i64, |i| i, f64_to_i64_x86)
    }
}

impl FromJson for u64 {
    /// `from_json( j, number_unsigned_t& )` (`get_arithmetic_value`, no booleans).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        arithmetic(value, false, |u| u, |i| i as u64, f64_to_u64_x86)
    }
}

impl FromJson for f64 {
    /// `from_json( j, number_float_t& )` (`get_arithmetic_value`, no booleans).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        arithmetic(value, false, |u| u as f64, |i| i as f64, |f| f)
    }
}

impl FromJson for f32 {
    /// Generic arithmetic `from_json` for `float` (accepts booleans).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        arithmetic(value, true, |u| u as f32, |i| i as f32, |f| f as f32)
    }
}

impl FromJson for String {
    /// `from_json( j, string_t& )` (also `wxString` via `json_conversions.cpp`): only strings.
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        match value {
            Value::String(s) => Ok(s.clone()),
            _ => Err(JsonError::type_must_be("string", value)),
        }
    }
}

impl FromJson for Value {
    /// `get<nlohmann::json>()`: a copy.
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        Ok(value.clone())
    }
}

impl<T: FromJson> FromJson for Vec<T> {
    /// `from_json` for `std::vector<T>`: arrays only, every element converted (the first
    /// failing element aborts).
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        match value {
            Value::Array(items) => items.iter().map(T::from_json).collect(),
            _ => Err(JsonError::type_must_be("array", value)),
        }
    }
}

impl<T: FromJson> FromJson for BTreeMap<String, T> {
    /// `from_json` for `std::map<std::string, T>`: objects only.
    fn from_json(value: &Value) -> Result<Self, JsonError> {
        match value {
            Value::Object(map) => {
                let mut out = BTreeMap::new();

                for (k, v) in map {
                    out.insert(k.clone(), T::from_json(v)?);
                }

                Ok(out)
            }
            _ => Err(JsonError::type_must_be("object", value)),
        }
    }
}

impl ToJson for bool {
    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }
}

impl ToJson for i32 {
    fn to_json(&self) -> Value {
        Value::Number(Number::from(*self))
    }
}

impl ToJson for u32 {
    fn to_json(&self) -> Value {
        Value::Number(Number::from(*self))
    }
}

impl ToJson for i64 {
    fn to_json(&self) -> Value {
        Value::Number(Number::from(*self))
    }
}

impl ToJson for u64 {
    fn to_json(&self) -> Value {
        Value::Number(Number::from(*self))
    }
}

impl ToJson for f64 {
    fn to_json(&self) -> Value {
        float_value(*self)
    }
}

impl ToJson for f32 {
    fn to_json(&self) -> Value {
        float_value(f64::from(*self))
    }
}

impl ToJson for String {
    fn to_json(&self) -> Value {
        Value::String(self.clone())
    }
}

impl ToJson for str {
    fn to_json(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToJson for &str {
    fn to_json(&self) -> Value {
        Value::String((*self).to_string())
    }
}

impl ToJson for Value {
    fn to_json(&self) -> Value {
        self.clone()
    }
}

impl<T: ToJson> ToJson for Vec<T> {
    fn to_json(&self) -> Value {
        Value::Array(self.iter().map(ToJson::to_json).collect())
    }
}

impl<T: ToJson> ToJson for BTreeMap<String, T> {
    fn to_json(&self) -> Value {
        Value::Object(self.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
    }
}
