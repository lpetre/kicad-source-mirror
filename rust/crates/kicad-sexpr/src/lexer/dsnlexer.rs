// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007-2013 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of `include/dsnlexer.h` and `common/dsnlexer.cpp`.
//!
//! [`DsnLexer`] implements a lexical analyzer for the SPECCTRA DSN file format and for KiCad's
//! own s-expression formats.  It reads lexical tokens from the current [`LineReader`] through
//! [`DsnLexer::next_tok`].
//!
//! The lexer is generic over its token type `T` (see [`KeywordSet`]), which plays the role of
//! the C++ classes generated from `*.keywords` files: `DsnLexer<pcb_keys_t::T>` is `PCB_LEXER`.
//! Internally tokens are the same `int` values as in C++.
//!
//! Functions that `throw` in C++ return `Result<_, IoError>`; the `Expecting()`/`Unexpected()`
//! family, which always throws, returns the error value for the caller to return:
//! `return Err( lexer.expecting( T_LEFT ) )`.

use std::marker::PhantomData;

use crate::ki_exception::IoError;
use crate::lexer::c_numeric::{from_chars_double, strtol};
use crate::lexer::keywords::{DsnSyntaxT, KeywordSet};
use crate::lexer::line_reader::{LineReader, StringLineReader};

/// `DSN_SYNTAX_T::DSN_NONE`
pub const DSN_NONE: i32 = -12;
/// `DSN_SYNTAX_T::DSN_BAR`, also called pipe `'|'`
pub const DSN_BAR: i32 = -11;
/// `DSN_SYNTAX_T::DSN_COMMENT`
pub const DSN_COMMENT: i32 = -10;
/// `DSN_SYNTAX_T::DSN_STRING_QUOTE`
pub const DSN_STRING_QUOTE: i32 = -9;
/// `DSN_SYNTAX_T::DSN_QUOTE_DEF`
pub const DSN_QUOTE_DEF: i32 = -8;
/// `DSN_SYNTAX_T::DSN_DASH`
pub const DSN_DASH: i32 = -7;
/// `DSN_SYNTAX_T::DSN_SYMBOL`
pub const DSN_SYMBOL: i32 = -6;
/// `DSN_SYNTAX_T::DSN_NUMBER`
pub const DSN_NUMBER: i32 = -5;
/// `DSN_SYNTAX_T::DSN_RIGHT`: right bracket, `')'`
pub const DSN_RIGHT: i32 = -4;
/// `DSN_SYNTAX_T::DSN_LEFT`: left bracket, `'('`
pub const DSN_LEFT: i32 = -3;
/// `DSN_SYNTAX_T::DSN_STRING`: a quoted string, stripped of the quotes
pub const DSN_STRING: i32 = -2;
/// `DSN_SYNTAX_T::DSN_EOF`: special case for end of file
pub const DSN_EOF: i32 = -1;

/// The C++ `FMT_CLIPBOARD`, `_( "clipboard" )`: the source name used when none is given.
const FMT_CLIPBOARD: &str = "clipboard";

/// Port of `DSNLEXER::Syntax`: the text of one of the negative syntax tokens.
pub fn syntax(tok: i32) -> &'static str {
    match tok {
        DSN_NONE => "NONE",
        DSN_STRING_QUOTE => "string_quote", // a special DSN syntax token, see specctra spec.
        DSN_QUOTE_DEF => "quoted text delimiter",
        DSN_DASH => "-",
        DSN_SYMBOL => "symbol",
        DSN_NUMBER => "number",
        DSN_RIGHT => ")",
        DSN_LEFT => "(",
        DSN_STRING => "quoted string",
        DSN_EOF => "end of input",
        DSN_BAR => "|",
        _ => "???",
    }
}

/// Port of the file-static `isSpace` of `dsnlexer.cpp`: test for whitespace.
///
/// Our whitespace, by our definition, is a subset of ASCII, i.e. no bytes with MSB on can be
/// considered whitespace, since they are likely part of a multibyte UTF8 character.
fn is_space(cc: u8) -> bool {
    matches!(
        cc,
        b' ' | b'\n' | b'\r' | b'\t' | b'\0' // PCAD s-expression files have this.
    )
}

/// Port of the protected static `DSNLEXER::isNumber( const char* cp, const char* limit )`:
/// return true if `text` is a number: either an integer, fixed point, or float with exponent.
/// The whole of `text` has to be the number.
///
/// This decides between `DSN_NUMBER` and a symbol for unquoted tokens.
pub fn is_number_text(text: &[u8]) -> bool {
    // regex for a float: "^[-+]?[0-9]*\\.?[0-9]+([eE][-+]?[0-9]+)?" i.e. any number,
    // code traversal manually here:

    let limit = text.len();
    let mut cp = 0;
    let mut saw_number = false;

    if cp < limit && (text[cp] == b'-' || text[cp] == b'+') {
        cp += 1;
    }

    while cp < limit && text[cp].is_ascii_digit() {
        cp += 1;
        saw_number = true;
    }

    if cp < limit && text[cp] == b'.' {
        cp += 1;

        while cp < limit && text[cp].is_ascii_digit() {
            cp += 1;
            saw_number = true;
        }
    }

    if saw_number {
        if cp < limit && (text[cp] == b'E' || text[cp] == b'e') {
            cp += 1;

            saw_number = false; // exponent mandates at least one digit thereafter.

            if cp < limit && (text[cp] == b'-' || text[cp] == b'+') {
                cp += 1;
            }

            while cp < limit && text[cp].is_ascii_digit() {
                cp += 1;
                saw_number = true;
            }
        }
    }

    saw_number && cp == limit
}

/// Replace the current token text by `bytes`.
///
/// `text` always holds valid UTF-8; if `bytes` is not, `text` gets the lossy conversion and
/// the exact bytes are kept in `raw`.
fn assign_text(text: &mut String, raw: &mut Option<Vec<u8>>, bytes: &[u8]) {
    text.clear();

    match std::str::from_utf8(bytes) {
        Ok(valid) => {
            text.push_str(valid);
            *raw = None;
        }
        Err(_) => {
            text.push_str(&String::from_utf8_lossy(bytes));
            *raw = Some(bytes.to_vec());
        }
    }
}

/// Port of `DSNLEXER`: a lexical analyzer for the SPECCTRA DSN file format, also used for all
/// of KiCad's s-expression file formats.
///
/// `T` is the token type; see [`KeywordSet`] and the [`keywords!`](crate::keywords!) macro.
pub struct DsnLexer<T: KeywordSet = DsnSyntaxT> {
    /// C++ `readerStack`: all the LINE_READERs.  The last entry is the C++ `reader`.
    ///
    /// A `None` entry stands for a reader that is currently lent to another lexer by
    /// [`sync_line_reader_with`](Self::sync_line_reader_with).
    reader_stack: Vec<Option<Box<dyn LineReader>>>,

    // C++ keeps `start`, `next` and `limit` as pointers into the reader's line buffer.  Here
    // `start` is offset 0 of `reader.line()` and the other two are offsets into it.
    next: usize,
    limit: usize,

    /// if true, then:
    /// 1) stringDelimiter can be changed
    /// 2) Kicad quoting protocol is not in effect
    /// 3) space_in_quoted_tokens is functional
    ///
    /// else not.
    specctra_mode: bool,

    /// True if the lexer knows about the bar token.
    /// This was introduced by 77797103f7eb1483e03660d3b7848537ebe7cb1f
    knows_bar: bool,

    string_delimiter: u8,

    /// Blank spaces within quoted strings.
    space_in_quoted_tokens: bool,

    /// True if should return comments as tokens.
    comments_are_tokens: bool,

    /// `curTok` from previous `NextTok()` call.
    prev_tok: i32,

    /// Offset within current line of the current token.
    cur_offset: i32,

    /// The current token obtained on last `NextTok()`.
    cur_tok: i32,

    /// The text of the current token.
    cur_text: String,

    /// The exact bytes of the current token if they are not valid UTF-8 (possible only through
    /// `\x..` and octal escapes in a quoted string); `cur_text` is the lossy conversion then.
    cur_text_raw: Option<Vec<u8>>,

    /// The text of the separator preceding the current text.
    cur_separator: String,

    /// C++ `keywords` / `keywordCount`: keyword `keywords[n]` is token `n`.
    keywords: &'static [&'static str],

    /// Indices into `keywords` in sorted order, if `keywords` itself is not sorted (C++ looks
    /// keywords up in a hash table, so a table may be in any order there).
    keyword_order: Option<Vec<u32>>,

    _token: PhantomData<fn() -> T>,
}

