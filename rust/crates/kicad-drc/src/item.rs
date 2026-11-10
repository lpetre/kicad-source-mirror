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

//! Port of `pcbnew/drc/drc_item.h` / `drc_item.cpp` (work package DRC-11): the static
//! `DRC_ITEM` table (code, title, settings key) in `allItemTypes` order with its headings,
//! `DRC_ITEM::Create` (by code and by settings key), `GetItemsWithSeverities`,
//! `GetViolatingRuleDesc`, the `GetAuxItem2ID` / `GetAuxItem3ID` overrides,
//! `ConvertPadCheckToDrcCode`, `DRC_ITEMS_PROVIDER` and `BOARD_RC_ITEM_RESOLVER`.
//!
//! The data of an item (`RC_ITEM` / `DRC_ITEM` members) is `kicad_pcb::pcb_marker::RcItem`;
//! the error codes and settings keys are `kicad_pcb::drc_api`. Titles are the untranslated
//! `_HKI` strings (the report writers and the CLI never translate). Not ported:
//! `DRC_ITEM::GetViolationLayers` (GUI: layer focus of the DRC dialog) and
//! `DRC_ITEMS_PROVIDER::DeleteItem` (GUI).

use kicad_core::{Kiid, NILUUID};
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::*;
use kicad_pcb::ids::MarkerId;
use kicad_pcb::pad_utils::PadCheckCode;
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker, RcItem};

use crate::rc_item::{RcItemResolver, RcReportItem, ResolveItemById};

/// One statically defined `DRC_ITEM` of `drc_item.cpp` (`DRC_ITEM( aErrorCode, aTitle,
/// aSettingsKey )`). Headings have code 0 and an empty settings key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrcItemType {
    /// `m_errorCode`.
    pub code: i32,
    /// `m_errorTitle` (untranslated).
    pub title: &'static str,
    /// `m_settingsKey`.
    pub settings_key: &'static str,
}

impl DrcItemType {
    const fn new(code: i32, title: &'static str, settings_key: &'static str) -> Self {
        DrcItemType {
            code,
            title,
            settings_key,
        }
    }

    /// A fresh `RC_ITEM` copied from this static item (`std::make_shared<DRC_ITEM>( item )`):
    /// code, title and key; empty message, no ids, no violating rule.
    pub fn instantiate(&self) -> RcItem {
        RcItem {
            error_code: self.code,
            error_title: self.title.to_string(),
            settings_key: self.settings_key.to_string(),
            ..RcItem::default()
        }
    }
}

// ---- the static items (drc_item.cpp) -------------------------------------------------------

/// `DRC_ITEM::heading_electrical`.
pub const HEADING_ELECTRICAL: DrcItemType = DrcItemType::new(0, "Electrical", "");
/// `DRC_ITEM::heading_DFM`.
pub const HEADING_DFM: DrcItemType = DrcItemType::new(0, "Design for Manufacturing", "");
/// `DRC_ITEM::heading_schematic_parity`.
pub const HEADING_SCHEMATIC_PARITY: DrcItemType = DrcItemType::new(0, "Schematic Parity", "");
/// `DRC_ITEM::heading_signal_integrity`.
pub const HEADING_SIGNAL_INTEGRITY: DrcItemType = DrcItemType::new(0, "Signal Integrity", "");
/// `DRC_ITEM::heading_readability`.
pub const HEADING_READABILITY: DrcItemType = DrcItemType::new(0, "Readability", "");
/// `DRC_ITEM::heading_misc`.
pub const HEADING_MISC: DrcItemType = DrcItemType::new(0, "Miscellaneous", "");
/// `DRC_ITEM::heading_internal`.
pub const HEADING_INTERNAL: DrcItemType = DrcItemType::new(0, "", "");
/// `DRC_ITEM::heading_deprecated` (not in `allItemTypes`).
pub const HEADING_DEPRECATED: DrcItemType = DrcItemType::new(0, "", "");

macro_rules! drc_items {
    ($($name:ident = ($code:expr, $title:expr, $key:expr);)*) => {
        $(
            #[doc = concat!("`DRC_ITEM` `", $key, "`.")]
            pub const $name: DrcItemType = DrcItemType::new($code, $title, $key);
        )*
    };
}

