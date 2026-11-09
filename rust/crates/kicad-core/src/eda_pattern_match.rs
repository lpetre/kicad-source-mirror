// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2015-2017 Chris Pavlina <pavlina.chris@gmail.com>
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

//! Port of `include/eda_pattern_match.h` and `common/eda_pattern_match.cpp`: substring,
//! regular expression, wildcard and relational matchers and `EDA_COMBINED_MATCHER`, which
//! KiCad uses for net class pattern assignments (`CTX_NETCLASS`), library and net searches.
//!
//! Positions and lengths count code points (`wxString` on Linux). Regular expressions are
//! `wxRegEx( ..., wxRE_ADVANCED )` (PCRE2 in the reference build), reproduced by [`pcre`] on
//! top of `regress`. Wildcard patterns, the common case for net classes, are matched by a hand
//! port of the regular expression the C++ builds from them (`?` -> `.`, `*` -> `.*`, greedy,
//! leftmost), so they do not depend on the regex engine.

pub mod pcre;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::wxstr::{lower, to_double, trim_left, trim_right};
use pcre::WxRegex;

/// `EDA_PATTERN_NOT_FOUND` (`wxNOT_FOUND`).
pub const EDA_PATTERN_NOT_FOUND: i32 = -1;

/// Port of `SEARCH_TERM`: a weighted text for [`EdaCombinedMatcher::score_terms`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchTerm {
    /// `Text`.
    pub text: String,
    /// `Score`.
    pub score: i32,
    /// `Normalized`: `text` has been lower-cased, trimmed and truncated.
    pub normalized: bool,
    /// `IsName`: the item's own name / LIB_ID (may promote it to the exact-match tier).
    pub is_name: bool,
}

impl SearchTerm {
    /// `SEARCH_TERM( aText, aScore, aIsName = false )`.
    pub fn new(text: &str, score: i32, is_name: bool) -> Self {
        Self {
            text: text.to_string(),
            score,
            normalized: false,
            is_name,
        }
    }
}

/// Port of `EDA_PATTERN_MATCH::FIND_RESULT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindResult {
    /// `start` (code points), [`EDA_PATTERN_NOT_FOUND`] when not found.
    pub start: i32,
    /// `length` (code points).
    pub length: i32,
}

impl Default for FindResult {
    fn default() -> Self {
        Self {
            start: EDA_PATTERN_NOT_FOUND,
            length: 0,
        }
    }
}

impl FindResult {
    /// `valid()` / `operator bool`.
    pub fn valid(&self) -> bool {
        self.start != EDA_PATTERN_NOT_FOUND
    }
}

/// `CLAMPED_VAL_INT_MAX( x )` then `static_cast<int>`.
fn clamp_int(x: usize) -> i32 {
    x.min(i32::MAX as usize) as i32
}

/// `wxString::Find( sub )`: code point index of the first occurrence.
fn wx_find(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle).map(|b| haystack[..b].chars().count())
}

/// Port of the `EDA_PATTERN_MATCH` interface. `Send + Sync` and [`EdaPatternMatch::clone_box`]
/// (no C++ counterpart: the C++ matchers live in `unique_ptr`s of objects that are never copied
/// or shared between threads) let a compiled [`EdaCombinedMatcher`] be a member of plain data
/// that is cloned and shared, e.g. the board's net settings.
pub trait EdaPatternMatch: Send + Sync {
    /// `SetPattern()`: false if the pattern cannot be processed.
    fn set_pattern(&mut self, pattern: &str) -> bool;
    /// `GetPattern()`.
    fn pattern(&self) -> &str;
    /// `Find()`: location and possibly length of a match, or an invalid result.
    fn find(&self, candidate: &str) -> FindResult;
    /// A copy of the matcher with its compiled pattern.
    fn clone_box(&self) -> Box<dyn EdaPatternMatch>;
}

impl Clone for Box<dyn EdaPatternMatch> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Port of `EDA_PATTERN_MATCH_SUBSTR`.
#[derive(Clone, Debug, Default)]
pub struct EdaPatternMatchSubstr {
    pattern: String,
}

