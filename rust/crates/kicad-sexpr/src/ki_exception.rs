// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007-2016 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of `include/ki_exception.h` and `common/exceptions.cpp`.
//!
//! The C++ exception hierarchy
//!
//! ```text
//! IO_ERROR
//!  +- IO_CANCELLED
//!  +- PARSE_ERROR
//!      +- FUTURE_FORMAT_ERROR
//! ```
//!
//! becomes the enum [`IoError`], whose variants carry the data members of the corresponding C++
//! class.  A function that `throw`s any of them in C++ returns `Result<_, IoError>` here; a C++
//! `catch( const PARSE_ERROR& )` is a match on [`IoError::as_parse_error`] (which, like the C++
//! handler, also sees a `FUTURE_FORMAT_ERROR`).
//!
//! The translatable message texts (`_( "..." )`) are the untranslated English originals.
//!
//! `IO_ERROR::Where()` is `"from <file> : <function>() line <n>"` of the C++ throw site.  The port
//! records the Rust source location of the constructor call instead (file name without directory
//! and line, no function name), see [`IoErrorBase::where_`].  It only shows up in
//! [`IoError::what_debug`]; the release-build `What()` text is the problem text alone.

use std::fmt;
use std::panic::Location;

/// Port of `KI_PARAM_ERROR`: holds a translatable error message (similar to
/// `std::invalid_argument`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KiParamError {
    message: String,
}

impl KiParamError {
    /// Port of `KI_PARAM_ERROR::KI_PARAM_ERROR( const wxString& aMessage )`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Port of `KI_PARAM_ERROR::What`.
    pub fn what(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for KiParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for KiParamError {}

/// Format the `where` text of an error from a Rust source location.
///
/// C++ formats `"from %s : %s() line %d"` with the short file name, function and line of the
/// throw site; Rust has no function name available, so that part is left out.
fn where_from_location(location: &Location<'_>) -> String {
    let file = location.file();
    // srcname.AfterLast( '/' ); also strip Windows separators for good measure.
    let short = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("from {} : line {}", short, location.line())
}

/// The data members of the C++ `IO_ERROR` base class: the problem text and the throw site.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IoErrorBase {
    problem: String,
    where_: String,
}

impl IoErrorBase {
    /// Port of `IO_ERROR::init`.
    #[track_caller]
    pub fn new(problem: impl Into<String>) -> Self {
        Self {
            problem: problem.into(),
            where_: where_from_location(Location::caller()),
        }
    }

    /// Port of `IO_ERROR::Problem`: what was the problem?
    pub fn problem(&self) -> &str {
        &self.problem
    }

    /// Port of `IO_ERROR::Where`: where did the problem occur (source location of the code that
    /// raised the error; see the module documentation for the difference to C++).
    pub fn where_(&self) -> &str {
        &self.where_
    }
}

/// Port of `PARSE_ERROR`: a filename or source description, a problem input line, a line number,
/// a byte offset, and an error message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseError {
    base: IoErrorBase,

    /// At which line number, 1 based index (C++ `lineNumber`).
    pub line_number: i32,

    /// At which byte offset within the line, 1 based index (C++ `byteIndex`).
    pub byte_index: i32,

    /// Problem line of input (say, from a `LineReader`) (C++ `inputLine`).
    pub input_line: String,

    parse_problem: String,
}

impl ParseError {
    /// Port of `PARSE_ERROR::PARSE_ERROR` / `PARSE_ERROR::init`, normally reached through the
    /// C++ macro `THROW_PARSE_ERROR( aProblem, aSource, aInputLine, aLineNumber, aByteIndex )`.
    ///
    /// The composed [`problem`](Self::problem) text is
    /// `"<aProblem> in '<aSource>', line <aLineNumber>, offset <aByteIndex>."`.
    #[track_caller]
    pub fn new(
        problem: impl Into<String>,
        source: &str,
        input_line: &str,
        line_number: i32,
        byte_index: i32,
    ) -> Self {
        let parse_problem = problem.into();

        // problem.Printf( _( "%s in '%s', line %d, offset %d." ), ... )
        let problem = format!(
            "{} in '{}', line {}, offset {}.",
            parse_problem, source, line_number, byte_index
        );

        // inputLine = aInputLine: a C string, i.e. it ends at the first NUL.
        let input_line = match input_line.find('\0') {
            Some(nul) => &input_line[..nul],
            None => input_line,
        };

        Self {
            base: IoErrorBase {
                problem,
                where_: where_from_location(Location::caller()),
            },
            line_number,
            byte_index,
            input_line: input_line.to_string(),
            parse_problem,
        }
    }

    /// Port of `PARSE_ERROR::ParseProblem`: the bare problem text, without the location suffix.
    pub fn parse_problem(&self) -> &str {
        &self.parse_problem
    }

