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

use kicad_core::json::Value as JsonValue;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId, Severity};
use kicad_geom::Vector2I;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_sexpr::Sexpr;

use crate::board_stackup::BoardStackup;
use crate::ctx::RuleResolver;
use crate::drc_api::*;
use crate::net_settings::NetSettings;
use crate::pcb_dimension::{DimPrecision, DimTextPosition, DimUnitsFormat, DimUnitsMode};
use crate::pcb_track::Viatype;
use crate::teardrop_parameters::TeardropParametersList;
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
        crate::port_pending!("BOARD_DESIGN_SETTINGS::SetEnabledLayers", mask)
    }

    /// `BOARD_DESIGN_SETTINGS::IsLayerEnabled`.
    pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::IsLayerEnabled", layer)
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
        crate::port_pending!(
            "BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue",
            rules.is_some()
        )
    }

    /// `BOARD_DESIGN_SETTINGS::GetSmallestClearanceValue`.
    pub fn get_smallest_clearance_value(&self) -> i32 {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetSmallestClearanceValue")
    }

    /// `BOARD_DESIGN_SETTINGS::GetDRCEpsilon`.
    pub fn get_drc_epsilon(&self) -> i32 {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetDRCEpsilon")
    }

    /// `BOARD_DESIGN_SETTINGS::GetHolePlatingThickness`.
    pub fn get_hole_plating_thickness(&self) -> i32 {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetHolePlatingThickness")
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
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetCurrentTrackWidth")
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentViaSize`.
    pub fn get_current_via_size(&self) -> i32 {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetCurrentViaSize")
    }

    /// `BOARD_DESIGN_SETTINGS::GetCurrentViaDrill`.
    pub fn get_current_via_drill(&self) -> i32 {
        crate::port_pending!("BOARD_DESIGN_SETTINGS::GetCurrentViaDrill")
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
