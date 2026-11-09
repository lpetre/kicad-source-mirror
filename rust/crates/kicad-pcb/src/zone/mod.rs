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

//! Port of `pcbnew/zone.h` and `pcbnew/zone.cpp` (`ZONE`, `ISOLATED_ISLANDS`),
//! `board-model-types.md` section 5.
//!
//! This file holds the data, the constructor, the delegation shims (frozen after M0), the
//! fill state with its derived-state writers (`board-model.md` 8.4) and the non-shape
//! inherent methods; the shape methods are in `shapes.rs`. Bodies with logic are
//! `port_pending!` stubs for package M18.

pub mod shapes;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::hash_128::Hash128;
use kicad_geom::{Box2I, EdaAngle, EdaOrientation, FlipDirection, Seg, ShapePolySet, Vector2I};

use crate::board::Board;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::ids::ZoneId;
use crate::item::{BoardItemBase, ConnectedItemBase, SharedShape};
use crate::teardrop_parameters::TeardropType;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, Flashing, Invalidate};
use crate::zone_settings::{
    CornerSmoothing, IslandRemovalMode, PlacementSourceT, RepeatLayoutOptions, ThievingPattern,
    ThievingSettings, ZONE_BORDER_HATCH_DIST_MM, ZONE_BORDER_HATCH_MAXDIST_MM,
    ZONE_BORDER_HATCH_MINDIST_MM, ZoneBorderDisplayStyle, ZoneConnection, ZoneFillMode,
    ZoneLayerProperties, ZoneSettings,
};

/// `ZONE`.
#[derive(Clone, Debug)]
pub struct Zone {
    pub base: ConnectedItemBase,
    /// m_Poly. Footprint library frame for a footprint zone, board frame for a board zone
    /// (`library_outline()` / `board_outline(ctx)` port GetLibraryOutline / GetBoardOutline;
    /// `outline_mut(env)` ports Outline() for editing).
    pub(crate) lib_outline: ShapePolySet,
    pub corner_smoothing_type: CornerSmoothing, // m_cornerSmoothingType
    pub corner_radius: u32,                     // m_cornerRadius
    pub zone_name: String,                      // m_zoneName
    pub layer_set: Lset,                        // m_layerSet
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_layerProperties
    pub priority: u32,                          // m_priority
    pub is_rule_area: bool,                     // m_isRuleArea
    pub placement_area_enabled: bool,           // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT, // m_placementAreaSourceType
    pub placement_area_source: String,          // m_placementAreaSource
    pub link_set: Kiid,                         // m_linkSet
    pub repeat_options: RepeatLayoutOptions,    // m_repeatOptions
    pub teardrop_type: TeardropType,            // m_teardropType
    pub do_not_allow_zone_fills: bool,          // m_doNotAllowZoneFills
    pub do_not_allow_vias: bool,                // m_doNotAllowVias
    pub do_not_allow_tracks: bool,              // m_doNotAllowTracks
    pub do_not_allow_pads: bool,                // m_doNotAllowPads
    pub do_not_allow_footprints: bool,          // m_doNotAllowFootprints
    pub pad_connection: ZoneConnection,         // m_PadConnection
    pub zone_clearance: i32,                    // m_ZoneClearance
    pub zone_min_thickness: i32,                // m_ZoneMinThickness
    pub fill_version: i32,                      // m_fillVersion
    pub island_removal_mode: IslandRemovalMode, // m_islandRemovalMode
    pub min_island_area: i64,                   // m_minIslandArea (long long)
    pub thermal_relief_gap: i32,                // m_thermalReliefGap
    pub thermal_relief_spoke_width: i32,        // m_thermalReliefSpokeWidth
    pub fill_mode: ZoneFillMode,                // m_fillMode
    pub hatch_thickness: i32,                   // m_hatchThickness
    pub hatch_gap: i32,                         // m_hatchGap
    pub hatch_orientation: EdaOrientation,      // m_hatchOrientation
    pub hatch_smoothing_level: i32,             // m_hatchSmoothingLevel
    pub hatch_smoothing_value: f64,             // m_hatchSmoothingValue
    pub hatch_hole_min_area: f64,               // m_hatchHoleMinArea
    pub hatch_border_algorithm: i32,            // m_hatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,    // m_thievingSettings
    pub local_flgs: i32,                        // m_localFlgs
    /// Fill results (class-2 derived state, written through `Board::zone_fill_mut`).
    pub(crate) fill: ZoneFill,
    pub border_style: ZoneBorderDisplayStyle, // m_borderStyle
    pub border_hatch_pitch: i32,              // m_borderHatchPitch
    pub border_hatch_lines: Vec<Seg>,         // m_borderHatchLines
    pub area: f64,                            // m_area
    pub outline_area: f64,                    // m_outlinearea
    /// m_bboxCache + m_bboxCacheTimeStamp (board time stamp).
    pub(crate) bbox_cache: KeyedCache<(Box2I, i32)>,
}

