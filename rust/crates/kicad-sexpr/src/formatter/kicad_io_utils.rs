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

//! Port of `include/io/kicad/kicad_io_utils.h` and `common/io/kicad/kicad_io_utils.cpp`
//! (namespace `KICAD_FORMAT`): small s-expression formatting helpers and [`prettify`], the
//! canonical re-formatter every saved KiCad file passes through.
//!
//! `LoadFileToMemory` reads a file and is not ported (no file system in library crates).

use crate::formatter::richio::OutputFormatter;
use crate::ki_exception::IoError;

/// Port of `KICAD_FORMAT::FormatBool`: writes a boolean to the formatter, in the style
/// `(aKey [yes|no])`.
///
/// `out` is the output formatter to write to, `key` is the name of the boolean flag and
/// `value` is the value to write.
pub fn format_bool(out: &mut dyn OutputFormatter, key: &str, value: bool) -> Result<(), IoError> {
    out.print(format_args!("({} {})", key, if value { "yes" } else { "no" }))?;
    Ok(())
}

/// Port of `KICAD_FORMAT::FormatOptBool`: writes an optional boolean to the formatter.
/// If a value is present, calls [`format_bool`].  If no value is present, writes `(aKey none)`.
pub fn format_opt_bool(
    out: &mut dyn OutputFormatter,
    key: &str,
    value: Option<bool>,
) -> Result<(), IoError> {
    match value {
        Some(value) => format_bool(out, key, value),
        None => {
            out.print(format_args!("({key} none)"))?;
            Ok(())
        }
    }
}

/// Port of `KICAD_FORMAT::FormatUuid`: writes `(uuid "<uuid>")`.
///
/// C++ takes a `KIID`; the port takes the text `KIID::AsString()` returns, so that this crate
/// does not depend on the crate holding `KIID`.
pub fn format_uuid(out: &mut dyn OutputFormatter, uuid: &str) -> Result<(), IoError> {
    let quoted = out.quotew(uuid);
    out.print(format_args!("(uuid {quoted})"))?;
    Ok(())
}

/// Port of `KICAD_FORMAT::FormatCustomProperties`: writes custom properties as a series of
/// `(custom_property "key" "value")`.
///
/// C++ takes an `EDA_ITEM` and iterates its `GetCustomProperties()` map (nothing is written if
/// the item has none); the port takes the key/value pairs, which the caller must supply in the
/// iteration order of that map.
pub fn format_custom_properties<K, V>(
    out: &mut dyn OutputFormatter,
    properties: impl IntoIterator<Item = (K, V)>,
) -> Result<(), IoError>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    for (key, value) in properties {
        let key = out.quotew(key.as_ref());
        let value = out.quotew(value.as_ref());
        out.print(format_args!("(custom_property {key} {value})"))?;
    }

    Ok(())
}

/// Encode `data` as base 64 text the way `wxBase64Encode` does: the standard alphabet with
/// `'='` padding and no line breaks.
pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(char::from(ALPHABET[usize::from(b0 >> 2)]));
        out.push(char::from(ALPHABET[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))]));

        if chunk.len() > 1 {
            out.push(char::from(ALPHABET[usize::from(((b1 & 0x0f) << 2) | (b2 >> 6))]));
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(char::from(ALPHABET[usize::from(b2 & 0x3f)]));
        } else {
            out.push('=');
        }
    }

    out
}

/// Port of `KICAD_FORMAT::FormatStreamData`: write binary data to the formatter as base 64
/// encoded string: `(data` followed by one quoted string of at most 76 characters per line and
/// the closing `)`.
///
/// C++ takes a `wxStreamBuffer`; the port takes its content.
pub fn format_stream_data(out: &mut dyn OutputFormatter, stream: &[u8]) -> Result<(), IoError> {
    out.print(format_args!("(data"))?;

    let encoded = base64_encode(stream);

    // Apparently the MIME standard character width for base64 encoding is 76 (unconfirmed)
    // so use it in a vein attempt to be standard like.
    const MIME_BASE64_LENGTH: usize = 76;

    let mut first = 0;

    while first < encoded.len() {
        let end = (first + MIME_BASE64_LENGTH).min(encoded.len());

        out.print(format_args!("\n\"{}\"", &encoded[first..end]))?;
        first += MIME_BASE64_LENGTH;
    }

    out.print(format_args!(")"))?; // Closes data token.
    Ok(())
}

