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

//! Port of `include/board_design_settings.h`, `include/board_design_settings_defaults.h` and
//! the constructor / trivial members of `pcbnew/board_design_settings.cpp`
//! (`BOARD_DESIGN_SETTINGS`): data and constructor defaults, every DRC severity default
//! included (package M0d). The derived getters and validation are package M5; the JSON loader
//! belongs to the project-settings packages.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use kicad_core::eda_units::ui::string_from_value;
use kicad_core::json::Value as JsonValue;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::{
    EdaDataType, EdaUnits, Lset, MAX_USER_DEFINED_LAYERS, PCB_IU_SCALE, PCB_LAYER_ID_COUNT,
    PcbLayerId, Severity,
};
use kicad_geom::Vector2I;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_sexpr::Sexpr;

use crate::board_stackup::BoardStackup;
use crate::ctx::RuleResolver;
use crate::drc_api::*;
use crate::net_settings::NetSettings;
use crate::pcb_dimension::{DimPrecision, DimTextPosition, DimUnitsFormat, DimUnitsMode};
use crate::pcb_track::Viatype;
use crate::teardrop_parameters::{TargetTd, TeardropParametersList, teardrop_target_canonical_name};
use crate::zone_settings::{ZoneLayerProperties, ZoneSettings};

// ---- include/board_design_settings_defaults.h (values in mm unless stated) ----
pub const DEFAULT_SILK_LINE_WIDTH: f64 = 0.1;
pub const DEFAULT_COPPER_LINE_WIDTH: f64 = 0.2;
pub const DEFAULT_EDGE_WIDTH: f64 = 0.05;
pub const DEFAULT_COURTYARD_WIDTH: f64 = 0.05;
pub const DEFAULT_LINE_WIDTH: f64 = 0.10;
pub const DEFAULT_SILK_TEXT_SIZE: f64 = 1.0;
pub const DEFAULT_COPPER_TEXT_SIZE: f64 = 1.5;
pub const DEFAULT_TEXT_SIZE: f64 = 1.0;
pub const DEFAULT_SILK_TEXT_WIDTH: f64 = 0.1;
pub const DEFAULT_COPPER_TEXT_WIDTH: f64 = 0.30;
pub const DEFAULT_TEXT_WIDTH: f64 = 0.15;
/// Mils, for legacy purposes.
pub const DEFAULT_DIMENSION_ARROW_LENGTH: i32 = 50;
pub const DEFAULT_DIMENSION_EXTENSION_OFFSET: f64 = 0.5;
pub const DEFAULT_BOARD_THICKNESS_MM: f64 = 1.6;
pub const DEFAULT_SOLDERMASK_EXPANSION: f64 = 0.0;
pub const DEFAULT_SOLDERMASK_TO_COPPER_CLEARANCE: f64 = 0.0;
pub const DEFAULT_SOLDERMASK_MIN_WIDTH: f64 = 0.0;
pub const DEFAULT_SOLDERPASTE_CLEARANCE: f64 = 0.0;
pub const DEFAULT_SOLDERPASTE_RATIO: f64 = 0.0;
pub const DEFAULT_CUSTOMTRACKWIDTH: f64 = 0.2;
pub const DEFAULT_CUSTOMDPAIRWIDTH: f64 = 0.125;
pub const DEFAULT_CUSTOMDPAIRGAP: f64 = 0.18;
pub const DEFAULT_CUSTOMDPAIRVIAGAP: f64 = 0.18;
pub const DEFAULT_MEANDER_SPACING: f64 = 0.6;
pub const DEFAULT_DP_MEANDER_SPACING: f64 = 1.0;
pub const DEFAULT_MINCLEARANCE: f64 = 0.0;
pub const DEFAULT_MINCONNECTION: f64 = 0.0;
pub const DEFAULT_TRACKMINWIDTH: f64 = 0.2;
pub const DEFAULT_VIASMINSIZE: f64 = 0.5;
pub const DEFAULT_MINTHROUGHDRILL: f64 = 0.3;
pub const DEFAULT_MICROVIASMINSIZE: f64 = 0.2;
pub const DEFAULT_MICROVIASMINDRILL: f64 = 0.1;
pub const DEFAULT_HOLETOHOLEMIN: f64 = 0.25;
pub const DEFAULT_HOLECLEARANCE: f64 = 0.25;
pub const DEFAULT_COPPEREDGECLEARANCE: f64 = 0.5;
/// A flag to indicate the legacy method (based on edge cut line thicknesses) should be used.
pub const LEGACY_COPPEREDGECLEARANCE: f64 = -0.01;
pub const DEFAULT_SILKCLEARANCE: f64 = 0.0;
pub const DEFAULT_MINGROOVEWIDTH: f64 = 0.0;
/// Fewer resolved spokes indicates a starved thermal.
pub const DEFAULT_MINRESOLVEDSPOKES: i32 = 2;
/// For arc approximation.
pub const MINIMUM_ERROR_SIZE_MM: f64 = 0.001;
/// For arc approximation.
pub const MAXIMUM_ERROR_SIZE_MM: f64 = 0.1;
/// Min line width entered in a dialog.
pub const MINIMUM_LINE_WIDTH_MM: f64 = 0.005;
/// Max line width entered in a dialog.
pub const MAXIMUM_LINE_WIDTH_MM: f64 = 100.0;

/// `MAXIMUM_CLEARANCE` (`pcbIUScale.mmToIU( 500 )`): to prevent int-overflows.
pub fn maximum_clearance() -> i32 {
    PCB_IU_SCALE.mm_to_iu(500.0)
}

/// Default of `ADVANCED_CFG::m_DRCEpsilon` (mm): 0.5um is small enough not to materially
/// violate any constraints (`common/advanced_config.cpp`).
pub const ADVANCED_CFG_DRC_EPSILON_MM: f64 = 0.0005;
/// Default of `ADVANCED_CFG::m_HoleWallThickness` (mm): IPC-6012 says 15-18um; Cadence says
/// at least 0.020 for a Class 2 board and at least 0.025 for Class 3.
pub const ADVANCED_CFG_HOLE_WALL_THICKNESS_MM: f64 = 0.020;

/// `BOARD_DESIGN_SETTINGS::VALIDATION_ERROR`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub setting_name: String,  // setting_name
    pub error_message: String, // error_message
}

impl ValidationError {
    fn new(setting_name: &str, error_message: &str) -> Self {
        ValidationError {
            setting_name: setting_name.to_string(),
            error_message: error_message.to_string(),
        }
    }
}

/// Port of the file-static `nextPredefinedIndex` (`board_design_settings.cpp`): compute the
/// next index when cycling a predefined-size list whose index 0 is the synthetic "use
/// netclass" placeholder. Roll-over skips that placeholder when real sizes exist so the cycle
/// stays monotonic.
fn next_predefined_index(index: i32, forward: bool, list_size: i32) -> i32 {
    // Nothing to cycle when the list holds only the netclass placeholder (or is empty).
    if list_size <= 1 {
        return 0;
    }

    const LOWEST_REAL_INDEX: i32 = 1;
    let mut next = index + if forward { 1 } else { -1 };

    if next >= list_size {
        next = LOWEST_REAL_INDEX;
    } else if next < LOWEST_REAL_INDEX {
        next = list_size - 1;
    }

    next
}

/// `EDA_UNIT_UTILS::UI::StringFromValue( pcbIUScale, aUnits, int aValue, true )`.
fn string_from_iu(units: EdaUnits, value: i32) -> String {
    string_from_value(
        &PCB_IU_SCALE,
        units,
        value as f64,
        true,
        EdaDataType::Distance,
    )
}

/// `GEOMETRY_MIN_SIZE` (`pcbnew/pcb_track.h`): `(int) ( 0.001 * pcbIUScale.IU_PER_MM )`.
fn geometry_min_size() -> i32 {
    (0.001 * PCB_IU_SCALE.iu_per_mm) as i32
}

/// `PCB_VIA::ValidateViaParameters( aDiameter, aPrimaryDrill )` with every other argument at
/// its default (`std::nullopt`, copper layer count 0), the only form
/// `ValidateDesignRules` uses: the layer, secondary and tertiary drill checks are all no-ops
/// for those arguments. Returns the error message. (The full function belongs to the via
/// package, `pcb_track/via.rs`, which has no port of it yet.)
fn validate_via_diameter_and_drill(
    diameter: Option<i32>,
    primary_drill: Option<i32>,
) -> Option<&'static str> {
    if diameter.is_some_and(|d| d < geometry_min_size()) {
        return Some("Via diameter is too small.");
    }

    if primary_drill.is_some_and(|d| d < geometry_min_size()) {
        return Some("Via drill is too small.");
    }

    if diameter.is_some() && primary_drill.is_none() {
        return Some("No via hole size defined.");
    }

    if primary_drill.is_some() && diameter.is_none() {
        return Some("No via diameter defined.");
    }

    if let (Some(diameter), Some(drill)) = (diameter, primary_drill) {
        if diameter <= drill {
            return Some("Via hole size must be smaller than via diameter");
        }
    }

    None
}

