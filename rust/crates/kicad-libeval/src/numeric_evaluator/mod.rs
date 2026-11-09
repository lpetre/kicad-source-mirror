// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2017 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `include/libeval/numeric_evaluator.h`, `common/libeval/numeric_evaluator.cpp` and
//! `common/libeval/grammar.lemon`: the unit-aware calculator behind KiCad's numeric input
//! fields.
//!
//! An evaluator object is used to replace an input string that represents
//! a mathematical expression by its result.
//!
//! Example: Consider the input "3+4". The result of this expression is "7".
//!
//! ```
//! use kicad_libeval::{EdaUnits, NumericEvaluator};
//!
//! let mut eval = NumericEvaluator::new(EdaUnits::Mm);
//! assert!(eval.process("3+4"));
//! assert_eq!(eval.result(), "7");
//!
//! // Unit conversion: the evaluator uses a default unit and constants can be specified
//! // with a unit.
//! assert!(eval.process("1in + 100mil"));
//! assert_eq!(eval.result(), "27.94");
//!
//! // Expressions can refer to variables if they were defined by previous expressions or by
//! // set_var().  Expressions that define/set variables do not have a result.
//! eval.process("x=1; y=2"); // Result is NaN
//! eval.set_var("z", 3.0);
//! eval.process("x+y+z");
//! assert_eq!(eval.result(), "6");
//! ```
//!
//! Please note that even a valid input string may result in an empty output string or "NaN".

pub(crate) mod grammar;

use std::collections::BTreeMap;

use crate::crt;
use crate::eda_units::EdaUnits;
use crate::lemon::YyParser;
use crate::string_utils::ui_double2_str;
use grammar::{
    ASSIGN, DIVIDE, ENDS, MINUS, MULT, NumEvalGrammar, PARENL, PARENR, PLUS, SEMCOL, TokenType,
    UNIT, VALUE, VAR,
};

/// Port of `enum class NUMERIC_EVALUATOR::Unit`.  (`Unit::Invalid` is `None` of an
/// `Option<Unit>` here.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Um,
    Mm,
    Cm,
    Inch,
    Mil,
    Degrees,
    Si,
    Femtoseconds,
    Picoseconds,
    PsPerInch,
    PsPerCm,
    PsPerMm,
}

/// Port of `struct NUMERIC_EVALUATOR::Token`: token type used by the tokenizer.
struct Token {
    token: i32,
    value: TokenType,
}

/// Port of `struct NUMERIC_EVALUATOR::TokenStat`: token state for input string.
#[derive(Default)]
struct TokenStat {
    /// current input string ("var=4"), as UTF-8 bytes; `None` is C++'s `nullptr`
    input: Option<Vec<u8>>,
    /// output token ("var", type:VAR; "4", type:VALUE) and, after a successful parse, the
    /// result; `None` is C++'s `nullptr`.  (C++: a `char` buffer of `outputLen` bytes.)
    token: Option<String>,
    /// strlen(input)
    input_len: usize,
    /// current index
    pos: usize,
}

/// Port of `class NUMERIC_EVALUATOR`.
///
/// The decimal separator of the locale is always `.` in this port (see `LocaleChanged`);
/// both `.` and `,` are accepted in the input, as in C++.
pub struct NumericEvaluator {
    /// the current lemon parser state machine
    parser: Option<Box<YyParser<NumEvalGrammar>>>,

    token: TokenStat,

    locale_decimal_separator: u8,

    // Parse progress. Set by parser actions.
    parse_error: bool,
    parse_finished: bool,

    /// Default unit for values
    default_units: Unit,

    original_text: String,

    var_map: BTreeMap<String, f64>,
}

impl NumericEvaluator {
    /// Port of `NUMERIC_EVALUATOR::NUMERIC_EVALUATOR( EDA_UNITS aUnits )`.
    pub fn new(units: EdaUnits) -> Self {
        let mut evaluator = NumericEvaluator {
            parser: Some(Box::new(YyParser::new())),
            token: TokenStat::default(),
            locale_decimal_separator: b'.',
            parse_error: false,
            parse_finished: false,
            default_units: Unit::Mm,
            original_text: String::new(),
            var_map: BTreeMap::new(),
        };

        evaluator.locale_changed();
        evaluator.set_default_units(units);
        evaluator
    }