/// Written by the zone filler and connectivity; read by DRC, plotting and the writer.
#[derive(Clone, Debug, Default)]
pub struct ZoneFill {
    /// m_FilledPolysList. `Arc` like the C++ `shared_ptr`: consumers may keep a snapshot while
    /// the filler installs a new one.
    pub filled_polys_list: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,
    pub fill_flags: Lset,                                       // m_fillFlags
    pub filled_polys_hash: BTreeMap<PcbLayerId, Hash128>,       // m_filledPolysHash
    pub insulated_islands: BTreeMap<PcbLayerId, BTreeSet<i32>>, // m_insulatedIslands
    pub is_filled: bool,                                        // m_isFilled
    pub need_refill: bool,                                      // m_needRefill
}

/// `ISOLATED_ISLANDS`: produced by connectivity, consumed by the filler and DRC.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IsolatedIslands {
    pub isolated_outlines: Vec<i32>,          // m_IsolatedOutlines
    pub single_connection_outlines: Vec<i32>, // m_SingleConnectionOutlines
}

// ---- constructor -------------------------------------------------------------------------

impl Default for Zone {
    /// `ZONE( aParent )` without a board: the member initialisers, `m_fillVersion = 5`, then
    /// `ZONE_SETTINGS().ExportSetting( *this, false )` (no layer set, name, priority or net),
    /// then `m_needRefill = false`. The exported values are written directly here (equal to
    /// what the setters store for these inputs: `SetMinThickness` raises the hatch values
    /// before the hatch setters overwrite them, `SetBorderDisplayStyle` clamps the pitch to
    /// 0.1 .. 2 mm and hatches an empty outline).
    fn default() -> Self {
        let settings = ZoneSettings::default();

        let border_hatch_pitch = settings
            .border_hatch_pitch
            .max(PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_MINDIST_MM))
            .min(PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_MAXDIST_MM));

        let mut base = ConnectedItemBase::default();
        base.item.is_locked = settings.locked; // SetLocked( m_Locked )

        Zone {
            base,
            lib_outline: ShapePolySet::new(),
            corner_smoothing_type: settings.corner_smoothing_type,
            corner_radius: settings.corner_radius,
            zone_name: String::new(),
            layer_set: Lset::new(),
            layer_properties: BTreeMap::new(),
            priority: 0,
            is_rule_area: settings.is_rule_area,
            placement_area_enabled: settings.placement_area_enabled,
            placement_area_source_type: settings.placement_area_source_type,
            placement_area_source: settings.placement_area_source.clone(),
            link_set: Kiid::default(),
            repeat_options: RepeatLayoutOptions::default(),
            teardrop_type: TeardropType::TdNone,
            do_not_allow_zone_fills: settings.keepout_do_not_allow_zone_fills,
            do_not_allow_vias: settings.keepout_do_not_allow_vias,
            do_not_allow_tracks: settings.keepout_do_not_allow_tracks,
            do_not_allow_pads: settings.keepout_do_not_allow_pads,
            do_not_allow_footprints: settings.keepout_do_not_allow_footprints,
            // Thieving zones never produce thermal reliefs (ExportSetting).
            pad_connection: if settings.fill_mode == ZoneFillMode::CopperThieving {
                ZoneConnection::None
            } else {
                settings.pad_connection
            },
            zone_clearance: settings.zone_clearance,
            zone_min_thickness: settings.zone_min_thickness,
            fill_version: 5, // the "old" way to build filled polygon areas (< 6.0.x)
            island_removal_mode: settings.remove_islands,
            min_island_area: settings.min_island_area,
            thermal_relief_gap: settings.thermal_relief_gap as i32,
            thermal_relief_spoke_width: settings.thermal_relief_spoke_width as i32,
            fill_mode: settings.fill_mode,
            hatch_thickness: settings.hatch_thickness,
            hatch_gap: settings.hatch_gap,
            hatch_orientation: EdaOrientation::new(settings.hatch_orientation),
            hatch_smoothing_level: settings.hatch_smoothing_level,
            hatch_smoothing_value: settings.hatch_smoothing_value,
            hatch_hole_min_area: settings.hatch_hole_min_area,
            hatch_border_algorithm: settings.hatch_border_algorithm,
            thieving_settings: settings.thieving_settings.clone(),
            local_flgs: 0, // flags temporary used in zone calculations
            fill: ZoneFill::default(),
            border_style: settings.zone_border_display_style,
            border_hatch_pitch,
            border_hatch_lines: Vec::new(),
            area: 0.0,
            outline_area: 0.0,
            bbox_cache: KeyedCache::default(),
        }
    }
}