impl<T: KeywordSet> DsnLexer<T> {
    /// Common part of the constructors, including `DSNLEXER::init`.
    fn with_table(keywords: &'static [&'static str]) -> Self {
        let keyword_order = if crate::lexer::keywords::is_sorted_keyword_table(keywords) {
            None
        } else {
            let mut order: Vec<u32> = (0..keywords.len() as u32).collect();
            order.sort_by_key(|&i| keywords[i as usize].as_bytes());
            Some(order)
        };

        Self {
            reader_stack: Vec::new(),
            next: 0,
            limit: 0,
            specctra_mode: false,
            knows_bar: true, // default since version 20240706
            string_delimiter: b'"',
            space_in_quoted_tokens: false,
            comments_are_tokens: false,
            prev_tok: DSN_NONE,
            cur_offset: 0,
            cur_tok: DSN_NONE,
            cur_text: String::new(),
            cur_text_raw: None,
            cur_separator: String::new(),
            keywords,
            keyword_order,
            _token: PhantomData,
        }
    }

    /// Port of `DSNLEXER::DSNLEXER( const KEYWORD* aKeywordTable, unsigned aKeywordCount,
    /// const KEYWORD_MAP* aKeywordMap, const std::string& aSExpression,
    /// const wxString& aSource = wxEmptyString )` (and, for token types without keywords, of
    /// `DSNLEXER( const std::string& aSExpression, const wxString& aSource )`): initialize a DSN
    /// lexer and prepare to read from `s_expression`.
    ///
    /// The keyword table is `T::KEYWORDS`.  `s_expression` is the text to feed through a
    /// [`StringLineReader`]; `source` is a description of it, used for error reporting.  An
    /// empty `source` becomes "clipboard", as in C++.
    pub fn new(s_expression: impl Into<String>, source: &str) -> Self {
        let mut this = Self::with_table(T::KEYWORDS);
        let source = if source.is_empty() {
            FMT_CLIPBOARD
        } else {
            source
        };

        this.push_reader(Box::new(StringLineReader::new(s_expression, source)));
        this
    }

    /// Port of `DSNLEXER::DSNLEXER( const KEYWORD* aKeywordTable, unsigned aKeywordCount,
    /// const KEYWORD_MAP* aKeywordMap, LINE_READER* aLineReader = nullptr )`: initialize a DSN
    /// lexer and prepare to read from `line_reader`, which may have been in use by other
    /// lexers before.
    ///
    /// C++ takes no ownership of the reader so that several lexers can share it; here the
    /// reader is owned by the lexer and handed on with [`pop_reader`](Self::pop_reader) or
    /// [`sync_line_reader_with`](Self::sync_line_reader_with).  Pass `None` (C++ `nullptr`) for
    /// a lexer that will get its reader later, in particular one that is about to share the
    /// reader of another lexer through `sync_line_reader_with`.
    pub fn from_line_reader(line_reader: Option<Box<dyn LineReader>>) -> Self {
        let mut this = Self::with_table(T::KEYWORDS);

        if let Some(line_reader) = line_reader {
            this.push_reader(line_reader);
        }

        this
    }

    /// C++ `reader`: the current line reader, if there is one and it is not lent out.
    fn reader(&self) -> Option<&dyn LineReader> {
        match self.reader_stack.last() {
            Some(Some(reader)) => Some(reader.as_ref()),
            _ => None,
        }
    }

    /// Port of `DSNLEXER::InitParserState`: reinit variables used during parsing, to ensure old
    /// states are not used in a new parsing.  Must be called before parsing a new file after
    /// parsing an old file to avoid starting with some variables in a non initial state.
    pub fn init_parser_state(&mut self) {
        self.cur_tok = DSN_NONE;
        self.prev_tok = DSN_NONE;
        self.comments_are_tokens = false;

        self.cur_offset = 0;
    }

    /// Port of `DSNLEXER::SyncLineReaderWith`: usable only for lexers which share the same
    /// line reader.  Synchronizes the position within the data read by the line reader.
    /// Allows two lexers to share the same current line, when switching from one lexer to
    /// another one (KiCad parsers hand over to a parser for another grammar and back in the
    /// middle of a line this way, e.g. for `(stroke ...)` or `(pcbplotparams ...)`).
    ///
    /// In C++ both lexers hold a pointer to the same `LINE_READER`.  Here a reader has exactly
    /// one owner, so "sharing" is a hand-over: if this lexer has no reader of its own (it was
    /// made with `from_line_reader( None )`, or it lent its reader out in an earlier call), the
    /// current reader of `lexer` moves to this lexer together with the position.  The usual
    /// C++ sequence therefore ports as is:
    ///
    /// ```text
    /// SUB_PARSER sub( reader );           let mut sub = DsnLexer::<SubT>::from_line_reader( None );
    /// sub.SyncLineReaderWith( *this );    sub.sync_line_reader_with( &mut self.lexer );
    /// ... parse with sub ...              ... parse with sub ...
    /// SyncLineReaderWith( sub );          self.lexer.sync_line_reader_with( &mut sub );
    /// ```
    ///
    /// Returns true if the sync can be made (at least the same line reader), false if both
    /// lexers have distinct readers.
    pub fn sync_line_reader_with<U: KeywordSet>(&mut self, lexer: &mut DsnLexer<U>) -> bool {
        // Synchronize the pointers handling the data read by the LINE_READER
        // only if aLexer shares the same LINE_READER, because only in this case
        // the char buffer is be common

        if self.reader().is_some() {
            // Two lexers that both own a reader cannot have the same one.
            return false;
        }

        if lexer.reader().is_some() {
            let reader = lexer.reader_stack.last_mut().and_then(Option::take);

            match self.reader_stack.last_mut() {
                // Our reader comes back from the loan.
                Some(slot) => *slot = reader,
                None => self.reader_stack.push(reader),
            }
        }

        // We can synchronize the pointers which handle the data currently read
        self.next = lexer.next;
        self.limit = lexer.limit;

        // Sync these parameters is not mandatory, but could help
        // for instance in debug
        self.cur_text.clone_from(&lexer.cur_text);
        self.cur_text_raw.clone_from(&lexer.cur_text_raw);
        self.cur_offset = lexer.cur_offset;

        true
    }

    /// Run `f` with a temporary lexer for another token type that shares this lexer's line
    /// reader and current position, the port of the C++ idiom
    ///
    /// ```text
    /// SUB_PARSER sub( reader );
    /// sub.SyncLineReaderWith( *this );
    /// ... use sub, may throw ...
    /// SyncLineReaderWith( sub );
    /// ```
    ///
    /// On `Ok` this lexer continues behind the last token the sub-lexer consumed.  On `Err` the
    /// position is not synchronized back, exactly as when the C++ code leaves by exception, but
    /// the line reader is returned to this lexer.
    pub fn with_sub_lexer<U: KeywordSet, R, E>(
        &mut self,
        f: impl FnOnce(&mut DsnLexer<U>) -> Result<R, E>,
    ) -> Result<R, E> {
        let mut sub = DsnLexer::<U>::from_line_reader(None);

        sub.sync_line_reader_with(self);

        let result = f(&mut sub);

        if result.is_ok() {
            self.sync_line_reader_with(&mut sub);
        } else {
            self.reclaim_line_reader_from(&mut sub);
        }

        result
    }

    /// Take the line reader back from `lexer`, to which it was handed over by
    /// [`sync_line_reader_with`](Self::sync_line_reader_with), *without* synchronizing the
    /// position.
    ///
    /// This is what remains of the C++ sharing when the other lexer goes out of scope before
    /// the closing `SyncLineReaderWith()` call, typically because an exception unwinds its
    /// scope: the first lexer still has its `LINE_READER` pointer, and its own (now stale)
    /// position.  Use it on the error path of code that calls `sync_line_reader_with` by hand;
    /// [`with_sub_lexer`](Self::with_sub_lexer) does it automatically.
    ///
    /// Returns true if a reader was taken back.
    pub fn reclaim_line_reader_from<U: KeywordSet>(&mut self, lexer: &mut DsnLexer<U>) -> bool {
        if self.reader().is_some() || lexer.reader().is_none() {
            return false;
        }

        let reader = lexer.reader_stack.last_mut().and_then(Option::take);

        match self.reader_stack.last_mut() {
            Some(slot) => *slot = reader,
            None => self.reader_stack.push(reader),
        }

        true
    }

    /// Port of `DSNLEXER::SetSpecctraMode`: change the behavior of this lexer into or out of
    /// "specctra mode".
    ///
    /// If specctra mode, then:
    ///  1. stringDelimiter can be changed.
    ///  2. KiCad quoting protocol is not in effect.
    ///  3. space_in_quoted_tokens is functional else none of the above are true.
    ///
    /// The default mode is non-specctra mode, meaning:
    ///  1. stringDelimiter cannot be changed.
    ///  2. KiCad quoting protocol is in effect.
    ///  3. space_in_quoted_tokens is not functional.
    pub fn set_specctra_mode(&mut self, mode: bool) {
        self.specctra_mode = mode;

        if mode {
            // specctra mode defaults, some of which can still be changed in this mode.
            self.space_in_quoted_tokens = true;
        } else {
            self.space_in_quoted_tokens = false;
            self.string_delimiter = b'"';
        }
    }

    /// Port of `DSNLEXER::PushReader`: manage a stack of line readers in order to handle nested
    /// file inclusion.
    ///
    /// This function pushes `line_reader` onto the top of a stack of readers and makes it the
    /// current reader with its own source name, line number and line text.  A grammar must be
    /// designed such that the "include" token (whatever its various names), and any of its
    /// parameters are not followed by anything on that same line, because
    /// [`pop_reader`](Self::pop_reader) always starts reading from a new line upon returning to
    /// the original reader.
    pub fn push_reader(&mut self, line_reader: Box<dyn LineReader>) {
        self.reader_stack.push(Some(line_reader));

        // force a new readLine() as first thing.
        self.limit = 0;
        self.next = 0;
    }

    /// Port of `DSNLEXER::PopReader`: remove the top most line reader from the internal stack.
    ///
    /// The most recently used former reader on the stack becomes the current reader and its
    /// previous position in its input stream and its latest line number should pertain.
    /// `pop_reader` always starts reading from a new line upon returning to the previous
    /// reader.
    ///
    /// Returns the reader that was in use before the pop, or `None` if there was no reader on
    /// the stack (or it is currently lent to another lexer).
    pub fn pop_reader(&mut self) -> Option<Box<dyn LineReader>> {
        let ret = self.reader_stack.pop().flatten();

        // force a new readLine() as first thing.
        self.limit = 0;
        self.next = 0;

        ret
    }

    /// Port of `DSNLEXER::readLine`.
    fn read_line(&mut self) -> Result<usize, IoError> {
        if let Some(Some(reader)) = self.reader_stack.last_mut() {
            reader.read_line()?;

            let len = reader.length() as usize;

            // start may have changed in ReadLine(), which can resize and
            // relocate reader's line buffer.
            self.next = 0;
            self.limit = len;

            return Ok(len);
        }

        Ok(0)
    }

    /// Look `tok` up in the keyword table.
    fn lookup_keyword(&self, tok: &[u8]) -> Option<i32> {
        match &self.keyword_order {
            None => self
                .keywords
                .binary_search_by(|keyword| keyword.as_bytes().cmp(tok))
                .ok()
                .map(|index| index as i32),
            Some(order) => {
                // `order` is stably sorted, so among equal keywords the first of the table
                // comes first (the C++ hash table keeps the first of duplicate keys as well).
                let position =
                    order.partition_point(|&index| self.keywords[index as usize].as_bytes() < tok);

                order
                    .get(position)
                    .filter(|&&index| self.keywords[index as usize].as_bytes() == tok)
                    .map(|&index| index as i32)
            }
        }
    }

    /// Port of `DSNLEXER::findToken`: take `tok` and look up the string in the keywords table.
    ///
    /// Returns the keyword's token value, or `DSN_SYMBOL` if `tok` is not in the keywords
    /// table.
    fn find_token(&self, tok: &[u8]) -> i32 {
        // The C++ lookup works on tok.c_str(), i.e. on the text up to the first NUL.
        let tok = match tok.iter().position(|&b| b == 0) {
            Some(nul) => &tok[..nul],
            None => tok,
        };

        if let Some(token) = self.lookup_keyword(tok) {
            return token;
        }

        // The Specctra DSN grammar is case-insensitive and the keyword table is lowercase
        if self.specctra_mode {
            let lower = tok.to_ascii_lowercase();

            if let Some(token) = self.lookup_keyword(&lower) {
                return token;
            }
        }

        DSN_SYMBOL // not a keyword, some arbitrary symbol.
    }

    /// Port of `DSNLEXER::GetTokenText` for a raw token value.
    fn token_text_i32(&self, tok: i32) -> &'static str {
        if tok < 0 {
            syntax(tok)
        } else if (tok as usize) < self.keywords.len() {
            self.keywords[tok as usize]
        } else {
            "token too big"
        }
    }

    /// Port of `DSNLEXER::GetTokenString` for a raw token value.
    fn token_string_i32(&self, tok: i32) -> String {
        format!("'{}'", self.token_text_i32(tok))
    }

    /// Port of `DSNLEXER::isSep`: true if `cc` is an s-expression separator character.
    fn is_sep(&self, cc: u8) -> bool {
        is_space(cc) || cc == b'(' || cc == b')' || (self.knows_bar && cc == b'|')
    }

    /// Port of `DSNLEXER::isStringTerminator`.
    fn is_string_terminator(&self, cc: u8) -> bool {
        if !self.space_in_quoted_tokens && cc == b' ' {
            return true;
        }

        if cc == self.string_delimiter {
            return true;
        }

        false
    }

    /// Port of `DSNLEXER::NextTok`: return the next token found in the input, or the EOF token
    /// when reaching the end of the input.
    ///
    /// # Errors
    ///
    /// A `PARSE_ERROR` for an unterminated quoted string (and for malformed string delimiter
    /// definitions in specctra mode); an `IO_ERROR` if the line reader fails.
    pub fn next_tok(&mut self) -> Result<T, IoError> {
        self.next_tok_i32().map(T::from_i32)
    }

    /// `DSNLEXER::NextTok` on the raw `int` token values.
    fn next_tok_i32(&mut self) -> Result<i32, IoError> {
        // In C++ `cur` and `head` are pointers into the line; here they are offsets from the
        // line start.
        let mut cur = self.next;
        let mut head = cur;

        self.prev_tok = self.cur_tok;
        self.cur_separator.clear();

        'exit: {
            if self.cur_tok == DSN_EOF {
                break 'exit;
            }

            // The code up to the token recognition is a loop over `goto L_read`.
            let mut read = cur >= self.limit;

            let line: &str = loop {
                if read {
                    // L_read:
                    // blank lines are returned as "\n" and will have a len of 1.
                    // EOF will have a len of 0 and so is detectable.
                    let len = self.read_line()?;

                    if len == 0 {
                        // after readLine(), since start can change, set cur offset to start
                        cur = 0;
                        self.cur_tok = DSN_EOF;
                        break 'exit;
                    }

                    cur = 0; // after readLine() since start can change.
                }

                let line: &str = match self.reader_stack.last() {
                    Some(Some(reader)) => reader.line(),
                    _ => "",
                };
                let bytes = line.as_bytes();

                // Guard against a reader whose line() and length() disagree.
                if self.limit > bytes.len() {
                    self.limit = bytes.len();
                }

                // skip leading whitespace
                while cur < self.limit && is_space(bytes[cur]) {
                    self.cur_separator.push(char::from(bytes[cur]));
                    cur += 1;
                }

                if read {
                    // If the first non-blank character is #, this line is a comment.
                    // Comments cannot follow any other token on the same line.
                    if cur < self.limit && bytes[cur] == b'#' {
                        if self.comments_are_tokens {
                            // Grab the entire current line [excluding end of line char(s)] as the
                            // current token.  The '#' character may not be at offset zero.

                            while self.limit > 0
                                && (bytes[self.limit - 1] == b'\n'
                                    || bytes[self.limit - 1] == b'\r')
                            {
                                self.limit -= 1;
                            }

                            assign_text(
                                &mut self.cur_text,
                                &mut self.cur_text_raw,
                                &bytes[..self.limit],
                            );

                            cur = 0; // ensure a good curOffset below
                            self.cur_tok = DSN_COMMENT;
                            head = self.limit; // do a readLine() on next call in here.
                            break 'exit;
                        } else {
                            continue; // goto L_read
                        }
                    }
                }

                if cur >= self.limit {
                    read = true;
                    continue; // goto L_read
                }

                break line;
            };

            let bytes = line.as_bytes();
            let limit = self.limit;
            let cc = bytes[cur];

            if cc == b'(' {
                assign_text(&mut self.cur_text, &mut self.cur_text_raw, b"(");
                self.cur_tok = DSN_LEFT;
                head = cur + 1;
                break 'exit;
            }

            if cc == b')' {
                assign_text(&mut self.cur_text, &mut self.cur_text_raw, b")");
                self.cur_tok = DSN_RIGHT;
                head = cur + 1;
                break 'exit;
            }

            if self.knows_bar && cc == b'|' {
                assign_text(&mut self.cur_text, &mut self.cur_text_raw, b"|");
                self.cur_tok = DSN_BAR;
                head = cur + 1;
                break 'exit;
            }

            // Non-specctraMode, understands and deciphers escaped \, \r, \n, and \".
            // Strips off leading and trailing double quotes
            if !self.specctra_mode {
                // a quoted string, will return DSN_STRING
                if cc == self.string_delimiter {
                    // copy the token, character by character so we can remove doubled up quotes.
                    let mut text = std::mem::take(&mut self.cur_text).into_bytes();
                    text.clear();

                    // skip over the leading delimiter, which is always " in non-specctraMode
                    cur += 1;

                    head = cur;

                    // The C++ line buffer is NUL terminated, so reading at `limit` yields 0.
                    let at = |index: usize| if index < limit { bytes[index] } else { 0 };

                    let mut terminated = false;

                    while head < limit {
                        // ESCAPE SEQUENCES:
                        if bytes[head] == b'\\' {
                            head += 1;

                            if head >= limit {
                                break; // throw exception at L_unterminated
                            }

                            let escaped = bytes[head];
                            head += 1;

                            let c: u8 = match escaped {
                                b'"' | b'\\' => escaped,
                                b'a' => 0x07,
                                b'b' => 0x08,
                                b'f' => 0x0c,
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => 0x09,
                                b'v' => 0x0b,

                                b'x' => {
                                    // 1 or 2 byte hex escape sequence
                                    let mut i = 0;
                                    let mut value: u32 = 0;

                                    while i < 2 {
                                        let Some(digit) = char::from(at(head + i)).to_digit(16)
                                        else {
                                            break;
                                        };

                                        value = value * 16 + digit;
                                        i += 1;
                                    }

                                    head += i;

                                    if i > 0 {
                                        value as u8
                                    } else {
                                        b'x' // a goofed hex escape sequence, interpret as 'x'
                                    }
                                }

                                _ => {
                                    // 1-3 byte octal escape sequence
                                    head -= 1;

                                    let mut i = 0;
                                    let mut value: u32 = 0;

                                    while i < 3 {
                                        let digit = at(head + i);

                                        if !(b'0'..=b'7').contains(&digit) {
                                            break;
                                        }

                                        value = value * 8 + u32::from(digit - b'0');
                                        i += 1;
                                    }

                                    head += i;

                                    if i > 0 {
                                        // c = (char) strtoul( tbuf, nullptr, 8 ): up to 0777,
                                        // truncated to 8 bits.
                                        value as u8
                                    } else {
                                        b'\\' // a goofed octal escape sequence, interpret as '\'
                                    }
                                }
                            };

                            text.push(c);
                        } else if bytes[head] == b'"' {
                            // end of the non-specctraMode DSN_STRING
                            self.cur_tok = DSN_STRING;
                            head += 1; // omit this trailing double quote
                            terminated = true;
                            break;
                        } else {
                            // Copy the run of ordinary characters up to the next quote or
                            // backslash in one go.
                            let run = bytes[head..limit]
                                .iter()
                                .position(|&b| b == b'\\' || b == b'"')
                                .unwrap_or(limit - head);

                            text.extend_from_slice(&bytes[head..head + run]);
                            head += run;
                        }
                    }

                    let text_len = text.len();

                    match String::from_utf8(text) {
                        Ok(valid) => {
                            self.cur_text = valid;
                            self.cur_text_raw = None;
                        }
                        Err(invalid) => {
                            let raw = invalid.into_bytes();
                            self.cur_text = String::from_utf8_lossy(&raw).into_owned();
                            self.cur_text_raw = Some(raw);
                        }
                    }

                    if terminated {
                        break 'exit;
                    }

                    // L_unterminated:
                    return Err(IoError::parse(
                        "Unterminated delimited string",
                        self.cur_source(),
                        self.cur_line(),
                        self.cur_line_number(),
                        (cur + text_len) as i32,
                    ));
                }
            } else {
                // is specctraMode, tests in this block should not occur in KiCad mode.

                /*  get the dash out of a <pin_reference> which is embedded for example
                    like:  U2-14 or "U2"-"14"
                    This is detectable by a non-space immediately preceding the dash.
                */
                if cc == b'-' && cur > 0 && !is_space(bytes[cur - 1]) {
                    assign_text(&mut self.cur_text, &mut self.cur_text_raw, b"-");
                    self.cur_tok = DSN_DASH;
                    head = cur + 1;
                    break 'exit;
                }

                // switching the string_quote character
                if self.prev_tok == DSN_STRING_QUOTE {
                    const ERRTXT: &str =
                        "String delimiter must be a single character of ', \", or $";

                    match cc {
                        b'\'' | b'$' | b'"' => {}
                        _ => {
                            return Err(self.parse_error(ERRTXT));
                        }
                    }

                    assign_text(&mut self.cur_text, &mut self.cur_text_raw, &[cc]);

                    head = cur + 1;

                    if head < limit && !self.is_sep(bytes[head]) {
                        return Err(self.parse_error(ERRTXT));
                    }

                    self.cur_tok = DSN_QUOTE_DEF;
                    break 'exit;
                }

                // specctraMode DSN_STRING
                if cc == self.string_delimiter {
                    cur += 1; // skip over the leading delimiter: ",', or $

                    head = cur;

                    while head < limit && !self.is_string_terminator(bytes[head]) {
                        head += 1;
                    }

                    if head >= limit {
                        return Err(self.parse_error("Un-terminated delimited string"));
                    }

                    assign_text(
                        &mut self.cur_text,
                        &mut self.cur_text_raw,
                        &bytes[cur..head],
                    );

                    head += 1; // skip over the trailing delimiter

                    self.cur_tok = DSN_STRING;
                    break 'exit;
                }
            } // specctraMode

            // non-quoted token, read it into curText.
            head = cur;

            while head < limit && !self.is_sep(bytes[head]) {
                head += 1;
            }

            match line.get(cur..head) {
                Some(token) => {
                    self.cur_text.clear();
                    self.cur_text.push_str(token);
                    self.cur_text_raw = None;
                }
                None => {
                    assign_text(
                        &mut self.cur_text,
                        &mut self.cur_text_raw,
                        &bytes[cur..head],
                    );
                }
            }

            if is_number_text(&bytes[cur..head]) {
                self.cur_tok = DSN_NUMBER;
                break 'exit;
            }

            if self.specctra_mode && &bytes[cur..head] == b"string_quote" {
                self.cur_tok = DSN_STRING_QUOTE;
                break 'exit;
            }

            self.cur_tok = self.find_token(&bytes[cur..head]);
        }

        // exit:   single point of exit, no returns elsewhere please.

        self.cur_offset = cur as i32;

        self.next = head;

        Ok(self.cur_tok)
    }

    /// Port of `DSNLEXER::NeedSYMBOL`: call [`next_tok`](Self::next_tok) and then verify that
    /// the token read in satisfies [`is_symbol`](Self::is_symbol).
    ///
    /// Returns the actual token read in.
    ///
    /// # Errors
    ///
    /// An error if the next token does not satisfy `is_symbol()`.
    pub fn need_symbol(&mut self) -> Result<T, IoError> {
        let tok = self.next_tok_i32()?;

        if !is_symbol_i32(tok) {
            return Err(self.expecting_i32(DSN_SYMBOL));
        }

        Ok(T::from_i32(tok))
    }

    /// Port of `DSNLEXER::NeedSYMBOLorNUMBER`: call [`next_tok`](Self::next_tok) and then
    /// verify that the token read in satisfies [`is_symbol`](Self::is_symbol) or is a number.
    ///
    /// Returns the actual token read in.
    ///
    /// # Errors
    ///
    /// An error if the next token does not satisfy the above test.
    pub fn need_symbol_or_number(&mut self) -> Result<T, IoError> {
        let tok = self.next_tok_i32()?;

        if !is_symbol_i32(tok) && tok != DSN_NUMBER {
            return Err(self.expecting_str("a symbol or number"));
        }

        Ok(T::from_i32(tok))
    }

    /// Port of `DSNLEXER::NeedNUMBER`: call [`next_tok`](Self::next_tok) and then verify that
    /// the token read is a number.
    ///
    /// Returns the actual token read in.
    ///
    /// # Errors
    ///
    /// An error (`need a number for '<expectation>'`) if the next token is not a number.
    pub fn need_number(&mut self, expectation: &str) -> Result<T, IoError> {
        let tok = self.next_tok_i32()?;

        if tok != DSN_NUMBER {
            return Err(self.parse_error(format!("need a number for '{expectation}'")));
        }

        Ok(T::from_i32(tok))
    }

    /// Port of `DSNLEXER::CurTok`: return whatever [`next_tok`](Self::next_tok) returned the
    /// last time it was called.
    pub fn cur_tok(&self) -> T {
        T::from_i32(self.cur_tok)
    }

    /// Port of `DSNLEXER::PrevTok`: return whatever [`next_tok`](Self::next_tok) returned the
    /// 2nd to last time it was called.
    pub fn prev_tok(&self) -> T {
        T::from_i32(self.prev_tok)
    }

    /// Port of `DSNLEXER::GetCurStrAsToken`: used to support "loose" matches (quoted tokens).
    pub fn cur_str_as_token(&self) -> T {
        T::from_i32(self.find_token(self.cur_bytes()))
    }

    /// Port of `DSNLEXER::SetStringDelimiter`: change the string delimiter from the default `"`
    /// to some other character and return the old value.  Only effective in specctra mode.
    pub fn set_string_delimiter(&mut self, string_delimiter: u8) -> u8 {
        let old = self.string_delimiter;

        if self.specctra_mode {
            self.string_delimiter = string_delimiter;
        }

        old
    }

    /// Port of `DSNLEXER::SetSpaceInQuotedTokens`: change the setting controlling whether a
    /// space in a quoted string is a terminator.  Only effective in specctra mode.  Returns the
    /// old setting.
    pub fn set_space_in_quoted_tokens(&mut self, val: bool) -> bool {
        let old = self.space_in_quoted_tokens;

        if self.specctra_mode {
            self.space_in_quoted_tokens = val;
        }

        old
    }

    /// Port of `DSNLEXER::SetKnowsBar`.
    ///
    /// This is used to control whether the lexer knows about the `DSN_BAR` token.  If it does
    /// not, then it will not return a `DSN_BAR` token, but rather treat it as a string.  This
    /// is used to support the KiCad legacy format, which does not have a `DSN_BAR`.
    pub fn set_knows_bar(&mut self, knows_bar: bool) {
        self.knows_bar = knows_bar;
    }

    /// Port of `DSNLEXER::SetCommentsAreTokens`: change the handling of comments.
    ///
    /// If set true, comments are returned as single line strings (without the line ending).
    /// Otherwise they are consumed by the lexer and not returned.  Returns the old setting.
    pub fn set_comments_are_tokens(&mut self, val: bool) -> bool {
        let old = self.comments_are_tokens;
        self.comments_are_tokens = val;
        old
    }

    /// Port of `DSNLEXER::ReadCommentLines`: check the next sequence of tokens and read them
    /// into a list if they are comments.
    ///
    /// Reading continues until a non-comment token is encountered, and such last read token
    /// remains as [`cur_tok`](Self::cur_tok) and as [`cur_text`](Self::cur_text).  No push back
    /// or "un get" mechanism is used for this support.  Upon return you simply avoid calling
    /// `next_tok()` for the next token, but rather `cur_tok()`.
    ///
    /// Returns the comment lines, or `None` if there are none.
    pub fn read_comment_lines(&mut self) -> Result<Option<Vec<String>>, IoError> {
        let mut ret: Option<Vec<String>> = None;
        let cmt_setting = self.set_comments_are_tokens(true);
        let mut tok = self.next_tok_i32()?;

        if tok == DSN_COMMENT {
            let mut lines = Vec::new();

            loop {
                lines.push(self.from_utf8());

                tok = self.next_tok_i32()?;

                if tok != DSN_COMMENT {
                    break;
                }
            }

            ret = Some(lines);
        }

        self.set_comments_are_tokens(cmt_setting);

        Ok(ret)
    }

    /// Port of `DSNLEXER::IsSymbol`: test a token to see if it is a symbol.
    ///
    /// This means it cannot be a special delimiter character such as `DSN_LEFT`, `DSN_RIGHT`,
    /// `DSN_QUOTE_DEF`, etc.  It may however, coincidentally match a keyword and still be a
    /// symbol.
    pub fn is_symbol(tok: T) -> bool {
        is_symbol_i32(tok.to_i32())
    }

    /// Port of `DSNLEXER::IsNumber`.
    pub fn is_number(tok: T) -> bool {
        tok.to_i32() == DSN_NUMBER
    }

    /// Port of the C++ macro call `THROW_PARSE_ERROR( problem, CurSource(), CurLine(),
    /// CurLineNumber(), CurOffset() )` that parsers built on the lexer use everywhere:
    /// a `PARSE_ERROR` located at the current token.
    pub fn parse_error(&self, problem: impl Into<String>) -> IoError {
        IoError::parse(
            problem,
            self.cur_source(),
            self.cur_line(),
            self.cur_line_number(),
            self.cur_offset(),
        )
    }

    fn expecting_i32(&self, tok: i32) -> IoError {
        self.parse_error(format!("Expecting '{}'", self.token_string_i32(tok)))
    }

    /// Port of `DSNLEXER::Expecting( int aTok )`: make an error with an input file specific
    /// error message.
    ///
    /// `tok` is the token/keyword type which was expected at the current input location.
    ///
    /// Note that the message quotes the token twice (`Expecting ''(''`), as C++ does: the
    /// format string adds quotes around `GetTokenString()`, which already has them.
    pub fn expecting(&self, tok: T) -> IoError {
        self.expecting_i32(tok.to_i32())
    }

    /// Port of `DSNLEXER::Expecting( const char* aTokenList )`: make an error with an input
    /// file specific error message.
    ///
    /// `token_list` is the token/keyword type which was expected at the current input location,
    /// e.g. "pin|graphic|property".
    pub fn expecting_str(&self, token_list: &str) -> IoError {
        self.parse_error(format!(
            "Expecting {}. Got '{}'",
            token_list,
            self.token_string_i32(self.cur_tok)
        ))
    }

    /// Port of `DSNLEXER::Unexpected( int aTok )`: make an error with an input file specific
    /// error message.
    ///
    /// `tok` is the token/keyword type which was not expected at the current input location.
    pub fn unexpected(&self, tok: T) -> IoError {
        self.parse_error(format!(
            "Unexpected '{}'",
            self.token_string_i32(tok.to_i32())
        ))
    }

    /// Port of `DSNLEXER::Unexpected( const char* aToken )`: make an error with an input file
    /// specific error message.
    ///
    /// `token` is the token which was not expected at the current input location.
    pub fn unexpected_str(&self, token: &str) -> IoError {
        self.parse_error(format!("Unexpected {token}"))
    }

    /// Port of `DSNLEXER::Duplicate`: make an error with a message saying specifically that
    /// `tok` is a duplicate of one already seen in current context.
    pub fn duplicate(&self, tok: T) -> IoError {
        self.parse_error(format!(
            "{} is a duplicate",
            self.token_string_i32(tok.to_i32())
        ))
    }

    /// Port of `DSNLEXER::NeedLEFT`: call [`next_tok`](Self::next_tok) and then verify that the
    /// token read in is a `DSN_LEFT`.
    pub fn need_left(&mut self) -> Result<(), IoError> {
        let tok = self.next_tok_i32()?;

        if tok != DSN_LEFT {
            return Err(self.expecting_i32(DSN_LEFT));
        }

        Ok(())
    }

    /// Port of `DSNLEXER::NeedRIGHT`: call [`next_tok`](Self::next_tok) and then verify that
    /// the token read in is a `DSN_RIGHT`.
    pub fn need_right(&mut self) -> Result<(), IoError> {
        let tok = self.next_tok_i32()?;

        if tok != DSN_RIGHT {
            return Err(self.expecting_i32(DSN_RIGHT));
        }

        Ok(())
    }

    /// Port of `DSNLEXER::NeedBAR`: call [`next_tok`](Self::next_tok) and then verify that the
    /// token read in is a `DSN_BAR`.
    pub fn need_bar(&mut self) -> Result<(), IoError> {
        let tok = self.next_tok_i32()?;

        if tok != DSN_BAR {
            return Err(self.expecting_i32(DSN_BAR));
        }

        Ok(())
    }

    /// Port of `DSNLEXER::GetTokenText`: return the text of a token value: the keyword for a
    /// keyword token, a description (see [`syntax`]) for a syntax token.
    pub fn token_text(&self, tok: T) -> &'static str {
        self.token_text_i32(tok.to_i32())
    }

    /// Port of `DSNLEXER::GetTokenString`: return a quote wrapped representation of a token
    /// value.
    pub fn token_string(&self, tok: T) -> String {
        self.token_string_i32(tok.to_i32())
    }

    /// Port of `DSNLEXER::CurText`: return the current token's text.
    ///
    /// The text of a quoted string token is the unescaped content.  In the (pathological) case
    /// that `\x..` or octal escapes produced bytes that are not valid UTF-8, this is the lossy
    /// conversion (U+FFFD for the invalid parts); [`cur_bytes`](Self::cur_bytes) has the exact
    /// bytes.
    pub fn cur_text(&self) -> &str {
        &self.cur_text
    }

    /// Port of `DSNLEXER::CurStr`: return the current token text (the same as
    /// [`cur_text`](Self::cur_text); C++ has a `const char*` and a `std::string` accessor).
    pub fn cur_str(&self) -> &str {
        &self.cur_text
    }

    /// The exact bytes of the current token text, as held by the C++ `curText`.
    pub fn cur_bytes(&self) -> &[u8] {
        match &self.cur_text_raw {
            Some(raw) => raw,
            None => self.cur_text.as_bytes(),
        }
    }

    /// Port of `DSNLEXER::CurSeparator`: the white space that preceded the current token.
    pub fn cur_separator(&self) -> &str {
        &self.cur_separator
    }

    /// Port of `DSNLEXER::FromUTF8`: return the current token text as a string, assuming that
    /// the input byte stream is UTF8 encoded.
    ///
    /// As with `wxString::FromUTF8`, the result is empty if the token text is not valid UTF-8.
    pub fn from_utf8(&self) -> String {
        if self.cur_text_raw.is_some() {
            String::new()
        } else {
            self.cur_text.clone()
        }
    }

    /// Port of `DSNLEXER::CurLineNumber`: return the current line number within my line reader
    /// (0 if there is no reader).
    pub fn cur_line_number(&self) -> i32 {
        self.reader()
            .map_or(0, |reader| reader.line_number() as i32)
    }

    /// Port of `DSNLEXER::CurLine`: return the current line of text from which the
    /// [`cur_text`](Self::cur_text) would return its token (empty if there is no reader).
    pub fn cur_line(&self) -> &str {
        self.reader().map_or("", |reader| reader.line())
    }

    /// Port of `DSNLEXER::CurSource`: return the current line reader source, e.g. a filename or
    /// "clipboard" (empty if there is no reader).
    pub fn cur_source(&self) -> &str {
        self.reader().map_or("", |reader| reader.source())
    }

    /// Port of `DSNLEXER::CurOffset`: return the byte offset within the current line, using a 1
    /// based index.
    pub fn cur_offset(&self) -> i32 {
        self.cur_offset + 1
    }

    /// Port of `DSNLEXER::parseDouble()`: parse the current token as an ASCII numeric string
    /// with possible leading whitespace into a double precision floating point number.
    ///
    /// The accepted syntax is that of `fast_float::from_chars` with the `general` format (see
    /// [`from_chars_double`]); as in C++ only a leading number is required, trailing characters
    /// in the token are ignored.
    ///
    /// # Errors
    ///
    /// "Invalid floating point number" if the current token does not start with a number or
    /// the number is out of the range of a double.
    pub fn parse_double(&self) -> Result<f64, IoError> {
        // Use fast_float::from_chars which is designed to be locale independent and
        // significantly faster than strtod and std::from_chars
        match from_chars_double(self.cur_bytes()) {
            Ok((dval, _)) => Ok(dval),
            Err(_) => Err(self.parse_error("Invalid floating point number")),
        }
    }

    /// Port of `DSNLEXER::parseDouble( const char* aExpected )`: read the next token, which
    /// must be a number, and parse it.
    pub fn parse_double_expected(&mut self, expected: &str) -> Result<f64, IoError> {
        self.need_number(expected)?;
        self.parse_double()
    }

    /// Port of the template `DSNLEXER::parseDouble( T aToken )`:
    /// `parseDouble( GetTokenText( aToken ) )`.
    pub fn parse_double_tok(&mut self, token: T) -> Result<f64, IoError> {
        let expected = self.token_text(token);
        self.parse_double_expected(expected)
    }

    /// Port of the `parseInt()` helper that the s-expression parsers built on `DSNLEXER` define
    /// (e.g. `PCB_IO_KICAD_SEXPR_PARSER::parseInt`): `(int) strtol( CurText(), nullptr, 10 )`.
    ///
    /// Like the C function this parses a leading decimal integer and yields 0 if there is none;
    /// the 64 bit `long` result is truncated to `int`.
    pub fn parse_int(&self) -> i32 {
        strtol(self.cur_bytes(), 10).0 as i32
    }

    /// Port of `parseInt( const char* aExpected )` of the same parsers: read the next token,
    /// which must be a number, and parse it with [`parse_int`](Self::parse_int).
    pub fn parse_int_expected(&mut self, expected: &str) -> Result<i32, IoError> {
        self.need_number(expected)?;
        Ok(self.parse_int())
    }

    /// Port of `parseHex()` of the same parsers: read the next token and return
    /// `strtol( CurText(), nullptr, 16 )`.
    pub fn parse_hex(&mut self) -> Result<i64, IoError> {
        self.next_tok_i32()?;
        Ok(strtol(self.cur_bytes(), 16).0)
    }
}

