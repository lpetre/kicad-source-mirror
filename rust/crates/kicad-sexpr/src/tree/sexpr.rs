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

//! Port of `libs/sexpr/include/sexpr/sexpr.h` and `libs/sexpr/sexpr.cpp`: the generic
//! s-expression tree of the `SEXPR` namespace.
//!
//! The C++ class hierarchy `SEXPR` <- `SEXPR_LIST`, `SEXPR_INTEGER`, `SEXPR_DOUBLE`,
//! `SEXPR_STRING`, `SEXPR_SYMBOL` becomes the enum [`Sexpr`] over one struct per subclass.
//! Methods of the base class that throw `INVALID_TYPE_EXCEPTION` when called on the wrong kind
//! of node return `Result<_, SexprException>`.

use crate::formatter::cformat::{format_f, format_g};
use crate::tree::isexprable::ISexprable;
use crate::tree::sexpr_exception::{InvalidTypeException, SexprException};

/// Port of `SEXPR::SEXPR_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SexprType {
    /// `SEXPR_TYPE_LIST`
    List,
    /// `SEXPR_TYPE_ATOM_INTEGER`
    AtomInteger,
    /// `SEXPR_TYPE_ATOM_DOUBLE`
    AtomDouble,
    /// `SEXPR_TYPE_ATOM_STRING`
    AtomString,
    /// `SEXPR_TYPE_ATOM_SYMBOL`
    AtomSymbol,
}

/// Port of `SEXPR::SEXPR_INTEGER`.
#[derive(Debug, Clone, PartialEq)]
pub struct SexprInteger {
    /// C++ `m_value`.
    pub value: i64,
    line_number: usize,
}

impl SexprInteger {
    /// Port of `SEXPR_INTEGER::SEXPR_INTEGER( int64_t aValue )` (line number 1).
    pub fn new(value: i64) -> Self {
        Self { value, line_number: 1 }
    }

    /// Port of `SEXPR_INTEGER::SEXPR_INTEGER( int64_t aValue, int aLineNumber )`.
    pub fn with_line_number(value: i64, line_number: i32) -> Self {
        Self { value, line_number: line_number as usize }
    }
}

/// Port of `SEXPR::SEXPR_DOUBLE`.
#[derive(Debug, Clone, PartialEq)]
pub struct SexprDouble {
    /// C++ `m_value`.
    pub value: f64,
    line_number: usize,
}

impl SexprDouble {
    /// Port of `SEXPR_DOUBLE::SEXPR_DOUBLE( double aValue )` (line number 1).
    pub fn new(value: f64) -> Self {
        Self { value, line_number: 1 }
    }

    /// Port of `SEXPR_DOUBLE::SEXPR_DOUBLE( double aValue, int aLineNumber )`.
    pub fn with_line_number(value: f64, line_number: i32) -> Self {
        Self { value, line_number: line_number as usize }
    }
}

/// Port of `SEXPR::SEXPR_STRING`: a quoted string.  The value is the raw text between the
/// quotes; the tree neither unescapes it on input nor escapes it on output.
#[derive(Debug, Clone, PartialEq)]
pub struct SexprString {
    /// C++ `m_value`.
    pub value: String,
    line_number: usize,
}

impl SexprString {
    /// Port of `SEXPR_STRING::SEXPR_STRING( const std::string& aValue )` (line number 1).
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into(), line_number: 1 }
    }

    /// Port of `SEXPR_STRING::SEXPR_STRING( const std::string& aValue, int aLineNumber )`.
    pub fn with_line_number(value: impl Into<String>, line_number: i32) -> Self {
        Self { value: value.into(), line_number: line_number as usize }
    }
}

/// Port of `SEXPR::SEXPR_SYMBOL`: an unquoted atom that is not a number.
#[derive(Debug, Clone, PartialEq)]
pub struct SexprSymbol {
    /// C++ `m_value`.
    pub value: String,
    line_number: usize,
}

impl SexprSymbol {
    /// Port of `SEXPR_SYMBOL::SEXPR_SYMBOL( const std::string& aValue )` (line number 1).
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into(), line_number: 1 }
    }

    /// Port of `SEXPR_SYMBOL::SEXPR_SYMBOL( const std::string& aValue, int aLineNumber )`.
    pub fn with_line_number(value: impl Into<String>, line_number: i32) -> Self {
        Self { value: value.into(), line_number: line_number as usize }
    }
}

/// Port of `SEXPR::_OUT_STRING`: a string together with the kind of node it should become when
/// added to a list (see [`as_symbol`] and [`as_string`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutString {
    /// C++ `_Symbol`: true for a symbol, false for a quoted string.
    pub symbol: bool,
    /// C++ `_String`.
    pub string: String,
}

/// Port of `SEXPR::AsSymbol( const std::string& aString )`: mark `string` to be added to a list
/// as a symbol.
pub fn as_symbol(string: &str) -> OutString {
    OutString { symbol: true, string: string.to_string() }
}

/// Port of `SEXPR::AsString( const std::string& aString )`: mark `string` to be added to a list
/// as a quoted string.
pub fn as_string(string: &str) -> OutString {
    OutString { symbol: false, string: string.to_string() }
}

/// Port of `SEXPR::_IN_STRING`: a string to read into, together with the kind of node it must
/// be read from (see [`as_symbol_in`] and [`as_string_in`]).
#[derive(Debug, PartialEq, Eq)]
pub struct InString<'a> {
    /// C++ `_Symbol`: true to read a symbol, false to read a quoted string.
    pub symbol: bool,
    /// C++ `_String`.
    pub string: &'a mut String,
}

