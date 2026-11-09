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

//! Port of the non-GUI parts of `include/string_utils.h` and `common/string_utils.cpp`.
//!
//! `wxString` becomes `&str` / `String`.  On the reference platform a `wxString` is a
//! sequence of 32 bit code points, so every C++ index is a `char` index here (see
//! `src/wxstr.rs`).  `TO_UTF8` and `From_UTF8` are the identity on Rust strings;
//! [`from_utf8_bytes`] ports the byte-level fallback of `From_UTF8`.
//!
//! Left out (see `PORT_STATUS.md`): `SplitGraphemeClusters` (needs the Unicode tables of
//! harfbuzz), `LinkifyHTML` / `IsURL` (`wxRegEx` with `\w`, `\b`, `\s`), `GetLine` (`FILE*`),
//! `GetISO8601CurrentDateTime` (clock), `NormalizeFileUri` / `ConvertPathToFileUri` (file
//! system), `ConvertMarkdown2Html`, `ExtractLibraryLoadException`.

use crate::printf::{format_f, format_g};
use crate::wxstr;

/// File name characters that are illegal on at least one platform (the Windows list, which
/// includes the Unix '/').  Port of the file-static `illegalFileNameChars`.
const ILLEGAL_FILE_NAME_CHARS: &str = "\\/:\"<>|*?";

/// Port of the file-static `defaultVariantName`.
const DEFAULT_VARIANT_NAME: &str = "< Default >";

/// `wxFileName::GetForbiddenChars()` on Unix (the reference platform).
const WX_FORBIDDEN_CHARS_UNIX: &str = "*?";

/// Port of `IsFullFileNameValid`: false if the (full) file name contains characters the
/// platform forbids (`wxFileName::GetForbiddenChars()`: `*` and `?` on Linux).
pub fn is_full_file_name_valid(full_filename: &str) -> bool {
    !full_filename
        .chars()
        .any(|c| WX_FORBIDDEN_CHARS_UNIX.contains(c))
}

/// Port of `ConvertToNewOverbarNotation`: convert the old `~overbar~` notation to the new
/// `~{overbar}` one.
pub fn convert_to_new_overbar_notation(old_str: &str) -> String {
    let mut new_str = String::with_capacity(old_str.len());
    let mut in_overbar = false;

    // Don't get tripped up by the legacy empty-string token.
    if old_str == "~" {
        return old_str.to_string();
    }

    let chars: Vec<char> = old_str.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '~' {
            let lookahead = i + 1;

            if lookahead < chars.len() && chars[lookahead] == '~' {
                if lookahead + 1 < chars.len() && chars[lookahead + 1] == '{' {
                    // This way the subsequent opening curly brace will not start an
                    // overbar.
                    new_str.push_str("~~{}");
                    i += 1;
                    continue;
                }

                // Two subsequent tildes mean a tilde.
                new_str.push('~');
                i += 2;
                continue;
            } else if lookahead < chars.len() && chars[lookahead] == '{' {
                // Could mean the user wants "{" with an overbar, but more likely this
                // is a case of double notation conversion.  Bail out.
                return old_str.to_string();
            } else {
                if in_overbar {
                    new_str.push('}');
                    in_overbar = false;
                } else {
                    new_str.push_str("~{");
                    in_overbar = true;
                }

                i += 1;
                continue;
            }
        } else if (ch == ' ' || ch == '}' || ch == ')') && in_overbar {
            // Spaces were used to terminate overbar as well
            new_str.push('}');
            in_overbar = false;
        }

        new_str.push(ch);
        i += 1;
    }

    // Explicitly end the overbar even if there was no terminating '~' in the aOldStr.
    if in_overbar {
        new_str.push('}');
    }

    new_str
}

/// Port of `ConvertSmartQuotesAndDashes`: replace typographic quotes and dashes by their
/// ASCII equivalents; true if anything changed.
pub fn convert_smart_quotes_and_dashes(string: &mut String) -> bool {
    let mut ret_val = false;
    let converted: String = string
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => {
                ret_val = true;
                '\''
            }
            '\u{201C}' | '\u{201D}' => {
                ret_val = true;
                '"'
            }
            '\u{2013}' | '\u{2014}' => {
                ret_val = true;
                '-'
            }
            _ => c,
        })
        .collect();

    *string = converted;
    ret_val
}

/// Port of `enum ESCAPE_CONTEXT`: where an escaped string is going to be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EscapeContext {
    /// `CTX_NETNAME`
    Netname = 0,
    /// `CTX_LIBID`
    Libid,
    /// `CTX_LEGACY_LIBID`
    LegacyLibid,
    /// `CTX_IPC`
    Ipc,
    /// `CTX_QUOTED_STR`
    QuotedStr,
    /// `CTX_JS_STR`
    JsStr,
    /// `CTX_LINE`
    Line,
    /// `CTX_CSV`
    Csv,
    /// `CTX_FILENAME`
    Filename,
    /// `CTX_NO_SPACE`: to replace spaces in names that do not accept spaces
    NoSpace,
    /// `CTX_REGEX`
    Regex,
}

impl EscapeContext {
    /// All contexts, in enum order.
    pub const ALL: [EscapeContext; 11] = [
        EscapeContext::Netname,
        EscapeContext::Libid,
        EscapeContext::LegacyLibid,
        EscapeContext::Ipc,
        EscapeContext::QuotedStr,
        EscapeContext::JsStr,
        EscapeContext::Line,
        EscapeContext::Csv,
        EscapeContext::Filename,
        EscapeContext::NoSpace,
        EscapeContext::Regex,
    ];
}

/// Port of `EscapeString`: encode reserved characters of the context with the
/// HTML-entity-like `{token}` notation (`/` -> `{slash}`, ...).  [`unescape_string`] reverses
/// it.
pub fn escape_string(source: &str, context: EscapeContext) -> String {
    use EscapeContext as C;

    let mut converted = String::with_capacity(source.len());

    for c in source.chars() {
        match context {
            C::Netname => match c {
                '/' => converted.push_str("{slash}"),
                '\n' | '\r' => {} // drop
                _ => converted.push(c),
            },
            C::Libid | C::LegacyLibid => match c {
                // We no longer escape '/' in LIB_IDs, but we used to
                '/' if context == C::LegacyLibid => converted.push_str("{slash}"),
                '\\' => converted.push_str("{backslash}"),
                '<' => converted.push_str("{lt}"),
                '>' => converted.push_str("{gt}"),
                ':' => converted.push_str("{colon}"),
                '"' => converted.push_str("{dblquote}"),
                '\n' | '\r' => {} // drop
                _ => converted.push(c),
            },
            C::Ipc => match c {
                '/' => converted.push_str("{slash}"),
                ',' => converted.push_str("{comma}"),
                '"' => converted.push_str("{dblquote}"),
                _ => converted.push(c),
            },
            C::QuotedStr => match c {
                '"' => converted.push_str("{dblquote}"),
                _ => converted.push(c),
            },
            C::JsStr => {
                if c as u32 >= 0x7F || matches!(c, '\'' | '"' | '\\' | '(' | ')') {
                    // snprintf( buffer, sizeof(buffer), "\\u%4.4X", code )
                    converted.push_str(&format!("\\u{:04X}", c as u32));
                } else {
                    converted.push(c);
                }
            }
            C::Line => match c {
                '\n' | '\r' => converted.push_str("{return}"),
                _ => converted.push(c),
            },
            C::Filename => match c {
                '/' => converted.push_str("{slash}"),
                '\\' => converted.push_str("{backslash}"),
                '"' => converted.push_str("{dblquote}"),
                '<' => converted.push_str("{lt}"),
                '>' => converted.push_str("{gt}"),
                '|' => converted.push_str("{bar}"),
                ':' => converted.push_str("{colon}"),
                '\t' => converted.push_str("{tab}"),
                '\n' | '\r' => converted.push_str("{return}"),
                _ => converted.push(c),
            },
            C::NoSpace => match c {
                ' ' => converted.push_str("{space}"),
                _ => converted.push(c),
            },
            C::Csv => match c {
                ',' => converted.push_str("{comma}"),
                '\n' | '\r' => converted.push_str("{return}"),
                _ => converted.push(c),
            },
            C::Regex => {
                const REG_EX_CTRL_CHARS: &str = ".^$*+?{}()[]~|\\";

                if REG_EX_CTRL_CHARS.contains(c) {
                    converted.push('\\');
                }

                converted.push(c);
            }
        }
    }

    converted
}

