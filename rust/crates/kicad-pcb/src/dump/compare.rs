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


//! Rust twin of `rust/tools/pcb-dump/compare.py` (same classification), for tests that
//! compare a Rust dump with a committed C++ golden.

use std::collections::BTreeMap;
use std::fmt::Write;

/// Classification of one `SECTION|KEY|FIELD` (see `compare.py`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Match,
    Mismatch,
    Pending,
    RustError,
    Missing,
    Extra,
    RefError,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Match,
        Kind::Mismatch,
        Kind::Pending,
        Kind::RustError,
        Kind::Missing,
        Kind::Extra,
        Kind::RefError,
    ];

    pub fn name(self) -> &'static str {
        [
            "match",
            "mismatch",
            "pending",
            "rust-error",
            "missing",
            "extra",
            "ref-error",
        ][self as usize]
    }

    /// Kinds that make a comparison fail.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Kind::Mismatch | Kind::RustError | Kind::Missing | Kind::Extra
        )
    }
}

#[derive(Default)]
struct Parsed {
    values: BTreeMap<String, String>,
    pending: BTreeMap<String, String>,
    errors: BTreeMap<String, String>,
    ref_errors: BTreeMap<String, Vec<String>>,
    order: Vec<String>,
}

fn parse(text: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut file = String::new();

    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let kind = line.split('|').next().unwrap_or("");

        match kind {
            "file" => file = line.split('|').nth(1).unwrap_or("").to_string(),
            "warn" => {}
            "pending" | "rust-error" | "error" => {
                let parts: Vec<&str> = line.splitn(4, '|').collect();

                if parts.len() < 4 {
                    continue;
                }

                if kind == "error" {
                    let what = parts[3].split(':').next().unwrap_or("").to_string();
                    p.ref_errors
                        .entry(format!("{file}|{}|{}", parts[1], parts[2]))
                        .or_default()
                        .push(what);
                } else {
                    let (field, value) = parts[3].split_once('=').unwrap_or((parts[3], ""));
                    let k = format!("{file}|{}|{}|{field}", parts[1], parts[2]);
                    let target = if kind == "pending" {
                        &mut p.pending
                    } else {
                        &mut p.errors
                    };
                    target.insert(k, value.to_string());
                }
            }
            _ => {
                let parts: Vec<&str> = line.splitn(3, '|').collect();

                if parts.len() < 3 {
                    continue;
                }

                let (field, value) = parts[2].split_once('=').unwrap_or((parts[2], ""));
                let k = format!("{file}|{}|{}|{field}", parts[0], parts[1]);
                p.order.push(k.clone());
                p.values.insert(k, value.to_string());
            }
        }
    }

    p
}

/// `fnmatch` with `*` only.
fn glob(pat: &str, s: &str) -> bool {
    match pat.split_once('*') {
        None => pat == s,
        Some((head, rest)) => {
            let Some(tail) = s.strip_prefix(head) else {
                return false;
            };
            (0..=tail.len())
                .filter(|&i| tail.is_char_boundary(i))
                .any(|i| glob(rest, &tail[i..]))
        }
    }
}

fn split_head(k: &str) -> (&str, &str) {
    k.rsplit_once('|').unwrap_or((k, ""))
}

/// The result of [`compare`].
pub struct Comparison {
    /// Counts per group (`SECTION|KEY` for defaults, else `SECTION`) and kind.
    pub groups: BTreeMap<String, BTreeMap<Kind, usize>>,
    /// Every classified line: (kind, key, ref value, rust value).
    pub lines: Vec<(Kind, String, Option<String>, Option<String>)>,
}

impl Comparison {
    pub fn total(&self, kind: Kind) -> usize {
        self.groups
            .values()
            .map(|g| g.get(&kind).copied().unwrap_or(0))
            .sum()
    }

    pub fn failures(&self) -> usize {
        Kind::ALL
            .iter()
            .filter(|k| k.is_failure())
            .map(|k| self.total(*k))
            .sum()
    }

    /// The scoreboard as text, followed by the first `show` problems of each failing kind.
    pub fn report(&self, show: usize) -> String {
        let mut r = String::new();
        let w = self.groups.keys().map(String::len).max().unwrap_or(5).max(5);
        let _ = write!(r, "{:<w$}", "group");

        for k in Kind::ALL {
            let _ = write!(r, " {:>10}", k.name());
        }

        r.push('\n');

        for (g, counts) in &self.groups {
            let _ = write!(r, "{g:<w$}");

            for k in Kind::ALL {
                let _ = write!(r, " {:>10}", counts.get(&k).copied().unwrap_or(0));
            }

            r.push('\n');
        }

        let _ = write!(r, "{:<w$}", "TOTAL");

        for k in Kind::ALL {
            let _ = write!(r, " {:>10}", self.total(k));
        }

        r.push('\n');

        for kind in Kind::ALL.into_iter().filter(|k| k.is_failure()) {
            for (_, k, rv, sv) in self.lines.iter().filter(|l| l.0 == kind).take(show) {
                let _ = writeln!(
                    r,
                    "{}: {k}\n    ref:  {}\n    rust: {}",
                    kind.name(),
                    rv.as_deref().unwrap_or("-"),
                    sv.as_deref().unwrap_or("-")
                );
            }
        }

        r
    }
}

