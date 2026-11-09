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

//! `wxRegEx( pattern, wxRE_ADVANCED )` of the reference build on top of `regress`.
//!
//! wxWidgets 3.2 on Linux implements `wxRegEx` with PCRE2 (10.42, 32 bit code units). For
//! `wxRE_ADVANCED` it first rewrites Tcl "advanced regular expression" metasyntax
//! (`ConvertMetasyntax`: `***:` / `***=` directors and leading `(?...)` option letters) and
//! word boundaries (`ConvertWordBoundaries`: `\m`, `\M`, `\y`, `\Y`), then compiles with
//! `PCRE2_UTF | PCRE2_ALT_BSUX | PCRE2_DOTALL`, and matches with `pcre2_match` from offset 0.
//!
//! [`translate`] ports both wx rewrites literally, then parses the result with PCRE2 10.42's
//! syntax rules and emits an equivalent ECMAScript pattern for `regress` (flags `u`, plus `i`
//! when the whole pattern is caseless). PCRE2 semantics are reproduced explicitly where the
//! two dialects differ: ASCII `\s` / `\d` / `\w` / POSIX classes, `$` also matching before a
//! final newline, `.` under `(?s)`/`(?-s)`, multiline `^`, `\Z`, `\R`, `\h`, `\v`, `\N`,
//! `ALT_BSUX` escapes (`\x41`, `A`; `\x`, `\u`, `\U` alone are letters), octal and
//! backreference numbering, `\Q...\E`, `(?x)` extended mode, `(?U)`, `(?n)`, atomic groups
//! and possessive quantifiers (emulated with a lookahead and a backreference), and references
//! to a group that is not yet set (they fail in PCRE2, match empty in ECMAScript).
//!
//! Patterns PCRE2 rejects are rejected ([`PcreError::Invalid`]). Constructs PCRE2 accepts but
//! the port cannot reproduce are rejected too ([`PcreError::Unsupported`]); then the regex
//! matcher fails to compile exactly as for an invalid pattern (no regex matcher; "no match").
//! The list is in `PORT_STATUS.md`.

/// Why a pattern does not compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PcreError {
    /// PCRE2 (as wxRegEx calls it) rejects the pattern too.
    Invalid(String),
    /// PCRE2 accepts the pattern but the port does not support the construct (deviation).
    Unsupported(String),
}

fn invalid<T>(msg: &str) -> Result<T, PcreError> {
    Err(PcreError::Invalid(msg.to_string()))
}

fn unsupported<T>(msg: &str) -> Result<T, PcreError> {
    Err(PcreError::Unsupported(msg.to_string()))
}

// ------------------------------------------------------------------------------------------
// wx rewrites
// ------------------------------------------------------------------------------------------

const PCRE2_CASELESS: u32 = 1;
const PCRE2_MULTILINE: u32 = 2;
const PCRE2_DOTALL: u32 = 4;
const PCRE2_EXTENDED: u32 = 8;

