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

//! The DRC vocabulary the board model needs below `kicad-drc` (`board-model.md` section 7):
//! port of `pcbnew/drc/drc_constraint_types.h` (`DRC_CONSTRAINT_T`, `DRC_DISALLOW_T`,
//! `DRC_IMPLICIT_SOURCE`), the value part of `DRC_CONSTRAINT` / `DRC_RULE` from
//! `pcbnew/drc/drc_rule.h`, and the `PCB_DRC_CODE` list of `pcbnew/drc/drc_item.h` with the
//! settings keys of `pcbnew/drc/drc_item.cpp`. `kicad-drc` re-exports all of it.

use kicad_core::{MinOptMax, Severity};

pub use crate::ctx::{ConnectivityQuery, RuleResolver};
pub use kicad_geom::geometry::approximation::ErrorLoc;

use crate::zone_settings::ZoneConnection;

/// `DRC_CONSTRAINT_T` (pcbnew/drc/drc_constraint_types.h), same order and values as C++.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum DrcConstraintT {
    #[default]
    NullConstraint = 0,
    ClearanceConstraint,
    CreepageConstraint,
    HoleClearanceConstraint,
    HoleToHoleConstraint,
    EdgeClearanceConstraint,
    HoleSizeConstraint,
    CourtyardClearanceConstraint,
    SilkClearanceConstraint,
    TextHeightConstraint,
    TextThicknessConstraint,
    TrackWidthConstraint,
    TrackSegmentLengthConstraint,
    AnnularWidthConstraint,
    ZoneConnectionConstraint,
    ThermalReliefGapConstraint,
    ThermalSpokeWidthConstraint,
    MinResolvedSpokesConstraint,
    SolderMaskExpansionConstraint,
    SolderPasteAbsMarginConstraint,
    SolderPasteRelMarginConstraint,
    DisallowConstraint,
    ViaDiameterConstraint,
    LengthConstraint,
    NetChainLengthConstraint,
    NetChainStubLengthConstraint,
    NetChainReturnPathConstraint,
    SkewConstraint,
    DiffPairGapConstraint,
    MaxUncoupledConstraint,
    DiffPairIntraSkewConstraint,
    ViaCountConstraint,
    PhysicalClearanceConstraint,
    PhysicalHoleClearanceConstraint,
    AssertionConstraint,
    ConnectionWidthConstraint,
    TrackAngleConstraint,
    ViaDanglingConstraint,
    BridgedMaskConstraint,
    SolderMaskSliverConstraint,
    MicroviaStackDepthConstraint,
    HoleAspectRatioConstraint,
}

/// `DRC_DISALLOW_T` bits (`DrcConstraint::disallow_flags`).
pub mod drc_disallow {
    pub const THROUGH_VIAS: i32 = 1 << 0;
    pub const MICRO_VIAS: i32 = 1 << 1;
    pub const BLIND_VIAS: i32 = 1 << 2;
    pub const BURIED_VIAS: i32 = 1 << 3;
    pub const TRACKS: i32 = 1 << 4;
    pub const PADS: i32 = 1 << 5;
    pub const ZONES: i32 = 1 << 6;
    pub const TEXTS: i32 = 1 << 7;
    pub const GRAPHICS: i32 = 1 << 8;
    pub const HOLES: i32 = 1 << 9;
    pub const FOOTPRINTS: i32 = 1 << 10;
    /// `DRC_DISALLOW_VIAS`.
    pub const VIAS: i32 = THROUGH_VIAS;
    /// `DRC_DISALLOW_BB_VIAS`.
    pub const BB_VIAS: i32 = BLIND_VIAS | BURIED_VIAS;
}

/// `DRC_IMPLICIT_SOURCE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum DrcImplicitSource {
    #[default]
    None,
    BoardSetupConstraint,
    BarcodeDefaults,
    Keepout,
    NetClass,
    TuningProfile,
}

/// `DRC_CONSTRAINT::OPTIONS` bit indices (`DrcConstraint::options` holds `1 << index`).
pub mod drc_constraint_options {
    pub const SKEW_WITHIN_DIFF_PAIRS: u32 = 0;
    pub const SPACE_DOMAIN: u32 = 1;
    pub const TIME_DOMAIN: u32 = 2;
    /// `NUM_OPTIONS`.
    pub const NUM_OPTIONS: u32 = 3;
}

