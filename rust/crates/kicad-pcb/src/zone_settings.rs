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

//! Port of `pcbnew/zone_settings.h` and `pcbnew/zone_settings.cpp` (`ZONE_SETTINGS` and the
//! zone enums / small structs declared there), of `ZONE_CONNECTION` and the zone default
//! constants (`pcbnew/zones.h`), `board-model-types.md` section 5.
//!
//! Data and defaults are complete (M0b); `ExportSetting` and `operator<<( const ZONE& )` are
//! stubs for package M5.

use std::collections::BTreeMap;

use kicad_core::{Kiid, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{ANGLE_0, EdaAngle, EdaOrientation, Vector2I};

use crate::board::Board;
use crate::ctx::{BoardCtx, MutEnv};
use crate::ids::{NetCode, ZoneId};
use crate::teardrop_parameters::TeardropType;
use crate::traits::BoardItem;
use crate::zone::Zone;

/// `ZONE_THERMAL_RELIEF_GAP_MM`: default of `ZONE_SETTINGS::m_ThermalReliefGap`.
pub const ZONE_THERMAL_RELIEF_GAP_MM: f64 = 0.5;
/// `ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM`: default of `m_ThermalReliefSpokeWidth`.
pub const ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM: f64 = 0.5;
/// `ZONE_THICKNESS_MM`: default of `m_ZoneMinThickness`.
pub const ZONE_THICKNESS_MM: f64 = 0.25;
/// `ZONE_THICKNESS_MIN_VALUE_MM`: minimum of `m_ZoneMinThickness`.
pub const ZONE_THICKNESS_MIN_VALUE_MM: f64 = 0.025;
/// `ZONE_CLEARANCE_MM`: default of `m_ZoneClearance`.
pub const ZONE_CLEARANCE_MM: f64 = 0.5;
/// `ZONE_CLEARANCE_MAX_VALUE_MM`: maximum of `m_ZoneClearance`.
pub const ZONE_CLEARANCE_MAX_VALUE_MM: f64 = 100.0;
/// `ZONE_BORDER_HATCH_DIST_MM`: default of `m_BorderHatchPitch`.
pub const ZONE_BORDER_HATCH_DIST_MM: f64 = 0.5;
/// `ZONE_BORDER_HATCH_MINDIST_MM`: minimum of `m_BorderHatchPitch`.
pub const ZONE_BORDER_HATCH_MINDIST_MM: f64 = 0.1;
/// `ZONE_BORDER_HATCH_MAXDIST_MM`: maximum of `m_BorderHatchPitch`.
pub const ZONE_BORDER_HATCH_MAXDIST_MM: f64 = 2.0;

/// `ZONE_CONNECTION`; the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ZoneConnection {
    /// `ZONE_CONNECTION::INHERITED`.
    #[default]
    Inherited = -1,
    /// `ZONE_CONNECTION::NONE`: pads are not covered.
    None = 0,
    /// `ZONE_CONNECTION::THERMAL`: use thermal relief for pads.
    Thermal,
    /// `ZONE_CONNECTION::FULL`: pads are covered by copper.
    Full,
    /// `ZONE_CONNECTION::THT_THERMAL`: thermal relief only for THT pads.
    ThtThermal,
}

/// `ZONE_LAYER_PROPERTIES` (`pcbnew/zone_settings.h`). Complete.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ZoneLayerProperties {
    pub hatching_offset: Option<Vector2I>, // hatching_offset
}

/// `ZONE_FILL_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ZoneFillMode {
    /// Fill zone with polygons.
    #[default]
    Polygons = 0,
    /// Fill zone using a grid pattern.
    HatchPattern = 1,
    /// Fill zone with non-functional copper stamps for plating uniformity.
    CopperThieving = 2,
}

/// `THIEVING_PATTERN`: shape stamped onto the grid for a copper-thieving fill.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ThievingPattern {
    #[default]
    Dots = 0,
    Squares = 1,
    Hatch = 2,
}

