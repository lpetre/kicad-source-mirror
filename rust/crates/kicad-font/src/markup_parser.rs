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

//! Port of `include/markup_parser.h` and `common/markup_parser.cpp`: the text markup grammar
//! (`^{superscript}`, `_{subscript}`, `~{overbar}`, URLs) and its parse tree.
//!
//! The C++ uses a PEGTL grammar with a `parse_tree` selector.  This is a hand-written
//! recursive-descent parser that reproduces the PEG semantics exactly (ordered choice with
//! backtracking, `until`, `plus`, `not_at`) and the tree the selector builds:
//!
//! * `anyString`, `anyStringWithinBraces` and `url` nodes are kept with their matched text
//!   (`store_content`);
//! * `subscript`, `superscript` and `overbar` nodes are kept without content, and dropped when
//!   they have no children (`discard_empty`);
//! * every other rule is transparent (its children move to the parent);
//! * nodes created while matching inside a `not_at` are always discarded (the enclosing
//!   unselected `seq` fails).
//!
//! The grammar (from `markup_parser.h`):
//!
//! ```text
//! markup                = subscript / superscript / overbar
//! anyString             = ( !markup !url utf8::any )+
//! escapeSequence        = '{' identifier '}'
//! anyStringWithinBraces = ( !markup escapeSequence / !markup utf8::not_one<'}'> )+
//! url                   = ( "http://" / "https://" ) utf8::not_one<' '>+
//! braces<C>             = C '{' until< '}', anyStringWithinBraces / subscript / superscript / overbar >
//! superscript = braces<'^'>   subscript = braces<'_'>   overbar = braces<'~'>
//! anything              = anyString / url / subscript / superscript / overbar
//! grammar               = until< eof, anything >
//! ```

use std::cell::RefCell;

/// The rule a [`MarkupNode`] was created by (`basic_node::type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupNodeType {
    /// The root node (no rule).
    Root,
    /// `MARKUP::anyString`
    AnyString,
    /// `MARKUP::anyStringWithinBraces`
    AnyStringWithinBraces,
    /// `MARKUP::url`
    Url,
    /// `MARKUP::subscript`
    Subscript,
    /// `MARKUP::superscript`
    Superscript,
    /// `MARKUP::overbar`
    Overbar,
}

impl MarkupNodeType {
    /// The demangled PEGTL rule name (`basic_node::type`), empty for the root.
    pub fn rule_name(self) -> &'static str {
        match self {
            MarkupNodeType::Root => "",
            MarkupNodeType::AnyString => "MARKUP::anyString",
            MarkupNodeType::AnyStringWithinBraces => "MARKUP::anyStringWithinBraces",
            MarkupNodeType::Url => "MARKUP::url",
            MarkupNodeType::Subscript => "MARKUP::subscript",
            MarkupNodeType::Superscript => "MARKUP::superscript",
            MarkupNodeType::Overbar => "MARKUP::overbar",
        }
    }
}

/// Port of `MARKUP::NODE` (a PEGTL `parse_tree::basic_node`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupNode {
    /// The node type (`is_type<...>()`, `is_root()`).
    pub type_: MarkupNodeType,
    /// The matched text for nodes that store their content (`has_content()`, `string()`).
    pub content: Option<String>,
    /// The child nodes in source order.
    pub children: Vec<MarkupNode>,
}

impl MarkupNode {
    fn new(type_: MarkupNodeType, content: Option<String>, children: Vec<MarkupNode>) -> Self {
        MarkupNode {
            type_,
            content,
            children,
        }
    }

    /// `basic_node::is_root()`
    pub fn is_root(&self) -> bool {
        self.type_ == MarkupNodeType::Root
    }

    /// `NODE::isOverbar()`
    pub fn is_overbar(&self) -> bool {
        self.type_ == MarkupNodeType::Overbar
    }

    /// `NODE::isSubscript()`
    pub fn is_subscript(&self) -> bool {
        self.type_ == MarkupNodeType::Subscript
    }

    /// `NODE::isSuperscript()`
    pub fn is_superscript(&self) -> bool {
        self.type_ == MarkupNodeType::Superscript
    }

    /// `NODE::isURL()`
    pub fn is_url(&self) -> bool {
        self.type_ == MarkupNodeType::Url
    }