impl EdaPatternMatch for EdaPatternMatchSubstr {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        self.pattern = pattern.to_string();
        true
    }

    fn pattern(&self) -> &str {
        &self.pattern
    }

    fn find(&self, candidate: &str) -> FindResult {
        match wx_find(candidate, &self.pattern) {
            None => FindResult::default(),
            Some(loc) => FindResult {
                start: clamp_int(loc),
                length: clamp_int(self.pattern.chars().count()),
            },
        }
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// Port of `EDA_PATTERN_MATCH_REGEX`: a pattern of the form `^...$` or `/.../` (the closing
/// slash optional) compiled as `wxRegEx( ..., wxRE_ADVANCED )`.
#[derive(Clone, Debug, Default)]
pub struct EdaPatternMatchRegex {
    pattern: String,
    regex: Option<WxRegex>,
}

impl EdaPatternMatchRegex {
    /// Port of `EDA_PATTERN_MATCH_REGEX::SetPattern`.
    fn set_regex_pattern(&mut self, pattern: &str) -> bool {
        if pattern.starts_with('^') && pattern.ends_with('$') {
            self.pattern = pattern.to_string();
        } else if let Some(rest) = pattern.strip_prefix('/') {
            // Requiring a '/' on the end means they get no feedback while they type
            self.pattern = rest.strip_suffix('/').unwrap_or(rest).to_string();
        } else {
            // For now regular expressions must be explicit
            return false;
        }

        self.regex = WxRegex::compile(&self.pattern);
        self.regex.is_some()
    }

    /// Port of `EDA_PATTERN_MATCH_REGEX::Find`: the regex match, or a substring search for the
    /// pattern text when the regex is not valid.
    fn regex_find(&self, candidate: &str) -> FindResult {
        match &self.regex {
            Some(re) => match re.find(candidate) {
                Some((start, len)) => FindResult {
                    start: clamp_int(start),
                    length: clamp_int(len),
                },
                None => FindResult::default(),
            },
            None => match wx_find(candidate, &self.pattern) {
                None => FindResult::default(),
                Some(loc) => FindResult {
                    start: clamp_int(loc),
                    length: clamp_int(self.pattern.chars().count()),
                },
            },
        }
    }
}

impl EdaPatternMatch for EdaPatternMatchRegex {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        self.set_regex_pattern(pattern)
    }

    fn pattern(&self) -> &str {
        &self.pattern
    }

    fn find(&self, candidate: &str) -> FindResult {
        self.regex_find(candidate)
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// Port of `EDA_PATTERN_MATCH_REGEX_ANCHORED`: `^` and `$` are added unless present.
#[derive(Clone, Debug, Default)]
pub struct EdaPatternMatchRegexAnchored {
    inner: EdaPatternMatchRegex,
}

impl EdaPatternMatch for EdaPatternMatchRegexAnchored {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        let mut p = pattern.to_string();

        if !p.starts_with('^') {
            p.insert(0, '^');
        }

        if !p.ends_with('$') {
            p.push('$');
        }

        self.inner.set_regex_pattern(&p)
    }

    fn pattern(&self) -> &str {
        &self.inner.pattern
    }

    fn find(&self, candidate: &str) -> FindResult {
        self.inner.regex_find(candidate)
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// The characters `EDA_PATTERN_MATCH_WILDCARD::SetPattern` escapes.
const WILDCARD_TO_REPLACE: &str = ".*+?^${}()|[]/\\";

/// The regular expression text the C++ builds from a wildcard pattern.
fn wildcard_to_regex(pattern: &str) -> String {
    let mut regex = String::with_capacity(2 * pattern.len());

    for c in pattern.chars() {
        if c == '?' {
            regex.push('.');
        } else if c == '*' {
            regex.push_str(".*");
        } else if WILDCARD_TO_REPLACE.contains(c) {
            regex.push('\\');
            regex.push(c);
        } else {
            regex.push(c);
        }
    }

    regex
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobTok {
    Lit(char),
    Any,
    Star,
}

/// Hand port of matching the regex built from a wildcard pattern: `.` (any code point, the
/// regex is compiled with DOTALL), `.*` greedy, leftmost start; for the anchored form `$` also
/// matches before a final newline. Results equal the PCRE2 match of that regex.
#[derive(Clone, Debug, Default)]
struct Glob {
    toks: Vec<GlobTok>,
}

impl Glob {
    fn new(pattern: &str) -> Self {
        Self {
            toks: pattern
                .chars()
                .map(|c| match c {
                    '?' => GlobTok::Any,
                    '*' => GlobTok::Star,
                    c => GlobTok::Lit(c),
                })
                .collect(),
        }
    }

    /// End of the first match (in backtracking order) of `toks[k..]` at `pos`.
    fn match_from(
        &self,
        s: &[char],
        k: usize,
        pos: usize,
        anchored_end: bool,
        failed: &mut Vec<bool>,
    ) -> Option<usize> {
        let w = s.len() + 1;

        if failed[k * w + pos] {
            return None;
        }

        let r = if k == self.toks.len() {
            if !anchored_end || pos == s.len() || (pos + 1 == s.len() && s[pos] == '\n') {
                Some(pos)
            } else {
                None
            }
        } else {
            match self.toks[k] {
                GlobTok::Lit(c) => {
                    if pos < s.len() && s[pos] == c {
                        self.match_from(s, k + 1, pos + 1, anchored_end, failed)
                    } else {
                        None
                    }
                }
                GlobTok::Any => {
                    if pos < s.len() {
                        self.match_from(s, k + 1, pos + 1, anchored_end, failed)
                    } else {
                        None
                    }
                }
                GlobTok::Star => (pos..=s.len())
                    .rev()
                    .find_map(|j| self.match_from(s, k + 1, j, anchored_end, failed)),
            }
        };

        if r.is_none() {
            failed[k * w + pos] = true;
        }

        r
    }

    fn find(&self, candidate: &str, anchored: bool) -> Option<(usize, usize)> {
        let s: Vec<char> = candidate.chars().collect();
        let mut failed = vec![false; (self.toks.len() + 1) * (s.len() + 1)];
        let last_start = if anchored { 0 } else { s.len() };

        (0..=last_start).find_map(|start| {
            self.match_from(&s, 0, start, anchored, &mut failed)
                .map(|end| (start, end - start))
        })
    }
}

/// Port of `EDA_PATTERN_MATCH_WILDCARD` (unanchored: the leftmost match anywhere).
#[derive(Clone, Debug, Default)]
pub struct EdaPatternMatchWildcard {
    inner: EdaPatternMatchRegex,
    wildcard_pattern: String,
    glob: Option<Glob>,
    anchored: bool,
}

impl EdaPatternMatchWildcard {
    fn set_wildcard(&mut self, pattern: &str, anchored: bool) -> bool {
        self.wildcard_pattern = pattern.to_string();
        self.anchored = anchored;

        // Compile the wildcard string to a regular expression
        let regex = wildcard_to_regex(pattern);

        let ok = if anchored {
            self.inner.set_regex_pattern(&format!("^{regex}$"))
        } else {
            self.inner.set_regex_pattern(&format!("/{regex}/"))
        };

        // The regex has no syntax but literals, `.` and `.*`, so a hand-written matcher gives
        // the same results; a NUL ends the C++ pattern early, keep the regex path then.
        self.glob = (ok && !pattern.contains('\0')).then(|| Glob::new(pattern));
        ok
    }

    fn wildcard_find(&self, candidate: &str) -> FindResult {
        match &self.glob {
            Some(g) => match g.find(candidate, self.anchored) {
                Some((start, len)) => FindResult {
                    start: clamp_int(start),
                    length: clamp_int(len),
                },
                None => FindResult::default(),
            },
            None => self.inner.regex_find(candidate),
        }
    }
}

impl EdaPatternMatch for EdaPatternMatchWildcard {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        self.set_wildcard(pattern, false)
    }

    fn pattern(&self) -> &str {
        &self.wildcard_pattern
    }

    fn find(&self, candidate: &str) -> FindResult {
        self.wildcard_find(candidate)
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// Port of `EDA_PATTERN_MATCH_WILDCARD_ANCHORED` (the whole candidate must match).
#[derive(Clone, Debug, Default)]
pub struct EdaPatternMatchWildcardAnchored {
    inner: EdaPatternMatchWildcard,
}

impl EdaPatternMatch for EdaPatternMatchWildcardAnchored {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        self.inner.set_wildcard(pattern, true)
    }

    fn pattern(&self) -> &str {
        &self.inner.wildcard_pattern
    }

    fn find(&self, candidate: &str) -> FindResult {
        self.inner.wildcard_find(candidate)
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// `EDA_PATTERN_MATCH_RELATIONAL::RELATION`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
    Any,
}

/// Port of `EDA_PATTERN_MATCH_RELATIONAL::m_units`.
fn relational_units() -> &'static BTreeMap<&'static str, f64> {
    static UNITS: OnceLock<BTreeMap<&'static str, f64>> = OnceLock::new();

    UNITS.get_or_init(|| {
        BTreeMap::from([
            ("p", 1e-12),
            ("n", 1e-9),
            ("u", 1e-6),
            ("m", 1e-3),
            ("", 1.),
            ("k", 1e3),
            ("meg", 1e6),
            ("g", 1e9),
            ("t", 1e12),
            ("ki", 1024.),
            ("mi", 1048576.),
            ("gi", 1073741824.),
            ("ti", 1099511627776.),
        ])
    })
}

fn relational_search_regex() -> Option<&'static WxRegex> {
    static RE: OnceLock<Option<WxRegex>> = OnceLock::new();
    RE.get_or_init(|| WxRegex::compile(r"^(\w+)(<|<=|=|>=|>)([-+]?[\d.]*)(\w*)$"))
        .as_ref()
}

fn relational_description_regex() -> Option<&'static WxRegex> {
    static RE: OnceLock<Option<WxRegex>> = OnceLock::new();
    RE.get_or_init(|| WxRegex::compile(r"(\w+)[=:]([-+]?[\d.]+)(\w*)"))
        .as_ref()
}

/// `wxRegEx::GetMatch( text, n )`: the text of capture group `n` (empty when unset).
fn group_text<'t>(text: &'t str, caps: &[Option<std::ops::Range<usize>>], n: usize) -> &'t str {
    caps.get(n).cloned().flatten().map_or("", |r| &text[r])
}

/// Port of `EDA_PATTERN_MATCH_RELATIONAL`: `key<op>value[unit]` patterns (`r<10k`) matched
/// against `key=value[unit]` / `key:value[unit]` tokens of the candidate.
#[derive(Clone, Debug)]
pub struct EdaPatternMatchRelational {
    pattern: String,
    key: String,
    relation: Option<Relation>,
    value: f64,
}

impl Default for EdaPatternMatchRelational {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            key: String::new(),
            relation: None,
            value: 0.0,
        }
    }
}