/// `THIEVING_SETTINGS`: parameters of copper-thieving fill generation. `Default` = the
/// in-class initialisers (all zero, dots, not staggered).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ThievingSettings {
    pub pattern: ThievingPattern,
    /// Diameter (dots) or side length (squares).
    pub element_size: i32,
    /// Edge-to-edge spacing between adjacent stamps (or between crosshatch lines).
    pub gap: i32,
    /// Line width for crosshatch only.
    pub line_width: i32,
    /// Offset alternating rows by half the stride.
    pub stagger: bool,
    pub orientation: EdaOrientation,
}

/// `ZONE_BORDER_DISPLAY_STYLE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ZoneBorderDisplayStyle {
    #[default]
    NoHatch,
    DiagonalFull,
    DiagonalEdge,
    /// Disable outline drawing for very special cases.
    InvisibleBorder,
}

/// `ISLAND_REMOVAL_MODE`: whether or not to remove isolated islands from a zone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum IslandRemovalMode {
    #[default]
    Always = 0,
    Never,
    Area,
}

/// `PLACEMENT_SOURCE_T`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PlacementSourceT {
    #[default]
    Sheetname = 0,
    ComponentClass,
    GroupPlacement,
    DesignBlock,
}

/// `ZONE_SETTINGS::CORNER_SMOOTHING`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum CornerSmoothing {
    /// Zone outline is used without change.
    #[default]
    NoSmoothing = 0,
    /// Zone outline is used after chamfering corners.
    Chamfer,
    /// Zone outline is used after rounding corners.
    Fillet,
}

/// `REPEAT_LAYOUT_OPTIONS`: options for copying a placement rule area onto the others linked
/// to it.
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatLayoutOptions {
    pub copy_routing: bool,           // m_copyRouting
    pub connected_routing_only: bool, // m_connectedRoutingOnly
    pub copy_placement: bool,         // m_copyPlacement
    pub copy_other_items: bool,       // m_copyOtherItems
    pub group_items: bool,            // m_groupItems
    pub include_locked_items: bool,   // m_includeLockedItems
    /// m_anchor: this area's own anchor footprint, or nil for the area centre.
    pub anchor: Kiid,
}

impl Default for RepeatLayoutOptions {
    /// The in-class initialisers.
    fn default() -> Self {
        RepeatLayoutOptions {
            copy_routing: true,
            connected_routing_only: false,
            copy_placement: true,
            copy_other_items: true,
            group_items: false,
            include_locked_items: true,
            anchor: Kiid::default(),
        }
    }
}

/// `ZONE_SETTINGS`: the default zone settings of the board (also the `(zone_defaults ..)`
/// reader). Equality is the C++ `operator==` (manual impl below).
#[derive(Clone, Debug)]
pub struct ZoneSettings {
    pub zone_priority: u32,                                // m_ZonePriority
    pub fill_mode: ZoneFillMode,                           // m_FillMode
    pub zone_clearance: i32,                               // m_ZoneClearance
    pub zone_min_thickness: i32,                           // m_ZoneMinThickness
    pub hatch_thickness: i32,                              // m_HatchThickness
    pub hatch_gap: i32,                                    // m_HatchGap
    pub hatch_orientation: EdaAngle,                       // m_HatchOrientation
    pub hatch_smoothing_level: i32,                        // m_HatchSmoothingLevel
    pub hatch_smoothing_value: f64,                        // m_HatchSmoothingValue
    pub hatch_hole_min_area: f64,                          // m_HatchHoleMinArea
    pub hatch_border_algorithm: i32,                       // m_HatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,               // m_ThievingSettings
    pub netcode: i32,                                      // m_Netcode
    pub name: String,                                      // m_Name
    pub layers: Lset,                                      // m_Layers
    pub zone_border_display_style: ZoneBorderDisplayStyle, // m_ZoneBorderDisplayStyle
    pub border_hatch_pitch: i32,                           // m_BorderHatchPitch
    pub thermal_relief_gap: i64,                           // m_ThermalReliefGap (long)
    pub thermal_relief_spoke_width: i64,                   // m_ThermalReliefSpokeWidth (long)
    pub locked: bool,                                      // m_Locked
    pub teardrop_type: TeardropType,                       // m_TeardropType
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_LayerProperties
    pub corner_smoothing_type: CornerSmoothing,            // m_cornerSmoothingType
    pub corner_radius: u32,                                // m_cornerRadius
    pub pad_connection: ZoneConnection,                    // m_padConnection
    pub is_rule_area: bool,                                // m_isRuleArea
    pub placement_area_enabled: bool,                      // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT,      // m_placementAreaSourceType
    pub placement_area_source: String,                     // m_placementAreaSource
    pub keepout_do_not_allow_zone_fills: bool,             // m_keepoutDoNotAllowZoneFills
    pub keepout_do_not_allow_vias: bool,                   // m_keepoutDoNotAllowVias
    pub keepout_do_not_allow_tracks: bool,                 // m_keepoutDoNotAllowTracks
    pub keepout_do_not_allow_pads: bool,                   // m_keepoutDoNotAllowPads
    pub keepout_do_not_allow_footprints: bool,             // m_keepoutDoNotAllowFootprints
    pub remove_islands: IslandRemovalMode,                 // m_removeIslands
    pub min_island_area: i64,                              // m_minIslandArea (long long)
}