/// Port of `UnescapeString`: decode the `{token}` escapes of [`escape_string`]; unknown
/// tokens and `${..}`, `~{..}`, `^{..}`, `_{..}` groups are kept (with their content
/// unescaped recursively).
pub fn unescape_string(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();

    unescape_chars(&chars)
}

fn unescape_chars(source: &[char]) -> String {
    let source_len = source.len();

    // smallest escape string is three characters, shortcut everything else
    if source_len <= 2 {
        return source.iter().collect();
    }

    let mut newbuf = String::with_capacity(source_len);
    let mut ch = '\0';
    let mut i = 0;

    while i < source_len {
        let prev = ch;
        ch = source[i];

        if ch == '{' {
            let mut token: Vec<char> = Vec::new();
            let mut depth = 1;
            let mut terminated = false;

            i += 1;

            while i < source_len {
                ch = source[i];

                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                }

                if depth <= 0 {
                    terminated = true;
                    break;
                } else {
                    token.push(ch);
                }

                i += 1;
            }

            let token_str: String = token.iter().collect();

            if !terminated {
                newbuf.push('{');
                newbuf.push_str(&unescape_chars(&token));
            } else if prev == '$' || prev == '~' || prev == '^' || prev == '_' {
                newbuf.push('{');
                newbuf.push_str(&unescape_chars(&token));
                newbuf.push('}');
            } else {
                match token_str.as_str() {
                    "dblquote" => newbuf.push('"'),
                    "quote" => newbuf.push('\''),
                    "lt" => newbuf.push('<'),
                    "gt" => newbuf.push('>'),
                    "backslash" => newbuf.push('\\'),
                    "slash" => newbuf.push('/'),
                    "bar" => newbuf.push('|'),
                    "comma" => newbuf.push(','),
                    "colon" => newbuf.push(':'),
                    "space" => newbuf.push(' '),
                    "dollar" => newbuf.push('$'),
                    "tab" => newbuf.push('\t'),
                    "return" => newbuf.push('\n'),
                    "brace" => newbuf.push('{'),
                    _ => {
                        newbuf.push('{');
                        newbuf.push_str(&unescape_chars(&token));
                        newbuf.push('}');
                    }
                }
            }
        } else {
            newbuf.push(ch);
        }

        i += 1;
    }

    newbuf
}

/// Port of `wxStringSplit`: split at every `splitter`; empty pieces are kept except a
/// trailing one.
pub fn wx_string_split(text: &str, splitter: char) -> Vec<String> {
    let mut strings = Vec::new();
    let mut tmp = String::new();

    for c in text.chars() {
        if c == splitter {
            strings.push(std::mem::take(&mut tmp));
        } else {
            tmp.push(c);
        }
    }

    if !tmp.is_empty() {
        strings.push(tmp);
    }

    strings
}

/// Port of `TitleCaps`: capitalise every space separated word (`wxString::Capitalize`).
pub fn title_caps(string: &str) -> String {
    let mut result = String::with_capacity(string.len());

    for word in wx_string_split(string, ' ') {
        if !result.is_empty() {
            result.push(' ');
        }

        result.push_str(&wxstr::capitalize(&word));
    }

    result
}

/// Port of `InitialCaps`: capitalise the first word, lower-case the others.
pub fn initial_caps(string: &str) -> String {
    let mut result = String::with_capacity(string.len());

    for word in wx_string_split(string, ' ') {
        if result.is_empty() {
            result.push_str(&wxstr::capitalize(&word));
        } else {
            result.push(' ');
            result.push_str(&wxstr::lower(&word));
        }
    }

    result
}

/// Reads byte `i` of a C string held in a slice; the end of the slice acts as the NUL.
fn c_byte(source: &[u8], i: usize) -> u8 {
    source.get(i).copied().unwrap_or(0)
}

/// Port of `ReadDelimitedText( wxString* aDest, const char* aSource )`: copy the text between
/// the first two unescaped double quotes, removing the backslash of `\"` and `\\`.
///
/// `source` is the C string (it ends at the first NUL byte or at the end of the slice).
/// Returns the text (converted with [`from_utf8_bytes`]) and the number of bytes consumed,
/// which like the C++ count includes the terminating NUL when the end was reached.
pub fn read_delimited_text(source: &[u8]) -> (String, usize) {
    let mut utf8: Vec<u8> = Vec::new(); // utf8 but without escapes and quotes.
    let mut inside = false;
    let mut pos = 0;

    loop {
        let mut cc = c_byte(source, pos);
        pos += 1;

        if cc == 0 {
            break;
        }

        if cc == b'"' {
            if inside {
                break; // 2nd double quote is end of delimited text
            }

            inside = true; // first delimiter found, make note, do not copy
        } else if inside {
            if cc == b'\\' {
                cc = c_byte(source, pos);
                pos += 1;

                if cc == 0 {
                    break;
                }

                // do no copy the escape byte if it is followed by \ or "
                if cc != b'"' && cc != b'\\' {
                    utf8.push(b'\\');
                }

                utf8.push(cc);
            } else {
                utf8.push(cc);
            }
        }
    }

    (from_utf8_bytes(&utf8), pos)
}

