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

//! Port of `include/netclass.h` and `common/netclass.cpp` (`NETCLASS`): data, constructor
//! defaults and accessors (package M0d), names, composites and comparisons (package M3).
//! `Serialize` / `Deserialize` (protobuf API) are not ported.

use std::sync::{Arc, OnceLock};

use kicad_core::{Color4D, PCB_IU_SCALE, SCH_IU_SCALE};

/// `NETCLASS::Default`: the name of the default net class.
pub const NETCLASS_DEFAULT_NAME: &str = "Default";

/// `DEFAULT_CLEARANCE` (`common/netclass.cpp`).
pub const DEFAULT_CLEARANCE: i32 = PCB_IU_SCALE.mm_to_iu(0.2);
/// `DEFAULT_VIA_DIAMETER`.
pub const DEFAULT_VIA_DIAMETER: i32 = PCB_IU_SCALE.mm_to_iu(0.6);
/// `DEFAULT_VIA_DRILL`.
pub const DEFAULT_VIA_DRILL: i32 = PCB_IU_SCALE.mm_to_iu(0.3);
/// `DEFAULT_UVIA_DIAMETER`.
pub const DEFAULT_UVIA_DIAMETER: i32 = PCB_IU_SCALE.mm_to_iu(0.3);
/// `DEFAULT_UVIA_DRILL`.
pub const DEFAULT_UVIA_DRILL: i32 = PCB_IU_SCALE.mm_to_iu(0.1);
/// `DEFAULT_TRACK_WIDTH`.
pub const DEFAULT_TRACK_WIDTH: i32 = PCB_IU_SCALE.mm_to_iu(0.2);
/// `DEFAULT_DIFF_PAIR_WIDTH`.
pub const DEFAULT_DIFF_PAIR_WIDTH: i32 = PCB_IU_SCALE.mm_to_iu(0.2);
/// `DEFAULT_DIFF_PAIR_GAP`.
pub const DEFAULT_DIFF_PAIR_GAP: i32 = PCB_IU_SCALE.mm_to_iu(0.25);
/// `DEFAULT_DIFF_PAIR_VIAGAP`.
pub const DEFAULT_DIFF_PAIR_VIAGAP: i32 = PCB_IU_SCALE.mm_to_iu(0.25);
/// `DEFAULT_WIRE_WIDTH` (schematic IU).
pub const DEFAULT_WIRE_WIDTH: i32 = SCH_IU_SCALE.mils_to_iu(6);
/// `DEFAULT_BUS_WIDTH` (schematic IU).
pub const DEFAULT_BUS_WIDTH: i32 = SCH_IU_SCALE.mils_to_iu(12);
/// `DEFAULT_LINE_STYLE` (solid).
pub const DEFAULT_LINE_STYLE: i32 = 0;

/// `NETCLASS` (`include/netclass.h`).
///
/// `constituents` ports `m_constituents` + `m_constituentOwners`. A C++ root class lists
/// itself (`m_constituents = { this }`), which an owned value cannot: here an **empty** list
/// means "root class" (the class is its own only constituent); a composite lists its
/// constituents in C++ order.
#[derive(Clone, Debug)]
pub struct Netclass {
    pub is_default: bool,                 // m_isDefault
    pub constituents: Vec<Arc<Netclass>>, // m_constituents + m_constituentOwners
    pub name: String,                     // m_Name
    pub priority: i32,                    // m_Priority
    pub description: String,              // m_Description
    pub clearance: Option<i32>,           // m_Clearance
    pub track_width: Option<i32>,         // m_TrackWidth
    pub via_dia: Option<i32>,             // m_ViaDia
    pub via_drill: Option<i32>,           // m_ViaDrill
    pub uvia_dia: Option<i32>,            // m_uViaDia
    pub uvia_drill: Option<i32>,          // m_uViaDrill
    pub diff_pair_width: Option<i32>,     // m_diffPairWidth
    pub diff_pair_gap: Option<i32>,       // m_diffPairGap
    pub diff_pair_via_gap: Option<i32>,   // m_diffPairViaGap
    pub wire_width: Option<i32>,          // m_wireWidth
    pub bus_width: Option<i32>,           // m_busWidth
    pub schematic_color: Color4D,         // m_schematicColor
    pub line_style: Option<i32>,          // m_lineStyle
    pub pcb_color: Color4D,               // m_pcbColor
    pub tuning_profile: String,           // m_tuningProfile
    pub parents: NetclassParents,
}

