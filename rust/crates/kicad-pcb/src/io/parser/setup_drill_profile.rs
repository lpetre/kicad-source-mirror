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

//! Port of the data part of `pcbnew/drill/drill_symbol_profile.cpp` / `.h`
//! (`DRILL_SYMBOL_PROFILE` without `GroupKeyString` / `Fingerprint`, the token tables and
//! their lookups), as far as the board file reader and writer (package PCBIO-04) need it.
//!
//! The board model keeps the profile as an s-expression
//! (`BoardDesignSettings::drill_symbol_profile: Option<Sexpr>`) until the tier-2 drill symbol
//! package ports the class. The reader therefore parses into [`DrillSymbolProfile`] exactly
//! like `parseDrillSymbolProfile`, and stores the result in the model in a canonical form
//! ([`DrillSymbolProfile::to_sexpr`], `None` for a profile equal to the default); the writer
//! reads that form back ([`DrillSymbolProfile::from_model`]) and formats it like
//! `formatDrillSymbolProfile`. This file is compiled into both the parser and the writer
//! module (`#[path]`), so each side sees the same type.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::PCB_IU_SCALE;
use kicad_sexpr::{Sexpr, SexprInteger, SexprList, SexprString, SexprSymbol};

/// `DRILL_GROUP_KEY`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum DrillGroupKey {
    SizeGrp = 0,
    Slot,
    Plating,
    Span,
    Operation,
    HoleFunction,
    Protection,
    PostMachining,
}

/// `DRILL_MARK_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum DrillMarkMode {
    #[default]
    Shape = 0,
    Letter,
    SizeText,
}

/// `DRILL_MARK_POLICY`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DrillMarkPolicy {
    Shapes = 0,
    Letters,
    ShapesThenLetters,
    SizeText,
}

/// `DRILL_SYMBOL_ASSIGNMENT`.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct DrillSymbolAssignment {
    pub mark_mode: DrillMarkMode, // m_MarkMode
    pub shape_index: i32,         // m_ShapeIndex
    pub letter: String,           // m_Letter
    pub description: String,     // m_Description
}

/// `DRILL_SYMBOL_PROFILE` (without the KIID, which is neither read nor written by the board
/// file I/O, and without `GroupKeyString` / `Fingerprint`).
#[derive(Clone, Debug)]
pub(crate) struct DrillSymbolProfile {
    pub name: String,                     // m_name
    pub group_by: BTreeSet<DrillGroupKey>, // m_groupBy (std::set)
    pub mark_policy: DrillMarkPolicy,     // m_markPolicy
    pub symbol_size: i32,                 // m_symbolSize
    pub symbol_width: i32,                // m_symbolWidth
    pub freeze_assignments: bool,         // m_freezeAssignments
    /// `m_assignments` (`std::map<std::string, ...>`: byte order of the keys).
    pub assignments: BTreeMap<String, DrillSymbolAssignment>,
}

impl Default for DrillSymbolProfile {
    /// `DRILL_SYMBOL_PROFILE::DRILL_SYMBOL_PROFILE()`.
    fn default() -> Self {
        DrillSymbolProfile {
            name: String::new(),
            group_by: [
                DrillGroupKey::SizeGrp,
                DrillGroupKey::Slot,
                DrillGroupKey::Plating,
                DrillGroupKey::Span,
                DrillGroupKey::Operation,
                DrillGroupKey::PostMachining,
            ]
            .into_iter()
            .collect(),
            mark_policy: DrillMarkPolicy::ShapesThenLetters,
            symbol_size: PCB_IU_SCALE.mm_to_iu(1.5),
            symbol_width: PCB_IU_SCALE.mm_to_iu(0.15),
            freeze_assignments: true,
            assignments: BTreeMap::new(),
        }
    }
}

impl PartialEq for DrillSymbolProfile {
    /// Port of `DRILL_SYMBOL_PROFILE::operator==` (the KIID is not compared).
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.group_by == other.group_by
            && self.mark_policy == other.mark_policy
            && self.symbol_size == other.symbol_size
            && self.symbol_width == other.symbol_width
            && self.freeze_assignments == other.freeze_assignments
            && self.assignments == other.assignments
    }
}

/// `groupKeyTokens`.
const GROUP_KEY_TOKENS: [(&str, DrillGroupKey); 8] = [
    ("size", DrillGroupKey::SizeGrp),
    ("slot", DrillGroupKey::Slot),
    ("plating", DrillGroupKey::Plating),
    ("span", DrillGroupKey::Span),
    ("operation", DrillGroupKey::Operation),
    ("hole_function", DrillGroupKey::HoleFunction),
    ("protection", DrillGroupKey::Protection),
    ("post_machining", DrillGroupKey::PostMachining),
];