/// `VIA_DIMENSION`. `<= 0` means "use the netclass value".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ViaDimension {
    pub diameter: i32, // m_Diameter
    pub drill: i32,    // m_Drill
}

/// `DIFF_PAIR_DIMENSION`. `<= 0` means "use the netclass value".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct DiffPairDimension {
    pub width: i32,   // m_Width
    pub gap: i32,     // m_Gap
    pub via_gap: i32, // m_ViaGap
}

/// `VIA_STACK_PRESET`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViaStackPreset {
    pub name: String,            // m_Name
    pub start_layer: PcbLayerId, // m_StartLayer
    pub end_layer: PcbLayerId,   // m_EndLayer
    pub staggered: bool,         // m_Staggered
    pub via_size: i32,           // m_ViaSize (<= 0 means use netclass)
    pub via_drill: i32,          // m_ViaDrill
    pub use_netclass: bool,      // m_UseNetclass
    pub filled: bool,            // m_Filled
    pub capped: bool,            // m_Capped
    pub pitch: i32,              // m_Pitch (staggered only)
}

impl Default for ViaStackPreset {
    /// The in-class initialisers of `VIA_STACK_PRESET`.
    fn default() -> Self {
        ViaStackPreset {
            name: String::new(),
            start_layer: PcbLayerId::FCu,
            end_layer: PcbLayerId::In1Cu,
            staggered: false,
            via_size: 0,
            via_drill: 0,
            use_netclass: false,
            filled: true,
            capped: false,
            pitch: 0,
        }
    }
}

/// `TEXT_ITEM_INFO`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItemInfo {
    pub text: String,      // m_Text
    pub visible: bool,     // m_Visible
    pub layer: PcbLayerId, // m_Layer
}

impl TextItemInfo {
    /// `TEXT_ITEM_INFO( aText, aVisible, aLayer )`.
    pub fn new(text: &str, visible: bool, layer: PcbLayerId) -> Self {
        TextItemInfo {
            text: text.to_string(),
            visible,
            layer,
        }
    }
}

pub const LAYER_CLASS_SILK: usize = 0;
pub const LAYER_CLASS_COPPER: usize = 1;
pub const LAYER_CLASS_EDGES: usize = 2;
pub const LAYER_CLASS_COURTYARD: usize = 3;
pub const LAYER_CLASS_FAB: usize = 4;
pub const LAYER_CLASS_OTHERS: usize = 5;
pub const LAYER_CLASS_COUNT: usize = 6;

/// `DRC_EXCLUSION` (C++: a wrapper around the kiapi `DrcExclusion` message, read from and
/// written to the project file JSON). Equality and order compare `sort_key` only, like
/// `DRC_EXCLUSION_COMPARE` and `operator==`.
#[derive(Clone, Debug)]
pub struct DrcExclusion {
    pub sort_key: String, // GetSortKey()
    pub comment: String,  // GetComment()
    /// The exclusion object as stored in the `.kicad_pro`.
    pub json: JsonValue,
}

impl PartialEq for DrcExclusion {
    fn eq(&self, other: &Self) -> bool {
        self.sort_key == other.sort_key
    }
}

impl Eq for DrcExclusion {}

impl PartialOrd for DrcExclusion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DrcExclusion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key.cmp(&other.sort_key)
    }
}

/// `BOARD_DESIGN_SETTINGS` (`include/board_design_settings.h`). Dropped: `m_DRCEngine`
/// (reached through `BoardCtx::rules`), the `NESTED_SETTINGS` / `FIELDS_TABLE_BOM_SETTINGS`
/// bases (JSON plumbing and BOM).
#[derive(Clone, Debug)]
pub struct BoardDesignSettings {
    pub track_width_list: Vec<i32>,              // m_TrackWidthList
    pub vias_dimensions_list: Vec<ViaDimension>, // m_ViasDimensionsList
    pub via_stack_presets: Vec<ViaStackPreset>,  // m_ViaStackPresets
    pub diff_pair_dimensions_list: Vec<DiffPairDimension>, // m_DiffPairDimensionsList
    pub teardrop_params_list: TeardropParametersList, // m_TeardropParamsList
    /// m_SingleTrackMeanderSettings, m_DiffPairMeanderSettings, m_SkewMeanderSettings
    /// (PNS::MEANDER_SETTINGS): kept as the JSON objects of `tuning_pattern_settings`
    /// (`single_track_defaults`, `diff_pair_defaults`, `diff_pair_skew_defaults`); only the
    /// length tuner reads them.
    pub meander_settings: [JsonValue; 3],
    pub current_via_type: Viatype,                  // m_CurrentViaType
    pub use_connected_track_width: bool,            // m_UseConnectedTrackWidth
    pub temp_override_track_width: bool,            // m_TempOverrideTrackWidth
    pub min_clearance: i32,                         // m_MinClearance
    pub min_groove_width: i32,                      // m_MinGrooveWidth
    pub min_conn: i32,                              // m_MinConn
    pub track_min_width: i32,                       // m_TrackMinWidth
    pub vias_min_annular_width: i32,                // m_ViasMinAnnularWidth
    pub vias_min_size: i32,                         // m_ViasMinSize
    pub min_through_drill: i32,                     // m_MinThroughDrill
    pub micro_vias_min_size: i32,                   // m_MicroViasMinSize
    pub micro_vias_min_drill: i32,                  // m_MicroViasMinDrill
    pub copper_edge_clearance: i32,                 // m_CopperEdgeClearance
    pub hole_clearance: i32,                        // m_HoleClearance
    pub hole_to_hole_min: i32,                      // m_HoleToHoleMin
    pub silk_clearance: i32,                        // m_SilkClearance
    pub min_resolved_spokes: i32,                   // m_MinResolvedSpokes
    pub min_silk_text_height: i32,                  // m_MinSilkTextHeight
    pub min_silk_text_thickness: i32,               // m_MinSilkTextThickness
    pub drc_severities: BTreeMap<i32, Severity>,    // m_DRCSeverities (DRCE code -> severity)
    pub drc_exclusions: BTreeSet<DrcExclusion>,     // m_DrcExclusions
    pub zone_keep_external_fillets: bool,           // m_ZoneKeepExternalFillets
    pub max_error: i32,                             // m_MaxError
    pub solder_mask_expansion: i32,                 // m_SolderMaskExpansion
    pub solder_mask_min_width: i32,                 // m_SolderMaskMinWidth
    pub solder_mask_to_copper_clearance: i32,       // m_SolderMaskToCopperClearance
    pub solder_paste_margin: i32,                   // m_SolderPasteMargin
    pub solder_paste_margin_ratio: f64,             // m_SolderPasteMarginRatio
    pub allow_soldermask_bridges_in_fps: bool,      // m_AllowSoldermaskBridgesInFPs
    pub tent_vias_front: bool,                      // m_TentViasFront
    pub tent_vias_back: bool,                       // m_TentViasBack
    pub cover_vias_front: bool,                     // m_CoverViasFront
    pub cover_vias_back: bool,                      // m_CoverViasBack
    pub plug_vias_front: bool,                      // m_PlugViasFront
    pub plug_vias_back: bool,                       // m_PlugViasBack
    pub cap_vias: bool,                             // m_CapVias
    pub fill_vias: bool,                            // m_FillVias
    pub net_settings: NetSettings,                  // m_NetSettings (shared_ptr)
    pub default_fp_text_items: Vec<TextItemInfo>,   // m_DefaultFPTextItems
    pub user_layer_names: BTreeMap<String, String>, // m_UserLayerNames
    pub zone_layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_ZoneLayerProperties
    pub line_thickness: [i32; LAYER_CLASS_COUNT],   // m_LineThickness
    pub text_size: [Vector2I; LAYER_CLASS_COUNT],   // m_TextSize
    pub text_thickness: [i32; LAYER_CLASS_COUNT],   // m_TextThickness
    pub text_italic: [bool; LAYER_CLASS_COUNT],     // m_TextItalic
    pub text_upright: [bool; LAYER_CLASS_COUNT],    // m_TextUpright
    pub dimension_units_mode: DimUnitsMode,         // m_DimensionUnitsMode
    pub dimension_precision: DimPrecision,          // m_DimensionPrecision
    pub dimension_units_format: DimUnitsFormat,     // m_DimensionUnitsFormat
    pub dimension_suppress_zeroes: bool,            // m_DimensionSuppressZeroes
    pub dimension_text_position: DimTextPosition,   // m_DimensionTextPosition
    pub dimension_keep_text_aligned: bool,          // m_DimensionKeepTextAligned
    pub dimension_arrow_length: i32,                // m_DimensionArrowLength
    pub dimension_extension_offset: i32,            // m_DimensionExtensionOffset
    pub style_fp_fields: bool,                      // m_StyleFPFields
    pub style_fp_text: bool,                        // m_StyleFPText
    pub style_fp_shapes: bool,                      // m_StyleFPShapes
    pub style_fp_dimensions: bool,                  // m_StyleFPDimensions
    pub style_fp_barcodes: bool,                    // m_StyleFPBarcodes
    pub has_stackup: bool,                          // m_HasStackup
    pub use_height_for_length_calcs: bool,          // m_UseHeightForLengthCalcs
    pub aux_origin: Vector2I,                       // m_auxOrigin
    pub grid_origin: Vector2I,                      // m_gridOrigin
    pub track_width_index: i32,                     // m_trackWidthIndex
    pub via_size_index: i32,                        // m_viaSizeIndex
    pub diff_pair_index: i32,                       // m_diffPairIndex
    pub via_stack_index: i32,                       // m_viaStackIndex
    pub use_custom_track_via: bool,                 // m_useCustomTrackVia
    pub custom_track_width: i32,                    // m_customTrackWidth
    pub custom_via_size: ViaDimension,              // m_customViaSize
    pub use_custom_diff_pair: bool,                 // m_useCustomDiffPair
    pub custom_diff_pair: DiffPairDimension,        // m_customDiffPair
    pub copper_layer_count: i32,                    // m_copperLayerCount
    pub user_defined_layer_count: i32,              // m_userDefinedLayerCount
    pub enabled_layers: Lset,                       // m_enabledLayers
    pub board_thickness: i32,                       // m_boardThickness
    pub current_net_class_name: String,             // m_currentNetClassName
    pub stackup: BoardStackup,                      // m_stackup
    /// m_drillSymbolProfile: the `(drill_symbol_profile ...)` node of `(setup ...)`, verbatim.
    pub drill_symbol_profile: Option<Sexpr>,
    pub default_zone_settings: ZoneSettings, // m_defaultZoneSettings
}