impl EdaPatternMatchRelational {
    /// Port of `EDA_PATTERN_MATCH_RELATIONAL::FindOne`: start of the `key=value` match in one
    /// token if it satisfies the relation.
    pub fn find_one(&self, candidate: &str) -> i32 {
        let Some(re) = relational_description_regex() else {
            return EDA_PATTERN_NOT_FOUND;
        };

        let Some(caps) = re.captures(candidate) else {
            return EDA_PATTERN_NOT_FOUND;
        };

        let start = caps[0]
            .clone()
            .map_or(0, |r| candidate[..r.start].chars().count());
        let key = group_text(candidate, &caps, 1);
        let val = group_text(candidate, &caps, 2);
        let unit = group_text(candidate, &caps, 3);
        let istart = clamp_int(start);

        if lower(key) != self.key {
            return EDA_PATTERN_NOT_FOUND;
        }

        let (parsed, complete) = to_double(val);

        let (Some(mut val_parsed), true) = (parsed, complete) else {
            return EDA_PATTERN_NOT_FOUND;
        };

        if let Some(mult) = relational_units().get(lower(unit).as_str()) {
            val_parsed *= mult;
        }

        let ok = match self.relation {
            Some(Relation::Lt) => val_parsed < self.value,
            Some(Relation::Le) => val_parsed <= self.value,
            Some(Relation::Eq) => val_parsed == self.value,
            Some(Relation::Ge) => val_parsed >= self.value,
            Some(Relation::Gt) => val_parsed > self.value,
            Some(Relation::Any) => true,
            None => false,
        };

        if ok { istart } else { EDA_PATTERN_NOT_FOUND }
    }
}

