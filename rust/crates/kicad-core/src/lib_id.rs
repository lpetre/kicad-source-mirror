// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2010-2012 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
// Copyright (C) 2012 Wayne Stambaugh <stambaughw@gmail.com>
// Copyright (C) 2013 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com> (core/utf8.h)
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

//! Port of `include/lib_id.h`, `common/lib_id.cpp` and `include/name_validation.h`
//! (`LIB_ID`, `GetLibIdForbiddenChars`, `GetLibFilenameForbiddenChars`).
//!
//! # `UTF8` strings
//!
//! `LIB_ID` stores its parts in `UTF8` objects (`libs/core/include/core/utf8.h`): a
//! `std::string` that is assuredly encoded in UTF-8.  That is exactly what a Rust [`String`]
//! is, so the port uses `String` / `&str` and no separate `UTF8` type:
//!
//! * `UTF8::find`, `substr`, `compare`, `size` and the byte iteration (`begin()`..`end()`)
//!   become the corresponding byte-wise operations of `str` (offsets are byte offsets);
//! * the code point iteration (`ubegin()`..`uend()`, `UTF8::uni_iter`) is `str::chars`;
//! * `UTF8::operator+=( unsigned w_ch )` is `String::push`;
//! * `wx_str()` conversions disappear.

use std::cmp::Ordering;
use std::fmt;

/// Port of `GetLibIdForbiddenChars`: characters illegal in a `LIB_ID` item name.  Library
/// nicknames use a different set (`isLegalLibraryNameChar`) and are not covered here.
///
/// This is the single source of truth for the characters KiCad forbids in library-related
/// names.  Filename rules are a strict superset of LIB_ID rules, so the filename list is
/// built from the LIB_ID list rather than hand-copied.
pub const fn get_lib_id_forbidden_chars() -> &'static str {
    "<>\"\\:\t\n\r"
}

/// Port of `GetLibFilenameForbiddenChars`: characters illegal in a footprint library
/// filename.  This is the LIB_ID set plus the path and format characters that a name may
/// contain but a filename may not.
pub const fn get_lib_filename_forbidden_chars() -> &'static str {
    // Filenames additionally forbid these path and format characters, which a LIB_ID name
    // is allowed to contain.
    //
    // GetLibIdForbiddenChars() + "%$/"
    "<>\"\\:\t\n\r%$/"
}

/// Port of the static `checkLibNickname`: the byte offset of the first `':'` in `field`, or
/// -1 (meaning success) when there is none.
fn check_lib_nickname(field: &str) -> i32 {
    // std::string::npos is largest positive number, casting to int makes it -1.
    // Returning that means success.
    match field.find(':') {
        Some(pos) => pos as i32,
        None => -1,
    }
}

/// The error of [`LibId::format_parts`]: the port of the `PARSE_ERROR` thrown by the static
/// `LIB_ID::Format( aLibraryName, aLibItemName )`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibIdParseError {
    /// The problem description (`PARSE_ERROR::ParseProblem()`).
    pub problem: String,
    /// The source of the text, here the offending library nickname.
    pub source: String,
    /// The problem line of input (`PARSE_ERROR::inputLine`).
    pub input_line: String,
    /// At which line number, 1 based index; 0 here (`PARSE_ERROR::lineNumber`).
    pub line_number: i32,
    /// The byte offset of the problem within the line (`PARSE_ERROR::byteIndex`).
    pub byte_index: i32,
}

impl fmt::Display for LibIdParseError {
    /// The text of `PARSE_ERROR::Problem()`: `"%s in '%s', line %d, offset %d."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} in '{}', line {}, offset {}.",
            self.problem, self.source, self.line_number, self.byte_index
        )
    }
}

impl std::error::Error for LibIdParseError {}

/// Port of `LIB_ID`: a logical library item identifier which consists of various portions
/// much like a URI.
///
/// It consists of a dyad of the library nickname and the name of the item in the library.
/// This is a generic library identifier that can be used for any type of library that
/// contains multiple named items such as footprint or symbol libraries.
///
/// Example LIB_ID string: `"smt:R_0805"`.
///
/// - "smt" is the logical library name used to look up library information saved in the
///   `LIBRARY_TABLE`.
/// - "R_0805" is the name of the item within the library.
///
/// Equality and ordering compare the library nickname, then the item name (byte-wise, like
/// `std::string::compare`); the sub-library name does not take part, exactly as in C++.
#[derive(Clone, Debug, Default)]
pub struct LibId {
    /// The nickname of the library or empty.
    library_name: String,
    /// The name of the entry in the logical library.
    item_name: String,
    /// Optional sub-library name used for grouping within a library.
    sub_library_name: String,
}