impl Default for ZoneSettings {
    /// `ZONE_SETTINGS::ZONE_SETTINGS()` (zone_settings.cpp).
    fn default() -> Self {
        let zone_min_thickness = PCB_IU_SCALE.mm_to_iu(ZONE_THICKNESS_MM);

        ZoneSettings {
            zone_priority: 0,
            fill_mode: ZoneFillMode::Polygons, // Mode for filling zone
            zone_clearance: PCB_IU_SCALE.mm_to_iu(ZONE_CLEARANCE_MM),
            // Min thickness value in filled areas (the minimum width of copper to fill solid
            // areas).
            zone_min_thickness,
            // Arbitrary defaults for the hatch settings.
            hatch_thickness: (zone_min_thickness * 4).max(PCB_IU_SCALE.mm_to_iu(1.0)),
            hatch_gap: (zone_min_thickness * 6).max(PCB_IU_SCALE.mm_to_iu(1.5)),
            hatch_orientation: ANGLE_0, // Grid style: orientation of grid lines
            hatch_smoothing_level: 0,   // Grid pattern smoothing type. 0 = no smoothing
            hatch_smoothing_value: 0.1, // Grid pattern chamfer value relative to the gap value
            hatch_hole_min_area: 0.15,  // Min size before holes are dropped (ratio of hole size)
            hatch_border_algorithm: 1,  // 0 = use zone min thickness; 1 = use hatch width
            // Provisional defaults for copper thieving stamps. Gap is the edge-to-edge
            // spacing between adjacent stamps, so the grid stride is element_size + gap.
            thieving_settings: ThievingSettings {
                pattern: ThievingPattern::Dots,
                element_size: PCB_IU_SCALE.mm_to_iu(0.5),
                gap: PCB_IU_SCALE.mm_to_iu(1.0),
                line_width: zone_min_thickness.max(PCB_IU_SCALE.mm_to_iu(0.3)),
                stagger: false,
                orientation: EdaOrientation::new(ANGLE_0),
            },
            netcode: 0, // Net code for the current zone
            name: String::new(),
            layers: Lset::from_layers(&[PcbLayerId::FCu]),
            // Option to show the zone outlines only, short hatches or full hatches.
            zone_border_display_style: ZoneBorderDisplayStyle::DiagonalEdge,
            border_hatch_pitch: PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_DIST_MM),
            // thickness of the gap in thermal reliefs:
            thermal_relief_gap: PCB_IU_SCALE.mm_to_iu(ZONE_THERMAL_RELIEF_GAP_MM) as i64,
            // thickness of the copper bridge in thermal reliefs:
            thermal_relief_spoke_width: PCB_IU_SCALE.mm_to_iu(ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM)
                as i64,
            locked: false,
            teardrop_type: TeardropType::TdNone,
            layer_properties: BTreeMap::new(),
            corner_smoothing_type: CornerSmoothing::NoSmoothing,
            corner_radius: 0,
            pad_connection: ZoneConnection::Thermal, // How pads are covered by copper in zone
            is_rule_area: false,
            placement_area_enabled: false,
            placement_area_source_type: PlacementSourceT::Sheetname,
            placement_area_source: String::new(),
            keepout_do_not_allow_zone_fills: false,
            keepout_do_not_allow_vias: true,
            keepout_do_not_allow_tracks: true,
            keepout_do_not_allow_pads: true,
            keepout_do_not_allow_footprints: false,
            remove_islands: IslandRemovalMode::Always,
            // 10 * IU_PER_MM * IU_PER_MM evaluated in double, converted to long long.
            min_island_area: (10.0 * PCB_IU_SCALE.iu_per_mm * PCB_IU_SCALE.iu_per_mm) as i64,
        }
    }
}