/// `markPolicyTokens`.
const MARK_POLICY_TOKENS: [(&str, DrillMarkPolicy); 4] = [
    ("shapes", DrillMarkPolicy::Shapes),
    ("letters", DrillMarkPolicy::Letters),
    ("shapes_then_letters", DrillMarkPolicy::ShapesThenLetters),
    ("size_text", DrillMarkPolicy::SizeText),
];

/// `markModeTokens`.
const MARK_MODE_TOKENS: [(&str, DrillMarkMode); 3] = [
    ("shape", DrillMarkMode::Shape),
    ("letter", DrillMarkMode::Letter),
    ("size_text", DrillMarkMode::SizeText),
];

/// `tokenFor`: the token of a value (the first entry for an unknown value).
fn token_for<V: PartialEq + Copy>(map: &[(&'static str, V)], value: V) -> &'static str {
    map.iter()
        .find(|(_, v)| *v == value)
        .map_or(map[0].0, |(t, _)| t)
}

/// `valueFor`: the value of a token (`wxString::IsSameAs`, case sensitive).
fn value_for<V: Copy>(map: &[(&'static str, V)], token: &str) -> Option<V> {
    map.iter().find(|(t, _)| *t == token).map(|(_, v)| *v)
}

/// `DrillGroupKeyToken`.
pub(crate) fn drill_group_key_token(key: DrillGroupKey) -> &'static str {
    token_for(&GROUP_KEY_TOKENS, key)
}

/// `DrillMarkPolicyToken`.
pub(crate) fn drill_mark_policy_token(policy: DrillMarkPolicy) -> &'static str {
    token_for(&MARK_POLICY_TOKENS, policy)
}

/// `DrillMarkModeToken`.
pub(crate) fn drill_mark_mode_token(mode: DrillMarkMode) -> &'static str {
    token_for(&MARK_MODE_TOKENS, mode)
}

/// `DrillGroupKeyFromToken` (`None` = returns false).
pub(crate) fn drill_group_key_from_token(token: &str) -> Option<DrillGroupKey> {
    value_for(&GROUP_KEY_TOKENS, token)
}

/// `DrillMarkPolicyFromToken` (`None` = returns false).
pub(crate) fn drill_mark_policy_from_token(token: &str) -> Option<DrillMarkPolicy> {
    value_for(&MARK_POLICY_TOKENS, token)
}

/// `DrillMarkModeFromToken` (`None` = returns false).
pub(crate) fn drill_mark_mode_from_token(token: &str) -> Option<DrillMarkMode> {
    value_for(&MARK_MODE_TOKENS, token)
}

impl DrillSymbolProfile {
    /// `SetGroupedBy( aKey, aOn )`.
    pub(crate) fn set_grouped_by(&mut self, key: DrillGroupKey, on: bool) {
        if on {
            self.group_by.insert(key);
        } else {
            self.group_by.remove(&key);
        }
    }

    /// `SetAssignment( aKey, aAssignment )`.
    pub(crate) fn set_assignment(&mut self, key: &str, assignment: DrillSymbolAssignment) {
        self.assignments.insert(key.to_string(), assignment);
    }

    /// The profile held by the model: the default profile for `None`, else the canonical
    /// s-expression written by [`to_sexpr`](Self::to_sexpr) read back. Unknown or malformed
    /// parts of a node that was not written by `to_sexpr` keep their default values.
    pub(crate) fn from_model(node: Option<&Sexpr>) -> DrillSymbolProfile {
        let mut profile = DrillSymbolProfile::default();

        let Some(Sexpr::List(list)) = node else {
            return profile;
        };

        profile.group_by.clear();

        for child in list.children.iter().skip(1) {
            let Sexpr::List(entry) = child else {
                continue;
            };

            let Some(key) = entry.children.first().and_then(symbol_text) else {
                continue;
            };

            let args = &entry.children[1..];

            match key {
                "name" => {
                    if let Some(s) = args.first().and_then(string_text) {
                        profile.name = s.to_string();
                    }
                }
                "group_by" => {
                    for arg in args {
                        if let Some(k) = symbol_text(arg).and_then(drill_group_key_from_token) {
                            profile.group_by.insert(k);
                        }
                    }
                }
                "default_marks" => {
                    if let Some(p) = args
                        .first()
                        .and_then(symbol_text)
                        .and_then(drill_mark_policy_from_token)
                    {
                        profile.mark_policy = p;
                    }
                }
                "size" => {
                    if let Some(v) = args.first().and_then(integer_value) {
                        profile.symbol_size = v;
                    }
                }
                "width" => {
                    if let Some(v) = args.first().and_then(integer_value) {
                        profile.symbol_width = v;
                    }
                }
                "freeze_assignments" => {
                    if let Some(v) = args.first().and_then(symbol_text) {
                        profile.freeze_assignments = v == "yes";
                    }
                }
                "assignment" => {
                    let mut key = String::new();
                    let mut assignment = DrillSymbolAssignment::default();

                    for part in args {
                        let Sexpr::List(part) = part else {
                            continue;
                        };

                        let values = &part.children[1.min(part.children.len())..];

                        match part.children.first().and_then(symbol_text) {
                            Some("key") => {
                                if let Some(s) = values.first().and_then(string_text) {
                                    key = s.to_string();
                                }
                            }
                            Some("mark") => {
                                if let Some(m) = values
                                    .first()
                                    .and_then(symbol_text)
                                    .and_then(drill_mark_mode_from_token)
                                {
                                    assignment.mark_mode = m;
                                }

                                match values.get(1) {
                                    Some(Sexpr::Integer(i)) => {
                                        assignment.shape_index = i.value as i32
                                    }
                                    Some(Sexpr::String(s)) => assignment.letter = s.value.clone(),
                                    _ => {}
                                }
                            }
                            Some("descr") => {
                                if let Some(s) = values.first().and_then(string_text) {
                                    assignment.description = s.to_string();
                                }
                            }
                            _ => {}
                        }
                    }

                    if !key.is_empty() {
                        profile.set_assignment(&key, assignment);
                    }
                }
                _ => {}
            }
        }

        profile
    }

    /// The canonical model form of the profile: `None` for the default profile (which the
    /// writer does not write), else a `(drill_symbol_profile ...)` list with the entries of
    /// `formatDrillSymbolProfile` in its order, lengths as integer IU and strings unescaped.
    pub(crate) fn to_model(&self) -> Option<Sexpr> {
        if *self == DrillSymbolProfile::default() {
            return None;
        }

        Some(self.to_sexpr())
    }

    /// See [`to_model`](Self::to_model).
    pub(crate) fn to_sexpr(&self) -> Sexpr {
        let mut list = SexprList::new();
        list.children.push(sym("drill_symbol_profile"));

        if !self.name.is_empty() {
            list.children.push(entry("name", vec![string(&self.name)]));
        }

        list.children.push(entry(
            "group_by",
            self.group_by
                .iter()
                .map(|k| sym(drill_group_key_token(*k)))
                .collect(),
        ));
        list.children.push(entry(
            "default_marks",
            vec![sym(drill_mark_policy_token(self.mark_policy))],
        ));
        list.children
            .push(entry("size", vec![integer(self.symbol_size)]));
        list.children
            .push(entry("width", vec![integer(self.symbol_width)]));
        list.children.push(entry(
            "freeze_assignments",
            vec![sym(if self.freeze_assignments { "yes" } else { "no" })],
        ));

        for (key, assignment) in &self.assignments {
            let mut mark = vec![sym(drill_mark_mode_token(assignment.mark_mode))];

            if assignment.mark_mode == DrillMarkMode::Shape {
                mark.push(integer(assignment.shape_index));
            } else if assignment.mark_mode == DrillMarkMode::Letter {
                mark.push(string(&assignment.letter));
            }

            let mut parts = vec![entry("key", vec![string(key)]), entry("mark", mark)];

            if !assignment.description.is_empty() {
                parts.push(entry("descr", vec![string(&assignment.description)]));
            }

            list.children.push(entry("assignment", parts));
        }

        Sexpr::List(list)
    }
}

fn sym(text: &str) -> Sexpr {
    Sexpr::Symbol(SexprSymbol::new(text))
}

fn string(text: &str) -> Sexpr {
    Sexpr::String(SexprString::new(text))
}

fn integer(value: i32) -> Sexpr {
    Sexpr::Integer(SexprInteger::new(i64::from(value)))
}

fn entry(key: &str, args: Vec<Sexpr>) -> Sexpr {
    let mut list = SexprList::new();
    list.children.push(sym(key));
    list.children.extend(args);
    Sexpr::List(list)
}

fn symbol_text(node: &Sexpr) -> Option<&str> {
    match node {
        Sexpr::Symbol(s) => Some(&s.value),
        _ => None,
    }
}

fn string_text(node: &Sexpr) -> Option<&str> {
    match node {
        Sexpr::String(s) => Some(&s.value),
        _ => None,
    }
}

fn integer_value(node: &Sexpr) -> Option<i32> {
    match node {
        Sexpr::Integer(i) => i32::try_from(i.value).ok(),
        _ => None,
    }
}