impl LibId {
    /// Port of `LIB_ID::LIB_ID()`: an empty id.
    pub fn new() -> Self {
        LibId::default()
    }

    // NOTE: don't define any constructors which call parse() on their arguments.  We want it
    // to be obvious to callers that parsing is involved (and that valid IDs are guaranteed in
    // the presence of disallowed characters, malformed ids, etc.).

    /// Port of `LIB_ID::LIB_ID( const wxString& aLibraryName, const wxString& aItemName )`.
    ///
    /// This is a special version which ignores the parsing due to symbol names allowing '/'
    /// as a valid character.  This was causing the symbol names to be truncated at the first
    /// occurrence of '/' in the symbol name.
    ///
    /// `library_name` is the library name used to look up the library item in the
    /// `LIBRARY_TABLE`; `item_name` is the name of the library item, which is not parsed by
    /// the standard [`LibId::parse`] function.
    pub fn from_parts(library_name: &str, item_name: &str) -> Self {
        LibId {
            library_name: library_name.to_string(),
            item_name: item_name.to_string(),
            sub_library_name: String::new(),
        }
    }

    /// Port of `LIB_ID::Parse`: parse LIB_ID with the information from `id`.
    ///
    /// A typical LIB_ID string consists of a library nickname followed by a library item
    /// name, e.g.: "smt:R_0805", or "mylib:R_0805", or "ttl:7400".
    ///
    /// `fix` indicates invalid chars should be replaced with '_' (the C++ default argument is
    /// `false`).
    ///
    /// Returns minus 1 (i.e. -1) for success; a value >= 0 indicates the character (byte)
    /// offset into `id` at which an error was detected.
    pub fn parse(&mut self, id: &str, fix: bool) -> i32 {
        self.clear();

        let part_ndx;
        let mut offset = -1;

        //=====<library nickname>=============================
        if let Some(colon) = id.find(':') {
            offset = self.set_lib_nickname(&id[..colon]);

            if offset > -1 {
                return offset;
            }

            part_ndx = colon + 1; // skip ':'
        } else {
            part_ndx = 0;
        }

        //=====<item name>====================================
        let mut fpname = id[part_ndx..].to_string();

        // Be sure the item name is valid.
        // Some chars can be found in legacy files converted files from other EDA tools.
        if fix {
            fpname = Self::fix_illegal_chars(&fpname, false);
        } else {
            offset = Self::has_illegal_chars(&fpname);
        }

        if offset > -1 {
            return offset;
        }

        self.set_lib_item_name(&fpname);

        -1
    }

    /// Port of `LIB_ID::GetLibNickname` (and of `GetUniStringLibNickname`): return the
    /// logical library name portion of a LIB_ID.
    pub fn lib_nickname(&self) -> &str {
        &self.library_name
    }

    /// Port of `LIB_ID::SetLibNickname`: override the logical library name portion of the
    /// LIB_ID to `lib_nickname`.
    ///
    /// Returns minus 1 (i.e. -1) for success; a value >= 0 indicates the character offset
    /// into the parameter at which an error was detected, usually because it contained ':'.
    /// The nickname is left unchanged in that case.
    pub fn set_lib_nickname(&mut self, lib_nickname: &str) -> i32 {
        let offset = check_lib_nickname(lib_nickname);

        if offset == -1 {
            self.library_name = lib_nickname.to_string();
        }

        offset
    }

    /// Port of `LIB_ID::GetLibItemName` (and of `GetUniStringLibItemName`): the library item
    /// name, i.e. footprintName.
    pub fn lib_item_name(&self) -> &str {
        &self.item_name
    }

    /// Port of `LIB_ID::SetLibItemName`: override the library item name portion of the LIB_ID
    /// to `lib_item_name`.  Always returns -1 (success).
    pub fn set_lib_item_name(&mut self, lib_item_name: &str) -> i32 {
        self.item_name = lib_item_name.to_string();

        -1
    }

