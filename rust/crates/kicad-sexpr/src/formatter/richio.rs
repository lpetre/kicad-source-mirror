// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007-2011 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of the output formatter half of `include/richio.h` and `common/richio.cpp`:
//! `OUTPUTFORMATTER`, `STRING_FORMATTER` and (in memory) `PRETTIFIED_FILE_OUTPUTFORMATTER`.
//!
//! `FILE_OUTPUTFORMATTER` and the file handling of `PRETTIFIED_FILE_OUTPUTFORMATTER` (sibling
//! temp file, fsync, atomic rename) need a file system and are not ported: writers format into
//! memory and the caller stores the bytes.  See `PORT_STATUS.md`.
//!
//! # `Print` and format strings
//!
//! `OUTPUTFORMATTER::Print` is a `printf` style function.  The port takes
//! [`std::fmt::Arguments`], so a C++ call
//!
//! ```text
//! aOut->Print( "(net %d %s)", netCode, aOut->Quotew( name ).c_str() );
//! ```
//!
//! becomes
//!
//! ```
//! # use kicad_sexpr::{OutputFormatter, StringFormatter};
//! # let mut out = StringFormatter::new();
//! # let (net_code, name) = (1, "GND");
//! let quoted = out.quotew(name);
//! out.print(format_args!("(net {} {})", net_code, quoted))?;
//! // or, equivalently, with the standard macro:
//! write!(out, "(net {} {})", net_code, quoted)?;
//! # Ok::<(), kicad_sexpr::IoError>(())
//! ```
//!
//! `%d`/`%s` map to `{}`.  For `%g`, `%f` and `%e` use [`format_g`], [`format_f`] and
//! [`format_e`], which reproduce the C conversions exactly (Rust's own float formatting does
//! not).
//!
//! [`format_g`]: crate::formatter::cformat::format_g
//! [`format_f`]: crate::formatter::cformat::format_f
//! [`format_e`]: crate::formatter::cformat::format_e

use std::fmt;

use crate::formatter::kicad_io_utils::{FormatMode, prettify_in_place};
use crate::ki_exception::IoError;

/// Port of `OUTPUTFMTBUFZ`: default buffer size for any `OUTPUT_FORMATTER`.
pub const OUTPUTFMTBUFZ: usize = 500;

/// Port of `NESTWIDTH` in `OUTPUTFORMATTER::Indent`: how many spaces per nestLevel.
pub const NESTWIDTH: usize = 2;

/// Port of the static `OUTPUTFORMATTER::GetQuoteChar( const char* wrapee,
/// const char* quote_char )`: perform quote character need determination according to the
/// Specctra DSN specification.
///
/// `wrapee` is a string that might need wrapping on each end; `quote_char` is a single
/// character string which provides the current quote character, should it be needed by the
/// wrapee.
///
/// Returns `quote_char`, or `""` if the wrapee does not need to be wrapped.
pub fn get_quote_char<'a>(wrapee: &str, quote_char: &'a str) -> &'a str {
    // The C++ function works on a C string: it ends at the first NUL.
    let wrapee = match wrapee.find('\0') {
        Some(nul) => &wrapee.as_bytes()[..nul],
        None => wrapee.as_bytes(),
    };

    // Include '#' so a symbol is not confused with a comment.  We intend
    // to wrap any symbol starting with a '#'.
    // Our LEXER class handles comments, and comments appear to be an extension
    // to the SPECCTRA DSN specification.
    if wrapee.first() == Some(&b'#') {
        return quote_char;
    }

    if wrapee.is_empty() {
        return quote_char;
    }

    let mut is_first = true;

    for &c in wrapee {
        // "\t ()"
        // "%"   per Alfons of freerouting.net, he does not like this unquoted as of 1-Feb-2008
        // "{}"  guessing that these are problems too
        const QUOTE_THESE: &[u8] = b"\t ()%{}";

        // if the string to be wrapped (wrapee) has a delimiter in it,
        // return the quote_char so caller wraps the wrapee.
        if QUOTE_THESE.contains(&c) {
            return quote_char;
        }

        if !is_first && c == b'-' {
            return quote_char;
        }

        is_first = false;
    }

    "" // caller does not need to wrap, can use an unwrapped string.
}