impl PartialEq for ZoneSettings {
    /// `ZONE_SETTINGS::operator==`. The layer properties are compared with the C++
    /// `std::equal( begin, end, other.begin )`: the other map may be longer (its extra
    /// entries are not looked at); a shorter one (undefined behaviour in C++) is unequal.
    fn eq(&self, other: &Self) -> bool {
        let layer_properties_equal = other.layer_properties.len() >= self.layer_properties.len()
            && self
                .layer_properties
                .iter()
                .zip(other.layer_properties.iter())
                .all(|(a, b)| a == b);

        self.zone_priority == other.zone_priority
            && self.fill_mode == other.fill_mode
            && self.zone_clearance == other.zone_clearance
            && self.zone_min_thickness == other.zone_min_thickness
            && self.hatch_thickness == other.hatch_thickness
            && self.hatch_gap == other.hatch_gap
            && self.hatch_orientation == other.hatch_orientation
            && self.hatch_smoothing_level == other.hatch_smoothing_level
            && self.hatch_smoothing_value == other.hatch_smoothing_value
            && self.hatch_border_algorithm == other.hatch_border_algorithm
            && self.hatch_hole_min_area == other.hatch_hole_min_area
            && self.thieving_settings == other.thieving_settings
            && self.netcode == other.netcode
            && self.name == other.name
            && self.zone_border_display_style == other.zone_border_display_style
            && self.border_hatch_pitch == other.border_hatch_pitch
            && self.thermal_relief_gap == other.thermal_relief_gap
            && self.thermal_relief_spoke_width == other.thermal_relief_spoke_width
            && self.pad_connection == other.pad_connection
            && self.corner_smoothing_type == other.corner_smoothing_type
            && self.corner_radius == other.corner_radius
            && self.is_rule_area == other.is_rule_area
            && self.placement_area_enabled == other.placement_area_enabled
            && self.placement_area_source_type == other.placement_area_source_type
            && self.placement_area_source == other.placement_area_source
            && self.keepout_do_not_allow_zone_fills == other.keepout_do_not_allow_zone_fills
            && self.keepout_do_not_allow_vias == other.keepout_do_not_allow_vias
            && self.keepout_do_not_allow_tracks == other.keepout_do_not_allow_tracks
            && self.keepout_do_not_allow_pads == other.keepout_do_not_allow_pads
            && self.keepout_do_not_allow_footprints == other.keepout_do_not_allow_footprints
            && self.locked == other.locked
            && self.remove_islands == other.remove_islands
            && self.min_island_area == other.min_island_area
            && layer_properties_equal
            && self.teardrop_type == other.teardrop_type
            && self.layers == other.layers
    }
}

impl ZoneSettings {
    /// `ZONE_SETTINGS::GetDefaultSettings()`.
    pub fn default_settings() -> ZoneSettings {
        ZoneSettings::default()
    }

    /// `ZONE_SETTINGS::operator<<( const ZONE& )`: copy the settings of `source`, a zone that
    /// is not on a board (`IsLocked()` is then always false). For a zone on a board use
    /// [`ZoneSettings::import_from_zone_ctx`].
    pub fn import_from_zone(&mut self, source: &Zone) {
        self.import_from_zone_impl(source, false);
    }

    /// `ZONE_SETTINGS::operator<<( const ZONE& )` for a zone on the board of `ctx`
    /// (`IsLocked()` sees the parent group and the board use).
    pub fn import_from_zone_ctx(&mut self, ctx: &BoardCtx<'_>, source: &Zone) {
        let locked = BoardItem::is_locked(source, ctx);
        self.import_from_zone_impl(source, locked);
    }

