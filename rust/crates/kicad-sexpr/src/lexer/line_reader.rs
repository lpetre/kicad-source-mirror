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

//! Port of the line reader half of `include/richio.h` and `common/richio.cpp`:
//! `LINE_READER` and `STRING_LINE_READER`.
//!
//! `FILE_LINE_READER`, `INPUTSTREAM_LINE_READER` and `SafeReadFile` need a file system or
//! wxWidgets streams and are not ported (library crates do their I/O on in-memory text); see
//! `PORT_STATUS.md`.

use crate::ki_exception::IoError;

/// Port of `LINE_READER_LINE_DEFAULT_MAX`.
pub const LINE_READER_LINE_DEFAULT_MAX: u32 = 16_000_000;

/// Port of `LINE_READER_LINE_INITIAL_SIZE`.
///
/// Only kept for reference: the C++ readers copy every line into a growing buffer that starts
/// at this size; the port hands out sub-slices of the source text instead.
pub const LINE_READER_LINE_INITIAL_SIZE: u32 = 5000;

/// Port of the abstract class `LINE_READER`, from which implementation specific readers may be
/// derived to read single lines of text and manage a line number counter.
pub trait LineReader {
    /// Port of `LINE_READER::ReadLine`: read a line of text and increment the line number
    /// counter.  The returned line includes its terminating `'\n'`, if it has one.
    ///
    /// Returns `Ok(None)` at the end of the input (C++ returns `NULL`); the line number is
    /// incremented even then, because this leads to better error reporting when we hit an end
    /// of file.
    ///
    /// # Errors
    ///
    /// An [`IoError`] when a line is too long.
    fn read_line(&mut self) -> Result<Option<&str>, IoError>;

    /// Port of `LINE_READER::GetSource`: the name of the source of the lines in an abstract
    /// sense.  This may be a file or it may be the clipboard or any other source of lines of
    /// text.  The returned string is useful for reporting error messages.
    fn source(&self) -> &str;

    /// Port of `LINE_READER::Line`: the last line that was read in (empty before the first
    /// `read_line()` and at the end of the input).
    fn line(&self) -> &str;

    /// Port of `LINE_READER::LineNumber`: the line number of the last line read from this
    /// reader.  Lines start from 1.
    fn line_number(&self) -> u32;

    /// Port of `LINE_READER::Length`: the number of bytes in the last line read from this
    /// reader.
    fn length(&self) -> u32;
}

/// Port of `STRING_LINE_READER`: a [`LineReader`] that reads from a multiline 8 bit wide string.
#[derive(Debug, Clone)]
pub struct StringLineReader {
    /// C++ `m_lines`.
    lines: String,
    /// C++ `m_ndx`: offset of the first byte that has not been read yet.
    ndx: usize,
    /// Offset in `lines` of the last line read (C++ copies the line into `m_line`).
    line_start: usize,
    /// C++ `m_length`: no. bytes in the line.
    length: u32,
    /// C++ `m_lineNum`.
    line_num: u32,
    /// C++ `m_maxLineLength`: maximum allowed line length.
    max_line_length: u32,
    /// C++ `m_source`: origin of text lines, e.g. filename or "clipboard".
    source: String,
}

impl StringLineReader {
    /// Port of `STRING_LINE_READER::STRING_LINE_READER( const std::string& aString,
    /// const wxString& aSource )`.
    ///
    /// `string` is a source string consisting of one or more lines of text, where multiple
    /// lines are separated with a `'\n'` character.  The last line does not necessarily need a
    /// trailing `'\n'`.  `source` describes the source of `string` for error reporting purposes
    /// and can be anything meaningful, such as "clipboard".
    pub fn new(string: impl Into<String>, source: impl Into<String>) -> Self {
        Self::with_max_line_length(string, source, LINE_READER_LINE_DEFAULT_MAX)
    }

    /// Like [`new`](Self::new) with an explicit maximum line length, the constructor argument
    /// of the C++ base class `LINE_READER( unsigned aMaxLineLength )`.  `STRING_LINE_READER`
    /// itself always passes [`LINE_READER_LINE_DEFAULT_MAX`].
    pub fn with_max_line_length(
        string: impl Into<String>,
        source: impl Into<String>,
        max_line_length: u32,
    ) -> Self {
        // Clipboard text should be nice and _use multiple lines_ so that
        // we can report _line number_ oriented error messages when parsing.
        Self {
            lines: string.into(),
            ndx: 0,
            line_start: 0,
            length: 0,
            line_num: 0,
            max_line_length,
            source: source.into(),
        }
    }

    /// Port of `STRING_LINE_READER::STRING_LINE_READER( const STRING_LINE_READER& aStartingPoint )`:
    /// allows for a continuation of the reading of a stream started by another
    /// `StringLineReader`.  The stream offset, line number and source name are taken from
    /// `starting_point`; the current line starts out empty.
    pub fn from_starting_point(starting_point: &StringLineReader) -> Self {
        // since we are keeping the same "source" name, for error reporting purposes
        // we need to have the same notion of line number and offset.
        Self {
            lines: starting_point.lines.clone(),
            ndx: starting_point.ndx,
            line_start: starting_point.ndx,
            length: 0,
            line_num: starting_point.line_num,
            max_line_length: LINE_READER_LINE_DEFAULT_MAX,
            source: starting_point.source.clone(),
        }
    }
}