/// Port of `ReadDelimitedText( char* aDest, const char* aSource, int aDestSize )`: like
/// [`read_delimited_text`] into a buffer of `dest_size` bytes (including the NUL, which is
/// not part of the returned bytes).  Returns the bytes and the number of source bytes
/// consumed.
pub fn read_delimited_text_buf(source: &[u8], dest_size: i32) -> (Vec<u8>, usize) {
    if dest_size <= 0 {
        return (Vec::new(), 0);
    }

    let limit = (dest_size - 1) as usize;
    let mut dest: Vec<u8> = Vec::new();
    let mut inside = false;
    let mut pos = 0;

    loop {
        let mut cc = c_byte(source, pos);
        pos += 1;

        if !(cc != 0 && dest.len() < limit) {
            break;
        }

        if cc == b'"' {
            if inside {
                break; // 2nd double quote is end of delimited text
            }

            inside = true; // first delimiter found, make note, do not copy
        } else if inside {
            if cc == b'\\' {
                cc = c_byte(source, pos);
                pos += 1;

                if cc == 0 {
                    break;
                }

                // do no copy the escape byte if it is followed by \ or "
                if cc != b'"' && cc != b'\\' {
                    dest.push(b'\\');
                }

                if dest.len() < limit {
                    dest.push(cc);
                }
            } else {
                dest.push(cc);
            }
        }
    }

    (dest, pos)
}

/// Port of `EscapedUTF8`: the UTF-8 text in double quotes, with `"` and `\` backslash
/// escaped (compatible with [`read_delimited_text`]).
///
/// The C++ prepares a copy with new lines replaced by `\r` but then encodes the original
/// string; the port does the same (new lines are kept).
pub fn escaped_utf8(string: &str) -> String {
    let mut ret = String::with_capacity(string.len() + 2);

    ret.push('"');

    for c in string.chars() {
        // this escaping strategy is designed to be compatible with ReadDelimitedText():
        match c {
            '"' => ret.push_str("\\\""),
            '\\' => ret.push_str("\\\\"), // double it up
            _ => ret.push(c),
        }
    }

    ret.push('"');
    ret
}

/// Port of `EscapeHTML`.
pub fn escape_html(string: &str) -> String {
    let mut converted = String::with_capacity(string.len());

    for c in string.chars() {
        match c {
            '"' => converted.push_str("&quot;"),
            '\'' => converted.push_str("&apos;"),
            '&' => converted.push_str("&amp;"),
            '<' => converted.push_str("&lt;"),
            '>' => converted.push_str("&gt;"),
            _ => converted.push(c),
        }
    }

    converted
}

/// `wxString::ToCULong( &val, base )` on a string of ASCII alphanumerics: the value C++
/// stores (0 when it leaves the output untouched: nothing parsed or overflow).
fn to_c_ulong(digits: &[char], base: u32) -> u64 {
    let mut value: u64 = 0;
    let mut any = false;
    let mut digits = digits;

    // strtoul accepts an optional "0x" prefix in base 16 when a hex digit follows it
    if base == 16
        && digits.len() > 2
        && digits[0] == '0'
        && (digits[1] == 'x' || digits[1] == 'X')
        && digits[2].is_ascii_hexdigit()
    {
        digits = &digits[2..];
    }

    for &c in digits {
        let Some(d) = c.to_digit(base) else { break };

        any = true;

        match value
            .checked_mul(base as u64)
            .and_then(|v| v.checked_add(d as u64))
        {
            Some(v) => value = v,
            None => return 0, // ERANGE: output not modified
        }
    }

    if any { value } else { 0 }
}

/// Port of `UnescapeHTML`: decode `&#NNN;`, `&#xHHHH;` and the named entities `quot`,
/// `apos`, `amp`, `lt`, `gt`.
///
/// The C++ uses the regular expression `&(#(\d*)|#x([a-zA-Z0-9]{4})|amp|apos|gt|lt|quot);`;
/// the port matches the same language by hand.  A numeric reference to a value that is not
/// a Unicode scalar value (a surrogate or a value above 0x10FFFF, which a 32 bit `wxString`
/// can hold) becomes U+FFFD.
pub fn unescape_html(string: &str) -> String {
    let chars: Vec<char> = string.chars().collect();
    let mut result = String::with_capacity(string.len());
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '&' {
            if let Some((len, replacement)) = match_html_entity(&chars[i + 1..]) {
                if let Some(r) = replacement {
                    result.push(r);
                }

                i += 1 + len;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Matches the part of the `UnescapeHTML` regex after `&`.  Returns the matched length and
/// the replacement (none for a numeric reference to 0).
fn match_html_entity(rest: &[char]) -> Option<(usize, Option<char>)> {
    let code_point = |v: u64| -> Option<char> {
        if v == 0 {
            None
        } else {
            Some(
                u32::try_from(v)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\u{FFFD}'),
            )
        }
    };

    if rest.first() == Some(&'#') {
        // #(\d*);
        let digits = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();

        if rest.get(1 + digits) == Some(&';') {
            return Some((digits + 2, code_point(to_c_ulong(&rest[1..1 + digits], 10))));
        }

        // #x([a-zA-Z0-9]{4});
        if rest.get(1) == Some(&'x')
            && rest.len() >= 7
            && rest[2..6].iter().all(|c| c.is_ascii_alphanumeric())
            && rest[6] == ';'
        {
            return Some((7, code_point(to_c_ulong(&rest[2..6], 16))));
        }

        return None;
    }

    // std::map order: amp, apos, gt, lt, quot
    for (key, value) in [
        ("amp", '&'),
        ("apos", '\''),
        ("gt", '>'),
        ("lt", '<'),
        ("quot", '"'),
    ] {
        let n = key.len();

        if rest.len() > n && rest[..n].iter().copied().eq(key.chars()) && rest[n] == ';' {
            return Some((n + 1, Some(value)));
        }
    }

    None
}

/// Port of `RemoveHTMLTags`: remove every `<...>` (regex `<[^>]*>`).
pub fn remove_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(open) = rest.find('<') {
        match rest[open..].find('>') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }

    out.push_str(rest);
    out
}

/// Port of `NoPrintableChars`: true if the string is empty after trimming white space.
pub fn no_printable_chars(string: &str) -> bool {
    wxstr::strip_both(string).is_empty()
}

/// Port of `PrintableCharCount`: the number of characters drawn, i.e. without tabs and the
/// markup of `^{..}`, `_{..}` and `~{..}`.
pub fn printable_char_count(string: &str) -> i32 {
    let chars: Vec<char> = string.chars().collect();
    let mut char_count = 0;
    let mut overbar_depth = -1;
    let mut super_sub_depth = -1;
    let mut brace_nesting = 0;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next_is_brace = chars.get(i + 1) == Some(&'{');

        if c == '\t' {
            // We don't format tabs in bitmap text (where this is currently used), so just
            // drop them from the count.
            i += 1;
            continue;
        } else if (c == '^' || c == '_') && super_sub_depth == -1 {
            if next_is_brace {
                i += 2;
                super_sub_depth = brace_nesting;
                brace_nesting += 1;
                continue;
            }
        } else if c == '~' && overbar_depth == -1 {
            if next_is_brace {
                i += 2;
                overbar_depth = brace_nesting;
                brace_nesting += 1;
                continue;
            }
        } else if c == '{' {
            brace_nesting += 1;
        } else if c == '}' {
            if brace_nesting > 0 {
                brace_nesting -= 1;
            }

            if brace_nesting == super_sub_depth {
                super_sub_depth = -1;
                i += 1;
                continue;
            }

            if brace_nesting == overbar_depth {
                overbar_depth = -1;
                i += 1;
                continue;
            }
        }

        char_count += 1;
        i += 1;
    }

    char_count
}

/// Port of `StrPurge`: the text without leading and trailing `" \t\n\r\f\v"`.
pub fn str_purge(text: &str) -> &str {
    const WHITESPACE: &[char] = &[' ', '\t', '\n', '\r', '\x0c', '\x0b'];

    text.trim_matches(WHITESPACE)
}

/// Port of `enum class CMP_T`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CmpT {
    /// `CASE_SENSITIVE`
    #[default]
    CaseSensitive,
    /// `IGNORE_CASE`
    IgnoreCase,
}