    /// Port of `NUMERIC_EVALUATOR::Clear()`.
    ///
    /// clear() should be invoked by the client if a new input string is to be processed. It
    /// will reset the parser. User defined variables are retained.
    pub fn clear(&mut self) {
        self.token.token = None;
        self.token.input = None;
        self.parse_error = true;
        self.original_text = String::new();
    }

    /// Port of `NUMERIC_EVALUATOR::SetDefaultUnits`.
    pub fn set_default_units(&mut self, units: EdaUnits) {
        self.default_units = match units {
            EdaUnits::Mm => Unit::Mm,
            EdaUnits::Mils => Unit::Mil,
            EdaUnits::Inch => Unit::Inch,
            EdaUnits::Degrees => Unit::Degrees,
            EdaUnits::Fs => Unit::Femtoseconds,
            EdaUnits::Ps => Unit::Picoseconds,
            EdaUnits::PsPerInch => Unit::PsPerInch,
            EdaUnits::PsPerCm => Unit::PsPerCm,
            EdaUnits::PsPerMm => Unit::PsPerMm,
            EdaUnits::Unscaled => Unit::Si,
            _ => Unit::Mm,
        };
    }

    /// Port of `NUMERIC_EVALUATOR::LocaleChanged`.  C++ reads the decimal separator from
    /// `localeconv()`; this port has no locale and always uses `.`.
    pub fn locale_changed(&mut self) {
        self.locale_decimal_separator = crt::LOCALE_DECIMAL_POINT as u8;
    }

    /// Port of `NUMERIC_EVALUATOR::parseError` (used by the lemon parser).
    pub(crate) fn parse_error(&mut self, _s: &str) {
        self.parse_error = true;
    }

    /// Port of `NUMERIC_EVALUATOR::parseOk` (used by the lemon parser).
    pub(crate) fn parse_ok(&mut self) {
        self.parse_finished = true;
    }

    /// Port of `NUMERIC_EVALUATOR::parseSetResult` (used by the lemon parser).
    pub(crate) fn parse_set_result(&mut self, val: f64) {
        // The actions only run while a string is being processed, i.e. while the output
        // buffer exists.
        let Some(token) = self.token.token.as_mut() else {
            return;
        };

        if val.is_nan() {
            // Naively printing this with %g produces "nan" on some platforms
            // and "-nan(ind)" on others (e.g. MSVC). So force a "standard" string.
            *token = "NaN".to_string();
        } else {
            // Can be printed as a floating point
            // Warning: DO NOT use a format like %f or %g, because they can create issues.
            // especially %g can generate an exponent, incompatible with UNIT_BINDER
            // Use the optimized UIDouble2Str
            *token = ui_double2_str(val);
        }
    }

    /// Port of `NUMERIC_EVALUATOR::IsValid`: check if previous invocation of process() was
    /// successful.
    pub fn is_valid(&self) -> bool {
        !self.parse_error
    }

    /// Port of `NUMERIC_EVALUATOR::Result`: result of string processing.  Undefined (but
    /// deterministic: the content of the token buffer) if `!is_valid()`.
    pub fn result(&self) -> String {
        self.token.token.clone().unwrap_or_default()
    }

    /// Port of `NUMERIC_EVALUATOR::OriginalText`: retrieve the original text before
    /// evaluation.
    pub fn original_text(&self) -> &str {
        &self.original_text
    }