/// The `m_xxxParent` pointers: which (simple) netclass a value of an aggregate class came from.
/// `None` means "this class itself" (C++ `ResetParents()` sets the pointers to `this`).
#[derive(Clone, Debug, Default)]
pub struct NetclassParents {
    pub clearance: Option<Arc<Netclass>>,
    pub track_width: Option<Arc<Netclass>>,
    pub via_diameter: Option<Arc<Netclass>>,
    pub via_drill: Option<Arc<Netclass>>,
    pub uvia_dia: Option<Arc<Netclass>>,
    pub uvia_drill: Option<Arc<Netclass>>,
    pub diff_pair_width: Option<Arc<Netclass>>,
    pub diff_pair_gap: Option<Arc<Netclass>>,
    pub diff_pair_via_gap: Option<Arc<Netclass>>,
    pub pcb_color: Option<Arc<Netclass>>,
    pub wire_width: Option<Arc<Netclass>>,
    pub bus_width: Option<Arc<Netclass>>,
    pub schematic_color: Option<Arc<Netclass>>,
    pub line_style: Option<Arc<Netclass>>,
    pub tuning_profile: Option<Arc<Netclass>>,
}

impl Default for Netclass {
    /// C++ has no default constructor; this is `NETCLASS( NETCLASS::Default )` (defaults on).
    fn default() -> Self {
        Netclass::new(NETCLASS_DEFAULT_NAME, true)
    }
}

impl Netclass {
    /// `NETCLASS::NETCLASS( aName, aInitWithDefaults )` (`common/netclass.cpp`): priority -1,
    /// empty tuning profile, both colours `UNSPECIFIED`, the sized fields set to the
    /// `DEFAULT_*` values when `init_with_defaults`, parents reset to the class itself.
    pub fn new(name: &str, init_with_defaults: bool) -> Self {
        let value = |v: i32| init_with_defaults.then_some(v);

        Netclass {
            // SetName: m_isDefault only ever becomes true here.
            is_default: name == NETCLASS_DEFAULT_NAME,
            constituents: Vec::new(),
            name: name.to_string(),
            priority: -1,
            description: String::new(),
            clearance: value(DEFAULT_CLEARANCE),
            track_width: value(DEFAULT_TRACK_WIDTH),
            via_dia: value(DEFAULT_VIA_DIAMETER),
            via_drill: value(DEFAULT_VIA_DRILL),
            uvia_dia: value(DEFAULT_UVIA_DIAMETER),
            uvia_drill: value(DEFAULT_UVIA_DRILL),
            diff_pair_width: value(DEFAULT_DIFF_PAIR_WIDTH),
            diff_pair_gap: value(DEFAULT_DIFF_PAIR_GAP),
            diff_pair_via_gap: value(DEFAULT_DIFF_PAIR_VIAGAP),
            wire_width: value(DEFAULT_WIRE_WIDTH),
            bus_width: value(DEFAULT_BUS_WIDTH),
            schematic_color: Color4D::UNSPECIFIED,
            line_style: value(DEFAULT_LINE_STYLE),
            pcb_color: Color4D::UNSPECIFIED,
            tuning_profile: String::new(),
            parents: NetclassParents::default(),
        }
    }