/// Port of `StrNumCmp`: compare strings treating runs of digits as numbers ("U10" > "U9").
/// Returns -1, 0 or 1.
///
/// Digit runs longer than `int` can hold overflow in C++ (undefined behaviour, wrapping in
/// practice); the port wraps.
pub fn str_num_cmp(string1: &str, string2: &str, case_sensitivity: CmpT) -> i32 {
    let s1: Vec<char> = string1.chars().collect();
    let s2: Vec<char> = string2.chars().collect();
    let (mut i1, mut i2) = (0usize, 0usize);

    while i1 < s1.len() && i2 < s2.len() {
        let mut c1 = s1[i1];
        let mut c2 = s2[i2];

        if wxstr::is_digit(c1) && wxstr::is_digit(c2) {
            // Both characters are digits, do numeric compare.
            let mut nb1: i32 = 0;
            let mut nb2: i32 = 0;

            loop {
                c1 = s1[i1];
                nb1 = nb1
                    .wrapping_mul(10)
                    .wrapping_add(c1 as i32)
                    .wrapping_sub('0' as i32);
                i1 += 1;

                if !(i1 < s1.len() && wxstr::is_digit(s1[i1])) {
                    break;
                }
            }

            loop {
                c2 = s2[i2];
                nb2 = nb2
                    .wrapping_mul(10)
                    .wrapping_add(c2 as i32)
                    .wrapping_sub('0' as i32);
                i2 += 1;

                if !(i2 < s2.len() && wxstr::is_digit(s2[i2])) {
                    break;
                }
            }

            if nb1 < nb2 {
                return -1;
            }

            if nb1 > nb2 {
                return 1;
            }

            c1 = if i1 < s1.len() { s1[i1] } else { '\0' };
            c2 = if i2 < s2.len() { s2[i2] } else { '\0' };
        }

        // Any numerical comparisons to here are identical.
        if case_sensitivity == CmpT::IgnoreCase {
            if c1 != c2 {
                let uc1 = wxstr::to_upper(c1);
                let uc2 = wxstr::to_upper(c2);

                if uc1 != uc2 {
                    return if uc1 < uc2 { -1 } else { 1 };
                }
            }
        } else {
            if c1 < c2 {
                return -1;
            }

            if c1 > c2 {
                return 1;
            }
        }

        if i1 < s1.len() {
            i1 += 1;
        }

        if i2 < s2.len() {
            i2 += 1;
        }
    }

    if i1 == s1.len() && i2 != s2.len() {
        -1 // Identical to here but aString1 is longer.
    } else if i1 != s1.len() && i2 == s2.len() {
        1 // Identical to here but aString2 is longer.
    } else {
        0
    }
}

/// Port of `StrNumSort`: sort with [`str_num_cmp`] (`std::sort` in C++; the comparator is a
/// strict weak order on distinct strings only up to case, so the stable sort used here can
/// order case-insensitively equal strings differently than libstdc++ would).
pub fn str_num_sort(list: &mut [String], case_sensitivity: CmpT) {
    list.sort_by(|a, b| str_num_cmp(a, b, case_sensitivity).cmp(&0));
}

/// The characters of a `wxString` as seen through `GetData()`: up to the first NUL, followed
/// by a NUL sentinel.
fn c_chars(s: &str) -> Vec<char> {
    let mut v: Vec<char> = s.chars().take_while(|&c| c != '\0').collect();
    v.push('\0');
    v
}

/// Port of `WildCompareString`: match `string_to_tst` against `pattern` with the wildcards
/// `*` and `?`.
pub fn wild_compare_string(pattern: &str, string_to_tst: &str, case_sensitive: bool) -> bool {
    let (wild, s) = if case_sensitive {
        (c_chars(pattern), c_chars(string_to_tst))
    } else {
        (
            c_chars(&wxstr::upper(pattern)),
            c_chars(&wxstr::upper(string_to_tst)),
        )
    };

    let (mut w, mut st) = (0usize, 0usize);
    let (mut mp, mut cp) = (0usize, 0usize);

    while s[st] != '\0' && wild[w] != '*' {
        if wild[w] != s[st] && wild[w] != '?' {
            return false;
        }

        w += 1;
        st += 1;
    }

    while s[st] != '\0' {
        if wild[w] == '*' {
            w += 1;

            if wild[w] == '\0' {
                return true;
            }

            mp = w;
            cp = st + 1;
        } else if wild[w] == s[st] || wild[w] == '?' {
            w += 1;
            st += 1;
        } else {
            w = mp;
            st = cp;
            cp += 1;
        }
    }

    while wild[w] == '*' {
        w += 1;
    }

    wild[w] == '\0'
}

/// Port of `NUMERIC_EVALUATOR::IsOldSchoolDecimalSeparator( wxUniChar, double* )`
/// (`common/libeval/numeric_evaluator.cpp`): the SI scale of an RKM style infix ("4K7").
pub fn is_old_school_decimal_separator(ch: char) -> Option<f64> {
    Some(match ch {
        'p' => 1.0e-12,
        'n' => 1.0e-9,
        '\u{00B5}' => 1.0e-6,
        '\u{03BC}' => 1.0e-6,
        'u' => 1.0e-6,
        'm' => 1.0e-3,
        'L' => 1.0e-3,
        'R' => 1.0,
        'F' => 1.0,
        'k' => 1.0e3,
        'K' => 1.0e3,
        'M' => 1.0e6,
        'G' => 1.0e9,
        'T' => 1.0e12,
        _ => return None,
    })
}