/// Port of `OUTPUTFORMATTER`: an interface used to output 8 bit text in a convenient way.
///
/// The primary interface is "printf() - like" but with support for indentation control.  The
/// destination of the 8 bit wide text is up to the implementer.
///
/// The implementer only has to implement [`write`](Self::write) and
/// [`quote_char`](Self::quote_char), but can also optionally re-implement
/// [`get_quote_char`](Self::get_quote_char) and [`quotes`](Self::quotes).
pub trait OutputFormatter {
    /// Port of the pure virtual `OUTPUTFORMATTER::write`: should be coded in the interface
    /// implementation (derived) classes.  Writes `out_buf` to the destination.
    ///
    /// # Errors
    ///
    /// An [`IoError`] if there is a problem outputting, such as a full disk.
    fn write(&mut self, out_buf: &str) -> Result<(), IoError>;

    /// The quote character given to the C++ constructor (`quoteChar`), as a one character
    /// string.  `"\""` unless the formatter was made with another one.
    fn quote_char(&self) -> &str;

    /// Port of `OUTPUTFORMATTER::Print( const char* fmt, ... )`: format and write text to the
    /// output stream.
    ///
    /// Returns the number of bytes output.
    ///
    /// # Errors
    ///
    /// An [`IoError`] if there is a problem outputting, such as a full disk.
    fn print(&mut self, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
        // OUTPUTFORMATTER::vprint: format into the buffer first, then write it in one piece.
        let formatted;

        let text = match args.as_str() {
            Some(text) => text,
            None => {
                formatted = format_args_to_string(args)?;
                &formatted
            }
        };

        let ret = text.len() as i32;

        if ret > 0 {
            self.write(text)?;
        }

        Ok(ret)
    }

    /// Port of `OUTPUTFORMATTER::Print( int nestLevel, const char* fmt, ... )`: format and
    /// write text to the output stream, preceded by the indentation for `nest_level`.
    ///
    /// Returns the number of bytes output.
    ///
    /// # Errors
    ///
    /// An [`IoError`] if there is a problem outputting, such as a full disk.
    fn print_nest(&mut self, nest_level: i32, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
        let mut total = self.indent(nest_level)?;

        // no error checking needed, an exception indicates an error.
        total += self.print(args)?;

        Ok(total)
    }

    /// Port of `OUTPUTFORMATTER::Indent`: write the leading whitespace for a nesting level,
    /// with no other output ([`NESTWIDTH`] spaces per level).
    ///
    /// Returns the number of bytes output.
    ///
    /// # Errors
    ///
    /// An [`IoError`] if there is a problem outputting, such as a full disk.
    fn indent(&mut self, nest_level: i32) -> Result<i32, IoError> {
        const SPACES: &str = "  ";

        let mut total = 0;

        for _ in 0..nest_level {
            // no error checking needed, an exception indicates an error.
            // sprint( "%*c", NESTWIDTH, ' ' )
            self.write(SPACES)?;
            total += NESTWIDTH as i32;
        }

        Ok(total)
    }

    /// Port of the virtual `OUTPUTFORMATTER::GetQuoteChar( const char* wrapee )`: perform quote
    /// character need determination.
    ///
    /// It returns the quote character as a single character string for a given input wrapee
    /// string.  If the wrappee does not need to be quoted, the return value is "" (the null
    /// string), such as when there are no delimiters in the input wrapee string.  If you want
    /// the quote_char to be assuredly not "", then pass in "(" as the wrappee.
    ///
    /// Implementations are free to override the default behavior, which is to call the free
    /// function [`get_quote_char`].
    fn get_quote_char(&self, wrapee: &str) -> &str {
        get_quote_char(wrapee, self.quote_char())
    }

    /// Port of the virtual `OUTPUTFORMATTER::Quotes`: wrap `wrapee` in double quotes and escape
    /// its content (`\n`, `\r`, `\\` and `\"`) such that the `DSNLEXER` will correctly parse
    /// the string from a file later.
    ///
    /// The result is always quoted, and always with `"`, whatever
    /// [`quote_char`](Self::quote_char) is.
    fn quotes(&self, wrapee: &str) -> String {
        let mut ret = String::with_capacity(wrapee.len() * 2 + 2);

        ret.push('"');

        for c in wrapee.chars() {
            match c {
                '\n' => {
                    ret.push('\\');
                    ret.push('n');
                }
                '\r' => {
                    ret.push('\\');
                    ret.push('r');
                }
                '\\' => {
                    ret.push('\\');
                    ret.push('\\');
                }
                '"' => {
                    ret.push('\\');
                    ret.push('"');
                }
                _ => ret.push(c),
            }
        }

        ret.push('"');

        ret
    }

