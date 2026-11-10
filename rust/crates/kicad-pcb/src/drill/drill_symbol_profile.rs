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

//! Port of `pcbnew/drill/drill_symbol_profile.h` / `.cpp`: `DRILL_GROUP_KEY`,
//! `DRILL_MARK_MODE`, `DRILL_MARK_POLICY`, `DRILL_SYMBOL_ASSIGNMENT`, `DRILL_SYMBOL_PROFILE`
//! (incl. `GroupKeyString` and `Fingerprint`) and the token functions.
//!
//! The board keeps its profile in the canonical s-expression form of the file I/O packages
//! (`BoardDesignSettings::drill_symbol_profile: Option<Sexpr>`, see
//! `io/parser/setup_drill_profile.rs`). That file's crate-private reader / writer of the form is
//! compiled in here a third time (`model_form`), so the board profile is read
//! ([`DrillSymbolProfile::from_board`], `GetDrillSymbolProfile()`) and written back
//! ([`DrillSymbolProfile::commit_to_board`], the assignment of `DRILL_PROFILE_PENDING::Commit`)
//! exactly as the parser and the writer see it. The `KIID m_uuid` member is not kept: the board
//! file does not carry it and `operator==` ignores it.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::PCB_IU_SCALE;
use kicad_core::layer_ids::copper_layer_to_ordinal;

use crate::board_design_settings::BoardDesignSettings;

use super::drill_operation::DrillOperation;

#[path = "../io/parser/setup_drill_profile.rs"]
mod model_form;

/// `DRILL_GROUP_KEY`: which properties split holes into separate chart rows and symbols.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DrillGroupKey {
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(i32)]
pub enum DrillMarkMode {
    #[default]
    Shape = 0,
    Letter,
    SizeText,
}

/// `DRILL_MARK_POLICY`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(i32)]
pub enum DrillMarkPolicy {
    Shapes = 0,
    Letters,
    ShapesThenLetters,
    SizeText,
}

/// `DRILL_SYMBOL_ASSIGNMENT`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DrillSymbolAssignment {
    pub mark_mode: DrillMarkMode, // m_MarkMode
    pub shape_index: i32,         // m_ShapeIndex
    pub letter: String,           // m_Letter
    pub description: String,      // m_Description
}

/// `DRILL_SYMBOL_PROFILE`: grouping rules and symbol assignments, shared by the drill charts
/// and the drill maps of a board.
#[derive(Clone, Debug)]
pub struct DrillSymbolProfile {
    name: String,                      // m_name
    group_by: BTreeSet<DrillGroupKey>, // m_groupBy (std::set)
    mark_policy: DrillMarkPolicy,      // m_markPolicy
    symbol_size: i32,                  // m_symbolSize
    symbol_width: i32,                 // m_symbolWidth
    freeze_assignments: bool,          // m_freezeAssignments
    /// m_assignments (`std::map<std::string, ...>`: byte order of the keys).
    assignments: BTreeMap<String, DrillSymbolAssignment>,
}

impl Default for DrillSymbolProfile {
    /// Port of `DRILL_SYMBOL_PROFILE::DRILL_SYMBOL_PROFILE()`.
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
    /// Port of `DRILL_SYMBOL_PROFILE::operator==` (name, keys, policy, sizes, freeze and every
    /// assignment).
    fn eq(&self, other: &Self) -> bool {
        if self.name != other.name
            || self.group_by != other.group_by
            || self.mark_policy != other.mark_policy
            || self.symbol_size != other.symbol_size
            || self.symbol_width != other.symbol_width
            || self.freeze_assignments != other.freeze_assignments
        {
            return false;
        }

        if self.assignments.len() != other.assignments.len() {
            return false;
        }

        for (key, assignment) in &self.assignments {
            match other.assignment(key) {
                Some(o) if o == assignment => {}
                _ => return false,
            }
        }

        true
    }
}

impl DrillSymbolProfile {
    /// `GetName()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `SetName()`.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// `IsGroupedBy( aKey )`.
    pub fn is_grouped_by(&self, key: DrillGroupKey) -> bool {
        self.group_by.contains(&key)
    }