impl LineReader for StringLineReader {
    /// Port of `STRING_LINE_READER::ReadLine`.
    fn read_line(&mut self) -> Result<Option<&str>, IoError> {
        let rest = &self.lines.as_bytes()[self.ndx..];

        // C++ computes the length as an `unsigned`; a single line beyond 4 GiB would wrap there.
        // Saturate instead, which reports such a line as too long.
        let new_length = match rest.iter().position(|&b| b == b'\n') {
            None => rest.len(),
            Some(nl_offset) => nl_offset + 1, // include the newline, so +1
        };
        let new_length = u32::try_from(new_length).unwrap_or(u32::MAX);

        if new_length != 0 {
            if new_length >= self.max_line_length {
                return Err(IoError::new("Line length exceeded"));
            }

            self.line_start = self.ndx;
            self.ndx += new_length as usize;
        } else {
            // m_line[0] = 0
            self.line_start = self.ndx;
        }

        self.length = new_length;
        self.line_num += 1; // this gets incremented even if no bytes were read

        Ok(if self.length != 0 { Some(self.line()) } else { None })
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn line(&self) -> &str {
        // Lines end right after a '\n' or at the end of the text, both of which are character
        // boundaries, so this cannot fail.
        self.lines.get(self.line_start..self.line_start + self.length as usize).unwrap_or("")
    }

    fn line_number(&self) -> u32 {
        self.line_num
    }

    fn length(&self) -> u32 {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_and_counts() {
        let mut reader = StringLineReader::new("one\ntwo\r\n\nlast", "test");

        assert_eq!(reader.source(), "test");
        assert_eq!(reader.line(), "");
        assert_eq!(reader.line_number(), 0);
        assert_eq!(reader.length(), 0);

        assert_eq!(reader.read_line().unwrap(), Some("one\n"));
        assert_eq!(reader.line_number(), 1);
        assert_eq!(reader.length(), 4);

        assert_eq!(reader.read_line().unwrap(), Some("two\r\n"));
        assert_eq!(reader.read_line().unwrap(), Some("\n"));
        assert_eq!(reader.length(), 1);

        assert_eq!(reader.read_line().unwrap(), Some("last"));
        assert_eq!(reader.line_number(), 4);
        assert_eq!(reader.line(), "last");

        // End of input: no line, but the line number still advances.
        assert_eq!(reader.read_line().unwrap(), None);
        assert_eq!(reader.line_number(), 5);
        assert_eq!(reader.length(), 0);
        assert_eq!(reader.line(), "");

        assert_eq!(reader.read_line().unwrap(), None);
        assert_eq!(reader.line_number(), 6);
    }

    #[test]
    fn empty_input() {
        let mut reader = StringLineReader::new("", "empty");

        assert_eq!(reader.read_line().unwrap(), None);
        assert_eq!(reader.line_number(), 1);
    }

    #[test]
    fn trailing_newline_gives_no_extra_line() {
        let mut reader = StringLineReader::new("a\n", "s");

        assert_eq!(reader.read_line().unwrap(), Some("a\n"));
        assert_eq!(reader.read_line().unwrap(), None);
    }

    #[test]
    fn line_length_limit() {
        // new_length >= m_maxLineLength throws: a line of exactly the maximum is refused.
        let mut reader = StringLineReader::with_max_line_length("ab\nabcd\nabc", "s", 4);

        assert_eq!(reader.read_line().unwrap(), Some("ab\n"));

        let err = reader.read_line().unwrap_err();
        assert_eq!(err.problem(), "Line length exceeded");
        assert!(matches!(err, IoError::Io(_)));

        // Nothing changes on the error: same line, same line number, and the same error again.
        assert_eq!(reader.line(), "ab\n");
        assert_eq!(reader.line_number(), 1);
        assert_eq!(reader.read_line().unwrap_err().problem(), "Line length exceeded");
    }

    #[test]
    fn default_limit_is_16_million() {
        // 15 999 999 bytes including the newline pass, 16 000 000 do not.
        let mut ok = String::with_capacity(16_000_000);
        ok.extend(std::iter::repeat_n('x', 15_999_998));
        ok.push('\n');

        let mut reader = StringLineReader::new(ok.clone(), "s");
        assert_eq!(reader.read_line().unwrap().map(str::len), Some(15_999_999));

        ok.insert(0, 'x');
        let mut reader = StringLineReader::new(ok, "s");
        assert_eq!(reader.read_line().unwrap_err().problem(), "Line length exceeded");
    }

    #[test]
    fn continuation_reader() {
        let mut first = StringLineReader::new("a\nb\nc\n", "src");
        first.read_line().unwrap();

        let mut second = StringLineReader::from_starting_point(&first);
        assert_eq!(second.source(), "src");
        assert_eq!(second.line_number(), 1);
        assert_eq!(second.line(), "");
        assert_eq!(second.read_line().unwrap(), Some("b\n"));
        assert_eq!(second.line_number(), 2);

        // The original is unaffected.
        assert_eq!(first.read_line().unwrap(), Some("b\n"));
    }
}