/// What model and report code read from a `DRC_RULE` through a constraint.
#[derive(Clone, Debug, PartialEq)]
pub struct DrcRuleInfo {
    pub name: String,       // m_Name
    pub severity: Severity, // m_Severity
    /// `IsImplicit()` (`m_implicitSource != DRC_IMPLICIT_SOURCE::NONE`).
    pub implicit: bool,
}

impl Default for DrcRuleInfo {
    /// `DRC_RULE()`: severity `RPT_SEVERITY_UNDEFINED`.
    fn default() -> Self {
        DrcRuleInfo {
            name: String::new(),
            severity: Severity::Undefined,
            implicit: false,
        }
    }
}

/// Value part of `DRC_CONSTRAINT` (pcbnew/drc/drc_rule.h), returned by
/// `RuleResolver::eval_rules`. Rule back pointers become `DrcRuleInfo` copies (the engine owns
/// the rules); `m_Test` (the condition) stays in the engine. `Default` = the constructor.
#[derive(Clone, Debug)]
pub struct DrcConstraint {
    pub type_: DrcConstraintT,            // m_Type
    pub value: MinOptMax<i32>,            // m_Value
    pub disallow_flags: i32,              // m_DisallowFlags
    pub zone_connection: ZoneConnection,  // m_ZoneConnection
    pub implicit_min: bool,               // m_ImplicitMin
    pub reference_layer: String,          // m_ReferenceLayer
    pub reference_net: String,            // m_ReferenceNet
    pub options: u32,                     // m_options (DRC_CONSTRAINT::OPTIONS bits)
    pub name: String,                     // m_name (just-in-time constraints)
    pub parent_rule: Option<DrcRuleInfo>, // m_parentRule
    pub min_rule: Option<DrcRuleInfo>,    // m_minRule
    pub opt_rule: Option<DrcRuleInfo>,    // m_optRule
    pub max_rule: Option<DrcRuleInfo>,    // m_maxRule
}

impl Default for DrcConstraint {
    fn default() -> Self {
        DrcConstraint::new(DrcConstraintT::NullConstraint, "")
    }
}

impl DrcConstraint {
    /// Port of `DRC_CONSTRAINT::GetName`: `GetRuleName( m_parentRule )`.
    pub fn name(&self) -> String {
        self.rule_name(self.parent_rule.as_ref())
    }

    /// Port of `DRC_CONSTRAINT::GetRuleName`: a user-facing name for `rule`, or this
    /// constraint's own name when `rule` is `None` (for constraints from board setup or local
    /// overrides rather than rules).
    pub fn rule_name(&self, rule: Option<&DrcRuleInfo>) -> String {
        if let Some(rule) = rule {
            if rule.implicit {
                return rule.name.clone();
            } else {
                // wxString::Format( _( "rule '%s'" ), aRule->m_Name )
                return format!("rule '{}'", rule.name);
            }
        }

        self.name.clone()
    }

    /// Port of `DRC_CONSTRAINT::Merge`: the tighter minimum and maximum win (each remembers
    /// the rule it came from), an optimum is taken only when this constraint has none.
    pub fn merge(&mut self, other: &DrcConstraint) {
        if other.value.has_min()
            && (!self.value.has_min() || (other.value.min() > self.value.min()))
        {
            self.value.set_min(other.value.min());
            self.min_rule = if other.min_rule.is_some() {
                other.min_rule.clone()
            } else {
                other.parent_rule.clone()
            };
        }

        if other.value.has_max() && (!self.value.has_max() || other.value.max() < self.value.max())
        {
            self.value.set_max(other.value.max());
            self.max_rule = if other.max_rule.is_some() {
                other.max_rule.clone()
            } else {
                other.parent_rule.clone()
            };
        }

        if other.value.has_opt() && !self.value.has_opt() {
            self.value.set_opt(other.value.opt());
            self.parent_rule = other.parent_rule.clone();
        }
    }