    /// Port of `SetGroupedBy( aKey, aOn )`.
    pub fn set_grouped_by(&mut self, key: DrillGroupKey, on: bool) {
        if on {
            self.group_by.insert(key);
        } else {
            self.group_by.remove(&key);
        }
    }

    /// `GroupKeys()`.
    pub fn group_keys(&self) -> &BTreeSet<DrillGroupKey> {
        &self.group_by
    }

    /// `GetMarkPolicy()`.
    pub fn mark_policy(&self) -> DrillMarkPolicy {
        self.mark_policy
    }

    /// `SetMarkPolicy()`.
    pub fn set_mark_policy(&mut self, policy: DrillMarkPolicy) {
        self.mark_policy = policy;
    }

    /// `GetSymbolSize()`.
    pub fn symbol_size(&self) -> i32 {
        self.symbol_size
    }

    /// `SetSymbolSize()`.
    pub fn set_symbol_size(&mut self, size: i32) {
        self.symbol_size = size;
    }

    /// `GetSymbolWidth()`.
    pub fn symbol_width(&self) -> i32 {
        self.symbol_width
    }

    /// `SetSymbolWidth()`.
    pub fn set_symbol_width(&mut self, width: i32) {
        self.symbol_width = width;
    }

    /// `GetFreezeAssignments()`.
    pub fn freeze_assignments(&self) -> bool {
        self.freeze_assignments
    }

    /// `SetFreezeAssignments()`.
    pub fn set_freeze_assignments(&mut self, freeze: bool) {
        self.freeze_assignments = freeze;
    }

    /// `Assignments()`.
    pub fn assignments(&self) -> &BTreeMap<String, DrillSymbolAssignment> {
        &self.assignments
    }

    /// Port of `GetAssignment( aKey )` (`None` = nullptr).
    pub fn assignment(&self, key: &str) -> Option<&DrillSymbolAssignment> {
        self.assignments.get(key)
    }

    /// Port of `SetAssignment( aKey, aAssignment )`.
    pub fn set_assignment(&mut self, key: &str, assignment: DrillSymbolAssignment) {
        self.assignments.insert(key.to_string(), assignment);
    }

    /// `ClearAssignments()`. Assignments whose key no longer matches any hole are kept by
    /// everything else, so deleting and restoring a hole size does not shuffle the chart.
    pub fn clear_assignments(&mut self) {
        self.assignments.clear();
    }

    /// Port of `GroupKeyString( aOperation )`: stable identity for a group under the currently
    /// enabled keys.
    ///
    /// Built from nanometre integers and copper ordinals rather than layer names, so renaming a
    /// layer or switching display units cannot silently reassign a symbol. The leading version
    /// lets a later key-format change migrate instead of resetting.
    pub fn group_key_string(&self, op: &DrillOperation) -> String {
        let mut key = String::from("v1");

        if self.is_grouped_by(DrillGroupKey::SizeGrp) {
            key += &format!("|d{}", op.diameter);
        }

        if self.is_grouped_by(DrillGroupKey::Slot) {
            if op.is_slot {
                key += &format!("|s{}x{}", op.size_xy.x, op.size_xy.y);
            } else {
                key += "|s-";
            }
        }

        if self.is_grouped_by(DrillGroupKey::Plating) {
            key += if op.not_plated { "|p0" } else { "|p1" };
        }

        if self.is_grouped_by(DrillGroupKey::Span) {
            // Copper ordinals, not layer names, so a stackup rename cannot invalidate the key
            key += &format!(
                "|L{}-{}",
                copper_layer_to_ordinal(op.top_layer),
                copper_layer_to_ordinal(op.bottom_layer)
            );
        }

        if self.is_grouped_by(DrillGroupKey::Operation) {
            key += &format!("|k{}", op.kind as i32);
        }

        if self.is_grouped_by(DrillGroupKey::HoleFunction) {
            key += &format!("|a{}", op.attribute as i32);
        }

        if self.is_grouped_by(DrillGroupKey::Protection) {
            let b = |v: bool| i32::from(v);
            key += &format!(
                "|r{}{}{}{}{}{}{}{}",
                b(op.filled),
                b(op.capped),
                b(op.top_covered),
                b(op.bottom_covered),
                b(op.top_plugged),
                b(op.bottom_plugged),
                b(op.top_tented),
                b(op.bottom_tented)
            );
        }

        if self.is_grouped_by(DrillGroupKey::PostMachining) {
            let front = &op.front_post_machining;
            let back = &op.back_post_machining;

            key += &format!(
                "|m{}:{}:{}:{}/{}:{}:{}:{}",
                front.mode as i32,
                front.size,
                front.depth,
                front.angle,
                back.mode as i32,
                back.size,
                back.depth,
                back.angle
            );
        }

        key
    }