    /// Port of `OUTPUTFORMATTER::Quotew`: [`quotes`](Self::quotes) for a `wxString`.
    ///
    /// wxStrings are always encoded as UTF-8 as we convert to a byte sequence; Rust strings
    /// already are UTF-8, so this is the same as `quotes()`.  The non-virtual function calls
    /// the virtual workhorse function, and if a different quoting or escaping strategy is
    /// desired from the standard, a derived class can overload `quotes()`, but should never be
    /// a reason to overload this `quotew()` here.
    fn quotew(&self, wrapee: &str) -> String {
        self.quotes(wrapee)
    }

    /// Port of the virtual `OUTPUTFORMATTER::Finish`: performs any cleanup needed at the end of
    /// a write.  Returns true if all is well.
    fn finish(&mut self) -> Result<bool, IoError> {
        Ok(true)
    }

    /// The same as [`print`](Self::print); having a method of this name makes the standard
    /// `write!( out, "...", ... )` macro work on output formatters.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
        self.print(args)
    }
}

/// Format `args` into a new string; a failing `Display` implementation becomes an [`IoError`].
fn format_args_to_string(args: fmt::Arguments<'_>) -> Result<String, IoError> {
    let mut formatted = String::new();

    fmt::write(&mut formatted, args)
        .map_err(|_| IoError::new("a formatting trait implementation returned an error"))?;

    Ok(formatted)
}

/// Format `args` at the end of `buffer`, returning the number of bytes added.  On a failing
/// `Display` implementation nothing is added.
fn append_formatted(buffer: &mut String, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
    let before = buffer.len();

    match args.as_str() {
        Some(text) => buffer.push_str(text),
        None => {
            if fmt::write(buffer, args).is_err() {
                buffer.truncate(before);
                return Err(IoError::new("a formatting trait implementation returned an error"));
            }
        }
    }

    Ok((buffer.len() - before) as i32)
}

/// Port of `STRING_FORMATTER`: implement an [`OutputFormatter`] to a memory buffer.
///
/// After printing the string is available through [`string`](Self::string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringFormatter {
    quote_char: String,
    mystring: String,
}

impl Default for StringFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl StringFormatter {
    /// Port of `STRING_FORMATTER::STRING_FORMATTER()` with its default arguments.
    pub fn new() -> Self {
        Self::with_quote_char(OUTPUTFMTBUFZ, '"')
    }

    /// Port of `STRING_FORMATTER::STRING_FORMATTER( int aReserve, char aQuoteChar )`.
    ///
    /// `reserve` is the size of the C++ formatting buffer; here it is the initial capacity of
    /// the output string.
    pub fn with_quote_char(reserve: usize, quote_char: char) -> Self {
        Self { quote_char: quote_char.to_string(), mystring: String::with_capacity(reserve) }
    }

    /// Port of `STRING_FORMATTER::Clear`: clear the buffer and empty the internal string.
    pub fn clear(&mut self) {
        self.mystring.clear();
    }

    /// Port of `STRING_FORMATTER::StripUseless`: removes whitespace, `'('`, `')'` and `'"'`
    /// from the string.
    pub fn strip_useless(&mut self) {
        // isspace() in the "C" locale
        self.mystring.retain(|c| {
            !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r') && c != ')' && c != '(' && c != '"'
        });
    }

    /// Port of `STRING_FORMATTER::GetString`.
    pub fn string(&self) -> &str {
        &self.mystring
    }

    /// Port of `STRING_FORMATTER::MutableString`.
    pub fn mutable_string(&mut self) -> &mut String {
        &mut self.mystring
    }

    /// Take the formatted text out of the formatter.
    pub fn into_string(self) -> String {
        self.mystring
    }
}

impl OutputFormatter for StringFormatter {
    /// Port of `STRING_FORMATTER::write`.
    fn write(&mut self, out_buf: &str) -> Result<(), IoError> {
        self.mystring.push_str(out_buf);
        Ok(())
    }

    fn quote_char(&self) -> &str {
        &self.quote_char
    }

    /// `OUTPUTFORMATTER::Print`, formatting straight into the string (same result as the
    /// default implementation, without the intermediate buffer).
    fn print(&mut self, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
        append_formatted(&mut self.mystring, args)
    }
}

/// In-memory port of `PRETTIFIED_FILE_OUTPUTFORMATTER`: an [`OutputFormatter`] that collects
/// everything written to it and runs [`prettify`](crate::formatter::kicad_io_utils::prettify)
/// over it in [`finish`](OutputFormatter::finish).  This is the formatter every KiCad
/// s-expression file is saved through.
///
/// The C++ class then writes the result to a sibling temp file and renames it over the target;
/// here the caller takes the text with [`string`](Self::string) or
/// [`into_string`](Self::into_string) after `finish()` and stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrettifiedOutputFormatter {
    quote_char: String,
    buf: String,
    mode: FormatMode,
    finished: bool,
}