    /// `basic_node::has_content()`
    pub fn has_content(&self) -> bool {
        self.content.is_some()
    }

    /// The matched text (`basic_node::string()`, `NODE::asWxString()`), `None` without
    /// content.
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    /// Port of `NODE::typeString()`.
    pub fn type_string(&self) -> &'static str {
        match self.type_ {
            MarkupNodeType::Subscript => "SUBSCRIPT",
            MarkupNodeType::Superscript => "SUPERSCRIPT",
            MarkupNodeType::Overbar => "OVERBAR",
            MarkupNodeType::AnyString => "ANYSTRING",
            MarkupNodeType::Url => "URL",
            MarkupNodeType::AnyStringWithinBraces => "ANYSTRINGWITHINBRACES",
            MarkupNodeType::Root => "other",
        }
    }

    /// Port of `NODE::asString()`: the rule name followed by the quoted content, if any.
    pub fn as_string(&self) -> String {
        let mut os = self.type_.rule_name().to_string();

        if let Some(content) = &self.content {
            os.push_str(" \"");
            os.push_str(content);
            os.push('"');
        }

        os
    }
}

/// Port of `MARKUP::MARKUP_PARSER`.
#[derive(Clone, Debug)]
pub struct MarkupParser<'a> {
    source: &'a str,
    /// Memo of `markup` results per start position.  The rule is a pure function of the
    /// position, so this only avoids the exponential re-parsing of nested unterminated markup
    /// (PEGTL re-parses); the result is unchanged.
    markup_memo: RefCell<Vec<Option<Match>>>,
}

/// A successful match: end position and the nodes the rule contributes to its parent.
type Match = Option<(usize, Vec<MarkupNode>)>;

impl<'a> MarkupParser<'a> {
    /// Port of `MARKUP_PARSER( const std::string& source )`.
    pub fn new(source: &'a str) -> Self {
        MarkupParser {
            source,
            markup_memo: RefCell::new(vec![None; source.len() + 1]),
        }
    }

    /// Port of `MARKUP_PARSER::Parse()`: the parse tree, or `None` when the grammar does not
    /// match (which cannot happen for valid UTF-8 input; the C++ also returns `nullptr` on a
    /// PEGTL `parse_error`, which this grammar never raises).
    pub fn parse(&self) -> Option<MarkupNode> {
        let (_, children) = self.grammar(0)?;

        Some(MarkupNode::new(MarkupNodeType::Root, None, children))
    }

    fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// `utf8::any`: the length of the code point at `pos`, `None` at the end.
    fn any_char(&self, pos: usize) -> Option<usize> {
        self.source[pos..].chars().next().map(char::len_utf8)
    }

    fn at_byte(&self, pos: usize, b: u8) -> bool {
        self.bytes().get(pos) == Some(&b)
    }

    /// `grammar = until< eof, anything >`
    fn grammar(&self, pos: usize) -> Match {
        let mut p = pos;
        let mut nodes = Vec::new();

        loop {
            if p == self.source.len() {
                return Some((p, nodes));
            }

            let (end, mut n) = self.anything(p)?;
            nodes.append(&mut n);
            p = end;
        }
    }

    /// `anything = sor< anyString, url, subscript, superscript, overbar >`
    fn anything(&self, pos: usize) -> Match {
        self.any_string(pos)
            .or_else(|| self.url(pos))
            .or_else(|| self.braces(pos, b'_', MarkupNodeType::Subscript))
            .or_else(|| self.braces(pos, b'^', MarkupNodeType::Superscript))
            .or_else(|| self.braces(pos, b'~', MarkupNodeType::Overbar))
    }

    /// `markup = sor< subscript, superscript, overbar >`
    fn markup(&self, pos: usize) -> Match {
        if let Some(cached) = &self.markup_memo.borrow()[pos] {
            return cached.clone();
        }

        let result = self.markup_uncached(pos);
        self.markup_memo.borrow_mut()[pos] = Some(result.clone());
        result
    }

    fn markup_uncached(&self, pos: usize) -> Match {
        self.braces(pos, b'_', MarkupNodeType::Subscript)
            .or_else(|| self.braces(pos, b'^', MarkupNodeType::Superscript))
            .or_else(|| self.braces(pos, b'~', MarkupNodeType::Overbar))
    }