/// Port of `SEXPR::AsSymbol( std::string& aString )`: read a symbol into `string`.
pub fn as_symbol_in(string: &mut String) -> InString<'_> {
    InString { symbol: true, string }
}

/// Port of `SEXPR::AsString( std::string& aString )`: read a quoted string into `string`.
pub fn as_string_in(string: &mut String) -> InString<'_> {
    InString { symbol: false, string }
}

/// Port of `SEXPR::SEXPR_SCAN_ARG`: one argument of [`SexprList::scan`].
#[derive(Debug)]
pub enum SexprScanArg<'a> {
    /// `SEXPR_SCAN_ARG( int32_t* )`: store an integer child.
    Int(&'a mut i32),
    /// `SEXPR_SCAN_ARG( int64_t* )`: store an integer child.
    LongInt(&'a mut i64),
    /// `SEXPR_SCAN_ARG( double* )`: store a double (or integer) child.
    Double(&'a mut f64),
    /// `SEXPR_SCAN_ARG( std::string* )`: store a symbol or string child.
    String(&'a mut String),
    /// `SEXPR_SCAN_ARG( _IN_STRING& )`: store a child that must be a symbol resp. a string.
    SexprString(InString<'a>),
    /// `SEXPR_SCAN_ARG( const std::string* )`, `( std::string )`, `( const char* )`: the child
    /// must be a symbol or string equal to this text.
    StringComp(String),
}

impl<'a> From<&'a mut i32> for SexprScanArg<'a> {
    fn from(value: &'a mut i32) -> Self {
        SexprScanArg::Int(value)
    }
}

impl<'a> From<&'a mut i64> for SexprScanArg<'a> {
    fn from(value: &'a mut i64) -> Self {
        SexprScanArg::LongInt(value)
    }
}

impl<'a> From<&'a mut f64> for SexprScanArg<'a> {
    fn from(value: &'a mut f64) -> Self {
        SexprScanArg::Double(value)
    }
}

impl<'a> From<&'a mut String> for SexprScanArg<'a> {
    fn from(value: &'a mut String) -> Self {
        SexprScanArg::String(value)
    }
}

impl<'a> From<InString<'a>> for SexprScanArg<'a> {
    fn from(value: InString<'a>) -> Self {
        SexprScanArg::SexprString(value)
    }
}

impl From<&str> for SexprScanArg<'_> {
    fn from(value: &str) -> Self {
        SexprScanArg::StringComp(value.to_string())
    }
}

impl From<String> for SexprScanArg<'_> {
    fn from(value: String) -> Self {
        SexprScanArg::StringComp(value)
    }
}

/// Port of `SEXPR::SEXPR_CHILDREN_ARG`: one argument of [`SexprList::add_children`] /
/// [`SexprList::push`].
#[derive(Debug, Clone, PartialEq)]
pub enum SexprChildrenArg {
    /// `SEXPR_CHILDREN_ARG( int32_t )`: becomes an integer node.
    Int(i32),
    /// `SEXPR_CHILDREN_ARG( int64_t )`: becomes an integer node.
    LongInt(i64),
    /// `SEXPR_CHILDREN_ARG( double )`: becomes a double node.
    Double(f64),
    /// `SEXPR_CHILDREN_ARG( const std::string& )`, `( const char* )`: becomes a string node.
    String(String),
    /// `SEXPR_CHILDREN_ARG( const _OUT_STRING& )`: becomes a symbol or a string node.
    SexprString(OutString),
    /// `SEXPR_CHILDREN_ARG( SEXPR* )`: an existing node.
    SexprAtom(Sexpr),
}

impl From<i32> for SexprChildrenArg {
    fn from(value: i32) -> Self {
        SexprChildrenArg::Int(value)
    }
}

impl From<i64> for SexprChildrenArg {
    fn from(value: i64) -> Self {
        SexprChildrenArg::LongInt(value)
    }
}

impl From<f64> for SexprChildrenArg {
    fn from(value: f64) -> Self {
        SexprChildrenArg::Double(value)
    }
}

impl From<f32> for SexprChildrenArg {
    /// `operator<<( SEXPR_LIST&, float )`: the value is widened to a double node.
    fn from(value: f32) -> Self {
        SexprChildrenArg::Double(f64::from(value))
    }
}

impl From<&str> for SexprChildrenArg {
    fn from(value: &str) -> Self {
        SexprChildrenArg::String(value.to_string())
    }
}

impl From<String> for SexprChildrenArg {
    fn from(value: String) -> Self {
        SexprChildrenArg::String(value)
    }
}

impl From<OutString> for SexprChildrenArg {
    fn from(value: OutString) -> Self {
        SexprChildrenArg::SexprString(value)
    }
}

impl From<Sexpr> for SexprChildrenArg {
    fn from(value: Sexpr) -> Self {
        SexprChildrenArg::SexprAtom(value)
    }
}

impl From<SexprList> for SexprChildrenArg {
    /// `operator<<( SEXPR_LIST&, SEXPR_LIST* )`
    fn from(value: SexprList) -> Self {
        SexprChildrenArg::SexprAtom(Sexpr::List(value))
    }
}

/// Port of `SEXPR::SEXPR_LIST`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SexprList {
    /// C++ `m_children`.
    pub children: Vec<Sexpr>,
    line_number: usize,
    /// C++ `m_inStreamChild`: index of the next child the extraction functions read.
    in_stream_child: i32,
}

impl SexprList {
    /// Port of `SEXPR_LIST::SEXPR_LIST()` (line number 1).
    pub fn new() -> Self {
        Self { children: Vec::new(), line_number: 1, in_stream_child: 0 }
    }

    /// Port of `SEXPR_LIST::SEXPR_LIST( int aLineNumber )`.
    pub fn with_line_number(line_number: i32) -> Self {
        Self { children: Vec::new(), line_number: line_number as usize, in_stream_child: 0 }
    }