/// Port of `KICAD_FORMAT::FORMAT_MODE`: control the pretty-printing mode used by [`prettify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FormatMode {
    /// `NORMAL`: follows standard pretty-printing rules.
    #[default]
    Normal,
    /// `COMPACT_TEXT_PROPERTIES`: collapses certain text properties to single-line.
    CompactTextProperties,
    /// `LIBRARY_TABLE`: puts library table rows on a single line.
    LibraryTable,
}

/// The `isWhitespace` lambda of `Prettify`.
fn is_whitespace(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
}

/// Append `"\n"` and the indentation for `list_depth` to `formatted`.
fn push_newline_and_indent(formatted: &mut Vec<u8>, list_depth: usize) {
    // Configuration
    const INDENT_CHAR: u8 = b'\t';
    const INDENT_SIZE: usize = 1;

    formatted.push(b'\n');
    formatted.extend(std::iter::repeat_n(INDENT_CHAR, list_depth * INDENT_SIZE));
}

/// The token made of the ASCII letters that follow the character at `it` (an opening
/// parenthesis); the scan of the `isShortForm` and `isLib` lambdas of `Prettify`.
///
/// C++ tests the bytes with `isalpha`; KiCad runs with a UTF-8 or "C" character type locale,
/// where that is true for the ASCII letters only.
fn alpha_token(source: &[u8], it: usize) -> &[u8] {
    let start = it + 1;
    let mut seek = start;

    while seek < source.len() && source[seek].is_ascii_alphabetic() {
        seek += 1;
    }

    source.get(start..seek).unwrap_or(&[])
}