/// `PNS::MEANDER_SETTINGS()` with `m_spacing = spacing_mm`, as the JSON object the
/// `tuning_pattern_settings` parameter writes (`make_settings`, `board_design_settings.cpp`):
/// min amplitude 0.2 mm, max amplitude 1 mm, corner style ROUND (1), radius 80 %, not single
/// sided (`pcbnew/router/pns_meander.cpp`).
fn default_meander_json(spacing_mm: f64) -> JsonValue {
    let mut entry = kicad_core::json::Map::new();
    let iu_to_mm = |iu: i32| JsonValue::from(PCB_IU_SCALE.iu_to_mm(iu));
    entry.insert("min_amplitude".into(), iu_to_mm(200_000));
    entry.insert("max_amplitude".into(), iu_to_mm(1_000_000));
    entry.insert(
        "spacing".into(),
        iu_to_mm(PCB_IU_SCALE.mm_to_iu(spacing_mm)),
    );
    entry.insert("corner_style".into(), JsonValue::from(1));
    entry.insert("corner_radius_percentage".into(), JsonValue::from(80));
    entry.insert("single_sided".into(), JsonValue::from(false));
    JsonValue::Object(entry)
}

/// The `m_DRCSeverities` defaults of the constructor: every code `DRCE_FIRST ..= DRCE_LAST`
/// is an error, then the listed overrides in C++ statement order.
fn default_drc_severities() -> BTreeMap<i32, Severity> {
    let mut map: BTreeMap<i32, Severity> = (DRCE_FIRST..=DRCE_LAST)
        .map(|code| (code, Severity::Error))
        .collect();

    let overrides: &[(i32, Severity)] = &[
        (DRCE_DRILLED_HOLES_COLOCATED, Severity::Warning),
        (DRCE_DRILLED_HOLES_TOO_CLOSE, Severity::Warning),
        (DRCE_MISSING_COURTYARD, Severity::Ignore),
        (DRCE_PTH_IN_COURTYARD, Severity::Error),
        (DRCE_NPTH_IN_COURTYARD, Severity::Error),
        (DRCE_DANGLING_TRACK, Severity::Warning),
        (DRCE_DANGLING_VIA, Severity::Warning),
        (DRCE_COPPER_OUTSIDE_BOARD, Severity::Warning),
        (DRCE_MICROVIA_CROSSES_CORE, Severity::Warning),
        (DRCE_MICROVIA_STACK_NOT_FILLED, Severity::Warning),
        (DRCE_COPPER_SLIVER, Severity::Warning),
        (DRCE_ISOLATED_COPPER, Severity::Warning),
        (DRCE_PADSTACK, Severity::Warning),
        (DRCE_FOOTPRINT_SCALED_WITH_PADS, Severity::Warning),
        (DRCE_MISSING_FOOTPRINT, Severity::Warning),
        (DRCE_DUPLICATE_FOOTPRINT, Severity::Warning),
        (DRCE_EXTRA_FOOTPRINT, Severity::Warning),
        (DRCE_NET_CONFLICT, Severity::Warning),
        (DRCE_SCHEMATIC_PARITY, Severity::Warning),
        (DRCE_FOOTPRINT_FILTERS, Severity::Ignore),
        (DRCE_SCHEMATIC_FIELDS_PARITY, Severity::Warning),
        (DRCE_SILK_CLEARANCE, Severity::Warning),
        (DRCE_SILK_MASK_CLEARANCE, Severity::Warning),
        (DRCE_SILK_EDGE_CLEARANCE, Severity::Warning),
        (DRCE_TEXT_HEIGHT, Severity::Warning),
        (DRCE_TEXT_THICKNESS, Severity::Warning),
        (DRCE_FOOTPRINT_TYPE_MISMATCH, Severity::Ignore),
        (DRCE_LIB_FOOTPRINT_ISSUES, Severity::Warning),
        (DRCE_LIB_FOOTPRINT_MISMATCH, Severity::Warning),
        (DRCE_CONNECTION_WIDTH, Severity::Warning),
        (DRCE_MIRRORED_TEXT_ON_FRONT_LAYER, Severity::Warning),
        (DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER, Severity::Warning),
        (DRCE_MISSING_TUNING_PROFILE, Severity::Warning),
        (DRCE_TUNING_PROFILE_IMPLICIT_RULES, Severity::Ignore),
        (DRCE_TRACK_NOT_CENTERED_ON_VIA, Severity::Ignore),
    ];

    for &(code, severity) in overrides {
        map.insert(code, severity);
    }

    map
}