drc_items! {
    UNCONNECTED_ITEMS = (DRCE_UNCONNECTED_ITEMS, "Missing connection between items", "unconnected_items");
    SHORTING_ITEMS = (DRCE_SHORTING_ITEMS, "Items shorting two nets", "shorting_items");
    ITEMS_NOT_ALLOWED = (DRCE_ALLOWED_ITEMS, "Items not allowed", "items_not_allowed");
    TEXT_ON_EDGE_CUTS = (DRCE_TEXT_ON_EDGECUTS, "Text or graphic on Edge.Cuts layer", "text_on_edge_cuts");
    CLEARANCE = (DRCE_CLEARANCE, "Clearance violation", "clearance");
    CREEPAGE = (DRCE_CREEPAGE, "Creepage violation", "creepage");
    TRACKS_CROSSING = (DRCE_TRACKS_CROSSING, "Tracks crossing", "tracks_crossing");
    EDGE_CLEARANCE = (DRCE_EDGE_CLEARANCE, "Board edge clearance violation", "copper_edge_clearance");
    COPPER_OUTSIDE_BOARD = (DRCE_COPPER_OUTSIDE_BOARD, "Copper outside board outline", "copper_outside_board");
    ISOLATED_COPPER = (DRCE_ISOLATED_COPPER, "Isolated copper fill", "isolated_copper");
    STARVED_THERMAL = (DRCE_STARVED_THERMAL, "Thermal relief connection to zone incomplete", "starved_thermal");
    VIA_DANGLING = (DRCE_DANGLING_VIA, "Via is not connected or connected on only one layer", "via_dangling");
    TRACK_DANGLING = (DRCE_DANGLING_TRACK, "Track has unconnected end", "track_dangling");
    HOLE_CLEARANCE = (DRCE_HOLE_CLEARANCE, "Hole clearance violation", "hole_clearance");
    HOLE_NEAR_HOLE = (DRCE_DRILLED_HOLES_TOO_CLOSE, "Drilled hole too close to other hole", "hole_to_hole");
    HOLES_CO_LOCATED = (DRCE_DRILLED_HOLES_COLOCATED, "Drilled holes co-located", "holes_co_located");
    CONNECTION_WIDTH = (DRCE_CONNECTION_WIDTH, "Copper connection too narrow", "connection_width");
    TRACK_WIDTH = (DRCE_TRACK_WIDTH, "Track width", "track_width");
    TRACK_ANGLE = (DRCE_TRACK_ANGLE, "Track angle", "track_angle");
    TRACK_SEGMENT_LENGTH = (DRCE_TRACK_SEGMENT_LENGTH, "Track segment length", "track_segment_length");
    ANNULAR_WIDTH = (DRCE_ANNULAR_WIDTH, "Annular width", "annular_width");
    DRILL_TOO_SMALL = (DRCE_DRILL_OUT_OF_RANGE, "Hole size out of range", "drill_out_of_range");
    VIA_DIAMETER = (DRCE_VIA_DIAMETER, "Via diameter", "via_diameter");
    PADSTACK = (DRCE_PADSTACK, "Padstack is questionable", "padstack");
    PADSTACK_INVALID = (DRCE_PADSTACK_INVALID, "Padstack is not valid", "padstack_invalid");
    MICROVIA_DRILL_TOO_SMALL = (DRCE_MICROVIA_DRILL_OUT_OF_RANGE, "Micro via hole size out of range", "microvia_drill_out_of_range");
    MALFORMED_MICROVIA_STACK_SPAN = (DRCE_MALFORMED_MICROVIA_STACK_SPAN, "Microvia stack has malformed span", "malformed_microvia_stack_span");
    MICROVIA_STACK_NOT_FILLED = (DRCE_MICROVIA_STACK_NOT_FILLED, "Microvia under microvia not filled", "microvia_stack_not_filled");
    MICROVIA_STACK_DEPTH = (DRCE_MICROVIA_STACK_DEPTH, "Microvia stack too deep", "microvia_stack_depth");
    HOLE_ASPECT_RATIO = (DRCE_HOLE_ASPECT_RATIO, "Hole too deep for its diameter", "hole_aspect_ratio");
    MICROVIA_CROSSES_CORE = (DRCE_MICROVIA_CROSSES_CORE, "Microvia crosses core material", "microvia_crosses_core");
    COURTYARDS_OVERLAP = (DRCE_OVERLAPPING_FOOTPRINTS, "Courtyards overlap", "courtyards_overlap");
    MISSING_COURTYARD = (DRCE_MISSING_COURTYARD, "Footprint has no courtyard defined", "missing_courtyard");
    MALFORMED_COURTYARD = (DRCE_MALFORMED_COURTYARD, "Footprint has malformed courtyard", "malformed_courtyard");
    PTH_INSIDE_COURTYARD = (DRCE_PTH_IN_COURTYARD, "PTH inside courtyard", "pth_inside_courtyard");
    NPTH_INSIDE_COURTYARD = (DRCE_NPTH_IN_COURTYARD, "NPTH inside courtyard", "npth_inside_courtyard");
    ITEM_ON_DISABLED_LAYER = (DRCE_DISABLED_LAYER_ITEM, "Item on a disabled copper layer", "item_on_disabled_layer");
    INVALID_OUTLINE = (DRCE_INVALID_OUTLINE, "Board has malformed outline", "invalid_outline");
    DUPLICATE_FOOTPRINTS = (DRCE_DUPLICATE_FOOTPRINT, "Duplicate footprints", "duplicate_footprints");
    MISSING_FOOTPRINT = (DRCE_MISSING_FOOTPRINT, "Missing footprint", "missing_footprint");
    EXTRA_FOOTPRINT = (DRCE_EXTRA_FOOTPRINT, "Extra footprint", "extra_footprint");
    NET_CONFLICT = (DRCE_NET_CONFLICT, "Pad net doesn't match schematic", "net_conflict");
    SCHEMATIC_PARITY = (DRCE_SCHEMATIC_PARITY, "Footprint attributes don't match symbol", "footprint_symbol_mismatch");
    FOOTPRINT_FILTERS = (DRCE_FOOTPRINT_FILTERS, "Footprint doesn't match symbol's footprint filters", "footprint_filters_mismatch");
    SCHEMATIC_FIELDS_PARITY = (DRCE_SCHEMATIC_FIELDS_PARITY, "Footprint field does not match symbol field", "footprint_symbol_field_mismatch");
    VIA_STITCH_OVERLAP = (DRCE_VIA_STITCH_OVERLAP, "Via-stitching zones overlap for same net", "via_stitch_overlap");
    LIB_FOOTPRINT_ISSUES = (DRCE_LIB_FOOTPRINT_ISSUES, "Footprint not found in libraries", "lib_footprint_issues");
    LIB_FOOTPRINT_MISMATCH = (DRCE_LIB_FOOTPRINT_MISMATCH, "Footprint doesn't match copy in library", "lib_footprint_mismatch");
    UNRESOLVED_VARIABLE = (DRCE_UNRESOLVED_VARIABLE, "Unresolved text variable", "unresolved_variable");
    ASSERTION_FAILURE = (DRCE_ASSERTION_FAILURE, "Assertion failure", "assertion_failure");
    GENERIC_WARNING = (DRCE_GENERIC_WARNING, "Warning", "generic_warning");
    GENERIC_ERROR = (DRCE_GENERIC_ERROR, "Error", "generic_error");
    COPPER_SLIVER = (DRCE_COPPER_SLIVER, "Copper sliver", "copper_sliver");
    SOLDER_MASK_BRIDGE = (DRCE_SOLDERMASK_BRIDGE, "Solder mask aperture bridges items with different nets", "solder_mask_bridge");
    SILK_MASK_CLEARANCE = (DRCE_SILK_MASK_CLEARANCE, "Silkscreen clipped by solder mask", "silk_over_copper");
    SILK_EDGE_CLEARANCE = (DRCE_SILK_EDGE_CLEARANCE, "Silkscreen clipped by board edge", "silk_edge_clearance");
    SILK_CLEARANCE = (DRCE_SILK_CLEARANCE, "Silkscreen clearance", "silk_overlap");
    TEXT_HEIGHT_OUT_OF_RANGE = (DRCE_TEXT_HEIGHT, "Text height out of range", "text_height");
    TEXT_THICKNESS_OUT_OF_RANGE = (DRCE_TEXT_THICKNESS, "Text thickness out of range", "text_thickness");
    LENGTH_OUT_OF_RANGE = (DRCE_LENGTH_OUT_OF_RANGE, "Track length out of range", "length_out_of_range");
    NET_CHAIN_STUB_TOO_LONG = (DRCE_NET_CHAIN_STUB_TOO_LONG, "Net chain stub length out of range", "net_chain_stub_length");
    NET_CHAIN_RETURN_PATH_BREAK = (DRCE_NET_CHAIN_RETURN_PATH_BREAK, "Net chain routed without continuous copper on the required reference layer", "net_chain_return_path");
    NET_CHAIN_TUNING_PROFILES = (DRCE_NET_CHAIN_TUNING_PROFILES, "Net chain nets do not have consistent tuning profile assignment", "net_chain_tuning_profiles");
    SKEW_OUT_OF_RANGE = (DRCE_SKEW_OUT_OF_RANGE, "Skew between tracks out of range", "skew_out_of_range");
    // Note: this used to only check against a max value, hence the settings key too_many_vias
    VIA_COUNT_OUT_OF_RANGE = (DRCE_VIA_COUNT_OUT_OF_RANGE, "Too many or too few vias on a connection", "too_many_vias");
    DIFF_PAIR_GAP_OUT_OF_RANGE = (DRCE_DP_GAP_OUT_OF_RANGE, "Differential pair gap out of range", "diff_pair_gap_out_of_range");
    DIFF_PAIR_UNCOUPLED_LENGTH_TOO_LONG = (DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG, "Differential uncoupled length too long", "diff_pair_uncoupled_length_too_long");
    FOOTPRINT = (DRCE_FOOTPRINT, "Footprint is not valid", "footprint");
    FOOTPRINT_TYPE_MISMATCH = (DRCE_FOOTPRINT_TYPE_MISMATCH, "Footprint component type doesn't match footprint pads", "footprint_type_mismatch");
    FOOTPRINT_TH_PAD_HAS_NO_HOLE = (DRCE_PAD_TH_WITH_NO_HOLE, "Through hole pad has no hole", "through_hole_pad_without_hole");
    FOOTPRINT_SCALED_WITH_PADS = (DRCE_FOOTPRINT_SCALED_WITH_PADS, "Footprint with pads is scaled (physical part size unchanged)", "footprint_scaled_with_pads");
    MIRRORED_TEXT_ON_FRONT_LAYER = (DRCE_MIRRORED_TEXT_ON_FRONT_LAYER, "Mirrored text on front layer", "mirrored_text_on_front_layer");
    UNMIRRORED_TEXT_ON_BACK_LAYER = (DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER, "Non-Mirrored text on back layer", "nonmirrored_text_on_back_layer");
    MISSING_TUNING_PROFILE = (DRCE_MISSING_TUNING_PROFILE, "Missing tuning profile", "missing_tuning_profile");
    TUNING_PROFILE_IMPLICIT_RULES = (DRCE_TUNING_PROFILE_IMPLICIT_RULES, "Tuning profile track geometries", "tuning_profile_track_geometries");
    TRACK_ON_POST_MACHINED_LAYER = (DRCE_TRACK_ON_POST_MACHINED_LAYER, "Track connected to post-machined or backdrilled layer", "track_on_post_machined_layer");
    TRACK_NOT_CENTERED_ON_VIA = (DRCE_TRACK_NOT_CENTERED_ON_VIA, "Track endpoint not centered on via", "track_not_centered_on_via");
}