    /// `not_at< markup >` (the nodes of a successful inner match are discarded).
    fn not_at_markup(&self, pos: usize) -> bool {
        self.markup(pos).is_none()
    }

    /// `anyString = plus< seq< not_at< markup >, not_at< url >, utf8::any > >`
    fn any_string(&self, pos: usize) -> Match {
        let mut p = pos;

        while self.not_at_markup(p) && self.url(p).is_none() {
            match self.any_char(p) {
                Some(len) => p += len,
                None => break,
            }
        }

        if p == pos {
            return None;
        }

        let node = MarkupNode::new(
            MarkupNodeType::AnyString,
            Some(self.source[pos..p].to_string()),
            Vec::new(),
        );

        Some((p, vec![node]))
    }

    /// `escapeSequence = seq< string<'{'>, identifier, string<'}'> >`
    fn escape_sequence(&self, pos: usize) -> Option<usize> {
        let b = self.bytes();

        if !self.at_byte(pos, b'{') {
            return None;
        }

        let mut p = pos + 1;

        // identifier = seq< identifier_first, star< identifier_other > > (ASCII)
        match b.get(p) {
            Some(c) if c.is_ascii_alphabetic() || *c == b'_' => p += 1,
            _ => return None,
        }

        while let Some(c) = b.get(p) {
            if c.is_ascii_alphanumeric() || *c == b'_' {
                p += 1;
            } else {
                break;
            }
        }

        if !self.at_byte(p, b'}') {
            return None;
        }

        Some(p + 1)
    }

    /// `anyStringWithinBraces = plus< sor< seq< not_at< markup >, escapeSequence >,
    ///                                     seq< not_at< markup >, utf8::not_one<'}'> > > >`
    fn any_string_within_braces(&self, pos: usize) -> Match {
        let mut p = pos;

        loop {
            if !self.not_at_markup(p) {
                break;
            }

            if let Some(end) = self.escape_sequence(p) {
                p = end;
                continue;
            }

            match self.any_char(p) {
                Some(len) if !self.at_byte(p, b'}') => p += len,
                _ => break,
            }
        }

        if p == pos {
            return None;
        }

        let node = MarkupNode::new(
            MarkupNodeType::AnyStringWithinBraces,
            Some(self.source[pos..p].to_string()),
            Vec::new(),
        );

        Some((p, vec![node]))
    }

    /// `url = seq< sor< "http://", "https://" >, plus< utf8::not_one<' '> > >`
    fn url(&self, pos: usize) -> Match {
        let rest = &self.source[pos..];
        let prefix = if rest.starts_with("http://") {
            7
        } else if rest.starts_with("https://") {
            8
        } else {
            return None;
        };

        let mut p = pos + prefix;

        while let Some(len) = self.any_char(p) {
            if self.at_byte(p, b' ') {
                break;
            }

            p += len;
        }

        if p == pos + prefix {
            return None;
        }

        let node = MarkupNode::new(
            MarkupNodeType::Url,
            Some(self.source[pos..p].to_string()),
            Vec::new(),
        );

        Some((p, vec![node]))
    }

    /// `braces< ControlChar > = seq< seq< ControlChar, string<'{'> >, until< string<'}'>,
    /// sor< anyStringWithinBraces, subscript, superscript, overbar > > >` with the
    /// `discard_empty` selector of `subscript`, `superscript` and `overbar`.
    fn braces(&self, pos: usize, control: u8, type_: MarkupNodeType) -> Match {
        if !self.at_byte(pos, control) || !self.at_byte(pos + 1, b'{') {
            return None;
        }

        let mut p = pos + 2;
        let mut children = Vec::new();

        loop {
            if self.at_byte(p, b'}') {
                p += 1;
                break;
            }

            let (end, mut n) = self
                .any_string_within_braces(p)
                .or_else(|| self.markup(p))?;

            children.append(&mut n);
            p = end;
        }

        if children.is_empty() {
            // discard_empty
            return Some((p, Vec::new()));
        }

        Some((p, vec![MarkupNode::new(type_, None, children)]))
    }
}