    /// `DRC_CONSTRAINT( aType, aName )`.
    pub fn new(type_: DrcConstraintT, name: &str) -> Self {
        DrcConstraint {
            type_,
            value: MinOptMax::default(),
            disallow_flags: 0,
            zone_connection: ZoneConnection::Inherited,
            implicit_min: false,
            reference_layer: String::new(),
            reference_net: String::new(),
            options: 0,
            name: name.to_string(),
            parent_rule: None,
            min_rule: None,
            opt_rule: None,
            max_rule: None,
        }
    }

    /// `IsNull()`.
    pub fn is_null(&self) -> bool {
        self.type_ == DrcConstraintT::NullConstraint
    }

    /// `GetValue()`.
    pub fn value(&self) -> &MinOptMax<i32> {
        &self.value
    }

    /// `GetSeverity()`: the parent rule's severity, `RPT_SEVERITY_UNDEFINED` without one.
    pub fn severity(&self) -> Severity {
        self.parent_rule
            .as_ref()
            .map_or(Severity::Undefined, |r| r.severity)
    }

    /// `GetOption( option )`.
    pub fn option(&self, option: u32) -> bool {
        self.options & (1 << option) != 0
    }

    /// `SetOption( option )`.
    pub fn set_option(&mut self, option: u32) {
        self.options |= 1 << option;
    }

    /// `ClearOption( option )`.
    pub fn clear_option(&mut self, option: u32) {
        self.options &= !(1 << option);
    }
}

// ---- PCB_DRC_CODE (pcbnew/drc/drc_item.h) ----------------------------------------------------