    /// Port of `LIB_ID::GetSubLibraryName` (and of `GetUniStringSubLibraryName`).
    ///
    /// Some LIB_IDs can have a sub-library identifier in addition to a library nickname.  This
    /// identifier is *not* part of the canonical LIB_ID and is not written out / parsed.  It
    /// is only used for internal sorting/grouping, if present.
    pub fn sub_library_name(&self) -> &str {
        &self.sub_library_name
    }

    /// Port of `LIB_ID::SetSubLibraryName`.
    pub fn set_sub_library_name(&mut self, name: &str) {
        self.sub_library_name = name.to_string();
    }

    /// Port of `LIB_ID::GetFullLibraryName`: a display-formatted name of the library and
    /// sublibrary (if present).
    pub fn full_library_name(&self) -> String {
        if self.sub_library_name.is_empty() {
            return self.library_name.clone();
        }

        format!("{} - {}", self.library_name, self.sub_library_name)
    }

    /// Port of the member `LIB_ID::Format()` (and of `GetUniStringLibId`): the fully
    /// formatted text of the LIB_ID.
    pub fn format(&self) -> String {
        let mut ret = String::new();

        if !self.library_name.is_empty() {
            ret += &self.library_name;
            ret.push(':');
        }

        ret += &self.item_name;

        ret
    }

    /// Port of the static `LIB_ID::Format( aLibraryName, aLibItemName )`: a string in the
    /// proper format as an LIB_ID for a combination of `library_name` and `lib_item_name`.
    ///
    /// Fails (C++ throws `PARSE_ERROR`) if the library nickname contains ':'.
    pub fn format_parts(library_name: &str, lib_item_name: &str) -> Result<String, LibIdParseError> {
        let mut ret = String::new();

        if !library_name.is_empty() {
            let offset = check_lib_nickname(library_name);

            if offset != -1 {
                return Err(LibIdParseError {
                    problem: "Illegal character found in library nickname".to_string(),
                    source: library_name.to_string(),
                    input_line: library_name.to_string(),
                    line_number: 0,
                    byte_index: offset,
                });
            }

            ret += library_name;
            ret.push(':');
        }

        ret += lib_item_name;

        Ok(ret)
    }

    /// Port of `LIB_ID::IsValid`: check if this LID_ID is valid.
    ///
    /// A valid `LIB_ID` must have both the library nickname and the library item name
    /// defined.
    ///
    /// A return value of true does not indicate that the `LIB_ID` is a valid `LIBRARY_TABLE`
    /// entry.
    pub fn is_valid(&self) -> bool {
        !self.library_name.is_empty() && !self.item_name.is_empty()
    }

    /// Port of `LIB_ID::IsLegacy`: true if the `LIB_ID` only has the item name defined.
    pub fn is_legacy(&self) -> bool {
        self.library_name.is_empty() && !self.item_name.is_empty()
    }

    /// Port of `LIB_ID::clear`: clear the contents of the library nickname, library entry
    /// name (and sub-library name).
    pub fn clear(&mut self) {
        self.library_name.clear();
        self.item_name.clear();
        self.sub_library_name.clear();
    }

    /// Port of `LIB_ID::empty`: true if the LIB_ID is empty.
    pub fn is_empty(&self) -> bool {
        self.library_name.is_empty() && self.item_name.is_empty()
    }