    /// Port of `NUMERIC_EVALUATOR::Process`: evaluate input string.
    ///
    /// Result can be retrieved by result().
    /// Returns true if input string could be evaluated, otherwise false.
    pub fn process(&mut self, string: &str) -> bool {
        // Feed parser token after token until end of input.

        self.new_string(string);
        self.parse_error = false;
        self.parse_finished = false;

        if string.is_empty() {
            self.parse_finished = true;
            return true;
        }

        loop {
            let tok = self.get_token();
            let token = tok.token;
            let value = tok.value.clone();

            self.parse(token, tok.value);

            if self.parse_finished || token == ENDS {
                // Reset parser by passing zero as token ID, value is ignored.
                self.parse(0, value);
                break;
            }

            if token == 0 {
                break;
            }
        }

        !self.parse_error
    }

    /// `numEval::Parse( m_parser, token, value, this )`
    fn parse(&mut self, major: i32, value: TokenType) {
        // The parser is a member of the evaluator but its actions call back into the
        // evaluator, so it is moved out for the duration of the call.
        if let Some(mut parser) = self.parser.take() {
            parser.parse(major, value, self);
            self.parser = Some(parser);
        }
    }

    /// Port of `NUMERIC_EVALUATOR::newString`: begin processing of a new input string.
    fn new_string(&mut self, string: &str) {
        self.clear();

        self.original_text = string.to_string();

        // C strings end at the first NUL.
        let bytes = string.as_bytes();
        let len = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());

        self.token.input = Some(bytes[..len].to_vec());
        self.token.input_len = len;
        self.token.pos = 0;
        self.token.token = Some("0".to_string());