    /// Port of the variadic constructor `SEXPR_LIST::SEXPR_LIST( const Args&... args )`: a list
    /// (line number 1) with one child per argument, see [`add_children`](Self::add_children).
    pub fn from_children<A: Into<SexprChildrenArg>>(args: impl IntoIterator<Item = A>) -> Self {
        let mut list = Self::new();

        list.add_children(args);
        list
    }

    /// Port of `SEXPR::GetLineNumber`.
    pub fn line_number(&self) -> usize {
        self.line_number
    }

    /// Port of `SEXPR::AddChild` on a list.
    pub fn add_child(&mut self, child: impl Into<Sexpr>) {
        self.children.push(child.into());
    }

    /// Port of `SEXPR::GetChildren` on a list.
    pub fn children(&self) -> &Vec<Sexpr> {
        &self.children
    }

    /// Port of `SEXPR::GetChild` on a list.
    ///
    /// # Errors
    ///
    /// [`SexprException::OutOfRange`] if there is no child `index` (C++ does not check).
    pub fn child(&self, index: usize) -> Result<&Sexpr, SexprException> {
        self.children
            .get(index)
            .ok_or(SexprException::OutOfRange { index, len: self.children.len() })
    }

    /// Port of `SEXPR::GetNumberOfChildren` on a list.
    pub fn number_of_children(&self) -> usize {
        self.children.len()
    }

    /// Port of `SEXPR::AsString` on a list.
    pub fn as_string(&self, level: usize) -> String {
        let mut result = String::new();
        let mut stack: Vec<(&SexprList, usize, usize)> = Vec::new();

        open_list(self, level, &mut result, &mut stack);
        write_remaining(&mut result, &mut stack);
        result
    }

    /// Port of `SEXPR_LIST::Scan( const Args&... args )` / `SEXPR_LIST::doScan`: match the
    /// first children of the list against `args`, storing values where an argument asks for
    /// it.
    ///
    /// Returns the number of arguments that matched: scanning stops at the first child that
    /// has the wrong type for its argument or differs from a
    /// [`StringComp`](SexprScanArg::StringComp) text.
    ///
    /// Differences to C++, both of which are undefined behaviour there: an
    /// [`Int`](SexprScanArg::Int) argument receives the integer (C++ writes a `double` through
    /// the `int32_t` pointer), and scanning stops if the list has fewer children than there are
    /// arguments (C++ reads beyond the end of the vector).
    pub fn scan(&self, args: &mut [SexprScanArg<'_>]) -> usize {
        let mut i = 0;

        while i < args.len() {
            let Some(child) = self.children.get(i) else {
                return i;
            };

            // Any INVALID_TYPE_EXCEPTION ends the scan: catch( const INVALID_TYPE_EXCEPTION& )
            match scan_one(child, &mut args[i]) {
                Ok(true) => {}
                Ok(false) | Err(_) => return i,
            }

            i += 1;
        }

        i
    }

    /// Port of `SEXPR_LIST::AddChildren( const Args&... args )` / `SEXPR_LIST::doAddChildren`:
    /// append one child per argument.
    pub fn add_children<A: Into<SexprChildrenArg>>(&mut self, args: impl IntoIterator<Item = A>) {
        for arg in args {
            match arg.into() {
                SexprChildrenArg::Double(value) => self.add_child(SexprDouble::new(value)),
                SexprChildrenArg::Int(value) => {
                    self.add_child(SexprInteger::new(i64::from(value)));
                }
                SexprChildrenArg::LongInt(value) => self.add_child(SexprInteger::new(value)),
                SexprChildrenArg::String(value) => self.add_child(SexprString::new(value)),
                SexprChildrenArg::SexprAtom(node) => self.add_child(node),
                SexprChildrenArg::SexprString(out_string) => {
                    if out_string.symbol {
                        self.add_child(SexprSymbol::new(out_string.string));
                    } else {
                        self.add_child(SexprString::new(out_string.string));
                    }
                }
            }
        }
    }

    /// Port of the stream insertion operators `operator<<( SEXPR_LIST& list, ... )` for
    /// `double`, `float`, `int64_t`, `int32_t`, `std::string`, `_OUT_STRING`, `SEXPR_LIST*` and
    /// `SEXPR*`: append `value` as a child and return the list, so that calls can be chained
    /// the way the C++ operators are: `list.push( 2 ).push( 42.42 ).push( "substring" )`.
    pub fn push(&mut self, value: impl Into<SexprChildrenArg>) -> &mut Self {
        self.add_children([value.into()]);
        self
    }

    /// Port of `operator<<( SEXPR_LIST& list, const ISEXPRABLE& obj )`: append the
    /// serialisation of `obj` as a child.
    ///
    /// If `obj` serialises to nothing (C++ `nullptr`), nothing is appended; C++ appends the
    /// null pointer, which later crashes `AsString()`.
    pub fn push_sexprable(&mut self, obj: &dyn ISexprable) -> &mut Self {
        if let Some(sobj) = obj.serialize_sexpr() {
            self.add_child(sobj);
        }

        self
    }

    /// The child the next extraction reads (C++ `input.GetChild( input.m_inStreamChild )`).
    fn in_stream_child(&self) -> Result<&Sexpr, SexprException> {
        let index = self.in_stream_child as usize;

        self.children
            .get(index)
            .ok_or(SexprException::OutOfRange { index, len: self.children.len() })
    }

    /// Port of `operator>>( SEXPR_LIST& input, ISEXPRABLE& obj )`: let `obj` read itself from
    /// this list.
    pub fn read_sexprable(&mut self, obj: &mut dyn ISexprable) -> &mut Self {
        obj.deserialize_sexpr(self);
        self
    }

    /// Port of `operator>>( SEXPR_LIST& input, int32_t& inte )`: read the next child, which
    /// must be an integer.
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "SEXPR is not a integer type!" )` otherwise.
    pub fn read_i32(&mut self) -> Result<i32, SexprException> {
        let child = self.in_stream_child()?;

        if child.is_integer() {
            let inte = child.integer()?;
            self.in_stream_child += 1;
            Ok(inte)
        } else {
            Err(SexprException::InvalidArgument("SEXPR is not a integer type!".to_string()))
        }
    }

    /// Port of `operator>>( SEXPR_LIST& input, std::string& str )`: read the next child, which
    /// must be a string.
    ///
    /// As in C++ a symbol child passes the type test of the operator but is then refused by
    /// `GetString()` with an `INVALID_TYPE_EXCEPTION`.
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "SEXPR is not a string type!" )` if the child is neither a
    /// string nor a symbol, `INVALID_TYPE_EXCEPTION` if it is a symbol.
    pub fn read_string(&mut self) -> Result<String, SexprException> {
        let child = self.in_stream_child()?;

        if child.is_string() || child.is_symbol() {
            let str = child.string()?.to_string();
            self.in_stream_child += 1;
            Ok(str)
        } else {
            Err(SexprException::InvalidArgument("SEXPR is not a string type!".to_string()))
        }
    }