impl Zone {
    /// `new ZONE( aParent )` without a board: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut zone = Zone::default();
        zone.base.item.uuid = Kiid::new(kiid_generator);
        zone
    }

    /// `new ZONE( aParent )` on a board: the member initialisers, then the board's default
    /// zone settings exported (`GetDesignSettings().GetDefaultZoneSettings().ExportSetting`),
    /// `SetIsRuleArea( true )` first for a footprint zone.
    pub fn new_with_settings(
        kiid_generator: &mut KiidGenerator,
        settings: &ZoneSettings,
        in_footprint: bool,
    ) -> Self {
        crate::port_pending!("ZONE::ZONE", kiid_generator, settings, in_footprint)
    }

    // ---- outline (board-model.md 5) -----------------------------------------------------

    /// `GetLibraryOutline()`: the stored outline (library frame in a footprint).
    pub fn library_outline(&self) -> &ShapePolySet {
        &self.lib_outline
    }

    /// The stored outline for the parser (library frame in a footprint, as C++ fills
    /// `m_Poly`).
    pub fn lib_outline_mut(&mut self) -> &mut ShapePolySet {
        &mut self.lib_outline
    }

    /// `GetBoardOutline()`: the outline in the board frame.
    pub fn board_outline(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        crate::port_pending!("ZONE::GetBoardOutline", ctx)
    }

    /// `Outline()` for editing a board-frame outline.
    pub fn outline_mut(&mut self, env: &MutEnv) -> &mut ShapePolySet {
        crate::port_pending!("ZONE::Outline", env)
    }

    // ---- fill state (read side) ---------------------------------------------------------

    /// The fill results.
    pub fn fill(&self) -> &ZoneFill {
        &self.fill
    }

    /// `GetFilledPolysList( aLayer )` (`std::map::at`: panics for a layer without a fill,
    /// where C++ asserts and throws).
    pub fn filled_polys_list(&self, layer: PcbLayerId) -> Arc<ShapePolySet> {
        Arc::clone(&self.fill.filled_polys_list[&layer])
    }

    /// `HasFilledPolysForLayer( aLayer )`.
    pub fn has_filled_polys_for_layer(&self, layer: PcbLayerId) -> bool {
        self.fill.filled_polys_list.contains_key(&layer)
    }

    /// `IsFilled()`.
    pub fn is_filled(&self) -> bool {
        self.fill.is_filled
    }

    /// `NeedRefill()`.
    pub fn need_refill(&self) -> bool {
        self.fill.need_refill
    }

    /// `IsIsland( aLayer, aPolyIdx )`: every fill polygon of a zone without a net is an
    /// island.
    pub fn is_island(&self, layer: PcbLayerId, poly_idx: i32) -> bool {
        if self.base.net.0 < 1 {
            return true;
        }

        self.fill
            .insulated_islands
            .get(&layer)
            .is_some_and(|set| set.contains(&poly_idx))
    }

    /// `GetHashValue( aLayer )`: the hash of an empty polygon set for a layer without one.
    pub fn hash_value(&self, layer: PcbLayerId) -> Hash128 {
        crate::port_pending!("ZONE::GetHashValue", layer)
    }

    // ---- trivial accessors --------------------------------------------------------------

    /// `GetZoneName()`.
    pub fn zone_name(&self) -> &str {
        &self.zone_name
    }

    /// `GetAssignedPriority()`.
    pub fn assigned_priority(&self) -> u32 {
        self.priority
    }

    /// `GetIsRuleArea()`.
    pub fn is_rule_area(&self) -> bool {
        self.is_rule_area
    }

    /// `GetFillMode()`.
    pub fn fill_mode(&self) -> ZoneFillMode {
        self.fill_mode
    }

    /// `IsCopperThieving()`.
    pub fn is_copper_thieving(&self) -> bool {
        self.fill_mode == ZoneFillMode::CopperThieving
    }

    /// `GetThievingPattern()`.
    pub fn thieving_pattern(&self) -> ThievingPattern {
        self.thieving_settings.pattern
    }

    /// `GetPadConnection()`.
    pub fn pad_connection(&self) -> ZoneConnection {
        self.pad_connection
    }

    /// `GetMinThickness()`.
    pub fn min_thickness(&self) -> i32 {
        self.zone_min_thickness
    }

    /// `GetHatchOrientation()`.
    pub fn hatch_orientation(&self) -> EdaAngle {
        self.hatch_orientation.angle()
    }

    /// `GetDefaultHatchPitch()`.
    pub fn default_hatch_pitch() -> i32 {
        PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_DIST_MM)
    }

    /// `GetLocalFlags()`.
    pub fn local_flags(&self) -> i32 {
        self.local_flgs
    }

    /// `SetLocalFlags()`.
    pub fn set_local_flags(&mut self, flags: i32) {
        self.local_flgs = flags;
    }

    // ---- setters with side effects (package M18) ----------------------------------------

    /// `ZONE::SetFillMode()`.
    pub fn set_fill_mode(&mut self, fill_mode: ZoneFillMode) {
        crate::port_pending!("ZONE::SetFillMode", fill_mode)
    }

    /// `ZONE::SetMinThickness()`: also raises the hatch thickness / gap and the thieving line
    /// width to at least `min_thickness`, and marks the zone for refill.
    pub fn set_min_thickness(&mut self, min_thickness: i32) {
        crate::port_pending!("ZONE::SetMinThickness", min_thickness)
    }

    /// `ZONE::SetCornerRadius()`.
    pub fn set_corner_radius(&mut self, radius: u32) {
        crate::port_pending!("ZONE::SetCornerRadius", radius)
    }

    /// `ZONE::SetBorderDisplayStyle( aStyle, aPitch, aRebuildBorderHatch )`.
    pub fn set_border_display_style(
        &mut self,
        env: &MutEnv,
        style: ZoneBorderDisplayStyle,
        pitch: i32,
        rebuild_border_hatch: bool,
    ) {
        crate::port_pending!(
            "ZONE::SetBorderDisplayStyle",
            env,
            style,
            pitch,
            rebuild_border_hatch
        )
    }

    /// `ZONE::HatchBorder()`: rebuild `border_hatch_lines` from the board-frame outline.
    pub fn hatch_border(&mut self, env: &MutEnv) {
        crate::port_pending!("ZONE::HatchBorder", env)
    }

    /// `ZONE::UnFill()`: true if there was a fill.
    pub fn unfill(&mut self) -> bool {
        crate::port_pending!("ZONE::UnFill")
    }

    /// `ZONE::CalculateFilledArea()`.
    pub fn calculate_filled_area(&mut self) -> f64 {
        crate::port_pending!("ZONE::CalculateFilledArea")
    }

    /// `ZONE::CalculateOutlineArea()`.
    pub fn calculate_outline_area(&mut self, ctx: &BoardCtx<'_>) -> f64 {
        crate::port_pending!("ZONE::CalculateOutlineArea", ctx)
    }

    /// `ZONE::GetFirstLayer()`.
    pub fn first_layer(&self) -> PcbLayerId {
        crate::port_pending!("ZONE::GetFirstLayer")
    }

    /// `ZONE::HigherPriority( aOther )`.
    pub fn higher_priority(&self, other: &Zone) -> bool {
        crate::port_pending!("ZONE::HigherPriority", other)
    }

    /// `ZONE::SameNet( aOther )`.
    pub fn same_net(&self, other: &Zone) -> bool {
        crate::port_pending!("ZONE::SameNet", other)
    }

    /// `ZONE::IsConflicting()`.
    pub fn is_conflicting(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("ZONE::IsConflicting", ctx)
    }

    /// `ZONE::BuildSmoothedPoly( aSmoothedPoly, aLayer, aBoardOutline, aSmoothedPolyWithApron )`:
    /// false if the outline is degenerate.
    pub fn build_smoothed_poly(
        &self,
        ctx: &BoardCtx<'_>,
        smoothed_poly: &mut ShapePolySet,
        layer: PcbLayerId,
        board_outline: Option<&ShapePolySet>,
        smoothed_poly_with_apron: Option<&mut ShapePolySet>,
    ) -> bool {
        crate::port_pending!(
            "ZONE::BuildSmoothedPoly",
            ctx,
            smoothed_poly,
            layer,
            board_outline,
            smoothed_poly_with_apron
        )
    }

    // ---- virtual overrides (package M18) ------------------------------------------------

    /// `ZONE::GetLayer()`.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        crate::port_pending!("ZONE::GetLayer", ctx)
    }

    /// `ZONE::GetLayerSet()`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("ZONE::GetLayerSet", ctx)
    }

    /// `ZONE::IsOnLayer()`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("ZONE::IsOnLayer", ctx, layer)
    }

    /// `ZONE::IsOnCopperLayer()`.
    pub fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("ZONE::IsOnCopperLayer", ctx)
    }

    /// `ZONE::GetPosition()`: the first outline corner.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("ZONE::GetPosition", ctx)
    }

    /// `ZONE::IsConnected()`: `!GetIsRuleArea()`.
    pub fn is_connected(&self, _ctx: &BoardCtx<'_>) -> bool {
        !self.is_rule_area
    }

    /// `ZONE::GetLocalClearance()`: none for a rule area, else `m_ZoneClearance`.
    pub fn local_clearance(&self) -> Option<i32> {
        crate::port_pending!("ZONE::GetLocalClearance")
    }

    /// `ZONE::GetFriendlyName()`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("ZONE::GetFriendlyName", ctx)
    }

    /// `ZONE::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("ZONE::GetItemDescription", ctx, units, full)
    }

    /// `ZONE::SetPosition()`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("ZONE::SetPosition", env, pos)
    }

    /// `ZONE::Move()`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("ZONE::Move", env, delta)
    }

    /// `ZONE::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("ZONE::Rotate", env, centre, angle)
    }

    /// `ZONE::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("ZONE::Flip", env, centre, dir)
    }

    /// `ZONE::Mirror()`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("ZONE::Mirror", env, centre, dir)
    }

    /// `ZONE::SetLayer()`.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        crate::port_pending!("ZONE::SetLayer", env, layer)
    }

    /// `ZONE::SetLayerSet()`: ignores an empty set, clamps thieving zones to one layer,
    /// clears the fills of a changed set.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("ZONE::SetLayerSet (virtual)", env, layers)
    }

    /// `ZONE::OnFootprintTransformed()`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("ZONE::OnFootprintTransformed", env)
    }

    /// `ZONE::OnFootprintRescaled()`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        crate::port_pending!(
            "ZONE::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `ZONE::operator==( const BOARD_ITEM& )`.
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Zone) -> bool {
        crate::port_pending!("ZONE::operator==", ctx, other)
    }
}