/// Port of `KICAD_FORMAT::Prettify` on bytes: pretty-prints s-expression text according to
/// KiCad format rules.  See [`prettify`].
pub fn prettify_bytes(source: &[u8], mode: FormatMode) -> Vec<u8> {
    // Configuration
    const QUOTE_CHAR: u8 = b'"';
    const INDENT_SIZE: i32 = 1;

    // In order to visually compress PCB files, it is helpful to special-case long lists of
    // (xy ...) lists, which we allow to exist on a single line until we reach column 99.
    const XY_SPECIAL_CASE_COLUMN_LIMIT: i32 = 99;

    // If whitespace occurs inside a list after this threshold, it will be converted into a
    // newline and the indentation will be increased.  This is mainly used for image and group
    // objects, which contain potentially long sets of string tokens within a single list.
    const CONSECUTIVE_TOKEN_WRAP_THRESHOLD: i32 = 72;

    let text_special_case = mode == FormatMode::CompactTextProperties;
    let lib_special_case = mode == FormatMode::LibraryTable;

    let end = source.len();
    let mut formatted: Vec<u8> = Vec::with_capacity(source.len());

    let mut cursor = 0;

    let mut list_depth: i32 = 0;
    let mut lib_depth: i32 = 0;
    let mut last_non_whitespace: u8 = 0;
    let mut in_quote = false;
    let mut has_inserted_space = false;
    let mut in_multi_line_list = false;
    let mut in_xy = false;
    let mut in_short_form = false;
    let mut in_lib_row = false;
    let mut short_form_depth: i32 = 0;
    let mut column: i32 = 0;
    let mut backslash_count: i32 = 0; // Count of successive backslash read since any other char

    // C++ calls nextNonWhitespace( cursor ) for every input character, which rescans a
    // whitespace run from each of its characters.  The result is only looked at for
    // whitespace, and within one run it is always the character behind the run, so remember
    // where the current run ends instead of searching again.
    let mut whitespace_run_end = 0;

    let mut next_non_whitespace = |it: usize| -> u8 {
        if it >= whitespace_run_end {
            let mut seek = it;

            while seek != end && is_whitespace(source[seek]) {
                seek += 1;
            }

            whitespace_run_end = seek;
        }

        if whitespace_run_end == end { 0 } else { source[whitespace_run_end] }
    };

    let is_xy = |it: usize| -> bool {
        it + 3 < end && source[it + 1] == b'x' && source[it + 2] == b'y' && source[it + 3] == b' '
    };

    let is_short_form = |it: usize| -> bool {
        matches!(
            alpha_token(source, it),
            b"font" | b"stroke" | b"fill" | b"teardrop" | b"offset" | b"rotate" | b"scale"
        )
    };

    let is_lib = |it: usize| -> bool { alpha_token(source, it) == b"lib" };

    while cursor != end {
        let c = source[cursor];

        // A comment line is not s-expression syntax and must keep its spacing, quotes and parens
        if list_depth == 0 && c == b'#' && (formatted.is_empty() || formatted.last() == Some(&b'\n'))
        {
            let line_end =
                source[cursor..].iter().position(|&b| b == b'\n').map_or(end, |p| cursor + p);

            formatted.extend_from_slice(&source[cursor..line_end]);
            formatted.push(b'\n');

            if line_end == end {
                break;
            }

            cursor = line_end + 1;
            continue;
        }

        if is_whitespace(c) && !in_quote {
            if !has_inserted_space           // Only permit one space between chars
                && list_depth > 0            // Do not permit spaces in outer list
                && last_non_whitespace != b'(' // Remove extra space after start of list
                && {
                    let next = next_non_whitespace(cursor);

                    next != b')'             // Remove extra space before end of list
                        && next != b'(' // Remove extra space before newline
                }
            {
                if in_xy || column < CONSECUTIVE_TOKEN_WRAP_THRESHOLD {
                    // Note that we only insert spaces here, no matter what kind of whitespace is
                    // in the input.  Newlines will be inserted as needed by the logic below.
                    formatted.push(b' ');
                    column += 1;
                } else if in_short_form || in_lib_row {
                    formatted.push(b' ');
                } else {
                    push_newline_and_indent(&mut formatted, list_depth as usize);
                    column = list_depth * INDENT_SIZE;
                    in_multi_line_list = true;
                }

                has_inserted_space = true;
            }
        } else {
            has_inserted_space = false;

            if c == b'(' && !in_quote {
                let current_is_xy = is_xy(cursor);
                let current_is_short_form = text_special_case && is_short_form(cursor);
                let current_is_lib = lib_special_case && is_lib(cursor);

                if formatted.is_empty() || formatted.last() == Some(&b'\n') {
                    formatted.push(b'(');
                    column += 1;
                } else if in_xy && current_is_xy && column < XY_SPECIAL_CASE_COLUMN_LIMIT {
                    // List-of-points special case
                    formatted.extend_from_slice(b" (");
                    column += 2;
                } else if in_short_form || in_lib_row {
                    formatted.extend_from_slice(b" (");
                    column += 2;
                } else {
                    push_newline_and_indent(&mut formatted, list_depth as usize);
                    formatted.push(b'(');
                    column = list_depth * INDENT_SIZE + 1;
                }

                in_xy = current_is_xy;

                if current_is_short_form {
                    in_short_form = true;
                    short_form_depth = list_depth;
                } else if current_is_lib {
                    in_lib_row = true;
                    lib_depth = list_depth;
                }

                list_depth += 1;
            } else if c == b')' && !in_quote {
                if list_depth > 0 {
                    list_depth -= 1;
                }

                if in_short_form {
                    formatted.push(b')');
                    column += 1;
                } else if in_lib_row && list_depth == lib_depth {
                    formatted.push(b')');
                    in_lib_row = false;
                } else if last_non_whitespace == b')' || in_multi_line_list {
                    push_newline_and_indent(&mut formatted, list_depth as usize);
                    formatted.push(b')');
                    column = list_depth * INDENT_SIZE + 1;
                    in_multi_line_list = false;
                } else {
                    formatted.push(b')');
                    column += 1;
                }

                if short_form_depth == list_depth {
                    in_short_form = false;
                    short_form_depth = 0;
                }
            } else {
                // The output formatter escapes double-quotes (like \")
                // But a corner case is a sequence like \\"
                // therefore a '\' is attached to a '"' if a odd number of '\' is detected
                if c == b'\\' {
                    backslash_count += 1;
                } else if c == QUOTE_CHAR && (backslash_count & 1) == 0 {
                    in_quote = !in_quote;
                }

                if c != b'\\' {
                    backslash_count = 0;
                }

                formatted.push(c);
                column += 1;
            }

            last_non_whitespace = c;
        }

        cursor += 1;
    }

    // newline required at end of line / file for POSIX compliance. Keeps git diffs clean.
    formatted.push(b'\n');

    formatted
}