    /// Port of `Fingerprint()`: a cheap FNV-1a style value used to notice an edit (not stored).
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 1469598103934665603;

        let mut mix = |value: u64| {
            hash ^= value;
            hash = hash.wrapping_mul(1099511628211);
        };

        for key in &self.group_by {
            mix(*key as i32 as u64);
        }

        mix(self.mark_policy as i32 as u64);
        mix(self.symbol_size as i64 as u64);
        mix(self.symbol_width as i64 as u64);
        mix(u64::from(self.freeze_assignments));

        for (key, assignment) in &self.assignments {
            for ch in key.bytes() {
                mix(u64::from(ch));
            }

            mix(assignment.mark_mode as i32 as u64);
            mix(assignment.shape_index as i64 as u64);

            // Letter and description are drawn, so a change to either has to invalidate the
            // caches keyed on this value
            for ch in assignment.letter.chars() {
                mix(u64::from(ch));
            }

            mix(assignment.letter.chars().count() as u64);

            for ch in assignment.description.chars() {
                mix(u64::from(ch));
            }

            mix(assignment.description.chars().count() as u64);
        }

        hash
    }

    /// `aBoard.GetDesignSettings().GetDrillSymbolProfile()`: the profile the board holds (the
    /// default profile when the board stores none).
    pub fn from_board(settings: &BoardDesignSettings) -> DrillSymbolProfile {
        let form = model_form::DrillSymbolProfile::from_model(settings.drill_symbol_profile.as_ref());
        DrillSymbolProfile::from_form(&form)
    }

    /// `aBoard.GetDesignSettings().GetDrillSymbolProfile() = *this` (`DRILL_PROFILE_PENDING::
    /// Commit`), in the board's canonical form (`None` for the default profile).
    pub fn commit_to_board(&self, settings: &mut BoardDesignSettings) {
        settings.drill_symbol_profile = self.to_form().to_model();
    }

    /// The I/O packages' value of the same profile.
    fn from_form(form: &model_form::DrillSymbolProfile) -> DrillSymbolProfile {
        use model_form::{DrillGroupKey as K, DrillMarkMode as M, DrillMarkPolicy as P};

        let key = |k: &K| match k {
            K::SizeGrp => DrillGroupKey::SizeGrp,
            K::Slot => DrillGroupKey::Slot,
            K::Plating => DrillGroupKey::Plating,
            K::Span => DrillGroupKey::Span,
            K::Operation => DrillGroupKey::Operation,
            K::HoleFunction => DrillGroupKey::HoleFunction,
            K::Protection => DrillGroupKey::Protection,
            K::PostMachining => DrillGroupKey::PostMachining,
        };
        let mode = |m: M| match m {
            M::Shape => DrillMarkMode::Shape,
            M::Letter => DrillMarkMode::Letter,
            M::SizeText => DrillMarkMode::SizeText,
        };

        DrillSymbolProfile {
            name: form.name.clone(),
            group_by: form.group_by.iter().map(key).collect(),
            mark_policy: match form.mark_policy {
                P::Shapes => DrillMarkPolicy::Shapes,
                P::Letters => DrillMarkPolicy::Letters,
                P::ShapesThenLetters => DrillMarkPolicy::ShapesThenLetters,
                P::SizeText => DrillMarkPolicy::SizeText,
            },
            symbol_size: form.symbol_size,
            symbol_width: form.symbol_width,
            freeze_assignments: form.freeze_assignments,
            assignments: form
                .assignments
                .iter()
                .map(|(k, a)| {
                    (
                        k.clone(),
                        DrillSymbolAssignment {
                            mark_mode: mode(a.mark_mode),
                            shape_index: a.shape_index,
                            letter: a.letter.clone(),
                            description: a.description.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// The inverse of [`from_form`](Self::from_form).
    fn to_form(&self) -> model_form::DrillSymbolProfile {
        use model_form::{DrillGroupKey as K, DrillMarkMode as M, DrillMarkPolicy as P};

        let key = |k: &DrillGroupKey| match k {
            DrillGroupKey::SizeGrp => K::SizeGrp,
            DrillGroupKey::Slot => K::Slot,
            DrillGroupKey::Plating => K::Plating,
            DrillGroupKey::Span => K::Span,
            DrillGroupKey::Operation => K::Operation,
            DrillGroupKey::HoleFunction => K::HoleFunction,
            DrillGroupKey::Protection => K::Protection,
            DrillGroupKey::PostMachining => K::PostMachining,
        };
        let mode = |m: DrillMarkMode| match m {
            DrillMarkMode::Shape => M::Shape,
            DrillMarkMode::Letter => M::Letter,
            DrillMarkMode::SizeText => M::SizeText,
        };

        model_form::DrillSymbolProfile {
            name: self.name.clone(),
            group_by: self.group_by.iter().map(key).collect(),
            mark_policy: match self.mark_policy {
                DrillMarkPolicy::Shapes => P::Shapes,
                DrillMarkPolicy::Letters => P::Letters,
                DrillMarkPolicy::ShapesThenLetters => P::ShapesThenLetters,
                DrillMarkPolicy::SizeText => P::SizeText,
            },
            symbol_size: self.symbol_size,
            symbol_width: self.symbol_width,
            freeze_assignments: self.freeze_assignments,
            assignments: self
                .assignments
                .iter()
                .map(|(k, a)| {
                    (
                        k.clone(),
                        model_form::DrillSymbolAssignment {
                            mark_mode: mode(a.mark_mode),
                            shape_index: a.shape_index,
                            letter: a.letter.clone(),
                            description: a.description.clone(),
                        },
                    )
                })
                .collect(),
        }
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

/// `tokenFor`: the token of a value (the first entry's for an unknown value).
fn token_for<V: PartialEq + Copy>(map: &[(&'static str, V)], value: V) -> &'static str {
    map.iter()
        .find(|(_, v)| *v == value)
        .map_or(map[0].0, |(t, _)| t)
}

/// `valueFor`: the value of a token (`wxString::IsSameAs`, case sensitive).
fn value_for<V: Copy>(map: &[(&'static str, V)], token: &str) -> Option<V> {
    map.iter().find(|(t, _)| *t == token).map(|(_, v)| *v)
}

/// `DrillGroupKeyToken( aKey )`.
pub fn drill_group_key_token(key: DrillGroupKey) -> &'static str {
    token_for(&GROUP_KEY_TOKENS, key)
}

/// `DrillMarkPolicyToken( aPolicy )`.
pub fn drill_mark_policy_token(policy: DrillMarkPolicy) -> &'static str {
    token_for(&MARK_POLICY_TOKENS, policy)
}

/// `DrillMarkModeToken( aMode )`.
pub fn drill_mark_mode_token(mode: DrillMarkMode) -> &'static str {
    token_for(&MARK_MODE_TOKENS, mode)
}

/// `DrillGroupKeyFromToken( aToken, aKey )` (`None` = returns false).
pub fn drill_group_key_from_token(token: &str) -> Option<DrillGroupKey> {
    value_for(&GROUP_KEY_TOKENS, token)
}

/// `DrillMarkPolicyFromToken( aToken, aPolicy )` (`None` = returns false).
pub fn drill_mark_policy_from_token(token: &str) -> Option<DrillMarkPolicy> {
    value_for(&MARK_POLICY_TOKENS, token)
}

/// `DrillMarkModeFromToken( aToken, aMode )` (`None` = returns false).
pub fn drill_mark_mode_from_token(token: &str) -> Option<DrillMarkMode> {
    value_for(&MARK_MODE_TOKENS, token)
}