    /// The body of `ZONE_SETTINGS::operator<<`; `locked` is `aSource.IsLocked()`.
    fn import_from_zone_impl(&mut self, source: &Zone, locked: bool) {
        self.zone_priority = source.assigned_priority();
        self.fill_mode = source.fill_mode();
        // ZONE::GetLocalClearance() (no source argument) always holds a value (0 for a rule
        // area), so the C++ `.value()` never throws.
        self.zone_clearance = source.local_clearance().unwrap_or(0);
        self.zone_min_thickness = source.min_thickness();
        self.hatch_thickness = source.hatch_thickness;
        self.hatch_gap = source.hatch_gap;
        self.hatch_orientation = source.hatch_orientation();
        self.hatch_smoothing_level = source.hatch_smoothing_level;
        self.hatch_smoothing_value = source.hatch_smoothing_value;
        self.hatch_border_algorithm = source.hatch_border_algorithm;
        self.hatch_hole_min_area = source.hatch_hole_min_area;
        self.thieving_settings = source.thieving_settings.clone();
        // GetNetCode(): the stored code (ORPHANED = -1 for a null m_netinfo).
        self.netcode = source.base.net.0;
        self.name = source.zone_name().to_string();
        self.zone_border_display_style = source.border_style;
        self.border_hatch_pitch = source.border_hatch_pitch;
        self.thermal_relief_gap = i64::from(source.thermal_relief_gap);
        self.thermal_relief_spoke_width = i64::from(source.thermal_relief_spoke_width);
        self.pad_connection = source.pad_connection();
        self.corner_smoothing_type = source.corner_smoothing_type;
        self.corner_radius = source.corner_radius;
        self.is_rule_area = source.is_rule_area();
        self.placement_area_enabled = source.placement_area_enabled;
        self.placement_area_source_type = source.placement_area_source_type;
        self.placement_area_source = source.placement_area_source.clone();
        self.keepout_do_not_allow_zone_fills = source.do_not_allow_zone_fills;
        self.keepout_do_not_allow_vias = source.do_not_allow_vias;
        self.keepout_do_not_allow_tracks = source.do_not_allow_tracks;
        self.keepout_do_not_allow_pads = source.do_not_allow_pads;
        self.keepout_do_not_allow_footprints = source.do_not_allow_footprints;
        self.locked = locked;
        self.remove_islands = source.island_removal_mode;
        self.min_island_area = source.min_island_area;
        self.layer_properties = source.layer_properties.clone();

        // Currently, the teardrop area type is not really a ZONE_SETTINGS parameter,
        // but a ZONE parameter only.
        // However it can be used in dialogs
        self.teardrop_type = source.teardrop_type;

        // ZONE::GetLayerSet() returns m_layerSet (not frame dependent).
        self.layers = source.layer_set.clone();
    }

    /// `ZONE_SETTINGS::ExportSetting( aTarget, aFullExport )` for a zone that is not on a
    /// board (a new `ZONE( nullptr )`, the teardrop builder's template): the setters see the
    /// null-board environment and `SetNetCode` gives net 0 (`OrphanedItem()`). For a zone on
    /// a board use [`Board::export_zone_setting`].
    pub fn export_setting(&self, target: &mut Zone, full_export: bool) {
        if self
            .export_setting_env(&MutEnv::detached(), target, full_export)
            .is_some()
        {
            // BOARD_CONNECTED_ITEM::SetNetCode without a board: OrphanedItem() (net code 0),
            // whatever the code and the copper layers.
            target.base.net = NetCode::UNCONNECTED;
        }
    }