    /// Port of `operator>>( SEXPR_LIST& input, int64_t& lint )`: read the next child, which
    /// must be an integer.
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "SEXPR is not a long integer type!" )` otherwise.
    pub fn read_i64(&mut self) -> Result<i64, SexprException> {
        let child = self.in_stream_child()?;

        if child.is_integer() {
            let lint = child.long_integer()?;
            self.in_stream_child += 1;
            Ok(lint)
        } else {
            Err(SexprException::InvalidArgument("SEXPR is not a long integer type!".to_string()))
        }
    }

    /// Port of `operator>>( SEXPR_LIST& input, float& fl )`: read the next child, which must be
    /// a double (an integer is not accepted here).
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "SEXPR is not a float type!" )` otherwise.
    pub fn read_f32(&mut self) -> Result<f32, SexprException> {
        let child = self.in_stream_child()?;

        if child.is_double() {
            let fl = child.float()?;
            self.in_stream_child += 1;
            Ok(fl)
        } else {
            Err(SexprException::InvalidArgument("SEXPR is not a float type!".to_string()))
        }
    }

    /// Port of `operator>>( SEXPR_LIST& input, double& dbl )`: read the next child, which must
    /// be a double (an integer is not accepted here).
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "SEXPR is not a double type!" )` otherwise.
    pub fn read_f64(&mut self) -> Result<f64, SexprException> {
        let child = self.in_stream_child()?;

        if child.is_double() {
            let dbl = child.double()?;
            self.in_stream_child += 1;
            Ok(dbl)
        } else {
            Err(SexprException::InvalidArgument("SEXPR is not a double type!".to_string()))
        }
    }

    /// Port of `operator>>( SEXPR_LIST& input, const _IN_STRING is )`: read the next child,
    /// which must be a symbol or a string as `is` demands, into `is.string`.
    ///
    /// # Errors
    ///
    /// `std::invalid_argument( "operator>>: SEXPR is not a symbol type!" )` resp.
    /// `std::invalid_argument( "SEXPR is not a string type!" )` otherwise.
    pub fn read_in_string(&mut self, is: InString<'_>) -> Result<&mut Self, SexprException> {
        let child = self.in_stream_child()?;

        if is.symbol {
            if child.is_symbol() {
                *is.string = child.symbol()?.to_string();
                self.in_stream_child += 1;
            } else {
                return Err(SexprException::InvalidArgument(
                    "operator>>: SEXPR is not a symbol type!".to_string(),
                ));
            }
        } else if child.is_string() {
            *is.string = child.string()?.to_string();
            self.in_stream_child += 1;
        } else {
            return Err(SexprException::InvalidArgument("SEXPR is not a string type!".to_string()));
        }

        Ok(self)
    }
}

impl Drop for SexprList {
    /// Port of `SEXPR_LIST::~SEXPR_LIST`, which deletes the children.  Done without recursion
    /// so that dropping a very deeply nested tree cannot overflow the stack.
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);

        while let Some(mut node) = pending.pop() {
            if let Sexpr::List(list) = &mut node {
                pending.append(&mut list.children);
            }
        }
    }
}

/// Port of `SEXPR::SEXPR`: a node of an s-expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    /// `SEXPR_LIST`
    List(SexprList),
    /// `SEXPR_INTEGER`
    Integer(SexprInteger),
    /// `SEXPR_DOUBLE`
    Double(SexprDouble),
    /// `SEXPR_STRING`
    String(SexprString),
    /// `SEXPR_SYMBOL`
    Symbol(SexprSymbol),
}

impl From<SexprList> for Sexpr {
    fn from(node: SexprList) -> Self {
        Sexpr::List(node)
    }
}

impl From<SexprInteger> for Sexpr {
    fn from(node: SexprInteger) -> Self {
        Sexpr::Integer(node)
    }
}

impl From<SexprDouble> for Sexpr {
    fn from(node: SexprDouble) -> Self {
        Sexpr::Double(node)
    }
}

impl From<SexprString> for Sexpr {
    fn from(node: SexprString) -> Self {
        Sexpr::String(node)
    }
}

impl From<SexprSymbol> for Sexpr {
    fn from(node: SexprSymbol) -> Self {
        Sexpr::Symbol(node)
    }
}

fn not_a_list() -> SexprException {
    InvalidTypeException::new("SEXPR is not a list type!").into()
}

impl Sexpr {
    /// The C++ `m_type` of the node.
    pub fn type_(&self) -> SexprType {
        match self {
            Sexpr::List(_) => SexprType::List,
            Sexpr::Integer(_) => SexprType::AtomInteger,
            Sexpr::Double(_) => SexprType::AtomDouble,
            Sexpr::String(_) => SexprType::AtomString,
            Sexpr::Symbol(_) => SexprType::AtomSymbol,
        }
    }

    /// Port of `SEXPR::IsList`.
    pub fn is_list(&self) -> bool {
        matches!(self, Sexpr::List(_))
    }

    /// Port of `SEXPR::IsSymbol`.
    pub fn is_symbol(&self) -> bool {
        matches!(self, Sexpr::Symbol(_))
    }

    /// Port of `SEXPR::IsString`.
    pub fn is_string(&self) -> bool {
        matches!(self, Sexpr::String(_))
    }

    /// Port of `SEXPR::IsDouble`.
    pub fn is_double(&self) -> bool {
        matches!(self, Sexpr::Double(_))
    }

    /// Port of `SEXPR::IsInteger`.
    pub fn is_integer(&self) -> bool {
        matches!(self, Sexpr::Integer(_))
    }

    /// Port of `SEXPR::AddChild`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a list type!" )` if this is not a list.
    pub fn add_child(&mut self, child: impl Into<Sexpr>) -> Result<(), SexprException> {
        match self {
            Sexpr::List(list) => {
                list.add_child(child);
                Ok(())
            }
            _ => Err(not_a_list()),
        }
    }

    /// Port of `SEXPR::GetChildren`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a list type!" )` if this is not a list.
    pub fn children(&self) -> Result<&Vec<Sexpr>, SexprException> {
        match self {
            Sexpr::List(list) => Ok(&list.children),
            _ => Err(not_a_list()),
        }
    }

    /// Port of `SEXPR::GetChild`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a list type!" )` if this is not a list;
    /// [`SexprException::OutOfRange`] if there is no child `index` (C++ does not check).
    pub fn child(&self, index: usize) -> Result<&Sexpr, SexprException> {
        match self {
            Sexpr::List(list) => list.child(index),
            _ => Err(not_a_list()),
        }
    }

    /// Port of `SEXPR::GetNumberOfChildren`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a list type!" )` if this is not a list.
    pub fn number_of_children(&self) -> Result<usize, SexprException> {
        match self {
            Sexpr::List(list) => Ok(list.children.len()),
            _ => Err(not_a_list()),
        }
    }

    /// Port of `SEXPR::GetString`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a string type!" )` if this is not a string.
    pub fn string(&self) -> Result<&str, SexprException> {
        match self {
            Sexpr::String(node) => Ok(&node.value),
            _ => Err(InvalidTypeException::new("SEXPR is not a string type!").into()),
        }
    }

    /// Port of `SEXPR::GetInteger`: the integer value converted to 32 bit, as by the C++
    /// `static_cast< int >`, i.e. truncated.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a integer type!" )` if this is not an integer.
    pub fn integer(&self) -> Result<i32, SexprException> {
        Ok(self.long_integer()? as i32)
    }

    /// Port of `SEXPR::GetLongInteger`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a integer type!" )` if this is not an integer.
    pub fn long_integer(&self) -> Result<i64, SexprException> {
        match self {
            Sexpr::Integer(node) => Ok(node.value),
            _ => Err(InvalidTypeException::new("SEXPR is not a integer type!").into()),
        }
    }

    /// Port of `SEXPR::GetDouble`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a double type!" )` if this is neither a double
    /// nor an integer.
    pub fn double(&self) -> Result<f64, SexprException> {
        // we may end up parsing "intended" floats/doubles as ints
        // so here we allow silent casting back to doubles
        match self {
            Sexpr::Double(node) => Ok(node.value),
            Sexpr::Integer(node) => Ok(node.value as f64),
            _ => Err(InvalidTypeException::new("SEXPR is not a double type!").into()),
        }
    }

    /// Port of `SEXPR::GetFloat`.
    ///
    /// # Errors
    ///
    /// As [`double`](Self::double).
    pub fn float(&self) -> Result<f32, SexprException> {
        Ok(self.double()? as f32)
    }

    /// Port of `SEXPR::GetSymbol`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "GetSymbol(): SEXPR is not a symbol type! error line <n>" )`
    /// if this is not a symbol.
    pub fn symbol(&self) -> Result<&str, SexprException> {
        match self {
            Sexpr::Symbol(node) => Ok(&node.value),
            _ => {
                let mut err_msg =
                    String::from("GetSymbol(): SEXPR is not a symbol type! error line ");
                err_msg += &self.line_number().to_string();
                Err(InvalidTypeException::new(err_msg).into())
            }
        }
    }

    /// Port of `SEXPR::GetList`.
    ///
    /// # Errors
    ///
    /// `INVALID_TYPE_EXCEPTION( "SEXPR is not a list type!" )` if this is not a list.
    pub fn list(&mut self) -> Result<&mut SexprList, SexprException> {
        match self {
            Sexpr::List(list) => Ok(list),
            _ => Err(not_a_list()),
        }
    }

    /// Port of `SEXPR::AsString`: the text of the tree.
    ///
    /// A list is written as `(` children separated by single spaces `)`; a list at a `level`
    /// other than 0 starts on a new line, indented by two spaces per level.  Strings are put
    /// between double quotes as they are (no escaping), integers are written as 32 bit values
    /// (`GetInteger()`), doubles with up to 10 significant digits and never in scientific
    /// notation.
    pub fn as_string(&self, level: usize) -> String {
        let mut result = String::new();
        let mut stack: Vec<(&SexprList, usize, usize)> = Vec::new();

        match self {
            Sexpr::List(list) => open_list(list, level, &mut result, &mut stack),
            atom => write_atom(atom, &mut result),
        }

        write_remaining(&mut result, &mut stack);
        result
    }

    /// Port of `SEXPR::GetLineNumber`: the line of the input the node was parsed from (1 for
    /// nodes that were not made by the parser).
    pub fn line_number(&self) -> usize {
        match self {
            Sexpr::List(node) => node.line_number,
            Sexpr::Integer(node) => node.line_number,
            Sexpr::Double(node) => node.line_number,
            Sexpr::String(node) => node.line_number,
            Sexpr::Symbol(node) => node.line_number,
        }
    }
}