/// `DRC_ITEM::allItemTypes`: every item type in display order, with the group headings
/// (code 0). The order is visible in the "ignored checks" sections of the reports and in the
/// `rule_severities` object of the project file.
pub const ALL_ITEM_TYPES: &[DrcItemType] = &[
    HEADING_ELECTRICAL,
    SHORTING_ITEMS,
    TRACKS_CROSSING,
    CLEARANCE,
    CREEPAGE,
    VIA_DANGLING,
    TRACK_DANGLING,
    STARVED_THERMAL,
    //
    HEADING_DFM,
    EDGE_CLEARANCE,
    COPPER_OUTSIDE_BOARD,
    HOLE_CLEARANCE,
    HOLE_NEAR_HOLE,
    HOLES_CO_LOCATED,
    TRACK_WIDTH,
    TRACK_ANGLE,
    TRACK_SEGMENT_LENGTH,
    ANNULAR_WIDTH,
    DRILL_TOO_SMALL,
    MICROVIA_DRILL_TOO_SMALL,
    MALFORMED_MICROVIA_STACK_SPAN,
    MICROVIA_STACK_NOT_FILLED,
    MICROVIA_STACK_DEPTH,
    HOLE_ASPECT_RATIO,
    MICROVIA_CROSSES_CORE,
    VIA_DIAMETER,
    COURTYARDS_OVERLAP,
    MISSING_COURTYARD,
    MALFORMED_COURTYARD,
    INVALID_OUTLINE,
    COPPER_SLIVER,
    SOLDER_MASK_BRIDGE,
    CONNECTION_WIDTH,
    TRACK_ON_POST_MACHINED_LAYER,
    TRACK_NOT_CENTERED_ON_VIA,
    VIA_STITCH_OVERLAP,
    TUNING_PROFILE_IMPLICIT_RULES,
    //
    HEADING_SCHEMATIC_PARITY,
    DUPLICATE_FOOTPRINTS,
    MISSING_FOOTPRINT,
    EXTRA_FOOTPRINT,
    SCHEMATIC_PARITY,
    SCHEMATIC_FIELDS_PARITY,
    FOOTPRINT_FILTERS,
    NET_CONFLICT,
    UNCONNECTED_ITEMS,
    //
    HEADING_SIGNAL_INTEGRITY,
    LENGTH_OUT_OF_RANGE,
    NET_CHAIN_STUB_TOO_LONG,
    NET_CHAIN_RETURN_PATH_BREAK,
    NET_CHAIN_TUNING_PROFILES,
    SKEW_OUT_OF_RANGE,
    VIA_COUNT_OUT_OF_RANGE,
    DIFF_PAIR_GAP_OUT_OF_RANGE,
    DIFF_PAIR_UNCOUPLED_LENGTH_TOO_LONG,
    //
    HEADING_READABILITY,
    SILK_CLEARANCE,
    SILK_MASK_CLEARANCE,
    SILK_EDGE_CLEARANCE,
    TEXT_HEIGHT_OUT_OF_RANGE,
    TEXT_THICKNESS_OUT_OF_RANGE,
    MIRRORED_TEXT_ON_FRONT_LAYER,
    UNMIRRORED_TEXT_ON_BACK_LAYER,
    //
    HEADING_MISC,
    ITEMS_NOT_ALLOWED,
    TEXT_ON_EDGE_CUTS,
    ISOLATED_COPPER,
    FOOTPRINT,
    PADSTACK,
    PTH_INSIDE_COURTYARD,
    NPTH_INSIDE_COURTYARD,
    ITEM_ON_DISABLED_LAYER,
    UNRESOLVED_VARIABLE,
    ASSERTION_FAILURE,
    FOOTPRINT_TYPE_MISMATCH,
    LIB_FOOTPRINT_ISSUES,
    LIB_FOOTPRINT_MISMATCH,
    FOOTPRINT_TH_PAD_HAS_NO_HOLE,
    FOOTPRINT_SCALED_WITH_PADS,
    MISSING_TUNING_PROFILE,
    // DRC_ITEM types with no user-editable severities
    // NOTE: this MUST be the last grouping in the list!
    HEADING_INTERNAL,
    PADSTACK_INVALID,
    GENERIC_ERROR,
    GENERIC_WARNING,
];