    /// `ZONE_SETTINGS::ExportSetting( aTarget, aFullExport )` with the mutation environment
    /// of `target`, except the `SetNetCode` call: the net code to give the zone (through
    /// `BOARD_CONNECTED_ITEM::SetNetCode`) is returned instead, `Some` only when the C++
    /// calls it. `SetNetCode` comes before `SetBorderDisplayStyle` in C++; the border hatch
    /// does not depend on the net, so the caller may apply it afterwards.
    pub fn export_setting_env(
        &self,
        env: &MutEnv,
        target: &mut Zone,
        full_export: bool,
    ) -> Option<i32> {
        let mut net_code = None;

        target.set_fill_mode(self.fill_mode);
        // SetLocalClearance( m_ZoneClearance ): m_ZoneClearance = aClearance.value_or( 0 ).
        target.zone_clearance = self.zone_clearance;
        target.set_min_thickness(self.zone_min_thickness);
        target.hatch_thickness = self.hatch_thickness;
        target.hatch_gap = self.hatch_gap;
        target.hatch_orientation = EdaOrientation::new(self.hatch_orientation);
        target.hatch_smoothing_level = self.hatch_smoothing_level;
        target.hatch_smoothing_value = self.hatch_smoothing_value;
        target.hatch_border_algorithm = self.hatch_border_algorithm;
        target.hatch_hole_min_area = self.hatch_hole_min_area;

        // ZONE::SetThievingSettings
        if target.thieving_settings != self.thieving_settings {
            target.fill.set_need_refill(true);
        }
        target.thieving_settings = self.thieving_settings.clone();

        // ZONE::SetThermalReliefGap / SetThermalReliefSpokeWidth take an int (implicit
        // long -> int conversion).
        let thermal_relief_gap = self.thermal_relief_gap as i32;
        if target.thermal_relief_gap != thermal_relief_gap {
            target.fill.set_need_refill(true);
        }
        target.thermal_relief_gap = thermal_relief_gap;

        let thermal_relief_spoke_width = self.thermal_relief_spoke_width as i32;
        if target.thermal_relief_spoke_width != thermal_relief_spoke_width {
            target.fill.set_need_refill(true);
        }
        target.thermal_relief_spoke_width = thermal_relief_spoke_width;

        // Thieving zones are non-electrical and must never produce thermal reliefs.
        // Force ZONE_CONNECTION::NONE so the filler skips thermal-spoke generation.
        if self.fill_mode == ZoneFillMode::CopperThieving {
            target.pad_connection = ZoneConnection::None;
        } else {
            target.pad_connection = self.pad_connection;
        }

        target.corner_smoothing_type = self.corner_smoothing_type;
        target.set_corner_radius(self.corner_radius);
        target.is_rule_area = self.is_rule_area;
        target.placement_area_enabled = self.placement_area_enabled;
        target.placement_area_source_type = self.placement_area_source_type;
        target.placement_area_source = self.placement_area_source.clone();
        target.do_not_allow_zone_fills = self.keepout_do_not_allow_zone_fills;
        target.do_not_allow_vias = self.keepout_do_not_allow_vias;
        target.do_not_allow_tracks = self.keepout_do_not_allow_tracks;
        target.do_not_allow_pads = self.keepout_do_not_allow_pads;
        target.do_not_allow_footprints = self.keepout_do_not_allow_footprints;
        // BOARD_ITEM::SetLocked (ZONE does not override it).
        target.base.item.is_locked = self.locked;
        target.island_removal_mode = self.remove_islands;
        target.min_island_area = self.min_island_area;

        // Currently, the teardrop area type is not imported from a ZONE_SETTINGS, because
        // it is not really a ZONE_SETTINGS parameter, but a ZONE parameter only
        // (C++: `#if 0 aTarget.SetTeardropAreaType( m_TeardropType ); #endif`)

        if full_export {
            target.priority = self.zone_priority;

            // ZONE::SetLayerProperties
            target.layer_properties = self.layer_properties.clone();
            target.set_layer_set(env, self.layers.clone());

            target.zone_name = self.name.clone();

            if self.fill_mode == ZoneFillMode::CopperThieving {
                net_code = Some(0);
            } else if !self.is_rule_area {
                net_code = Some(self.netcode);
            }
        }

        // call SetBorderDisplayStyle last, because hatch lines will be rebuilt,
        // using new parameters values
        target.set_border_display_style(
            env,
            self.zone_border_display_style,
            self.border_hatch_pitch,
            true,
        );

        net_code
    }

    /// `ZONE_SETTINGS::SetCornerRadius()`: negative radii become 0.
    pub fn set_corner_radius(&mut self, radius: i32) {
        self.corner_radius = if radius < 0 { 0 } else { radius as u32 };
    }