    /// Port of `LIB_ID::compare`: compare the contents of LIB_ID objects by performing a
    /// `std::string` comparison of the library nickname, then of the library entry name.
    ///
    /// Returns -1 if less than `lib_id`, 1 if greater than `lib_id`, and 0 if equal to
    /// `lib_id`.  (`std::string::compare` only guarantees the sign of its result; the port
    /// normalises it to -1 / 0 / 1.)
    pub fn compare(&self, lib_id: &LibId) -> i32 {
        // Don't bother comparing the same object.
        if std::ptr::eq(self, lib_id) {
            return 0;
        }

        let retv = self.library_name.as_bytes().cmp(lib_id.library_name.as_bytes());

        let ordering = if retv != Ordering::Equal {
            retv
        } else {
            self.item_name.as_bytes().cmp(lib_id.item_name.as_bytes())
        };

        match ordering {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    /// Port of `LIB_ID::HasIllegalChars`: examine `lib_item_name` for invalid `LIB_ID` item
    /// name characters.
    ///
    /// Returns the (byte) offset of the first illegal character, otherwise -1.
    pub fn has_illegal_chars(lib_item_name: &str) -> i32 {
        // The C++ loop walks the bytes of the UTF8 string.  All forbidden characters are
        // ASCII and no byte of a multi-byte sequence can be mistaken for one of them.
        for (offset, ch) in lib_item_name.bytes().enumerate() {
            if ch.is_ascii() && !Self::is_legal_char(u32::from(ch)) {
                return offset as i32;
            }
        }

        -1
    }

    /// Port of `LIB_ID::FixIllegalChars`: replace illegal `LIB_ID` item name characters with
    /// underscores '_'.
    ///
    /// `lib` is true if we are checking library names, false if we are checking item names.
    /// Returns the corrected version of `lib_item_name`.
    pub fn fix_illegal_chars(lib_item_name: &str, lib: bool) -> String {
        let mut fixed_name = String::with_capacity(lib_item_name.len());

        for ch in lib_item_name.chars() {
            let legal = if lib {
                Self::is_legal_library_name_char(u32::from(ch))
            } else {
                Self::is_legal_char(u32::from(ch))
            };

            fixed_name.push(if legal { ch } else { '_' });
        }

        fixed_name
    }

    /// Port of `LIB_ID::FindIllegalLibraryNameChar`: looks for characters that are illegal in
    /// library nicknames.
    ///
    /// Returns the invalid character found in the name, or 0 if the name is valid.
    pub fn find_illegal_library_name_char(library_name: &str) -> u32 {
        // The C++ loop walks the bytes of the UTF8 string; the bytes of multi-byte sequences
        // are always legal.
        for ch in library_name.bytes() {
            if ch.is_ascii() && !Self::is_legal_library_name_char(u32::from(ch)) {
                return u32::from(ch);
            }
        }

        0
    }

    /// Port of `LIB_ID::isLegalChar`: tests whether a Unicode character is a legal LIB_ID
    /// item name character.
    ///
    /// The criteria for legal LIB_ID character is as follows:
    /// - For both symbol and footprint names, neither '/' or ':' are legal.  They are
    ///   reserved characters used by `LIB_ID::Parse`.
    /// - Spaces are allowed in footprint names as they are a legal filename character on all
    ///   operating systems.
    /// - Spaces are not allowed in symbol names since symbol names are not quoted in the
    ///   schematic or symbol library file formats.
    /// - Spaces are allowed in footprint library nicknames as they are quoted in the
    ///   footprint library table file format.
    /// - Spaces are now also allowed in symbol library nicknames since they are quoted in the
    ///   new symbol library sexpr file format.
    /// - Illegal file name characters are not allowed in footprint names since the file name
    ///   is the footprint name.
    /// - Illegal file name characters except '/' are allowed in symbol names since the name
    ///   is not the file name.
    ///
    /// (The list above is the historical C++ documentation; the test actually performed is
    /// membership in [`get_lib_id_forbidden_chars`].)
    ///
    /// `uni_char` is expected to be a 32 bit Unicode character, not a UTF8 char, that use a
    /// variable length coding value.
    pub fn is_legal_char(uni_char: u32) -> bool {
        !get_lib_id_forbidden_chars()
            .chars()
            .any(|forbidden| u32::from(forbidden) == uni_char)
    }

    /// Port of `LIB_ID::isLegalLibraryNameChar`: tests whether a Unicode character is a legal
    /// LIB_ID library nickname character.
    ///
    /// `uni_char` is expected to be a 32 bit Unicode character, not a UTF8 char, that use a
    /// variable length coding value.
    pub fn is_legal_library_name_char(uni_char: u32) -> bool {
        let space_allowed = true;

        if uni_char < u32::from(' ') {
            return false;
        }

        match char::from_u32(uni_char) {
            Some('\\') | Some(':') => false,
            Some(' ') => space_allowed,
            _ => true,
        }
    }
}

impl PartialEq for LibId {
    /// Port of `LIB_ID::operator==` (`compare() == 0`).
    fn eq(&self, other: &Self) -> bool {
        self.compare(other) == 0
    }
}

impl Eq for LibId {}

impl PartialOrd for LibId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LibId {
    /// Port of `LIB_ID::operator<` and `operator>` (the sign of `compare()`).
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other).cmp(&0)
    }
}