/// Index of `heading_internal` in [`ALL_ITEM_TYPES`] (the end of the user-editable items).
const HEADING_INTERNAL_INDEX: usize = ALL_ITEM_TYPES.len() - 4;

/// `DRC_ITEM::GetItemsWithSeverities()`: [`ALL_ITEM_TYPES`] up to (not including)
/// `heading_internal`. The headings are part of the list (code 0); callers skip them.
pub fn items_with_severities() -> &'static [DrcItemType] {
    &ALL_ITEM_TYPES[..HEADING_INTERNAL_INDEX]
}

/// The static item of an error code: the item `DRC_ITEM::Create( aErrorCode )` copies.
/// `None` for codes without an item (`DRCE_ZONES_INTERSECT`, `DRCE_TUNING_PROFILE_IMPLICIT_RULES`
/// and anything out of range).
pub fn item_type(error_code: i32) -> Option<&'static DrcItemType> {
    // DRC_ITEM::Create( int ) has no case for DRCE_TUNING_PROFILE_IMPLICIT_RULES although the
    // item exists in allItemTypes (it only carries a severity).
    if error_code == DRCE_TUNING_PROFILE_IMPLICIT_RULES || error_code <= 0 {
        return None;
    }

    ALL_ITEM_TYPES.iter().find(|item| item.code == error_code)
}