    /// The static class `effective_netclass` falls back to when a board has neither a net
    /// class on the net nor a default class (`board-model.md` 10): `NETCLASS( Default )`.
    pub fn fallback() -> &'static Netclass {
        static FALLBACK: OnceLock<Netclass> = OnceLock::new();
        FALLBACK.get_or_init(Netclass::default)
    }

    /// `NETCLASS::SetName`: `m_isDefault` becomes true for the name `Default` (and is never
    /// cleared, as in C++).
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();

        if name == NETCLASS_DEFAULT_NAME {
            self.is_default = true;
        }
    }

    /// `NETCLASS::IsDefault`.
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// The number of entries of the C++ `m_constituents`: a root class lists itself (the empty
    /// Rust list), a composite its constituents.
    fn constituent_count(&self) -> usize {
        self.constituents.len().max(1)
    }

    /// Entry `i` of the C++ `m_constituents` (`self` for a root class).
    fn constituent(&self, i: usize) -> &Netclass {
        if self.constituents.is_empty() {
            self
        } else {
            &self.constituents[i]
        }
    }

    /// Port of `NETCLASS::GetName`: the root name, or the constituents' names (`m_Name`)
    /// joined with `,` in constituent order.
    pub fn get_name(&self) -> String {
        if self.constituent_count() == 1 {
            return self.name.clone();
        }

        // Count commas
        let str_len = self.constituent_count() - 1
            + self
                .constituents
                .iter()
                .map(|c| c.name.len())
                .sum::<usize>();

        let mut name = String::with_capacity(str_len);
        name.push_str(&self.constituents[0].name);

        for constituent in &self.constituents[1..] {
            name.push(',');
            name.push_str(&constituent.name);
        }

        name
    }

    /// Port of `NETCLASS::GetHumanReadableName` (the English originals of the translated
    /// formats).
    pub fn get_human_readable_name(&self) -> String {
        let count = self.constituent_count();

        if count == 1 {
            return self.name.clone();
        }

        let c = |i: usize| self.constituent(i).get_name();

        if count == 2 {
            format!("{} and {}", c(0), c(1))
        } else if count == 3 {
            format!("{}, {} and {}", c(0), c(1), c(2))
        } else {
            format!("{}, {} and {} more", c(0), c(1), count as i32 - 2)
        }
    }

    /// Port of `NETCLASS::NameEquals`: `get_name() == name` without building the string.
    pub fn name_equals(&self, other: &str) -> bool {
        if self.constituent_count() <= 1 {
            return self.name == other;
        }

        // Byte offsets: every match below ends on a character boundary of `other` (UTF-8 is
        // self-synchronising), so this equals the C++ character-offset comparison.
        let bytes = other.as_bytes();
        let mut offset = 0usize;

        for (i, constituent) in self.constituents.iter().enumerate() {
            if i > 0 {
                if offset >= bytes.len() || bytes[offset] != b',' {
                    return false;
                }

                offset += 1;
            }

            let constituent_name = constituent.name.as_bytes();

            // aName.compare( offset, len, constituentName ) != 0
            if !bytes[offset..].starts_with(constituent_name) {
                return false;
            }

            offset += constituent_name.len();
        }

        offset == bytes.len()
    }

    /// Port of `NETCLASS::ContainsNetclassWithName`: any constituent whose `GetName()` matches
    /// the wildcard pattern `netclass` (`wxString::Matches`).
    pub fn contains_netclass_with_name(&self, netclass: &str) -> bool {
        (0..self.constituent_count())
            .any(|i| wx_string_matches(&self.constituent(i).get_name(), netclass))
    }

    /// Port of `NETCLASS::operator==`: the constituent lists hold the same class objects.
    /// A root class is only equal to itself (C++ compares `{ this }`).
    pub fn same_constituents(&self, other: &Netclass) -> bool {
        if self.constituents.is_empty() || other.constituents.is_empty() {
            return std::ptr::eq(self, other)
                || (self.constituents.len() == 1
                    && other.constituents.is_empty()
                    && std::ptr::eq(&*self.constituents[0], other))
                || (other.constituents.len() == 1
                    && self.constituents.is_empty()
                    && std::ptr::eq(&*other.constituents[0], self));
        }

        self.constituents.len() == other.constituents.len()
            && self
                .constituents
                .iter()
                .zip(&other.constituents)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// Port of `NETCLASS::EqualsByPersistedFields`.
    pub fn equals_by_persisted_fields(&self, other: &Netclass) -> bool {
        // m_Description intentionally omitted: the project-file JSON writer does not
        // serialize description, so a load round-trip drops it.  Including it here
        // would mark every fresh load dirty (or every CopyFrom-produced copy unequal).
        //
        // This list is a hand-maintained mirror of the netclass JSON serialization in
        // NET_SETTINGS (common/project/net_settings.cpp, the saveNetclass/readNetClass
        // lambdas).  Any new persisted field added there must be added here too, or two
        // netclasses differing only in the new field will silently compare equal.
        self.name == other.name
            && self.priority == other.priority
            && self.clearance == other.clearance
            && self.track_width == other.track_width
            && self.via_dia == other.via_dia
            && self.via_drill == other.via_drill
            && self.uvia_dia == other.uvia_dia
            && self.uvia_drill == other.uvia_drill
            && self.diff_pair_width == other.diff_pair_width
            && self.diff_pair_gap == other.diff_pair_gap
            && self.diff_pair_via_gap == other.diff_pair_via_gap
            && self.wire_width == other.wire_width
            && self.bus_width == other.bus_width
            && self.schematic_color == other.schematic_color
            && self.line_style == other.line_style
            && self.pcb_color == other.pcb_color
            && self.tuning_profile == other.tuning_profile
    }

    /// Port of `NETCLASS::GetConstituentNetclasses` with the self reference made explicit:
    /// `[self]` for a root class.
    pub fn constituent_netclasses(&self) -> Vec<&Netclass> {
        (0..self.constituent_count())
            .map(|i| self.constituent(i))
            .collect()
    }

    /// Port of `NETCLASS::SetConstituentNetclasses` / `SetConstituentOwners`. Pass an empty
    /// list for C++ `{ this }`.
    pub fn set_constituent_netclasses(&mut self, constituents: Vec<Arc<Netclass>>) {
        self.constituents = constituents;
    }

    /// `NETCLASS::ResetParents`: every parent is the class itself again.
    pub fn reset_parents(&mut self) {
        self.parents = NetclassParents::default();
    }

    /// Port of `NETCLASS::ResetParameters`: clears every optional and both colours (not the
    /// tuning profile), then `ResetParents()`.
    pub fn reset_parameters(&mut self) {
        self.pcb_color = Color4D::UNSPECIFIED;
        self.schematic_color = Color4D::UNSPECIFIED;
        self.clearance = None;
        self.via_drill = None;
        self.uvia_drill = None;
        self.track_width = None;
        self.via_dia = None;
        self.uvia_dia = None;
        self.diff_pair_width = None;
        self.diff_pair_gap = None;
        self.diff_pair_via_gap = None;
        self.wire_width = None;
        self.bus_width = None;
        self.line_style = None;

        self.reset_parents();
    }

    /// `NETCLASS::GetPriority`.
    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// `NETCLASS::GetDescription`.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// `NETCLASS::HasClearance`.
    pub fn has_clearance(&self) -> bool {
        self.clearance.is_some()
    }

    /// `NETCLASS::GetClearance` (`-1` when unset, as `m_Clearance.value_or( -1 )`).
    pub fn clearance(&self) -> i32 {
        self.clearance.unwrap_or(-1)
    }

    /// `NETCLASS::HasTrackWidth`.
    pub fn has_track_width(&self) -> bool {
        self.track_width.is_some()
    }

    /// `NETCLASS::GetTrackWidth`.
    pub fn track_width(&self) -> i32 {
        self.track_width.unwrap_or(-1)
    }

    /// `NETCLASS::HasViaDiameter`.
    pub fn has_via_diameter(&self) -> bool {
        self.via_dia.is_some()
    }

    /// `NETCLASS::GetViaDiameter`.
    pub fn via_diameter(&self) -> i32 {
        self.via_dia.unwrap_or(-1)
    }

    /// `NETCLASS::HasViaDrill`.
    pub fn has_via_drill(&self) -> bool {
        self.via_drill.is_some()
    }

    /// `NETCLASS::GetViaDrill`.
    pub fn via_drill(&self) -> i32 {
        self.via_drill.unwrap_or(-1)
    }

    /// `NETCLASS::HasuViaDiameter`.
    pub fn has_uvia_diameter(&self) -> bool {
        self.uvia_dia.is_some()
    }

    /// `NETCLASS::GetuViaDiameter`.
    pub fn uvia_diameter(&self) -> i32 {
        self.uvia_dia.unwrap_or(-1)
    }

    /// `NETCLASS::HasuViaDrill`.
    pub fn has_uvia_drill(&self) -> bool {
        self.uvia_drill.is_some()
    }

    /// `NETCLASS::GetuViaDrill`.
    pub fn uvia_drill(&self) -> i32 {
        self.uvia_drill.unwrap_or(-1)
    }

    /// `NETCLASS::HasDiffPairWidth`.
    pub fn has_diff_pair_width(&self) -> bool {
        self.diff_pair_width.is_some()
    }

    /// `NETCLASS::GetDiffPairWidth`.
    pub fn diff_pair_width(&self) -> i32 {
        self.diff_pair_width.unwrap_or(-1)
    }

    /// `NETCLASS::HasDiffPairGap`.
    pub fn has_diff_pair_gap(&self) -> bool {
        self.diff_pair_gap.is_some()
    }

    /// `NETCLASS::GetDiffPairGap`.
    pub fn diff_pair_gap(&self) -> i32 {
        self.diff_pair_gap.unwrap_or(-1)
    }

    /// `NETCLASS::HasDiffPairViaGap`.
    pub fn has_diff_pair_via_gap(&self) -> bool {
        self.diff_pair_via_gap.is_some()
    }

    /// `NETCLASS::GetDiffPairViaGap`.
    pub fn diff_pair_via_gap(&self) -> i32 {
        self.diff_pair_via_gap.unwrap_or(-1)
    }

    /// `NETCLASS::HasWireWidth`.
    pub fn has_wire_width(&self) -> bool {
        self.wire_width.is_some()
    }

    /// `NETCLASS::GetWireWidth`.
    pub fn wire_width(&self) -> i32 {
        self.wire_width.unwrap_or(-1)
    }

    /// `NETCLASS::HasBusWidth`.
    pub fn has_bus_width(&self) -> bool {
        self.bus_width.is_some()
    }

    /// `NETCLASS::GetBusWidth`.
    pub fn bus_width(&self) -> i32 {
        self.bus_width.unwrap_or(-1)
    }

    /// `NETCLASS::HasLineStyle`.
    pub fn has_line_style(&self) -> bool {
        self.line_style.is_some()
    }

    /// `NETCLASS::GetLineStyle` (`0` when unset).
    pub fn line_style(&self) -> i32 {
        self.line_style.unwrap_or(0)
    }

    /// `NETCLASS::GetTuningProfile`.
    pub fn tuning_profile(&self) -> &str {
        &self.tuning_profile
    }

    /// `NETCLASS::HasTuningProfile`.
    pub fn has_tuning_profile(&self) -> bool {
        !self.tuning_profile.is_empty()
    }
}