    /// Port of `IO_ERROR::Problem`: the full problem text including source, line and offset.
    pub fn problem(&self) -> &str {
        &self.base.problem
    }

    /// Port of `IO_ERROR::Where`.
    pub fn where_(&self) -> &str {
        &self.base.where_
    }

    /// Port of `IO_ERROR::What` (release build: the same as [`problem`](Self::problem)).
    pub fn what(&self) -> &str {
        &self.base.problem
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.what())
    }
}

impl std::error::Error for ParseError {}

/// Port of `FUTURE_FORMAT_ERROR`: variant of `PARSE_ERROR` indicating that a syntax or related
/// error was likely caused by a file generated by a newer version of KiCad than this.  This can
/// be used to generate more informative error messages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FutureFormatError {
    parse: ParseError,

    /// Date of KiCad file format required to open file (C++ `requiredVersion`).
    pub required_version: String,

    /// Version of KiCad required to open file (C++ `requiredGenerator`).
    pub required_generator: String,
}

impl FutureFormatError {
    /// Port of `FUTURE_FORMAT_ERROR::init`.
    fn init(&mut self, required_version: &str, required_generator: &str) {
        self.required_version = required_version.to_string();
        self.required_generator = required_generator.to_string();

        if self.required_generator.is_empty() {
            self.parse.base.problem = format!(
                "KiCad was unable to open this file because it was created with a more \
                 recent version than the one you are running.\n\n\
                 To open it you will need to upgrade KiCad to a version dated {} or \
                 later.",
                required_version
            );
        } else {
            self.parse.base.problem = format!(
                "KiCad was unable to open this file because it was created with a more \
                 recent version than the one you are running.\n\n\
                 To open it you will need to upgrade KiCad to version {} or \
                 later (file format dated {} or later).",
                required_generator, required_version
            );
        }
    }

    /// Port of `FUTURE_FORMAT_ERROR::FUTURE_FORMAT_ERROR( const wxString& aRequiredVersion,
    /// const wxString& aRequiredGenerator )`.  Pass an empty string for the C++ default
    /// `wxEmptyString` generator.
    pub fn new(required_version: &str, required_generator: &str) -> Self {
        let mut this = Self::default();

        this.init(required_version, required_generator);

        this.parse.line_number = 0;
        this.parse.byte_index = 0;
        this
    }

    /// Port of `FUTURE_FORMAT_ERROR::FUTURE_FORMAT_ERROR( const PARSE_ERROR& aParseError,
    /// const wxString& aRequiredVersion, const wxString& aRequiredGenerator )` for an argument
    /// that is a plain `PARSE_ERROR` (the `dynamic_cast` fails).
    pub fn from_parse_error(
        parse_error: &ParseError,
        required_version: &str,
        required_generator: &str,
    ) -> Self {
        let mut this = Self::default();

        this.init(required_version, required_generator);

        if !parse_error.problem().is_empty() {
            this.parse.base.problem.push_str("\n\n");
            this.parse.base.problem.push_str("Full error text:");
            this.parse.base.problem.push('\n');
            this.parse.base.problem.push_str(parse_error.problem());
        }

        this.parse.line_number = parse_error.line_number;
        this.parse.byte_index = parse_error.byte_index;
        this.parse.input_line = parse_error.input_line.clone();
        this
    }

    /// Port of the same C++ constructor as [`from_parse_error`](Self::from_parse_error) for an
    /// argument that already is a `FUTURE_FORMAT_ERROR` (the `dynamic_cast` succeeds): the
    /// required version/generator and the problem text of `error` are kept and the version
    /// arguments of the C++ constructor are ignored.
    pub fn from_future_format_error(error: &FutureFormatError) -> Self {
        let mut this = Self {
            required_version: error.required_version.clone(),
            required_generator: error.required_generator.clone(),
            ..Self::default()
        };

        this.parse.base.problem = error.problem().to_string();

        this.parse.line_number = error.parse.line_number;
        this.parse.byte_index = error.parse.byte_index;
        this.parse.input_line = error.parse.input_line.clone();
        this
    }

    /// The `PARSE_ERROR` base class sub-object.
    pub fn as_parse_error(&self) -> &ParseError {
        &self.parse
    }

    /// Port of `IO_ERROR::Problem`.
    pub fn problem(&self) -> &str {
        self.parse.problem()
    }

    /// Port of `IO_ERROR::What` (release build: the same as [`problem`](Self::problem)).
    pub fn what(&self) -> &str {
        self.parse.problem()
    }
}

impl fmt::Display for FutureFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.what())
    }
}

impl std::error::Error for FutureFormatError {}