impl Default for BoardDesignSettings {
    /// `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS( nullptr, .. )`
    /// (`pcbnew/board_design_settings.cpp:47-266`). The `PARAM` registrations do not write
    /// the members; members the constructor body does not set keep their in-class value or
    /// their type's default constructor.
    fn default() -> Self {
        let mm = |v: f64| PCB_IU_SCALE.mm_to_iu(v);
        let square = |v: f64| Vector2I::new(mm(v), mm(v));

        let mut bds = BoardDesignSettings {
            track_width_list: Vec::new(),
            vias_dimensions_list: Vec::new(),
            via_stack_presets: Vec::new(),
            diff_pair_dimensions_list: Vec::new(),
            teardrop_params_list: TeardropParametersList::default(),
            // Default spacing for meanders.
            meander_settings: [
                default_meander_json(DEFAULT_MEANDER_SPACING),
                default_meander_json(DEFAULT_DP_MEANDER_SPACING),
                default_meander_json(DEFAULT_MEANDER_SPACING),
            ],
            current_via_type: Viatype::Through,
            // if true, when creating a new track starting on an existing track, use this
            // track width
            use_connected_track_width: false,
            temp_override_track_width: false,
            min_clearance: mm(DEFAULT_MINCLEARANCE),
            min_groove_width: mm(DEFAULT_MINGROOVEWIDTH),
            min_conn: mm(DEFAULT_MINCONNECTION),
            track_min_width: mm(DEFAULT_TRACKMINWIDTH),
            vias_min_annular_width: mm(DEFAULT_VIASMINSIZE - DEFAULT_MINTHROUGHDRILL) / 2,
            vias_min_size: mm(DEFAULT_VIASMINSIZE),
            min_through_drill: mm(DEFAULT_MINTHROUGHDRILL),
            micro_vias_min_size: mm(DEFAULT_MICROVIASMINSIZE),
            micro_vias_min_drill: mm(DEFAULT_MICROVIASMINDRILL),
            copper_edge_clearance: mm(DEFAULT_COPPEREDGECLEARANCE),
            hole_clearance: mm(DEFAULT_HOLECLEARANCE),
            hole_to_hole_min: mm(DEFAULT_HOLETOHOLEMIN),
            silk_clearance: mm(DEFAULT_SILKCLEARANCE),
            min_resolved_spokes: DEFAULT_MINRESOLVEDSPOKES,
            min_silk_text_height: mm(DEFAULT_SILK_TEXT_SIZE * 0.8),
            min_silk_text_thickness: mm(DEFAULT_SILK_TEXT_WIDTH * 0.8),
            drc_severities: default_drc_severities(),
            drc_exclusions: BTreeSet::new(),
            zone_keep_external_fillets: false,
            max_error: ARC_HIGH_DEF,
            // Global mask margins:
            solder_mask_expansion: mm(DEFAULT_SOLDERMASK_EXPANSION),
            solder_mask_min_width: mm(DEFAULT_SOLDERMASK_MIN_WIDTH),
            solder_mask_to_copper_clearance: mm(DEFAULT_SOLDERMASK_TO_COPPER_CLEARANCE),
            // Solder paste margin absolute value
            solder_paste_margin: mm(DEFAULT_SOLDERPASTE_CLEARANCE),
            // Solder paste margin as a ratio of pad size. The final margin is the sum of these
            // 2 values. Usually < 0 because the mask is smaller than pad
            solder_paste_margin_ratio: DEFAULT_SOLDERPASTE_RATIO,
            allow_soldermask_bridges_in_fps: false,
            tent_vias_front: true,
            tent_vias_back: true,
            cover_vias_front: false,
            cover_vias_back: false,
            plug_vias_front: false,
            plug_vias_back: false,
            cap_vias: false,
            fill_vias: false,
            // Create a default NET_SETTINGS so that things don't break horribly if there's no
            // project loaded.
            net_settings: NetSettings::default(),
            default_fp_text_items: vec![
                // First is always the reference designator
                TextItemInfo::new("REF**", true, PcbLayerId::FSilkS),
                // Second is always the value
                TextItemInfo::new("", true, PcbLayerId::FFab),
                // Any following ones are freebies
                TextItemInfo::new("${REFERENCE}", true, PcbLayerId::FFab),
            ],
            user_layer_names: BTreeMap::new(),
            zone_layer_properties: BTreeMap::new(),
            line_thickness: [
                mm(DEFAULT_SILK_LINE_WIDTH),
                mm(DEFAULT_COPPER_LINE_WIDTH),
                mm(DEFAULT_EDGE_WIDTH),
                mm(DEFAULT_COURTYARD_WIDTH),
                mm(DEFAULT_LINE_WIDTH),
                mm(DEFAULT_LINE_WIDTH),
            ],
            text_size: [
                square(DEFAULT_SILK_TEXT_SIZE),
                square(DEFAULT_COPPER_TEXT_SIZE),
                square(DEFAULT_TEXT_SIZE),
                square(DEFAULT_TEXT_SIZE),
                square(DEFAULT_TEXT_SIZE),
                square(DEFAULT_TEXT_SIZE),
            ],
            text_thickness: [
                mm(DEFAULT_SILK_TEXT_WIDTH),
                mm(DEFAULT_COPPER_TEXT_WIDTH),
                mm(DEFAULT_TEXT_WIDTH),
                mm(DEFAULT_TEXT_WIDTH),
                mm(DEFAULT_TEXT_WIDTH),
                mm(DEFAULT_TEXT_WIDTH),
            ],
            text_italic: [false; LAYER_CLASS_COUNT],
            text_upright: [false; LAYER_CLASS_COUNT],
            dimension_units_mode: DimUnitsMode::Automatic,
            dimension_precision: DimPrecision::X_XXXX,
            dimension_units_format: DimUnitsFormat::NoSuffix,
            dimension_suppress_zeroes: true,
            dimension_text_position: DimTextPosition::Outside,
            dimension_keep_text_aligned: true,
            dimension_arrow_length: PCB_IU_SCALE.mils_to_iu(DEFAULT_DIMENSION_ARROW_LENGTH),
            dimension_extension_offset: mm(DEFAULT_DIMENSION_EXTENSION_OFFSET),
            style_fp_fields: false,
            style_fp_text: false,
            style_fp_shapes: false,
            style_fp_dimensions: false,
            style_fp_barcodes: false,
            has_stackup: false, // no stackup defined by default
            use_height_for_length_calcs: true,
            aux_origin: Vector2I::new(0, 0),
            grid_origin: Vector2I::new(0, 0),
            track_width_index: 0,
            via_size_index: 0,
            diff_pair_index: 0,
            via_stack_index: 0,
            use_custom_track_via: false,
            custom_track_width: mm(DEFAULT_CUSTOMTRACKWIDTH),
            custom_via_size: ViaDimension {
                diameter: mm(DEFAULT_VIASMINSIZE),
                drill: mm(DEFAULT_MINTHROUGHDRILL),
            },
            use_custom_diff_pair: false,
            custom_diff_pair: DiffPairDimension {
                width: mm(DEFAULT_CUSTOMDPAIRWIDTH),
                gap: mm(DEFAULT_CUSTOMDPAIRGAP),
                via_gap: mm(DEFAULT_CUSTOMDPAIRVIAGAP),
            },
            copper_layer_count: 0,
            user_defined_layer_count: 0,
            // All layers enabled at first; SetCopperLayerCount() will adjust this.
            enabled_layers: Lset::all_layers_mask().clone(),
            // Layer thickness for 3D viewer
            board_thickness: mm(DEFAULT_BOARD_THICKNESS_MM),
            current_net_class_name: String::new(),
            stackup: BoardStackup::default(),
            drill_symbol_profile: None,
            default_zone_settings: ZoneSettings::default(),
        };

        // Default design is a double layer board with 4 user defined layers
        bds.set_copper_layer_count(2);
        bds.set_user_defined_layer_count(4);
        bds
    }
}

impl BoardDesignSettings {
    /// `m_MaxError` (what `BOARD_ITEM::GetMaxError()` returns for an item on a board).
    pub fn max_error(&self) -> i32 {
        self.max_error
    }

    /// `BOARD_DESIGN_SETTINGS::GetCopperLayerCount`.
    pub fn copper_layer_count(&self) -> i32 {
        self.copper_layer_count
    }