/// Port of `KICAD_FORMAT::Prettify`: pretty-prints s-expression text according to KiCad format
/// rules, returning the formatted text.  This is the main entry point; C++ modifies its
/// argument in place, for which there is [`prettify_in_place`].
///
/// Formatting rules:
/// - All extra (non-indentation) whitespace is trimmed
/// - Indentation is one tab
/// - Starting a new list (open paren) starts a new line with one deeper indentation
/// - Lists with no inner lists go on a single line
/// - End of multi-line lists (close paren) goes on a single line at same indentation as its
///   start
///
/// For example:
/// ```text
/// (first
///     (second
///         (third list)
///         (another list)
///     )
///     (fifth)
///     (sixth thing with lots of tokens
///         (and a sub list)
///     )
/// )
/// ```
///
/// Beyond these rules, consecutive `(xy ...)` lists share a line up to column 99, a list of
/// atoms is wrapped once a line is 72 columns long, comment lines (`#` at the start of a line
/// outside of any list) are copied verbatim, and the result always ends with a newline.
/// [`FormatMode::CompactTextProperties`] keeps `font`, `stroke`, `fill`, `teardrop`, `offset`,
/// `rotate` and `scale` lists on one line; [`FormatMode::LibraryTable`] does so for `lib` rows.
pub fn prettify(source: &str, mode: FormatMode) -> String {
    let formatted = prettify_bytes(source.as_bytes(), mode);

    // Only ASCII bytes are removed from or inserted into the text, and never inside a
    // multi-byte sequence, so the result of formatting valid UTF-8 is valid UTF-8.
    String::from_utf8(formatted)
        .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned())
}