impl PrettifiedOutputFormatter {
    /// Port of `PRETTIFIED_FILE_OUTPUTFORMATTER::PRETTIFIED_FILE_OUTPUTFORMATTER`.
    ///
    /// `compact_save` is the value of `ADVANCED_CFG::GetCfg().m_CompactSave` (advanced config
    /// key `CompactSave`, default false), which the C++ constructor reads from the global
    /// configuration: if it is set and `format_mode` is [`FormatMode::Normal`], the mode
    /// becomes [`FormatMode::CompactTextProperties`].
    pub fn new(format_mode: FormatMode, compact_save: bool) -> Self {
        Self::with_quote_char(format_mode, compact_save, '"')
    }

    /// Like [`new`](Self::new) with the C++ constructor argument `aQuoteChar`.
    pub fn with_quote_char(format_mode: FormatMode, compact_save: bool, quote_char: char) -> Self {
        let mut mode = format_mode;

        if compact_save && mode == FormatMode::Normal {
            mode = FormatMode::CompactTextProperties;
        }

        Self { quote_char: quote_char.to_string(), buf: String::new(), mode, finished: false }
    }

    /// The effective format mode (C++ `m_mode`).
    pub fn mode(&self) -> FormatMode {
        self.mode
    }

    /// The collected text: raw before [`finish`](OutputFormatter::finish), prettified after.
    pub fn string(&self) -> &str {
        &self.buf
    }

    /// Take the text out of the formatter; call [`finish`](OutputFormatter::finish) first.
    pub fn into_string(self) -> String {
        self.buf
    }
}

impl OutputFormatter for PrettifiedOutputFormatter {
    /// Port of `PRETTIFIED_FILE_OUTPUTFORMATTER::write`.
    fn write(&mut self, out_buf: &str) -> Result<(), IoError> {
        self.buf.push_str(out_buf);
        Ok(())
    }

    fn quote_char(&self) -> &str {
        &self.quote_char
    }

    fn print(&mut self, args: fmt::Arguments<'_>) -> Result<i32, IoError> {
        append_formatted(&mut self.buf, args)
    }