/// Port of `ApplyModifier` (`string_utils.cpp`, not declared in the header): scale `value`
/// by the SI / IEC 60062 multiplier the string starts with, accepting an optional unit.
/// False if the string is empty or the unit is not one of F, Hz, Ω, W, V, A, H, s, m.
pub fn apply_modifier(value: &mut f64, string: &str) -> bool {
    // Although the two 'μ's look the same, they are U+03BC and U+00B5
    const MODIFIERS: &str = "afpnu\u{00B5}\u{03BC}mLRFkKMGTPE";

    let Some(first) = string.chars().next() else {
        return false;
    };

    // wxString::Strip() strips trailing white space only
    let stripped = wxstr::trim_right(string);

    if stripped == "m" {
        // a single m is meters, not milli-undefineds
        return true;
    } else if stripped == "mils" {
        *value *= 0.0000254;
        return true;
    } else if stripped == "in" {
        *value *= 0.0254;
        return true;
    }

    let (modifier, units) = if MODIFIERS.contains(first) {
        (first, wxstr::trim_right(wxstr::mid(string, 1)))
    } else {
        (' ', wxstr::trim_right(string))
    };

    if !units.is_empty()
        && !["F", "hz", "\u{03A9}", "W", "V", "A", "H", "s", "m"]
            .iter()
            .any(|u| wxstr::is_same_as_no_case(units, u))
    {
        return false;
    }

    // Note: most of these are SI, but some (L, R, F) are IEC 60062.
    if modifier == 'a' {
        *value *= 1.0e-18;
    } else if modifier == 'f' {
        *value *= 1.0e-15;
    }

    if modifier == 'p' {
        *value *= 1.0e-12;
    }

    if modifier == 'n' {
        *value *= 1.0e-9;
    }
    // Although the two 'μ's look the same, they are U+03BC and U+00B5
    else if modifier == 'u' || modifier == '\u{00B5}' || modifier == '\u{03BC}' {
        *value *= 1.0e-6;
    } else if modifier == 'm' || modifier == 'L' {
        *value *= 1.0e-3;
    } else if modifier == 'R' || modifier == 'F' {
        // unity scalar
    } else if modifier == 'c' {
        // primarily for cm
        *value *= 1.0e2;
    } else if modifier == 'k' || modifier == 'K' {
        *value *= 1.0e3;
    } else if modifier == 'M' {
        *value *= 1.0e6;
    } else if modifier == 'G' {
        *value *= 1.0e9;
    } else if modifier == 'T' {
        *value *= 1.0e12;
    } else if modifier == 'P' {
        *value *= 1.0e15;
    } else if modifier == 'E' {
        *value *= 1.0e18;
    }

    true
}

/// Port of `convertSeparators` (`string_utils.cpp`): work out which of `.` and `,` is the
/// decimal and which the thousands separator and convert the number to C locale notation.
/// False if the separators are inconsistent.
pub fn convert_separators(value: &mut String) -> bool {
    // Note: fetching the decimal separator from the current locale isn't a silver bullet
    // because it assumes the current computer's locale is the same as the locale the
    // schematic was authored in -- something that isn't true, for instance, when sharing
    // designs through DIYAudio.com.
    //
    // Some values are self-describing: multiple instances of a single separator character
    // must be thousands separators; a single instance of each character must be a thousands
    // separator followed by a decimal separator; etc.
    //
    // Only when presented with an ambiguous value do we fall back on the current locale.
    *value = value.replace(' ', "");

    let chars: Vec<char> = value.chars().collect();
    let mut ambiguous_separator = '?';
    let mut thousands_separator = '?';
    let mut thousands_separator_found = false;
    let mut decimal_separator = '?';
    let mut decimal_separator_found = false;
    let mut digits = 0;

    for ii in (0..chars.len()).rev() {
        let c = chars[ii];

        if c.is_ascii_digit() {
            digits += 1;
        } else if c == '.' || c == ',' {
            if decimal_separator != '?' || thousands_separator != '?' {
                // We've previously found a non-ambiguous separator...
                if c == decimal_separator {
                    if thousands_separator_found || decimal_separator_found {
                        // decimal before thousands, or more than one decimal
                        return false;
                    }

                    decimal_separator_found = true;
                } else if c == thousands_separator {
                    if digits != 3 {
                        return false; // thousands not followed by 3 digits
                    }

                    thousands_separator_found = true;
                }
            } else if ambiguous_separator != '?' {
                // We've previously found a separator, but we don't know for sure which...
                if c == ambiguous_separator {
                    // They both must be thousands separators
                    thousands_separator = ambiguous_separator;
                    thousands_separator_found = true;
                    decimal_separator = if c == '.' { ',' } else { '.' };
                } else {
                    // The first must have been a decimal, and this must be a thousands.
                    decimal_separator = ambiguous_separator;
                    decimal_separator_found = true;
                    thousands_separator = c;
                    thousands_separator_found = true;
                }
            } else {
                // This is the first separator...
                //
                // If it's preceded by a '0' (only), or if it's followed by some number of
                // digits not equal to 3, then it -must- be a decimal separator.
                //
                // In all other cases we don't really know what it is yet.
                if (ii == 1 && chars[0] == '0') || digits != 3 {
                    decimal_separator = c;
                    decimal_separator_found = true;
                    thousands_separator = if c == '.' { ',' } else { '.' };
                } else {
                    ambiguous_separator = c;
                }
            }

            digits = 0;
        } else {
            digits = 0;
        }
    }

    // If we found nothing definitive then we have to look at the current locale
    if decimal_separator == '?' && thousands_separator == '?' {
        decimal_separator = wxstr::LOCALE_DECIMAL_POINT;
        thousands_separator = if decimal_separator == '.' { ',' } else { '.' };
    }

    // Convert to C-locale
    *value = value
        .chars()
        .filter(|&c| c != thousands_separator)
        .map(|c| if c == decimal_separator { '.' } else { c })
        .collect();

    true
}

/// Port of `QuacksLikeADuck`: the numeric value of a component value string such as `4K7`
/// or `10 nF`, or `None` if it does not look like a value (C++ returns false and stores NaN).
pub fn quacks_like_a_duck(str_: &str) -> Option<f64> {
    let unescaped = unescape_string(str_);
    let (before_digits, mut digits, after_digits) = split_string(&unescaped);

    if !wxstr::strip_both(&before_digits).is_empty() {
        return None;
    }

    convert_separators(&mut digits);

    let (parsed, complete) = wxstr::to_double(&digits);

    if !complete {
        return None;
    }

    let mut number = parsed.unwrap_or(0.0);

    if !after_digits.is_empty() && !apply_modifier(&mut number, &after_digits) {
        return None;
    }

    Some(number)
}

/// Port of `ValueStringCompare`: compare component values ("1K5" == "1.5K").  The sign of the
/// result is the order; like C++ the last comparison returns the raw `CmpNoCase` difference.
pub fn value_string_compare(str_f_word: &str, str_s_word: &str) -> i32 {
    // Compare unescaped text
    let f_word = unescape_string(str_f_word);
    let s_word = unescape_string(str_s_word);

    // Split the two strings into separate parts
    let (str_f_word_beg, mut str_f_word_mid, str_f_word_end) = split_string(&f_word);
    let (str_s_word_beg, mut str_s_word_mid, str_s_word_end) = split_string(&s_word);

    // Compare the Beginning section of the strings
    let is_equal = wxstr::cmp_no_case(&str_f_word_beg, &str_s_word_beg);

    if is_equal > 0 {
        return 1;
    } else if is_equal < 0 {
        return -1;
    }

    // If the first sections are equal compare their digits
    let mut ending_is_modifier = false;

    convert_separators(&mut str_f_word_mid);
    convert_separators(&mut str_s_word_mid);

    let mut l_first_number = wxstr::to_double(&str_f_word_mid).0.unwrap_or(0.0);
    let mut l_second_number = wxstr::to_double(&str_s_word_mid).0.unwrap_or(0.0);

    ending_is_modifier |= apply_modifier(&mut l_first_number, &str_f_word_end);
    ending_is_modifier |= apply_modifier(&mut l_second_number, &str_s_word_end);

    if l_first_number > l_second_number {
        1
    } else if l_first_number < l_second_number {
        -1
    }
    // If the first two sections are equal and the endings are modifiers then compare them
    else if !ending_is_modifier {
        wxstr::cmp_no_case(&str_f_word_end, &str_s_word_end)
    }
    // Ran out of things to compare; they must match
    else {
        0
    }
}