/// Port of `DRC_ITEM::Create( int aErrorCode )`.
pub fn create(error_code: i32) -> Option<RcItem> {
    item_type(error_code).map(DrcItemType::instantiate)
}

/// Port of `DRC_ITEM::Create( const wxString& aErrorKey )`: the first item of `allItemTypes`
/// with that settings key (headings included: an empty key yields a copy of
/// `heading_electrical`, as in C++).
pub fn create_from_key(error_key: &str) -> Option<RcItem> {
    // This can happen if a project has old-format exclusions.  Just drop these items.
    ALL_ITEM_TYPES
        .iter()
        .find(|item| item.settings_key == error_key)
        .map(DrcItemType::instantiate)
}

/// Port of `DRC_ITEM::GetViolatingRuleDesc( aTranslate = false )`.
pub fn violating_rule_desc(item: &RcItem) -> String {
    match &item.violating_rule {
        Some(rule) => format!("Rule: {}", rule.name),
        None => "Local override".to_string(),
    }
}

/// Port of `DRC_ITEM::GetAuxItem2ID()`.
pub fn aux_item2_id(item: &RcItem) -> Kiid {
    if item.error_code == DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG {
        // we have lots of segments, but it's enough to show the first P and the first N
        return NILUUID;
    }

    item.aux_item2_id()
}

