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

//! Port of `libs/sexpr/include/sexpr/sexpr_exception.h`.

use std::fmt;

/// Port of `SEXPR::PARSE_EXCEPTION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseException {
    msg: String,
}

impl ParseException {
    /// Port of `PARSE_EXCEPTION::PARSE_EXCEPTION( const std::string& aMessage )`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            msg: message.into(),
        }
    }

    /// Port of `PARSE_EXCEPTION::what`.
    pub fn what(&self) -> &str {
        &self.msg
    }
}

impl fmt::Display for ParseException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ParseException {}

/// Port of `SEXPR::INVALID_TYPE_EXCEPTION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTypeException {
    msg: String,
}

impl InvalidTypeException {
    /// Port of `INVALID_TYPE_EXCEPTION::INVALID_TYPE_EXCEPTION( const std::string& aMessage )`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            msg: message.into(),
        }
    }

    /// Port of `INVALID_TYPE_EXCEPTION::what`.
    pub fn what(&self) -> &str {
        &self.msg
    }
}

impl fmt::Display for InvalidTypeException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for InvalidTypeException {}

/// Everything the `SEXPR` library throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SexprException {
    /// `SEXPR::PARSE_EXCEPTION`
    Parse(ParseException),
    /// `SEXPR::INVALID_TYPE_EXCEPTION`
    InvalidType(InvalidTypeException),
    /// `std::invalid_argument`, thrown by the stream extraction operators and the argument
    /// scanners.
    InvalidArgument(String),
    /// A child index beyond the end of a list.  C++ does not check the index (undefined
    /// behaviour); the port reports it.
    OutOfRange {
        /// The requested child index.
        index: usize,
        /// The number of children of the list.
        len: usize,
    },
}

impl SexprException {
    /// The message of the exception (C++ `what()`).
    pub fn what(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for SexprException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SexprException::Parse(e) => f.write_str(e.what()),
            SexprException::InvalidType(e) => f.write_str(e.what()),
            SexprException::InvalidArgument(msg) => f.write_str(msg),
            SexprException::OutOfRange { index, len } => {
                write!(
                    f,
                    "SEXPR child index {index} is out of range (list has {len} children)"
                )
            }
        }
    }
}

impl std::error::Error for SexprException {}

impl From<ParseException> for SexprException {
    fn from(e: ParseException) -> Self {
        SexprException::Parse(e)
    }
}

impl From<InvalidTypeException> for SexprException {
    fn from(e: InvalidTypeException) -> Self {
        SexprException::InvalidType(e)
    }
}
