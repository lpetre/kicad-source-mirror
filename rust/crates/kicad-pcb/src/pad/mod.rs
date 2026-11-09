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

//! Port of `pcbnew/pad.h` and `pcbnew/pad.cpp` (`PAD`), `board-model-types.md` section 3.
//!
//! This file holds the data, the constructor, the delegation shims (frozen after M0, see
//! `pcb_track/mod.rs` for the pattern), the trivial accessors and the zone-layer-override
//! writers of `Board` (`board-model.md` 8.4). The inherent methods behind the shims live in
//! `derive.rs` (frames, layers, attributes, clearances, transforms: package M14) and
//! `shapes.rs` (effective shapes, polygons, hit tests: package M15).

pub mod derive;
pub mod shapes;

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, LayerRange, Lset, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{
    ANGLE_0, Box2I, EdaAngle, EdaOrientation, FlipDirection, ShapeCompound, ShapePolySet,
    ShapeSegment, Vector2I,
};

use crate::board::Board;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::ids::ItemId;
use crate::item::{BoardItemBase, ConnectedItemBase, SharedShape};
use crate::padstack::{
    PadAttrib, PadDrillShape, PadProp, PadShape, PadSimElectricalType, Padstack,
};
use crate::pcb_track::ZoneLayerOverride;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, Flashing, Invalidate};

/// `PAD`.
#[derive(Clone, Debug)]
pub struct Pad {
    pub base: ConnectedItemBase,
    pub number: String,       // m_number
    pub pin_function: String, // m_pinFunction
    pub pin_type: String,     // m_pinType
    /// m_libPos: position in the parent footprint's library frame.
    pub(crate) lib_pos: Vector2I,
    /// m_libOrientation: orientation in the parent footprint's library frame.
    pub(crate) lib_orientation: EdaOrientation,
    /// m_padStack: sizes, offsets, drill, primitives in the library frame. Read through
    /// `lib_padstack()` (writer) or the board-frame accessors `size(ctx, layer)` ...
    /// Exception, as in C++: `padstack.orientation` holds the **board-frame** orientation
    /// (`lib_orientation` + parent orientation), re-baked by `set_orientation`,
    /// `set_fp_relative_orientation` and `on_footprint_transformed`.
    pub(crate) padstack: Padstack,
    pub sub_ratsnest: i32,                         // m_subRatsnest
    pub attribute: PadAttrib,                      // m_attribute
    pub property: PadProp,                         // m_property
    pub length_pad_to_die: i32,                    // m_lengthPadToDie
    pub delay_pad_to_die: i32,                     // m_delayPadToDie
    pub sim_electrical_type: PadSimElectricalType, // m_simElectricalType
    /// m_zoneLayerOverrides: written by the zone filler through
    /// `Board::set_zone_layer_override` (derived state, `board-model.md` 8.4).
    pub zone_layer_overrides: BTreeMap<PcbLayerId, ZoneLayerOverride>,
    pub(crate) caches: PadCaches,
}

/// m_drawCache + m_shapesDirty + m_polyDirty[2] + m_effectiveBoundingRadius + m_maskShapes.
/// An empty cell is the C++ "dirty" state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PadCaches {
    /// BuildEffectiveShapes.
    pub shapes: KeyedCache<PadEffectiveShapes>,
    /// BuildEffectivePolygon( ERROR_INSIDE / ERROR_OUTSIDE ).
    pub polygons: [KeyedCache<PadEffectivePolygons>; 2],
    /// m_maskShapes.
    pub mask_shapes: KeyedCache<PadMaskShapes>,
}

impl PadCaches {
    /// Drop every cached value (`PAD::SetDirty()`).
    fn clear(&mut self) {
        self.shapes.clear();
        for polygons in &mut self.polygons {
            polygons.clear();
        }
        self.mask_shapes.clear();
    }
}

/// Every input of the pad caches that is not the pad itself. A cache entry built for another
/// key is rebuilt (insurance against a missed `set_dirty`; C++ relies on the dirty flags only,
/// which `set_dirty()` reproduces by clearing).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct PadCacheKey {
    pub transform: Option<TransformTrs>,
    pub flipped: bool,
    pub max_error: i32,
}

/// m_maskShapes: per layer, the expansion the shape was built for and the shape.
#[derive(Debug)]
#[allow(dead_code)] // built and read by package M15
pub(crate) struct PadMaskShapes {
    pub key: PadCacheKey,
    pub by_layer: BTreeMap<PcbLayerId, (i32, Arc<ShapeCompound>)>,
}