// ---- fill writers (derived state, board-model.md 8.4) ------------------------------------

impl ZoneFill {
    /// `ZONE::SetFilledPolysList( aLayer, aPolysList )`: installs a copy.
    pub fn set_filled_polys_list(&mut self, layer: PcbLayerId, polys: &ShapePolySet) {
        self.filled_polys_list
            .insert(layer, Arc::new(polys.clone()));
    }

    /// `ZONE::GetFillFlag( aLayer )`.
    pub fn fill_flag(&self, layer: PcbLayerId) -> bool {
        self.fill_flags.test(layer)
    }

    /// `ZONE::SetFillFlag( aLayer, aFlag )`.
    pub fn set_fill_flag(&mut self, layer: PcbLayerId, flag: bool) {
        self.fill_flags.set_value(layer, flag);
    }

    /// `ZONE::SetIsFilled()`.
    pub fn set_is_filled(&mut self, is_filled: bool) {
        self.is_filled = is_filled;
    }

    /// `ZONE::SetNeedRefill()`.
    pub fn set_need_refill(&mut self, need_refill: bool) {
        self.need_refill = need_refill;
    }

    /// `ZONE::SetIsIsland( aLayer, aPolyIdx )`.
    pub fn set_is_island(&mut self, layer: PcbLayerId, poly_idx: i32) {
        self.insulated_islands
            .entry(layer)
            .or_default()
            .insert(poly_idx);
    }

