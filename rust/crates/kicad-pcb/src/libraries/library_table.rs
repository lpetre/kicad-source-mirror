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

//! Port of `common/libraries/library_table.cpp`, `common/libraries/library_table_parser.cpp`,
//! `include/libraries/library_table.h` and the PEGTL grammar of
//! `include/libraries/library_table_grammar.h` (package DRC-LP).
//!
//! The grammar is ported as a hand-written backtracking parser over the bytes of the file
//! that accepts exactly the language of `LIB_TABLE_FILE` and applies the same actions
//! (`LIBRARY_TABLE_PARSER_ACTION`): the last value of a repeated property wins, `(hidden)` /
//! `(disabled)` set their flag, quoted strings are unescaped as the `QUOTED_TEXT` action does.

use std::collections::BTreeMap;

use super::LibrarySource;

/// `LIBRARY_TABLE_TYPE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LibraryTableType {
    #[default]
    Uninitialized,
    Symbol,
    Footprint,
    DesignBlock,
}

/// `LIBRARY_TABLE_SCOPE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LibraryTableScope {
    #[default]
    Uninitialized,
    Global,
    Project,
    Both,
}

/// `LIBRARY_TABLE_ROW`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryTableRow {
    pub nickname: String,          // m_nickname
    pub uri: String,               // m_uri
    pub type_: String,             // m_type
    pub options: String,           // m_options
    pub description: String,       // m_description
    pub disabled: bool,            // m_disabled
    pub hidden: bool,              // m_hidden
    pub ok: bool,                  // m_ok
    pub error_description: String, // m_errorDescription
    pub scope: LibraryTableScope,  // m_scope
}

impl LibraryTableRow {
    /// `LIBRARY_TABLE_ROW::TABLE_TYPE_NAME`: the row type of a nested table reference.
    pub const TABLE_TYPE_NAME: &'static str = "Table";

    /// `LIBRARY_TABLE_ROW::GetOptionsMap()`.
    pub fn options_map(&self) -> BTreeMap<String, String> {
        LibraryTable::parse_options(&self.options)
    }
}

/// `LIBRARY_TABLE_ROW_IR`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryTableRowIr {
    pub nickname: String,
    pub uri: String,
    pub type_: String,
    pub options: String,
    pub description: String,
    pub hidden: bool,
    pub disabled: bool,
}

/// `LIBRARY_TABLE_IR`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryTableIr {
    pub type_: LibraryTableType,
    pub version: String,
    pub rows: Vec<LibraryTableRowIr>,
}

/// `LIBRARY_PARSE_ERROR`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryParseError {
    pub description: String,
    pub line: usize,
    pub column: usize,
}

/// `LIBRARY_TABLE`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryTable {
    pub path: String,               // m_path
    pub scope: LibraryTableScope,   // m_scope
    pub type_: LibraryTableType,    // m_type
    pub version: Option<i32>,       // m_version
    pub rows: Vec<LibraryTableRow>, // m_rows
    pub ok: bool,                   // m_ok
    pub error_description: String,  // m_errorDescription
}

/// Outcome of a grammar rule: no match (the input is rewound), or a raised `parse_error`
/// (an `if_must` / `must` failure, which aborts the whole parse).
enum Fail {
    NoMatch,
    Raise(usize),
}

type PResult<T> = Result<T, Fail>;

/// The `LIB_TABLE_FILE` grammar with the `LIBRARY_TABLE_PARSER_ACTION` actions.
struct GrammarParser<'a> {
    input: &'a [u8],
    pos: usize,
    model: LibraryTableIr,
    current_row: LibraryTableRowIr,
}

/// The fields `DEFINE_STRING_ACTION` points `target_string` at.
#[derive(Clone, Copy)]
enum Target {
    Version,
    Name,
    Type,
    Uri,
    Options,
    Descr,
}

/// PEGTL `space`: `' '`, `'\n'`, `'\r'`, `'\t'`, `'\v'`, `'\f'`.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | 0x0b | 0x0c)
}