impl DsnLexer<i32> {
    /// Port of `DSNLEXER::DSNLEXER( const KEYWORD* aKeywordTable, unsigned aKeywordCount,
    /// const KEYWORD_MAP* aKeywordMap, const std::string& aSExpression,
    /// const wxString& aSource )` for a keyword table given at run time: the lexer returns the
    /// index into `keywords` for a keyword and the negative `DSN_*` values otherwise.
    ///
    /// Unlike for [`KeywordSet::KEYWORDS`] the table may be in any order.
    pub fn with_keywords(
        keywords: &'static [&'static str],
        s_expression: impl Into<String>,
        source: &str,
    ) -> Self {
        let mut this = Self::with_table(keywords);
        let source = if source.is_empty() {
            FMT_CLIPBOARD
        } else {
            source
        };

        this.push_reader(Box::new(StringLineReader::new(s_expression, source)));
        this
    }

    /// Port of `DSNLEXER::DSNLEXER( const KEYWORD* aKeywordTable, unsigned aKeywordCount,
    /// const KEYWORD_MAP* aKeywordMap, LINE_READER* aLineReader = nullptr )` for a keyword
    /// table given at run time; see [`with_keywords`](Self::with_keywords) and
    /// [`from_line_reader`](Self::from_line_reader).
    pub fn with_keywords_from_line_reader(
        keywords: &'static [&'static str],
        line_reader: Option<Box<dyn LineReader>>,
    ) -> Self {
        let mut this = Self::with_table(keywords);

        if let Some(line_reader) = line_reader {
            this.push_reader(line_reader);
        }

        this
    }
}