    /// `ZONE::BuildHashValue( aLayer )`.
    pub fn build_hash_value(&mut self, layer: PcbLayerId) {
        crate::port_pending!("ZONE::BuildHashValue", layer)
    }

    /// `ZONE::CacheTriangulation( aLayer )` (`UNDEFINED_LAYER`: every layer).
    pub fn cache_triangulation(&mut self, layer: PcbLayerId) {
        crate::port_pending!("ZONE::CacheTriangulation", layer)
    }
}

impl Board {
    /// The fill results of zone `id` for the zone filler and connectivity (derived state:
    /// neither invalidates caches nor bumps the time stamp, like C++). Panics on a stale
    /// handle.
    pub fn zone_fill_mut(&mut self, id: ZoneId) -> &mut ZoneFill {
        &mut self
            .zones
            .get_mut(id)
            .expect("zone_fill_mut: stale zone handle")
            .fill
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for Zone {
    fn invalidate_caches(&mut self) {
        self.bbox_cache.clear();
    }
}

impl BoardItem for Zone {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbZoneT
    }
    fn class_name(&self) -> &'static str {
        "ZONE"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        Zone::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        Zone::layer_set(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        Zone::is_on_layer(self, ctx, layer)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        Zone::is_on_copper_layer(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        Zone::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        Zone::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        Zone::effective_shape(self, ctx, layer, flash, usage)
    }
    fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        Zone::transform_shape_to_polygon(
            self,
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        )
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        Zone::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        Zone::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        Zone::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        Zone::friendly_name(self, ctx)
    }
}

impl BoardConnectedItem for Zone {
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        Zone::is_connected(self, ctx)
    }
    fn local_clearance(&self) -> Option<i32> {
        Zone::local_clearance(self)
    }
}

