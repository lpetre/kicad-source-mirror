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

//! Text-level helpers for comparing s-expression files (`.kicad_pcb`, `.kicad_mod`) in
//! differential tests (new code, test support; no C++ counterpart).
//!
//! These work on the *text* of a file, not on a parsed board, so that a comparison never
//! depends on the code under test: a light scanner that finds the lists (`(keyword ...)`) and
//! their byte spans, the first differing line of two texts with context and the keyword path
//! leading to it, and removal of top-level blocks by keyword.
//!
//! The scanner follows the KiCad s-expression lexer closely enough for files KiCad writes:
//! strings are `"..."` with backslash escapes, parentheses inside strings do not count, and the
//! keyword of a list is the first atom after its `(` (empty for a list starting with a string
//! or another list).

/// One list of an s-expression text: the bytes `start..=end` are `(` .. `)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SexprNode {
    /// Byte offset of the opening `(`.
    pub start: usize,
    /// Byte offset of the closing `)` (or the last byte of the text for an unterminated list).
    pub end: usize,
    /// The first atom after `(`, or `""`.
    pub keyword: String,
    /// The lists directly inside this one, in text order.
    pub children: Vec<SexprNode>,
}

impl SexprNode {
    /// True when this list or one of its descendants (not counting itself when `strict`) has
    /// the given keyword.
    pub fn contains_keyword(&self, keyword: &str, strict: bool) -> bool {
        if !strict && self.keyword == keyword {
            return true;
        }

        self.children
            .iter()
            .any(|c| c.contains_keyword(keyword, false))
    }

    /// The text of this list.
    pub fn text<'t>(&self, text: &'t str) -> &'t str {
        &text[self.start..=self.end.min(text.len() - 1)]
    }
}

/// Byte length of the atom starting at `i` (stops at whitespace, parentheses or a quote).
fn atom_len(bytes: &[u8], i: usize) -> usize {
    let mut j = i;

    while j < bytes.len() && !matches!(bytes[j], b'(' | b')' | b'"' | b' ' | b'\t' | b'\r' | b'\n')
    {
        j += 1;
    }

    j - i
}

/// The keyword of the list whose `(` is at `open`.
fn keyword_after(text: &str, open: usize) -> String {
    let bytes = text.as_bytes();
    let mut i = open + 1;

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }

    let n = atom_len(bytes, i);
    text[i..i + n].to_string()
}

/// Byte offset just past the string whose opening quote is at `i`.
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }

    bytes.len()
}

/// Parse the lists of `text` into a forest (the top-level lists, normally one `(kicad_pcb ..)`).
///
/// Unbalanced text does not fail: unterminated lists end at the end of the text and stray `)`
/// are ignored.
pub fn parse_lists(text: &str) -> Vec<SexprNode> {
    let bytes = text.as_bytes();
    let mut roots: Vec<SexprNode> = Vec::new();
    let mut stack: Vec<SexprNode> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'(' => stack.push(SexprNode {
                start: i,
                end: i,
                keyword: keyword_after(text, i),
                children: Vec::new(),
            }),
            b')' => {
                if let Some(mut node) = stack.pop() {
                    node.end = i;

                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => roots.push(node),
                    }
                }
            }
            _ => {}
        }

        i += 1;
    }

    // Unterminated lists end at the end of the text.
    while let Some(mut node) = stack.pop() {
        node.end = bytes.len().saturating_sub(1);

        match stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => roots.push(node),
        }
    }

    roots
}

/// The keywords of the lists open at byte `offset` of `text` (outermost first), followed by the
/// keyword of the list starting at `offset` (after leading blanks) if there is one.
///
/// Used to name the place of a difference, e.g. `["kicad_pcb", "footprint", "pad", "net"]`.
pub fn keyword_path_at(text: &str, offset: usize) -> Vec<String> {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;

    while i < offset {
        match bytes[i] {
            b'"' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'(' => stack.push(keyword_after(text, i)),
            b')' => {
                stack.pop();
            }
            _ => {}
        }

        i += 1;
    }

    let mut j = offset;

    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
        j += 1;
    }

    if j < bytes.len() && bytes[j] == b'(' {
        stack.push(keyword_after(text, j));
    }

    stack
}

/// The first line where two texts differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstDifference {
    /// 1-based line number (the same in both texts: every earlier line is identical).
    pub line: usize,
    /// The expected line (without its newline), `None` past the end of the expected text.
    pub expected: Option<String>,
    /// The actual line, `None` past the end of the actual text.
    pub actual: Option<String>,
    /// Keyword path of the difference (see [`keyword_path_at`]), taken from the expected text,
    /// or from the actual text when the expected text has ended.
    pub path: Vec<String>,
    /// A small diff excerpt: up to `context` identical lines before the difference (`  `),
    /// then up to `context` lines of each side from the difference on (`- ` expected,
    /// `+ ` actual).
    pub excerpt: String,
}

impl FirstDifference {
    /// The path as `a > b > c` without the root keyword (`kicad_pcb` / `footprint` of a
    /// library file stays when it is the only element).
    pub fn path_label(&self) -> String {
        let p: Vec<&str> = self.path.iter().map(String::as_str).collect();

        match p.len() {
            0 => "(end of file)".to_string(),
            1 => p[0].to_string(),
            _ => p[1..].join(" > "),
        }
    }

    /// A short label for histograms: the top-level block and the innermost keyword, e.g.
    /// `footprint > net` or `setup`.
    pub fn short_label(&self) -> String {
        match self.path.len() {
            0 => "(end of file)".to_string(),
            1 => self.path[0].clone(),
            2 => self.path[1].clone(),
            n => format!("{} > {}", self.path[1], self.path[n - 1]),
        }
    }
}