/// Port of `IO_ERROR` and its subclasses: holds an error message and may be used when returning
/// errors containing meaningful error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoError {
    /// A plain `IO_ERROR`.
    Io(IoErrorBase),

    /// `IO_CANCELLED`: the user cancelled an interactive part of a load, import, or export,
    /// such as declining a query dialog or dismissing a progress dialog, rather than an error
    /// in the input.
    Cancelled(IoErrorBase),

    /// `PARSE_ERROR`.  (Boxed to keep `Result<_, IoError>` small.)
    Parse(Box<ParseError>),

    /// `FUTURE_FORMAT_ERROR`.  (Boxed to keep `Result<_, IoError>` small.)
    FutureFormat(Box<FutureFormatError>),
}

impl IoError {
    /// Port of the C++ macro `THROW_IO_ERROR( msg )` (construct the error; the caller returns
    /// it).  `THROW_IO_ERRORF( fmt, ... )` is `IoError::new( format!( ... ) )`.
    #[track_caller]
    pub fn new(problem: impl Into<String>) -> Self {
        IoError::Io(IoErrorBase::new(problem))
    }

    /// Port of the C++ macro `THROW_IO_CANCELLED()`: an `IO_CANCELLED` with the default problem
    /// text "Canceled by user.".
    #[track_caller]
    pub fn cancelled() -> Self {
        IoError::Cancelled(IoErrorBase::new("Canceled by user."))
    }

    /// Port of the C++ macro `THROW_IO_CANCELLED_MSG( msg )`.  An empty message selects the
    /// default text, as in `IO_CANCELLED::IO_CANCELLED`.
    #[track_caller]
    pub fn cancelled_msg(problem: impl Into<String>) -> Self {
        let problem = problem.into();

        if problem.is_empty() {
            IoError::Cancelled(IoErrorBase::new("Canceled by user."))
        } else {
            IoError::Cancelled(IoErrorBase::new(problem))
        }
    }

    /// Port of the C++ macro `THROW_PARSE_ERROR( aProblem, aSource, aInputLine, aLineNumber,
    /// aByteIndex )` (construct the error; the caller returns it).
    #[track_caller]
    pub fn parse(
        problem: impl Into<String>,
        source: &str,
        input_line: &str,
        line_number: i32,
        byte_index: i32,
    ) -> Self {
        IoError::Parse(Box::new(ParseError::new(
            problem,
            source,
            input_line,
            line_number,
            byte_index,
        )))
    }

    fn base(&self) -> &IoErrorBase {
        match self {
            IoError::Io(base) | IoError::Cancelled(base) => base,
            IoError::Parse(parse) => &parse.base,
            IoError::FutureFormat(future) => &future.parse.base,
        }
    }

    /// Port of `IO_ERROR::Problem`: what was the problem?
    pub fn problem(&self) -> &str {
        self.base().problem()
    }

    /// Port of `IO_ERROR::Where`: where did the `problem()` occur?
    pub fn where_(&self) -> &str {
        self.base().where_()
    }

    /// Port of `IO_ERROR::What` as compiled in a release build of KiCad (no `DEBUG` define):
    /// the same as [`problem`](Self::problem).
    pub fn what(&self) -> &str {
        self.problem()
    }

    /// Port of `IO_ERROR::What` as compiled in a `DEBUG` build of KiCad: a composite of
    /// `Problem()` and `Where()`.
    pub fn what_debug(&self) -> String {
        format!("IO_ERROR: {}\n\n{}", self.problem(), self.where_())
    }

    /// The `PARSE_ERROR` part of this error, if it is a `PARSE_ERROR` or a
    /// `FUTURE_FORMAT_ERROR` (what a C++ `catch( const PARSE_ERROR& )` would receive).
    pub fn as_parse_error(&self) -> Option<&ParseError> {
        match self {
            IoError::Parse(parse) => Some(parse),
            IoError::FutureFormat(future) => Some(&future.parse),
            IoError::Io(_) | IoError::Cancelled(_) => None,
        }
    }

    /// The `FUTURE_FORMAT_ERROR` part of this error, if it is one.
    pub fn as_future_format_error(&self) -> Option<&FutureFormatError> {
        match self {
            IoError::FutureFormat(future) => Some(future),
            _ => None,
        }
    }

    /// True for an `IO_CANCELLED`.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, IoError::Cancelled(_))
    }

    /// Port of `FUTURE_FORMAT_ERROR::FUTURE_FORMAT_ERROR( const PARSE_ERROR& aParseError,
    /// const wxString& aRequiredVersion, const wxString& aRequiredGenerator )` applied to this
    /// error.  Returns `None` if this error is not a `PARSE_ERROR` (C++ could not make the call).
    pub fn to_future_format_error(
        &self,
        required_version: &str,
        required_generator: &str,
    ) -> Option<FutureFormatError> {
        match self {
            IoError::Parse(parse) => Some(FutureFormatError::from_parse_error(
                parse,
                required_version,
                required_generator,
            )),
            IoError::FutureFormat(future) => {
                Some(FutureFormatError::from_future_format_error(future))
            }
            IoError::Io(_) | IoError::Cancelled(_) => None,
        }
    }
}