        self.parse_finished = false;
    }

    /// Port of `NUMERIC_EVALUATOR::IsOldSchoolDecimalSeparator( wxUniChar ch, double* siScaler )`.
    ///
    /// Support for old school decimal separators (ie: "2K2") according to IEC 60062.
    /// Returns the SI scale factor if `ch` is such a separator.
    pub fn is_old_school_decimal_separator(ch: char) -> Option<f64> {
        match ch {
            'p' => Some(1.0e-12),
            'n' => Some(1.0e-9),
            '\u{B5}' => Some(1.0e-6),
            '\u{3BC}' => Some(1.0e-6),
            'u' => Some(1.0e-6),
            'm' => Some(1.0e-3),
            'L' => Some(1.0e-3),
            'R' => Some(1.0),
            'F' => Some(1.0),
            'k' => Some(1.0e3),
            'K' => Some(1.0e3),
            'M' => Some(1.0e6),
            'G' => Some(1.0e9),
            'T' => Some(1.0e12),
            _ => None,
        }
    }

    /// Port of `NUMERIC_EVALUATOR::IsOldSchoolDecimalSeparator( char ch, double* siScaler )`:
    /// limited version of [`NumericEvaluator::is_old_school_decimal_separator`] for 8-bit
    /// chars.
    pub fn is_old_school_decimal_separator_byte(ch: u8) -> Option<f64> {
        match ch {
            b'p' => Some(1.0e-12),
            b'n' => Some(1.0e-9),
            b'u' => Some(1.0e-6),
            b'm' => Some(1.0e-3),
            b'L' => Some(1.0e-3),
            b'R' => Some(1.0),
            b'F' => Some(1.0),
            b'k' => Some(1.0e3),
            b'K' => Some(1.0e3),
            b'M' => Some(1.0e6),
            b'G' => Some(1.0e9),
            b'T' => Some(1.0e12),
            _ => None,
        }
    }

    /// Port of `NUMERIC_EVALUATOR::IsDecimalSeparator`.
    pub fn is_decimal_separator(ch: u8, locale_separator: u8, allow_infix_notation: bool) -> bool {
        if ch == locale_separator || ch == b'.' || ch == b',' {
            return true;
        }

        if allow_infix_notation && Self::is_old_school_decimal_separator_byte(ch).is_some() {
            return true;
        }

        false
    }

    /// Port of `NUMERIC_EVALUATOR::IsDigit`.
    pub fn is_digit(ch: u8) -> bool {
        ch.is_ascii_digit()
    }

    /// `m_token.input[ aIndex ]`: the input is NUL terminated in C++.
    fn input_at(&self, index: usize) -> u8 {
        match &self.token.input {
            Some(input) => input.get(index).copied().unwrap_or(0),
            None => 0,
        }
    }

    /// Port of `NUMERIC_EVALUATOR::getToken`: next token/value taken from input string.
    fn get_token(&mut self) -> Token {
        let allow_infix = self.default_units == Unit::Si;

        let mut retval = Token {
            token: ENDS,
            value: TokenType { d_value: 0.0, valid: false, text: String::new() },
        };

        if self.token.token.is_none() {
            return retval;
        }

        if self.token.input.is_none() {
            return retval;
        }

        if self.token.pos >= self.token.input_len {
            return retval;
        }

        // Start processing of first/next token: Remove whitespace
        let mut ch;

        loop {
            ch = self.input_at(self.token.pos);

            if ch == b' ' {
                self.token.pos += 1;
            } else {
                break;
            }
        }

        let mut si_scaler = 1.0;

        if ch == 0 {
            /* End of input */
        } else if Self::is_digit(ch)
            || Self::is_decimal_separator(ch, self.locale_decimal_separator, allow_infix)
        {
            // VALUE
            self.extract_number(&mut si_scaler, allow_infix);
            retval.token = VALUE;
            retval.value.d_value =
                crt::atof(self.token.token.as_deref().unwrap_or("")) * si_scaler;
        } else if let Some(convert_from) = self.check_unit(&mut si_scaler, allow_infix) {
            // UNIT
            // Units are appended to a VALUE.
            // Determine factor to default unit if unit for value is given.
            // Example: Default is mm, unit is inch: factor is 25.4
            // The factor is assigned to the terminal UNIT. The actual
            // conversion is done within a parser action.
            retval.token = UNIT;

            if self.default_units == Unit::Mm {
                match convert_from {
                    Unit::Inch => retval.value.d_value = 25.4,
                    Unit::Mil => retval.value.d_value = 25.4 / 1000.0,
                    Unit::Um => retval.value.d_value = 1.0 / 1000.0,
                    Unit::Mm => retval.value.d_value = 1.0,
                    Unit::Cm => retval.value.d_value = 10.0,
                    _ => {}
                }
            } else if self.default_units == Unit::Inch {
                match convert_from {
                    Unit::Inch => retval.value.d_value = 1.0,
                    Unit::Mil => retval.value.d_value = 1.0 / 1000.0,
                    Unit::Um => retval.value.d_value = 1.0 / 25400.0,
                    Unit::Mm => retval.value.d_value = 1.0 / 25.4,
                    Unit::Cm => retval.value.d_value = 1.0 / 2.54,
                    _ => {}
                }
            } else if self.default_units == Unit::Mil {
                match convert_from {
                    Unit::Inch => retval.value.d_value = 1.0 * 1000.0,
                    Unit::Mil => retval.value.d_value = 1.0,
                    Unit::Um => retval.value.d_value = 1.0 / 25.4,
                    Unit::Mm => retval.value.d_value = 1000.0 / 25.4,
                    Unit::Cm => retval.value.d_value = 1000.0 / 2.54,
                    _ => {}
                }
            } else if self.default_units == Unit::Degrees && convert_from == Unit::Degrees {
                retval.value.d_value = 1.0;
            } else if self.default_units == Unit::Si {
                retval.value.d_value = si_scaler;
            } else if self.default_units == Unit::Picoseconds {
                match convert_from {
                    Unit::Picoseconds => retval.value.d_value = 1.0,
                    Unit::Femtoseconds => retval.value.d_value = 1.0 / 1000.0,
                    _ => {}
                }
            } else if self.default_units == Unit::Femtoseconds {
                match convert_from {
                    Unit::Picoseconds => retval.value.d_value = 1000.0,
                    Unit::Femtoseconds => retval.value.d_value = 1.0,
                    _ => {}
                }
            } else if self.default_units == Unit::PsPerInch {
                match convert_from {
                    Unit::PsPerInch => retval.value.d_value = 1.0,
                    Unit::PsPerCm => retval.value.d_value = 2.54,
                    Unit::PsPerMm => retval.value.d_value = 25.4,
                    _ => {}
                }
            } else if self.default_units == Unit::PsPerCm {
                match convert_from {
                    Unit::PsPerInch => retval.value.d_value = 1.0 / 2.54,
                    Unit::PsPerCm => retval.value.d_value = 1.0,
                    Unit::PsPerMm => retval.value.d_value = 10.0,
                    _ => {}
                }
            } else if self.default_units == Unit::PsPerMm {
                match convert_from {
                    Unit::PsPerInch => retval.value.d_value = 1.0 / 25.4,
                    Unit::PsPerCm => retval.value.d_value = 1.0 / 10.0,
                    Unit::PsPerMm => retval.value.d_value = 1.0,
                    _ => {}
                }
            }
        } else if ch.is_ascii_alphabetic() {
            // VAR
            let start = self.token.pos;
            let mut cptr = start + 1;

            while self.input_at(cptr).is_ascii_alphanumeric() {
                cptr += 1;
            }

            retval.token = VAR;

            let mut bytes_to_copy = cptr - start;

            // sizeof( retval.value.text ) is 32
            if bytes_to_copy >= 32 {
                bytes_to_copy = 32 - 1;
            }

            // Only ASCII letters and digits were accepted above.
            retval.value.text = (start..start + bytes_to_copy)
                .map(|i| self.input_at(i) as char)
                .collect();
            self.token.pos += cptr - start;
        } else {
            // Single char tokens
            match ch {
                b'+' => retval.token = PLUS,
                b'-' => retval.token = MINUS,
                b'*' => retval.token = MULT,
                b'/' => retval.token = DIVIDE,
                b'(' => retval.token = PARENL,
                b')' => retval.token = PARENR,
                b'=' => retval.token = ASSIGN,
                b';' => retval.token = SEMCOL,
                _ => self.parse_error = true, /* invalid character */
            }

            self.token.pos += 1;
        }

        if !self.parse_error {
            retval.value.valid = true;
        }

        retval
    }

    /// Port of the lambda `extractNumber` of `getToken`: get value as string, store into
    /// `m_token.token` and update current index.
    fn extract_number(&mut self, scaler: &mut f64, allow_infix: bool) {
        let mut have_separator = false;
        let mut ch = self.input_at(self.token.pos);
        let mut text = String::new();

        loop {
            if Self::is_decimal_separator(ch, self.locale_decimal_separator, allow_infix) {
                if have_separator {
                    break;
                } else {
                    have_separator = true;
                }

                if let Some(si_scaler) = Self::is_old_school_decimal_separator_byte(ch) {
                    *scaler = si_scaler;
                }

                text.push(self.locale_decimal_separator as char);
            } else {
                text.push(ch as char);
            }

            self.token.pos += 1;
            ch = self.input_at(self.token.pos);

            if !(Self::is_digit(ch)
                || Self::is_decimal_separator(ch, self.locale_decimal_separator, allow_infix))
            {
                break;
            }
        }

        self.token.token = Some(text);
    }

    /// Port of the lambda `checkUnit` of `getToken`: get unit for current token.
    ///
    /// Valid units are ", in, um, cm, mm, mil, ps and thou (and more, see below). Returns
    /// `None` (C++ `Unit::Invalid`) otherwise.
    fn check_unit(&mut self, si_scaler: &mut f64, allow_infix: bool) -> Option<Unit> {
        let pos = self.token.pos;
        let ch = self.input_at(pos);

        // const char* cptr = &m_token.input[ m_token.pos ];  (at most cptr[5] is read)
        let window: [u8; 6] = std::array::from_fn(|offset| self.input_at(pos + offset));
        let cptr = |offset: usize| -> u8 { window[offset] };
        let is_alnum = |c: u8| -> bool { c.is_ascii_alphanumeric() };

        if ch == b'"' {
            self.token.pos += 1;
            return Some(Unit::Inch);
        }

        // Do not use strcasecmp() as it is not available on all platforms
        let size_left = self.token.input_len - pos;

        // We should really give this unicode support
        if size_left >= 2 && ch == 0xC2 && cptr(1) == 0xB0 {
            self.token.pos += 2;
            return Some(Unit::Degrees);
        }

        // Ideally we should also handle the unicode characters that can be used for micro,
        // but unicode handling in this tokenizer doesn't work.
        // (e.g. add support for μm (µ is MICRO SIGN), µm (µ is GREEK SMALL LETTER MU)
        // later)
        if size_left >= 2 && ch == b'u' && cptr(1) == b'm' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Um);
        }

        if size_left >= 5
            && ch == b'p'
            && cptr(1) == b's'
            && cptr(2) == b'/'
            && cptr(3) == b'i'
            && cptr(4) == b'n'
            && !is_alnum(cptr(5))
        {
            self.token.pos += 5;
            return Some(Unit::PsPerInch);
        }

        if size_left >= 5
            && ch == b'p'
            && cptr(1) == b's'
            && cptr(2) == b'/'
            && cptr(3) == b'c'
            && cptr(4) == b'm'
            && !is_alnum(cptr(5))
        {
            self.token.pos += 5;
            return Some(Unit::PsPerCm);
        }

        if size_left >= 5
            && ch == b'p'
            && cptr(1) == b's'
            && cptr(2) == b'/'
            && cptr(3) == b'm'
            && cptr(4) == b'm'
            && !is_alnum(cptr(5))
        {
            self.token.pos += 5;
            return Some(Unit::PsPerMm);
        }

        if size_left >= 2 && ch == b'f' && cptr(1) == b's' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Femtoseconds);
        }

        if size_left >= 2 && ch == b'p' && cptr(1) == b's' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Picoseconds);
        }

        if size_left >= 2 && ch == b'm' && cptr(1) == b'm' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Mm);
        }

        if size_left >= 2 && ch == b'c' && cptr(1) == b'm' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Cm);
        }

        if size_left >= 2 && ch == b'i' && cptr(1) == b'n' && !is_alnum(cptr(2)) {
            self.token.pos += 2;
            return Some(Unit::Inch);
        }

        if size_left >= 3
            && ch == b'm'
            && cptr(1) == b'i'
            && cptr(2) == b'l'
            && !is_alnum(cptr(3))
        {
            self.token.pos += 3;
            return Some(Unit::Mil);
        }

        if size_left >= 4
            && ch == b't'
            && cptr(1) == b'h'
            && cptr(2) == b'o'
            && cptr(3) == b'u'
            && !is_alnum(cptr(4))
        {
            self.token.pos += 4;
            return Some(Unit::Mil);
        }

        if allow_infix && size_left >= 1 {
            if let Some(scaler) = Self::is_old_school_decimal_separator_byte(ch) {
                *si_scaler = scaler;
                self.token.pos += 1;
                return Some(Unit::Si);
            }
        }

        // Unit::Invalid
        None
    }

    /// Port of `NUMERIC_EVALUATOR::SetVar`: add/set variable with value.
    pub fn set_var(&mut self, string: &str, value: f64) {
        self.var_map.insert(string.to_string(), value);
    }

    /// Port of `NUMERIC_EVALUATOR::GetVar`: get value of variable.  Returns 0.0 and flags a
    /// parse error if it is not defined.
    pub fn var(&mut self, string: &str) -> f64 {
        match self.var_map.get(string) {
            Some(value) => *value,
            None => {
                self.parse_error = true;
                0.0
            }
        }
    }

    /// Port of `NUMERIC_EVALUATOR::RemoveVar`: remove single variable.
    pub fn remove_var(&mut self, string: &str) {
        self.var_map.remove(string);
    }

    /// Port of `NUMERIC_EVALUATOR::ClearVar`: remove all variables.
    pub fn clear_var(&mut self) {
        self.var_map.clear();
    }
}
