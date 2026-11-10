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
//! inherent methods; the shape methods are in `shapes.rs`.

pub mod shapes;

#[cfg(test)]
mod oracle_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::shape_poly_set::VertexIndex;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::hash_128::Hash128;
use kicad_geom::{
    Box2I, EdaAngle, EdaOrientation, FlipDirection, Seg, Shape, ShapeLineChain, ShapePolySet,
    Vector2I,
};

use crate::board::Board;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::ids::{NetCode, ZoneId};
use crate::item::{BoardItemBase, ConnectedItemBase, ItemFlags, SharedShape};
use crate::pad::Pad;
use crate::teardrop_parameters::TeardropType;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, Flashing, Invalidate};
use crate::zone_settings::{
    CornerSmoothing, IslandRemovalMode, PlacementSourceT, RepeatLayoutOptions, ThievingPattern,
    ThievingSettings, ZONE_BORDER_HATCH_DIST_MM, ZONE_BORDER_HATCH_MAXDIST_MM,
    ZONE_BORDER_HATCH_MINDIST_MM, ZoneBorderDisplayStyle, ZoneConnection, ZoneFillMode,
    ZoneLayerProperties, ZoneSettings,
};
use crate::{board_connected_item, board_item};

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

    /// `new ZONE( aParent )` on a board: the member initialisers, `SetIsRuleArea( true )` for
    /// a footprint zone, then the board's default zone settings exported
    /// (`GetDesignSettings().GetDefaultZoneSettings().ExportSetting( *this, false )`), then
    /// `m_needRefill = false`.
    ///
    /// Note (C++ behaviour kept): `ExportSetting` calls `SetIsRuleArea( m_isRuleArea )` of the
    /// settings, so the footprint rule-area flag set before it is overwritten.
    pub fn new_with_settings(
        kiid_generator: &mut KiidGenerator,
        settings: &ZoneSettings,
        in_footprint: bool,
    ) -> Self {
        // Every member the C++ initialiser list leaves at another value than Zone::default()
        // is overwritten by ExportSetting( *this, false ) below (fill mode, clearance, min
        // thickness, hatch and thieving settings, thermal values, pad connection, corner
        // smoothing, keepout flags, lock, island removal, border style / pitch).
        let mut zone = Zone::new(kiid_generator);

        if in_footprint {
            zone.is_rule_area = true; // Zones living in footprints have the rule area option
        }

        settings.export_setting(&mut zone, false);

        zone.fill.need_refill = false; // True only after edits.
        zone
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

    /// The parent footprint's transform (`GetParentFootprint()->GetTransform()`), `None` for
    /// a zone that is not in a footprint.
    pub(crate) fn parent_transform(&self, ctx: &BoardCtx<'_>) -> Option<TransformTrs> {
        board_item::parent_footprint(self, ctx).map(|fp| fp.transform())
    }

    /// The parent footprint's transform as a mutator sees it.
    fn env_transform(env: &MutEnv) -> Option<TransformTrs> {
        env.parent.map(|frame| frame.transform)
    }

    /// Port of `ZONE::GetBoardOutline()`: the outline in the board frame (the library
    /// outline with the parent footprint transform applied to every vertex, holes included).
    pub fn board_outline(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        let mut poly = self.lib_outline.clone_drop_triangulation();

        if let Some(xform) = self.parent_transform(ctx) {
            let mut it = poly.iterate_with_holes();
            let mut moves: Vec<(VertexIndex, Vector2I)> = Vec::new();

            while it.is_valid() {
                moves.push((it.get_index(), xform.apply(it.get())));
                it.advance();
            }

            for (index, pos) in moves {
                poly.set_vertex_index(&index, pos);
            }
        }

        poly
    }

    /// `Outline()`: the stored outline for editing. As in C++ this is `m_Poly`, i.e. the
    /// library frame for a footprint zone (equal to the board frame for a board zone).
    pub fn outline_mut(&mut self, env: &MutEnv) -> &mut ShapePolySet {
        let _ = env;
        &mut self.lib_outline
    }

    /// `Outline()` (const): the stored outline, see [`Zone::outline_mut`].
    pub fn outline(&self) -> &ShapePolySet {
        &self.lib_outline
    }

    /// Port of `ZONE::GetNumCorners()`: `m_Poly->TotalVertices()`.
    pub fn num_corners(&self) -> i32 {
        self.lib_outline.total_vertices()
    }

    /// Port of `ZONE::GetCornerPosition( aCornerIndex )` (stored frame, like C++). `None`
    /// where C++ throws `std::out_of_range`.
    pub fn corner_position(&self, corner_index: i32) -> Option<Vector2I> {
        let index = self.lib_outline.get_relative_indices(corner_index)?;
        Some(self.lib_outline.c_vertex_index(index))
    }

    /// Port of `ZONE::RemoveAllContours()`.
    pub fn remove_all_contours(&mut self) {
        self.lib_outline.remove_all_contours();
    }

    /// Port of `ZONE::NewHole()`: `m_Poly->NewHole()` (in the last outline).
    pub fn new_hole(&mut self) {
        self.lib_outline.new_hole(-1);
    }

    /// Port of `ZONE::AppendCorner( aPosition, aHoleIdx, aAllowDuplication )`: false when
    /// `hole_idx` names a hole that does not exist.
    pub fn append_corner(
        &mut self,
        position: Vector2I,
        hole_idx: i32,
        allow_duplication: bool,
    ) -> bool {
        // Ensure the main outline exists:
        if self.lib_outline.outline_count() == 0 {
            self.lib_outline.new_outline();
        }

        // If aHoleIdx >= 0, the corner musty be added to the hole, index aHoleIdx.
        // (remember: the index of the first hole is 0)
        // Return error if it does not exist.
        if hole_idx >= self.lib_outline.hole_count(0) {
            return false;
        }

        self.lib_outline
            .append(position.x, position.y, -1, hole_idx, allow_duplication);

        self.fill.need_refill = true;

        true
    }

    /// Port of `ZONE::AddPolygon( const SHAPE_LINE_CHAIN& )`: the first polygon becomes the
    /// main outline, later ones holes in it.
    pub fn add_polygon(&mut self, polygon: &ShapeLineChain) {
        debug_assert!(polygon.is_closed());

        // Add the outline as a new polygon in the polygon set
        if self.lib_outline.outline_count() == 0 {
            self.lib_outline.add_outline(polygon);
        } else {
            self.lib_outline.add_hole(polygon, -1);
        }

        self.fill.need_refill = true;
    }

    /// Port of `ZONE::AddPolygon( std::vector<VECTOR2I>& )`.
    pub fn add_polygon_points(&mut self, polygon: &[Vector2I]) {
        if polygon.is_empty() {
            return;
        }

        let mut outline = ShapeLineChain::new();

        // Create an outline and populate it with the points of aPolygon
        for pt in polygon {
            outline.append_point(*pt, false);
        }

        outline.set_closed(true);

        self.add_polygon(&outline);
    }

    /// Port of `ZONE::RemoveCutout( aOutlineIdx, aHoleIdx )`.
    pub fn remove_cutout(&mut self, outline_idx: i32, hole_idx: i32) {
        // Ensure the requested cutout is valid
        if self.lib_outline.outline_count() < outline_idx
            || self.lib_outline.hole_count(outline_idx) < hole_idx
        {
            return;
        }

        let cut_poly = ShapePolySet::from_outline(self.lib_outline.c_hole(outline_idx, hole_idx));

        // Add the cutout back to the zone
        self.lib_outline.boolean_add(&cut_poly);

        self.fill.need_refill = true;
    }

    /// Port of `ZONE::MoveEdge( offset, aEdge )`: moves both ends of edge `edge` (a global
    /// vertex index) by the board-frame `offset`.
    pub fn move_edge(&mut self, env: &MutEnv, offset: Vector2I, edge: i32) {
        if let Some((_, next_corner)) = self.lib_outline.get_neighbour_indexes(edge) {
            let mut lib_offset = offset;

            if let Some(xform) = Self::env_transform(env) {
                lib_offset = xform.inverse_apply(offset) - xform.inverse_apply(Vector2I::new(0, 0));
            }

            let a = self.lib_outline.c_vertex_global(edge) + lib_offset;
            self.lib_outline.set_vertex(edge, a);
            let b = self.lib_outline.c_vertex_global(next_corner) + lib_offset;
            self.lib_outline.set_vertex(next_corner, b);
            self.hatch_border(env);

            self.fill.need_refill = true;
        }
    }

    // ---- fill state (read side) ---------------------------------------------------------

    /// The fill results.
    pub fn fill(&self) -> &ZoneFill {
        &self.fill
    }

    /// The fill results of a detached zone for writing (`SetFilledPolysList` and friends on a
    /// zone that is not on a board, e.g. the DRC solder mask bridge zone). A zone on a board
    /// is written through `Board::zone_fill_mut`.
    pub fn fill_mut(&mut self) -> &mut ZoneFill {
        &mut self.fill
    }

    /// `GetFilledPolysList( aLayer )` (`std::map::at`: panics for a layer without a fill,
    /// where C++ asserts and throws).
    pub fn filled_polys_list(&self, layer: PcbLayerId) -> Arc<ShapePolySet> {
        Arc::clone(&self.fill.filled_polys_list[&layer])
    }

    /// `GetFill( aLayer )`: the fill of `layer`, `None` without one.
    pub fn get_fill(&self, layer: PcbLayerId) -> Option<&ShapePolySet> {
        self.fill.filled_polys_list.get(&layer).map(|p| p.as_ref())
    }

    /// `HasFilledPolysForLayer( aLayer )`.
    pub fn has_filled_polys_for_layer(&self, layer: PcbLayerId) -> bool {
        self.fill.filled_polys_list.contains_key(&layer)
    }

    /// Port of `ZONE::GetFilledLayers()`: the layers that carry a fill entry, in layer
    /// order (`std::map` order).
    pub fn filled_layers(&self) -> Vec<PcbLayerId> {
        self.fill.filled_polys_list.keys().copied().collect()
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

    /// Port of `ZONE::GetHashValue( aLayer )`: the hash of an empty polygon set for a layer
    /// without one.
    pub fn hash_value(&self, layer: PcbLayerId) -> Hash128 {
        match self.fill.filled_polys_hash.get(&layer) {
            None => ShapePolySet::new().get_hash(),
            Some(hash) => *hash,
        }
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

    /// `SetAssignedPriority()`.
    pub fn set_assigned_priority(&mut self, priority: u32) {
        self.priority = priority;
    }

    /// `GetIsRuleArea()`.
    pub fn is_rule_area(&self) -> bool {
        self.is_rule_area
    }

    /// `IsTeardropArea()`.
    pub fn is_teardrop_area(&self) -> bool {
        self.teardrop_type != TeardropType::TdNone
    }

    /// `HasKeepoutParametersSet()`.
    pub fn has_keepout_parameters_set(&self) -> bool {
        self.do_not_allow_tracks
            || self.do_not_allow_vias
            || self.do_not_allow_pads
            || self.do_not_allow_footprints
            || self.do_not_allow_zone_fills
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

    /// `GetThievingOrientation()`.
    pub fn thieving_orientation(&self) -> EdaAngle {
        self.thieving_settings.orientation.angle()
    }

    /// `SetThievingOrientation()`: stored normalised; a change marks the zone for refill.
    pub fn set_thieving_orientation(&mut self, orientation: EdaAngle) {
        let normalized_orientation = EdaOrientation::new(orientation);

        if self.thieving_settings.orientation != normalized_orientation {
            self.fill.need_refill = true;
        }

        self.thieving_settings.orientation = normalized_orientation;
    }

    /// `SetThievingSettings()`: a change marks the zone for refill.
    pub fn set_thieving_settings(&mut self, settings: &ThievingSettings) {
        if self.thieving_settings != *settings {
            self.fill.need_refill = true;
        }

        self.thieving_settings = settings.clone();
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

    /// `SetHatchOrientation()`.
    pub fn set_hatch_orientation(&mut self, orientation: EdaAngle) {
        self.hatch_orientation = EdaOrientation::new(orientation);
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

    /// `GetThermalReliefGap()`.
    pub fn thermal_relief_gap(&self) -> i32 {
        self.thermal_relief_gap
    }

    /// Port of `ZONE::GetThermalReliefGap( PAD* aPad, wxString* aSource )`: (gap, source).
    ///
    /// C++ compares the pad's `std::optional` override with 0, so the zone value is only
    /// used when the pad override is explicitly 0; without an override the pad branch
    /// returns 0 (kept as in C++).
    pub fn thermal_relief_gap_for_pad(
        &self,
        pad: &Pad,
        want_source: bool,
    ) -> (i32, Option<String>) {
        if pad.padstack.thermal_gap(PcbLayerId::FCu) == Some(0) {
            let source = want_source.then(|| "zone".to_string());
            return (self.thermal_relief_gap, source);
        }

        pad.local_thermal_gap_override(want_source)
    }

    /// `GetThermalReliefSpokeWidth()`.
    pub fn thermal_relief_spoke_width(&self) -> i32 {
        self.thermal_relief_spoke_width
    }

    /// The `ZONE::SetNet` / `SetNetCode` override: copper-thieving zones are always
    /// net-less. Returns the net code `BOARD_CONNECTED_ITEM::SetNetCode` must be given for a
    /// requested `net` (callers that set a zone's net apply it first).
    pub fn filter_net_code(&self, net: NetCode) -> NetCode {
        if self.is_copper_thieving() {
            return NetCode::UNCONNECTED;
        }

        net
    }

    /// Port of `ZONE::SetNet( aNetInfo )`: a copper-thieving zone drops the net (C++ stores
    /// a null `m_netinfo`, i.e. `NetCode::ORPHANED`), every other zone stores it as given.
    /// `SetNet` bypasses the `SetNetCode` clamp; importers and `BOARD::MapNets` use it.
    pub fn set_net(&mut self, net: NetCode) {
        let net = if self.is_copper_thieving() {
            NetCode::ORPHANED
        } else {
            net
        };

        crate::board_connected_item::set_net(self, net)
    }

    // ---- setters with side effects ------------------------------------------------------

    /// Port of `ZONE::SetFillMode()`. Switching to copper thieving clears the net
    /// (`BOARD_CONNECTED_ITEM::SetNetCode( 0, true )`: net code 0 with or without a board)
    /// and clamps the layer set to the first layer.
    pub fn set_fill_mode(&mut self, fill_mode: ZoneFillMode) {
        if self.fill_mode == fill_mode {
            return;
        }

        self.fill.need_refill = true;
        self.fill_mode = fill_mode;

        // Thieving zones are net-less and single-layer; clamp on transition so a
        // multi-layer netted POLYGONS zone converted via the property panel or a
        // load-time SetLayerSet-then-SetFillMode sequence cannot keep stale state.
        if self.fill_mode == ZoneFillMode::CopperThieving {
            self.base.net = NetCode::UNCONNECTED;
            let first = self.first_layer();
            self.set_layer_set(&MutEnv::detached(), Lset::from_layers(&[first]));
        }
    }

    /// Port of `ZONE::SetMinThickness()`: also raises the hatch thickness / gap and the
    /// thieving line width to at least `min_thickness`, and marks the zone for refill.
    pub fn set_min_thickness(&mut self, min_thickness: i32) {
        self.zone_min_thickness = min_thickness;
        self.hatch_thickness = self.hatch_thickness.max(min_thickness);
        self.hatch_gap = self.hatch_gap.max(min_thickness);
        self.thieving_settings.line_width = self.thieving_settings.line_width.max(min_thickness);
        self.fill.need_refill = true;
    }

    /// Port of `ZONE::SetCornerRadius()`.
    pub fn set_corner_radius(&mut self, radius: u32) {
        if self.corner_radius != radius {
            self.fill.need_refill = true;
        }

        self.corner_radius = radius;
    }

    /// Port of `ZONE::SetBorderDisplayStyle( aStyle, aPitch, aRebuildBorderHatch )`: the
    /// pitch is clamped to `ZONE_BORDER_HATCH_MINDIST_MM .. ZONE_BORDER_HATCH_MAXDIST_MM`.
    pub fn set_border_display_style(
        &mut self,
        env: &MutEnv,
        style: ZoneBorderDisplayStyle,
        pitch: i32,
        rebuild_border_hatch: bool,
    ) {
        let mut pitch = pitch.max(PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_MINDIST_MM));
        pitch = pitch.min(PCB_IU_SCALE.mm_to_iu(ZONE_BORDER_HATCH_MAXDIST_MM));
        self.border_hatch_pitch = pitch;
        self.border_style = style;

        if rebuild_border_hatch {
            self.hatch_border(env);
        }
    }

    /// Port of `ZONE::UnHatchBorder()`.
    pub fn unhatch_border(&mut self) {
        self.border_hatch_lines.clear();
    }

    /// Port of `ZONE::HatchBorder()`: rebuild `border_hatch_lines` from the stored outline
    /// (`m_Poly`, library frame for a footprint zone; [`Zone::hatch_lines`] returns them in
    /// the board frame).
    pub fn hatch_border(&mut self, env: &MutEnv) {
        let _ = env;
        self.unhatch_border();

        if self.border_style == ZoneBorderDisplayStyle::NoHatch
            || self.border_hatch_pitch == 0
            || self.lib_outline.is_empty()
        {
            return;
        }

        // set the "length" of hatch lines (the length on horizontal axis)
        let mut hatch_line_len = self.border_hatch_pitch; // OK for DIAGONAL_EDGE style

        // Calculate spacing between 2 hatch lines
        let mut spacing = self.border_hatch_pitch; // OK for DIAGONAL_EDGE style

        if self.border_style == ZoneBorderDisplayStyle::DiagonalFull {
            // The spacing is twice the spacing for DIAGONAL_EDGE because one
            // full diagonal replaces 2 edge diagonal hatch segments in code
            spacing = self.border_hatch_pitch * 2;
            hatch_line_len = -1; // Use full diagonal hatch line
        }

        // To have a better look, give a slope depending on the layer
        let layer = self.first_layer().0;
        let slopes: Vec<f64> = if self.is_teardrop_area() {
            vec![0.7, -0.7]
        } else if layer & 1 != 0 {
            vec![1.0]
        } else {
            vec![-1.0]
        };

        self.border_hatch_lines =
            self.lib_outline
                .generate_hatch_lines(&slopes, spacing, hatch_line_len);
    }

    /// Port of `ZONE::GetHatchLines()`: the border hatch lines in the board frame.
    pub fn hatch_lines(&self, ctx: &BoardCtx<'_>) -> Vec<Seg> {
        let Some(xform) = self.parent_transform(ctx) else {
            return self.border_hatch_lines.clone();
        };

        self.border_hatch_lines
            .iter()
            .map(|seg| Seg::new(xform.apply(seg.a), xform.apply(seg.b)))
            .collect()
    }

    /// Port of `ZONE::UnFill()`: true if there was a fill. Every fill entry is replaced by
    /// a new empty set (holders of the old `Arc` keep their snapshot, like the C++
    /// `shared_ptr` replacement).
    pub fn unfill(&mut self) -> bool {
        self.fill.unfill_locked()
    }

    /// Port of `ZONE::CalculateFilledArea()`.
    pub fn calculate_filled_area(&mut self) -> f64 {
        self.area = 0.0;

        for poly in self.fill.filled_polys_list.values() {
            self.area += poly.area();
        }

        self.area
    }

    /// Port of `ZONE::CalculateOutlineArea()`: the area of the stored outline (`m_Poly`,
    /// library frame for a footprint zone, as in C++).
    pub fn calculate_outline_area(&mut self, ctx: &BoardCtx<'_>) -> f64 {
        let _ = ctx;
        self.outline_area = self.lib_outline.area().abs();
        self.outline_area
    }

    /// `GetFilledArea()`.
    pub fn filled_area(&self) -> f64 {
        self.area
    }

    /// `GetOutlineArea()`.
    pub fn outline_area(&self) -> f64 {
        self.outline_area
    }

    /// Port of `ZONE::GetFirstLayer()`: the first layer in UI order (a zone on a rescue layer
    /// outside the UI order: the first layer of the set), `UNDEFINED_LAYER` for no layer.
    pub fn first_layer(&self) -> PcbLayerId {
        if self.layer_set.count() == 0 {
            return PcbLayerId::UndefinedLayer;
        }

        let ui_layers = self.layer_set.ui_order();

        // This can't use m_layerSet.count() because it's possible to have a zone on
        // a rescue layer that is not in the UI order.
        if let Some(first) = ui_layers.first() {
            return *first;
        }

        // If it's not in the UI set at all, just return the first layer in the set.
        // (we know the count > 0)
        self.layer_set.seq()[0]
    }

    /// Port of `ZONE::HigherPriority( aOther )`: teardrops first, then the priority, then
    /// the UUID.
    pub fn higher_priority(&self, other: &Zone) -> bool {
        // Teardrops are always higher priority than regular zones, so if one zone is a teardrop
        // and the other is not, then return higher priority as the teardrop
        if (self.teardrop_type == TeardropType::TdNone)
            ^ (other.teardrop_type == TeardropType::TdNone)
        {
            return (self.teardrop_type as i32) > (other.teardrop_type as i32);
        }

        if self.priority != other.priority {
            return self.priority > other.priority;
        }

        self.base.item.uuid > other.base.item.uuid
    }

    /// Port of `ZONE::SameNet( aOther )`.
    pub fn same_net(&self, other: &Zone) -> bool {
        self.base.net == other.base.net
    }

    /// Port of `ZONE::IsConflicting()`: `HasFlag( COURTYARD_CONFLICT )`.
    pub fn is_conflicting(&self, ctx: &BoardCtx<'_>) -> bool {
        let _ = ctx;
        self.base.item.flags.has(ItemFlags::COURTYARD_CONFLICT)
    }

    /// Port of `ZONE::SetLayerSetAndRemoveUnusedFills()`. The C++ loop only visits layers of
    /// the new set and resets those not in it, i.e. none: only the layer set changes.
    pub fn set_layer_set_and_remove_unused_fills(&mut self, layers: Lset) {
        if layers.count() == 0 {
            return;
        }

        self.layer_set = layers;
    }

    /// Port of `ZONE::GetInteractingZones( aLayer, aSameNetCollidingZones,
    /// aOtherNetIntersectingZones )`: the board zones (not footprint zones, `BOARD::Zones()`)
    /// on `layer` whose bounding box touches this one's (inflated by 1 um), excluding rule
    /// and teardrop areas; same-net zones only if their outlines collide. Empty without a
    /// board.
    pub fn interacting_zones<'a>(
        &self,
        ctx: &BoardCtx<'a>,
        layer: PcbLayerId,
    ) -> (Vec<(ZoneId, &'a Zone)>, Vec<(ZoneId, &'a Zone)>) {
        let mut same_net_colliding = Vec::new();
        let mut other_net_intersecting = Vec::new();

        if !board_item::has_board(self, ctx) {
            return (same_net_colliding, other_net_intersecting);
        }

        let board: &'a Board = ctx.board;
        let epsilon = PCB_IU_SCALE.mm_to_iu(0.001);
        let mut bbox = self.bounding_box(ctx);

        bbox.inflate(epsilon);

        let self_in_fp = self.parent_transform(ctx).is_some();

        for (candidate_id, candidate) in board.zones() {
            if self.base.item.id() == Some(candidate_id.into()) {
                continue;
            }

            if !candidate.layer_set.test(layer) {
                continue;
            }

            if candidate.is_rule_area() || candidate.is_teardrop_area() {
                continue;
            }

            if !candidate.bounding_box(ctx).intersects(&bbox) {
                continue;
            }

            if candidate.base.net == self.base.net {
                // GetBoardOutline() is expensive.  Don't use it in Zone Filler unless we have to.
                let self_storage;
                let self_outline = if self_in_fp {
                    self_storage = self.board_outline(ctx);
                    &self_storage
                } else {
                    &self.lib_outline
                };

                let candidate_storage;
                let candidate_outline = if candidate.parent_transform(ctx).is_some() {
                    candidate_storage = candidate.board_outline(ctx);
                    &candidate_storage
                } else {
                    &candidate.lib_outline
                };

                if self_outline.collide_shape(candidate_outline, 0, None, None) {
                    same_net_colliding.push((candidate_id, candidate));
                }
            } else {
                other_net_intersecting.push((candidate_id, candidate));
            }
        }

        (same_net_colliding, other_net_intersecting)
    }

    // ---- virtual overrides --------------------------------------------------------------

    /// Port of `ZONE::GetLayer()`: the layer of a single-layer zone, `UNDEFINED_LAYER`
    /// otherwise.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        let _ = ctx;

        if self.layer_set.count() == 1 {
            return self.first_layer();
        }

        PcbLayerId::UndefinedLayer
    }

    /// Port of `ZONE::GetLayerSet()`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        let _ = ctx;
        self.layer_set.clone()
    }

    /// Port of `ZONE::IsOnLayer()`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let _ = ctx;
        self.layer_set.test(layer)
    }

    /// Port of `ZONE::IsOnCopperLayer()`.
    pub fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        let _ = ctx;
        (self.layer_set.clone() & Lset::all_cu_mask()).count() > 0
    }

    /// Port of `ZONE::GetPosition()`: the first corner of the stored outline (library frame
    /// for a footprint zone, as in C++), (0, 0) for an empty outline.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        let _ = ctx;

        if self.lib_outline.outline_count() == 0 || self.lib_outline.total_vertices() == 0 {
            return Vector2I::new(0, 0);
        }

        self.corner_position(0).unwrap_or_default()
    }

    /// `ZONE::IsConnected()`: `!GetIsRuleArea()`.
    pub fn is_connected(&self, _ctx: &BoardCtx<'_>) -> bool {
        !self.is_rule_area
    }

    /// Port of `ZONE::GetLocalClearance()` (no source argument): always a value, 0 for a
    /// rule area, else `m_ZoneClearance`.
    pub fn local_clearance(&self) -> Option<i32> {
        Some(if self.is_rule_area {
            0
        } else {
            self.zone_clearance
        })
    }

    /// Port of `ZONE::GetLocalClearance( wxString* aSource )`: `None` for a rule area, else
    /// (`m_ZoneClearance`, "zone" when asked).
    pub fn local_clearance_with_source(&self, want_source: bool) -> Option<(i32, Option<String>)> {
        if self.is_rule_area {
            return None;
        }

        let source = want_source.then(|| "zone".to_string());
        Some((self.local_clearance().unwrap_or(0), source))
    }

    /// Port of `ZONE::GetFriendlyName()` (English).
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        if self.is_rule_area() {
            "Rule Area".to_string()
        } else if self.is_teardrop_area() {
            "Teardrop Area".to_string()
        } else if self.is_on_copper_layer(ctx) {
            "Copper Zone".to_string()
        } else {
            "Non-copper Zone".to_string()
        }
    }

    /// Port of `ZONE::GetItemDescription()` (English).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        let _ = (units, full);
        let layers = self.layer_set.seq();
        let has_board = board_item::has_board(self, ctx);

        let layer_name = |layer: PcbLayerId| -> String {
            if has_board {
                return ctx.board.layer_name(layer);
            }

            Board::standard_layer_name(layer)
        };

        let layer_desc = match layers.len() {
            0 => String::new(),
            1 => format!("on {}", layer_name(layers[0])),
            2 => format!("on {} and {}", layer_name(layers[0]), layer_name(layers[1])),
            3 => format!(
                "on {}, {} and {}",
                layer_name(layers[0]),
                layer_name(layers[1]),
                layer_name(layers[2])
            ),
            n => format!(
                "on {}, {} and {} more",
                layer_name(layers[0]),
                layer_name(layers[1]),
                n - 2
            ),
        };

        if self.is_rule_area() {
            if self.zone_name.is_empty() {
                format!("Rule Area {layer_desc}")
            } else {
                format!("Rule area '{}' {}", self.zone_name, layer_desc)
            }
        } else if self.is_teardrop_area() {
            format!(
                "Teardrop {} {}",
                board_connected_item::netname_msg(self, ctx),
                layer_desc
            )
        } else if self.zone_name.is_empty() {
            // "%d" of an unsigned priority
            format!(
                "Zone {} {}, priority {}",
                board_connected_item::netname_msg(self, ctx),
                layer_desc,
                self.priority as i32
            )
        } else {
            format!(
                "Zone '{}' {} {}, priority {}",
                self.zone_name,
                board_connected_item::netname_msg(self, ctx),
                layer_desc,
                self.priority as i32
            )
        }
    }

    /// `ZONE::SetPosition()`: does nothing.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        let _ = (env, pos);
    }

    /// Port of `ZONE::Move()`: the outline moves by the offset expressed in its own frame,
    /// the hatch lines are translated (not regenerated), the fills and the cached bounding
    /// box move by the board-frame offset.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let mut outline_offset = delta;

        if let Some(xform) = Self::env_transform(env) {
            outline_offset = xform.inverse_apply(delta) - xform.inverse_apply(Vector2I::new(0, 0));
        }

        self.lib_outline.move_(outline_offset);

        // Translate existing hatch lines instead of regenerating them. HatchBorder() is expensive
        // (O(n*m) segment intersections + point-in-polygon tests) and the hatch pattern is
        // invariant under translation.
        for seg in &mut self.border_hatch_lines {
            seg.a += outline_offset;
            seg.b += outline_offset;
        }

        /* move fills */
        for poly in self.fill.filled_polys_list.values_mut() {
            Arc::make_mut(poly).move_(delta);
        }

        // move boundingbox cache
        //
        // While the cache will get nuked at the conclusion of the operation, we use it for some
        // things (such as drawing the parent group) during the move.
        self.bbox_cache.update(|(bbox, _)| bbox.move_(delta));
    }

    /// Port of `ZONE::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut outline_centre = centre;

        if let Some(xform) = Self::env_transform(env) {
            outline_centre = xform.inverse_apply(centre);
        }

        self.lib_outline.rotate(angle, outline_centre);
        self.bbox_cache.clear();
        self.hatch_border(env);

        /* rotate filled areas: */
        for poly in self.fill.filled_polys_list.values_mut() {
            Arc::make_mut(poly).rotate(angle, centre);
        }
    }

    /// Port of `ZONE::Flip()`: mirror, then move every layer (layer set, per-layer
    /// properties and fills) to its flipped layer.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror(env, centre, dir);

        let fills_copy: BTreeMap<PcbLayerId, ShapePolySet> = self
            .fill
            .filled_polys_list
            .iter()
            .map(|(layer, poly)| (*layer, poly.as_ref().clone()))
            .collect();

        let layer_properties_copy = self.layer_properties.clone();

        let mut flipped = Lset::new();

        for layer in self.layer_set.seq().iter().copied() {
            flipped.set(env.layers.flip_layer(layer));
        }

        self.set_layer_set(env, flipped);

        for (old_layer, properties) in layer_properties_copy {
            let new_layer = env.layers.flip_layer(old_layer);
            self.layer_properties.insert(new_layer, properties);
        }

        for (old_layer, shape) in &fills_copy {
            let new_layer = env.layers.flip_layer(*old_layer);
            self.fill.set_filled_polys_list(new_layer, shape);
        }
    }

    /// Port of `ZONE::Mirror( aMirrorRef, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(env, &board_item::mirror_axis(centre, dir));
    }

    /// Port of `ZONE::Mirror( const SEG& aAxis )`.
    pub fn mirror_seg(&mut self, env: &MutEnv, axis: &Seg) {
        let axis_angle = -EdaAngle::from_vector(axis.b - axis.a);

        self.set_hatch_orientation(self.hatch_orientation().reflect(axis_angle).normalized());
        self.set_thieving_orientation(self.thieving_orientation().reflect(axis_angle).normalized());

        if let Some(xform) = Self::env_transform(env) {
            // Footprints are only ever flipped, never mirrored, so this is reached only from Flip(),
            // which always passes a cardinal axis.
            // (Handy, because otherwise we have to handle the footprint transform!)
            let flip_direction = if axis.a.x == axis.b.x {
                FlipDirection::LeftRight
            } else {
                FlipDirection::TopBottom
            };

            let flip_axis_start = xform.inverse_apply(axis.a);
            self.lib_outline.mirror(flip_axis_start, flip_direction);
        } else {
            self.lib_outline.mirror_seg(axis);
        }

        self.bbox_cache.clear();

        self.hatch_border(env);

        for poly in self.fill.filled_polys_list.values_mut() {
            Arc::make_mut(poly).mirror_seg(axis);
        }
    }

    /// Port of `ZONE::SetLayer()`: `SetLayerSet( LSET( { aLayer } ) )`.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        self.set_layer_set(env, Lset::from_layers(&[layer]));
    }

    /// Port of `ZONE::SetLayerSet()`: ignores an empty set, clamps thieving zones to one
    /// layer, clears the fills of a changed set (one empty fill entry per new layer) and drops
    /// the per-layer properties of layers no longer in the set.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        let _ = env;

        if layers.count() == 0 {
            return;
        }

        // Thieving zones are single-layer; clamp here so direct callers cannot violate
        // the invariant.  UIOrder().front() matches GetFirstLayer().
        let effective_set = if self.is_copper_thieving() && layers.count() > 1 {
            Lset::from_layers(&[layers.ui_order()[0]])
        } else {
            layers
        };

        if self.layer_set != effective_set {
            self.fill.need_refill = true;

            self.fill.unfill_locked();

            self.fill.filled_polys_list.clear();
            self.fill.filled_polys_hash.clear();
            self.fill.insulated_islands.clear();

            let fill = &mut self.fill;
            effective_set.run_on_layers(|layer| {
                fill.filled_polys_list
                    .insert(layer, Arc::new(ShapePolySet::new()));
                fill.filled_polys_hash.insert(layer, Hash128::default());
                fill.insulated_islands.insert(layer, BTreeSet::new());
            });

            self.layer_properties
                .retain(|layer, _| effective_set.contains(*layer));
        }

        self.layer_set = effective_set;
    }

    /// Port of `ZONE::OnFootprintTransformed()`: footprint zone bounding boxes are derived
    /// through the parent transform; the fill is dropped.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        let _ = env;
        self.bbox_cache.clear();
        self.fill.need_refill = true;
        self.unfill();
    }

    /// Port of `ZONE::OnFootprintRescaled()`: the outline auto-derives from the library
    /// storage; only the fill is invalidated.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        let _ = (env, linear_factor, anchor, parent_rotate);

        if ratio_x == 1.0 && ratio_y == 1.0 {
            return;
        }

        // Zone outline auto-derives from lib storage through the parent transform.
        // Just invalidate the fill since geometry-on-screen has changed.
        self.bbox_cache.clear();
        self.fill.need_refill = true;
        self.unfill();
    }

    /// Port of `ZONE::operator==( const BOARD_ITEM& )`: rule-area flags, or priority / min
    /// thickness / corner smoothing / teardrop parameters, then the stored corners.
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Zone) -> bool {
        let _ = ctx;

        if self.is_rule_area() != other.is_rule_area() {
            return false;
        }

        if self.is_rule_area() {
            if self.do_not_allow_zone_fills != other.do_not_allow_zone_fills
                || self.do_not_allow_tracks != other.do_not_allow_tracks
                || self.do_not_allow_vias != other.do_not_allow_vias
                || self.do_not_allow_footprints != other.do_not_allow_footprints
                || self.do_not_allow_pads != other.do_not_allow_pads
                || self.placement_area_enabled != other.placement_area_enabled
                || self.placement_area_source_type != other.placement_area_source_type
                || self.placement_area_source != other.placement_area_source
            {
                return false;
            }
        } else if self.priority != other.priority
            || self.zone_min_thickness != other.zone_min_thickness
            || self.corner_smoothing_type != other.corner_smoothing_type
            || self.corner_radius != other.corner_radius
            || self.base.teardrop_params != other.base.teardrop_params
        {
            return false;
        }

        if self.num_corners() != other.num_corners() {
            return false;
        }

        for ii in 0..self.num_corners() {
            if self.corner_position(ii) != other.corner_position(ii) {
                return false;
            }
        }

        true
    }

    /// Port of `ZONE::CacheOutlineTriangulation()`.
    pub fn cache_outline_triangulation(&mut self) {
        self.lib_outline.cache_triangulation(false);
    }

    /// Port of `ZONE::CacheTriangulation( aLayer )`: `UNDEFINED_LAYER` triangulates every
    /// fill and the outline.
    pub fn cache_triangulation(&mut self, layer: PcbLayerId) {
        self.fill.cache_triangulation(layer);

        if layer == PcbLayerId::UndefinedLayer {
            self.lib_outline.cache_triangulation(false);
        }
    }

    /// Port of `ZONE::CacheBoundingBox()`: preload the bounding-box cache for the current
    /// board time stamp (no-op without a board, like C++).
    pub fn cache_bounding_box(&self, ctx: &BoardCtx<'_>) {
        if board_item::has_board(self, ctx) {
            let bbox = self.compute_bounding_box(ctx);
            self.bbox_cache.store((bbox, ctx.board.time_stamp()));
        }
    }
}