/// Port of `DRC_ITEM::GetAuxItem3ID()`.
pub fn aux_item3_id(item: &RcItem) -> Kiid {
    if item.error_code == DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG {
        // we have lots of segments, but it's enough to show the first P and the first N
        return NILUUID;
    }

    item.aux_item3_id()
}

/// Port of `ConvertPadCheckToDrcCode( PAD_UTILS::PAD_CHECK_CODE )`.
pub fn convert_pad_check_to_drc_code(code: PadCheckCode) -> i32 {
    use PadCheckCode as C;

    match code {
        C::PadSizeNotPositive
        | C::PthHoleTooSmall
        | C::UnexpectedHole
        | C::RoundrectRadiusNegative
        | C::ChamferNegative
        | C::ChamferTooLarge
        | C::TrapezoidDeltaTooLarge
        | C::CustomShapeNotSinglePolygon
        | C::NonComponentNoCopper
        | C::NonComponentTechOnInner => DRCE_PADSTACK_INVALID,

        C::ThNoHole => DRCE_PAD_TH_WITH_NO_HOLE,

        C::PthNoOuterCopper
        | C::PthHoleLeavesNoCopper
        | C::PthHoleNotInsideCopper
        | C::HoleNotInsidePad
        | C::NegativeLocalClearance
        | C::NegativeMaskClearanceTooLarge
        | C::NegativePasteMarginTooLarge
        | C::RoundrectRadiusTooLarge
        | C::MaskWrongSide
        | C::PasteWrongSide
        | C::PadNoOuterCopper
        | C::ConnHasPaste
        | C::FiducialNotPlated
        | C::TestpointNotPlated
        | C::HeatsinkNotPlated
        | C::CastellatedNotPth
        | C::BgaNotSmd
        | C::MechanicalNotPth
        | C::PressfitNotPth
        | C::MultipleCopperLayers
        | C::NonComponentInnerNeedsCustomStackup => DRCE_PADSTACK,
    }
}

// ---- DRC_ITEMS_PROVIDER ----------------------------------------------------------------------

/// Port of `DRC_ITEMS_PROVIDER`: the board markers of one or two marker types, filtered by a
/// severity mask and sorted errors first. Holds marker handles; the board is passed to each
/// call (C++ keeps a `BOARD*`).
#[derive(Clone, Debug)]
pub struct DrcItemsProvider {
    marker_types: Vec<MarkerT>,
    severities: i32,
    filtered_markers: Vec<MarkerId>,
}

impl DrcItemsProvider {
    /// `DRC_ITEMS_PROVIDER( aBoard, aMarkerType, otherMarkerType = MARKER_UNSPEC )`.
    pub fn new(marker_type: MarkerT, other_marker_type: MarkerT) -> Self {
        let mut marker_types = vec![marker_type];

        if other_marker_type != MarkerT::Unspec {
            marker_types.push(other_marker_type);
        }

        DrcItemsProvider {
            marker_types,
            severities: 0,
            filtered_markers: Vec::new(),
        }
    }

    fn matches(&self, ctx: &BoardCtx<'_>, marker: &PcbMarker, severities: i32) -> bool {
        self.marker_types.contains(&marker.marker_type())
            && (marker.severity(ctx) as i32 & severities) > 0
    }