impl<'a> GrammarParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn one(&mut self, c: u8) -> PResult<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(Fail::NoMatch)
        }
    }

    /// `TAO_PEGTL_STRING( s )`.
    fn string(&mut self, s: &str) -> PResult<()> {
        if self.input[self.pos..].starts_with(s.as_bytes()) {
            self.pos += s.len();
            Ok(())
        } else {
            Err(Fail::NoMatch)
        }
    }

    /// `star< space >`.
    fn star_space(&mut self) {
        while self.peek().is_some_and(is_space) {
            self.pos += 1;
        }
    }

    /// `plus< space >`.
    fn plus_space(&mut self) -> PResult<()> {
        if !self.peek().is_some_and(is_space) {
            return Err(Fail::NoMatch);
        }

        self.star_space();
        Ok(())
    }

    /// `must< R >`: a no-match becomes a raised error at the current position.
    fn must<T>(&mut self, r: PResult<T>) -> PResult<T> {
        match r {
            Err(Fail::NoMatch) => Err(Fail::Raise(self.pos)),
            other => other,
        }
    }

    /// Run `f`; on no-match rewind the input (PEGTL's rewind marker).
    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let start = self.pos;
        let r = f(self);

        if matches!(r, Err(Fail::NoMatch)) {
            self.pos = start;
        }

        r
    }

    fn set_target(&mut self, target: Target, value: String) {
        match target {
            Target::Version => self.model.version = value,
            Target::Name => self.current_row.nickname = value,
            Target::Type => self.current_row.type_ = value,
            Target::Uri => self.current_row.uri = value,
            Target::Options => self.current_row.options = value,
            Target::Descr => self.current_row.description = value,
        }
    }

    /// `QUOTED_TEXT : if_must< one< '"' >, until< one< '"' >, sor< ESCAPE, any > > >` with
    /// its action (the unescaping of `\n`, `\r`, `\\`, `\"`).
    fn quoted_text(&mut self) -> PResult<String> {
        let start = self.pos;
        self.one(b'"')?;

        loop {
            match self.peek() {
                None => return Err(Fail::Raise(self.pos)),
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') if self.pos + 1 < self.input.len() => self.pos += 2,
                Some(_) => self.pos += 1,
            }
        }

        let inner = &self.input[start + 1..self.pos - 1];
        let mut unescaped: Vec<u8> = Vec::with_capacity(inner.len());
        let mut ii = 0;

        while ii < inner.len() {
            if inner[ii] != b'\\' || ii + 1 == inner.len() {
                unescaped.push(inner[ii]);
            } else {
                ii += 1;

                match inner[ii] {
                    b'n' => unescaped.push(b'\n'),
                    b'r' => unescaped.push(b'\r'),
                    b'\\' => unescaped.push(b'\\'),
                    b'"' => unescaped.push(b'"'),
                    other => {
                        unescaped.push(b'\\');
                        unescaped.push(other);
                    }
                }
            }

            ii += 1;
        }

        // wxString::FromUTF8 of invalid UTF-8 is an empty string (addRowFromIR)
        Ok(String::from_utf8(unescaped).unwrap_or_default())
    }

    /// `TOKEN : plus< not_one< '(', ')', ' ', '\t', '\n', '\r' > >` with its action.
    fn token(&mut self) -> PResult<String> {
        let start = self.pos;

        while self
            .peek()
            .is_some_and(|c| !matches!(c, b'(' | b')' | b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.pos += 1;
        }

        if self.pos == start {
            return Err(Fail::NoMatch);
        }

        Ok(String::from_utf8(self.input[start..self.pos].to_vec()).unwrap_or_default())
    }

    /// `PROPERTY_VALUE : sor< QUOTED_TEXT, TOKEN >`.
    fn property_value(&mut self) -> PResult<String> {
        match self.quoted_text() {
            Err(Fail::NoMatch) => self.token(),
            other => other,
        }
    }

    /// `LIB_PROPERTY : seq< LPAREN, LIB_PROPERTY_KEY, plus< space >, PROPERTY_VALUE, RPAREN >`.
    fn lib_property(&mut self) -> PResult<()> {
        self.attempt(|p| {
            p.one(b'(')?;

            let target = if p.string("name").is_ok() {
                Target::Name
            } else if p.string("type").is_ok() {
                Target::Type
            } else if p.string("uri").is_ok() {
                Target::Uri
            } else if p.string("options").is_ok() {
                Target::Options
            } else if p.string("descr").is_ok() {
                Target::Descr
            } else {
                return Err(Fail::NoMatch);
            };

            p.plus_space()?;
            let value = p.property_value()?;
            p.one(b')')?;
            p.set_target(target, value);
            Ok(())
        })
    }

    /// `HIDDEN_MARKER` / `DISABLED_MARKER : seq< LPAREN, KEYWORD, RPAREN >`.
    fn marker(&mut self, keyword: &str) -> PResult<()> {
        self.attempt(|p| {
            p.one(b'(')?;
            p.string(keyword)?;
            p.one(b')')
        })
    }

    /// `LIB_ROW_MEMBER : sor< LIB_PROPERTY, HIDDEN_MARKER, DISABLED_MARKER >`.
    fn lib_row_member(&mut self) -> PResult<()> {
        match self.lib_property() {
            Err(Fail::NoMatch) => {}
            other => return other,
        }

        match self.marker("hidden") {
            Ok(()) => {
                self.current_row.hidden = true;
                return Ok(());
            }
            Err(Fail::Raise(p)) => return Err(Fail::Raise(p)),
            Err(Fail::NoMatch) => {}
        }

        self.marker("disabled")?;
        self.current_row.disabled = true;
        Ok(())
    }

    /// `pad< R, space >`: `seq< star< space >, R, star< space > >`, rewound on no-match.
    fn pad<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        self.attempt(|p| {
            p.star_space();
            let r = f(p)?;
            p.star_space();
            Ok(r)
        })
    }

    /// `LIB_ROW : if_must< pad< LPAREN, space >, KEYWORDS::LIB, plus< space >,
    /// plus< pad< LIB_ROW_MEMBER, space > >, pad< RPAREN, space > >`, then the action that
    /// appends the row.
    fn lib_row(&mut self) -> PResult<()> {
        self.pad(|p| p.one(b'('))?;

        let r = p_string_must(self, "lib");
        r?;
        let r = self.plus_space();
        self.must(r)?;

        let r = self.pad(Self::lib_row_member);
        self.must(r)?;

        loop {
            match self.pad(Self::lib_row_member) {
                Ok(()) => {}
                Err(Fail::NoMatch) => break,
                Err(raise) => return Err(raise),
            }
        }

        let r = self.pad(|p| p.one(b')'));
        self.must(r)?;

        self.model.rows.push(std::mem::take(&mut self.current_row));
        Ok(())
    }

    /// `TABLE_VERSION : seq< LPAREN, KEYWORDS::VERSION, plus< space >, PROPERTY_VALUE, RPAREN >`.
    fn table_version(&mut self) -> PResult<()> {
        self.attempt(|p| {
            p.one(b'(')?;
            p.string("version")?;
            p.plus_space()?;
            let value = p.property_value()?;
            p.one(b')')?;
            p.set_target(Target::Version, value);
            Ok(())
        })
    }

    /// `LIB_TABLE_FILE : seq< LIB_TABLE, eof >` with `LIB_TABLE : if_must< LPAREN, TABLE_TYPE,
    /// pad_opt< TABLE_VERSION, space >, star< pad< LIB_ROW, space > >, pad< RPAREN, space > >`.
    fn lib_table_file(&mut self) -> PResult<()> {
        self.one(b'(')?;

        let table_type = if self.string("sym_lib_table").is_ok() {
            LibraryTableType::Symbol
        } else if self.string("fp_lib_table").is_ok() {
            LibraryTableType::Footprint
        } else if self.string("design_block_lib_table").is_ok() {
            LibraryTableType::DesignBlock
        } else {
            return Err(Fail::Raise(self.pos));
        };

        self.model.type_ = table_type;

        // pad_opt< TABLE_VERSION, space >: seq< star< space >, opt< TABLE_VERSION, star< space > > >
        self.star_space();

        if self.table_version().is_ok() {
            self.star_space();
        }

        loop {
            match self.pad(Self::lib_row) {
                Ok(()) => {}
                Err(Fail::NoMatch) => break,
                Err(raise) => return Err(raise),
            }
        }

        let r = self.pad(|p| p.one(b')'));
        self.must(r)?;

        if self.pos != self.input.len() {
            // seq< LIB_TABLE, eof >: a plain (local) failure
            return Err(Fail::NoMatch);
        }

        Ok(())
    }

    /// 1-based line and column of byte `pos` (`tao::pegtl::position`).
    fn line_column(&self, pos: usize) -> (usize, usize) {
        let before = &self.input[..pos.min(self.input.len())];
        let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
        let line_start = before
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |p| p + 1);
        (line, pos - line_start + 1)
    }
}