    /// `BOARD_DESIGN_SETTINGS::SetCopperLayerCount`: updates only the copper part of the
    /// enabled-layer mask.
    pub fn set_copper_layer_count(&mut self, new_layer_count: i32) {
        self.copper_layer_count = new_layer_count;

        // Update only enabled copper layers mask
        self.enabled_layers.clear_copper_layers();

        if new_layer_count > 0 {
            self.enabled_layers |= &Lset::all_cu_mask_n(new_layer_count);
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetUserDefinedLayerCount`.
    pub fn user_defined_layer_count(&self) -> i32 {
        self.user_defined_layer_count
    }

    /// `BOARD_DESIGN_SETTINGS::SetUserDefinedLayerCount`.
    pub fn set_user_defined_layer_count(&mut self, new_layer_count: i32) {
        self.user_defined_layer_count = new_layer_count;

        self.enabled_layers.clear_user_defined_layers();

        if new_layer_count > 0 {
            self.enabled_layers |= &Lset::user_defined_layers_mask(new_layer_count);
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetEnabledLayers`.
    pub fn enabled_layers(&self) -> &Lset {
        &self.enabled_layers
    }

    /// `BOARD_DESIGN_SETTINGS::SetEnabledLayers`: forces the mandatory layers and recounts the
    /// copper and user-defined layers from the argument mask.
    pub fn set_enabled_layers(&mut self, mask: &Lset) {
        self.enabled_layers = mask.clone();

        // Ensures mandatory back and front layers are always enabled regardless of board file
        // configuration.
        self.enabled_layers
            .set(PcbLayerId::BCu)
            .set(PcbLayerId::FCu)
            .set(PcbLayerId::BCrtYd)
            .set(PcbLayerId::FCrtYd)
            .set(PcbLayerId::EdgeCuts)
            .set(PcbLayerId::Margin);

        // update layer counts to ensure their consistency with m_EnabledLayers
        let mut copper_layers = mask.clone();
        copper_layers.clear_non_copper_layers();

        let user_layers = mask & &Lset::user_defined_layers_mask(MAX_USER_DEFINED_LAYERS);

        self.copper_layer_count = copper_layers.count() as i32;
        self.user_defined_layer_count = user_layers.count() as i32;
    }

    /// `BOARD_DESIGN_SETTINGS::IsLayerEnabled`.
    pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool {
        if layer.0 >= 0 && layer.0 < PCB_LAYER_ID_COUNT {
            return self.enabled_layers.test(layer);
        }

        false
    }

    /// `BOARD_DESIGN_SETTINGS::SetBoardThickness`.
    pub fn set_board_thickness(&mut self, thickness: i32) {
        self.board_thickness = thickness;
    }

    /// Mutable `BOARD_DESIGN_SETTINGS::GetStackupDescriptor`.
    pub fn stackup_descriptor_mut(&mut self) -> &mut BoardStackup {
        &mut self.stackup
    }

    /// Mutable `BOARD_DESIGN_SETTINGS::GetDefaultZoneSettings`.
    pub fn default_zone_settings_mut(&mut self) -> &mut ZoneSettings {
        &mut self.default_zone_settings
    }

    /// `BOARD_DESIGN_SETTINGS::SetDefaultZoneSettings`.
    pub fn set_default_zone_settings(&mut self, settings: &ZoneSettings) {
        self.default_zone_settings = settings.clone();
    }

    /// Mutable `BOARD_DESIGN_SETTINGS::GetTeadropParamsList`.
    pub fn teardrop_params_list_mut(&mut self) -> &mut TeardropParametersList {
        &mut self.teardrop_params_list
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentNetClassName`.
    pub fn current_net_class_name(&self) -> &str {
        &self.current_net_class_name
    }

    /// `BOARD_DESIGN_SETTINGS::SetAuxOrigin`.
    pub fn set_aux_origin(&mut self, origin: Vector2I) {
        self.aux_origin = origin;
    }

    /// `BOARD_DESIGN_SETTINGS::SetGridOrigin`.
    pub fn set_grid_origin(&mut self, origin: Vector2I) {
        self.grid_origin = origin;
    }

    /// `BOARD_DESIGN_SETTINGS( const BOARD_DESIGN_SETTINGS& )`: the copy constructor. The
    /// members `initFromOther` copies come from `other`; the others (teardrop parameters,
    /// tuning pattern settings, zone layer properties, drill symbol profile) are
    /// default-constructed, exactly as the C++ copy constructor leaves them.
    pub fn detached_copy(&self) -> BoardDesignSettings {
        let mut copy = BoardDesignSettings::default();

        // The members initFromOther() does not copy, default-constructed (not the values the
        // main constructor body writes).
        copy.teardrop_params_list = TeardropParametersList::default();
        copy.meander_settings = [
            default_meander_json(DEFAULT_MEANDER_SPACING),
            default_meander_json(DEFAULT_MEANDER_SPACING),
            default_meander_json(DEFAULT_MEANDER_SPACING),
        ];
        copy.zone_layer_properties = BTreeMap::new();
        copy.drill_symbol_profile = None;

        copy.init_from_other(self);
        copy
    }

    /// `BOARD_DESIGN_SETTINGS::operator=` / `initFromOther`: copy every member that
    /// `initFromOther` copies; the teardrop parameters, tuning pattern settings, zone layer
    /// properties and drill symbol profile of `self` are left unchanged. (Dropped: the BOM
    /// members.)
    pub fn init_from_other(&mut self, other: &BoardDesignSettings) {
        // Copy of NESTED_SETTINGS around is not allowed, so let's just update the params.
        self.track_width_list = other.track_width_list.clone();
        self.vias_dimensions_list = other.vias_dimensions_list.clone();
        self.via_stack_presets = other.via_stack_presets.clone();
        self.diff_pair_dimensions_list = other.diff_pair_dimensions_list.clone();
        self.current_via_type = other.current_via_type;
        self.use_connected_track_width = other.use_connected_track_width;
        self.temp_override_track_width = other.temp_override_track_width;
        self.min_clearance = other.min_clearance;
        self.min_groove_width = other.min_groove_width;
        self.min_conn = other.min_conn;
        self.track_min_width = other.track_min_width;
        self.vias_min_annular_width = other.vias_min_annular_width;
        self.vias_min_size = other.vias_min_size;
        self.min_through_drill = other.min_through_drill;
        self.micro_vias_min_size = other.micro_vias_min_size;
        self.micro_vias_min_drill = other.micro_vias_min_drill;
        self.copper_edge_clearance = other.copper_edge_clearance;
        self.hole_clearance = other.hole_clearance;
        self.hole_to_hole_min = other.hole_to_hole_min;
        self.silk_clearance = other.silk_clearance;
        self.min_resolved_spokes = other.min_resolved_spokes;
        self.min_silk_text_height = other.min_silk_text_height;
        self.min_silk_text_thickness = other.min_silk_text_thickness;
        self.drc_severities = other.drc_severities.clone();
        self.drc_exclusions = other.drc_exclusions.clone();
        self.zone_keep_external_fillets = other.zone_keep_external_fillets;
        self.max_error = other.max_error;
        self.solder_mask_expansion = other.solder_mask_expansion;
        self.solder_mask_min_width = other.solder_mask_min_width;
        self.solder_mask_to_copper_clearance = other.solder_mask_to_copper_clearance;
        self.solder_paste_margin = other.solder_paste_margin;
        self.solder_paste_margin_ratio = other.solder_paste_margin_ratio;
        self.allow_soldermask_bridges_in_fps = other.allow_soldermask_bridges_in_fps;
        self.tent_vias_front = other.tent_vias_front;
        self.tent_vias_back = other.tent_vias_back;
        self.cover_vias_front = other.cover_vias_front;
        self.cover_vias_back = other.cover_vias_back;
        self.plug_vias_front = other.plug_vias_front;
        self.plug_vias_back = other.plug_vias_back;
        self.cap_vias = other.cap_vias;
        self.fill_vias = other.fill_vias;
        self.default_fp_text_items = other.default_fp_text_items.clone();
        self.user_layer_names = other.user_layer_names.clone();

        self.line_thickness = other.line_thickness;
        self.text_size = other.text_size;
        self.text_thickness = other.text_thickness;
        self.text_italic = other.text_italic;
        self.text_upright = other.text_upright;

        self.dimension_units_mode = other.dimension_units_mode;
        self.dimension_precision = other.dimension_precision;
        self.dimension_units_format = other.dimension_units_format;
        self.dimension_suppress_zeroes = other.dimension_suppress_zeroes;
        self.dimension_text_position = other.dimension_text_position;
        self.dimension_keep_text_aligned = other.dimension_keep_text_aligned;
        self.dimension_arrow_length = other.dimension_arrow_length;
        self.dimension_extension_offset = other.dimension_extension_offset;

        self.aux_origin = other.aux_origin;
        self.grid_origin = other.grid_origin;
        self.has_stackup = other.has_stackup;
        self.use_height_for_length_calcs = other.use_height_for_length_calcs;

        self.track_width_index = other.track_width_index;
        self.via_size_index = other.via_size_index;
        self.via_stack_index = other.via_stack_index;
        self.diff_pair_index = other.diff_pair_index;
        self.use_custom_track_via = other.use_custom_track_via;
        self.custom_track_width = other.custom_track_width;
        self.custom_via_size = other.custom_via_size;
        self.use_custom_diff_pair = other.use_custom_diff_pair;
        self.custom_diff_pair = other.custom_diff_pair;
        self.copper_layer_count = other.copper_layer_count;
        self.user_defined_layer_count = other.user_defined_layer_count;
        self.enabled_layers = other.enabled_layers.clone();
        self.board_thickness = other.board_thickness;
        self.current_net_class_name = other.current_net_class_name.clone();
        self.stackup = other.stackup.clone();
        // m_NetSettings is a shared_ptr in C++ (the copy shares the object); the Rust model
        // owns its NetSettings by value.
        self.net_settings = other.net_settings.clone();
        self.default_zone_settings = other.default_zone_settings.clone();

        self.style_fp_fields = other.style_fp_fields;
        self.style_fp_text = other.style_fp_text;
        self.style_fp_shapes = other.style_fp_shapes;
        self.style_fp_dimensions = other.style_fp_dimensions;
        self.style_fp_barcodes = other.style_fp_barcodes;
    }

    /// `BOARD_DESIGN_SETTINGS::GetBoardThickness`.
    pub fn board_thickness(&self) -> i32 {
        self.board_thickness
    }

    /// `BOARD_DESIGN_SETTINGS::GetStackupDescriptor`.
    pub fn stackup_descriptor(&self) -> &BoardStackup {
        &self.stackup
    }

    /// `BOARD_DESIGN_SETTINGS::GetDefaultZoneSettings`.
    pub fn default_zone_settings(&self) -> &ZoneSettings {
        &self.default_zone_settings
    }

    /// `BOARD_DESIGN_SETTINGS::GetTeadropParamsList`.
    pub fn teardrop_params_list(&self) -> &TeardropParametersList {
        &self.teardrop_params_list
    }

    /// `BOARD_DESIGN_SETTINGS::GetAuxOrigin`.
    pub fn aux_origin(&self) -> Vector2I {
        self.aux_origin
    }

    /// `BOARD_DESIGN_SETTINGS::GetGridOrigin`.
    pub fn grid_origin(&self) -> Vector2I {
        self.grid_origin
    }

    /// `BOARD_DESIGN_SETTINGS::GetSeverity`: `ERROR` for a code that is not in the map (C++
    /// also asserts).
    pub fn get_severity(&self, drc_error_code: i32) -> Severity {
        self.drc_severities
            .get(&drc_error_code)
            .copied()
            .unwrap_or(Severity::Error)
    }

    /// `BOARD_DESIGN_SETTINGS::Ignore`.
    pub fn ignore(&self, drc_error_code: i32) -> bool {
        self.get_severity(drc_error_code) == Severity::Ignore
    }

    /// `BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue` (asks the engine's
    /// `QueryWorstConstraint` when there is one).
    pub fn get_biggest_clearance_value(&self, rules: Option<&dyn RuleResolver>) -> i32 {
        let mut biggest = self.min_clearance.max(self.hole_clearance);

        biggest = biggest.max(self.hole_to_hole_min);
        biggest = biggest.max(self.copper_edge_clearance);

        if let Some(engine) = rules {
            // `QueryWorstConstraint` leaves the constraint unchanged when it finds none; the
            // previous value is already part of `biggest`, so `None` changes nothing.
            for constraint in [
                DrcConstraintT::ClearanceConstraint,
                DrcConstraintT::PhysicalClearanceConstraint,
                DrcConstraintT::PhysicalHoleClearanceConstraint,
                DrcConstraintT::HoleClearanceConstraint,
                DrcConstraintT::EdgeClearanceConstraint,
                DrcConstraintT::HoleToHoleConstraint,
            ] {
                if let Some(worst) = engine.query_worst_constraint(constraint) {
                    biggest = biggest.max(worst.value().min());
                }
            }
        }

        // Clip to avoid integer overflows in subsequent calculations
        biggest.min(maximum_clearance())
    }

    /// `BOARD_DESIGN_SETTINGS::GetSmallestClearanceValue`: the default netclass clearance,
    /// lowered by every netclass that has one.
    pub fn get_smallest_clearance_value(&self) -> i32 {
        let mut clearance = self.net_settings.default_netclass().clearance();

        for netclass in self.net_settings.netclasses.values() {
            if netclass.has_clearance() {
                clearance = clearance.min(netclass.clearance());
            }
        }

        clearance
    }

    /// `BOARD_DESIGN_SETTINGS::GetDRCEpsilon`: `ADVANCED_CFG::m_DRCEpsilon` (the built-in
    /// default; the advanced config file is not read, see `PORT_STATUS.md`).
    pub fn get_drc_epsilon(&self) -> i32 {
        PCB_IU_SCALE.mm_to_iu(ADVANCED_CFG_DRC_EPSILON_MM)
    }

    /// `BOARD_DESIGN_SETTINGS::GetHolePlatingThickness`: `ADVANCED_CFG::m_HoleWallThickness`
    /// (built-in default).
    pub fn get_hole_plating_thickness(&self) -> i32 {
        PCB_IU_SCALE.mm_to_iu(ADVANCED_CFG_HOLE_WALL_THICKNESS_MM)
    }

    /// `BOARD_DESIGN_SETTINGS::GetLayerClass`.
    pub fn get_layer_class(&self, layer: PcbLayerId) -> usize {
        if layer == PcbLayerId::FSilkS || layer == PcbLayerId::BSilkS {
            LAYER_CLASS_SILK
        } else if is_copper_layer(layer) {
            LAYER_CLASS_COPPER
        } else if layer == PcbLayerId::EdgeCuts {
            LAYER_CLASS_EDGES
        } else if layer == PcbLayerId::FCrtYd || layer == PcbLayerId::BCrtYd {
            LAYER_CLASS_COURTYARD
        } else if layer == PcbLayerId::FFab || layer == PcbLayerId::BFab {
            LAYER_CLASS_FAB
        } else {
            LAYER_CLASS_OTHERS
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetLineThickness`.
    pub fn get_line_thickness(&self, layer: PcbLayerId) -> i32 {
        self.line_thickness[self.get_layer_class(layer)]
    }

    /// `BOARD_DESIGN_SETTINGS::GetTextSize`.
    pub fn get_text_size(&self, layer: PcbLayerId) -> Vector2I {
        self.text_size[self.get_layer_class(layer)]
    }

    /// `BOARD_DESIGN_SETTINGS::GetTextThickness`.
    pub fn get_text_thickness(&self, layer: PcbLayerId) -> i32 {
        self.text_thickness[self.get_layer_class(layer)]
    }

    /// `BOARD_DESIGN_SETTINGS::GetTextItalic`.
    pub fn get_text_italic(&self, layer: PcbLayerId) -> bool {
        self.text_italic[self.get_layer_class(layer)]
    }

    /// `BOARD_DESIGN_SETTINGS::GetTextUpright`.
    pub fn get_text_upright(&self, layer: PcbLayerId) -> bool {
        self.text_upright[self.get_layer_class(layer)]
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentTrackWidth`.
    pub fn get_current_track_width(&self) -> i32 {
        if self.use_custom_track_via {
            self.custom_track_width
        } else if self.track_width_index <= 0
            || self.track_width_index >= self.track_width_list.len() as i32
        {
            self.net_settings.default_netclass().track_width()
        } else {
            self.track_width_list[self.track_width_index as usize]
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentViaSize`.
    pub fn get_current_via_size(&self) -> i32 {
        if self.use_custom_track_via {
            self.custom_via_size.diameter
        } else if self.via_size_index <= 0
            || self.via_size_index >= self.vias_dimensions_list.len() as i32
        {
            self.net_settings.default_netclass().via_diameter()
        } else {
            self.vias_dimensions_list[self.via_size_index as usize].diameter
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentViaDrill`: `-1` when the selected drill is not
    /// positive.
    pub fn get_current_via_drill(&self) -> i32 {
        let drill = if self.use_custom_track_via {
            self.custom_via_size.drill
        } else if self.via_size_index <= 0
            || self.via_size_index >= self.vias_dimensions_list.len() as i32
        {
            self.net_settings.default_netclass().via_drill()
        } else {
            self.vias_dimensions_list[self.via_size_index as usize].drill
        };

        if drill > 0 { drill } else { -1 }
    }

    /// `BOARD_DESIGN_SETTINGS::UseNetClassTrack`.
    pub fn use_net_class_track(&self) -> bool {
        self.track_width_index <= 0 && !self.use_custom_track_via
    }

    /// `BOARD_DESIGN_SETTINGS::UseNetClassVia`.
    pub fn use_net_class_via(&self) -> bool {
        self.via_size_index <= 0 && !self.use_custom_track_via
    }

    /// `BOARD_DESIGN_SETTINGS::UseNetClassDiffPair` (note: `== 0`, not `<= 0`, as C++).
    pub fn use_net_class_diff_pair(&self) -> bool {
        self.diff_pair_index == 0 && !self.use_custom_diff_pair
    }

    /// `BOARD_DESIGN_SETTINGS::GetTrackWidthIndex`.
    pub fn track_width_index(&self) -> i32 {
        self.track_width_index
    }

    /// `BOARD_DESIGN_SETTINGS::SetTrackWidthIndex`: clamped to the list; disables the custom
    /// track / via sizes.
    pub fn set_track_width_index(&mut self, index: i32) {
        self.track_width_index = if self.track_width_list.is_empty() {
            0
        } else {
            index.clamp(0, self.track_width_list.len() as i32 - 1)
        };
        self.use_custom_track_via = false;
    }

    /// `BOARD_DESIGN_SETTINGS::GetNextTrackWidthIndex`.
    pub fn get_next_track_width_index(&self, index: i32, forward: bool) -> i32 {
        next_predefined_index(index, forward, self.track_width_list.len() as i32)
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomTrackWidth`.
    pub fn set_custom_track_width(&mut self, width: i32) {
        self.custom_track_width = width;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomTrackWidth`.
    pub fn custom_track_width(&self) -> i32 {
        self.custom_track_width
    }

    /// `BOARD_DESIGN_SETTINGS::GetViaSizeIndex`.
    pub fn via_size_index(&self) -> i32 {
        self.via_size_index
    }

    /// `BOARD_DESIGN_SETTINGS::GetViaStackIndex`.
    pub fn via_stack_index(&self) -> i32 {
        self.via_stack_index
    }

    /// `BOARD_DESIGN_SETTINGS::SetViaStackIndex` (not clamped, as C++).
    pub fn set_via_stack_index(&mut self, index: i32) {
        self.via_stack_index = index;
    }

    /// `BOARD_DESIGN_SETTINGS::SetViaSizeIndex`: clamped to the list; disables the custom
    /// track / via sizes.
    pub fn set_via_size_index(&mut self, index: i32) {
        self.via_size_index = if self.vias_dimensions_list.is_empty() {
            0
        } else {
            index.clamp(0, self.vias_dimensions_list.len() as i32 - 1)
        };
        self.use_custom_track_via = false;
    }

    /// `BOARD_DESIGN_SETTINGS::GetNextViaSizeIndex`.
    pub fn get_next_via_size_index(&self, index: i32, forward: bool) -> i32 {
        next_predefined_index(index, forward, self.vias_dimensions_list.len() as i32)
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomViaSize`.
    pub fn set_custom_via_size(&mut self, size: i32) {
        self.custom_via_size.diameter = size;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomViaSize`.
    pub fn custom_via_size(&self) -> i32 {
        self.custom_via_size.diameter
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomViaDrill`.
    pub fn set_custom_via_drill(&mut self, drill: i32) {
        self.custom_via_size.drill = drill;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomViaDrill`.
    pub fn custom_via_drill(&self) -> i32 {
        self.custom_via_size.drill
    }

    /// `BOARD_DESIGN_SETTINGS::GetDiffPairIndex`.
    pub fn diff_pair_index(&self) -> i32 {
        self.diff_pair_index
    }

    /// `BOARD_DESIGN_SETTINGS::SetDiffPairIndex`: clamped to the list; disables the custom
    /// diff pair dimensions.
    pub fn set_diff_pair_index(&mut self, index: i32) {
        self.diff_pair_index = if self.diff_pair_dimensions_list.is_empty() {
            0
        } else {
            index.clamp(0, self.diff_pair_dimensions_list.len() as i32 - 1)
        };
        self.use_custom_diff_pair = false;
    }

    /// `BOARD_DESIGN_SETTINGS::GetNextDiffPairIndex`.
    pub fn get_next_diff_pair_index(&self, index: i32, forward: bool) -> i32 {
        next_predefined_index(index, forward, self.diff_pair_dimensions_list.len() as i32)
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomDiffPairWidth`.
    pub fn set_custom_diff_pair_width(&mut self, width: i32) {
        self.custom_diff_pair.width = width;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomDiffPairWidth`.
    pub fn custom_diff_pair_width(&self) -> i32 {
        self.custom_diff_pair.width
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomDiffPairGap`.
    pub fn set_custom_diff_pair_gap(&mut self, gap: i32) {
        self.custom_diff_pair.gap = gap;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomDiffPairGap`.
    pub fn custom_diff_pair_gap(&self) -> i32 {
        self.custom_diff_pair.gap
    }

    /// `BOARD_DESIGN_SETTINGS::SetCustomDiffPairViaGap`.
    pub fn set_custom_diff_pair_via_gap(&mut self, gap: i32) {
        self.custom_diff_pair.via_gap = gap;
    }

    /// `BOARD_DESIGN_SETTINGS::GetCustomDiffPairViaGap`: the gap when the via gap is not
    /// positive.
    pub fn custom_diff_pair_via_gap(&self) -> i32 {
        if self.custom_diff_pair.via_gap > 0 {
            self.custom_diff_pair.via_gap
        } else {
            self.custom_diff_pair.gap
        }
    }

    /// `BOARD_DESIGN_SETTINGS::UseCustomDiffPairDimensions( aEnabled )`.
    pub fn set_use_custom_diff_pair_dimensions(&mut self, enabled: bool) {
        self.use_custom_diff_pair = enabled;
    }

    /// `BOARD_DESIGN_SETTINGS::UseCustomDiffPairDimensions()`.
    pub fn use_custom_diff_pair_dimensions(&self) -> bool {
        self.use_custom_diff_pair
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentDiffPairWidth`.
    pub fn get_current_diff_pair_width(&self) -> i32 {
        if self.use_custom_diff_pair {
            self.custom_diff_pair.width
        } else if self.diff_pair_index <= 0
            || self.diff_pair_index >= self.diff_pair_dimensions_list.len() as i32
        {
            let default_netclass = self.net_settings.default_netclass();

            if default_netclass.has_diff_pair_width() {
                default_netclass.diff_pair_width()
            } else {
                default_netclass.track_width()
            }
        } else {
            self.diff_pair_dimensions_list[self.diff_pair_index as usize].width
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentDiffPairGap`.
    pub fn get_current_diff_pair_gap(&self) -> i32 {
        if self.use_custom_diff_pair {
            self.custom_diff_pair.gap
        } else if self.diff_pair_index <= 0
            || self.diff_pair_index >= self.diff_pair_dimensions_list.len() as i32
        {
            let default_netclass = self.net_settings.default_netclass();

            if default_netclass.has_diff_pair_gap() {
                default_netclass.diff_pair_gap()
            } else {
                default_netclass.clearance()
            }
        } else {
            self.diff_pair_dimensions_list[self.diff_pair_index as usize].gap
        }
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentDiffPairViaGap`.
    pub fn get_current_diff_pair_via_gap(&self) -> i32 {
        if self.use_custom_diff_pair {
            self.custom_diff_pair.via_gap
        } else if self.diff_pair_index <= 0
            || self.diff_pair_index >= self.diff_pair_dimensions_list.len() as i32
        {
            let default_netclass = self.net_settings.default_netclass();

            if default_netclass.has_diff_pair_via_gap() {
                default_netclass.diff_pair_via_gap()
            } else {
                self.get_current_diff_pair_gap()
            }
        } else {
            self.diff_pair_dimensions_list[self.diff_pair_index as usize].via_gap
        }
    }

    /// `BOARD_DESIGN_SETTINGS::ValidateDesignRules( aUnits )`: per-field range errors of the
    /// constraint values, teardrop ratios, diff pair and via dimension lists (entry 0 of the
    /// lists is the netclass placeholder and is skipped). `units` defaults to millimetres.
    /// Messages are the untranslated C++ texts.
    pub fn validate_design_rules(&self, units: Option<EdaUnits>) -> Vec<ValidationError> {
        let mut errors: Vec<ValidationError> = Vec::new();
        let units = units.unwrap_or(EdaUnits::Mm);
        let mm = |v: f64| PCB_IU_SCALE.mm_to_iu(v);

        let add_range_error = |errors: &mut Vec<ValidationError>,
                                   field: &str,
                                   value: i32,
                                   min: i32,
                                   max: i32| {
            if value < min || value > max {
                let min = string_from_iu(units, min);
                let max = string_from_iu(units, max);

                errors.push(ValidationError::new(
                    field,
                    &format!("Value must be between {min} and {max}."),
                ));
            }
        };

        let add_ratio_range_error =
            |errors: &mut Vec<ValidationError>, field: &str, value: f64, min: f64, max: f64| {
                if !value.is_finite() || value < min || value > max {
                    errors.push(ValidationError::new(
                        field,
                        &format!("Value must be between {min:.3} and {max:.3}."),
                    ));
                }
            };

        let e = &mut errors;
        add_range_error(e, "min_clearance", self.min_clearance, mm(0.00), mm(25.0));
        add_range_error(e, "min_connection", self.min_conn, mm(0.00), mm(100.0));
        add_range_error(e, "min_track_width", self.track_min_width, mm(0.00), mm(25.0));
        add_range_error(
            e,
            "min_via_annular_width",
            self.vias_min_annular_width,
            mm(0.00),
            mm(25.0),
        );
        add_range_error(e, "min_via_diameter", self.vias_min_size, mm(0.00), mm(25.0));
        add_range_error(
            e,
            "min_through_hole_diameter",
            self.min_through_drill,
            mm(0.00),
            mm(25.0),
        );
        add_range_error(
            e,
            "min_microvia_diameter",
            self.micro_vias_min_size,
            mm(0.00),
            mm(10.0),
        );
        add_range_error(
            e,
            "min_microvia_drill",
            self.micro_vias_min_drill,
            mm(0.00),
            mm(10.0),
        );
        add_range_error(e, "min_hole_to_hole", self.hole_to_hole_min, mm(0.00), mm(10.0));
        add_range_error(e, "min_hole_clearance", self.hole_clearance, mm(0.00), mm(100.0));
        add_range_error(e, "min_silk_clearance", self.silk_clearance, mm(-10.0), mm(100.0));
        add_range_error(e, "min_groove_width", self.min_groove_width, mm(0.00), mm(25.0));
        add_range_error(e, "min_text_height", self.min_silk_text_height, mm(0.00), mm(100.0));
        add_range_error(
            e,
            "min_text_thickness",
            self.min_silk_text_thickness,
            mm(0.00),
            mm(25.0),
        );
        add_range_error(
            e,
            "min_copper_edge_clearance",
            self.copper_edge_clearance,
            mm(-0.01),
            mm(25.0),
        );

        if self.min_resolved_spokes < 0 || self.min_resolved_spokes > 99 {
            e.push(ValidationError::new(
                "min_resolved_spokes",
                "Value must be between 0 and 99.",
            ));
        }

        add_range_error(
            e,
            "max_error",
            self.max_error,
            mm(MINIMUM_ERROR_SIZE_MM),
            mm(MAXIMUM_ERROR_SIZE_MM),
        );

        add_range_error(
            e,
            "solder_mask_expansion",
            self.solder_mask_expansion,
            mm(-25.0),
            mm(25.0),
        );
        add_range_error(
            e,
            "solder_mask_min_width",
            self.solder_mask_min_width,
            mm(0.0),
            mm(25.0),
        );
        add_range_error(
            e,
            "solder_mask_to_copper_clearance",
            self.solder_mask_to_copper_clearance,
            mm(0.0),
            mm(25.0),
        );
        add_range_error(
            e,
            "solder_paste_margin",
            self.solder_paste_margin,
            mm(-25.0),
            mm(25.0),
        );
        add_ratio_range_error(
            e,
            "solder_paste_margin_ratio",
            self.solder_paste_margin_ratio,
            -1.0,
            1.0,
        );

        let teardrop_params_list = &self.teardrop_params_list;

        for ii in 0..teardrop_params_list.parameters_count() {
            // GetParameters( static_cast<TARGET_TD>( ii ) ) is m_params_list.at( ii ).
            let params = &teardrop_params_list.params_list[ii];
            let target = teardrop_target_canonical_name(match ii {
                0 => TargetTd::Round,
                1 => TargetTd::Rect,
                2 => TargetTd::Track,
                _ => TargetTd::Unknown,
            });

            add_ratio_range_error(
                e,
                &format!("teardrop_parameters[{target}].best_length_ratio"),
                params.best_length_ratio,
                0.0,
                1.0,
            );
            add_ratio_range_error(
                e,
                &format!("teardrop_parameters[{target}].best_width_ratio"),
                params.best_width_ratio,
                0.0,
                1.0,
            );
            add_ratio_range_error(
                e,
                &format!("teardrop_parameters[{target}].width_to_size_filter_ratio"),
                params.width_to_size_filter_ratio,
                0.0,
                1.0,
            );
        }

        for (ii, diff_pair) in self.diff_pair_dimensions_list.iter().enumerate().skip(1) {
            if diff_pair.width > 0 && diff_pair.gap <= 0 {
                e.push(ValidationError::new(
                    &format!("diff_pair_dimensions_list[{ii}].gap"),
                    "No differential pair gap defined.",
                ));
            }
        }

        for (ii, via_dim) in self.vias_dimensions_list.iter().enumerate().skip(1) {
            let via_diameter = (via_dim.diameter > 0).then_some(via_dim.diameter);
            let via_drill = (via_dim.drill > 0).then_some(via_dim.drill);

            if let Some(message) = validate_via_diameter_and_drill(via_diameter, via_drill) {
                e.push(ValidationError::new(
                    &format!("via_dimensions_list[{ii}]"),
                    message,
                ));
            }
        }

        errors
    }

    /// `BOARD_DESIGN_SETTINGS::GetDesignRuleWarnings( aUnits )`: legal values without effect
    /// because other rules imply a stricter limit (advisory).
    pub fn get_design_rule_warnings(&self, units: Option<EdaUnits>) -> Vec<ValidationError> {
        let mut warnings = Vec::new();
        let units = units.unwrap_or(EdaUnits::Mm);
        let str_ = |value: i32| string_from_iu(units, value);

        // Microvias are excluded because their default drill plus annulus already exceeds
        // their default diameter
        let implied_via_min = self.min_through_drill + 2 * self.vias_min_annular_width;

        if self.vias_min_size < implied_via_min {
            warnings.push(ValidationError::new(
                "min_via_diameter",
                &format!(
                    "Minimum via diameter ({}) is smaller than the minimum drill size ({}) plus \
                     twice the minimum annular width ({}), so it has no effect. The effective \
                     minimum via diameter is {}.",
                    str_(self.vias_min_size),
                    str_(self.min_through_drill),
                    str_(self.vias_min_annular_width),
                    str_(implied_via_min)
                ),
            ));
        }

        warnings
    }

    /// `BOARD_DESIGN_SETTINGS::UseCustomTrackViaSize( aEnabled )`.
    pub fn set_use_custom_track_via_size(&mut self, enabled: bool) {
        self.use_custom_track_via = enabled;
    }

    /// `BOARD_DESIGN_SETTINGS::UseCustomTrackViaSize()`.
    pub fn use_custom_track_via_size(&self) -> bool {
        self.use_custom_track_via
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS` (`pcbnew/board_design_settings.cpp:47`),
    /// checked against the verified tables of `rust/docs/analysis/settings.md` 3.6 / 3.7.
    #[test]
    fn constructor_defaults() {
        let b = BoardDesignSettings::default();
        assert_eq!(b.max_error(), 5000);
        assert_eq!(b.copper_layer_count, 2);
        assert_eq!(b.user_defined_layer_count, 4);
        assert!(b.enabled_layers.contains(PcbLayerId::FCu));
        assert!(b.enabled_layers.contains(PcbLayerId::BCu));
        assert!(!b.enabled_layers.contains(PcbLayerId::In1Cu));
        assert!(b.enabled_layers.contains(PcbLayerId::User4));
        assert!(!b.enabled_layers.contains(PcbLayerId::User5));
        assert!(b.enabled_layers.contains(PcbLayerId::FFab));
        assert_eq!(
            (b.min_clearance, b.min_conn, b.track_min_width),
            (0, 0, 200_000)
        );
        assert_eq!(b.vias_min_annular_width, 100_000);
        assert_eq!((b.vias_min_size, b.min_through_drill), (500_000, 300_000));
        assert_eq!(
            (b.micro_vias_min_size, b.micro_vias_min_drill),
            (200_000, 100_000)
        );
        assert_eq!(b.copper_edge_clearance, 500_000);
        assert_eq!((b.hole_clearance, b.hole_to_hole_min), (250_000, 250_000));
        assert_eq!((b.silk_clearance, b.min_groove_width), (0, 0));
        assert_eq!(b.min_resolved_spokes, 2);
        assert_eq!(
            (b.min_silk_text_height, b.min_silk_text_thickness),
            (800_000, 80_000)
        );
        assert!(b.use_height_for_length_calcs && !b.zone_keep_external_fillets);
        assert!(b.tent_vias_front && b.tent_vias_back && !b.cap_vias && !b.fill_vias);
        assert_eq!(b.board_thickness, 1_600_000);
        assert_eq!(b.custom_track_width, 200_000);
        assert_eq!(
            b.custom_via_size,
            ViaDimension {
                diameter: 500_000,
                drill: 300_000
            }
        );
        assert_eq!(
            b.custom_diff_pair,
            DiffPairDimension {
                width: 125_000,
                gap: 180_000,
                via_gap: 180_000
            }
        );
        assert_eq!(b.current_via_type, Viatype::Through);
        assert_eq!(
            b.line_thickness,
            [100_000, 200_000, 50_000, 50_000, 100_000, 100_000]
        );
        assert_eq!(
            b.text_size[LAYER_CLASS_COPPER],
            Vector2I::new(1_500_000, 1_500_000)
        );
        assert_eq!(
            b.text_size[LAYER_CLASS_FAB],
            Vector2I::new(1_000_000, 1_000_000)
        );
        assert_eq!(
            b.text_thickness,
            [100_000, 300_000, 150_000, 150_000, 150_000, 150_000]
        );
        assert_eq!(b.default_fp_text_items.len(), 3);
        assert_eq!(
            b.default_fp_text_items[0],
            TextItemInfo::new("REF**", true, PcbLayerId::FSilkS)
        );
        assert_eq!(b.default_fp_text_items[2].text, "${REFERENCE}");
        assert_eq!(b.dimension_precision, DimPrecision::X_XXXX);
        assert_eq!(b.dimension_units_mode, DimUnitsMode::Automatic);
        assert_eq!(b.dimension_arrow_length, 1_270_000);
        assert_eq!(b.dimension_extension_offset, 500_000);
        assert!(!b.has_stackup && b.drill_symbol_profile.is_none());
        assert_eq!(b.meander_settings[1]["spacing"], JsonValue::from(1.0));
        assert_eq!(b.meander_settings[0]["spacing"], JsonValue::from(0.6));
        assert_eq!(b.get_layer_class(PcbLayerId::BCrtYd), LAYER_CLASS_COURTYARD);
        assert_eq!(b.get_text_thickness(PcbLayerId::In3Cu), 300_000);
    }

    /// `m_DRCSeverities` (`board_design_settings.cpp:169-219`; settings.md 3.7: 78 codes,
    /// 33 not `error`).
    #[test]
    fn drc_severity_defaults() {
        let b = BoardDesignSettings::default();
        assert_eq!(b.drc_severities.len(), 78);
        let not_error = b
            .drc_severities
            .values()
            .filter(|s| **s != Severity::Error)
            .count();
        assert_eq!(not_error, 33);
        let ignored: Vec<i32> = b
            .drc_severities
            .iter()
            .filter(|(_, s)| **s == Severity::Ignore)
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(ignored, vec![33, 44, 45, 73, 75]);
        assert_eq!(b.get_severity(DRCE_ISOLATED_COPPER), Severity::Warning);
        assert_eq!(b.get_severity(DRCE_CLEARANCE), Severity::Error);
        assert_eq!(b.get_severity(999), Severity::Error);
        assert!(b.ignore(DRCE_MISSING_COURTYARD));
    }
}