impl std::hash::Hash for LibId {
    /// Consistent with equality: the sub-library name is not hashed.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.library_name.hash(state);
        self.item_name.hash(state);
    }
}

impl fmt::Display for LibId {
    /// Writes [`LibId::format`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errors_and_fixing() {
        let mut id = LibId::new();

        // An illegal character in the item name: offset within the item name.
        assert_eq!(id.parse("lib:ab<cd", false), 2);
        assert!(id.lib_item_name().is_empty());
        assert_eq!(id.lib_nickname(), "lib");

        // With fixing the character is replaced.  Only the first ':' separates the nickname.
        assert_eq!(id.parse("lib:ab<c:d\"e", true), -1);
        assert_eq!(id.lib_nickname(), "lib");
        assert_eq!(id.lib_item_name(), "ab_c_d_e");

        // '/' is legal in item names.
        assert_eq!(id.parse("lib:a/b", false), -1);
        assert_eq!(id.lib_item_name(), "a/b");

        // Empty nickname.
        assert_eq!(id.parse(":item", false), -1);
        assert!(id.is_legacy());
        assert_eq!(id.format(), "item");

        // Non-ASCII text is legal; offsets are byte offsets.
        assert_eq!(id.parse("Bibliothèque:Résistance", false), -1);
        assert_eq!(id.lib_nickname(), "Bibliothèque");
        assert_eq!(id.lib_item_name(), "Résistance");
        assert_eq!(LibId::has_illegal_chars("é:"), 2);
    }

    #[test]
    fn nickname_checks() {
        let mut id = LibId::from_parts("lib", "item");

        assert_eq!(id.set_lib_nickname("a:b"), 1);
        assert_eq!(id.lib_nickname(), "lib");
        assert_eq!(id.set_lib_nickname("other"), -1);
        assert_eq!(id.lib_nickname(), "other");

        assert_eq!(LibId::find_illegal_library_name_char("my lib"), 0);
        assert_eq!(LibId::find_illegal_library_name_char("my:lib"), u32::from(':'));
        assert_eq!(LibId::find_illegal_library_name_char("my\\lib"), u32::from('\\'));
        assert_eq!(LibId::find_illegal_library_name_char("my\tlib"), u32::from('\t'));
        assert_eq!(LibId::find_illegal_library_name_char("bibliothèque"), 0);

        assert_eq!(LibId::fix_illegal_chars("a:b\\c d\te", true), "a_b_c d_e");
        assert_eq!(LibId::fix_illegal_chars("a:b\\c d\te<é>", false), "a_b_c d_e_é_");
    }

    #[test]
    fn static_format() {
        assert_eq!(LibId::format_parts("lib", "item").unwrap(), "lib:item");
        assert_eq!(LibId::format_parts("", "item").unwrap(), "item");

        let err = LibId::format_parts("l:ib", "item").unwrap_err();
        assert_eq!(err.byte_index, 1);
        assert_eq!(
            err.to_string(),
            "Illegal character found in library nickname in 'l:ib', line 0, offset 1."
        );
    }

    #[test]
    fn comparison_ignores_sub_library() {
        let mut a = LibId::from_parts("lib", "item");
        let b = LibId::from_parts("lib", "item");
        a.set_sub_library_name("sub");

        assert_eq!(a, b);
        assert_eq!(a.compare(&b), 0);
        assert_eq!(a.full_library_name(), "lib - sub");
        assert_eq!(b.full_library_name(), "lib");
        assert_eq!(a.sub_library_name(), "sub");

        let c = LibId::from_parts("lib", "itemz");
        let d = LibId::from_parts("lia", "zzz");
        assert_eq!(a.compare(&c), -1);
        assert_eq!(c.compare(&a), 1);
        assert!(d < a);
        assert!(c > a);
        assert_eq!(a.to_string(), "lib:item");

        a.clear();
        assert!(a.is_empty());
        assert!(a.sub_library_name().is_empty());
    }
}
