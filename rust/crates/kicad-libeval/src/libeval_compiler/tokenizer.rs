// This file is part of libeval, a simple math expression evaluator
//
// Copyright (C) 2007 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `LIBEVAL::TOKENIZER` from `include/libeval_compiler/libeval_compiler.h` and
//! `common/libeval_compiler/libeval_compiler.cpp`.

/// Port of `class LIBEVAL::TOKENIZER`: a cursor over the expression text.
///
/// Positions count characters (Unicode scalar values), which is what `wxString` indexing
/// gives on platforms with a 32 bit `wchar_t`; all source offsets reported by the compiler
/// are therefore character offsets, not UTF-8 byte offsets.
#[derive(Clone, Debug, Default)]
pub struct Tokenizer {
    str: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    /// Port of `TOKENIZER::Restart`.
    pub fn restart(&mut self, s: &str) {
        self.str = s.chars().collect();
        self.pos = 0;
    }

    /// Port of `TOKENIZER::Clear`.
    pub fn clear(&mut self) {
        self.str.clear();
        self.pos = 0;
    }

    /// Port of `TOKENIZER::GetChar`: the current character, `'\0'` at the end of the input.
    pub fn get_char(&self) -> char {
        if self.pos >= self.str.len() {
            return '\0';
        }

        self.str[self.pos]
    }

    /// Port of `TOKENIZER::Done`.
    pub fn done(&self) -> bool {
        self.pos >= self.str.len()
    }

    /// Port of `TOKENIZER::NextChar( int aAdvance = 1 )`.
    pub fn next_char(&mut self, advance: usize) {
        self.pos += advance;
    }

    /// Port of `TOKENIZER::GetPos`.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Port of `TOKENIZER::GetString`: read up to the closing quote of a string literal.
    ///
    /// The only escape sequence is `\'` (a quote); any other backslash is kept as is.  A
    /// missing closing quote ends the string at the end of the input; the position is moved
    /// past the (possibly missing) quote in both cases.
    pub fn get_string(&mut self) -> String {
        let mut rv = String::new();

        while self.pos < self.str.len() && self.str[self.pos] != '\'' {
            if self.str[self.pos] == '\\'
                && self.pos + 1 < self.str.len()
                && self.str[self.pos + 1] == '\''
            {
                self.pos += 1;
            }

            rv.push(self.str[self.pos]);
            self.pos += 1;
        }

        self.pos += 1;

        rv
    }

    /// Port of `TOKENIZER::GetChars`: the run of characters from the current position for
    /// which `cond` holds (the position is not changed).
    pub fn get_chars(&self, cond: impl Fn(char) -> bool) -> String {
        let mut rv = String::new();
        let mut p = self.pos;

        while p < self.str.len() && cond(self.str[p]) {
            rv.push(self.str[p]);
            p += 1;
        }

        rv
    }

    /// Port of `TOKENIZER::MatchAhead`: does the input continue with `match_`, followed by
    /// either the end of the input or a character for which `stop_cond` holds?
    pub fn match_ahead(&self, match_: &str, stop_cond: impl Fn(char) -> bool) -> bool {
        let match_chars: Vec<char> = match_.chars().collect();
        let remaining = self.str.len() as i64 - self.pos as i64;

        if remaining < match_chars.len() as i64 {
            return false;
        }

        if self.str[self.pos..self.pos + match_chars.len()] == match_chars[..] {
            return remaining == match_chars.len() as i64
                || stop_cond(self.str[self.pos + match_chars.len()]);
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_escapes() {
        let mut t = Tokenizer::default();
        t.restart("it\\'s' rest");
        assert_eq!(t.get_string(), "it's");
        assert_eq!(t.pos(), 6);

        t.restart("a\\b'");
        assert_eq!(t.get_string(), "a\\b");

        t.restart("unterminated");
        assert_eq!(t.get_string(), "unterminated");
        assert_eq!(t.pos(), 13);
        assert!(t.done());
        assert_eq!(t.get_char(), '\0');
    }

    #[test]
    fn match_ahead() {
        let mut t = Tokenizer::default();
        t.restart("== 1");
        assert!(t.match_ahead("==", |c| c != '='));
        t.restart("===");
        assert!(!t.match_ahead("==", |c| c != '='));
        t.restart("==");
        assert!(t.match_ahead("==", |c| c != '='));
        t.restart("=");
        assert!(!t.match_ahead("==", |c| c != '='));
        t.restart("mm");
        t.next_char(5);
        assert!(!t.match_ahead("mm", |_| true));
        assert!(!t.match_ahead("", |_| true));
    }
}