/// Port of `KICAD_FORMAT::Prettify( std::string& aSource, FORMAT_MODE aMode )` with its C++
/// signature: replaces `source` by its pretty-printed form.  See [`prettify`].
pub fn prettify_in_place(source: &mut String, mode: FormatMode) {
    *source = prettify(source, mode);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::richio::StringFormatter;

    fn pretty(source: &str) -> String {
        prettify(source, FormatMode::Normal)
    }

    #[test]
    fn format_bool_and_opt_bool() {
        let mut out = StringFormatter::new();

        format_bool(&mut out, "locked", true).unwrap();
        format_bool(&mut out, "hide", false).unwrap();
        format_opt_bool(&mut out, "a", Some(true)).unwrap();
        format_opt_bool(&mut out, "b", Some(false)).unwrap();
        format_opt_bool(&mut out, "c", None).unwrap();

        assert_eq!(out.string(), "(locked yes)(hide no)(a yes)(b no)(c none)");
    }

    #[test]
    fn format_uuid_quotes_the_uuid() {
        let mut out = StringFormatter::new();

        format_uuid(&mut out, "0f1a2b3c-0000-4000-8000-00000000abcd").unwrap();

        assert_eq!(out.string(), "(uuid \"0f1a2b3c-0000-4000-8000-00000000abcd\")");
    }

    #[test]
    fn format_custom_properties_quotes_keys_and_values() {
        let mut out = StringFormatter::new();

        format_custom_properties(&mut out, Vec::<(&str, &str)>::new()).unwrap();
        assert_eq!(out.string(), "");

        format_custom_properties(&mut out, [("key", "value"), ("a \"b\"", "line1\nline2")])
            .unwrap();
        assert_eq!(
            out.string(),
            "(custom_property \"key\" \"value\")(custom_property \"a \\\"b\\\"\" \"line1\\nline2\")"
        );
    }

    #[test]
    fn base64() {
        // RFC 4648 test vectors
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd, 0xfc]), "//79/A==");
    }

    #[test]
    fn format_stream_data_wraps_at_76_characters() {
        let mut out = StringFormatter::new();

        format_stream_data(&mut out, &[]).unwrap();
        assert_eq!(out.string(), "(data)");

        out.clear();
        format_stream_data(&mut out, b"foobar").unwrap();
        assert_eq!(out.string(), "(data\n\"Zm9vYmFy\")");

        // 57 bytes are exactly one line of 76 characters, 58 start a second one.
        out.clear();
        format_stream_data(&mut out, &[0x42; 57]).unwrap();
        assert_eq!(out.string(), format!("(data\n\"{}\")", "QkJC".repeat(19)));

        out.clear();
        format_stream_data(&mut out, &[0x42; 58]).unwrap();
        assert_eq!(out.string(), format!("(data\n\"{}\"\n\"Qg==\")", "QkJC".repeat(19)));
    }

    #[test]
    fn prettify_basic_rules() {
        assert_eq!(
            pretty(
                "(first (second (third list) (another list)) (fifth) (sixth thing with lots of \
                 tokens (and a sub list)))"
            ),
            "(first\n\t(second\n\t\t(third list)\n\t\t(another list)\n\t)\n\t(fifth)\n\t(sixth \
             thing with lots of tokens\n\t\t(and a sub list)\n\t)\n)\n"
        );
    }

    #[test]
    fn prettify_trims_whitespace() {
        assert_eq!(pretty("(a)"), "(a)\n");
        assert_eq!(pretty("  ( a   b\t\tc \r\n d )  \n\n"), "(a b c d)\n");
        assert_eq!(pretty("(a\n  (b)\n)\n"), "(a\n\t(b)\n)\n");
        assert_eq!(pretty("( a ( b ) )"), "(a\n\t(b)\n)\n");
        assert_eq!(pretty("(a)(b)"), "(a)\n(b)\n");
        assert_eq!(pretty("(a) (b)"), "(a)\n(b)\n");
    }

    #[test]
    fn prettify_empty_and_degenerate_input() {
        assert_eq!(pretty(""), "\n");
        assert_eq!(pretty("   \n\t"), "\n");
        assert_eq!(pretty("abc def"), "abcdef\n");
        assert_eq!(pretty(")"), ")\n");
        assert_eq!(pretty("(a))"), "(a)\n)\n");
        assert_eq!(pretty("((a"), "(\n\t(a\n");
        assert_eq!(pretty("()"), "()\n");
        assert_eq!(pretty("(()())"), "(\n\t()\n\t()\n)\n");
    }

    #[test]
    fn prettify_keeps_quoted_text() {
        assert_eq!(pretty("(a \"b   c\" \"(d)\"  \" \")"), "(a \"b   c\" \"(d)\" \" \")\n");
        // An escaped quote does not end the string, an escaped backslash before a quote does.
        assert_eq!(pretty("(a \"x \\\" (  y\" b)"), "(a \"x \\\" (  y\" b)\n");
        assert_eq!(pretty("(a \"x\\\\\" (b))"), "(a \"x\\\\\"\n\t(b)\n)\n");
        assert_eq!(pretty("(a \"x\\\\\\\" (b)\")"), "(a \"x\\\\\\\" (b)\")\n");
        // Newlines inside a quoted string are kept as they are.
        assert_eq!(pretty("(a \"l1\nl2\")"), "(a \"l1\nl2\")\n");
    }

    #[test]
    fn prettify_xy_lists_share_lines_up_to_column_99() {
        let mut source = String::from("(pts");

        for i in 0..12 {
            source.push_str(&format!(" (xy {}.5 {}.25)", 100 + i, 200 + i));
        }

        source.push(')');

        assert_eq!(
            pretty(&source),
            "(pts\n\t(xy 100.5 200.25) (xy 101.5 201.25) (xy 102.5 202.25) (xy 103.5 203.25) \
             (xy 104.5 204.25) (xy 105.5 205.25)\n\t(xy 106.5 206.25) (xy 107.5 207.25) (xy \
             108.5 208.25) (xy 109.5 209.25) (xy 110.5 210.25) (xy 111.5 211.25)\n)\n"
        );

        // "(xyz" and "(xy)" are not points.
        assert_eq!(pretty("(pts (xyz 1 2) (xyz 3 4))"), "(pts\n\t(xyz 1 2)\n\t(xyz 3 4)\n)\n");
        assert_eq!(pretty("(pts (xy) (xy))"), "(pts\n\t(xy)\n\t(xy)\n)\n");
    }

    #[test]
    fn prettify_wraps_long_token_lists_at_column_72() {
        let mut source = String::from("(members");

        for i in 0..8 {
            source.push_str(&format!(" \"0000000{i}-1111-2222-3333-444444444444\""));
        }

        source.push(')');

        assert_eq!(
            pretty(&source),
            "(members \"00000000-1111-2222-3333-444444444444\" \
             \"00000001-1111-2222-3333-444444444444\"\n\t\
             \"00000002-1111-2222-3333-444444444444\" \"00000003-1111-2222-3333-444444444444\"\n\t\
             \"00000004-1111-2222-3333-444444444444\" \"00000005-1111-2222-3333-444444444444\"\n\t\
             \"00000006-1111-2222-3333-444444444444\" \"00000007-1111-2222-3333-444444444444\"\n\
             )\n"
        );
    }

    #[test]
    fn prettify_comment_lines() {
        assert_eq!(pretty("# a  (comment) \"x\n(a  b)"), "# a  (comment) \"x\n(a b)\n");
        // The line break the comment ends with is followed by the final one.
        assert_eq!(pretty("# only"), "# only\n\n");
        assert_eq!(pretty("# only\n"), "# only\n\n");
        assert_eq!(pretty("# c1\n#  c2\n(a)"), "# c1\n#  c2\n(a)\n");
        // A comment is only recognised where the output is at the start of a line, which it
        // is not behind a top level list: there the '#' line is treated as tokens.
        assert_eq!(pretty("(a)\n#  c1\n#c2\n(b)"), "(a)#c1#c2\n(b)\n");
        // Not at the start of a line, or inside a list: ordinary characters.
        assert_eq!(pretty("(a # b)"), "(a # b)\n");
        assert_eq!(pretty("(a\n# b\n)"), "(a # b)\n");
    }

    #[test]
    fn prettify_compact_text_properties() {
        let source = "(effects (font (size 1 1) (thickness 0.15)) (justify left))";

        assert_eq!(
            pretty(source),
            "(effects\n\t(font\n\t\t(size 1 1)\n\t\t(thickness 0.15)\n\t)\n\t(justify left)\n)\n"
        );
        assert_eq!(
            prettify(source, FormatMode::CompactTextProperties),
            "(effects\n\t(font (size 1 1) (thickness 0.15))\n\t(justify left)\n)\n"
        );
        assert_eq!(
            prettify("(a (stroke (width 0.1) (type solid)) (fill none))", FormatMode::CompactTextProperties),
            "(a\n\t(stroke (width 0.1) (type solid))\n\t(fill none)\n)\n"
        );
        // Not affected in the library table mode.
        assert_eq!(prettify(source, FormatMode::LibraryTable), pretty(source));
    }

    #[test]
    fn prettify_library_table() {
        let source = "(fp_lib_table (version 7) (lib (name \"A\")(type \"KiCad\")(uri \
                      \"${KICAD9_FOOTPRINT_DIR}/A.pretty\")(options \"\")(descr \"A long \
                      description of the library\")) (lib (name \"B\")(type \"KiCad\")(uri \
                      \"b\")(options \"\")(descr \"\")))";

        assert_eq!(
            prettify(source, FormatMode::LibraryTable),
            "(fp_lib_table\n\t(version 7)\n\t(lib (name \"A\") (type \"KiCad\") (uri \
             \"${KICAD9_FOOTPRINT_DIR}/A.pretty\") (options \"\") (descr \"A long description \
             of the library\"))\n\t(lib (name \"B\") (type \"KiCad\") (uri \"b\") (options \
             \"\") (descr \"\"))\n)\n"
        );
    }

    #[test]
    fn prettify_in_place_matches() {
        let mut text = String::from("(a (b))");
        prettify_in_place(&mut text, FormatMode::Normal);
        assert_eq!(text, "(a\n\t(b)\n)\n");
    }

    #[test]
    fn prettify_bytes_passes_non_utf8_through() {
        assert_eq!(
            prettify_bytes(b"(a \"\xff\xfe\"  (b \xc3))", FormatMode::Normal),
            b"(a \"\xff\xfe\"\n\t(b \xc3)\n)\n"
        );
    }

    #[test]
    fn prettify_long_whitespace_runs_are_linear() {
        // C++ rescans the run from every whitespace character; the port must not.
        let source = format!("(a{}(b))", " ".repeat(2_000_000));
        assert_eq!(pretty(&source), "(a\n\t(b)\n)\n");
    }
}