/// Port of `SplitString`: split a string into the text before the trailing number, the
/// number (with an RKM infix such as the `K` of `4K7` replaced by `.`) and the rest.
/// Returns `(beginning, digits, end)`.
pub fn split_string(str_to_split: &str) -> (String, String, String) {
    const SEPARATORS: &str = ".,";

    let chars: Vec<char> = str_to_split.chars().collect();
    let mut infix = '\0';
    let mut beginning = String::new();
    let mut digits = String::new();
    let mut end = String::new();

    // There no need to do anything if the string is empty
    if chars.is_empty() {
        return (beginning, digits, end);
    }

    // Starting at the end of the string look for the first digit
    let mut ii = chars.len() as isize - 1;

    while ii >= 0 {
        if wxstr::is_digit(chars[ii as usize]) {
            break;
        }

        ii -= 1;
    }

    // If there were no digits then just set the single string
    if ii < 0 {
        beginning = str_to_split.to_string();
    } else {
        // Since there is at least one digit this is the trailing string
        end = chars[(ii + 1) as usize..].iter().collect();

        // Go to the end of the digits
        let position = ii + 1;

        while ii >= 0 {
            let c = chars[ii as usize];

            if wxstr::is_digit(c) {
                ii -= 1;
                continue;
            }

            // This can be tricky to get to parse things like 4K7 (or just K7)
            // but not G4W
            let infix_ok = infix == '\0'
                // Only allow one infix character e.g. 4K7, not 4KK7
                && (ii > 0 || end.is_empty())
                // Allowed only it isn't the first character, or nothing follows it,
                // e.g. K7 but not K7X
                && is_old_school_decimal_separator(c).is_some_and(|mut scale| {
                    // Also make sure its a valid SI separator, e.g. 4K7 but not 4X7.
                    // Finally make that the combo makes sense e.g. T9G fails because TG is
                    // not a valid combo, but unfortunately cursed constructions like 1u5F
                    // are indeed found in the wild
                    apply_modifier(&mut scale, &(c.to_string() + &end))
                });

            if infix_ok {
                infix = c;
                ii -= 1;
                continue;
            } else if SEPARATORS.contains(c) {
                ii -= 1;
                continue;
            } else {
                break;
            }
        }

        // If all that was left was digits, then just set the digits string
        if ii < 0 {
            digits = chars[..position as usize].iter().collect();
        }
        // Otherwise everything else is part of the preamble
        else {
            digits = chars[(ii + 1) as usize..position as usize].iter().collect();
            beginning = chars[..(ii + 1) as usize].iter().collect();
        }

        if infix != '\0' {
            digits = digits.replace(infix, ".");
            end = infix.to_string() + &end;
        }
    }

    (beginning, digits, end)
}

/// Port of `GetTrailingInt`: the number formed by the trailing ASCII digits (0 if none).
/// Overflow wraps (undefined behaviour in C++).
pub fn get_trailing_int(str_: &str) -> i32 {
    let mut number: i32 = 0;
    let mut base: i32 = 1;

    // Trim and extract the trailing numeric part
    for c in str_.chars().rev() {
        if !c.is_ascii_digit() {
            break;
        }

        number = number.wrapping_add((c as i32 - '0' as i32).wrapping_mul(base));
        base = base.wrapping_mul(10);
    }

    number
}

/// Port of `GetIllegalFileNameWxChars`.
pub fn get_illegal_file_name_wx_chars() -> String {
    ILLEGAL_FILE_NAME_CHARS.to_string()
}

/// Port of `ReplaceIllegalFileNameChars( std::string&, int )`: replace the illegal file name
/// characters by `replace_char`, or by `%xx` (lower-case hex) when it is 0.  The string is
/// treated as bytes like the C++; true if anything changed.
pub fn replace_illegal_file_name_chars_std(name: &mut String, replace_char: u8) -> bool {
    let Some(first_illegal_pos) = name.find(|c| ILLEGAL_FILE_NAME_CHARS.contains(c)) else {
        return false;
    };

    let mut result = String::with_capacity(name.len() + 16);
    // append the valid part
    result.push_str(&name[..first_illegal_pos]);

    for c in name[first_illegal_pos..].chars() {
        // Check if this specific char is illegal
        if ILLEGAL_FILE_NAME_CHARS.contains(c) {
            if replace_char != 0 {
                result.push(replace_char as char);
            } else {
                result.push_str(&format!("%{:02x}", c as u32));
            }
        } else {
            result.push(c);
        }
    }

    *name = result;
    true
}

/// Port of `ReplaceIllegalFileNameChars( wxString&, int )`: the `wxString` overload, which
/// appends `replace_char` as a code point (`'\0'` selects `%xx` encoding).
pub fn replace_illegal_file_name_chars(name: &mut String, replace_char: char) -> bool {
    let mut changed = false;
    let mut result = String::with_capacity(name.len());

    for c in name.chars() {
        if ILLEGAL_FILE_NAME_CHARS.contains(c) {
            if replace_char != '\0' {
                result.push(replace_char);
            } else {
                result.push_str(&format!("%{:02x}", c as u32));
            }

            changed = true;
        } else {
            result.push(c);
        }
    }

    if changed {
        *name = result;
    }

    changed
}

/// Port of `StripTrailingZeros`: remove trailing zeros, keeping `trailing_zero_allowed`
/// digits after the decimal separator.
///
/// Faithful to a C++ quirk: without a separator `Find` returns -1, which as `unsigned` is
/// 0xFFFFFFFF, so the minimum length wraps around to `trailing_zero_allowed` and zeros of
/// the integer part are removed ("100" -> "1").
pub fn strip_trailing_zeros(string_value: &mut String, trailing_zero_allowed: u32) {
    let sep = wxstr::LOCALE_DECIMAL_POINT;
    let sep_pos: u32 = match string_value.chars().position(|c| c == sep) {
        Some(p) => p as u32,
        None => u32::MAX,
    };

    if sep_pos > 0 {
        // We want to keep at least aTrailingZeroAllowed digits after the separator
        let min_len = sep_pos.wrapping_add(trailing_zero_allowed).wrapping_add(1) as usize;

        while string_value.chars().count() > min_len {
            if string_value.ends_with('0') {
                string_value.pop();
            } else {
                break;
            }
        }
    }
}

/// Port of `FormatDouble2Str`: a double as written to KiCad files: `{:.10g}`, or `{:.16f}`
/// without trailing zeros for magnitudes up to 0.0001 (where `%g` would use an exponent).
pub fn format_double2_str(value: f64) -> String {
    if value != 0.0 && value.abs() <= 0.0001 {
        let mut buf = format_f(value, 16);

        // remove trailing zeros (and the decimal marker if needed)
        while buf.ends_with('0') {
            buf.pop();
        }

        // if the value was really small
        // we may have just stripped all the zeros after the decimal
        if buf.ends_with('.') {
            buf.pop();
        }

        buf
    } else {
        format_g(value, 10)
    }
}