impl fmt::Display for IoError {
    /// Port of `IO_ERROR::what` (the `std::exception` interface): the problem text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.what())
    }
}

impl std::error::Error for IoError {}

impl From<ParseError> for IoError {
    fn from(error: ParseError) -> Self {
        IoError::Parse(Box::new(error))
    }
}

impl From<FutureFormatError> for IoError {
    fn from(error: FutureFormatError) -> Self {
        IoError::FutureFormat(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_problem_and_what() {
        let e = IoError::new("Line length exceeded");

        assert_eq!(e.problem(), "Line length exceeded");
        assert_eq!(e.what(), "Line length exceeded");
        assert_eq!(e.to_string(), "Line length exceeded");
        assert!(e.where_().starts_with("from ki_exception.rs : line "));
        assert_eq!(
            e.what_debug(),
            format!("IO_ERROR: Line length exceeded\n\n{}", e.where_())
        );
        assert!(e.as_parse_error().is_none());
        assert!(!e.is_cancelled());
    }

    #[test]
    fn io_cancelled_default_text() {
        assert_eq!(IoError::cancelled().problem(), "Canceled by user.");
        assert_eq!(IoError::cancelled_msg("").problem(), "Canceled by user.");
        assert_eq!(IoError::cancelled_msg("stop").problem(), "stop");
        assert!(IoError::cancelled().is_cancelled());
    }

    #[test]
    fn parse_error_layout() {
        let e = ParseError::new("Expecting '('", "board.kicad_pcb", "(kicad_pcb\n", 12, 5);

        assert_eq!(e.parse_problem(), "Expecting '('");
        assert_eq!(
            e.problem(),
            "Expecting '(' in 'board.kicad_pcb', line 12, offset 5."
        );
        assert_eq!(e.what(), e.problem());
        assert_eq!(e.line_number, 12);
        assert_eq!(e.byte_index, 5);
        assert_eq!(e.input_line, "(kicad_pcb\n");

        let io: IoError = e.clone().into();
        assert_eq!(io.as_parse_error(), Some(&e));
        assert_eq!(io.problem(), e.problem());
    }

    #[test]
    fn parse_error_input_line_stops_at_nul() {
        let e = ParseError::new("x", "s", "abc\0def\n", 1, 1);
        assert_eq!(e.input_line, "abc");
    }

    #[test]
    fn future_format_error_texts() {
        let e = FutureFormatError::new("20250101", "");
        assert_eq!(
            e.problem(),
            "KiCad was unable to open this file because it was created with a more recent \
             version than the one you are running.\n\nTo open it you will need to upgrade \
             KiCad to a version dated 20250101 or later."
        );
        assert_eq!(e.as_parse_error().line_number, 0);
        assert_eq!(e.as_parse_error().byte_index, 0);

        let e = FutureFormatError::new("20250101", "10.0");
        assert_eq!(
            e.problem(),
            "KiCad was unable to open this file because it was created with a more recent \
             version than the one you are running.\n\nTo open it you will need to upgrade \
             KiCad to version 10.0 or later (file format dated 20250101 or later)."
        );
    }

    #[test]
    fn future_format_error_from_parse_error() {
        let pe = ParseError::new("Unexpected 'foo'", "f", "line\n", 3, 9);
        let e = FutureFormatError::from_parse_error(&pe, "20250101", "");

        assert!(e.problem().ends_with(
            "or later.\n\nFull error text:\nUnexpected 'foo' in 'f', line 3, offset 9."
        ));
        assert_eq!(e.as_parse_error().line_number, 3);
        assert_eq!(e.as_parse_error().byte_index, 9);
        assert_eq!(e.as_parse_error().input_line, "line\n");
        assert_eq!(e.required_version, "20250101");

        // dynamic_cast branch: everything is taken from the existing FUTURE_FORMAT_ERROR.
        let io: IoError = e.clone().into();
        let again = io.to_future_format_error("1", "2").unwrap();
        assert_eq!(again.problem(), e.problem());
        assert_eq!(again.required_version, "20250101");
        assert_eq!(again.required_generator, "");
        assert_eq!(again.as_parse_error().line_number, 3);

        assert!(IoError::new("x").to_future_format_error("1", "2").is_none());
    }

    #[test]
    fn ki_param_error() {
        let e = KiParamError::new("bad parameter");
        assert_eq!(e.what(), "bad parameter");
        assert_eq!(e.to_string(), "bad parameter");
    }
}