    /// Port of `PRETTIFIED_FILE_OUTPUTFORMATTER::Finish`: runs prettification over the buffered
    /// bytes.
    ///
    /// `finish()` must be called at most once.  Calling it again is a programming error; like
    /// the C++ `wxCHECK_MSG` in `SIBLING_TEMP_FILE::Commit` it then does nothing and returns
    /// false.
    fn finish(&mut self) -> Result<bool, IoError> {
        debug_assert!(!self.finished, "finish() called on an already-used formatter");

        if self.finished {
            return Ok(false);
        }

        prettify_in_place(&mut self.buf, self.mode);
        self.finished = true;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_char_determination() {
        // No quoting needed.
        for wrapee in ["abc", "F.Cu", "-12", "-", "a_b", "1.5", "é", "a\"b", "a\\b", "a#b", "a|b"]
        {
            assert_eq!(get_quote_char(wrapee, "\""), "", "{wrapee:?}");
        }

        // Quoting needed.
        for wrapee in [
            "", "#abc", "#", "a b", " a", "a\tb", "a(b", "a)b", "a%b", "a{b", "a}b", "a-b", "a-",
            "--",
        ] {
            assert_eq!(get_quote_char(wrapee, "\""), "\"", "{wrapee:?}");
        }

        // The current quote character is what gets returned.
        assert_eq!(get_quote_char("a b", "'"), "'");

        // C string semantics: nothing behind a NUL is looked at.
        assert_eq!(get_quote_char("a\0 b", "\""), "");
        assert_eq!(get_quote_char("\0abc", "\""), "\"");
    }

    #[test]
    fn formatter_get_quote_char_uses_its_quote_char() {
        let fmt = StringFormatter::new();
        assert_eq!(fmt.get_quote_char("a b"), "\"");
        assert_eq!(fmt.get_quote_char("ab"), "");

        let fmt = StringFormatter::with_quote_char(OUTPUTFMTBUFZ, '\'');
        assert_eq!(fmt.quote_char(), "'");
        assert_eq!(fmt.get_quote_char("a b"), "'");

        // Quotes() always uses the double quote.
        assert_eq!(fmt.quotes("a b"), "\"a b\"");
    }

    #[test]
    fn quotes_always_wraps_and_escapes() {
        let fmt = StringFormatter::new();

        assert_eq!(fmt.quotes(""), "\"\"");
        assert_eq!(fmt.quotes("abc"), "\"abc\"");
        assert_eq!(fmt.quotes("a b"), "\"a b\"");
        assert_eq!(fmt.quotes("line1\nline2\r"), "\"line1\\nline2\\r\"");
        assert_eq!(fmt.quotes("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(fmt.quotes("say \"hi\""), "\"say \\\"hi\\\"\"");
        // Everything else is passed through, including tabs and non-ASCII text.
        assert_eq!(fmt.quotes("a\tb\x07 é µ"), "\"a\tb\x07 é µ\"");
        assert_eq!(fmt.quotew("Ω \"x\""), "\"Ω \\\"x\\\"\"");
    }

    #[test]
    fn print_returns_byte_count() {
        let mut fmt = StringFormatter::new();

        assert_eq!(fmt.print(format_args!("")).unwrap(), 0);
        assert_eq!(fmt.print(format_args!("(a)")).unwrap(), 3);
        assert_eq!(fmt.print(format_args!("(net {} {})", 42, "é")).unwrap(), 11);
        assert_eq!(fmt.string(), "(a)(net 42 é)");

        assert_eq!(write!(fmt, " {}", 1).unwrap(), 2);
        assert_eq!(fmt.string(), "(a)(net 42 é) 1");

        fmt.clear();
        assert_eq!(fmt.string(), "");
    }

    /// An implementation that relies on the provided `print()`.
    struct Collect(Vec<String>);

    impl OutputFormatter for Collect {
        fn write(&mut self, out_buf: &str) -> Result<(), IoError> {
            self.0.push(out_buf.to_string());
            Ok(())
        }

        fn quote_char(&self) -> &str {
            "\""
        }
    }

    #[test]
    fn default_print_writes_once_and_skips_empty_output() {
        let mut out = Collect(Vec::new());

        assert_eq!(out.print(format_args!("")).unwrap(), 0);
        assert_eq!(out.print(format_args!("{}", "")).unwrap(), 0);
        assert!(out.0.is_empty());

        assert_eq!(out.print(format_args!("(a {} {})", 1, "b")).unwrap(), 7);
        assert_eq!(out.print_nest(2, format_args!("x")).unwrap(), 5);
        assert_eq!(out.0, ["(a 1 b)", "  ", "  ", "x"]);
        assert!(out.finish().unwrap());
    }

    #[test]
    fn strip_useless() {
        let mut fmt = StringFormatter::new();
        fmt.print(format_args!("(a \"b c\"\n\t(d)\r\x0b\x0c)")).unwrap();
        fmt.strip_useless();
        assert_eq!(fmt.string(), "abcd");
    }

    #[test]
    fn mutable_string_and_into_string() {
        let mut fmt = StringFormatter::new();
        fmt.print(format_args!("abc")).unwrap();
        fmt.mutable_string().push('d');
        assert_eq!(fmt.into_string(), "abcd");
    }

    #[test]
    fn prettified_formatter() {
        let mut out = PrettifiedOutputFormatter::new(FormatMode::Normal, false);
        assert_eq!(out.mode(), FormatMode::Normal);

        out.print(format_args!("(kicad_pcb (version {}) (generator \"pcbnew\"))", 20241229)).unwrap();
        assert_eq!(out.string(), "(kicad_pcb (version 20241229) (generator \"pcbnew\"))");

        assert!(out.finish().unwrap());
        assert_eq!(out.string(), "(kicad_pcb\n\t(version 20241229)\n\t(generator \"pcbnew\")\n)\n");
        assert_eq!(
            out.into_string(),
            "(kicad_pcb\n\t(version 20241229)\n\t(generator \"pcbnew\")\n)\n"
        );
    }

    #[test]
    fn prettified_formatter_compact_save() {
        // m_CompactSave only upgrades the NORMAL mode.
        assert_eq!(
            PrettifiedOutputFormatter::new(FormatMode::Normal, true).mode(),
            FormatMode::CompactTextProperties
        );
        assert_eq!(
            PrettifiedOutputFormatter::new(FormatMode::LibraryTable, true).mode(),
            FormatMode::LibraryTable
        );
        assert_eq!(
            PrettifiedOutputFormatter::new(FormatMode::CompactTextProperties, false).mode(),
            FormatMode::CompactTextProperties
        );
    }
}