/// Find the first line where `expected` and `actual` differ (a missing final newline counts),
/// or `None` when the texts are identical.
pub fn first_difference(expected: &str, actual: &str, context: usize) -> Option<FirstDifference> {
    if expected == actual {
        return None;
    }

    let e: Vec<&str> = expected.split_inclusive('\n').collect();
    let a: Vec<&str> = actual.split_inclusive('\n').collect();
    let mut i = 0;
    let mut offset_e = 0;
    let mut offset_a = 0;

    while i < e.len() && i < a.len() && e[i] == a[i] {
        offset_e += e[i].len();
        offset_a += a[i].len();
        i += 1;
    }

    let strip = |s: &str| s.strip_suffix('\n').unwrap_or(s).to_string();
    let path = if i < e.len() {
        keyword_path_at(expected, offset_e)
    } else {
        keyword_path_at(actual, offset_a)
    };

    let mut excerpt = String::new();

    for line in &e[i.saturating_sub(context)..i] {
        excerpt.push_str("  ");
        excerpt.push_str(&strip(line));
        excerpt.push('\n');
    }

    for line in e.iter().skip(i).take(context.max(1)) {
        excerpt.push_str("- ");
        excerpt.push_str(&strip(line));
        excerpt.push('\n');
    }

    if i >= e.len() {
        excerpt.push_str("- (end of file)\n");
    }

    for line in a.iter().skip(i).take(context.max(1)) {
        excerpt.push_str("+ ");
        excerpt.push_str(&strip(line));
        excerpt.push('\n');
    }

    if i >= a.len() {
        excerpt.push_str("+ (end of file)\n");
    }

    Some(FirstDifference {
        line: i + 1,
        expected: e.get(i).map(|s| strip(s)),
        actual: a.get(i).map(|s| strip(s)),
        path,
        excerpt,
    })
}

/// Byte range of `node` extended to whole lines: from just after the previous newline when
/// only blanks precede the node on its line, to just after the following newline when only
/// blanks follow it.
fn whole_line_span(text: &str, node: &SexprNode) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut start = node.start;

    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }

    if start > 0 && bytes[start - 1] != b'\n' {
        start = node.start;
    }

    let mut end = node.end + 1;

    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\r') {
        end += 1;
    }

    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1;
    } else {
        end = (node.end + 1).min(bytes.len());
    }

    (start, end)
}

/// Remove the children of the root list (the top-level blocks of a board file) for which
/// `remove` returns true, with their lines. Text outside the root list is kept.
pub fn strip_top_level(text: &str, remove: impl Fn(&SexprNode) -> bool) -> String {
    let roots = parse_lists(text);
    let mut cut: Vec<(usize, usize)> = Vec::new();

    for root in &roots {
        for child in &root.children {
            if remove(child) {
                cut.push(whole_line_span(text, child));
            }
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut pos = 0;

    for (s, e) in cut {
        if s >= pos {
            out.push_str(&text[pos..s]);
            pos = e;
        }
    }

    out.push_str(&text[pos..]);
    out
}

/// The keywords of the children of the root list (the top-level blocks of a board file), in
/// text order.
pub fn top_level_keywords(text: &str) -> Vec<String> {
    parse_lists(text)
        .into_iter()
        .flat_map(|r| r.children.into_iter().map(|c| c.keyword))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = "(kicad_pcb\n\t(version 1)\n\t(net 0 \"a)(\")\n\t(footprint \"x\"\n\t\t(pad \"1\"\n\t\t\t(net 1)\n\t\t)\n\t)\n\t(segment\n\t\t(start 0 0)\n\t)\n)\n";

    #[test]
    fn lists_and_paths() {
        let roots = parse_lists(BOARD);
        assert_eq!(roots.len(), 1);
        let kws: Vec<&str> = roots[0]
            .children
            .iter()
            .map(|c| c.keyword.as_str())
            .collect();
        assert_eq!(kws, ["version", "net", "footprint", "segment"]);
        assert!(roots[0].children[2].contains_keyword("net", true));
        assert!(!roots[0].children[3].contains_keyword("net", true));

        let at = BOARD.find("\t\t\t(net 1)").unwrap();
        assert_eq!(
            keyword_path_at(BOARD, at),
            ["kicad_pcb", "footprint", "pad", "net"]
        );
    }

    #[test]
    fn first_difference_and_strip() {
        let other = BOARD.replace("(net 1)", "(net 2)");
        let d = first_difference(BOARD, &other, 2).unwrap();
        assert_eq!(d.line, 6);
        assert_eq!(d.expected.as_deref(), Some("\t\t\t(net 1)"));
        assert_eq!(d.short_label(), "footprint > net");
        assert_eq!(d.path_label(), "footprint > pad > net");
        assert!(
            d.excerpt
                .contains("- \t\t\t(net 1)\n- \t\t)\n+ \t\t\t(net 2)\n")
        );
        assert!(first_difference(BOARD, BOARD, 2).is_none());

        let shorter = &BOARD[..BOARD.len() - 2];
        let d = first_difference(BOARD, shorter, 1).unwrap();
        assert_eq!(d.line, 12);
        assert!(d.actual.is_none());

        let stripped = strip_top_level(BOARD, |n| n.keyword == "footprint");
        assert_eq!(
            stripped,
            "(kicad_pcb\n\t(version 1)\n\t(net 0 \"a)(\")\n\t(segment\n\t\t(start 0 0)\n\t)\n)\n"
        );
        assert_eq!(top_level_keywords(&stripped), ["version", "net", "segment"]);
    }
}