    /// Port of `DRC_ITEMS_PROVIDER::SetSeverities`: keep the markers of the provider's types
    /// whose severity is in `severities`, in board order, then stable-sort them by severity,
    /// highest (error) first.
    pub fn set_severities(&mut self, ctx: &BoardCtx<'_>, severities: i32) {
        self.severities = severities;

        self.filtered_markers.clear();

        for (id, marker) in ctx.board.markers() {
            if self.matches(ctx, marker, self.severities) {
                self.filtered_markers.push(id);
            }
        }

        // Sort markers so that errors appear before warnings
        let board = ctx.board;
        self.filtered_markers.sort_by(|a, b| {
            let sa = board[*a].severity(ctx) as i32;
            let sb = board[*b].severity(ctx) as i32;
            sb.cmp(&sa)
        });
    }

    /// `DRC_ITEMS_PROVIDER::GetSeverities()`.
    pub fn severities(&self) -> i32 {
        self.severities
    }

    /// `DRC_ITEMS_PROVIDER::GetCount()` (`aSeverity = -1`): the number of filtered markers.
    pub fn count(&self) -> usize {
        self.filtered_markers.len()
    }

    /// `DRC_ITEMS_PROVIDER::GetCount( aSeverity )` for `aSeverity >= 0`: the board markers of
    /// the provider's types whose severity is in `severity` (independent of the filter).
    pub fn count_of_severity(&self, ctx: &BoardCtx<'_>, severity: i32) -> usize {
        if severity < 0 {
            return self.count();
        }

        ctx.board
            .markers()
            .filter(|(_, marker)| self.matches(ctx, marker, severity))
            .count()
    }

    /// The marker behind `GetItem( aIndex )` (C++ returns its `RC_ITEM`; the report writers
    /// reach the marker through `GetParent()`).
    pub fn marker_id(&self, index: usize) -> MarkerId {
        self.filtered_markers[index]
    }

    /// The filtered markers in report order.
    pub fn marker_ids(&self) -> &[MarkerId] {
        &self.filtered_markers
    }

    /// `DRC_ITEMS_PROVIDER::GetItem( aIndex )`.
    pub fn item<'b>(&self, board: &'b Board, index: usize) -> &'b RcItem {
        board[self.filtered_markers[index]].rc_item()
    }
}

// ---- BOARD_RC_ITEM_RESOLVER --------------------------------------------------------------------

/// Port of `BOARD_RC_ITEM_RESOLVER`: resolves report items with `BOARD::ResolveItem( aKiid,
/// true )`. Wrap it in [`RcItemResolver`] (the `RC_ITEM_RESOLVER` base with its miss cache).
pub struct BoardRcItemResolver<'a> {
    ctx: &'a BoardCtx<'a>,
}

impl<'a> BoardRcItemResolver<'a> {
    /// `BOARD_RC_ITEM_RESOLVER( aBoard )`, already wrapped in the `RC_ITEM_RESOLVER` base.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ctx: &'a BoardCtx<'a>) -> RcItemResolver<BoardRcItemResolver<'a>> {
        RcItemResolver::new(BoardRcItemResolver { ctx })
    }
}