/// The shape part of PAD_DRAW_CACHE_DATA.
#[derive(Debug)]
#[allow(dead_code)] // built and read by package M15
pub(crate) struct PadEffectiveShapes {
    pub key: PadCacheKey,
    pub bounding_box: Box2I,                              // m_effectiveBoundingBox
    pub shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>>, // m_effectiveShapes
    pub hole_shape: Arc<ShapeSegment>,                    // m_effectiveHoleShape
}

/// The polygon part of PAD_DRAW_CACHE_DATA for one ERROR_LOC.
#[derive(Debug)]
#[allow(dead_code)] // built and read by package M15
pub(crate) struct PadEffectivePolygons {
    pub key: PadCacheKey,
    pub polygons: BTreeMap<PcbLayerId, Arc<ShapePolySet>>, // m_effectivePolygons[layer][errorLoc]
    pub bounding_radius: i32,                              // m_effectiveBoundingRadius
}

// ---- constructor -------------------------------------------------------------------------

impl Default for Pad {
    /// `PAD( FOOTPRINT* parent )` without a parent (nil UUID): 60 mil circle with a circle
    /// anchor, 30 mil circular drill, PTH on `PTHMask()`, round-rect ratio 0.25, chamfer ratio
    /// 0.2 without chamfers, no zone-layer override on the 32 copper layers of
    /// `LAYER_RANGE( F_Cu, B_Cu, BoardCopperLayerCount() )` (32 without a board).
    fn default() -> Self {
        let mut padstack = Padstack::default();

        // The constructor's padstack statements, applied to the NORMAL-mode ALL_LAYERS entry
        // that CopperLayer( F_Cu ) returns.
        let mils60 = kicad_core::PCB_IU_SCALE.mils_to_iu(60);
        let mils30 = kicad_core::PCB_IU_SCALE.mils_to_iu(30);
        {
            let all = padstack
                .copper_props
                .get_mut(&Padstack::ALL_LAYERS)
                .expect("PADSTACK() creates ALL_LAYERS");
            all.shape.size = Vector2I::new(mils60, mils60);
            all.shape.shape = PadShape::Circle; // Default pad shape is PAD_CIRCLE.
            // Default anchor shape for custom shaped pads is PAD_CIRCLE.
            all.shape.anchor_shape = PadShape::Circle;
            all.shape.round_rect_radius_ratio = 0.25; // from IPC-7351C standard
            all.shape.chamfered_rect_ratio = 0.2;
            all.shape.chamfered_rect_positions = 0; // RECT_NO_CHAMFER
        }
        padstack.drill.size = Vector2I::new(mils30, mils30); // Default drill size 30 mils.
        padstack.drill.shape = PadDrillShape::Circle; // Default pad drill shape is a circle.
        padstack.layer_set = Pad::pth_mask(); // default for a standard thru hole pad

        let zone_layer_overrides = LayerRange::new(PcbLayerId::FCu, PcbLayerId::BCu, 32)
            .expect("copper layers")
            .iter()
            .map(|layer| (layer, ZoneLayerOverride::None))
            .collect();

        Pad {
            base: ConnectedItemBase::default(),
            number: String::new(),
            pin_function: String::new(),
            pin_type: String::new(),
            lib_pos: Vector2I::new(0, 0),
            lib_orientation: EdaOrientation::new(ANGLE_0),
            padstack,
            sub_ratsnest: 0,
            attribute: PadAttrib::Pth, // Default pad type is plated through hole
            property: PadProp::None,   // no special fabrication property
            length_pad_to_die: 0,
            delay_pad_to_die: 0,
            sim_electrical_type: PadSimElectricalType::None,
            zone_layer_overrides,
            caches: PadCaches::default(),
        }
    }
}

