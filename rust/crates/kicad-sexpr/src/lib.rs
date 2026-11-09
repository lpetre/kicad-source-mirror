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

//! KiCad s-expression lexer (DSNLEXER-compatible), generic tree and formatter.
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and this crate's
//! `PORT_STATUS.md`.
//!
//! | module | ports |
//! |---|---|
//! | [`ki_exception`] | `include/ki_exception.h`, `common/exceptions.cpp`: `IO_ERROR`, `PARSE_ERROR`, ... |
//! | [`lexer`] | `DSNLEXER`, `LINE_READER`/`STRING_LINE_READER`, the `*.keywords` token enums |
//! | [`formatter`] | `OUTPUTFORMATTER`/`STRING_FORMATTER`, `KICAD_FORMAT::Prettify` and friends |
//! | [`tree`] | `libs/sexpr`: the generic `SEXPR` tree, its parser and `AsString` |
//!
//! # Reading
//!
//! ```
//! use kicad_sexpr::keywords::pcb_keys_t::T::{self, *};
//! use kicad_sexpr::{DsnLexer, IoError};
//!
//! fn parse_version(text: &str) -> Result<i32, IoError> {
//!     let mut lexer = DsnLexer::<T>::new(text, "board.kicad_pcb");
//!
//!     lexer.need_left()?;
//!
//!     if lexer.next_tok()? != T_kicad_pcb {
//!         return Err(lexer.expecting(T_kicad_pcb));
//!     }
//!
//!     lexer.need_left()?;
//!
//!     if lexer.next_tok()? != T_version {
//!         return Err(lexer.expecting(T_version));
//!     }
//!
//!     let version = lexer.parse_int_expected("version")?;
//!     lexer.need_right()?;
//!     Ok(version)
//! }
//!
//! assert_eq!(parse_version("(kicad_pcb (version 20241229) (generator \"pcbnew\"))"), Ok(20241229));
//!
//! let err = parse_version("(kicad_pcb version)").unwrap_err();
//! assert_eq!(err.what(), "Expecting ''('' in 'board.kicad_pcb', line 1, offset 12.");
//! ```
//!
//! # Writing
//!
//! ```
//! use kicad_sexpr::{FormatMode, OutputFormatter, StringFormatter, format_bool, prettify};
//!
//! let mut out = StringFormatter::new();
//! let name = out.quotew("net \"1\"");
//!
//! write!(out, "(kicad_pcb (version {}) (net {} {})", 20241229, 1, name)?;
//! format_bool(&mut out, "locked", true)?;
//! write!(out, ")")?;
//!
//! assert_eq!(
//!     prettify(out.string(), FormatMode::Normal),
//!     "(kicad_pcb\n\t(version 20241229)\n\t(net 1 \"net \\\"1\\\"\")\n\t(locked yes)\n)\n"
//! );
//! # Ok::<(), kicad_sexpr::IoError>(())
//! ```

pub mod formatter;
pub mod ki_exception;
pub mod lexer;
pub mod tree;

pub use formatter::cformat::{format_e, format_f, format_g};
pub use formatter::kicad_io_utils::{
    FormatMode, base64_encode, format_bool, format_custom_properties, format_opt_bool,
    format_stream_data, format_uuid, prettify, prettify_bytes, prettify_in_place,
};
pub use formatter::richio::{
    OutputFormatter, PrettifiedOutputFormatter, StringFormatter, get_quote_char,
};
pub use ki_exception::{FutureFormatError, IoError, IoErrorBase, KiParamError, ParseError};
pub use lexer::dsnlexer::{
    DSN_BAR, DSN_COMMENT, DSN_DASH, DSN_EOF, DSN_LEFT, DSN_NONE, DSN_NUMBER, DSN_QUOTE_DEF,
    DSN_RIGHT, DSN_STRING, DSN_STRING_QUOTE, DSN_SYMBOL, DsnLexer, is_number_text, syntax,
};
pub use lexer::keywords;
pub use lexer::keywords::{DsnSyntaxT, KeywordSet};
pub use lexer::line_reader::{
    LINE_READER_LINE_DEFAULT_MAX, LINE_READER_LINE_INITIAL_SIZE, LineReader, StringLineReader,
};
pub use tree::isexprable::ISexprable;
pub use tree::sexpr::{
    InString, OutString, Sexpr, SexprChildrenArg, SexprDouble, SexprInteger, SexprList,
    SexprScanArg, SexprString, SexprSymbol, SexprType, as_string, as_string_in, as_symbol,
    as_symbol_in,
};
pub use tree::sexpr_exception::{InvalidTypeException, ParseException, SexprException};
pub use tree::sexpr_parser::Parser;