impl ResolveItemById for BoardRcItemResolver<'_> {
    /// `BOARD_RC_ITEM_RESOLVER::doResolveItemById`.
    fn do_resolve_item_by_id(&self, kiid: &Kiid) -> Option<Box<dyn RcReportItem + '_>> {
        self.ctx
            .resolve(kiid)
            .map(|item| Box::new(item) as Box<dyn RcReportItem + '_>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code of `PCB_DRC_CODE` except `DRCE_ZONES_INTERSECT` has exactly one item, with
    /// the settings key of `kicad_pcb::drc_api`.
    #[test]
    fn item_table_matches_codes() {
        for code in DRCE_FIRST..=DRCE_LAST {
            let entries: Vec<&DrcItemType> =
                ALL_ITEM_TYPES.iter().filter(|i| i.code == code).collect();

            if code == DRCE_ZONES_INTERSECT {
                assert!(entries.is_empty());
                assert!(create(code).is_none());
                continue;
            }

            assert_eq!(entries.len(), 1, "code {code}");
            assert_eq!(Some(entries[0].settings_key), drc_settings_key(code));

            if code == DRCE_TUNING_PROFILE_IMPLICIT_RULES {
                assert!(create(code).is_none());
            } else {
                let item = create(code).unwrap();
                assert_eq!(item.error_code, code);
                assert_eq!(item.error_text(), entries[0].title);
            }
        }

        assert!(create(0).is_none());
        assert!(create(DRCE_LAST + 1).is_none());
        assert_eq!(ALL_ITEM_TYPES.len(), 77 + 7);
        assert_eq!(ALL_ITEM_TYPES[HEADING_INTERNAL_INDEX], HEADING_INTERNAL);
    }

    /// The `allItemTypes` order of `drc.md` section 3.6 (settings keys, headings skipped).
    #[test]
    fn items_with_severities_order() {
        let keys: Vec<&str> = items_with_severities()
            .iter()
            .filter(|i| i.code > 0)
            .map(|i| i.settings_key)
            .collect();

        let expected = "shorting_items tracks_crossing clearance creepage via_dangling \
            track_dangling starved_thermal copper_edge_clearance copper_outside_board \
            hole_clearance hole_to_hole holes_co_located track_width track_angle \
            track_segment_length annular_width drill_out_of_range microvia_drill_out_of_range \
            malformed_microvia_stack_span microvia_stack_not_filled microvia_stack_depth \
            hole_aspect_ratio microvia_crosses_core via_diameter courtyards_overlap \
            missing_courtyard malformed_courtyard invalid_outline copper_sliver \
            solder_mask_bridge connection_width track_on_post_machined_layer \
            track_not_centered_on_via via_stitch_overlap tuning_profile_track_geometries \
            duplicate_footprints missing_footprint extra_footprint footprint_symbol_mismatch \
            footprint_symbol_field_mismatch footprint_filters_mismatch net_conflict \
            unconnected_items length_out_of_range net_chain_stub_length net_chain_return_path \
            net_chain_tuning_profiles skew_out_of_range too_many_vias diff_pair_gap_out_of_range \
            diff_pair_uncoupled_length_too_long silk_overlap silk_over_copper \
            silk_edge_clearance text_height text_thickness mirrored_text_on_front_layer \
            nonmirrored_text_on_back_layer items_not_allowed text_on_edge_cuts isolated_copper \
            footprint padstack pth_inside_courtyard npth_inside_courtyard \
            item_on_disabled_layer unresolved_variable assertion_failure \
            footprint_type_mismatch lib_footprint_issues lib_footprint_mismatch \
            through_hole_pad_without_hole footprint_scaled_with_pads missing_tuning_profile";
        let expected: Vec<&str> = expected.split_whitespace().collect();
        assert_eq!(keys, expected);
        assert_eq!(
            items_with_severities().iter().filter(|i| i.code == 0).count(),
            6
        );
    }

    #[test]
    fn create_by_key() {
        let item = create_from_key("too_many_vias").unwrap();
        assert_eq!(item.error_code, DRCE_VIA_COUNT_OUT_OF_RANGE);
        assert_eq!(item.error_message(), "Too many or too few vias on a connection");
        assert!(create_from_key("hole_near_hole").is_none());
        // The settings-only item exists by key.
        assert_eq!(
            create_from_key("tuning_profile_track_geometries")
                .unwrap()
                .error_code,
            DRCE_TUNING_PROFILE_IMPLICIT_RULES
        );
        // An empty key finds the first heading, as in C++.
        let heading = create_from_key("").unwrap();
        assert_eq!(heading.error_code, 0);
        assert_eq!(heading.error_text(), "Electrical");
    }

    #[test]
    fn rule_desc_and_aux_ids() {
        let mut item = create(DRCE_CLEARANCE).unwrap();
        assert_eq!(violating_rule_desc(&item), "Local override");
        item.violating_rule = Some(DrcRuleInfo {
            name: "netclass 'Default'".to_string(),
            ..DrcRuleInfo::default()
        });
        assert_eq!(violating_rule_desc(&item), "Rule: netclass 'Default'");

        let ids: Vec<Kiid> = (1..=4)
            .map(|i| Kiid::parse(&format!("{i:08x}")).unwrap())
            .collect();
        item.ids = ids.clone();
        assert!(aux_item2_id(&item) == ids[2]);
        assert!(aux_item3_id(&item) == ids[3]);
        item.error_code = DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG;
        assert!(aux_item2_id(&item) == NILUUID);
        assert!(aux_item3_id(&item) == NILUUID);
    }

    #[test]
    fn pad_check_codes() {
        assert_eq!(
            convert_pad_check_to_drc_code(PadCheckCode::ThNoHole),
            DRCE_PAD_TH_WITH_NO_HOLE
        );
        assert_eq!(
            convert_pad_check_to_drc_code(PadCheckCode::ChamferTooLarge),
            DRCE_PADSTACK_INVALID
        );
        assert_eq!(
            convert_pad_check_to_drc_code(PadCheckCode::MultipleCopperLayers),
            DRCE_PADSTACK
        );
    }
}