impl Pad {
    /// `new PAD( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut pad = Pad::default();
        pad.base.item.uuid = Kiid::new(kiid_generator);
        pad
    }

    /// `PAD::PTHMask()`: all copper layers and both masks.
    pub fn pth_mask() -> Lset {
        let mut mask = Lset::all_cu_mask().clone();
        mask.set(PcbLayerId::FMask);
        mask.set(PcbLayerId::BMask);
        mask
    }

    /// `PAD::SMDMask()`.
    pub fn smd_mask() -> Lset {
        Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FPaste, PcbLayerId::FMask])
    }

    /// `PAD::ConnSMDMask()`.
    pub fn conn_smd_mask() -> Lset {
        Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FMask])
    }

    /// `PAD::UnplatedHoleMask()`.
    pub fn unplated_hole_mask() -> Lset {
        Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::FMask, PcbLayerId::BMask])
    }

    /// `PAD::ApertureMask()`.
    pub fn aperture_mask() -> Lset {
        Lset::from_layers(&[PcbLayerId::FPaste])
    }

    // ---- trivial accessors --------------------------------------------------------------

    /// `GetLibraryPosition()`: the position in the footprint's library frame (writer).
    pub fn lib_pos(&self) -> Vector2I {
        self.lib_pos
    }

    /// The orientation in the footprint's library frame (`m_libOrientation`, writer).
    pub fn lib_orientation(&self) -> EdaAngle {
        self.lib_orientation.angle()
    }

    /// `Padstack()` as stored: library-frame sizes, offsets and drill, board-frame
    /// orientation (writer, `board-model-types.md` 3).
    pub fn lib_padstack(&self) -> &Padstack {
        &self.padstack
    }

    /// `GetNumber()`.
    pub fn number(&self) -> &str {
        &self.number
    }

    /// `SetNumber()`.
    pub fn set_number(&mut self, number: &str) {
        self.number = number.to_string();
    }

    /// `GetPinFunction()`.
    pub fn pin_function(&self) -> &str {
        &self.pin_function
    }

    /// `GetPinType()`.
    pub fn pin_type(&self) -> &str {
        &self.pin_type
    }

    /// `GetAttribute()`.
    pub fn attribute(&self) -> PadAttrib {
        self.attribute
    }

    /// `GetProperty()`.
    pub fn property(&self) -> PadProp {
        self.property
    }

    /// `SetProperty()`: stores the property and marks the shapes dirty.
    pub fn set_property(&mut self, property: PadProp) {
        self.property = property;
        self.set_dirty();
    }

    /// `GetSubRatsnest()`.
    pub fn sub_ratsnest(&self) -> i32 {
        self.sub_ratsnest
    }

    /// `SetSubRatsnest()`.
    pub fn set_sub_ratsnest(&mut self, sub_ratsnest: i32) {
        self.sub_ratsnest = sub_ratsnest;
    }

    /// `GetPadToDieLength()`.
    pub fn pad_to_die_length(&self) -> i32 {
        self.length_pad_to_die
    }

    /// `GetPadToDieDelay()`.
    pub fn pad_to_die_delay(&self) -> i32 {
        self.delay_pad_to_die
    }

    /// `GetSimElectricalType()`.
    pub fn sim_electrical_type(&self) -> PadSimElectricalType {
        self.sim_electrical_type
    }

    /// `GetShape( aLayer )`: the padstack shape of `layer` (not frame dependent).
    pub fn shape(&self, layer: PcbLayerId) -> PadShape {
        self.padstack.shape(layer)
    }

    /// `GetZoneLayerOverride( aLayer )`: `ZLO_NONE` for a layer without an entry.
    pub fn zone_layer_override(&self, layer: PcbLayerId) -> ZoneLayerOverride {
        self.zone_layer_overrides
            .get(&layer)
            .copied()
            .unwrap_or(ZoneLayerOverride::None)
    }

    /// `PAD::SetDirty()`: drop the effective shapes, polygons and mask shapes.
    pub fn set_dirty(&mut self) {
        self.caches.clear();
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for Pad {
    fn invalidate_caches(&mut self) {
        self.caches.clear();
    }
}

impl BoardItem for Pad {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbPadT
    }
    fn class_name(&self) -> &'static str {
        "PAD"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        Pad::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        Pad::layer_set(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        Pad::is_on_layer(self, ctx, layer)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        Pad::is_on_copper_layer(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        Pad::position(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        Pad::center(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        Pad::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        Pad::effective_shape(self, ctx, layer, flash, usage)
    }
    fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        Pad::effective_hole_shape(self, ctx, layer, usage)
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
        Pad::transform_shape_to_polygon(
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
    fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        Pad::has_hole(self, ctx)
    }
    fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        Pad::has_drilled_hole(self, ctx)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        Pad::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        Pad::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool {
        Pad::is_locked(self, ctx)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        Pad::item_description(self, ctx, units, full)
    }
}

impl BoardConnectedItem for Pad {
    fn own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        Pad::own_clearance(self, ctx, layer, want_source)
    }
    fn local_clearance(&self) -> Option<i32> {
        Pad::local_clearance(self)
    }
    fn clearance_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> Option<(i32, Option<String>)> {
        Pad::clearance_overrides(self, ctx, want_source)
    }
}

impl BoardItemMut for Pad {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        Pad::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        Pad::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        Pad::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        Pad::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        Pad::mirror(self, env, centre, dir)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        Pad::set_layer_set(self, env, layers)
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        Pad::on_footprint_transformed(self, env)
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
        Pad::on_footprint_rescaled(
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

// ---- derived-state writers of Board (board-model.md 8.4) ---------------------------------

impl Board {
    /// `PAD::SetZoneLayerOverride` / `PCB_VIA::SetZoneLayerOverride` on the pad or via `id`
    /// (the via form ignores non-copper layers, `hasLayerOrdinal`). Derived state: neither
    /// invalidates caches nor bumps the time stamp.
    pub fn set_zone_layer_override(
        &mut self,
        id: ItemId,
        layer: PcbLayerId,
        value: ZoneLayerOverride,
    ) {
        crate::port_pending!("PAD/PCB_VIA::SetZoneLayerOverride", id, layer, value)
    }

    /// `PAD::ClearZoneLayerOverrides` (the board's copper layers) / `PCB_VIA::
    /// ClearZoneLayerOverrides` (every slot) on the pad or via `id`. Derived state.
    pub fn clear_zone_layer_overrides(&mut self, id: ItemId) {
        crate::port_pending!("PAD/PCB_VIA::ClearZoneLayerOverrides", id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PAD::PAD( FOOTPRINT* parent )` (pad.cpp) without a board.
    #[test]
    fn pad_constructor_defaults() {
        let pad = Pad::default();
        let all = &pad.padstack.copper_props[&Padstack::ALL_LAYERS];
        assert_eq!(all.shape.size, Vector2I::new(1_524_000, 1_524_000));
        assert_eq!(all.shape.shape, PadShape::Circle);
        assert_eq!(all.shape.anchor_shape, PadShape::Circle);
        assert_eq!(all.shape.round_rect_radius_ratio, 0.25);
        assert_eq!(all.shape.chamfered_rect_ratio, 0.2);
        assert_eq!(all.shape.chamfered_rect_positions, 0);
        assert_eq!(pad.padstack.drill.size, Vector2I::new(762_000, 762_000));
        assert_eq!(pad.padstack.drill.shape, PadDrillShape::Circle);
        assert_eq!(pad.padstack.layer_set, Pad::pth_mask());
        assert!(pad.padstack.layer_set.contains(PcbLayerId::In30Cu));
        assert!(pad.padstack.layer_set.contains(PcbLayerId::BMask));
        assert!(!pad.padstack.layer_set.contains(PcbLayerId::FPaste));
        assert_eq!(pad.attribute, PadAttrib::Pth);
        assert_eq!(pad.property, PadProp::None);
        assert_eq!(pad.sub_ratsnest, 0);
        assert_eq!((pad.length_pad_to_die, pad.delay_pad_to_die), (0, 0));
        assert_eq!(pad.sim_electrical_type, PadSimElectricalType::None);
        assert_eq!(pad.lib_pos, Vector2I::new(0, 0));
        assert_eq!(pad.lib_orientation(), ANGLE_0);
        assert_eq!(pad.zone_layer_overrides.len(), 32);
        assert_eq!(pad.zone_layer_override(PcbLayerId::In5Cu), ZoneLayerOverride::None);
        assert!(pad.base.item.uuid == Kiid::default());
        assert!(pad.number.is_empty() && pad.pin_function.is_empty());
    }

    #[test]
    fn pad_in_fixture() {
        let mut fx = rotated_flipped_scaled();
        fx.board.check_invariants().unwrap();
        let id = fx.child(ItemKind::Pad).typed::<Pad>().unwrap();
        let mut pad = Pad::new(&mut fx.board.kiid_generator);
        pad.set_number("7");
        pad.set_property(PadProp::Testpoint);
        fx.set_child(id, pad);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].number(), "7");
        assert_eq!(fx.board[id].property(), PadProp::Testpoint);

        let new_id = fx.add_child(Pad::default(), |fp| &mut fp.pads);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[new_id].attribute(), PadAttrib::Pth);
    }
}