/// `if_must< ..., KEYWORDS::LIB, ... >`: the keyword after the condition must match.
fn p_string_must(p: &mut GrammarParser<'_>, keyword: &str) -> PResult<()> {
    let r = p.string(keyword);
    p.must(r)
}

/// Port of `LIBRARY_TABLE_PARSER::ParseBuffer` (and the body of `Parse` after the file has
/// been read). `path` is used in the "unexpected error" message of `Parse`; `None` gives the
/// `ParseBuffer` message.
pub fn parse_library_table(
    data: &[u8],
    path: Option<&str>,
) -> Result<LibraryTableIr, LibraryParseError> {
    let mut parser = GrammarParser {
        input: data,
        pos: 0,
        model: LibraryTableIr::default(),
        current_row: LibraryTableRowIr::default(),
    };

    match parser.lib_table_file() {
        Ok(()) => Ok(parser.model),
        Err(Fail::NoMatch) => Err(LibraryParseError {
            description: match path {
                Some(path) => {
                    format!("An unexpected error occurred while reading library table {path} ")
                }
                None => "An unexpected error occurred while reading library table".to_string(),
            },
            line: 0,
            column: 0,
        }),
        Err(Fail::Raise(pos)) => {
            let (line, column) = parser.line_column(pos);

            Err(LibraryParseError {
                description: format!("Syntax error at line {line}, column {column}"),
                line,
                column,
            })
        }
    }
}