/// The body of the loop of `SEXPR_LIST::doScan` for one child and its argument: `Ok( false )`
/// for a failed string comparison, `Err` where C++ throws `INVALID_TYPE_EXCEPTION`.
fn scan_one(child: &Sexpr, arg: &mut SexprScanArg<'_>) -> Result<bool, SexprException> {
    match arg {
        SexprScanArg::Double(value) => **value = child.double()?,
        SexprScanArg::Int(value) => **value = child.integer()?,
        SexprScanArg::String(value) => {
            if child.is_symbol() {
                **value = child.symbol()?.to_string();
            } else if child.is_string() {
                **value = child.string()?.to_string();
            }
        }
        SexprScanArg::LongInt(value) => **value = child.long_integer()?,
        SexprScanArg::SexprString(in_string) => {
            if in_string.symbol {
                *in_string.string = child.symbol()?.to_string();
            } else {
                *in_string.string = child.string()?.to_string();
            }
        }
        SexprScanArg::StringComp(expected) => {
            if child.is_symbol() {
                if child.symbol()? != expected.as_str() {
                    return Ok(false);
                }
            } else if child.is_string() {
                if child.string()? != expected.as_str() {
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
}

/// The list part of `SEXPR::AsString` up to and including the opening parenthesis.
fn open_list<'a>(
    list: &'a SexprList,
    level: usize,
    result: &mut String,
    stack: &mut Vec<(&'a SexprList, usize, usize)>,
) {
    if level != 0 {
        result.push('\n');
    }

    result.extend(std::iter::repeat_n(' ', level * 2));
    result.push('(');

    // (list, index of the next child to write, level of the children)
    stack.push((list, 0, level + 1));
}

/// The atom branches of `SEXPR::AsString`.
fn write_atom(atom: &Sexpr, result: &mut String) {
    match atom {
        Sexpr::List(_) => {}
        Sexpr::String(node) => {
            result.push('"');
            result.push_str(&node.value);
            result.push('"');
        }
        Sexpr::Symbol(node) => result.push_str(&node.value),
        Sexpr::Integer(node) => {
            // out << GetInteger();
            result.push_str(&(node.value as i32).to_string());
        }
        Sexpr::Double(node) => {
            // This code does the same thing as FormatDouble2Str() in string_utils.cpp
            // Unfortunately on msys2 currently trying to use FormatDouble2Str creates a link
            // issue. So duplicate a similar code here for now.
            let mut out = format_g(node.value, 10);

            // Scientific notation not allowed. So detect and change it
            if out.contains('e') {
                out = format_f(node.value, 10);

                // remove trailing zeros
                while out.ends_with('0') {
                    out.pop();
                }

                // we may have just stripped all the zeros after the decimal
                if out.len() > 1 && out.ends_with('.') {
                    out.pop();
                }
            }

            result.push_str(&out);
        }
    }
}

/// The recursion of `SEXPR::AsString` over the children of the open lists on `stack`, done
/// iteratively so that a very deeply nested tree cannot overflow the call stack.
fn write_remaining<'a>(result: &mut String, stack: &mut Vec<(&'a SexprList, usize, usize)>) {
    while let Some((list, index, child_level)) = stack.last_mut() {
        let list: &'a SexprList = list;

        if *index < list.children.len() {
            // result += " " after every child but the last one.
            if *index > 0 {
                result.push(' ');
            }

            let child = &list.children[*index];
            let level = *child_level;
            *index += 1;

            match child {
                Sexpr::List(child_list) => open_list(child_list, level, result, stack),
                atom => write_atom(atom, result),
            }
        } else {
            result.push(')');
            stack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double_text(value: f64) -> String {
        Sexpr::from(SexprDouble::new(value)).as_string(0)
    }

    #[test]
    fn double_formatting() {
        assert_eq!(double_text(3.14), "3.14");
        assert_eq!(double_text(42.42), "42.42");
        assert_eq!(double_text(0.0), "0");
        assert_eq!(double_text(-0.0), "-0");
        assert_eq!(double_text(1.0), "1");
        assert_eq!(double_text(-2.5), "-2.5");
        assert_eq!(double_text(0.1 + 0.2), "0.3");
        assert_eq!(double_text(1.0 / 3.0), "0.3333333333");
        assert_eq!(double_text(123456789.123456789), "123456789.1");
        assert_eq!(double_text(9999999999.0), "9999999999");
        // {:.10g} would use scientific notation here: {:.10f} with trailing zeros removed.
        assert_eq!(double_text(1e10), "10000000000");
        assert_eq!(double_text(1.5e10), "15000000000");
        assert_eq!(double_text(1e20), "100000000000000000000");
        assert_eq!(double_text(0.00001), "0.00001");
        assert_eq!(double_text(0.0000123456), "0.0000123456");
        assert_eq!(double_text(1.23456789e-7), "0.0000001235");
        assert_eq!(double_text(1e-11), "0");
        assert_eq!(double_text(-1e-11), "-0");
        assert_eq!(double_text(-1.5e12), "-1500000000000");
        assert_eq!(double_text(f64::INFINITY), "inf");
        assert_eq!(double_text(f64::NEG_INFINITY), "-inf");
        assert_eq!(double_text(f64::NAN), "nan");
    }

    #[test]
    fn integer_is_written_as_32_bit() {
        assert_eq!(Sexpr::from(SexprInteger::new(42)).as_string(0), "42");
        assert_eq!(Sexpr::from(SexprInteger::new(-42)).as_string(0), "-42");
        assert_eq!(Sexpr::from(SexprInteger::new(i64::from(i32::MAX))).as_string(0), "2147483647");
        // out << GetInteger() truncates
        assert_eq!(Sexpr::from(SexprInteger::new(1 << 32)).as_string(0), "0");
        assert_eq!(Sexpr::from(SexprInteger::new((1 << 31) + 5)).as_string(0), "-2147483643");
    }

    #[test]
    fn string_is_not_escaped() {
        assert_eq!(Sexpr::from(SexprString::new("a \"b\" \\ c")).as_string(0), "\"a \"b\" \\ c\"");
    }

    #[test]
    fn nested_levels() {
        let mut inner = SexprList::new();
        inner.push(as_symbol("c"));

        let mut middle = SexprList::new();
        middle.push(as_symbol("b")).push(inner).push(1);

        let mut outer = SexprList::new();
        outer.push(as_symbol("a")).push(middle);

        assert_eq!(outer.as_string(0), "(a \n  (b \n    (c) 1))");
        assert_eq!(outer.as_string(1), "\n  (a \n    (b \n      (c) 1))");
        assert_eq!(Sexpr::from(outer.clone()).as_string(0), outer.as_string(0));
        assert_eq!(SexprList::new().as_string(0), "()");
        assert_eq!(SexprList::new().as_string(2), "\n    ()");
    }

    #[test]
    fn typed_accessors_and_errors() {
        let int = Sexpr::from(SexprInteger::new(7));
        let dbl = Sexpr::from(SexprDouble::new(2.5));
        let string = Sexpr::from(SexprString::new("s"));
        let symbol = Sexpr::from(SexprSymbol::with_line_number("sym", 12));
        let mut list = Sexpr::from(SexprList::new());

        assert_eq!(int.type_(), SexprType::AtomInteger);
        assert_eq!(dbl.type_(), SexprType::AtomDouble);
        assert_eq!(string.type_(), SexprType::AtomString);
        assert_eq!(symbol.type_(), SexprType::AtomSymbol);
        assert_eq!(list.type_(), SexprType::List);

        assert_eq!(int.long_integer().unwrap(), 7);
        assert_eq!(int.integer().unwrap(), 7);
        // integers silently convert to doubles, not the other way round
        assert_eq!(int.double().unwrap(), 7.0);
        assert_eq!(int.float().unwrap(), 7.0);
        assert_eq!(dbl.double().unwrap(), 2.5);
        assert_eq!(dbl.float().unwrap(), 2.5);
        assert_eq!(string.string().unwrap(), "s");
        assert_eq!(symbol.symbol().unwrap(), "sym");
        assert_eq!(symbol.line_number(), 12);
        assert_eq!(int.line_number(), 1);

        assert_eq!(dbl.long_integer().unwrap_err().what(), "SEXPR is not a integer type!");
        assert_eq!(string.double().unwrap_err().what(), "SEXPR is not a double type!");
        assert_eq!(symbol.string().unwrap_err().what(), "SEXPR is not a string type!");
        assert_eq!(
            string.symbol().unwrap_err().what(),
            "GetSymbol(): SEXPR is not a symbol type! error line 1"
        );
        assert_eq!(int.children().unwrap_err().what(), "SEXPR is not a list type!");
        assert_eq!(int.child(0).unwrap_err().what(), "SEXPR is not a list type!");
        assert_eq!(int.number_of_children().unwrap_err().what(), "SEXPR is not a list type!");
        assert!(matches!(
            int.clone().add_child(SexprInteger::new(1)),
            Err(SexprException::InvalidType(_))
        ));
        assert!(int.clone().list().is_err());

        list.add_child(SexprInteger::new(1)).unwrap();
        assert_eq!(list.number_of_children().unwrap(), 1);
        assert_eq!(list.children().unwrap().len(), 1);
        assert!(list.child(0).unwrap().is_integer());
        assert_eq!(list.child(1).unwrap_err(), SexprException::OutOfRange { index: 1, len: 1 });
        assert_eq!(list.list().unwrap().number_of_children(), 1);
    }

    #[test]
    fn add_children_and_variadic_constructor() {
        let list = SexprList::from_children([
            SexprChildrenArg::from(1i32),
            SexprChildrenArg::from(2i64),
            SexprChildrenArg::from(3.5f64),
            SexprChildrenArg::from("str"),
            SexprChildrenArg::from(as_symbol("sym")),
            SexprChildrenArg::from(as_string("str2")),
            SexprChildrenArg::from(Sexpr::from(SexprSymbol::new("node"))),
            SexprChildrenArg::from(1.5f32),
        ]);

        assert_eq!(list.number_of_children(), 8);
        assert_eq!(list.line_number(), 1);
        assert_eq!(list.as_string(0), "(1 2 3.5 \"str\" sym \"str2\" node 1.5)");
    }

    #[test]
    fn stream_extraction() {
        let mut list = SexprList::new();
        list.push(as_symbol("name")).push("text").push(5).push(6i64).push(1.5).push(2.5).push(9);

        let mut symbol = String::new();
        list.read_in_string(as_symbol_in(&mut symbol)).unwrap();
        assert_eq!(symbol, "name");

        let mut string = String::new();
        // wrong kind: nothing is consumed
        assert_eq!(
            list.read_in_string(as_symbol_in(&mut string)).unwrap_err().what(),
            "operator>>: SEXPR is not a symbol type!"
        );
        list.read_in_string(as_string_in(&mut string)).unwrap();
        assert_eq!(string, "text");

        assert_eq!(list.read_f64().unwrap_err().what(), "SEXPR is not a double type!");
        assert_eq!(list.read_f32().unwrap_err().what(), "SEXPR is not a float type!");
        assert_eq!(list.read_string().unwrap_err().what(), "SEXPR is not a string type!");
        assert_eq!(list.read_i32().unwrap(), 5);
        assert_eq!(list.read_i64().unwrap(), 6);
        assert_eq!(list.read_i32().unwrap_err().what(), "SEXPR is not a integer type!");
        assert_eq!(list.read_i64().unwrap_err().what(), "SEXPR is not a long integer type!");
        assert_eq!(list.read_f64().unwrap(), 1.5);
        assert_eq!(list.read_f32().unwrap(), 2.5);
        assert_eq!(list.read_i32().unwrap(), 9);

        // past the end
        assert_eq!(list.read_i32().unwrap_err(), SexprException::OutOfRange { index: 7, len: 7 });
    }

    #[test]
    fn read_string_refuses_symbols_like_cpp() {
        let mut list = SexprList::new();
        list.push(as_symbol("sym")).push("str");

        // IsSymbol() passes the test of operator>>, GetString() then throws.
        assert!(matches!(list.read_string(), Err(SexprException::InvalidType(_))));

        let mut symbol = String::new();
        list.read_in_string(as_symbol_in(&mut symbol)).unwrap();
        assert_eq!(list.read_string().unwrap(), "str");
    }

    #[test]
    fn scan() {
        let mut list = SexprList::new();
        list.push(as_symbol("at")).push(10).push(2.5).push("text").push(1i64 << 40);

        let mut x = 0i32;
        let mut y = 0.0f64;
        let mut text = String::new();
        let mut big = 0i64;

        let matched = list.scan(&mut [
            "at".into(),
            (&mut x).into(),
            (&mut y).into(),
            (&mut text).into(),
            (&mut big).into(),
        ]);

        assert_eq!(matched, 5);
        assert_eq!(x, 10);
        assert_eq!(y, 2.5);
        assert_eq!(text, "text");
        assert_eq!(big, 1 << 40);

        // A differing keyword stops the scan at its index.
        assert_eq!(list.scan(&mut ["xy".into(), (&mut x).into()]), 0);

        // A type mismatch stops the scan as well.
        let mut wrong = 0i32;
        assert_eq!(list.scan(&mut ["at".into(), (&mut x).into(), (&mut wrong).into()]), 2);

        // An integer child satisfies a double argument.
        let mut d = 0.0;
        assert_eq!(list.scan(&mut ["at".into(), (&mut d).into()]), 2);
        assert_eq!(d, 10.0);

        // _IN_STRING arguments demand the exact kind.
        let mut s = String::new();
        assert_eq!(list.scan(&mut [as_string_in(&mut s).into()]), 0);
        assert_eq!(list.scan(&mut [as_symbol_in(&mut s).into()]), 1);
        assert_eq!(s, "at");

        // A string comparison against a non-string child is skipped, not failed.
        assert_eq!(list.scan(&mut ["at".into(), "ten".into()]), 2);

        // More arguments than children.
        let mut list = SexprList::new();
        list.push(1);
        assert_eq!(list.scan(&mut [(&mut x).into(), (&mut wrong).into()]), 1);
    }

    struct Point {
        x: i32,
        y: i32,
    }

    impl ISexprable for Point {
        fn serialize_sexpr(&self) -> Option<Sexpr> {
            let mut list = SexprList::new();
            list.push(as_symbol("xy")).push(self.x).push(self.y);
            Some(list.into())
        }

        fn deserialize_sexpr(&mut self, sexp: &mut SexprList) {
            let mut keyword = String::new();

            if sexp.read_in_string(as_symbol_in(&mut keyword)).is_ok() {
                self.x = sexp.read_i32().unwrap_or(0);
                self.y = sexp.read_i32().unwrap_or(0);
            }
        }
    }

    struct Nothing;

    impl ISexprable for Nothing {}

    #[test]
    fn isexprable_round_trip() {
        let mut list = SexprList::new();
        list.push_sexprable(&Point { x: 3, y: -4 }).push_sexprable(&Nothing);

        assert_eq!(list.as_string(0), "(\n  (xy 3 -4))");

        let mut point = Point { x: 0, y: 0 };
        let Sexpr::List(inner) = &mut list.children[0] else { panic!("not a list") };
        inner.read_sexprable(&mut point);

        assert_eq!((point.x, point.y), (3, -4));
    }

    #[test]
    fn deep_trees_do_not_overflow_the_stack() {
        // Dropping must not recurse.
        let mut node = SexprList::new();

        for _ in 0..500_000 {
            let mut parent = SexprList::new();
            parent.add_child(node);
            node = parent;
        }

        drop(node);

        // Neither must AsString() (the text grows with the square of the depth, hence the
        // smaller tree).
        let mut node = SexprList::new();

        for _ in 0..3_000 {
            let mut parent = SexprList::new();
            parent.add_child(node);
            node = parent;
        }

        let text = node.as_string(0);
        assert!(text.starts_with("(\n  (\n    ("));
        assert!(text.ends_with(")))"));
        assert_eq!(text.matches('(').count(), 3_001);
    }
}