impl BoardItemMut for Zone {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        Zone::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        Zone::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        Zone::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        Zone::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        Zone::mirror(self, env, centre, dir)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        Zone::set_layer(self, env, layer)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        Zone::set_layer_set(self, env, layers)
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        Zone::on_footprint_transformed(self, env)
    }
    fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        Zone::on_footprint_rescaled(
            self,
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ItemKind, NetCode};
    use crate::test_support::rotated_flipped_scaled;
    use kicad_geom::ANGLE_0;

    /// `ZONE::ZONE( aParent )` without a board (zone.cpp): member initialisers, then
    /// `ZONE_SETTINGS().ExportSetting( *this, false )`.
    #[test]
    fn zone_constructor_defaults() {
        let z = Zone::default();
        assert_eq!(z.corner_smoothing_type, CornerSmoothing::NoSmoothing);
        assert_eq!(z.corner_radius, 0);
        assert_eq!(z.priority, 0);
        assert!(z.layer_set.count() == 0 && z.zone_name.is_empty());
        assert!(!z.is_rule_area && !z.placement_area_enabled);
        assert_eq!(z.placement_area_source_type, PlacementSourceT::Sheetname);
        assert_eq!(z.link_set, Kiid::default());
        assert_eq!(z.teardrop_type, TeardropType::TdNone);
        assert!(!z.do_not_allow_zone_fills && !z.do_not_allow_footprints);
        assert!(z.do_not_allow_vias && z.do_not_allow_tracks && z.do_not_allow_pads);
        assert_eq!(z.pad_connection, ZoneConnection::Thermal);
        assert_eq!((z.zone_clearance, z.zone_min_thickness), (500_000, 250_000));
        assert_eq!(z.fill_version, 5);
        assert_eq!(z.island_removal_mode, IslandRemovalMode::Always);
        assert_eq!(z.min_island_area, 10_000_000_000_000);
        assert_eq!(
            (z.thermal_relief_gap, z.thermal_relief_spoke_width),
            (500_000, 500_000)
        );
        assert_eq!(z.fill_mode, ZoneFillMode::Polygons);
        assert_eq!((z.hatch_thickness, z.hatch_gap), (1_000_000, 1_500_000));
        assert_eq!(z.hatch_orientation(), ANGLE_0);
        assert_eq!((z.hatch_smoothing_level, z.hatch_smoothing_value), (0, 0.1));
        assert_eq!((z.hatch_hole_min_area, z.hatch_border_algorithm), (0.15, 1));
        assert_eq!(z.thieving_settings.line_width, 300_000);
        assert_eq!(z.local_flgs, 0);
        assert!(!z.is_filled() && !z.need_refill());
        assert_eq!(z.border_style, ZoneBorderDisplayStyle::DiagonalEdge);
        assert_eq!(z.border_hatch_pitch, 500_000);
        assert!(z.border_hatch_lines.is_empty());
        assert_eq!((z.area, z.outline_area), (0.0, 0.0));
        assert!(!z.base.item.is_locked);
        assert_eq!(z.base.net, NetCode::UNCONNECTED);
        assert!(z.is_island(PcbLayerId::FCu, 0));
    }

    #[test]
    fn zone_fill_writers() {
        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Zone).typed::<Zone>().unwrap();
        let stamp = fx.board.time_stamp();
        {
            let fill = fx.board.zone_fill_mut(id);
            fill.set_filled_polys_list(PcbLayerId::FCu, &ShapePolySet::new());
            fill.set_fill_flag(PcbLayerId::FCu, true);
            fill.set_is_filled(true);
            fill.set_is_island(PcbLayerId::FCu, 3);
        }
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board.time_stamp(), stamp);
        let zone = &fx.board[id];
        assert!(zone.is_filled() && zone.fill().fill_flag(PcbLayerId::FCu));
        assert!(zone.has_filled_polys_for_layer(PcbLayerId::FCu));
        assert!(zone.fill().insulated_islands[&PcbLayerId::FCu].contains(&3));
    }
}