impl EdaPatternMatch for EdaPatternMatchRelational {
    fn set_pattern(&mut self, pattern: &str) -> bool {
        let Some(re) = relational_search_regex() else {
            return false;
        };

        // GetMatchCount() is 5 for this expression.
        let Some(caps) = re.captures(pattern) else {
            return false;
        };

        self.pattern = pattern.to_string();
        let key = group_text(pattern, &caps, 1);
        let rel = group_text(pattern, &caps, 2);
        let val = group_text(pattern, &caps, 3);
        let unit = group_text(pattern, &caps, 4);

        self.key = lower(key);

        self.relation = Some(match rel {
            "<" => Relation::Lt,
            "<=" => Relation::Le,
            "=" => Relation::Eq,
            ">=" => Relation::Ge,
            ">" => Relation::Gt,
            _ => return false,
        });

        if val.is_empty() {
            // Matching on empty values keeps the match list from going empty when the user
            // types the relational operator character, which helps prevent confusion.
            self.relation = Some(Relation::Any);
        } else {
            let (parsed, complete) = to_double(val);

            if let Some(v) = parsed {
                self.value = v;
            }

            if !complete {
                return false;
            }
        }

        match relational_units().get(lower(unit).as_str()) {
            Some(mult) => self.value *= mult,
            None => return false,
        }

        self.pattern = pattern.to_string();
        true
    }

    fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Port of `EDA_PATTERN_MATCH_RELATIONAL::Find`: the first white-space separated token
    /// that satisfies [`find_one`](Self::find_one). The reported position adds the token's
    /// offset within the token's search start (`wxStringTokenizer::GetPosition()` after the
    /// previous token), as the C++ does.
    fn find(&self, candidate: &str) -> FindResult {
        let chars: Vec<char> = candidate.chars().collect();
        let is_delim = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
        let mut pos = 0usize;
        let mut lastpos = 0usize;

        // wxStringTokenizer( aCandidate, " \t\r\n", wxTOKEN_STRTOK )
        while chars[pos..].iter().any(|&c| !is_delim(c)) {
            // GetNextToken(): skip empty tokens.
            let token: String = loop {
                let end = chars[pos..]
                    .iter()
                    .position(|&c| is_delim(c))
                    .map(|p| p + pos);

                let tok: String = match end {
                    None => {
                        let t = chars[pos..].iter().collect();
                        pos = chars.len();
                        t
                    }
                    Some(e) => {
                        let t = chars[pos..e].iter().collect();
                        pos = e + 1;
                        t
                    }
                };

                if !tok.is_empty() || !chars[pos..].iter().any(|&c| !is_delim(c)) {
                    break tok;
                }
            };

            let found_delta = self.find_one(&token);

            if found_delta != EDA_PATTERN_NOT_FOUND {
                let found = found_delta as usize + lastpos;
                return FindResult {
                    start: clamp_int(found),
                    length: 0,
                };
            }

            lastpos = pos;
        }

        FindResult::default()
    }

    fn clone_box(&self) -> Box<dyn EdaPatternMatch> {
        Box::new(self.clone())
    }
}

/// Port of `enum COMBINED_MATCHER_CONTEXT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombinedMatcherContext {
    /// `CTX_LIBITEM`
    LibItem,
    /// `CTX_NET`
    Net,
    /// `CTX_NETCLASS`
    Netclass,
    /// `CTX_SIGNAL`
    Signal,
    /// `CTX_SEARCH`
    Search,
}

/// Port of `EDA_COMBINED_MATCHER`. `Clone + Send + Sync` (see [`EdaPatternMatch`]).
#[derive(Clone)]
pub struct EdaCombinedMatcher {
    matchers: Vec<Box<dyn EdaPatternMatch>>,
    pattern: String,
}

impl std::fmt::Debug for EdaCombinedMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdaCombinedMatcher")
            .field("pattern", &self.pattern)
            .field("matchers", &self.matchers.len())
            .finish()
    }
}