/// Port of `DSNLEXER::IsSymbol` on a raw token value.
fn is_symbol_i32(tok: i32) -> bool {
    // if aTok is >= 0, then it is a coincidental match to a keyword.
    tok == DSN_SYMBOL || tok == DSN_STRING || tok >= 0
}

impl<T: KeywordSet> std::fmt::Debug for DsnLexer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DsnLexer")
            .field("source", &self.cur_source())
            .field("line_number", &self.cur_line_number())
            .field("cur_offset", &self.cur_offset)
            .field("cur_tok", &T::from_i32(self.cur_tok))
            .field("prev_tok", &T::from_i32(self.prev_tok))
            .field("cur_text", &self.cur_text)
            .field("specctra_mode", &self.specctra_mode)
            .field("knows_bar", &self.knows_bar)
            .field("string_delimiter", &char::from(self.string_delimiter))
            .field("space_in_quoted_tokens", &self.space_in_quoted_tokens)
            .field("comments_are_tokens", &self.comments_are_tokens)
            .field("reader_stack_depth", &self.reader_stack.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    // The behaviour of the tokenizer itself is pinned against the C++ original by
    // tests/oracle_goldens.rs and tests/corpus.rs; the tests here cover the typed interface
    // and the parts that only exist in the port.

    use super::*;
    use crate::keywords::pcb_keys_t::T::{self, *};
    use crate::keywords::strokeparams_t;
    use crate::ki_exception::IoError;

    fn tokens(lexer: &mut DsnLexer<T>) -> Vec<(T, String)> {
        let mut out = Vec::new();

        loop {
            let tok = lexer.next_tok().unwrap();

            if tok == T_EOF {
                return out;
            }

            out.push((tok, lexer.cur_text().to_string()));
        }
    }

    #[test]
    fn typed_tokens() {
        let mut lexer = DsnLexer::<T>::new(
            "(kicad_pcb (version 20241229)\n  (at 1.5 -2) \"F.Cu\" foo|)",
            "",
        );

        assert_eq!(lexer.cur_tok(), T_NONE);
        assert_eq!(lexer.cur_source(), "clipboard");

        assert_eq!(
            tokens(&mut lexer),
            [
                (T_LEFT, "(".to_string()),
                (T_kicad_pcb, "kicad_pcb".to_string()),
                (T_LEFT, "(".to_string()),
                (T_version, "version".to_string()),
                (T_NUMBER, "20241229".to_string()),
                (T_RIGHT, ")".to_string()),
                (T_LEFT, "(".to_string()),
                (T_at, "at".to_string()),
                (T_NUMBER, "1.5".to_string()),
                (T_NUMBER, "-2".to_string()),
                (T_RIGHT, ")".to_string()),
                (T_STRING, "F.Cu".to_string()),
                (T_SYMBOL, "foo".to_string()),
                (T_BAR, "|".to_string()),
                (T_RIGHT, ")".to_string()),
            ]
        );

        assert_eq!(lexer.cur_tok(), T_EOF);
        assert_eq!(lexer.prev_tok(), T_RIGHT);
        assert_eq!(lexer.cur_line_number(), 3);
    }

    #[test]
    fn positions() {
        let mut lexer = DsnLexer::<T>::new("(at 1\n   \"x\" layer)", "pos.kicad_pcb");

        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!((lexer.cur_line_number(), lexer.cur_offset()), (1, 1));
        assert_eq!(lexer.cur_line(), "(at 1\n");
        assert_eq!(lexer.cur_source(), "pos.kicad_pcb");

        assert_eq!(lexer.next_tok().unwrap(), T_at);
        assert_eq!((lexer.cur_line_number(), lexer.cur_offset()), (1, 2));
        assert_eq!(lexer.cur_separator(), "");

        assert_eq!(lexer.next_tok().unwrap(), T_NUMBER);
        assert_eq!((lexer.cur_line_number(), lexer.cur_offset()), (1, 5));
        assert_eq!(lexer.cur_separator(), " ");

        // The offset of a string is that of its first character behind the quote.
        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!((lexer.cur_line_number(), lexer.cur_offset()), (2, 5));
        assert_eq!(lexer.cur_separator(), "\n   ");
        assert_eq!(lexer.cur_line(), "   \"x\" layer)");

        assert_eq!(lexer.next_tok().unwrap(), T_layer);
        assert_eq!(lexer.cur_offset(), 8);
        assert_eq!(lexer.cur_str(), "layer");
        assert_eq!(lexer.cur_bytes(), b"layer");
        assert_eq!(lexer.from_utf8(), "layer");
    }

    #[test]
    fn symbol_and_number_predicates() {
        assert!(DsnLexer::<T>::is_symbol(T_SYMBOL));
        assert!(DsnLexer::<T>::is_symbol(T_STRING));
        assert!(DsnLexer::<T>::is_symbol(T_at));
        assert!(!DsnLexer::<T>::is_symbol(T_NUMBER));
        assert!(!DsnLexer::<T>::is_symbol(T_LEFT));
        assert!(!DsnLexer::<T>::is_symbol(T_EOF));
        assert!(DsnLexer::<T>::is_number(T_NUMBER));
        assert!(!DsnLexer::<T>::is_number(T_SYMBOL));
    }

    #[test]
    fn token_texts() {
        let lexer = DsnLexer::<T>::new("", "s");

        assert_eq!(lexer.token_text(T_at), "at");
        assert_eq!(lexer.token_text(T_LEFT), "(");
        assert_eq!(lexer.token_text(T_STRING), "quoted string");
        assert_eq!(lexer.token_string(T_layer), "'layer'");
        assert_eq!(lexer.token_string(T_EOF), "'end of input'");
        assert_eq!(syntax(DSN_NUMBER), "number");
        assert_eq!(syntax(0), "???");
    }

    #[test]
    fn error_messages() {
        let mut lexer = DsnLexer::<T>::new("(at 1 2)\n", "board.kicad_pcb");

        lexer.next_tok().unwrap();
        lexer.next_tok().unwrap();

        // GetTokenString() already quotes the token and the format string quotes it again.
        let err = lexer.expecting(T_LEFT);
        assert_eq!(
            err.problem(),
            "Expecting ''('' in 'board.kicad_pcb', line 1, offset 2."
        );

        let pe = err.as_parse_error().expect("a PARSE_ERROR");
        assert_eq!(pe.parse_problem(), "Expecting ''(''");
        assert_eq!(pe.line_number, 1);
        assert_eq!(pe.byte_index, 2);
        assert_eq!(pe.input_line, "(at 1 2)\n");

        assert_eq!(
            lexer
                .expecting(T_layer)
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Expecting ''layer''"
        );
        assert_eq!(
            lexer
                .expecting_str("yes or no")
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Expecting yes or no. Got ''at''"
        );
        assert_eq!(
            lexer
                .unexpected(T_at)
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Unexpected ''at''"
        );
        assert_eq!(
            lexer
                .unexpected_str("end of file")
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Unexpected end of file"
        );
        assert_eq!(
            lexer
                .duplicate(T_at)
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "'at' is a duplicate"
        );
        assert_eq!(
            lexer.parse_error("custom").problem(),
            "custom in 'board.kicad_pcb', line 1, offset 2."
        );
    }

    #[test]
    fn need_functions() {
        let mut lexer = DsnLexer::<T>::new("( at 5 \"s\" ) | foo", "s");

        lexer.need_left().unwrap();
        assert_eq!(lexer.need_symbol().unwrap(), T_at);
        assert_eq!(lexer.need_number("count").unwrap(), T_NUMBER);
        assert_eq!(lexer.need_symbol_or_number().unwrap(), T_STRING);
        lexer.need_right().unwrap();
        lexer.need_bar().unwrap();

        let err = lexer.need_left().unwrap_err();
        assert_eq!(
            err.as_parse_error().unwrap().parse_problem(),
            "Expecting ''(''"
        );
        // the offending token has been consumed
        assert_eq!(lexer.cur_tok(), T_SYMBOL);

        let mut lexer = DsnLexer::<T>::new("x ( )", "s");
        assert_eq!(
            lexer
                .need_number("width")
                .unwrap_err()
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "need a number for 'width'"
        );
        assert_eq!(
            lexer
                .need_symbol()
                .unwrap_err()
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Expecting ''symbol''"
        );
        assert_eq!(
            lexer
                .need_symbol_or_number()
                .unwrap_err()
                .as_parse_error()
                .unwrap()
                .parse_problem(),
            "Expecting a symbol or number. Got '')''"
        );
        assert!(lexer.need_right().is_err());
        assert!(lexer.need_bar().is_err());
    }

    #[test]
    fn numbers() {
        let mut lexer = DsnLexer::<T>::new("1.5 2.25 0.125 abc 17 -3 1F 7.9", "s");

        lexer.next_tok().unwrap();
        assert_eq!(lexer.parse_double().unwrap(), 1.5);
        assert_eq!(lexer.parse_double_expected("width").unwrap(), 2.25);
        assert_eq!(lexer.parse_double_tok(T_width).unwrap(), 0.125);

        let err = lexer.parse_double_tok(T_width).unwrap_err();
        assert_eq!(
            err.as_parse_error().unwrap().parse_problem(),
            "need a number for 'width'"
        );

        let err = lexer.parse_double().unwrap_err();
        assert_eq!(
            err.as_parse_error().unwrap().parse_problem(),
            "Invalid floating point number"
        );
        assert_eq!(lexer.parse_int(), 0);

        assert_eq!(lexer.parse_int_expected("count").unwrap(), 17);
        lexer.next_tok().unwrap();
        assert_eq!(lexer.parse_int(), -3);
        assert_eq!(lexer.parse_hex().unwrap(), 0x1f);
        lexer.next_tok().unwrap();
        assert_eq!(lexer.parse_int(), 7);
    }

    #[test]
    fn number_classification() {
        for text in ["0", "-1", "+1", "1.", ".5", "-.5", "1e5", "1E-5", "1.5e+10"] {
            assert!(is_number_text(text.as_bytes()), "{text}");
        }

        for text in [
            "", "-", "+", ".", "e5", "1e", "1e+", "1.2.3", "1a", "0x10", "--1", "1 ",
        ] {
            assert!(!is_number_text(text.as_bytes()), "{text}");
        }
    }

    #[test]
    fn escapes_that_are_not_utf8() {
        let mut lexer = DsnLexer::<T>::new("\"a\\xe9b\" \"\\xc3\\xa9\" plain", "s");

        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!(lexer.cur_bytes(), b"a\xe9b");
        assert_eq!(lexer.cur_text(), "a\u{fffd}b");
        // wxString::FromUTF8() of invalid UTF-8 is empty
        assert_eq!(lexer.from_utf8(), "");

        // Escapes may also build a valid multi-byte character.
        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!(lexer.cur_text(), "é");
        assert_eq!(lexer.from_utf8(), "é");

        assert_eq!(lexer.next_tok().unwrap(), T_SYMBOL);
        assert_eq!(lexer.cur_bytes(), b"plain");
    }

    #[test]
    fn unterminated_string() {
        let mut lexer = DsnLexer::<T>::new("(at \"abc\n)", "s");

        lexer.next_tok().unwrap();
        lexer.next_tok().unwrap();

        let err = lexer.next_tok().unwrap_err();
        let pe = err.as_parse_error().expect("a PARSE_ERROR");

        assert_eq!(pe.parse_problem(), "Unterminated delimited string");
        assert_eq!(pe.line_number, 1);
        // offset of the string start (0 based) plus the length of what was read
        assert_eq!(pe.byte_index, 9);
        assert_eq!(pe.input_line, "(at \"abc\n");

        // The lexer did not advance: the same error again.
        assert!(lexer.next_tok().is_err());
        assert_eq!(lexer.cur_tok(), T_at);
    }

    #[test]
    fn comments() {
        let text = "# one\n  # two\n(at) # three\n# four\n";

        let mut lexer = DsnLexer::<T>::new(text, "s");
        assert_eq!(
            lexer.read_comment_lines().unwrap(),
            Some(vec!["# one".to_string(), "  # two".to_string()])
        );
        assert_eq!(lexer.cur_tok(), T_LEFT);
        assert_eq!(lexer.read_comment_lines().unwrap(), None);
        assert_eq!(lexer.cur_tok(), T_at);

        let mut lexer = DsnLexer::<T>::new(text, "s");
        assert!(!lexer.set_comments_are_tokens(true));
        assert_eq!(lexer.next_tok().unwrap(), T_COMMENT);
        assert_eq!(lexer.cur_text(), "# one");
        assert!(lexer.set_comments_are_tokens(false));
        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
    }

    #[test]
    fn keyword_lookup_of_quoted_text() {
        let mut lexer = DsnLexer::<T>::new("\"layer\" \"Layer\"", "s");

        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!(lexer.cur_str_as_token(), T_layer);
        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!(lexer.cur_str_as_token(), T_SYMBOL);
    }

    #[test]
    fn lexer_without_keywords() {
        let mut lexer = DsnLexer::<DsnSyntaxT>::new("(at 1 \"s\")", "s");
        let mut toks = Vec::new();

        loop {
            let tok = lexer.next_tok().unwrap();
            toks.push(tok);

            if tok == DsnSyntaxT::Eof {
                break;
            }
        }

        assert_eq!(
            toks,
            [
                DsnSyntaxT::Left,
                DsnSyntaxT::Symbol,
                DsnSyntaxT::Number,
                DsnSyntaxT::String,
                DsnSyntaxT::Right,
                DsnSyntaxT::Eof
            ]
        );

        // The default type parameter is the keyword-less lexer.
        let mut lexer: DsnLexer = DsnLexer::new("x", "s");
        assert_eq!(lexer.next_tok().unwrap(), DsnSyntaxT::Symbol);
    }

    #[test]
    fn run_time_keyword_table_in_any_order() {
        static TABLE: [&str; 5] = ["zeta", "alpha", "mid", "alpha", "beta"];

        let mut lexer = DsnLexer::<i32>::with_keywords(&TABLE, "alpha beta mid zeta gamma (", "s");
        let mut toks = Vec::new();

        loop {
            let tok = lexer.next_tok().unwrap();

            if tok == DSN_EOF {
                break;
            }

            toks.push(tok);
        }

        // the first "alpha" wins, as in a hash table built from the list
        assert_eq!(toks, [1, 4, 2, 0, DSN_SYMBOL, DSN_LEFT]);
        assert_eq!(lexer.token_text(0), "zeta");
        assert_eq!(lexer.token_text(5), "token too big");
        assert_eq!(lexer.token_text(DSN_LEFT), "(");

        let mut lexer = DsnLexer::<i32>::with_keywords_from_line_reader(
            &TABLE,
            Some(Box::new(StringLineReader::new("beta", "reader"))),
        );
        assert_eq!(lexer.next_tok().unwrap(), 4);
        assert_eq!(lexer.cur_source(), "reader");
    }

    #[test]
    fn lexer_without_reader_is_at_eof() {
        let mut lexer = DsnLexer::<T>::from_line_reader(None);

        assert_eq!(lexer.next_tok().unwrap(), T_EOF);
        assert_eq!(lexer.cur_line_number(), 0);
        assert_eq!(lexer.cur_line(), "");
        assert_eq!(lexer.cur_source(), "");
        assert!(lexer.pop_reader().is_none());
    }

    #[test]
    fn sub_lexer_by_hand() {
        let mut lexer = DsnLexer::<T>::new(
            "(gr_line (stroke (width 0.1) (type solid)) (layer \"F.Cu\"))",
            "s",
        );

        for expected in [T_LEFT, T_gr_line, T_LEFT, T_stroke] {
            assert_eq!(lexer.next_tok().unwrap(), expected);
        }

        // STROKE_PARAMS_PARSER strokeParser( reader ); strokeParser.SyncLineReaderWith( *this );
        let mut stroke_lexer = DsnLexer::<strokeparams_t::T>::from_line_reader(None);
        assert!(stroke_lexer.sync_line_reader_with(&mut lexer));

        // The reader is with the other lexer now.
        assert_eq!(lexer.cur_source(), "");
        assert_eq!(stroke_lexer.cur_source(), "s");
        assert_eq!(stroke_lexer.cur_tok(), strokeparams_t::T::T_NONE);
        assert_eq!(stroke_lexer.cur_text(), "stroke");

        let mut seen = Vec::new();

        loop {
            let tok = stroke_lexer.next_tok().unwrap();
            seen.push(tok);

            if tok == strokeparams_t::T::T_solid {
                break;
            }
        }

        use strokeparams_t::T as S;
        assert_eq!(
            seen,
            [
                S::T_LEFT,
                S::T_width,
                S::T_NUMBER,
                S::T_RIGHT,
                S::T_LEFT,
                S::T_type,
                S::T_solid
            ]
        );

        stroke_lexer.need_right().unwrap();
        stroke_lexer.need_right().unwrap();

        // SyncLineReaderWith( strokeParser );
        assert!(lexer.sync_line_reader_with(&mut stroke_lexer));
        assert_eq!(lexer.cur_source(), "s");
        // The token state of the first lexer is its own.
        assert_eq!(lexer.cur_tok(), T_stroke);

        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!(lexer.next_tok().unwrap(), T_layer);

        // Two lexers that both have a reader do not share one.
        let mut other = DsnLexer::<T>::new("x", "other");
        assert!(!lexer.sync_line_reader_with(&mut other));
        assert!(!lexer.reclaim_line_reader_from(&mut other));

        // Two lexers without reader "share" the null reader.
        let mut a = DsnLexer::<T>::from_line_reader(None);
        let mut b = DsnLexer::<T>::from_line_reader(None);
        assert!(a.sync_line_reader_with(&mut b));
    }

    #[test]
    fn with_sub_lexer_success_and_failure() {
        let mut lexer =
            DsnLexer::<T>::new("(stroke (width 0.1)) (layer) (stroke (width x)) (at)", "s");

        lexer.need_left().unwrap();
        assert_eq!(lexer.next_tok().unwrap(), T_stroke);

        let parse_width = |sub: &mut DsnLexer<strokeparams_t::T>| -> Result<f64, IoError> {
            sub.need_left()?;

            if sub.next_tok()? != strokeparams_t::T::T_width {
                return Err(sub.expecting(strokeparams_t::T::T_width));
            }

            let width = sub.parse_double_expected("stroke width")?;
            sub.need_right()?;
            sub.need_right()?;
            Ok(width)
        };

        // Success: the lexer continues behind the tokens the sub-lexer consumed.
        assert_eq!(lexer.with_sub_lexer(parse_width).unwrap(), 0.1);
        lexer.need_left().unwrap();
        assert_eq!(lexer.next_tok().unwrap(), T_layer);
        lexer.need_right().unwrap();
        lexer.need_left().unwrap();
        assert_eq!(lexer.next_tok().unwrap(), T_stroke);

        // Failure: the error comes from the sub-lexer, the reader is back, and the position is
        // the one from before the hand-over (as after a C++ exception).
        let err = lexer.with_sub_lexer(parse_width).unwrap_err();
        assert_eq!(
            err.problem(),
            "need a number for 'stroke width' in 's', line 1, offset 45."
        );
        assert_eq!(lexer.cur_source(), "s");
        assert_eq!(lexer.cur_tok(), T_stroke);
        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!(lexer.next_tok().unwrap(), T_width);
    }

    /// A reader that fails at a given line, like the `CANCEL_AT_LINE_READER` of the C++ tests.
    struct FailingReader {
        inner: StringLineReader,
        fail_at: u32,
    }

    impl LineReader for FailingReader {
        fn read_line(&mut self) -> Result<Option<&str>, IoError> {
            if self.inner.line_number() + 1 >= self.fail_at {
                return Err(IoError::cancelled());
            }

            self.inner.read_line()
        }

        fn source(&self) -> &str {
            self.inner.source()
        }

        fn line(&self) -> &str {
            self.inner.line()
        }

        fn line_number(&self) -> u32 {
            self.inner.line_number()
        }

        fn length(&self) -> u32 {
            self.inner.length()
        }
    }

    #[test]
    fn reader_errors_pass_through() {
        let reader = FailingReader {
            inner: StringLineReader::new("(at\n1\n2)", "r"),
            fail_at: 3,
        };
        let mut lexer = DsnLexer::<T>::from_line_reader(Some(Box::new(reader)));

        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!(lexer.next_tok().unwrap(), T_at);
        assert_eq!(lexer.next_tok().unwrap(), T_NUMBER);

        let err = lexer.next_tok().unwrap_err();
        assert!(err.is_cancelled());
        assert_eq!(err.problem(), "Canceled by user.");

        // prevTok was already shifted when the reader failed.
        assert_eq!(lexer.cur_tok(), T_NUMBER);
        assert_eq!(lexer.prev_tok(), T_NUMBER);

        let reader = lexer.pop_reader().expect("the reader");
        assert_eq!(reader.line_number(), 2);
    }

    #[test]
    fn reader_stack() {
        let mut lexer = DsnLexer::<T>::new("(at\n layer)", "outer");

        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!(lexer.next_tok().unwrap(), T_at);

        lexer.push_reader(Box::new(StringLineReader::new("width", "inner")));
        assert_eq!(lexer.next_tok().unwrap(), T_width);
        assert_eq!(lexer.cur_source(), "inner");
        assert_eq!(lexer.cur_line_number(), 1);

        assert_eq!(lexer.pop_reader().expect("inner reader").source(), "inner");
        assert_eq!(lexer.cur_source(), "outer");
        assert_eq!(lexer.next_tok().unwrap(), T_layer);
        assert_eq!(lexer.cur_line_number(), 2);
    }

    #[test]
    fn init_parser_state() {
        let mut lexer = DsnLexer::<T>::new("(at)", "s");

        lexer.set_comments_are_tokens(true);
        lexer.next_tok().unwrap();
        lexer.next_tok().unwrap();
        lexer.init_parser_state();

        assert_eq!(lexer.cur_tok(), T_NONE);
        assert_eq!(lexer.prev_tok(), T_NONE);
        assert_eq!(lexer.cur_offset(), 1);
        assert!(!lexer.set_comments_are_tokens(false));
        // the position in the input is kept
        assert_eq!(lexer.next_tok().unwrap(), T_RIGHT);
    }

    #[test]
    fn specctra_mode_setters() {
        let mut lexer = DsnLexer::<T>::new("('a b' x)", "s");

        // No effect outside of specctra mode.
        assert_eq!(lexer.set_string_delimiter(b'\''), b'"');
        assert_eq!(lexer.set_string_delimiter(b'$'), b'"');
        assert!(!lexer.set_space_in_quoted_tokens(true));
        assert!(!lexer.set_space_in_quoted_tokens(true));

        lexer.set_specctra_mode(true);
        assert!(lexer.set_space_in_quoted_tokens(true));
        assert_eq!(lexer.set_string_delimiter(b'\''), b'"');
        assert_eq!(lexer.set_string_delimiter(b'\''), b'\'');

        assert_eq!(lexer.next_tok().unwrap(), T_LEFT);
        assert_eq!(lexer.next_tok().unwrap(), T_STRING);
        assert_eq!(lexer.cur_text(), "a b");

        // Leaving specctra mode restores the defaults.
        lexer.set_specctra_mode(false);
        assert_eq!(lexer.set_string_delimiter(b'$'), b'"');
    }

    #[test]
    fn debug_output() {
        let mut lexer = DsnLexer::<T>::new("(at)", "s");
        lexer.next_tok().unwrap();

        let text = format!("{lexer:?}");
        assert!(text.contains("cur_tok: T_LEFT"), "{text}");
        assert!(text.contains("source: \"s\""), "{text}");
    }
}