/// Compare a C++ dump (`reference`) with a Rust dump (`rust`), like `compare.py`.
pub fn compare(reference: &str, rust: &str) -> Comparison {
    let r = parse(reference);
    let s = parse(rust);
    let mut patterns: BTreeMap<&str, Vec<(&str, Kind, &str)>> = BTreeMap::new();

    for (source, kind) in [(&s.pending, Kind::Pending), (&s.errors, Kind::RustError)] {
        for (k, v) in source {
            let (head, field) = split_head(k);

            if field.contains('*') {
                patterns.entry(head).or_default().push((field, kind, v));
            }
        }
    }

    let pattern_failure = |k: &str| {
        let (head, field) = split_head(k);
        patterns
            .get(head)
            .and_then(|v| v.iter().find(|(p, _, _)| glob(p, field)))
            .map(|(_, kind, v)| (*kind, (*v).to_string()))
    };
    let covered_by_ref_error = |k: &str| {
        let (head, field) = split_head(k);
        let parts: Vec<&str> = field.split('.').collect();
        r.ref_errors.get(head).is_some_and(|whats| {
            whats.iter().any(|w| {
                let wp: Vec<&str> = w.split('.').collect();
                parts.windows(wp.len()).any(|win| win == wp.as_slice())
            })
        })
    };

    let mut lines = Vec::new();

    for k in &r.order {
        let v = &r.values[k];
        let rv = Some(v.clone());
        let line = if let Some(sv) = s.values.get(k) {
            let kind = if sv == v { Kind::Match } else { Kind::Mismatch };
            (kind, k.clone(), rv, Some(sv.clone()))
        } else if let Some(p) = s.pending.get(k) {
            (Kind::Pending, k.clone(), rv, Some(p.clone()))
        } else if let Some(e) = s.errors.get(k) {
            (Kind::RustError, k.clone(), rv, Some(e.clone()))
        } else if let Some((kind, p)) = pattern_failure(k) {
            (kind, k.clone(), rv, Some(p))
        } else {
            (Kind::Missing, k.clone(), rv, None)
        };
        lines.push(line);
    }

    for (source, is_value) in [(&s.values, true), (&s.pending, false), (&s.errors, false)] {
        for (k, v) in source {
            if r.values.contains_key(k) || split_head(k).1.contains('*') {
                continue;
            }

            if covered_by_ref_error(k) {
                lines.push((Kind::RefError, k.clone(), None, Some(v.clone())));
            } else if is_value {
                lines.push((Kind::Extra, k.clone(), None, Some(v.clone())));
            }
        }
    }

    let by_class = lines
        .iter()
        .all(|l| l.1.split('|').nth(1) == Some("defaults"));
    let mut groups: BTreeMap<String, BTreeMap<Kind, usize>> = BTreeMap::new();

    for (kind, k, _, _) in &lines {
        let mut it = k.split('|').skip(1);
        let section = it.next().unwrap_or("");
        let g = if by_class {
            format!("{section}|{}", it.next().unwrap_or(""))
        } else {
            section.to_string()
        };
        *groups.entry(g).or_default().entry(*kind).or_default() += 1;
    }

    Comparison { groups, lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        let reference = "# comment\ns|k|a=1\ns|k|b=2\ns|k|c=3\ns|k|d=4\ns|k|x.F.Cu=5\ns|k|y=6\nerror|s|k|tpoly: boom\n";
        let rust = "s|k|a=1\ns|k|b=9\npending|s|k|c=X::C\nrust-error|s|k|d=oops\npending|s|k|*.F.Cu=X::L\ns|k|z=7\ns|k|tpoly.F.Cu.c0=8\n";
        let c = compare(reference, rust);
        assert_eq!(c.total(Kind::Match), 1);
        assert_eq!(c.total(Kind::Mismatch), 1);
        assert_eq!(c.total(Kind::Pending), 2);
        assert_eq!(c.total(Kind::RustError), 1);
        assert_eq!(c.total(Kind::Missing), 1);
        assert_eq!(c.total(Kind::Extra), 1);
        assert_eq!(c.total(Kind::RefError), 1);
        assert!(glob("*.F.Cu", "shape.F.Cu") && !glob("*.F.Cu", "shape.B.Cu"));
    }
}