impl EdaCombinedMatcher {
    /// Port of the constructor: the matchers of the context whose `SetPattern` succeeds, in
    /// order.
    pub fn new(pattern: &str, context: CombinedMatcherContext) -> Self {
        let mut m = Self {
            matchers: Vec::new(),
            pattern: pattern.to_string(),
        };

        match context {
            CombinedMatcherContext::LibItem => {
                // Whatever syntax users prefer, it shall be matched.
                m.add_matcher(pattern, Box::<EdaPatternMatchRegex>::default());
                m.add_matcher(pattern, Box::<EdaPatternMatchWildcard>::default());
                m.add_matcher(pattern, Box::<EdaPatternMatchRelational>::default());

                // If any of the above matchers couldn't be created because the pattern
                // syntax does not match, the substring will try its best.
                m.add_matcher(pattern, Box::<EdaPatternMatchSubstr>::default());
            }
            CombinedMatcherContext::Net
            | CombinedMatcherContext::Signal
            | CombinedMatcherContext::Search => {
                m.add_matcher(pattern, Box::<EdaPatternMatchRegex>::default());
                m.add_matcher(pattern, Box::<EdaPatternMatchWildcard>::default());
                m.add_matcher(pattern, Box::<EdaPatternMatchSubstr>::default());
            }
            CombinedMatcherContext::Netclass => {
                m.add_matcher(pattern, Box::<EdaPatternMatchRegexAnchored>::default());
                m.add_matcher(pattern, Box::<EdaPatternMatchWildcardAnchored>::default());
            }
        }

        m
    }

    fn add_matcher(&mut self, pattern: &str, mut matcher: Box<dyn EdaPatternMatch>) {
        if matcher.set_pattern(pattern) {
            self.matchers.push(matcher);
        }
    }

    /// Number of matchers whose pattern compiled.
    pub fn matcher_count(&self) -> usize {
        self.matchers.len()
    }

    /// Port of `Find( aTerm, aMatchersTriggered, aPosition )`: (found, matchers triggered,
    /// earliest position or [`EDA_PATTERN_NOT_FOUND`]).
    pub fn find_detailed(&self, term: &str) -> (bool, i32, i32) {
        let mut position = EDA_PATTERN_NOT_FOUND;
        let mut triggered = 0;

        for matcher in &self.matchers {
            let local = matcher.find(term);

            if local.valid() {
                triggered += 1;

                if local.start < position || position == EDA_PATTERN_NOT_FOUND {
                    position = local.start;
                }
            }
        }

        (position != EDA_PATTERN_NOT_FOUND, triggered, position)
    }

    /// Port of `Find( aTerm )`.
    pub fn find(&self, term: &str) -> bool {
        self.matchers.iter().any(|m| m.find(term).start >= 0)
    }

    /// Port of `StartsWith( aTerm )`: some matcher's leftmost match starts at 0 (the only entry
    /// point net class resolution uses).
    pub fn starts_with(&self, term: &str) -> bool {
        self.matchers.iter().any(|m| m.find(term).start == 0)
    }

    /// `GetPattern()`.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Port of `ScoreTerms`: normalises each term once (lower case, trimmed, at most 1000 code
    /// points) and sums the weighted scores; `exact_match` is set when the pattern equals an
    /// `is_name` term.
    pub fn score_terms(&self, terms: &mut [SearchTerm], mut exact_match: Option<&mut bool>) -> i32 {
        let mut score = 0;

        for term in terms.iter_mut() {
            if !term.normalized {
                let lowered = lower(&term.text);
                let mut text = trim_right(trim_left(&lowered)).to_string();

                // Don't cause KiCad to hang if someone accidentally pastes the PCB or schematic
                // into the search box.
                if text.chars().count() > 1000 {
                    text = text.chars().take(1000).collect();
                }

                term.text = text;
                term.normalized = true;
            }

            if self.pattern == term.text {
                score += 8 * term.score;

                // Only the item's own name/LIB_ID can promote it into the exact-match tier; an
                // incidental keyword or description field equalling the query shouldn't tie
                // with an item whose actual name is the query.
                if term.is_name {
                    if let Some(e) = exact_match.as_deref_mut() {
                        *e = true;
                    }
                }
            } else {
                let (found, _, found_pos) = self.find_detailed(&term.text);

                if found {
                    if found_pos == 0 {
                        score += 2 * term.score;
                    } else {
                        score += term.score;
                    }
                }
            }
        }

        score
    }
}