/// `boost::lexical_cast<int>( aIR.version )`: the whole string an `int`.
fn lexical_cast_int(text: &str) -> Option<i32> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);

    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }

    text.parse::<i32>().ok()
}

impl LibraryTable {
    /// Port of `LIBRARY_TABLE::LIBRARY_TABLE( const wxFileName& aPath, LIBRARY_TABLE_SCOPE aScope,
    /// LIBRARY_TABLE_TYPE aExpectedType )`: the table file `path` read through `source`.
    pub fn from_file(
        source: &dyn LibrarySource,
        path: &str,
        scope: LibraryTableScope,
        expected_type: LibraryTableType,
    ) -> LibraryTable {
        let mut table = LibraryTable {
            scope,
            ..Default::default()
        };

        // WX_FILENAME::ResolvePossibleSymlinks( fn ); m_path = fn.GetAbsolutePath()
        let resolved = source.resolve_symlinks(path);
        table.path = resolved.clone();

        let Some(data) = source.read_file(&resolved) else {
            table.ok = false;
            table.error_description = format!("The library table path '{resolved}' does not exist");
            return table;
        };

        if data.is_empty() {
            // blank files are treated as "new" and "ok" as we want to allow the user to start
            // fleshing them out and save
            table.ok = true;
            table.type_ = expected_type;
            return table;
        }

        match parse_library_table(&data, Some(&table.path)) {
            Ok(ir) => {
                if expected_type != LibraryTableType::Uninitialized && ir.type_ != expected_type {
                    table.ok = false;
                    table.error_description = "The library table is of wrong type".to_string();
                    return table;
                }

                table.ok = table.init_from_ir(&ir);
            }
            Err(error) => {
                table.ok = false;
                table.error_description = error.description;
            }
        }

        table
    }

    /// Port of `LIBRARY_TABLE::LIBRARY_TABLE( bool aFromClipboard, const wxString& aBuffer,
    /// LIBRARY_TABLE_SCOPE aScope )`.
    pub fn from_buffer(buffer: &str, scope: LibraryTableScope) -> LibraryTable {
        let mut table = LibraryTable {
            scope,
            ..Default::default()
        };

        match parse_library_table(buffer.as_bytes(), None) {
            Ok(ir) => table.ok = table.init_from_ir(&ir),
            Err(error) => {
                table.ok = false;
                table.error_description = error.description;
            }
        }

        table
    }

    /// Port of `LIBRARY_TABLE::initFromIR`.
    fn init_from_ir(&mut self, ir: &LibraryTableIr) -> bool {
        self.type_ = ir.type_;
        self.version = lexical_cast_int(&ir.version);

        for row in &ir.rows {
            self.add_row_from_ir(row);
        }

        true
    }

    /// Port of `LIBRARY_TABLE::addRowFromIR`.
    fn add_row_from_ir(&mut self, ir: &LibraryTableRowIr) -> bool {
        self.rows.push(LibraryTableRow {
            nickname: ir.nickname.clone(),
            uri: ir.uri.clone(),
            type_: ir.type_.clone(),
            options: ir.options.clone(),
            description: ir.description.clone(),
            hidden: ir.hidden,
            disabled: ir.disabled,
            ok: true,
            error_description: String::new(),
            scope: self.scope,
        });

        true
    }

    /// `LIBRARY_TABLE::IsOk()`.
    pub fn is_ok(&self) -> bool {
        self.ok
    }