// ---- fill writers (derived state, board-model.md 8.4) ------------------------------------

impl ZoneFill {
    /// `ZONE::SetFilledPolysList( aLayer, aPolysList )`: installs a copy.
    pub fn set_filled_polys_list(&mut self, layer: PcbLayerId, polys: &ShapePolySet) {
        self.filled_polys_list
            .insert(layer, Arc::new(polys.clone()));
    }

    /// `ZONE::MoveFilledPolysList( aLayer, aPolysList )` equivalent: installs `polys`
    /// without copying.
    pub fn move_filled_polys_list(&mut self, layer: PcbLayerId, polys: ShapePolySet) {
        self.filled_polys_list.insert(layer, Arc::new(polys));
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

    /// Port of `ZONE::unFillLocked()`: every fill entry is replaced by a new empty set, the
    /// island sets of those layers are emptied, `is_filled` and the fill flags are reset.
    /// True if any fill was not empty. Public for the zone filler, which reaches a stored
    /// zone's fill through `Board::zone_fill_mut` (`ZONE::UnFill()` on it).
    pub fn unfill_locked(&mut self) -> bool {
        let mut change = false;

        for (layer, poly) in self.filled_polys_list.iter_mut() {
            change |= !poly.is_empty();
            self.insulated_islands.entry(*layer).or_default().clear();

            // Replace the shared_ptr with a new empty object rather than clearing the existing one.
            // This ensures that any CN_ZONE_LAYERs holding shared_ptr copies still have valid data
            // (the old, now orphaned SHAPE_POLY_SET) while we get a fresh container.
            *poly = Arc::new(ShapePolySet::new());
        }

        self.is_filled = false;
        self.fill_flags = Lset::new();

        change
    }

    /// Port of `ZONE::BuildHashValue( aLayer )`: the hash of the layer's fill (of an empty
    /// set for a layer without one).
    pub fn build_hash_value(&mut self, layer: PcbLayerId) {
        let hash = match self.filled_polys_list.get(&layer) {
            None => ShapePolySet::new().get_hash(),
            Some(poly) => poly.get_hash(),
        };

        self.filled_polys_hash.insert(layer, hash);
    }

    /// Port of the fill part of `ZONE::CacheTriangulation( aLayer )` (`UNDEFINED_LAYER`:
    /// every layer); [`Zone::cache_triangulation`] also triangulates the outline. A fill
    /// shared with another holder is copied first (`Arc::make_mut`).
    pub fn cache_triangulation(&mut self, layer: PcbLayerId) {
        if layer == PcbLayerId::UndefinedLayer {
            for poly in self.filled_polys_list.values_mut() {
                Arc::make_mut(poly).cache_triangulation(false);
            }
        } else if let Some(poly) = self.filled_polys_list.get_mut(&layer) {
            Arc::make_mut(poly).cache_triangulation(false);
        }
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
    fn set_net(&mut self, net: NetCode) {
        Zone::set_net(self, net)
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

    // ---- qa/tests/pcbnew/test_zone.cpp ----------------------------------------------------

    use crate::board::AddMode;
    use crate::padstack::{Padstack, PadstackMode};
    use crate::pcb_track::{PcbVia, Track};
    use crate::zone_utils::{auto_assign_zone_priorities, merge_zones_with_same_outline};
    use kicad_geom::Box2I;

    /// `ZONE( &m_board )`: the board's default zone settings.
    fn board_zone(board: &mut Board) -> Zone {
        let settings = board.design_settings().default_zone_settings().clone();
        let mut zone = Zone::new_with_settings(&mut board.kiid_generator, &settings, false);
        zone.base.item.parent = crate::item::ParentRef::Board;
        zone
    }

    /// `KIGEOM::BoxToLineChain( BOX2I( aOrigin, aSize ) )` as a polygon set.
    fn square(origin: Vector2I, size: Vector2I) -> ShapePolySet {
        let b = Box2I::new(origin, size);
        let mut chain = ShapeLineChain::new();
        chain.append_point(Vector2I::new(b.left(), b.top()), false);
        chain.append_point(Vector2I::new(b.right(), b.top()), false);
        chain.append_point(Vector2I::new(b.right(), b.bottom()), false);
        chain.append_point(Vector2I::new(b.left(), b.bottom()), false);
        chain.set_closed(true);
        ShapePolySet::from_outline(&chain)
    }

    /// `CreateSquareZone( m_board, aLayer, aOrigin, aSize )`.
    fn create_square_zone(
        board: &mut Board,
        layer: PcbLayerId,
        origin: Vector2I,
        size: Vector2I,
    ) -> Zone {
        let mut zone = board_zone(board);
        zone.set_layer(&MutEnv::detached(), layer);
        *zone.lib_outline_mut() = square(origin, size);
        zone
    }

    /// `CreateSimilarZone( m_board, aLayer, aOther )`.
    fn create_similar_zone(board: &mut Board, layer: PcbLayerId, other: &Zone) -> Zone {
        let mut zone = board_zone(board);
        zone.set_layer(&MutEnv::detached(), layer);
        *zone.lib_outline_mut() = other.outline().clone();
        zone
    }

    fn mm(v: f64) -> i32 {
        PCB_IU_SCALE.mm_to_iu(v)
    }

    /// `AddVia( aBoard, aPos, aNetCode, aTopLayer, aBotLayer )`.
    fn add_via(
        board: &mut Board,
        pos: Vector2I,
        net: NetCode,
        top: PcbLayerId,
        bottom: PcbLayerId,
    ) {
        let env = board.child_env(crate::item::ParentRef::Board);
        let mut via = PcbVia::default();
        via.set_padstack_mode(PadstackMode::Normal);
        via.set_position(&env, pos);
        via.set_layer_pair(&env, top, bottom);
        via.set_width(Padstack::ALL_LAYERS, mm(0.6));
        via.set_drill(mm(0.3));
        via.track.base.net = net;
        board
            .add_track(Track::Via(Box::new(via)), AddMode::Insert)
            .unwrap_or_else(|_| panic!("via refused"));
    }

    /// Board with nets "NetA" / "NetB" (or the given names).
    fn board_with_nets(names: &[&str]) -> (Board, Vec<NetCode>) {
        let mut board = Board::new();
        let codes = names
            .iter()
            .map(|n| board.find_or_add_net(n, None))
            .collect();
        (board, codes)
    }

    #[test]
    fn single_layer() {
        let mut board = Board::new();
        let mut zone = board_zone(&mut board);
        zone.set_layer(&MutEnv::detached(), PcbLayerId::FCu);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(zone.layer(&ctx), PcbLayerId::FCu);
        assert_eq!(zone.layer(&ctx), zone.first_layer());
        assert!(zone.is_on_copper_layer(&ctx));
    }

    #[test]
    fn multiple_layers() {
        let mut board = Board::new();
        let mut zone = board_zone(&mut board);
        zone.set_layer_set(
            &MutEnv::detached(),
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]),
        );
        let ctx = BoardCtx::bare(&board);

        // There is no "the" layer in a multi-layer zone
        assert_eq!(zone.layer(&ctx), PcbLayerId::UndefinedLayer);
        // ... but there is a first layer
        assert_eq!(zone.first_layer(), PcbLayerId::FCu);
        assert!(zone.is_on_copper_layer(&ctx));
    }

    /// https://gitlab.com/kicad/code/kicad/-/issues/18553
    #[test]
    fn rescued_layers() {
        let mut board = Board::new();
        let mut zone = board_zone(&mut board);
        zone.set_layer(&MutEnv::detached(), PcbLayerId::Rescue);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(zone.layer(&ctx), PcbLayerId::Rescue);
        assert_eq!(zone.layer(&ctx), zone.first_layer());
        assert!(!zone.is_on_copper_layer(&ctx));
    }

    /// https://gitlab.com/kicad/code/kicad/-/issues/23125
    #[test]
    fn empty_zone_get_position() {
        let mut board = Board::new();
        let mut zone = board_zone(&mut board);
        zone.set_layer(&MutEnv::detached(), PcbLayerId::FCu);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(zone.num_corners(), 0);
        assert_eq!(zone.position(&ctx), Vector2I::new(0, 0));
    }

    /// `test_board_item.cpp` `BoardlessItemsUseStandardLayerMetadata` (zone part): `GetInteractingZones` of a
    /// zone without a board (C++: its parent footprint has none) returns nothing.
    #[test]
    fn boardless_zone_interacting_zones() {
        let board = Board::new();
        let mut kiids = KiidGenerator::default();
        let zone = Zone::new(&mut kiids);
        let ctx = BoardCtx::bare(&board);
        let (same, other) = zone.interacting_zones(&ctx, PcbLayerId::FCu);

        assert!(same.is_empty());
        assert!(other.is_empty());
    }

    /// `CircleZoneCutoutRoundTrip`: a circular outline with its arcs dropped before the
    /// boolean, as `ZONE_CREATE_HELPER::performZoneCutout` does.
    /// https://gitlab.com/kicad/code/kicad/-/issues/24053
    #[test]
    fn circle_zone_cutout_round_trip() {
        use kicad_geom::geometry::shape_arc::ShapeArc;

        // Build a circular zone outline the same way convert_tool.cpp does for CIRCLE shapes
        let center = Vector2I::new(0, 0);
        let radius = mm(10.0);
        let full_circle = ShapeArc::from_start_mid_end(
            center - Vector2I::new(radius, 0),
            center + Vector2I::new(radius, 0),
            center - Vector2I::new(radius, 0),
            0,
        );

        let mut source_outline = ShapePolySet::new();
        source_outline.new_outline();
        source_outline.append_arc(&full_circle, -1, -1, None);

        assert!(source_outline.arc_count() > 0);
        let source_area = source_outline.area();
        assert!(source_area > 0.0);

        // Draw a small rectangular cutout well inside the circle
        let mut cutout_outline = ShapePolySet::new();
        cutout_outline.new_outline();
        let cutout_half = mm(1.0);
        cutout_outline.append_point(Vector2I::new(-cutout_half, -cutout_half), -1, -1);
        cutout_outline.append_point(Vector2I::new(cutout_half, -cutout_half), -1, -1);
        cutout_outline.append_point(Vector2I::new(cutout_half, cutout_half), -1, -1);
        cutout_outline.append_point(Vector2I::new(-cutout_half, cutout_half), -1, -1);

        // Mirror the fix applied in ZONE_CREATE_HELPER::performZoneCutout: drop arcs before
        // the boolean so the saved outline cannot reference stale arc endpoints.
        let mut working = source_outline.clone();
        working.clear_arcs();

        let mut cutout = cutout_outline.clone();
        cutout.clear_arcs();

        working.boolean_subtract(&cutout);

        assert_eq!(working.arc_count(), 0);
        assert_eq!(working.outline_count(), 1);
        assert_eq!(working.hole_count(0), 1);

        // BOOST_CHECK_CLOSE( .., 0.1 ): relative difference in percent
        let expected_area = source_area - cutout_outline.area();
        let rel = (working.area() - expected_area).abs() / expected_area.abs();
        assert!(rel * 100.0 <= 0.1, "{} vs {expected_area}", working.area());

        // Outline must be a simple closed curve with reasonable point count
        let outer_chain = working.c_outline(0);
        assert!(outer_chain.is_closed());
        assert!(outer_chain.point_count() >= 8);
        assert!(outer_chain.self_intersecting().is_none());

        let hole_chain = working.c_hole(0, 0);
        assert!(hole_chain.is_closed());
        assert_eq!(hole_chain.point_count(), 4);
    }

    #[test]
    fn zone_merge_null() {
        let mut board = Board::new();
        let mut zones = Vec::new();

        for _ in 0..2 {
            let mut zone = board_zone(&mut board);
            zone.set_layer(&MutEnv::detached(), PcbLayerId::FCu);
            zones.push(zone);
        }

        // They are the same, so they do merge
        assert_eq!(merge_zones_with_same_outline(zones).len(), 1);
    }

    #[test]
    fn zone_merge_non_null_no_merge() {
        let mut board = Board::new();
        let zones = vec![
            create_square_zone(
                &mut board,
                PcbLayerId::FCu,
                Vector2I::new(0, 0),
                Vector2I::new(100, 100),
            ),
            create_square_zone(
                &mut board,
                PcbLayerId::BCu,
                Vector2I::new(200, 200),
                Vector2I::new(300, 300),
            ),
        ];

        // They are different, so they don't merge
        assert_eq!(merge_zones_with_same_outline(zones).len(), 2);
    }

    #[test]
    fn zone_merge_non_null_merge() {
        let mut board = Board::new();
        let a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(100, 100),
        );
        let b = create_similar_zone(&mut board, PcbLayerId::BCu, &a);

        let merged = merge_zones_with_same_outline(vec![a, b]);

        // They are the same, so they do merge
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].layer_set,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu])
        );
        assert_eq!(merged[0].num_corners(), 4);
    }

    #[test]
    fn zone_merge_merge_same_geom_different_order() {
        let mut board = Board::new();
        let a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(100, 100),
        );
        let mut b = create_similar_zone(&mut board, PcbLayerId::BCu, &a);

        // Reverse the outline of one of them
        b.lib_outline_mut().outline(0).reverse();

        let merged = merge_zones_with_same_outline(vec![a, b]);

        // They are the same, so they do merge
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].layer_set,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu])
        );
        assert_eq!(merged[0].num_corners(), 4);
    }

    #[test]
    fn auto_priority_non_overlapping() {
        let (mut board, nets) = board_with_nets(&["NetA", "NetB"]);
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        a.base.net = nets[0];
        a.priority = 5;
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(20.0), 0),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.base.net = nets[1];
        b.priority = 10;
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        auto_assign_zone_priorities(&mut board);

        assert!(board[ia].assigned_priority() <= board[ib].assigned_priority());
    }

    #[test]
    fn auto_priority_item_count_wins() {
        let (mut board, nets) = board_with_nets(&["NetA", "NetB"]);
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(20.0), mm(20.0)),
        );
        a.base.net = nets[0];
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(5.0), mm(5.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.base.net = nets[1];
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        for i in 0..5 {
            add_via(
                &mut board,
                Vector2I::new(mm(7.0 + f64::from(i)), mm(10.0)),
                nets[0],
                PcbLayerId::FCu,
                PcbLayerId::BCu,
            );
        }

        add_via(
            &mut board,
            Vector2I::new(mm(10.0), mm(7.0)),
            nets[1],
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        );

        auto_assign_zone_priorities(&mut board);

        assert!(board[ia].assigned_priority() > board[ib].assigned_priority());
    }

    #[test]
    fn auto_priority_similar_counts_smaller_wins() {
        let (mut board, nets) = board_with_nets(&["NetA", "NetB"]);
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(30.0), mm(30.0)),
        );
        a.base.net = nets[0];
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(5.0), mm(5.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.base.net = nets[1];
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        for (x, y, n) in [
            (8.0, 8.0, 0),
            (12.0, 8.0, 0),
            (8.0, 12.0, 1),
            (12.0, 12.0, 1),
        ] {
            add_via(
                &mut board,
                Vector2I::new(mm(x), mm(y)),
                nets[n],
                PcbLayerId::FCu,
                PcbLayerId::BCu,
            );
        }

        auto_assign_zone_priorities(&mut board);

        assert!(board[ib].assigned_priority() > board[ia].assigned_priority());
    }

    #[test]
    fn auto_priority_multi_layer_aggregate() {
        let (mut board, nets) = board_with_nets(&["NetA", "NetB"]);
        board.set_copper_layer_count(2);
        let both = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]);
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(20.0), mm(20.0)),
        );
        a.set_layer_set(&MutEnv::detached(), both.clone());
        a.base.net = nets[0];
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(5.0), mm(5.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.set_layer_set(&MutEnv::detached(), both);
        b.base.net = nets[1];
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        for (x, y, n) in [
            (8.0, 8.0, 0),
            (12.0, 8.0, 0),
            (8.0, 12.0, 1),
            (10.0, 10.0, 1),
            (12.0, 12.0, 1),
        ] {
            add_via(
                &mut board,
                Vector2I::new(mm(x), mm(y)),
                nets[n],
                PcbLayerId::FCu,
                PcbLayerId::BCu,
            );
        }

        auto_assign_zone_priorities(&mut board);

        assert!(board[ib].assigned_priority() > board[ia].assigned_priority());
    }

    #[test]
    fn auto_priority_same_net_equal_priority() {
        let (mut board, nets) = board_with_nets(&["SharedNet"]);
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(30.0), mm(30.0)),
        );
        a.base.net = nets[0];
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(5.0), mm(5.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.base.net = nets[0];
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        add_via(
            &mut board,
            Vector2I::new(mm(8.0), mm(8.0)),
            nets[0],
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        );
        add_via(
            &mut board,
            Vector2I::new(mm(12.0), mm(12.0)),
            nets[0],
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        );

        auto_assign_zone_priorities(&mut board);

        // Same-net overlapping zones are cooperative and must share equal priority
        assert_eq!(board[ia].assigned_priority(), board[ib].assigned_priority());
    }

    #[test]
    fn auto_priority_equal_area_no_change() {
        let (mut board, nets) = board_with_nets(&["NetA", "NetB"]);

        // Two identical-sized overlapping zones with no items in the overlap
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(20.0), mm(20.0)),
        );
        a.base.net = nets[0];
        a.priority = 50;
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(20.0), mm(20.0)),
        );
        b.base.net = nets[1];
        b.priority = 50;
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);

        let changed = auto_assign_zone_priorities(&mut board);

        // Equal areas, no items: no ordering evidence, priorities must not change
        assert!(!changed);
        assert_eq!(board[ia].assigned_priority(), 50);
        assert_eq!(board[ib].assigned_priority(), 50);
    }

    #[test]
    fn auto_priority_same_net_group_inherits_edge() {
        let (mut board, nets) = board_with_nets(&["GND", "VCC"]);

        // Large GND zone (A) overlaps with small VCC zone (C).
        // Small GND zone (B) overlaps with A but NOT with C.
        // A should beat C (more items), and B should inherit A's priority.
        let mut a = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(0, 0),
            Vector2I::new(mm(40.0), mm(40.0)),
        );
        a.base.net = nets[0];
        let mut b = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(25.0), mm(25.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        b.base.net = nets[0];
        let mut c = create_square_zone(
            &mut board,
            PcbLayerId::FCu,
            Vector2I::new(mm(5.0), mm(5.0)),
            Vector2I::new(mm(10.0), mm(10.0)),
        );
        c.base.net = nets[1];
        let ia = board.add_zone(a, AddMode::Insert);
        let ib = board.add_zone(b, AddMode::Insert);
        let ic = board.add_zone(c, AddMode::Insert);

        // GND items in the A/C overlap region
        for i in 0..4 {
            add_via(
                &mut board,
                Vector2I::new(mm(8.0 + f64::from(i) * 2.0), mm(10.0)),
                nets[0],
                PcbLayerId::FCu,
                PcbLayerId::BCu,
            );
        }

        add_via(
            &mut board,
            Vector2I::new(mm(10.0), mm(8.0)),
            nets[1],
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        );

        auto_assign_zone_priorities(&mut board);

        // A beats C because GND has more items in the overlap
        assert!(board[ia].assigned_priority() > board[ic].assigned_priority());

        // B shares A's priority because they are same-net and overlap
        assert_eq!(board[ia].assigned_priority(), board[ib].assigned_priority());
    }

    /// `ZONE::GetLocalClearance()` without a source holds a value for a rule area (0); the
    /// overload with a source is empty then.
    #[test]
    fn local_clearance_overloads() {
        let mut zone = Zone::default();
        zone.zone_clearance = 300_000;
        assert_eq!(zone.local_clearance(), Some(300_000));
        assert_eq!(
            zone.local_clearance_with_source(true),
            Some((300_000, Some("zone".to_string())))
        );
        zone.is_rule_area = true;
        assert_eq!(zone.local_clearance(), Some(0));
        assert_eq!(zone.local_clearance_with_source(true), None);
    }

    /// `SetFillMode( COPPER_THIEVING )` clears the net and clamps the layer set; `SetLayerSet`
    /// keeps a thieving zone on one layer and resets the fills of a changed set.
    #[test]
    fn copper_thieving_and_layer_set() {
        let mut zone = Zone::default();
        let env = MutEnv::detached();
        zone.base.net = NetCode(3);
        zone.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::FCu]));
        assert_eq!(zone.fill.filled_polys_list.len(), 2);
        zone.fill.set_filled_polys_list(
            PcbLayerId::FCu,
            &square(Vector2I::new(0, 0), Vector2I::new(10, 10)),
        );
        zone.fill.set_is_filled(true);
        zone.fill.need_refill = false;

        zone.set_fill_mode(ZoneFillMode::CopperThieving);
        assert!(zone.need_refill());
        assert_eq!(zone.base.net, NetCode::UNCONNECTED);
        assert_eq!(zone.layer_set, Lset::from_layers(&[PcbLayerId::FCu]));
        assert!(!zone.is_filled());
        assert!(zone.fill.filled_polys_list[&PcbLayerId::FCu].is_empty());

        zone.set_layer_set(
            &env,
            Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::In1Cu]),
        );
        assert_eq!(zone.layer_set, Lset::from_layers(&[PcbLayerId::In1Cu]));
        assert_eq!(zone.filter_net_code(NetCode(2)), NetCode::UNCONNECTED);

        // An empty set is ignored.
        zone.set_layer_set(&env, Lset::new());
        assert_eq!(zone.layer_set, Lset::from_layers(&[PcbLayerId::In1Cu]));
    }
}