/// `DRCE_UNCONNECTED_ITEMS`.
pub const DRCE_UNCONNECTED_ITEMS: i32 = 1;
/// `DRCE_SHORTING_ITEMS`.
pub const DRCE_SHORTING_ITEMS: i32 = 2;
/// `DRCE_ALLOWED_ITEMS`.
pub const DRCE_ALLOWED_ITEMS: i32 = 3;
/// `DRCE_TEXT_ON_EDGECUTS`.
pub const DRCE_TEXT_ON_EDGECUTS: i32 = 4;
/// `DRCE_CLEARANCE`.
pub const DRCE_CLEARANCE: i32 = 5;
/// `DRCE_CREEPAGE`.
pub const DRCE_CREEPAGE: i32 = 6;
/// `DRCE_TRACKS_CROSSING`.
pub const DRCE_TRACKS_CROSSING: i32 = 7;
/// `DRCE_EDGE_CLEARANCE`.
pub const DRCE_EDGE_CLEARANCE: i32 = 8;
/// `DRCE_ZONES_INTERSECT`.
pub const DRCE_ZONES_INTERSECT: i32 = 9;
/// `DRCE_ISOLATED_COPPER`.
pub const DRCE_ISOLATED_COPPER: i32 = 10;
/// `DRCE_STARVED_THERMAL`.
pub const DRCE_STARVED_THERMAL: i32 = 11;
/// `DRCE_DANGLING_VIA`.
pub const DRCE_DANGLING_VIA: i32 = 12;
/// `DRCE_DANGLING_TRACK`.
pub const DRCE_DANGLING_TRACK: i32 = 13;
/// `DRCE_DRILLED_HOLES_TOO_CLOSE`.
pub const DRCE_DRILLED_HOLES_TOO_CLOSE: i32 = 14;
/// `DRCE_DRILLED_HOLES_COLOCATED`.
pub const DRCE_DRILLED_HOLES_COLOCATED: i32 = 15;
/// `DRCE_HOLE_CLEARANCE`.
pub const DRCE_HOLE_CLEARANCE: i32 = 16;
/// `DRCE_TRACK_WIDTH`.
pub const DRCE_TRACK_WIDTH: i32 = 17;
/// `DRCE_TRACK_ANGLE`.
pub const DRCE_TRACK_ANGLE: i32 = 18;
/// `DRCE_TRACK_SEGMENT_LENGTH`.
pub const DRCE_TRACK_SEGMENT_LENGTH: i32 = 19;
/// `DRCE_ANNULAR_WIDTH`.
pub const DRCE_ANNULAR_WIDTH: i32 = 20;
/// `DRCE_CONNECTION_WIDTH`.
pub const DRCE_CONNECTION_WIDTH: i32 = 21;
/// `DRCE_DRILL_OUT_OF_RANGE`.
pub const DRCE_DRILL_OUT_OF_RANGE: i32 = 22;
/// `DRCE_VIA_DIAMETER`.
pub const DRCE_VIA_DIAMETER: i32 = 23;
/// `DRCE_PADSTACK`.
pub const DRCE_PADSTACK: i32 = 24;
/// `DRCE_PADSTACK_INVALID`.
pub const DRCE_PADSTACK_INVALID: i32 = 25;
/// `DRCE_MICROVIA_DRILL_OUT_OF_RANGE`.
pub const DRCE_MICROVIA_DRILL_OUT_OF_RANGE: i32 = 26;
/// `DRCE_MALFORMED_MICROVIA_STACK_SPAN`.
pub const DRCE_MALFORMED_MICROVIA_STACK_SPAN: i32 = 27;
/// `DRCE_MICROVIA_STACK_NOT_FILLED`.
pub const DRCE_MICROVIA_STACK_NOT_FILLED: i32 = 28;
/// `DRCE_MICROVIA_STACK_DEPTH`.
pub const DRCE_MICROVIA_STACK_DEPTH: i32 = 29;
/// `DRCE_HOLE_ASPECT_RATIO`.
pub const DRCE_HOLE_ASPECT_RATIO: i32 = 30;
/// `DRCE_MICROVIA_CROSSES_CORE`.
pub const DRCE_MICROVIA_CROSSES_CORE: i32 = 31;
/// `DRCE_OVERLAPPING_FOOTPRINTS`.
pub const DRCE_OVERLAPPING_FOOTPRINTS: i32 = 32;
/// `DRCE_MISSING_COURTYARD`.
pub const DRCE_MISSING_COURTYARD: i32 = 33;
/// `DRCE_MALFORMED_COURTYARD`.
pub const DRCE_MALFORMED_COURTYARD: i32 = 34;
/// `DRCE_PTH_IN_COURTYARD`.
pub const DRCE_PTH_IN_COURTYARD: i32 = 35;
/// `DRCE_NPTH_IN_COURTYARD`.
pub const DRCE_NPTH_IN_COURTYARD: i32 = 36;
/// `DRCE_DISABLED_LAYER_ITEM`.
pub const DRCE_DISABLED_LAYER_ITEM: i32 = 37;
/// `DRCE_INVALID_OUTLINE`.
pub const DRCE_INVALID_OUTLINE: i32 = 38;
/// `DRCE_MISSING_FOOTPRINT`.
pub const DRCE_MISSING_FOOTPRINT: i32 = 39;
/// `DRCE_DUPLICATE_FOOTPRINT`.
pub const DRCE_DUPLICATE_FOOTPRINT: i32 = 40;
/// `DRCE_EXTRA_FOOTPRINT`.
pub const DRCE_EXTRA_FOOTPRINT: i32 = 41;
/// `DRCE_NET_CONFLICT`.
pub const DRCE_NET_CONFLICT: i32 = 42;
/// `DRCE_SCHEMATIC_PARITY`.
pub const DRCE_SCHEMATIC_PARITY: i32 = 43;
/// `DRCE_FOOTPRINT_FILTERS`.
pub const DRCE_FOOTPRINT_FILTERS: i32 = 44;
/// `DRCE_FOOTPRINT_TYPE_MISMATCH`.
pub const DRCE_FOOTPRINT_TYPE_MISMATCH: i32 = 45;
/// `DRCE_LIB_FOOTPRINT_ISSUES`.
pub const DRCE_LIB_FOOTPRINT_ISSUES: i32 = 46;
/// `DRCE_LIB_FOOTPRINT_MISMATCH`.
pub const DRCE_LIB_FOOTPRINT_MISMATCH: i32 = 47;
/// `DRCE_PAD_TH_WITH_NO_HOLE`.
pub const DRCE_PAD_TH_WITH_NO_HOLE: i32 = 48;
/// `DRCE_FOOTPRINT`.
pub const DRCE_FOOTPRINT: i32 = 49;
/// `DRCE_FOOTPRINT_SCALED_WITH_PADS`.
pub const DRCE_FOOTPRINT_SCALED_WITH_PADS: i32 = 50;
/// `DRCE_UNRESOLVED_VARIABLE`.
pub const DRCE_UNRESOLVED_VARIABLE: i32 = 51;
/// `DRCE_ASSERTION_FAILURE`.
pub const DRCE_ASSERTION_FAILURE: i32 = 52;
/// `DRCE_GENERIC_WARNING`.
pub const DRCE_GENERIC_WARNING: i32 = 53;
/// `DRCE_GENERIC_ERROR`.
pub const DRCE_GENERIC_ERROR: i32 = 54;
/// `DRCE_COPPER_SLIVER`.
pub const DRCE_COPPER_SLIVER: i32 = 55;
/// `DRCE_SOLDERMASK_BRIDGE`.
pub const DRCE_SOLDERMASK_BRIDGE: i32 = 56;
/// `DRCE_SILK_MASK_CLEARANCE`.
pub const DRCE_SILK_MASK_CLEARANCE: i32 = 57;
/// `DRCE_SILK_EDGE_CLEARANCE`.
pub const DRCE_SILK_EDGE_CLEARANCE: i32 = 58;
/// `DRCE_SILK_CLEARANCE`.
pub const DRCE_SILK_CLEARANCE: i32 = 59;
/// `DRCE_TEXT_HEIGHT`.
pub const DRCE_TEXT_HEIGHT: i32 = 60;
/// `DRCE_TEXT_THICKNESS`.
pub const DRCE_TEXT_THICKNESS: i32 = 61;
/// `DRCE_LENGTH_OUT_OF_RANGE`.
pub const DRCE_LENGTH_OUT_OF_RANGE: i32 = 62;
/// `DRCE_NET_CHAIN_STUB_TOO_LONG`.
pub const DRCE_NET_CHAIN_STUB_TOO_LONG: i32 = 63;
/// `DRCE_NET_CHAIN_RETURN_PATH_BREAK`.
pub const DRCE_NET_CHAIN_RETURN_PATH_BREAK: i32 = 64;
/// `DRCE_NET_CHAIN_TUNING_PROFILES`.
pub const DRCE_NET_CHAIN_TUNING_PROFILES: i32 = 65;
/// `DRCE_SKEW_OUT_OF_RANGE`.
pub const DRCE_SKEW_OUT_OF_RANGE: i32 = 66;
/// `DRCE_VIA_COUNT_OUT_OF_RANGE`.
pub const DRCE_VIA_COUNT_OUT_OF_RANGE: i32 = 67;
/// `DRCE_DP_GAP_OUT_OF_RANGE`.
pub const DRCE_DP_GAP_OUT_OF_RANGE: i32 = 68;
/// `DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG`.
pub const DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG: i32 = 69;
/// `DRCE_MIRRORED_TEXT_ON_FRONT_LAYER`.
pub const DRCE_MIRRORED_TEXT_ON_FRONT_LAYER: i32 = 70;
/// `DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER`.
pub const DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER: i32 = 71;
/// `DRCE_MISSING_TUNING_PROFILE`.
pub const DRCE_MISSING_TUNING_PROFILE: i32 = 72;
/// `DRCE_TUNING_PROFILE_IMPLICIT_RULES`.
pub const DRCE_TUNING_PROFILE_IMPLICIT_RULES: i32 = 73;
/// `DRCE_TRACK_ON_POST_MACHINED_LAYER`.
pub const DRCE_TRACK_ON_POST_MACHINED_LAYER: i32 = 74;
/// `DRCE_TRACK_NOT_CENTERED_ON_VIA`.
pub const DRCE_TRACK_NOT_CENTERED_ON_VIA: i32 = 75;
/// `DRCE_SCHEMATIC_FIELDS_PARITY`.
pub const DRCE_SCHEMATIC_FIELDS_PARITY: i32 = 76;
/// `DRCE_VIA_STITCH_OVERLAP`.
pub const DRCE_VIA_STITCH_OVERLAP: i32 = 77;
/// `DRCE_COPPER_OUTSIDE_BOARD`.
pub const DRCE_COPPER_OUTSIDE_BOARD: i32 = 78;
/// `DRCE_FIRST`.
pub const DRCE_FIRST: i32 = DRCE_UNCONNECTED_ITEMS;
/// `DRCE_LAST`.
pub const DRCE_LAST: i32 = DRCE_COPPER_OUTSIDE_BOARD;

