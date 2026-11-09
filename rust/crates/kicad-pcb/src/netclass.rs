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
//! defaults and accessors (package M0d). The name / composite logic is package M3.

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

    /// `NETCLASS::GetName`: the root name, or the constituents' names joined with `,`.
    pub fn get_name(&self) -> String {
        crate::port_pending!("NETCLASS::GetName")
    }

    /// `NETCLASS::GetHumanReadableName`.
    pub fn get_human_readable_name(&self) -> String {
        crate::port_pending!("NETCLASS::GetHumanReadableName")
    }

    /// `NETCLASS::NameEquals`.
    pub fn name_equals(&self, other: &str) -> bool {
        crate::port_pending!("NETCLASS::NameEquals", other)
    }

    /// `NETCLASS::ContainsNetclassWithName` (wildcard match on the constituents' names).
    pub fn contains_netclass_with_name(&self, netclass: &str) -> bool {
        crate::port_pending!("NETCLASS::ContainsNetclassWithName", netclass)
    }

    /// `NETCLASS::EqualsByPersistedFields`.
    pub fn equals_by_persisted_fields(&self, other: &Netclass) -> bool {
        crate::port_pending!("NETCLASS::EqualsByPersistedFields", other)
    }

    /// `NETCLASS::ResetParents`: every parent is the class itself again.
    pub fn reset_parents(&mut self) {
        self.parents = NetclassParents::default();
    }

    /// `NETCLASS::ResetParameters`: clears every optional and both colours (not the tuning
    /// profile), then `ResetParents()`.
    pub fn reset_parameters(&mut self) {
        crate::port_pending!("NETCLASS::ResetParameters")
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
}