/// `wxString::FromCDouble( val, precision )`: an `std::ostringstream` in the C locale, fixed
/// notation with the given precision, or the default (`%g`) for -1; an empty string for a
/// precision below -1 (`wxCHECK_MSG`).
pub fn wx_from_c_double(value: f64, precision: i32) -> String {
    match precision {
        -1 => format_g(value, 6),
        p if p < -1 => String::new(),
        p => format_f(value, p as usize),
    }
}

/// Port of `FormatTrimmedDecimal`: fixed notation with `precision` decimals, trailing zeros
/// trimmed to one, and no sign on a rounded zero.
pub fn format_trimmed_decimal(value: f64, precision: i32) -> String {
    let mut str_ = wx_from_c_double(value, precision);
    let dot = str_.find('.');

    // Keep one zero after the last significant digit and never trim the integer part
    if let Some(dot) = dot {
        while str_.len() - dot > 2 && str_.ends_with("00") {
            str_.pop();
        }
    }

    // Rounded zero has no meaningful sign
    if str_.starts_with('-') && str_[1..].chars().all(|c| c == '0' || c == '.') {
        str_.remove(0);
    }

    str_
}

/// Port of `UIDouble2Str`: like [`format_double2_str`] but through `snprintf` (identical
/// output).
pub fn ui_double2_str(value: f64) -> String {
    if value != 0.0 && value.abs() <= 0.0001 {
        // For these small values, %f works fine,
        // and %g gives an exponent
        let mut buf: Vec<u8> = format_f(value, 16).into_bytes();
        let mut len = buf.len() as isize;

        loop {
            len -= 1;

            if !(len > 0 && buf[len as usize] == b'0') {
                break;
            }
        }

        if buf[len as usize] == b'.' || buf[len as usize] == b',' {
            // drop the separator
        } else {
            len += 1;
        }

        buf.truncate(len as usize);
        String::from_utf8(buf).unwrap_or_default()
    } else {
        // For these values, %g works fine, and sometimes %f
        // gives a bad value (try aValue = 1.222222222222, with %.16f format!)
        format_g(value, 10)
    }
}

/// Port of `From_UTF8( const char* )` / `From_UTF8( const std::string& )` for raw bytes: the
/// UTF-8 text, or (for invalid UTF-8) the bytes as Latin-1 (`wxString::From8BitData`).
///
/// The intermediate C++ fallback, `wxConvCurrent->cMB2WC`, fails for invalid UTF-8 in the
/// UTF-8 locales KiCad runs in, so it is not reproduced.
pub fn from_utf8_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Port of `struct rsort_wxString`: "less than" comparing the strings from their last
/// characters (an empty string is first).
pub fn rsort_wx_string_less(str_a: &str, str_b: &str) -> bool {
    if str_a.is_empty() {
        // note: this rule implies that a null string is first in the sort order
        return !str_b.is_empty();
    }

    if str_b.is_empty() {
        return false;
    }

    let mut a = str_a.chars().rev();
    let mut b = str_b.chars().rev();

    loop {
        match (a.next(), b.next()) {
            (Some(ca), Some(cb)) => {
                if ca == cb {
                    continue;
                }

                return ca < cb;
            }
            (_, None) => return false,
            (None, Some(_)) => return true,
        }
    }
}

/// Port of the inline `split( const std::string&, const std::string& )`: split at any of the
/// delimiter characters (a trailing delimiter gives no empty last token).
pub fn split(str_: &str, delim: &str) -> Vec<String> {
    let bytes = str_.as_bytes();
    let delim = delim.as_bytes();
    let mut pos = 0;
    let mut last_pos = 0;
    let mut tokens = Vec::new();

    while pos < bytes.len() {
        pos = bytes[last_pos.min(bytes.len())..]
            .iter()
            .position(|b| delim.contains(b))
            .map_or(bytes.len(), |p| p + last_pos);

        tokens.push(String::from_utf8_lossy(&bytes[last_pos..pos]).into_owned());
        last_pos = pos + 1;
    }

    tokens
}

/// Port of `AccumulateDescription`: append `item` to a comma separated list.
pub fn accumulate_description(desc: &mut String, item: &str) {
    if !desc.is_empty() {
        desc.push_str(", ");
    }

    desc.push_str(item);
}

/// Port of `AccumulateDescriptions`.
pub fn accumulate_descriptions<I, S>(items: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut desc = String::new();

    for item in items {
        accumulate_description(&mut desc, item.as_ref());
    }

    desc
}

// Stacked pin notation ("[1,2,5-7]").

/// Characters that carry structural meaning inside stacked pin notation and therefore must be
/// backslash-escaped when they appear literally inside an individual pin number.
fn is_stacked_pin_special_char(c: char) -> bool {
    matches!(c, '[' | ']' | ',' | '-' | '\\')
}

/// A backslash starts an escape sequence only when it precedes one of the structural
/// characters.  Any other backslash is a literal character, which keeps legacy notation that
/// happened to use an un-escaped backslash (e.g. [A\B,C]) intact.
fn is_escape_at(text: &[char], index: usize) -> bool {
    text[index] == '\\' && index + 1 < text.len() && is_stacked_pin_special_char(text[index + 1])
}

/// Split the inner part of a stacked notation string on commas, honouring backslash escaping
/// so that a literal comma in a pin number (written "\,") does not start a new item.  The
/// returned parts are still escaped.
fn split_stacked_pin_items(inner: &[char]) -> Vec<Vec<char>> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;

    while i < inner.len() {
        if is_escape_at(inner, i) {
            current.push(inner[i]);
            current.push(inner[i + 1]);
            i += 1;
        } else if inner[i] == ',' {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(inner[i]);
        }

        i += 1;
    }

    parts.push(current);
    parts
}

fn find_unescaped(text: &[char], c: char) -> Option<usize> {
    let mut i = 0;

    while i < text.len() {
        if is_escape_at(text, i) {
            i += 1;
        } else if text[i] == c {
            return Some(i);
        }

        i += 1;
    }

    None
}

/// A backslash that does not precede a structural character is a literal and is left
/// untouched.
fn unescape_stacked_pin_item(item: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < item.len() {
        if is_escape_at(item, i) {
            i += 1;
        }

        out.push(item[i]);
        i += 1;
    }

    out
}

/// Extract (prefix, numericValue) where numericValue = -1 if no numeric suffix.
fn parse_alpha_numeric_pin(pin_num: &str) -> (String, i64) {
    let chars: Vec<char> = pin_num.chars().collect();
    let mut prefix = String::new();
    let mut num_value: i64 = -1;
    let mut num_start = chars.len();

    for i in (0..chars.len()).rev() {
        if !wxstr::is_digit(chars[i]) {
            num_start = i + 1;
            break;
        }

        if i == 0 {
            num_start = 0; // all digits
        }
    }

    if num_start < chars.len() {
        prefix = chars[..num_start].iter().collect();
        let numeric_part: String = chars[num_start..].iter().collect();

        // wxString::ToLong: the value is not stored on overflow
        if let Ok(v) = numeric_part.parse::<i64>() {
            num_value = v;
        }
    }

    (prefix, num_value)
}