/// Port of `wxString::Matches( mask )` (wxWidgets 3.2, `src/common/string.cpp`, the
/// non-regex branch): `*` and `?` wildcards, case sensitive, with wx's quirks kept: any `*` /
/// `?` run after a `*` is skipped (so `"*?"` matches the empty string), the literal run after a
/// `*` is found with `wxStrstr`, and only the last `*` is backtracked to.
pub fn wx_string_matches(text: &str, mask: &str) -> bool {
    // The C strings the wx code walks: up to the first NUL, NUL-terminated.
    fn c_chars(s: &str) -> Vec<char> {
        let mut v: Vec<char> = s.chars().take_while(|&c| c != '\0').collect();
        v.push('\0');
        v
    }

    let mask = c_chars(mask);
    let txt = c_chars(text);
    let mut m = 0usize;
    let mut t = 0usize;
    let mut last_star_in_text: Option<usize> = None;
    let mut last_star_in_mask = 0usize;

    loop {
        // match:
        let mut failed = false;

        while mask[m] != '\0' {
            match mask[m] {
                '?' => {
                    if txt[t] == '\0' {
                        return false;
                    }
                }
                '*' => {
                    // remember where we started to be able to backtrack later
                    last_star_in_text = Some(t);
                    last_star_in_mask = m;

                    // ignore special chars immediately following this one
                    while mask[m] == '*' || mask[m] == '?' {
                        m += 1;
                    }

                    // if there is nothing more, match
                    if mask[m] == '\0' {
                        return true;
                    }

                    // are there any other metacharacters in the mask?
                    let len_mask = mask[m..]
                        .iter()
                        .position(|&c| c == '*' || c == '?' || c == '\0')
                        .unwrap_or(0);
                    let to_match = &mask[m..m + len_mask];

                    // wxStrstr( pszTxt, strToMatch )
                    let hay = &txt[t..txt.len() - 1];
                    let Some(found) = hay.windows(len_mask).position(|w| w == to_match) else {
                        return false;
                    };

                    // -1 to compensate "++" in the loop
                    t = t + found + len_mask - 1;
                    m += len_mask - 1;
                }
                c => {
                    if c != txt[t] {
                        failed = true;
                        break;
                    }
                }
            }

            m += 1;
            t += 1;
        }

        if failed {
            return false;
        }

        // match only if nothing left
        if txt[t] == '\0' {
            return true;
        }

        // if we failed to match, backtrack if we can
        match last_star_in_text.take() {
            Some(star) => {
                t = star + 1;
                m = last_star_in_mask;
            }
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NETCLASS::NETCLASS( aName, aInitWithDefaults )` (`common/netclass.cpp:51`).
    #[test]
    fn constructor_defaults() {
        let nc = Netclass::new("Default", true);
        assert!(nc.is_default);
        assert_eq!(nc.priority, -1);
        assert!(nc.constituents.is_empty());
        assert_eq!(nc.clearance, Some(200_000));
        assert_eq!(nc.track_width, Some(200_000));
        assert_eq!(nc.via_dia, Some(600_000));
        assert_eq!(nc.via_drill, Some(300_000));
        assert_eq!(nc.uvia_dia, Some(300_000));
        assert_eq!(nc.uvia_drill, Some(100_000));
        assert_eq!(nc.diff_pair_width, Some(200_000));
        assert_eq!(nc.diff_pair_gap, Some(250_000));
        assert_eq!(nc.diff_pair_via_gap, Some(250_000));
        assert_eq!(nc.wire_width, Some(1524));
        assert_eq!(nc.bus_width, Some(3048));
        assert_eq!(nc.line_style, Some(0));
        assert_eq!(nc.pcb_color, Color4D::UNSPECIFIED);
        assert_eq!(nc.schematic_color, Color4D::UNSPECIFIED);
        assert!(nc.tuning_profile.is_empty());

        let bare = Netclass::new("HV", false);
        assert!(!bare.is_default);
        assert_eq!(bare.clearance, None);
        assert_eq!(bare.clearance(), -1);
        assert_eq!(bare.line_style(), 0);
        assert_eq!(bare.priority, -1);
    }

    fn nc(name: &str) -> Arc<Netclass> {
        Arc::new(Netclass::new(name, true))
    }

    fn composite(constituents: &[&Arc<Netclass>]) -> Netclass {
        let mut c = Netclass::new("", true);
        c.set_constituent_netclasses(constituents.iter().map(|&n| Arc::clone(n)).collect());
        c
    }

    /// `qa/tests/common/test_netclass.cpp`, `NameEqualsMatchesGetName`.
    #[test]
    fn name_equals_matches_get_name() {
        // The allocation-free comparison must preserve GetName() equality for every valid name
        let check = |netclass: &Netclass, name: &str, expected: bool| {
            assert_eq!(netclass.name_equals(name), expected, "{name:?}");
            assert_eq!(
                netclass.name_equals(name),
                netclass.get_name() == name,
                "{name:?}"
            );
        };

        let first = nc("Power");
        let second = nc("Ground");
        let third = nc("Signal");
        let empty = nc("");
        let comma = nc("A,B");

        check(&first, "Power", true);
        check(&first, "Powe", false);
        check(&first, "PowerX", false);
        check(&empty, "", true);
        check(&empty, "Power", false);

        let mut alias = Netclass::new("Alias", true);
        alias.set_constituent_netclasses(vec![Arc::clone(&first)]);
        check(&alias, "Alias", true);
        check(&alias, "Power", false);

        let c = composite(&[&first, &second]);
        check(&c, "Power,Ground", true);
        check(&c, "Power;Ground", false);

        let c = composite(&[&comma, &third]);
        check(&c, "A,B,Signal", true);
        check(&c, "A,B;Signal", false);

        let c = composite(&[&empty, &first]);
        check(&c, ",Power", true);
        check(&c, "Power", false);
    }

    /// `NETCLASS::GetHumanReadableName` (`common/netclass.cpp:347`).
    #[test]
    fn human_readable_name() {
        let (a, b, c, d) = (nc("A"), nc("B"), nc("C"), nc("D"));
        assert_eq!(a.get_human_readable_name(), "A");
        assert_eq!(composite(&[&a, &b]).get_human_readable_name(), "A and B");
        assert_eq!(
            composite(&[&a, &b, &c]).get_human_readable_name(),
            "A, B and C"
        );
        assert_eq!(
            composite(&[&a, &b, &c, &d]).get_human_readable_name(),
            "A, B and 2 more"
        );
        assert_eq!(composite(&[&a, &b, &c, &d]).get_name(), "A,B,C,D");
        assert_eq!(composite(&[&a, &b]).constituent_netclasses().len(), 2);
        assert_eq!(a.constituent_netclasses()[0].name, "A");
    }

    /// `NETCLASS::ContainsNetclassWithName` (`common/netclass.cpp:337`) and the
    /// `wxString::Matches` rules it relies on.
    #[test]
    fn contains_netclass_with_name() {
        let (hv, power, default) = (nc("HV"), nc("Power"), nc("Default"));
        let c = composite(&[&hv, &power, &default]);
        assert!(c.contains_netclass_with_name("HV"));
        assert!(c.contains_netclass_with_name("Pow*"));
        assert!(c.contains_netclass_with_name("?efault"));
        assert!(!c.contains_netclass_with_name("hv"));
        assert!(!c.contains_netclass_with_name("HV,Power"));
        assert!(hv.contains_netclass_with_name("H*"));
        assert!(!hv.contains_netclass_with_name("Power"));

        assert!(wx_string_matches("abc", "a*c"));
        assert!(wx_string_matches("abc", "*"));
        assert!(wx_string_matches("", "*"));
        assert!(!wx_string_matches("", "?"));
        assert!(wx_string_matches("abcbc", "a*bc"));
        assert!(wx_string_matches("aXbYb", "a*b"));
        assert!(!wx_string_matches("abd", "a*c"));
        // wx skips '?' directly after '*': "a*?" matches "a" (WildCompareString would not)
        assert!(wx_string_matches("a", "a*?"));
        assert!(wx_string_matches("a.b.c", "*.c"));
        assert!(!wx_string_matches("ab", "a"));
    }

    /// `NETCLASS::ResetParameters`, `EqualsByPersistedFields`, `operator==`
    /// (`common/netclass.cpp:105-158`).
    #[test]
    fn reset_parameters_and_equality() {
        let mut a = Netclass::new("HV", true);
        a.tuning_profile = "fast".to_string();
        a.parents.clearance = Some(nc("X"));
        a.reset_parameters();
        assert_eq!(a.clearance, None);
        assert_eq!(a.line_style, None);
        assert_eq!(a.bus_width, None);
        assert!(a.parents.clearance.is_none());
        assert_eq!(a.tuning_profile, "fast");

        let mut b = Netclass::new("HV", false);
        b.tuning_profile = "fast".to_string();
        b.description = "ignored".to_string();
        assert!(a.equals_by_persisted_fields(&b));
        b.priority = 3;
        assert!(!a.equals_by_persisted_fields(&b));

        let (x, y) = (nc("X"), nc("Y"));
        assert!(composite(&[&x, &y]).same_constituents(&composite(&[&x, &y])));
        assert!(!composite(&[&x, &y]).same_constituents(&composite(&[&y, &x])));
        assert!(x.same_constituents(&x));
        assert!(!x.same_constituents(&Netclass::new("X", true)));
        assert!(composite(&[&x]).same_constituents(&x));
    }
}