/// Port of `PCREOptionsToString`.
fn pcre_options_to_string(opts: u32) -> String {
    let mut s = String::new();

    if opts & PCRE2_CASELESS != 0 {
        s.push('i');
    }

    if opts & PCRE2_MULTILINE != 0 {
        s.push('m');
    }

    if opts & PCRE2_DOTALL != 0 {
        s.push('s');
    }

    if opts & PCRE2_EXTENDED != 0 {
        s.push('x');
    }

    s
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Syntax {
    Advanced,
    Basic,
    Extended,
}

/// Port of `ConvertMetasyntax( expr, flags )` (the `flags` start as `wxRE_ADVANCED`).
fn convert_metasyntax(mut expr: Vec<char>) -> (Vec<char>, Syntax) {
    let mut syntax = Syntax::Advanced;
    let starts = |e: &[char], p: &str| {
        e.len() >= p.chars().count() && e.iter().copied().take(p.chars().count()).eq(p.chars())
    };

    if expr.len() > 3 && starts(&expr, "***") {
        match expr[3] {
            ':' => {
                expr.drain(0..4);
            }
            '=' => {
                // flags &= ~( wxRE_BASIC | wxRE_ADVANCED ): plain extended syntax.
                syntax = Syntax::Extended;
                expr.splice(0..4, ['\\', 'Q']);
            }
            _ => {}
        }
    }

    if syntax == Syntax::Advanced && starts(&expr, "(?") {
        let mut opts_string = String::new();
        let mut opts = 0u32;
        let mut negopts = 0u32;

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Sel {
            None,
            Basic,
            Extended,
            Literal,
        }

        let mut sel = Sel::None;
        let mut it = 2usize;

        while it < expr.len() {
            let c = expr[it];

            if c == ')' {
                opts_string += &pcre_options_to_string(opts);

                if negopts != 0 {
                    opts_string.push('-');
                    opts_string += &pcre_options_to_string(negopts);
                }

                let pos_after_opts = if opts_string.is_empty() {
                    expr.drain(0..=it);
                    0
                } else {
                    let n = opts_string.chars().count();
                    expr.splice(2..it, opts_string.chars());
                    n + 3 // (?opts)
                };

                syntax = match sel {
                    Sel::None => Syntax::Advanced,
                    Sel::Basic => Syntax::Basic,
                    Sel::Extended => Syntax::Extended,
                    Sel::Literal => {
                        expr.splice(pos_after_opts..pos_after_opts, ['\\', 'Q']);
                        Syntax::Extended
                    }
                };

                break;
            }

            // Avoid misinterpreting other constructs (non-capturing groups, look ahead
            // assertions etc) as options, which always consist in alphabetic characters only.
            if !c.is_ascii_lowercase() {
                break;
            }

            match c {
                'b' => sel = Sel::Basic,
                'e' => sel = Sel::Extended,
                'q' => sel = Sel::Literal,
                'm' | 'n' => {
                    negopts &= !PCRE2_MULTILINE;
                    opts |= PCRE2_MULTILINE;
                    negopts |= PCRE2_DOTALL;
                }
                'p' => negopts |= PCRE2_DOTALL,
                'w' => {
                    negopts &= !(PCRE2_MULTILINE | PCRE2_DOTALL);
                    opts |= PCRE2_MULTILINE | PCRE2_DOTALL;
                }
                'c' => negopts |= PCRE2_CASELESS,
                't' => negopts |= PCRE2_EXTENDED,
                's' => {
                    negopts &= !PCRE2_DOTALL;
                    opts |= PCRE2_DOTALL;
                }
                'i' => {
                    negopts &= !PCRE2_CASELESS;
                    opts |= PCRE2_CASELESS;
                }
                'x' => {
                    negopts &= !PCRE2_EXTENDED;
                    opts |= PCRE2_EXTENDED;
                }
                other => opts_string.push(other),
            }

            it += 1;
        }
    }

    (expr, syntax)
}

/// Port of `ConvertWordBoundaries`.
fn convert_word_boundaries(expr: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(expr.len());
    let mut i = 0;

    while i < expr.len() {
        if expr[i] == '\\' {
            i += 1;

            if i == expr.len() {
                out.push('\\');
                break;
            }

            let replacement = match expr[i] {
                'm' => Some("[[:<:]]"),
                'M' => Some("[[:>:]]"),
                'y' => Some("\\b"),
                'Y' => Some("\\B"),
                _ => None,
            };

            if let Some(r) = replacement {
                out.extend(r.chars());
                i += 1;
                continue;
            }

            out.push('\\');
        }

        out.push(expr[i]);
        i += 1;
    }

    out
}

// ------------------------------------------------------------------------------------------
// PCRE2 syntax tree
// ------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum ClassItem {
    Range(u32, u32),
    /// Pre-computed code point set (types, POSIX classes) as sorted ranges.
    Set(Vec<(u32, u32)>),
    /// `\p{...}` / `\P{...}` (ECMAScript text).
    Prop(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assert {
    Circumflex { multiline: bool },
    Dollar { multiline: bool },
    StartOfSubject,
    EndOfSubject,
    EndOrFinalNewline,
    WordBoundary,
    NotWordBoundary,
    StartOfWord,
    EndOfWord,
}

#[derive(Clone, Debug)]
enum GroupKind {
    Capture(usize),
    NonCapture,
    Atomic,
    LookAhead(bool),
    LookBehind(bool),
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    /// A literal code point; `caseless` per PCRE2_CASELESS at that point.
    Char(u32, bool),
    Class {
        negated: bool,
        items: Vec<ClassItem>,
        caseless: bool,
    },
    Assert(Assert),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Group(GroupKind, Box<Node>),
    Repeat {
        body: Box<Node>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        possessive: bool,
    },
    /// Backreference by group number (resolved), caseless flag.
    Backref(usize, bool),
    /// Backreference by name (resolved after parsing).
    NamedBackref(String, bool),
    /// `(*FAIL)`
    Fail,
}

#[derive(Clone, Copy)]
struct Opts {
    caseless: bool,
    multiline: bool,
    dotall: bool,
    extended: bool,
    no_auto_capture: bool,
    ungreedy: bool,
}

struct Parser {
    p: Vec<char>,
    i: usize,
    opts: Opts,
    group_count: usize,
    names: Vec<(String, usize)>,
}

const MAX_CP: u32 = 0x10FFFF;

fn ascii_ranges(s: &[(u32, u32)]) -> Vec<(u32, u32)> {
    s.to_vec()
}

const DIGIT: &[(u32, u32)] = &[(0x30, 0x39)];
const WORD: &[(u32, u32)] = &[(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)];
const SPACE: &[(u32, u32)] = &[(0x09, 0x0D), (0x20, 0x20)];
const HSPACE: &[(u32, u32)] = &[
    (0x09, 0x09),
    (0x20, 0x20),
    (0xA0, 0xA0),
    (0x1680, 0x1680),
    (0x180E, 0x180E),
    (0x2000, 0x200A),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x3000, 0x3000),
];
const VSPACE: &[(u32, u32)] = &[(0x0A, 0x0D), (0x85, 0x85), (0x2028, 0x2029)];

fn complement(set: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut next = 0u32;

    for &(a, b) in set {
        if a > next {
            out.push((next, a - 1));
        }

        next = b + 1;
    }

    if next <= MAX_CP {
        out.push((next, MAX_CP));
    }

    out
}

/// PCRE2 POSIX class (ASCII, no UCP).
fn posix_class(name: &str) -> Option<Vec<(u32, u32)>> {
    let v: &[(u32, u32)] = match name {
        "alpha" => &[(0x41, 0x5A), (0x61, 0x7A)],
        "lower" => &[(0x61, 0x7A)],
        "upper" => &[(0x41, 0x5A)],
        "alnum" => &[(0x30, 0x39), (0x41, 0x5A), (0x61, 0x7A)],
        "ascii" => &[(0x00, 0x7F)],
        "blank" => &[(0x09, 0x09), (0x20, 0x20)],
        "cntrl" => &[(0x00, 0x1F), (0x7F, 0x7F)],
        "digit" => DIGIT,
        "graph" => &[(0x21, 0x7E)],
        "print" => &[(0x20, 0x7E)],
        "punct" => &[(0x21, 0x2F), (0x3A, 0x40), (0x5B, 0x60), (0x7B, 0x7E)],
        "space" => SPACE,
        "word" => WORD,
        "xdigit" => &[(0x30, 0x39), (0x41, 0x46), (0x61, 0x66)],
        _ => return None,
    };

    Some(ascii_ranges(v))
}

const GENERAL_CATEGORIES: &[&str] = &[
    "C", "Cc", "Cf", "Cn", "Co", "Cs", "L", "Ll", "Lm", "Lo", "Lt", "Lu", "M", "Mc", "Me", "Mn",
    "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "S", "Sc", "Sk", "Sm",
    "So", "Z", "Zl", "Zp", "Zs",
];

/// `\p{name}` of PCRE2 10.42 (loose matching: case, spaces, `_` and `-` ignored) as an
/// ECMAScript property escape body.
fn unicode_property(name: &str, negated: bool) -> Result<ClassItem, PcreError> {
    let (neg2, name) = match name.strip_prefix('^') {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let negated = negated ^ neg2;
    let loose: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect::<String>()
        .to_lowercase();
    let p = if negated { 'P' } else { 'p' };

    if loose == "any" {
        return Ok(if negated {
            ClassItem::Set(Vec::new())
        } else {
            ClassItem::Set(vec![(0, MAX_CP)])
        });
    }

    if loose == "l&" || loose == "lc" {
        return Ok(ClassItem::Prop(format!("\\{p}{{LC}}")));
    }

    if let Some(gc) = GENERAL_CATEGORIES
        .iter()
        .find(|g| g.to_lowercase() == loose)
    {
        return Ok(ClassItem::Prop(format!("\\{p}{{{gc}}}")));
    }

    if loose.starts_with('x') && matches!(loose.as_str(), "xan" | "xps" | "xsp" | "xwd" | "xuc") {
        return unsupported("PCRE2 special property");
    }

    if loose.is_empty() || !loose.chars().all(|c| c.is_ascii_alphanumeric()) {
        return invalid("unknown property name");
    }

    // Script name: PCRE2 accepts the Unicode script names loosely; regress wants the canonical
    // spelling, which is tried with each word capitalised.
    let canon: String = name
        .split(['_', ' ', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("_");
    let text = format!("\\{p}{{Script={canon}}}");

    if regress::Regex::with_flags(&text, "u").is_ok() {
        Ok(ClassItem::Prop(text))
    } else {
        invalid("unknown property name")
    }
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

fn is_octal(c: char) -> bool {
    ('0'..='7').contains(&c)
}

/// What a backslash sequence denotes.
enum Esc {
    Char(u32),
    Item(ClassItem),
    Node(Node),
    /// `\Q`: quote mode starts.
    Quote,
    /// `\E` outside quoting: ignored.
    Nothing,
}

fn is_cased(c: u32) -> bool {
    match char::from_u32(c) {
        Some(ch) => {
            let lower: Vec<char> = ch.to_lowercase().collect();
            let upper: Vec<char> = ch.to_uppercase().collect();
            lower != [ch] || upper != [ch]
        }
        None => false,
    }
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.p.get(self.i + k).copied()
    }

    fn starts(&self, s: &str) -> bool {
        let n = s.chars().count();
        self.i + n <= self.p.len() && self.p[self.i..self.i + n].iter().copied().eq(s.chars())
    }

    /// Extended mode: skip white space and `#` comments.
    fn skip_extended(&mut self) {
        if !self.opts.extended {
            return;
        }

        loop {
            match self.peek() {
                Some(
                    ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{200E}'
                    | '\u{200F}' | '\u{2028}' | '\u{2029}',
                ) => self.i += 1,
                Some('#') => {
                    while let Some(c) = self.peek() {
                        self.i += 1;

                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn parse_alt(&mut self) -> Result<Node, PcreError> {
        let saved = self.opts;
        let mut alts = vec![self.parse_seq()?];

        while self.peek() == Some('|') {
            self.i += 1;
            alts.push(self.parse_seq()?);
        }

        self.opts = saved;
        Ok(if alts.len() == 1 {
            alts.pop().unwrap_or(Node::Empty)
        } else {
            Node::Alt(alts)
        })
    }

    /// Reads a `{n}`, `{n,}`, `{n,m}` quantifier at the current position (PCRE2 10.42: no
    /// spaces, `{,m}` is not a quantifier).
    fn read_repeat_counts(&self) -> Result<Option<(u32, Option<u32>, usize)>, PcreError> {
        let mut j = self.i + 1;
        let digits = |j: &mut usize| {
            let start = *j;
            while *j < self.p.len() && self.p[*j].is_ascii_digit() {
                *j += 1;
            }
            self.p[start..*j].iter().collect::<String>()
        };
        let a = digits(&mut j);

        if a.is_empty() {
            return Ok(None);
        }

        let (min, max);

        if self.p.get(j) == Some(&'}') {
            min = a;
            max = Some(min.clone());
        } else if self.p.get(j) == Some(&',') {
            j += 1;
            let b = digits(&mut j);

            if self.p.get(j) != Some(&'}') {
                return Ok(None);
            }

            min = a;
            max = if b.is_empty() { None } else { Some(b) };
        } else {
            return Ok(None);
        }

        let parse = |s: &str| -> Result<u32, PcreError> {
            match s.parse::<u32>() {
                Ok(v) if v <= 65535 => Ok(v),
                _ => invalid("number too big in {} quantifier"),
            }
        };
        let min = parse(&min)?;
        let max = match max {
            Some(m) => Some(parse(&m)?),
            None => None,
        };

        if let Some(m) = max {
            if m < min {
                return invalid("numbers out of order in {} quantifier");
            }
        }

        Ok(Some((min, max, j + 1 - self.i)))
    }

    fn parse_seq(&mut self) -> Result<Node, PcreError> {
        let mut items: Vec<Node> = Vec::new();

        loop {
            self.skip_extended();

            let Some(c) = self.peek() else { break };

            if c == '|' || c == ')' {
                break;
            }

            // A quantifier with nothing to repeat.
            if matches!(c, '*' | '+' | '?') || (c == '{' && self.read_repeat_counts()?.is_some()) {
                return invalid("quantifier does not follow a repeatable item");
            }

            let atom = self.parse_atom(&mut items)?;

            let Some(atom) = atom else { continue };

            self.skip_extended();
            let atom = self.parse_quantifier(atom)?;
            items.push(atom);
        }

        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap_or(Node::Empty),
            _ => Node::Seq(items),
        })
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, PcreError> {
        let (min, max, len) = match self.peek() {
            Some('*') => (0, None, 1),
            Some('+') => (1, None, 1),
            Some('?') => (0, Some(1), 1),
            Some('{') => match self.read_repeat_counts()? {
                Some(q) => q,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };

        match &atom {
            Node::Assert(_) => return invalid("quantifier does not follow a repeatable item"),
            Node::Group(GroupKind::LookAhead(_) | GroupKind::LookBehind(_), _) => {
                return unsupported("quantified assertion");
            }
            Node::Empty => return invalid("quantifier does not follow a repeatable item"),
            _ => {}
        }

        self.i += len;

        let mut greedy = true;
        let mut possessive = false;

        match self.peek() {
            Some('?') => {
                greedy = false;
                self.i += 1;
            }
            Some('+') => {
                possessive = true;
                self.i += 1;
            }
            _ => {}
        }

        if self.opts.ungreedy && !possessive {
            greedy = !greedy;
        }

        // A further quantifier is an error ("a**", "a{1}{2}").
        self.skip_extended();

        if matches!(self.peek(), Some('*' | '+' | '?'))
            || (self.peek() == Some('{') && self.read_repeat_counts()?.is_some())
        {
            return invalid("quantifier does not follow a repeatable item");
        }

        Ok(Node::Repeat {
            body: Box::new(atom),
            min,
            max,
            greedy,
            possessive,
        })
    }

    fn literal(&self, c: u32) -> Node {
        Node::Char(c, self.opts.caseless)
    }

    /// Parses one atom; `None` when the construct produced nothing (an option setting, a
    /// comment, `\E`, a callout). Quoted `\Q...\E` text is pushed to `items` directly, the
    /// last character is returned as the atom (a quantifier applies to it only).
    fn parse_atom(&mut self, items: &mut Vec<Node>) -> Result<Option<Node>, PcreError> {
        let c = self.peek().unwrap_or('\0');

        match c {
            '\\' => {
                self.i += 1;

                match self.parse_escape(false)? {
                    Esc::Char(ch) => Ok(Some(self.literal(ch))),
                    Esc::Item(item) => Ok(Some(Node::Class {
                        negated: false,
                        items: vec![item],
                        caseless: self.opts.caseless,
                    })),
                    Esc::Node(n) => Ok(Some(n)),
                    Esc::Nothing => Ok(None),
                    Esc::Quote => {
                        let mut last = None;

                        while self.i < self.p.len() {
                            if self.starts("\\E") {
                                self.i += 2;
                                break;
                            }

                            if let Some(prev) = last.take() {
                                items.push(prev);
                            }

                            last = Some(self.literal(self.p[self.i] as u32));
                            self.i += 1;
                        }

                        Ok(last)
                    }
                }
            }
            '[' => {
                if self.starts("[[:<:]]") {
                    self.i += 7;
                    return Ok(Some(Node::Assert(Assert::StartOfWord)));
                }

                if self.starts("[[:>:]]") {
                    self.i += 7;
                    return Ok(Some(Node::Assert(Assert::EndOfWord)));
                }

                // POSIX class syntax outside a class is an error.
                if matches!(self.peek_at(1), Some(':' | '.' | '='))
                    && self.check_posix_syntax(self.i + 1).is_some()
                {
                    return invalid("POSIX named classes are supported only within a class");
                }

                self.i += 1;
                self.parse_class().map(Some)
            }
            '(' => {
                self.i += 1;
                self.parse_group()
            }
            '.' => {
                self.i += 1;

                Ok(Some(if self.opts.dotall {
                    Node::Class {
                        negated: false,
                        items: vec![ClassItem::Set(vec![(0, MAX_CP)])],
                        caseless: false,
                    }
                } else {
                    Node::Class {
                        negated: true,
                        items: vec![ClassItem::Range(0x0A, 0x0A)],
                        caseless: false,
                    }
                }))
            }
            '^' => {
                self.i += 1;
                Ok(Some(Node::Assert(Assert::Circumflex {
                    multiline: self.opts.multiline,
                })))
            }
            '$' => {
                self.i += 1;
                Ok(Some(Node::Assert(Assert::Dollar {
                    multiline: self.opts.multiline,
                })))
            }
            _ => {
                self.i += 1;
                Ok(Some(self.literal(c as u32)))
            }
        }
    }

    /// PCRE2's `check_posix_syntax`: `start` points at the `:` / `.` / `=` after `[`.
    /// Returns the index of the terminator character.
    fn check_posix_syntax(&self, start: usize) -> Option<usize> {
        let term = self.p[start];
        let mut j = start + 1;

        while j < self.p.len() {
            let c = self.p[j];

            if c == '\\' && matches!(self.p.get(j + 1), Some(']' | '\\')) {
                j += 1;
            } else if (c == '[' && self.p.get(j + 1) == Some(&term)) || c == ']' {
                return None;
            } else if c == term && self.p.get(j + 1) == Some(&']') {
                return Some(j);
            }

            j += 1;
        }

        None
    }

    fn parse_class(&mut self) -> Result<Node, PcreError> {
        let mut negated = false;

        if self.peek() == Some('^') {
            negated = true;
            self.i += 1;
        }

        let mut items: Vec<ClassItem> = Vec::new();
        let mut first = true;
        // Last single character (for ranges) and whether the last item was a class type.
        let mut last_char: Option<u32> = None;
        let mut last_was_type = false;
        let mut in_quote = false;

        loop {
            let Some(c) = self.peek() else {
                return invalid("missing terminating ] for character class");
            };

            if in_quote {
                if self.starts("\\E") {
                    self.i += 2;
                    in_quote = false;
                    continue;
                }

                self.i += 1;
                items.push(ClassItem::Range(c as u32, c as u32));
                last_char = Some(c as u32);
                last_was_type = false;
                first = false;
                continue;
            }

            if c == ']' && !first {
                self.i += 1;
                break;
            }

            first = false;

            // POSIX class.
            if c == '[' && matches!(self.peek_at(1), Some(':' | '.' | '=')) {
                if let Some(end) = self.check_posix_syntax(self.i + 1) {
                    if self.p[self.i + 1] != ':' {
                        return invalid("POSIX collating elements are not supported");
                    }

                    let mut name: String = self.p[self.i + 2..end].iter().collect();
                    let neg = name.starts_with('^');

                    if neg {
                        name.remove(0);
                    }

                    let Some(set) = posix_class(&name) else {
                        return invalid("unknown POSIX class name");
                    };

                    self.i = end + 2;

                    items.push(ClassItem::Set(if neg { complement(&set) } else { set }));
                    last_char = None;
                    last_was_type = true;
                    self.check_type_range()?;
                    continue;
                }
            }

            // Range?
            if c == '-' && last_char.is_some() && !last_was_type {
                if let Some(next) = self.peek_at(1) {
                    if next != ']' {
                        // Parse the range end.
                        self.i += 1;
                        let end = self.class_single()?;

                        let Some(end) = end else {
                            return invalid("invalid range in character class");
                        };

                        let start = last_char.unwrap_or(0);

                        if end < start {
                            return invalid("range out of order in character class");
                        }

                        // Replace the start character by the range.
                        items.pop();
                        items.push(ClassItem::Range(start, end));
                        last_char = None;
                        last_was_type = false;
                        continue;
                    }
                }
            }

            if c == '\\' {
                self.i += 1;

                match self.parse_escape(true)? {
                    Esc::Char(ch) => {
                        items.push(ClassItem::Range(ch, ch));
                        last_char = Some(ch);
                        last_was_type = false;
                    }
                    Esc::Item(item) => {
                        items.push(item);
                        last_char = None;
                        last_was_type = true;
                        self.check_type_range()?;
                    }
                    Esc::Quote => in_quote = true,
                    Esc::Nothing => {}
                    Esc::Node(_) => {
                        return invalid("escape sequence is invalid in character class");
                    }
                }

                continue;
            }

            self.i += 1;
            items.push(ClassItem::Range(c as u32, c as u32));
            last_char = Some(c as u32);
            last_was_type = false;
        }

        Ok(Node::Class {
            negated,
            items,
            caseless: self.opts.caseless,
        })
    }

    /// After a class type (`\d`, `[:alpha:]`): a `-` that is not followed by `]` is an
    /// invalid range in PCRE2 10.42.
    fn check_type_range(&self) -> Result<(), PcreError> {
        if self.peek() == Some('-') && self.peek_at(1).is_some_and(|c| c != ']') {
            return invalid("invalid range in character class");
        }

        Ok(())
    }

    /// One character of a class range end; `None` when it is not a single character.
    fn class_single(&mut self) -> Result<Option<u32>, PcreError> {
        let Some(c) = self.peek() else {
            return invalid("missing terminating ] for character class");
        };

        if c == '['
            && matches!(self.peek_at(1), Some(':' | '.' | '='))
            && self.check_posix_syntax(self.i + 1).is_some()
        {
            return Ok(None);
        }

        if c == '\\' {
            self.i += 1;

            return match self.parse_escape(true)? {
                Esc::Char(ch) => Ok(Some(ch)),
                Esc::Quote | Esc::Nothing => unsupported("\\Q or \\E as range end"),
                _ => Ok(None),
            };
        }

        self.i += 1;
        Ok(Some(c as u32))
    }

    fn read_hex(&self, from: usize, n: usize) -> Option<u32> {
        if from + n > self.p.len() || !self.p[from..from + n].iter().all(|&c| is_hex(c)) {
            return None;
        }

        u32::from_str_radix(&self.p[from..from + n].iter().collect::<String>(), 16).ok()
    }

    fn check_cp(cp: u32) -> Result<u32, PcreError> {
        if cp > MAX_CP {
            return invalid("character code point value too large");
        }

        if (0xD800..=0xDFFF).contains(&cp) {
            return invalid("disallowed Unicode code point (>= 0xd800 && <= 0xdfff)");
        }

        Ok(cp)
    }

    /// Reads a group name (`[A-Za-z_][A-Za-z0-9_]*`, at most 32) followed by `term`.
    fn read_name(&mut self, term: char) -> Result<String, PcreError> {
        let start = self.i;

        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.i += 1;
            } else {
                break;
            }
        }

        let name: String = self.p[start..self.i].iter().collect();

        if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || name.len() > 32 {
            return invalid("subpattern name expected");
        }

        if self.peek() != Some(term) {
            return invalid("syntax error in subpattern name (missing terminator?)");
        }

        self.i += 1;
        Ok(name)
    }

    /// Parses the escape after a backslash (already consumed).
    fn parse_escape(&mut self, in_class: bool) -> Result<Esc, PcreError> {
        let Some(c) = self.peek() else {
            return invalid("\\ at end of pattern");
        };

        self.i += 1;
        let caseless = self.opts.caseless;

        Ok(match c {
            '0' => {
                let mut v = 0u32;
                let mut n = 0;

                while n < 2 && self.peek().is_some_and(is_octal) {
                    v = v * 8 + self.peek().and_then(|d| d.to_digit(8)).unwrap_or(0);
                    self.i += 1;
                    n += 1;
                }

                Esc::Char(v)
            }
            '1'..='9' => {
                if in_class {
                    if c == '8' || c == '9' {
                        Esc::Char(c as u32)
                    } else {
                        self.i -= 1;
                        Esc::Char(self.read_octal3())
                    }
                } else {
                    let start = self.i - 1;
                    let mut j = start;

                    while j < self.p.len() && self.p[j].is_ascii_digit() {
                        j += 1;
                    }

                    let text: String = self.p[start..j].iter().collect();
                    let n: u64 = text.parse().unwrap_or(u64::MAX);

                    if n < 10 || c == '8' || c == '9' || n <= self.group_count as u64 {
                        self.i = j;

                        if n > 65535 {
                            return invalid("subpattern number is too big");
                        }

                        Esc::Node(Node::Backref(n as usize, caseless))
                    } else {
                        self.i = start;
                        Esc::Char(self.read_octal3())
                    }
                }
            }
            'a' => Esc::Char(7),
            'e' => Esc::Char(0x1B),
            'f' => Esc::Char(0x0C),
            'n' => Esc::Char(0x0A),
            'r' => Esc::Char(0x0D),
            't' => Esc::Char(0x09),
            'b' if in_class => Esc::Char(0x08),
            'x' => match self.read_hex(self.i, 2) {
                Some(v) => {
                    self.i += 2;
                    Esc::Char(v)
                }
                None => Esc::Char('x' as u32),
            },
            'u' => match self.read_hex(self.i, 4) {
                Some(v) => {
                    self.i += 4;
                    Esc::Char(Self::check_cp(v)?)
                }
                None => Esc::Char('u' as u32),
            },
            'U' => Esc::Char('U' as u32),
            'o' => {
                if self.peek() != Some('{') {
                    return invalid("missing opening brace after \\o");
                }

                let start = self.i + 1;
                let mut j = start;

                while j < self.p.len() && is_octal(self.p[j]) {
                    j += 1;
                }

                if j == start || self.p.get(j) != Some(&'}') {
                    return invalid("non-octal character in \\o{} (closing brace missing?)");
                }

                let v = u32::from_str_radix(&self.p[start..j].iter().collect::<String>(), 8)
                    .map_err(|_| {
                        PcreError::Invalid("character code point value too large".into())
                    })?;
                self.i = j + 1;
                Esc::Char(Self::check_cp(v)?)
            }
            'c' => {
                let Some(x) = self.peek() else {
                    return invalid("\\c at end of pattern");
                };

                if !(' '..='~').contains(&x) {
                    return invalid("\\c must be followed by a printable ASCII character");
                }

                self.i += 1;
                Esc::Char((x.to_ascii_uppercase() as u32) ^ 0x40)
            }
            'd' => Esc::Item(ClassItem::Set(DIGIT.to_vec())),
            'D' => Esc::Item(ClassItem::Set(complement(DIGIT))),
            'w' => Esc::Item(ClassItem::Set(WORD.to_vec())),
            'W' => Esc::Item(ClassItem::Set(complement(WORD))),
            's' => Esc::Item(ClassItem::Set(SPACE.to_vec())),
            'S' => Esc::Item(ClassItem::Set(complement(SPACE))),
            'h' => Esc::Item(ClassItem::Set(HSPACE.to_vec())),
            'H' => Esc::Item(ClassItem::Set(complement(HSPACE))),
            'v' => Esc::Item(ClassItem::Set(VSPACE.to_vec())),
            'V' => Esc::Item(ClassItem::Set(complement(VSPACE))),
            'p' | 'P' => {
                let name = if self.peek() == Some('{') {
                    let start = self.i + 1;
                    let Some(end) = self.p[start..].iter().position(|&c| c == '}') else {
                        return invalid("malformed \\P or \\p sequence");
                    };
                    self.i = start + end + 1;
                    self.p[start..start + end].iter().collect::<String>()
                } else {
                    let Some(n) = self.peek() else {
                        return invalid("malformed \\P or \\p sequence");
                    };
                    self.i += 1;
                    n.to_string()
                };

                Esc::Item(unicode_property(&name, c == 'P')?)
            }
            'Q' => Esc::Quote,
            'E' => Esc::Nothing,
            _ if in_class => {
                if c.is_ascii_alphanumeric() {
                    return invalid("escape sequence is invalid in character class");
                }

                Esc::Char(c as u32)
            }
            'N' => {
                if self.peek() == Some('{') {
                    if self.starts("{U+") {
                        let start = self.i + 3;
                        let mut j = start;

                        while j < self.p.len() && is_hex(self.p[j]) {
                            j += 1;
                        }

                        if j > start && self.p.get(j) == Some(&'}') {
                            let v = u32::from_str_radix(
                                &self.p[start..j].iter().collect::<String>(),
                                16,
                            )
                            .map_err(|_| {
                                PcreError::Invalid("character code point value too large".into())
                            })?;
                            self.i = j + 1;
                            return Ok(Esc::Char(Self::check_cp(v)?));
                        }
                    }

                    return invalid("PCRE2 does not support \\F, \\L, \\l, \\N{name}, \\U, or \\u");
                }

                Esc::Item(ClassItem::Set(complement(&[(0x0A, 0x0A)])))
            }
            'R' => Esc::Node(Node::Group(
                GroupKind::Atomic,
                Box::new(Node::Alt(vec![
                    Node::Seq(vec![Node::Char(0x0D, false), Node::Char(0x0A, false)]),
                    Node::Class {
                        negated: false,
                        items: vec![ClassItem::Set(VSPACE.to_vec())],
                        caseless: false,
                    },
                ])),
            )),
            'X' => return unsupported("\\X"),
            'C' => Esc::Item(ClassItem::Set(vec![(0, MAX_CP)])),
            'K' => return unsupported("\\K"),
            'b' => Esc::Node(Node::Assert(Assert::WordBoundary)),
            'B' => Esc::Node(Node::Assert(Assert::NotWordBoundary)),
            'A' => Esc::Node(Node::Assert(Assert::StartOfSubject)),
            'z' => Esc::Node(Node::Assert(Assert::EndOfSubject)),
            'Z' => Esc::Node(Node::Assert(Assert::EndOrFinalNewline)),
            // Matching always starts at offset 0, so \G is \A.
            'G' => Esc::Node(Node::Assert(Assert::StartOfSubject)),
            'g' => self.parse_g_escape(caseless)?,
            'k' => {
                let term = match self.peek() {
                    Some('<') => '>',
                    Some('\'') => '\'',
                    Some('{') => '}',
                    _ => {
                        return invalid(
                            "\\k is not followed by a braced, angle-bracketed, or quoted name",
                        );
                    }
                };
                self.i += 1;
                let name = self.read_name(term)?;
                Esc::Node(Node::NamedBackref(name, caseless))
            }
            _ if c.is_ascii_alphanumeric() => return invalid("unrecognized character follows \\"),
            _ => Esc::Char(c as u32),
        })
    }

    fn read_octal3(&mut self) -> u32 {
        let mut v = 0u32;
        let mut n = 0;

        while n < 3 && self.peek().is_some_and(is_octal) {
            v = v * 8 + self.peek().and_then(|d| d.to_digit(8)).unwrap_or(0);
            self.i += 1;
            n += 1;
        }

        v
    }

    fn parse_g_escape(&mut self, caseless: bool) -> Result<Esc, PcreError> {
        match self.peek() {
            Some('<') | Some('\'') => {
                // Subroutine call: \g<n> / \g'name'.
                let term = if self.peek() == Some('<') { '>' } else { '\'' };
                let start = self.i + 1;
                let Some(end) = self.p[start..].iter().position(|&c| c == term) else {
                    return invalid(
                        "\\g is not followed by a braced, angle-bracketed, or quoted name/number or by a plain number",
                    );
                };
                let body: String = self.p[start..start + end].iter().collect();
                self.i = start + end + 1;
                Ok(Esc::Node(Node::Group(
                    GroupKind::NonCapture,
                    Box::new(Node::NamedBackref(format!("\u{0}sub:{body}"), caseless)),
                )))
            }
            Some('{') => {
                let start = self.i + 1;
                let Some(end) = self.p[start..].iter().position(|&c| c == '}') else {
                    return invalid(
                        "\\g is not followed by a braced, angle-bracketed, or quoted name/number or by a plain number",
                    );
                };
                let body: String = self.p[start..start + end].iter().collect();
                self.i = start + end + 1;
                self.g_number_or_name(&body, caseless)
            }
            _ => {
                let start = self.i;
                let mut j = start;

                if self.p.get(j) == Some(&'-') || self.p.get(j) == Some(&'+') {
                    j += 1;
                }

                while j < self.p.len() && self.p[j].is_ascii_digit() {
                    j += 1;
                }

                let body: String = self.p[start..j].iter().collect();

                if body.is_empty() || body == "-" || body == "+" {
                    return invalid(
                        "\\g is not followed by a braced, angle-bracketed, or quoted name/number or by a plain number",
                    );
                }

                self.i = j;
                self.g_number_or_name(&body, caseless)
            }
        }
    }

    fn g_number_or_name(&mut self, body: &str, caseless: bool) -> Result<Esc, PcreError> {
        if let Some(rel) = body.strip_prefix('-') {
            let n: usize = rel
                .parse()
                .map_err(|_| PcreError::Invalid("invalid \\g".into()))?;

            if n == 0 || n > self.group_count {
                return invalid("reference to non-existent subpattern");
            }

            return Ok(Esc::Node(Node::Backref(self.group_count + 1 - n, caseless)));
        }

        if body.starts_with('+') {
            return invalid("a relative value of zero is not allowed");
        }

        if let Ok(n) = body.parse::<usize>() {
            if n == 0 {
                return invalid("a relative value of zero is not allowed");
            }

            return Ok(Esc::Node(Node::Backref(n, caseless)));
        }

        Ok(Esc::Node(Node::NamedBackref(body.to_string(), caseless)))
    }

    fn parse_group(&mut self) -> Result<Option<Node>, PcreError> {
        if self.peek() == Some('*') {
            return self.parse_verb();
        }

        if self.peek() != Some('?') {
            let kind = if self.opts.no_auto_capture {
                GroupKind::NonCapture
            } else {
                self.group_count += 1;
                GroupKind::Capture(self.group_count)
            };

            return self.finish_group(kind).map(Some);
        }

        self.i += 1;

        let Some(c) = self.peek() else {
            return invalid("unrecognized character after (? or (?-");
        };

        match c {
            '#' => {
                while let Some(c) = self.peek() {
                    self.i += 1;

                    if c == ')' {
                        return Ok(None);
                    }
                }

                invalid("missing ) at end of (?# comment")
            }
            ':' => {
                self.i += 1;
                self.finish_group(GroupKind::NonCapture).map(Some)
            }
            '|' => {
                self.i += 1;
                let before = self.group_count;
                let node = self.finish_group(GroupKind::NonCapture)?;

                if self.group_count != before {
                    return unsupported("(?| branch reset with groups");
                }

                Ok(Some(node))
            }
            '>' => {
                self.i += 1;
                self.finish_group(GroupKind::Atomic).map(Some)
            }
            '=' => {
                self.i += 1;
                self.finish_group(GroupKind::LookAhead(false)).map(Some)
            }
            '!' => {
                self.i += 1;
                self.finish_group(GroupKind::LookAhead(true)).map(Some)
            }
            '<' if self.peek_at(1) == Some('=') => {
                self.i += 2;
                self.finish_group(GroupKind::LookBehind(false)).map(Some)
            }
            '<' if self.peek_at(1) == Some('!') => {
                self.i += 2;
                self.finish_group(GroupKind::LookBehind(true)).map(Some)
            }
            '<' | '\'' => {
                self.i += 1;
                let name = self.read_name(if c == '<' { '>' } else { '\'' })?;
                self.named_group(name).map(Some)
            }
            'P' => {
                self.i += 1;

                match self.peek() {
                    Some('<') => {
                        self.i += 1;
                        let name = self.read_name('>')?;
                        self.named_group(name).map(Some)
                    }
                    Some('=') => {
                        self.i += 1;
                        let name = self.read_name(')')?;
                        Ok(Some(Node::NamedBackref(name, self.opts.caseless)))
                    }
                    Some('>') => {
                        self.i += 1;
                        let name = self.read_name(')')?;
                        Ok(Some(Node::NamedBackref(format!("\u{0}sub:{name}"), false)))
                    }
                    _ => invalid("unrecognized character after (?P"),
                }
            }
            '&' => {
                self.i += 1;
                let name = self.read_name(')')?;
                Ok(Some(Node::NamedBackref(format!("\u{0}sub:{name}"), false)))
            }
            'R' | '0'..='9' | '+' | '-'
                if c != '-' || self.peek_at(1).is_some_and(|d| d.is_ascii_digit()) =>
            {
                // Recursion / subroutine call by number.
                let start = self.i;

                while self.peek().is_some_and(|d| d != ')') {
                    self.i += 1;
                }

                if self.peek() != Some(')') {
                    return invalid("missing closing parenthesis");
                }

                let body: String = self.p[start..self.i].iter().collect();
                self.i += 1;
                Ok(Some(Node::NamedBackref(format!("\u{0}sub:{body}"), false)))
            }
            'C' => {
                // Callout: (?C), (?Cn), (?C"text") ... no callout function is set, so it has
                // no effect on matching.
                while let Some(c) = self.peek() {
                    self.i += 1;

                    if c == ')' {
                        return Ok(None);
                    }
                }

                invalid("missing closing parenthesis")
            }
            '(' => unsupported("conditional group"),
            _ => self.parse_option_setting(),
        }
    }

    fn named_group(&mut self, name: String) -> Result<Node, PcreError> {
        if self.names.iter().any(|(n, _)| *n == name) {
            return invalid("two named subpatterns have the same name (PCRE2_DUPNAMES not set)");
        }

        self.group_count += 1;
        let idx = self.group_count;
        self.names.push((name, idx));
        self.finish_group(GroupKind::Capture(idx))
    }

    /// `(?imnsxJU-imnsx)` or `(?imnsx-imnsx:...)`, `(?^...)`.
    fn parse_option_setting(&mut self) -> Result<Option<Node>, PcreError> {
        let mut opts = self.opts;
        let mut on = true;

        if self.peek() == Some('^') {
            opts.caseless = false;
            opts.multiline = false;
            opts.no_auto_capture = false;
            opts.dotall = false;
            opts.extended = false;
            self.i += 1;
        }

        loop {
            let Some(c) = self.peek() else {
                return invalid("missing closing parenthesis");
            };

            self.i += 1;

            match c {
                '-' if on => on = false,
                'i' => opts.caseless = on,
                'm' => opts.multiline = on,
                'n' => opts.no_auto_capture = on,
                's' => opts.dotall = on,
                'x' => {
                    if self.peek() == Some('x') {
                        return unsupported("(?xx)");
                    }

                    opts.extended = on
                }
                'U' => opts.ungreedy = on,
                'J' => return unsupported("(?J)"),
                ')' => {
                    // Applies to the rest of the enclosing group.
                    self.opts = opts;
                    return Ok(None);
                }
                ':' => {
                    let saved = self.opts;
                    self.opts = opts;
                    let node = self.finish_group(GroupKind::NonCapture);
                    self.opts = saved;
                    return node.map(Some);
                }
                _ => return invalid("unrecognized character after (? or (?-"),
            }
        }
    }

    fn parse_verb(&mut self) -> Result<Option<Node>, PcreError> {
        // '(' consumed, at '*'.
        let start = self.i + 1;
        let Some(end) = self.p[start..].iter().position(|&c| c == ')') else {
            return invalid("missing closing parenthesis");
        };
        let name: String = self.p[start..start + end].iter().collect();
        self.i = start + end + 1;

        match name.as_str() {
            // Start-of-pattern settings are only recognised at the very start (handled in
            // `translate`).
            _ if is_start_setting(&name) => invalid("(*VERB) not recognized or malformed"),
            "FAIL" | "F" => Ok(Some(Node::Fail)),
            _ => unsupported("backtracking control verb"),
        }
    }

    fn finish_group(&mut self, kind: GroupKind) -> Result<Node, PcreError> {
        let body = self.parse_alt()?;

        if self.peek() != Some(')') {
            return invalid("missing closing parenthesis");
        }

        self.i += 1;
        Ok(Node::Group(kind, Box::new(body)))
    }
}

/// PCRE2 start-of-pattern settings `(*NAME)` (newline conventions, limits, optimisation
/// switches, `(*UTF)`, `(*UCP)`).
fn is_start_setting(name: &str) -> bool {
    matches!(
        name,
        "UTF"
            | "UCP"
            | "CR"
            | "LF"
            | "CRLF"
            | "ANY"
            | "ANYCRLF"
            | "NUL"
            | "BSR_ANYCRLF"
            | "BSR_UNICODE"
            | "NO_AUTO_POSSESS"
            | "NO_DOTSTAR_ANCHOR"
            | "NO_JIT"
            | "NO_START_OPT"
            | "NOTEMPTY"
            | "NOTEMPTY_ATSTART"
    ) || name.starts_with("LIMIT_DEPTH=")
        || name.starts_with("LIMIT_HEAP=")
        || name.starts_with("LIMIT_MATCH=")
}

/// Removes the leading start-of-pattern settings. Those that change matching (other newline
/// conventions, `(*UCP)`, `(*NOTEMPTY)`) are not supported.
fn strip_start_settings(p: &mut Vec<char>) -> Result<(), PcreError> {
    loop {
        if p.len() < 3 || p[0] != '(' || p[1] != '*' {
            return Ok(());
        }

        let Some(end) = p.iter().position(|&c| c == ')') else {
            return Ok(());
        };

        let name: String = p[2..end].iter().collect();

        if !is_start_setting(&name) {
            return Ok(());
        }

        if !matches!(
            name.as_str(),
            "UTF" | "LF" | "NO_AUTO_POSSESS" | "NO_DOTSTAR_ANCHOR" | "NO_JIT" | "NO_START_OPT"
        ) && !name.starts_with("LIMIT_")
        {
            return unsupported("start-of-pattern setting that changes matching");
        }

        p.drain(0..=end);
    }
}

/// Characters PCRE2 treats as case-equivalent to `c` (simple case folding orbit; the common
/// multi-member orbits are listed explicitly).
fn case_variants(c: u32) -> Vec<u32> {
    const ORBITS: &[&[u32]] = &[
        &[0x6B, 0x4B, 0x212A],
        &[0x73, 0x53, 0x17F],
        &[0xC5, 0xE5, 0x212B],
        &[0xB5, 0x39C, 0x3BC],
        &[0x3B2, 0x3D0, 0x392],
        &[0x3B5, 0x3F5, 0x395],
        &[0x3B8, 0x3D1, 0x398, 0x3F4],
        &[0x3B9, 0x345, 0x399, 0x1FBE],
        &[0x3BA, 0x3F0, 0x39A],
        &[0x3C0, 0x3D6, 0x3A0],
        &[0x3C1, 0x3F1, 0x3A1],
        &[0x3C2, 0x3C3, 0x3A3],
        &[0x3C6, 0x3D5, 0x3A6],
        &[0x3C9, 0x2126, 0x3A9],
        &[0x1E61, 0x1E9B, 0x1E60],
    ];

    if let Some(orbit) = ORBITS.iter().find(|o| o.contains(&c)) {
        return orbit.to_vec();
    }

    let mut v = vec![c];

    if let Some(ch) = char::from_u32(c) {
        let mut add = |x: char| {
            if !v.contains(&(x as u32)) {
                v.push(x as u32);
            }
        };

        let single = |it: &mut dyn Iterator<Item = char>| -> Option<char> {
            match (it.next(), it.next()) {
                (Some(x), None) => Some(x),
                _ => None,
            }
        };

        if let Some(l) = single(&mut ch.to_lowercase()) {
            add(l);

            if let Some(u) = single(&mut l.to_uppercase()) {
                add(u);
            }
        }

        if let Some(u) = single(&mut ch.to_uppercase()) {
            add(u);

            if let Some(l) = single(&mut u.to_lowercase()) {
                add(l);
            }
        }
    }

    v
}

// ------------------------------------------------------------------------------------------
// Checks and emission
// ------------------------------------------------------------------------------------------

/// Fixed match width (code points) as PCRE2 10.42 requires inside lookbehind.
fn fixed_width(n: &Node) -> Result<Option<usize>, PcreError> {
    Ok(match n {
        Node::Empty | Node::Assert(_) | Node::Fail => Some(0),
        Node::Char(..) | Node::Class { .. } => Some(1),
        Node::Seq(items) => {
            let mut w = 0;

            for it in items {
                match fixed_width(it)? {
                    Some(x) => w += x,
                    None => return Ok(None),
                }
            }

            Some(w)
        }
        Node::Alt(alts) => {
            let mut w = None;

            for a in alts {
                let x = fixed_width(a)?;

                match (w, x) {
                    (_, None) => return Ok(None),
                    (None, Some(x)) => w = Some(x),
                    (Some(y), Some(x)) if x != y => return Ok(None),
                    _ => {}
                }
            }

            w
        }
        Node::Group(GroupKind::LookAhead(_) | GroupKind::LookBehind(_), _) => Some(0),
        Node::Group(_, body) => fixed_width(body)?,
        Node::Repeat { body, min, max, .. } => {
            if Some(*min) == *max {
                fixed_width(body)?.map(|w| w * *min as usize)
            } else {
                None
            }
        }
        Node::Backref(..) | Node::NamedBackref(..) => {
            return unsupported("backreference in lookbehind");
        }
    })
}

fn check_lookbehind(body: &Node) -> Result<(), PcreError> {
    let alts: Vec<&Node> = match body {
        Node::Alt(a) => a.iter().collect(),
        other => vec![other],
    };

    for a in alts {
        if fixed_width(a)?.is_none() {
            return invalid("lookbehind assertion is not fixed length");
        }
    }

    Ok(())
}

/// Resolves named backrefs, validates group references and the lookbehind rule, and collects
/// the caseless state of every cased atom.
fn check(
    n: &mut Node,
    names: &[(String, usize)],
    groups: usize,
    case: &mut (bool, bool),
) -> Result<(), PcreError> {
    match n {
        Node::Char(c, ci) => {
            if is_cased(*c) {
                if *ci { case.0 = true } else { case.1 = true }
            }
        }
        Node::Class {
            items, caseless, ..
        } => {
            let cased = items.iter().any(|it| match it {
                ClassItem::Range(a, b) => (*a..=*b).take(512).any(is_cased) || (*b - *a) >= 512,
                ClassItem::Set(_) => false,
                ClassItem::Prop(p) => {
                    p.contains("{Lu}")
                        || p.contains("{Ll}")
                        || p.contains("{Lt}")
                        || p.contains("{LC}")
                }
            });

            if cased {
                if *caseless {
                    case.0 = true
                } else {
                    case.1 = true
                }
            }
        }
        Node::NamedBackref(name, ci) => {
            if let Some(body) = name.strip_prefix("\u{0}sub:") {
                // Subroutine call / recursion: an error if the group does not exist.
                let exists = body == "R"
                    || body == "0"
                    || body.parse::<usize>().is_ok_and(|g| g <= groups)
                    || body
                        .strip_prefix('+')
                        .or(body.strip_prefix('-'))
                        .and_then(|g| g.parse::<usize>().ok())
                        .is_some_and(|g| g >= 1 && g <= groups)
                    || names.iter().any(|(nm, _)| nm == body);

                if !exists {
                    return invalid("reference to non-existent subpattern");
                }

                return unsupported("subroutine call or recursion");
            }

            let Some((_, idx)) = names.iter().find(|(nm, _)| nm == name) else {
                return invalid("reference to non-existent subpattern");
            };

            *n = Node::Backref(*idx, *ci);
            return check(n, names, groups, case);
        }
        Node::Backref(idx, ci) => {
            if *idx == 0 || *idx > groups {
                return invalid("reference to non-existent subpattern");
            }

            if *ci { case.0 = true } else { case.1 = true }
        }
        Node::Seq(items) | Node::Alt(items) => {
            for it in items {
                check(it, names, groups, case)?;
            }
        }
        Node::Group(kind, body) => {
            check(body, names, groups, case)?;

            if matches!(kind, GroupKind::LookBehind(_)) {
                check_lookbehind(body)?;
            }
        }
        Node::Repeat { body, .. } => check(body, names, groups, case)?,
        Node::Empty | Node::Assert(_) | Node::Fail => {}
    }

    Ok(())
}

struct Emitter {
    out: String,
    /// Caseless and case-sensitive atoms are mixed: caseless atoms list their case variants.
    mixed_case: bool,
    /// ECMAScript group number of each PCRE2 group (index = PCRE2 number).
    map: Vec<usize>,
    next_es_group: usize,
    closed: Vec<bool>,
    repeat_depth: usize,
}

fn push_cp(out: &mut String, c: u32) {
    match char::from_u32(c) {
        Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => out.push(ch),
        _ => out.push_str(&format!("\\u{{{c:X}}}")),
    }
}

fn push_ranges(out: &mut String, ranges: &[(u32, u32)]) {
    for &(a, b) in ranges {
        push_cp(out, a);

        if b != a {
            out.push('-');
            push_cp(out, b);
        }
    }
}

impl Emitter {
    /// Appends the ECMAScript text of `n`.
    fn emit(&mut self, n: &Node) -> Result<(), PcreError> {
        match n {
            Node::Empty => {}
            Node::Char(c, caseless) => {
                if self.mixed_case && *caseless && is_cased(*c) {
                    self.out.push('[');

                    for v in case_variants(*c) {
                        push_cp(&mut self.out, v);
                    }

                    self.out.push(']');
                } else {
                    push_cp(&mut self.out, *c);
                }
            }
            Node::Class {
                negated,
                items,
                caseless,
            } => {
                self.out.push('[');

                if *negated {
                    self.out.push('^');
                }

                for it in items {
                    match it {
                        ClassItem::Range(a, b) if self.mixed_case && *caseless => {
                            if b - a > 512 {
                                return unsupported("large caseless range in a mixed-case pattern");
                            }

                            for c in *a..=*b {
                                for v in case_variants(c) {
                                    push_cp(&mut self.out, v);
                                }
                            }
                        }
                        ClassItem::Prop(_) if self.mixed_case && *caseless => {
                            return unsupported("caseless property in a mixed-case pattern");
                        }
                        ClassItem::Range(a, b) => push_ranges(&mut self.out, &[(*a, *b)]),
                        ClassItem::Set(r) => push_ranges(&mut self.out, r),
                        ClassItem::Prop(p) => self.out.push_str(p),
                    }
                }

                self.out.push(']');
            }
            Node::Assert(a) => self.out.push_str(match a {
                Assert::Circumflex { multiline: false } | Assert::StartOfSubject => "^",
                Assert::Circumflex { multiline: true } => "(?:^|(?<=\\n)(?=[^]))",
                Assert::Dollar { multiline: false } | Assert::EndOrFinalNewline => "(?=\\n?$)",
                Assert::Dollar { multiline: true } => "(?=\\n|$)",
                Assert::EndOfSubject => "$",
                Assert::WordBoundary => "\\b",
                Assert::NotWordBoundary => "\\B",
                Assert::StartOfWord => "\\b(?=[0-9A-Z_a-z])",
                Assert::EndOfWord => "\\b(?<=[0-9A-Z_a-z])",
            }),
            Node::Seq(items) => {
                for it in items {
                    self.emit(it)?;
                }
            }
            Node::Alt(alts) => {
                for (k, a) in alts.iter().enumerate() {
                    if k > 0 {
                        self.out.push('|');
                    }

                    self.emit(a)?;
                }
            }
            Node::Group(kind, body) => match kind {
                GroupKind::Capture(idx) => {
                    self.next_es_group += 1;
                    self.map[*idx] = self.next_es_group;
                    self.out.push('(');
                    self.emit(body)?;
                    self.out.push(')');
                    self.closed[*idx] = true;
                }
                GroupKind::NonCapture => {
                    self.out.push_str("(?:");
                    self.emit(body)?;
                    self.out.push(')');
                }
                GroupKind::Atomic => self.emit_atomic(body)?,
                GroupKind::LookAhead(neg) => {
                    self.out.push_str(if *neg { "(?!" } else { "(?=" });
                    self.emit(body)?;
                    self.out.push(')');
                }
                GroupKind::LookBehind(neg) => {
                    self.out.push_str(if *neg { "(?<!" } else { "(?<=" });
                    self.emit(body)?;
                    self.out.push(')');
                }
            },
            Node::Repeat {
                body,
                min,
                max,
                greedy,
                possessive,
            } => {
                if *possessive {
                    let inner = Node::Repeat {
                        body: body.clone(),
                        min: *min,
                        max: *max,
                        greedy: true,
                        possessive: false,
                    };
                    return self.emit_atomic(&inner);
                }

                self.repeat_depth += 1;
                self.out.push_str("(?:");
                self.emit(body)?;
                self.out.push(')');
                self.repeat_depth -= 1;

                match (min, max) {
                    (0, None) => self.out.push('*'),
                    (1, None) => self.out.push('+'),
                    (0, Some(1)) => self.out.push('?'),
                    (a, None) => self.out.push_str(&format!("{{{a},}}")),
                    (a, Some(b)) if a == b => self.out.push_str(&format!("{{{a}}}")),
                    (a, Some(b)) => self.out.push_str(&format!("{{{a},{b}}}")),
                }

                if !*greedy {
                    self.out.push('?');
                }
            }
            Node::Backref(idx, caseless) => {
                if self.mixed_case && *caseless {
                    return unsupported("caseless backreference in a mixed-case pattern");
                }

                if self.closed[*idx] {
                    self.out.push_str(&format!("(?:\\{})", self.map[*idx]));
                } else if self.repeat_depth == 0 {
                    // A reference to a group that is not set yet never matches in PCRE2.
                    self.out.push_str("(?!)");
                } else {
                    return unsupported("backreference to a group that may be unset");
                }
            }
            Node::NamedBackref(..) => return unsupported("unresolved named reference"),
            Node::Fail => self.out.push_str("(?!)"),
        }

        Ok(())
    }

    /// `(?>X)` as `(?=(X))\N`.
    fn emit_atomic(&mut self, body: &Node) -> Result<(), PcreError> {
        self.next_es_group += 1;
        let own = self.next_es_group;
        self.out.push_str("(?=(");
        self.emit(body)?;
        self.out.push_str(&format!("))\\{own}"));
        Ok(())
    }
}

/// Translates a `wxRegEx( pattern, wxRE_ADVANCED )` pattern into an ECMAScript pattern and
/// `regress` flags.
pub fn translate(pattern: &str) -> Result<(String, &'static str), PcreError> {
    // pcre2_compile( ..., PCRE2_ZERO_TERMINATED ): the pattern ends at the first NUL.
    let pattern = pattern.split('\0').next().unwrap_or("");
    let (expr, syntax) = convert_metasyntax(pattern.chars().collect());

    let mut expr = match syntax {
        Syntax::Basic => return unsupported("(?b) basic regular expression syntax"),
        Syntax::Advanced => convert_word_boundaries(&expr),
        Syntax::Extended => expr,
    };

    strip_start_settings(&mut expr)?;

    let mut parser = Parser {
        p: expr,
        i: 0,
        opts: Opts {
            caseless: false,
            multiline: false,
            dotall: true,
            extended: false,
            no_auto_capture: false,
            ungreedy: false,
        },
        group_count: 0,
        names: Vec::new(),
    };

    let mut tree = parser.parse_alt()?;

    if parser.i < parser.p.len() {
        // Only an unmatched ')' stops parse_alt before the end.
        return invalid("unmatched closing parenthesis");
    }

    let groups = parser.group_count;
    let mut case = (false, false);
    check(&mut tree, &parser.names, groups, &mut case)?;

    let mixed_case = case.0 && case.1;
    let mut em = Emitter {
        out: String::new(),
        mixed_case,
        map: vec![0; groups + 1],
        next_es_group: 0,
        closed: vec![false; groups + 1],
        repeat_depth: 0,
    };

    // ECMAScript numbers groups by opening parenthesis; the emitter assigns numbers in the
    // same order (user groups and the groups of the atomic-group emulation).
    em.emit(&tree)?;

    Ok((em.out, if case.0 && !mixed_case { "iu" } else { "u" }))
}

/// A compiled `wxRegEx`.
#[derive(Clone, Debug)]
pub struct WxRegex {
    re: regress::Regex,
}

impl WxRegex {
    /// `wxRegEx::Compile( pattern, wxRE_ADVANCED )`; `None` when it fails (invalid or
    /// unsupported pattern).
    pub fn compile(pattern: &str) -> Option<Self> {
        let (es, flags) = translate(pattern).ok()?;
        regress::Regex::with_flags(&es, flags)
            .ok()
            .map(|re| Self { re })
    }

    /// `Matches( text )` then `GetMatch( &start, &len, 0 )`: the leftmost match as
    /// (start, length) in code points.
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        let m = self.re.find(text)?;
        let r = m.range();
        let start = text[..r.start].chars().count();
        let len = text[r.start..r.end].chars().count();
        Some((start, len))
    }

    /// Byte range and capture byte ranges of the leftmost match (for the relational matcher).
    pub(crate) fn captures(&self, text: &str) -> Option<Vec<Option<std::ops::Range<usize>>>> {
        let m = self.re.find(text)?;
        let mut v = vec![Some(m.range())];
        v.extend(m.captures.iter().cloned());
        Some(v)
    }
}