    /// `ZONE_SETTINGS::HasKeepoutParametersSet()`.
    pub fn has_keepout_parameters_set(&self) -> bool {
        self.keepout_do_not_allow_tracks
            || self.keepout_do_not_allow_vias
            || self.keepout_do_not_allow_pads
            || self.keepout_do_not_allow_footprints
            || self.keepout_do_not_allow_zone_fills
    }
}

impl Board {
    /// `aSettings.ExportSetting( *zone, aFullExport )` for a zone on this board: the zone's
    /// setters run with its mutation environment (`modify_zone`), then the net code goes
    /// through `BOARD_CONNECTED_ITEM::SetNetCode` (`set_item_net`, board lookup, net 0 off
    /// copper). `false` for a stale handle.
    pub fn export_zone_setting(
        &mut self,
        id: ZoneId,
        settings: &ZoneSettings,
        full_export: bool,
    ) -> bool {
        let Some(net_code) = self.modify_zone(id, |zone, env| {
            settings.export_setting_env(env, zone, full_export)
        }) else {
            return false;
        };

        if let Some(code) = net_code {
            self.set_item_net(id.into(), NetCode(code), false);
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ZONE_SETTINGS::ZONE_SETTINGS()` (zone_settings.cpp).
    #[test]
    fn zone_settings_defaults() {
        let s = ZoneSettings::default();
        assert_eq!(s.zone_priority, 0);
        assert_eq!(s.fill_mode, ZoneFillMode::Polygons);
        assert_eq!(s.zone_clearance, 500_000);
        assert_eq!(s.zone_min_thickness, 250_000);
        assert_eq!(s.hatch_thickness, 1_000_000);
        assert_eq!(s.hatch_gap, 1_500_000);
        assert_eq!(s.hatch_orientation, ANGLE_0);
        assert_eq!((s.hatch_smoothing_level, s.hatch_smoothing_value), (0, 0.1));
        assert_eq!((s.hatch_hole_min_area, s.hatch_border_algorithm), (0.15, 1));
        assert_eq!(s.thieving_settings.pattern, ThievingPattern::Dots);
        assert_eq!(s.thieving_settings.element_size, 500_000);
        assert_eq!(s.thieving_settings.gap, 1_000_000);
        assert_eq!(s.thieving_settings.line_width, 300_000);
        assert!(!s.thieving_settings.stagger);
        assert_eq!(s.netcode, 0);
        assert_eq!(s.layers, Lset::from_layers(&[PcbLayerId::FCu]));
        assert_eq!(
            s.zone_border_display_style,
            ZoneBorderDisplayStyle::DiagonalEdge
        );
        assert_eq!(s.border_hatch_pitch, 500_000);
        assert_eq!(
            (s.thermal_relief_gap, s.thermal_relief_spoke_width),
            (500_000, 500_000)
        );
        assert_eq!(s.pad_connection, ZoneConnection::Thermal);
        assert!(!s.locked && !s.is_rule_area && !s.placement_area_enabled);
        assert_eq!(s.corner_smoothing_type, CornerSmoothing::NoSmoothing);
        assert_eq!(s.corner_radius, 0);
        assert_eq!(s.remove_islands, IslandRemovalMode::Always);
        assert_eq!(s.min_island_area, 10_000_000_000_000);
        assert!(!s.keepout_do_not_allow_zone_fills && !s.keepout_do_not_allow_footprints);
        assert!(s.keepout_do_not_allow_vias && s.keepout_do_not_allow_tracks);
        assert!(s.keepout_do_not_allow_pads && s.has_keepout_parameters_set());
        assert_eq!(s.teardrop_type, TeardropType::TdNone);
        assert_eq!(s, ZoneSettings::default_settings());
    }

    #[test]
    fn zone_settings_equality_and_corner_radius() {
        let mut a = ZoneSettings::default();
        let mut b = ZoneSettings::default();
        b.layer_properties
            .insert(PcbLayerId::FCu, ZoneLayerProperties::default());
        // std::equal over a's (empty) range.
        assert_eq!(a, b);
        assert_ne!(b, a);
        a.set_corner_radius(-5);
        assert_eq!(a.corner_radius, 0);
        a.set_corner_radius(7);
        assert_eq!(a.corner_radius, 7);
        assert_eq!(RepeatLayoutOptions::default().anchor, Kiid::default());
    }
}