/// `(code, settings key)` of every `DRC_ITEM` defined in `drc_item.cpp`, in code order
/// (`DRCE_ZONES_INTERSECT` has no item and no key).
pub const DRC_ERROR_KEYS: &[(i32, &str)] = &[
    (DRCE_UNCONNECTED_ITEMS, "unconnected_items"),
    (DRCE_SHORTING_ITEMS, "shorting_items"),
    (DRCE_ALLOWED_ITEMS, "items_not_allowed"),
    (DRCE_TEXT_ON_EDGECUTS, "text_on_edge_cuts"),
    (DRCE_CLEARANCE, "clearance"),
    (DRCE_CREEPAGE, "creepage"),
    (DRCE_TRACKS_CROSSING, "tracks_crossing"),
    (DRCE_EDGE_CLEARANCE, "copper_edge_clearance"),
    (DRCE_ISOLATED_COPPER, "isolated_copper"),
    (DRCE_STARVED_THERMAL, "starved_thermal"),
    (DRCE_DANGLING_VIA, "via_dangling"),
    (DRCE_DANGLING_TRACK, "track_dangling"),
    (DRCE_DRILLED_HOLES_TOO_CLOSE, "hole_to_hole"),
    (DRCE_DRILLED_HOLES_COLOCATED, "holes_co_located"),
    (DRCE_HOLE_CLEARANCE, "hole_clearance"),
    (DRCE_TRACK_WIDTH, "track_width"),
    (DRCE_TRACK_ANGLE, "track_angle"),
    (DRCE_TRACK_SEGMENT_LENGTH, "track_segment_length"),
    (DRCE_ANNULAR_WIDTH, "annular_width"),
    (DRCE_CONNECTION_WIDTH, "connection_width"),
    (DRCE_DRILL_OUT_OF_RANGE, "drill_out_of_range"),
    (DRCE_VIA_DIAMETER, "via_diameter"),
    (DRCE_PADSTACK, "padstack"),
    (DRCE_PADSTACK_INVALID, "padstack_invalid"),
    (
        DRCE_MICROVIA_DRILL_OUT_OF_RANGE,
        "microvia_drill_out_of_range",
    ),
    (
        DRCE_MALFORMED_MICROVIA_STACK_SPAN,
        "malformed_microvia_stack_span",
    ),
    (DRCE_MICROVIA_STACK_NOT_FILLED, "microvia_stack_not_filled"),
    (DRCE_MICROVIA_STACK_DEPTH, "microvia_stack_depth"),
    (DRCE_HOLE_ASPECT_RATIO, "hole_aspect_ratio"),
    (DRCE_MICROVIA_CROSSES_CORE, "microvia_crosses_core"),
    (DRCE_OVERLAPPING_FOOTPRINTS, "courtyards_overlap"),
    (DRCE_MISSING_COURTYARD, "missing_courtyard"),
    (DRCE_MALFORMED_COURTYARD, "malformed_courtyard"),
    (DRCE_PTH_IN_COURTYARD, "pth_inside_courtyard"),
    (DRCE_NPTH_IN_COURTYARD, "npth_inside_courtyard"),
    (DRCE_DISABLED_LAYER_ITEM, "item_on_disabled_layer"),
    (DRCE_INVALID_OUTLINE, "invalid_outline"),
    (DRCE_MISSING_FOOTPRINT, "missing_footprint"),
    (DRCE_DUPLICATE_FOOTPRINT, "duplicate_footprints"),
    (DRCE_EXTRA_FOOTPRINT, "extra_footprint"),
    (DRCE_NET_CONFLICT, "net_conflict"),
    (DRCE_SCHEMATIC_PARITY, "footprint_symbol_mismatch"),
    (DRCE_FOOTPRINT_FILTERS, "footprint_filters_mismatch"),
    (DRCE_FOOTPRINT_TYPE_MISMATCH, "footprint_type_mismatch"),
    (DRCE_LIB_FOOTPRINT_ISSUES, "lib_footprint_issues"),
    (DRCE_LIB_FOOTPRINT_MISMATCH, "lib_footprint_mismatch"),
    (DRCE_PAD_TH_WITH_NO_HOLE, "through_hole_pad_without_hole"),
    (DRCE_FOOTPRINT, "footprint"),
    (
        DRCE_FOOTPRINT_SCALED_WITH_PADS,
        "footprint_scaled_with_pads",
    ),
    (DRCE_UNRESOLVED_VARIABLE, "unresolved_variable"),
    (DRCE_ASSERTION_FAILURE, "assertion_failure"),
    (DRCE_GENERIC_WARNING, "generic_warning"),
    (DRCE_GENERIC_ERROR, "generic_error"),
    (DRCE_COPPER_SLIVER, "copper_sliver"),
    (DRCE_SOLDERMASK_BRIDGE, "solder_mask_bridge"),
    (DRCE_SILK_MASK_CLEARANCE, "silk_over_copper"),
    (DRCE_SILK_EDGE_CLEARANCE, "silk_edge_clearance"),
    (DRCE_SILK_CLEARANCE, "silk_overlap"),
    (DRCE_TEXT_HEIGHT, "text_height"),
    (DRCE_TEXT_THICKNESS, "text_thickness"),
    (DRCE_LENGTH_OUT_OF_RANGE, "length_out_of_range"),
    (DRCE_NET_CHAIN_STUB_TOO_LONG, "net_chain_stub_length"),
    (DRCE_NET_CHAIN_RETURN_PATH_BREAK, "net_chain_return_path"),
    (DRCE_NET_CHAIN_TUNING_PROFILES, "net_chain_tuning_profiles"),
    (DRCE_SKEW_OUT_OF_RANGE, "skew_out_of_range"),
    (DRCE_VIA_COUNT_OUT_OF_RANGE, "too_many_vias"),
    (DRCE_DP_GAP_OUT_OF_RANGE, "diff_pair_gap_out_of_range"),
    (
        DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG,
        "diff_pair_uncoupled_length_too_long",
    ),
    (
        DRCE_MIRRORED_TEXT_ON_FRONT_LAYER,
        "mirrored_text_on_front_layer",
    ),
    (
        DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER,
        "nonmirrored_text_on_back_layer",
    ),
    (DRCE_MISSING_TUNING_PROFILE, "missing_tuning_profile"),
    (
        DRCE_TUNING_PROFILE_IMPLICIT_RULES,
        "tuning_profile_track_geometries",
    ),
    (
        DRCE_TRACK_ON_POST_MACHINED_LAYER,
        "track_on_post_machined_layer",
    ),
    (DRCE_TRACK_NOT_CENTERED_ON_VIA, "track_not_centered_on_via"),
    (
        DRCE_SCHEMATIC_FIELDS_PARITY,
        "footprint_symbol_field_mismatch",
    ),
    (DRCE_VIA_STITCH_OVERLAP, "via_stitch_overlap"),
    (DRCE_COPPER_OUTSIDE_BOARD, "copper_outside_board"),
];