    /// Port of `LIBRARY_TABLE::HasRow`.
    pub fn has_row(&self, nickname: &str) -> bool {
        self.rows.iter().any(|row| row.nickname == nickname)
    }

    /// Port of `LIBRARY_TABLE::Row( aNickname )`.
    pub fn row(&self, nickname: &str) -> Option<&LibraryTableRow> {
        self.rows.iter().find(|row| row.nickname == nickname)
    }

    /// Port of `LIBRARY_TABLE::ParseOptions`: `name=value` pairs separated by `|` (`\|` is
    /// an escaped separator), leading white space skipped, a name without `=` maps to "".
    pub fn parse_options(options_list: &str) -> BTreeMap<String, String> {
        const OPT_SEP: u8 = b'|';
        let mut props = BTreeMap::new();
        let bytes = options_list.as_bytes();
        let end = bytes.len();
        let mut cp = 0;

        while cp < end {
            let mut pair: Vec<u8> = Vec::new();

            // Skip leading white space (C isspace)
            while cp < end && matches!(bytes[cp], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                cp += 1;
            }

            // Find the end of pair/field
            while cp < end {
                if bytes[cp] == b'\\' && cp + 1 < end && bytes[cp + 1] == OPT_SEP {
                    cp += 1; // skip the escape
                    pair.push(bytes[cp]); // add the separator
                    cp += 1;
                } else if bytes[cp] == OPT_SEP {
                    cp += 1; // skip the separator
                    break; // process the pair
                } else {
                    pair.push(bytes[cp]);
                    cp += 1;
                }
            }

            // stash the pair
            if !pair.is_empty() {
                let pair = String::from_utf8_lossy(&pair).into_owned();

                // first equals sign separates 'name' and 'value'.
                match pair.find('=') {
                    Some(eq) => {
                        props.insert(pair[..eq].to_string(), pair[eq + 1..].to_string());
                    }
                    None => {
                        props.insert(pair, String::new()); // present, but with no value.
                    }
                }
            }
        }

        props
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_demo_table() {
        let text = "(fp_lib_table\n  (version 7)\n  (lib (name \"CM5IO\")(type \"KiCad\")(uri \"${KIPRJMOD}/CM5IO.pretty\")(options \"\")(descr \"\"))\n)\n";
        let ir = parse_library_table(text.as_bytes(), None).unwrap();
        assert_eq!(ir.type_, LibraryTableType::Footprint);
        assert_eq!(ir.version, "7");
        assert_eq!(ir.rows.len(), 1);
        assert_eq!(ir.rows[0].nickname, "CM5IO");
        assert_eq!(ir.rows[0].uri, "${KIPRJMOD}/CM5IO.pretty");
    }

    #[test]
    fn legacy_and_flags() {
        // no version, unquoted tokens, markers, spaces inside the row parentheses
        let text = "(fp_lib_table ( lib (name a)(type KiCad)(uri /x/a.pretty)(disabled)(hidden) ) (lib (name \"b\\\"c\")(type Table)(uri sub/fp-lib-table)))";
        let ir = parse_library_table(text.as_bytes(), None).unwrap();
        assert_eq!(ir.version, "");
        assert_eq!(ir.rows.len(), 2);
        assert!(ir.rows[0].disabled && ir.rows[0].hidden);
        assert_eq!(ir.rows[1].nickname, "b\"c");
        assert_eq!(ir.rows[1].type_, "Table");
    }

    #[test]
    fn syntax_errors() {
        // unknown member, leading white space, missing close
        for text in [
            "(fp_lib_table (lib (name a)(foo b)))",
            " (fp_lib_table)",
            "(fp_lib_table (lib (name a))",
            "(fp_lib_table (library (name a)))",
        ] {
            assert!(
                parse_library_table(text.as_bytes(), None).is_err(),
                "{text}"
            );
        }

        let e = parse_library_table(b"(fp_lib_table\n (lib (name a)(foo b)))", None).unwrap_err();
        assert_eq!((e.line, e.column), (2, 15));
        assert!(parse_library_table(b"(fp_lib_table)\n\n", None).is_ok());
    }

    #[test]
    fn options() {
        let m = LibraryTable::parse_options(" a=1|b|c=x\\|y");
        assert_eq!(m.get("a").map(String::as_str), Some("1"));
        assert_eq!(m.get("b").map(String::as_str), Some(""));
        assert_eq!(m.get("c").map(String::as_str), Some("x|y"));
    }
}