/// Port of `EscapeStackedPinItem`: backslash-escape the structural characters of a pin
/// number.
pub fn escape_stacked_pin_item(pin_number: &str) -> String {
    let mut escaped = String::with_capacity(pin_number.len());

    for c in pin_number.chars() {
        if is_stacked_pin_special_char(c) {
            escaped.push('\\');
        }

        escaped.push(c);
    }

    escaped
}

/// Port of `SplitStackedPinDisplayItems`: the comma separated items of the inner part,
/// unescaped (ranges are kept as text).
pub fn split_stacked_pin_display_items(inner: &str) -> Vec<String> {
    let inner: Vec<char> = inner.chars().collect();

    split_stacked_pin_items(&inner)
        .iter()
        .map(|p| unescape_stacked_pin_item(p))
        .collect()
}

/// The parts of `[...]` notation, trimmed; `None` for an invalid range (the C++ early
/// return), else the expanded pins.
fn expand_stacked_parts(inner: &[char]) -> Option<Vec<String>> {
    let mut expanded = Vec::new();

    for part in split_stacked_pin_items(inner) {
        let part_str: String = part.iter().collect();
        let part: Vec<char> = wxstr::strip_both(&part_str).chars().collect();

        if part.is_empty() {
            continue;
        }

        // A range (e.g. A1-A4) is only recognized on an unescaped dash; an escaped dash is a
        // literal character inside a single pin number.
        if let Some(dash_pos) = find_unescaped(&part, '-') {
            let start_txt = unescape_stacked_pin_item(&part[..dash_pos]);
            let end_txt = unescape_stacked_pin_item(&part[dash_pos + 1..]);

            let (start_prefix, start_val) = parse_alpha_numeric_pin(wxstr::strip_both(&start_txt));
            let (end_prefix, end_val) = parse_alpha_numeric_pin(wxstr::strip_both(&end_txt));

            if start_prefix != end_prefix || start_val == -1 || end_val == -1 || start_val > end_val
            {
                return None;
            }

            for ii in start_val..=end_val {
                expanded.push(format!("{start_prefix}{ii}"));
            }
        } else {
            expanded.push(unescape_stacked_pin_item(&part));
        }
    }

    Some(expanded)
}

/// Port of `ExpandStackedPinNotation`: expand `[1,3,5-7]` into its pin numbers.  Returns the
/// pins and the validity flag (`aValid`).
pub fn expand_stacked_pin_notation(pin_name: &str) -> (Vec<String>, bool) {
    let has_open_bracket = pin_name.contains('[');
    let has_close_bracket = pin_name.contains(']');
    let bracketed = pin_name.starts_with('[') && pin_name.ends_with(']');

    if (has_open_bracket || has_close_bracket) && !bracketed {
        return (vec![pin_name.to_string()], false);
    }

    if !bracketed {
        return (vec![pin_name.to_string()], true);
    }

    let chars: Vec<char> = pin_name.chars().collect();
    let inner = &chars[1..chars.len() - 1];

    match expand_stacked_parts(inner) {
        None => (vec![pin_name.to_string()], false),
        Some(expanded) if expanded.is_empty() => (vec![pin_name.to_string()], false),
        Some(expanded) => (expanded, true),
    }
}

/// Port of `CountStackedPinNotation`: the number of pins of a stacked notation without
/// building them.  Returns the count and the validity flag (`aValid`; the C++ fast path for
/// a null `aValid` gives the same count).
pub fn count_stacked_pin_notation(pin_name: &str) -> (i32, bool) {
    let chars: Vec<char> = pin_name.chars().collect();
    let len = chars.len();

    // An empty pin number is a single (valid) pin; guard before indexing below.
    if len == 0 {
        return (1, true);
    }

    // Fast path: if no brackets, it's a single pin
    let has_open_bracket = pin_name.contains('[');
    let has_close_bracket = pin_name.contains(']');

    if (has_open_bracket || has_close_bracket) && (chars[0] != '[' || chars[len - 1] != ']') {
        return (1, false);
    }

    if chars[0] != '[' || chars[len - 1] != ']' {
        return (1, true);
    }

    // "[" alone: Mid( 1, -1 ) is everything after the first character
    let inner: &[char] = if len >= 2 { &chars[1..len - 1] } else { &[] };
    let mut count: i32 = 0;

    for part in split_stacked_pin_items(inner) {
        let part_str: String = part.iter().collect();
        let part: Vec<char> = wxstr::strip_both(&part_str).chars().collect();

        if part.is_empty() {
            continue;
        }

        if let Some(dash_pos) = find_unescaped(&part, '-') {
            let start_txt = unescape_stacked_pin_item(&part[..dash_pos]);
            let end_txt = unescape_stacked_pin_item(&part[dash_pos + 1..]);

            let (start_prefix, start_val) = parse_alpha_numeric_pin(wxstr::strip_both(&start_txt));
            let (end_prefix, end_val) = parse_alpha_numeric_pin(wxstr::strip_both(&end_txt));

            if start_prefix != end_prefix || start_val == -1 || end_val == -1 || start_val > end_val
            {
                return (1, false);
            }

            // Count pins in the range
            count = count.wrapping_add((end_val - start_val + 1) as i32);
        } else {
            // Single pin
            count += 1;
        }
    }

    if count == 0 {
        return (1, false);
    }

    (count, true)
}

/// Port of `IsAutoGeneratedNetName`.
pub fn is_auto_generated_net_name(net_name: &str) -> bool {
    net_name.starts_with("unconnected-(") || net_name.starts_with("Net-(")
}

/// Port of `GetDefaultVariantName`: `"< Default >"`.
pub fn get_default_variant_name() -> String {
    DEFAULT_VARIANT_NAME.to_string()
}

/// Port of `GetVariantFileToken`: the variant name usable in file names (empty for the
/// default variant).
pub fn get_variant_file_token(variant_name: &str) -> String {
    if variant_name.is_empty() || wxstr::cmp_no_case(variant_name, DEFAULT_VARIANT_NAME) == 0 {
        return String::new();
    }

    let mut token = variant_name.to_string();
    replace_illegal_file_name_chars(&mut token, '_');
    token
}

/// Port of `ExpandVariantOutputPath`: replace `${VARIANT}`.
pub fn expand_variant_output_path(path: &str, variant_token: &str) -> String {
    path.replace("${VARIANT}", variant_token)
}

/// Port of `SortVariantNames`: the default variant first, then [`str_num_cmp`].
pub fn sort_variant_names(lhs: &str, rhs: &str) -> i32 {
    if lhs == DEFAULT_VARIANT_NAME && rhs != DEFAULT_VARIANT_NAME {
        return -1;
    }

    if lhs != DEFAULT_VARIANT_NAME && rhs == DEFAULT_VARIANT_NAME {
        return 1;
    }

    str_num_cmp(lhs, rhs, CmpT::CaseSensitive)
}