/// The settings key (`.kicad_pro` `rule_severities`, `DRC_ITEM::GetSettingsKey`) of a code.
pub fn drc_settings_key(code: i32) -> Option<&'static str> {
    DRC_ERROR_KEYS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, k)| *k)
}

/// The code of a settings key (`.kicad_pro` reader).
pub fn drc_code_from_settings_key(key: &str) -> Option<i32> {
    DRC_ERROR_KEYS
        .iter()
        .find(|(_, k)| *k == key)
        .map(|(c, _)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraint_defaults_and_codes() {
        let c = DrcConstraint::default();
        assert!(c.is_null());
        assert_eq!(c.zone_connection, ZoneConnection::Inherited);
        assert_eq!(c.severity(), Severity::Undefined);
        assert_eq!(DrcConstraintT::HoleAspectRatioConstraint as i32, 41);

        assert_eq!(DRCE_LAST, 78);
        assert_eq!(DRCE_COPPER_OUTSIDE_BOARD, 78);
        assert_eq!(drc_settings_key(DRCE_CLEARANCE), Some("clearance"));
        assert_eq!(
            drc_code_from_settings_key("courtyards_overlap"),
            Some(DRCE_OVERLAPPING_FOOTPRINTS)
        );
        assert_eq!(drc_settings_key(DRCE_ZONES_INTERSECT), None);
        assert!(DRC_ERROR_KEYS.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
