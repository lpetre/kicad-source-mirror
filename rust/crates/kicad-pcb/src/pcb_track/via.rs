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

//! Port of the `PCB_VIA` part of `pcbnew/pcb_track.cpp` and `pcb_track.h` (layers, tenting /
//! covering / plugging / capping / filling, backdrill, post-machining, FlashLayer, zone layer
//! overrides, ValidateViaParameters, CollectMicroviaColumns). Package M17; M0a wrote the
//! signatures of the trait-backing methods. Methods named `PCB_TRACK::X (PCB_VIA)` are the
//! `PCB_TRACK` bodies as they run for a via. Via shapes are in `shapes.rs`.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::layer_ids::{
    copper_layer_to_ordinal, is_back_layer, is_copper_layer, is_copper_layer_lower_than,
    is_front_layer, to_layer_id,
};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, LayerRange, Lset, MinOptMax, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::math::box2::box2i_safe_from_int;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, Vector2D, Vector2I, Vector2L};

use super::{constraint_name, with_this};
use crate::board_connected_item;
use crate::board_item::{self, mirror_axis};
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::{DrcConstraint, DrcConstraintT};
use crate::ids::TrackId;
use crate::item::{BoardItemBase, ItemFlags};
use crate::item_ref::{AnyItem, ItemRef};
use crate::padstack::{
    BackdrillMode, DrillProps, PadDrillPostMachiningMode, PadDrillShape, Padstack, PadstackMode,
    UnconnectedLayerMode,
};
use crate::pcb_track::{
    CappingMode, CoveringMode, FillingMode, PcbTrack, PcbVia, PluggingMode, TentingMode, Track,
    UNDEFINED_DRILL_DIAMETER, Viatype, ZoneLayerOverride,
};
use crate::traits::{BoardConnectedItem, BoardItem};

/// `GEOMETRY_MIN_SIZE` (`pcbnew/pcb_track.h`): `(int) ( 0.001 * pcbIUScale.IU_PER_MM )`.
pub fn geometry_min_size() -> i32 {
    (0.001 * PCB_IU_SCALE.iu_per_mm) as i32
}

/// The item types `PCB_VIA::FlashLayer` / `GetOutermostConnectedLayers` ask connectivity about
/// (`nonZoneTypes`).
const NON_ZONE_TYPES: [KicadT; 4] = [
    KicadT::PcbTraceT,
    KicadT::PcbArcT,
    KicadT::PcbViaT,
    KicadT::PcbPadT,
];

/// `PCB_VIA::VIA_PARAMETER_ERROR::FIELD`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViaParameterField {
    #[default]
    None,
    Diameter,
    Drill,
    StartLayer,
    EndLayer,
    SecondaryDrill,
    SecondaryStartLayer,
    SecondaryEndLayer,
    TertiaryDrill,
    TertiaryStartLayer,
    TertiaryEndLayer,
}

/// `PCB_VIA::VIA_PARAMETER_ERROR`: the message (English, `_()` untranslated) and the field.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ViaParameterError {
    pub message: String,
    pub field: ViaParameterField,
}

/// Port of `DRC_CONSTRAINT::Merge( aOther )` (`pcbnew/drc/drc_rule.cpp`): the tighter min /
/// max and the first opt, with the rules they came from.
fn merge_constraint(this: &mut DrcConstraint, other: &DrcConstraint) {
    if other.value.has_min() && (!this.value.has_min() || (other.value.min() > this.value.min()))
    {
        this.value.set_min(other.value.min());
        this.min_rule = if other.min_rule.is_some() {
            other.min_rule.clone()
        } else {
            other.parent_rule.clone()
        };
    }

    if other.value.has_max() && (!this.value.has_max() || other.value.max() < this.value.max()) {
        this.value.set_max(other.value.max());
        this.max_rule = if other.max_rule.is_some() {
            other.max_rule.clone()
        } else {
            other.parent_rule.clone()
        };
    }

    if other.value.has_opt() && !this.value.has_opt() {
        this.value.set_opt(other.value.opt());
        this.parent_rule = other.parent_rule.clone();
    }
}

/// `hasLayerOrdinal( aLayer )`: `IsCopperLayer()` accepts any even id below
/// `PCB_LAYER_ID_COUNT`, but only F_Cu..In30_Cu carry a distinct ordinal. Anything above
/// aliases B_Cu or indexes past the override array.
fn has_layer_ordinal(layer: PcbLayerId) -> bool {
    is_copper_layer(layer.0) && layer <= PcbLayerId::In30Cu
}

/// `LAYER_RANGE( aStart, aEnd, aCount )` iterated; C++ throws `std::invalid_argument` for a
/// non-copper end point (a programmer error: via end points are copper layers), here the
/// range is empty.
fn layer_range(start: PcbLayerId, end: PcbLayerId, count: i32) -> Vec<PcbLayerId> {
    match LayerRange::new(start, end, count) {
        Ok(range) => range.iter().collect(),
        Err(_) => {
            debug_assert!(false, "LAYER_RANGE of a non-copper layer");
            Vec::new()
        }
    }
}

impl PcbVia {
    // ---- helpers ---------------------------------------------------------------------------

    /// The stored item (`this`) for a rule or connectivity query; a copy of the value for a via
    /// without a live handle (`super::with_this`).
    fn with_this<R>(&self, ctx: &BoardCtx<'_>, f: impl FnOnce(ItemRef<'_>) -> R) -> R {
        with_this(
            ctx,
            self.base_item(),
            || Track::Via(Box::new(self.clone())),
            f,
        )
    }

    fn base_item(&self) -> &BoardItemBase {
        &self.track.base.item
    }

    /// `GetBoard() != nullptr`.
    fn has_board(&self, ctx: &BoardCtx<'_>) -> bool {
        board_item::has_board(self, ctx)
    }

    // ---- copy / compare --------------------------------------------------------------------

    /// Port of the copy constructor `PCB_VIA( const PCB_VIA& )` (C++ `Clone()`, `PCB_VIA
    /// dummy( *via )`): `PCB_VIA::operator=` copies the `BOARD_CONNECTED_ITEM` part, `m_Start`,
    /// `m_End`, the via type, the padstack and `m_isFree`; the zone layer overrides are copied
    /// by the constructor. The `PCB_TRACK` members (`m_width`, `m_hasSolderMask`,
    /// `m_solderMaskMargin`) keep their constructor values. The copy has no handle
    /// (`board-model.md` 3). Shadows `DetachedCopy::detached_copy` for a `PcbVia` value; a
    /// `Track` value still uses the generic clone (see `PORT_STATUS.md`).
    pub fn detached_copy(&self) -> PcbVia {
        let defaults = PcbTrack::default();
        let mut copy = self.clone();
        copy.track.width = defaults.width;
        copy.track.has_solder_mask = defaults.has_solder_mask;
        copy.track.solder_mask_margin = defaults.solder_mask_margin;
        copy.track.base.item.id = None;
        copy
    }

    /// Port of `PCB_VIA::sameZoneLayerOverrides`.
    fn same_zone_layer_overrides(&self, other: &PcbVia) -> bool {
        self.zone_layer_overrides == other.zone_layer_overrides
    }

    /// Port of `PCB_VIA::operator==( const PCB_VIA& )`: ends, `m_layer`, padstack, via type and
    /// zone layer overrides.
    pub fn eq_item(&self, other: &PcbVia) -> bool {
        self.track.start == other.track.start
            && self.track.end == other.track.end
            && self.track.base.item.layer == other.track.base.item.layer
            && self.padstack.eq_padstack(&other.padstack)
            && self.via_type == other.via_type
            && self.same_zone_layer_overrides(other)
    }

    /// Port of `PCB_VIA::Similarity` for two vias (the C++ returns 0.0 for another type).
    pub fn similarity(&self, other: &PcbVia) -> f64 {
        let mut similarity = 1.0;

        if self.track.base.item.layer != other.track.base.item.layer {
            similarity *= 0.9;
        }

        if self.track.start != other.track.start {
            similarity *= 0.9;
        }

        if self.track.end != other.track.end {
            similarity *= 0.9;
        }

        if !self.padstack.eq_padstack(&other.padstack) {
            similarity *= 0.9;
        }

        if self.via_type != other.via_type {
            similarity *= 0.9;
        }

        if !self.same_zone_layer_overrides(other) {
            similarity *= 0.9;
        }

        similarity
    }

    // ---- width / size ----------------------------------------------------------------------

    /// `PCB_VIA::GetWidth()` (the override without a layer argument): `wxCHECK_MSG( false, .. )`
    /// answering with the `PADSTACK::ALL_LAYERS` size. Use `width( layer )`.
    pub fn width_no_layer(&self) -> i32 {
        // This is present because of the parent class.  It should never be actually called on
        // a via.
        debug_assert!(
            false,
            "Warning: PCB_VIA::GetWidth() called without a layer argument"
        );
        self.padstack.size(Padstack::ALL_LAYERS).x
    }

    /// `PCB_VIA::SetWidth( aWidth )` (the override without a layer argument): sets the
    /// `PADSTACK::ALL_LAYERS` size and asserts. Use `set_width( layer, width )`.
    pub fn set_width_no_layer(&mut self, width: i32) {
        self.padstack
            .set_size(Vector2I::new(width, width), Padstack::ALL_LAYERS);
        debug_assert!(
            false,
            "Warning: PCB_VIA::SetWidth() called without a layer argument"
        );
    }

    /// Port of `PCB_VIA::SetWidth( aLayer, aWidth )`.
    pub fn set_width(&mut self, layer: PcbLayerId, width: i32) {
        self.padstack.set_size(Vector2I::new(width, width), layer);
    }

    /// Port of `PCB_VIA::GetWidth( aLayer )`: `m_padStack.Size( aLayer ).x`.
    pub fn width(&self, layer: PcbLayerId) -> i32 {
        self.padstack.size(layer).x
    }

    /// `SetFrontWidth()`: `SetWidth( F_Cu, aWidth )`.
    pub fn set_front_width(&mut self, width: i32) {
        self.set_width(PcbLayerId::FCu, width);
    }

    /// `GetFrontWidth()`: `GetWidth( F_Cu )`.
    pub fn front_width(&self) -> i32 {
        self.width(PcbLayerId::FCu)
    }

    /// `SetPadstackMode()`.
    pub fn set_padstack_mode(&mut self, mode: PadstackMode) {
        self.padstack.set_mode(mode);
    }

    /// `Padstack()`.
    pub fn padstack(&self) -> &Padstack {
        &self.padstack
    }

    /// `Padstack()` (mutable).
    pub fn padstack_mut(&mut self) -> &mut Padstack {
        &mut self.padstack
    }

    /// `SetPadstack()`.
    pub fn set_padstack(&mut self, padstack: Padstack) {
        self.padstack = padstack;
    }

    /// Port of `PCB_VIA::GetWidthConstraint( aSource )`: `VIA_DIAMETER_CONSTRAINT` merged over
    /// the via's layer range (a null constraint without rules or board); (value, constraint
    /// name if asked).
    pub fn width_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        let mut aggregate = DrcConstraint::default();

        if self.has_board(ctx) {
            let layer_count = ctx.copper_layer_count();

            if let Some(drc_engine) = ctx.rules {
                let layers = layer_range(
                    self.padstack.start_layer(),
                    self.padstack.end_layer(),
                    layer_count,
                );

                self.with_this(ctx, |this| {
                    for layer in layers {
                        let c = drc_engine.eval_rules(
                            ctx,
                            DrcConstraintT::ViaDiameterConstraint,
                            Some(this),
                            None,
                            layer,
                        );
                        merge_constraint(&mut aggregate, &c);
                    }
                });
            }
        }

        let source = want_source.then(|| constraint_name(&aggregate));

        (aggregate.value, source)
    }

    /// Port of `PCB_VIA::GetDrillConstraint( aSource )`: `HOLE_SIZE_CONSTRAINT` on
    /// `UNDEFINED_LAYER` (holes are not layer-specific, as in the hole size test).
    pub fn drill_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        let mut aggregate = DrcConstraint::default();

        if self.has_board(ctx) {
            if let Some(drc_engine) = ctx.rules {
                // Holes are not layer-specific, as in the hole size test
                aggregate = self.with_this(ctx, |this| {
                    drc_engine.eval_rules(
                        ctx,
                        DrcConstraintT::HoleSizeConstraint,
                        Some(this),
                        None,
                        PcbLayerId::UndefinedLayer,
                    )
                });
            }
        }

        let source = want_source.then(|| constraint_name(&aggregate));

        (aggregate.value, source)
    }

    /// Port of `PCB_VIA::SetSizeFromRules( aDiameter, aDrill )`: size the via at the preferred
    /// values, then pin them to the diameter / drill constraints evaluated on the via at that
    /// size. A `&mut self` method with a context: apply it clone-edit-store (`board-model.md`
    /// 8.2). The rules see the edited value (C++ evaluates `this` after the first `SetWidth`),
    /// which has no handle here, like the `ROUTER_TRANSIENT` item C++ keeps out of the
    /// pointer-keyed rule caches.
    pub fn set_size_from_rules(&mut self, ctx: &BoardCtx<'_>, diameter: i32, drill: i32) {
        // Area rules test the via's shape, so evaluate them at the preferred size
        self.set_padstack_mode(PadstackMode::Normal);
        self.set_width(Padstack::ALL_LAYERS, diameter);
        self.set_drill(drill);

        // Callers size vias that may still move, so keep them out of the pointer-keyed rule
        // caches
        let was_transient = self.track.base.item.flags.0 & ItemFlags::ROUTER_TRANSIENT != 0;
        self.track.base.item.flags.0 |= ItemFlags::ROUTER_TRANSIENT;

        let mut probe = self.clone();
        probe.track.base.item.id = None;
        let new_diameter = probe.width_constraint(ctx, false).0.pinned_opt_or(diameter);
        let new_drill = probe.drill_constraint(ctx, false).0.pinned_opt_or(drill);

        if !was_transient {
            self.track.base.item.flags.0 &= !ItemFlags::ROUTER_TRANSIENT;
        }

        self.set_width(Padstack::ALL_LAYERS, new_diameter);
        self.set_drill(new_drill);
    }

    /// Port of `PCB_VIA::GetMinAnnulus( aLayer, aSource )`: 0 ("removed annular ring") where
    /// the via is not flashed, else the min of the `ANNULAR_WIDTH_CONSTRAINT` (0 without one).
    pub fn min_annulus(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        if !self.flash_layer(ctx, layer) {
            return (0, want_source.then(|| "removed annular ring".to_string()));
        }

        let mut constraint = DrcConstraint::default();

        if self.has_board(ctx) {
            if let Some(drc_engine) = ctx.rules {
                constraint = self.with_this(ctx, |this| {
                    drc_engine.eval_rules(
                        ctx,
                        DrcConstraintT::AnnularWidthConstraint,
                        Some(this),
                        None,
                        layer,
                    )
                });
            }
        }

        if constraint.value.has_min() {
            return (
                constraint.value.min(),
                want_source.then(|| constraint_name(&constraint)),
            );
        }

        (0, None)
    }

    // ---- drills ----------------------------------------------------------------------------

    /// `SetPrimaryDrillSize()`.
    pub fn set_primary_drill_size(&mut self, size: Vector2I) {
        self.padstack.drill_mut().size = size;
    }

    /// `GetPrimaryDrillSize()`.
    pub fn primary_drill_size(&self) -> Vector2I {
        self.padstack.drill().size
    }

    /// `SetPrimaryDrillShape()`.
    pub fn set_primary_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.drill_mut().shape = shape;
    }

    /// `GetPrimaryDrillShape()`.
    pub fn primary_drill_shape(&self) -> PadDrillShape {
        self.padstack.drill().shape
    }

    /// `SetPrimaryDrillStartLayer()`.
    pub fn set_primary_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.drill_mut().start = layer;
    }

    /// `GetPrimaryDrillStartLayer()`.
    pub fn primary_drill_start_layer(&self) -> PcbLayerId {
        self.padstack.drill().start
    }

    /// `SetPrimaryDrillEndLayer()`.
    pub fn set_primary_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.drill_mut().end = layer;
    }

    /// `GetPrimaryDrillEndLayer()`.
    pub fn primary_drill_end_layer(&self) -> PcbLayerId {
        self.padstack.drill().end
    }

    /// `SetPrimaryDrillFilled()` / `SetPrimaryDrillFilledFlag()`.
    pub fn set_primary_drill_filled(&mut self, filled: Option<bool>) {
        self.padstack.drill_mut().is_filled = filled;
    }

    /// `GetPrimaryDrillFilled()`.
    pub fn primary_drill_filled(&self) -> Option<bool> {
        self.padstack.drill().is_filled
    }

    /// `GetPrimaryDrillFilledFlag()`.
    pub fn primary_drill_filled_flag(&self) -> bool {
        self.padstack.drill().is_filled.unwrap_or(false)
    }

    /// `SetPrimaryDrillCapped()` / `SetPrimaryDrillCappedFlag()`.
    pub fn set_primary_drill_capped(&mut self, capped: Option<bool>) {
        self.padstack.drill_mut().is_capped = capped;
    }

    /// `GetPrimaryDrillCapped()`.
    pub fn primary_drill_capped(&self) -> Option<bool> {
        self.padstack.drill().is_capped
    }

    /// `GetPrimaryDrillCappedFlag()`.
    pub fn primary_drill_capped_flag(&self) -> bool {
        self.padstack.drill().is_capped.unwrap_or(false)
    }

    /// `SetDrill( aDrill )`: a round primary drill.
    pub fn set_drill(&mut self, drill: i32) {
        self.set_primary_drill_size(Vector2I::new(drill, drill));
    }

    /// `GetDrill()`: the stored drill (`UNDEFINED_DRILL_DIAMETER` for the netclass default).
    pub fn drill(&self) -> i32 {
        self.primary_drill_size().x
    }

    /// `SetDrillDefault()`: use the netclass drill.
    pub fn set_drill_default(&mut self) {
        self.padstack.drill_mut().size =
            Vector2I::new(UNDEFINED_DRILL_DIAMETER, UNDEFINED_DRILL_DIAMETER);
    }

    /// Port of `PCB_VIA::GetDrillValue`: the specific drill, else the (micro)via drill of the
    /// effective netclass.
    pub fn drill_value(&self, ctx: &BoardCtx<'_>) -> i32 {
        if self.padstack.drill().size.x > 0 {
            // Use the specific value.
            return self.padstack.drill().size.x;
        }

        // Use the default value from the Netclass
        let netclass = self.effective_netclass(ctx);

        if self.via_type == Viatype::Microvia {
            return netclass.uvia_drill();
        }

        netclass.via_drill()
    }

    /// `SetSecondaryDrillSize( const VECTOR2I& )`.
    pub fn set_secondary_drill_size(&mut self, size: Vector2I) {
        self.padstack.secondary_drill_mut().size = size;
    }

    /// `ClearSecondaryDrillSize()`.
    pub fn clear_secondary_drill_size(&mut self) {
        self.padstack.secondary_drill_mut().size = Vector2I::new(0, 0);
    }

    /// Port of `SetSecondaryDrillSize( const std::optional<int>& )`.
    pub fn set_secondary_drill_size_opt(&mut self, drill: Option<i32>) {
        match drill {
            Some(d) if d > 0 => self.set_secondary_drill_size(Vector2I::new(d, d)),
            _ => self.clear_secondary_drill_size(),
        }
    }

    /// Port of `GetSecondaryDrillSize()`.
    pub fn secondary_drill_size(&self) -> Option<i32> {
        (self.padstack.secondary_drill().size.x > 0)
            .then_some(self.padstack.secondary_drill().size.x)
    }

    /// Port of `SetSecondaryDrillStartLayer()`: `UNDEFINED_LAYER` also clears the size.
    pub fn set_secondary_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.secondary_drill_mut().start = layer;

        if layer == PcbLayerId::UndefinedLayer {
            self.padstack.secondary_drill_mut().size = Vector2I::new(0, 0);
        }
    }

    /// `GetSecondaryDrillStartLayer()`.
    pub fn secondary_drill_start_layer(&self) -> PcbLayerId {
        self.padstack.secondary_drill().start
    }

    /// `SetSecondaryDrillEndLayer()`.
    pub fn set_secondary_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.secondary_drill_mut().end = layer;
    }

    /// `GetSecondaryDrillEndLayer()`.
    pub fn secondary_drill_end_layer(&self) -> PcbLayerId {
        self.padstack.secondary_drill().end
    }

    /// `SetSecondaryDrillShape()`.
    pub fn set_secondary_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.secondary_drill_mut().shape = shape;
    }

    /// `GetSecondaryDrillShape()`.
    pub fn secondary_drill_shape(&self) -> PadDrillShape {
        self.padstack.secondary_drill().shape
    }

    /// `SetTertiaryDrillSize( const VECTOR2I& )`.
    pub fn set_tertiary_drill_size(&mut self, size: Vector2I) {
        self.padstack.tertiary_drill_mut().size = size;
    }

    /// `ClearTertiaryDrillSize()`.
    pub fn clear_tertiary_drill_size(&mut self) {
        self.padstack.tertiary_drill_mut().size = Vector2I::new(0, 0);
    }

    /// Port of `SetTertiaryDrillSize( const std::optional<int>& )`.
    pub fn set_tertiary_drill_size_opt(&mut self, drill: Option<i32>) {
        match drill {
            Some(d) if d > 0 => self.set_tertiary_drill_size(Vector2I::new(d, d)),
            _ => self.clear_tertiary_drill_size(),
        }
    }

    /// Port of `GetTertiaryDrillSize()`.
    pub fn tertiary_drill_size(&self) -> Option<i32> {
        (self.padstack.tertiary_drill().size.x > 0)
            .then_some(self.padstack.tertiary_drill().size.x)
    }

    /// Port of `SetTertiaryDrillStartLayer()`: `UNDEFINED_LAYER` also clears the size.
    pub fn set_tertiary_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.tertiary_drill_mut().start = layer;

        if layer == PcbLayerId::UndefinedLayer {
            self.padstack.tertiary_drill_mut().size = Vector2I::new(0, 0);
        }
    }

    /// `GetTertiaryDrillStartLayer()`.
    pub fn tertiary_drill_start_layer(&self) -> PcbLayerId {
        self.padstack.tertiary_drill().start
    }

    /// `SetTertiaryDrillEndLayer()`.
    pub fn set_tertiary_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.tertiary_drill_mut().end = layer;
    }

    /// `GetTertiaryDrillEndLayer()`.
    pub fn tertiary_drill_end_layer(&self) -> PcbLayerId {
        self.padstack.tertiary_drill().end
    }

    /// `SetTertiaryDrillShape()`.
    pub fn set_tertiary_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.tertiary_drill_mut().shape = shape;
    }

    /// `GetTertiaryDrillShape()`.
    pub fn tertiary_drill_shape(&self) -> PadDrillShape {
        self.padstack.tertiary_drill().shape
    }

    /// `GetBackdrillMode()`.
    pub fn backdrill_mode(&self) -> BackdrillMode {
        self.padstack.backdrill_mode()
    }

    /// `SetBackdrillMode()`.
    pub fn set_backdrill_mode(&mut self, mode: BackdrillMode) {
        self.padstack.set_backdrill_mode(mode);
    }

    /// `GetBottomBackdrillSize()`.
    pub fn bottom_backdrill_size(&self) -> Option<i32> {
        self.padstack.backdrill_size(false)
    }

    /// `SetBottomBackdrillSize()`.
    pub fn set_bottom_backdrill_size(&mut self, size: Option<i32>) {
        self.padstack.set_backdrill_size(false, size);
    }

    /// `GetBottomBackdrillLayer()`.
    pub fn bottom_backdrill_layer(&self) -> PcbLayerId {
        self.padstack.backdrill_end_layer(false)
    }

    /// `SetBottomBackdrillLayer()`.
    pub fn set_bottom_backdrill_layer(&mut self, layer: PcbLayerId) {
        self.padstack.set_backdrill_end_layer(false, layer);
    }

    /// `GetTopBackdrillSize()`.
    pub fn top_backdrill_size(&self) -> Option<i32> {
        self.padstack.backdrill_size(true)
    }

    /// `SetTopBackdrillSize()`.
    pub fn set_top_backdrill_size(&mut self, size: Option<i32>) {
        self.padstack.set_backdrill_size(true, size);
    }

    /// `GetTopBackdrillLayer()`.
    pub fn top_backdrill_layer(&self) -> PcbLayerId {
        self.padstack.backdrill_end_layer(true)
    }

    /// `SetTopBackdrillLayer()`.
    pub fn set_top_backdrill_layer(&mut self, layer: PcbLayerId) {
        self.padstack.set_backdrill_end_layer(true, layer);
    }

    /// `SetFrontPostMachining()` / `SetFrontPostMachiningMode()`.
    pub fn set_front_post_machining(&mut self, mode: Option<PadDrillPostMachiningMode>) {
        self.padstack.front_post_machining_mut().mode = mode;
    }

    /// `GetFrontPostMachining()`.
    pub fn front_post_machining(&self) -> Option<PadDrillPostMachiningMode> {
        self.padstack.front_post_machining().mode
    }

    /// `GetFrontPostMachiningMode()`: `NOT_POST_MACHINED` when unset.
    pub fn front_post_machining_mode(&self) -> PadDrillPostMachiningMode {
        self.padstack
            .front_post_machining()
            .mode
            .unwrap_or(PadDrillPostMachiningMode::NotPostMachined)
    }

    /// `SetFrontPostMachiningSize()`.
    pub fn set_front_post_machining_size(&mut self, size: i32) {
        self.padstack.front_post_machining_mut().size = size;
    }

    /// `GetFrontPostMachiningSize()`.
    pub fn front_post_machining_size(&self) -> i32 {
        self.padstack.front_post_machining().size
    }

    /// `SetFrontPostMachiningDepth()`.
    pub fn set_front_post_machining_depth(&mut self, depth: i32) {
        self.padstack.front_post_machining_mut().depth = depth;
    }

    /// `GetFrontPostMachiningDepth()`.
    pub fn front_post_machining_depth(&self) -> i32 {
        self.padstack.front_post_machining().depth
    }

    /// `SetFrontPostMachiningAngle()` (tenths of a degree).
    pub fn set_front_post_machining_angle(&mut self, angle: i32) {
        self.padstack.front_post_machining_mut().angle = angle;
    }

    /// `GetFrontPostMachiningAngle()`.
    pub fn front_post_machining_angle(&self) -> i32 {
        self.padstack.front_post_machining().angle
    }

    /// `SetBackPostMachining()` / `SetBackPostMachiningMode()`.
    pub fn set_back_post_machining(&mut self, mode: Option<PadDrillPostMachiningMode>) {
        self.padstack.back_post_machining_mut().mode = mode;
    }

    /// `GetBackPostMachining()`.
    pub fn back_post_machining(&self) -> Option<PadDrillPostMachiningMode> {
        self.padstack.back_post_machining().mode
    }

    /// `GetBackPostMachiningMode()`: `NOT_POST_MACHINED` when unset.
    pub fn back_post_machining_mode(&self) -> PadDrillPostMachiningMode {
        self.padstack
            .back_post_machining()
            .mode
            .unwrap_or(PadDrillPostMachiningMode::NotPostMachined)
    }

    /// `SetBackPostMachiningSize()`.
    pub fn set_back_post_machining_size(&mut self, size: i32) {
        self.padstack.back_post_machining_mut().size = size;
    }

    /// `GetBackPostMachiningSize()`.
    pub fn back_post_machining_size(&self) -> i32 {
        self.padstack.back_post_machining().size
    }

    /// `SetBackPostMachiningDepth()`.
    pub fn set_back_post_machining_depth(&mut self, depth: i32) {
        self.padstack.back_post_machining_mut().depth = depth;
    }

    /// `GetBackPostMachiningDepth()`.
    pub fn back_post_machining_depth(&self) -> i32 {
        self.padstack.back_post_machining().depth
    }

    /// `SetBackPostMachiningAngle()` (tenths of a degree).
    pub fn set_back_post_machining_angle(&mut self, angle: i32) {
        self.padstack.back_post_machining_mut().angle = angle;
    }

    /// `GetBackPostMachiningAngle()`.
    pub fn back_post_machining_angle(&self) -> i32 {
        self.padstack.back_post_machining().angle
    }

    /// Port of `PCB_VIA::IsBackdrilledOrPostMachined( aLayer )`.
    pub fn is_backdrilled_or_post_machined(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.backdrill_or_post_machining_knockout(ctx, layer) > 0
    }

    /// Port of `PCB_VIA::HasCopperRemoved( aLayer )`: the knockout covers the whole pad.
    pub fn has_copper_removed(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let knockout = self.backdrill_or_post_machining_knockout(ctx, layer);

        knockout > 0 && knockout >= self.width(layer)
    }

    /// Port of `PCB_VIA::GetBackdrillOrPostMachiningKnockout( aLayer )`: 0 off copper or
    /// without a board, else `PADSTACK::MachiningKnockout`.
    pub fn backdrill_or_post_machining_knockout(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
    ) -> i32 {
        if !is_copper_layer(layer.0) || !self.has_board(ctx) {
            return 0;
        }

        self.padstack.machining_knockout(ctx, layer)
    }

    /// Port of `PCB_VIA::GetPostMachiningKnockout( aLayer )`.
    pub fn post_machining_knockout(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        if !is_copper_layer(layer.0) || !self.has_board(ctx) {
            return 0;
        }

        self.padstack
            .post_machining_knockout(ctx.board.design_settings().stackup_descriptor(), layer)
    }

    // ---- tenting / covering / plugging / capping / filling ---------------------------------

    /// Port of `PCB_VIA::SetFrontTentingMode`.
    pub fn set_front_tenting_mode(&mut self, mode: TentingMode) {
        self.padstack.front_outer_layers_mut().has_solder_mask = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetFrontTentingMode`.
    pub fn front_tenting_mode(&self) -> TentingMode {
        match self.padstack.front_outer_layers().has_solder_mask {
            Some(true) => TentingMode::Tented,
            Some(false) => TentingMode::NotTented,
            None => TentingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveFrontTentingMode`: `FROM_BOARD` resolved with
    /// `m_TentViasFront` (not tented without a board).
    pub fn resolve_front_tenting_mode(&self, ctx: &BoardCtx<'_>) -> TentingMode {
        if self.front_tenting_mode() == TentingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().tent_vias_front {
                return TentingMode::Tented;
            } else {
                return TentingMode::NotTented;
            }
        }

        self.front_tenting_mode()
    }

    /// Port of `PCB_VIA::SetBackTentingMode`.
    pub fn set_back_tenting_mode(&mut self, mode: TentingMode) {
        self.padstack.back_outer_layers_mut().has_solder_mask = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetBackTentingMode`.
    pub fn back_tenting_mode(&self) -> TentingMode {
        match self.padstack.back_outer_layers().has_solder_mask {
            Some(true) => TentingMode::Tented,
            Some(false) => TentingMode::NotTented,
            None => TentingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveBackTentingMode`.
    pub fn resolve_back_tenting_mode(&self, ctx: &BoardCtx<'_>) -> TentingMode {
        if self.back_tenting_mode() == TentingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().tent_vias_back {
                return TentingMode::Tented;
            } else {
                return TentingMode::NotTented;
            }
        }

        self.back_tenting_mode()
    }

    /// Port of `PCB_VIA::SetFrontCoveringMode`.
    pub fn set_front_covering_mode(&mut self, mode: CoveringMode) {
        self.padstack.front_outer_layers_mut().has_covering = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetFrontCoveringMode`.
    pub fn front_covering_mode(&self) -> CoveringMode {
        match self.padstack.front_outer_layers().has_covering {
            Some(true) => CoveringMode::Covered,
            Some(false) => CoveringMode::NotCovered,
            None => CoveringMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveFrontCoveringMode`.
    pub fn resolve_front_covering_mode(&self, ctx: &BoardCtx<'_>) -> CoveringMode {
        if self.front_covering_mode() == CoveringMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().cover_vias_front {
                return CoveringMode::Covered;
            } else {
                return CoveringMode::NotCovered;
            }
        }

        self.front_covering_mode()
    }

    /// Port of `PCB_VIA::SetBackCoveringMode`.
    pub fn set_back_covering_mode(&mut self, mode: CoveringMode) {
        self.padstack.back_outer_layers_mut().has_covering = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetBackCoveringMode`.
    pub fn back_covering_mode(&self) -> CoveringMode {
        match self.padstack.back_outer_layers().has_covering {
            Some(true) => CoveringMode::Covered,
            Some(false) => CoveringMode::NotCovered,
            None => CoveringMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveBackCoveringMode`.
    pub fn resolve_back_covering_mode(&self, ctx: &BoardCtx<'_>) -> CoveringMode {
        if self.back_covering_mode() == CoveringMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().cover_vias_back {
                return CoveringMode::Covered;
            } else {
                return CoveringMode::NotCovered;
            }
        }

        self.back_covering_mode()
    }

    /// Port of `PCB_VIA::SetFrontPluggingMode`.
    pub fn set_front_plugging_mode(&mut self, mode: PluggingMode) {
        self.padstack.front_outer_layers_mut().has_plugging = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetFrontPluggingMode`.
    pub fn front_plugging_mode(&self) -> PluggingMode {
        match self.padstack.front_outer_layers().has_plugging {
            Some(true) => PluggingMode::Plugged,
            Some(false) => PluggingMode::NotPlugged,
            None => PluggingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveFrontPluggingMode`.
    pub fn resolve_front_plugging_mode(&self, ctx: &BoardCtx<'_>) -> PluggingMode {
        if self.front_plugging_mode() == PluggingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().plug_vias_front {
                return PluggingMode::Plugged;
            } else {
                return PluggingMode::NotPlugged;
            }
        }

        self.front_plugging_mode()
    }

    /// Port of `PCB_VIA::SetBackPluggingMode`.
    pub fn set_back_plugging_mode(&mut self, mode: PluggingMode) {
        self.padstack.back_outer_layers_mut().has_plugging = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetBackPluggingMode`.
    pub fn back_plugging_mode(&self) -> PluggingMode {
        match self.padstack.back_outer_layers().has_plugging {
            Some(true) => PluggingMode::Plugged,
            Some(false) => PluggingMode::NotPlugged,
            None => PluggingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveBackPluggingMode`.
    pub fn resolve_back_plugging_mode(&self, ctx: &BoardCtx<'_>) -> PluggingMode {
        if self.back_plugging_mode() == PluggingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().plug_vias_back {
                return PluggingMode::Plugged;
            } else {
                return PluggingMode::NotPlugged;
            }
        }

        self.back_plugging_mode()
    }

    /// Port of `PCB_VIA::SetCappingMode`.
    pub fn set_capping_mode(&mut self, mode: CappingMode) {
        self.padstack.drill_mut().is_capped = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetCappingMode`.
    pub fn capping_mode(&self) -> CappingMode {
        match self.padstack.drill().is_capped {
            Some(true) => CappingMode::Capped,
            Some(false) => CappingMode::NotCapped,
            None => CappingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveCappingMode`.
    pub fn resolve_capping_mode(&self, ctx: &BoardCtx<'_>) -> CappingMode {
        if self.capping_mode() == CappingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().cap_vias {
                return CappingMode::Capped;
            } else {
                return CappingMode::NotCapped;
            }
        }

        self.capping_mode()
    }

    /// Port of `PCB_VIA::SetFillingMode`.
    pub fn set_filling_mode(&mut self, mode: FillingMode) {
        self.padstack.drill_mut().is_filled = tri_state(mode as i32);
    }

    /// Port of `PCB_VIA::GetFillingMode`.
    pub fn filling_mode(&self) -> FillingMode {
        match self.padstack.drill().is_filled {
            Some(true) => FillingMode::Filled,
            Some(false) => FillingMode::NotFilled,
            None => FillingMode::FromBoard,
        }
    }

    /// Port of `PCB_VIA::ResolveFillingMode`.
    pub fn resolve_filling_mode(&self, ctx: &BoardCtx<'_>) -> FillingMode {
        if self.filling_mode() == FillingMode::FromBoard {
            if self.has_board(ctx) && ctx.board.design_settings().fill_vias {
                return FillingMode::Filled;
            } else {
                return FillingMode::NotFilled;
            }
        }

        self.filling_mode()
    }

    /// Port of `PCB_VIA::IsTented( aLayer )`: the padstack's explicit value for the side, else
    /// the board setting, else true (no board). `wxCHECK_MSG` for a layer of neither side.
    pub fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        if !(is_front_layer(layer) || is_back_layer(layer)) {
            debug_assert!(false, "Invalid layer passed to IsTented");
            return true;
        }

        let front = is_front_layer(layer);

        if front {
            if let Some(v) = self.padstack.front_outer_layers().has_solder_mask {
                return v;
            }
        }

        if !front {
            if let Some(v) = self.padstack.back_outer_layers().has_solder_mask {
                return v;
            }
        }

        if self.has_board(ctx) {
            let bds = ctx.board.design_settings();
            return if front {
                bds.tent_vias_front
            } else {
                bds.tent_vias_back
            };
        }

        true
    }

    /// Port of `PCB_VIA::GetSolderMaskExpansion` (hides the `PCB_TRACK` body): the board's
    /// `m_SolderMaskExpansion`, 0 without a board.
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>) -> i32 {
        if self.has_board(ctx) {
            ctx.board.design_settings().solder_mask_expansion
        } else {
            0
        }
    }

    // ---- layers ----------------------------------------------------------------------------

    /// `PCB_VIA::GetLayer()`: `Padstack().Drill().start`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.padstack.drill().start
    }

    /// `PCB_VIA::SetLayer()`: `Padstack().Drill().start = aLayer`.
    pub fn set_layer(&mut self, _env: &MutEnv, layer: PcbLayerId) {
        self.padstack.drill_mut().start = layer;
    }

    /// Port of `PCB_VIA::GetLayerSet`: the copper layers of the span (every copper layer of
    /// the board for a through via), plus F_Mask / B_Mask on an untented outer side.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        let mut layermask = Lset::new();

        if self.padstack.drill().start < PcbLayerId::FCu {
            return layermask;
        }

        let board_copper_layer_count = board_item::board_copper_layer_count(self, ctx);

        if self.via_type == Viatype::Through {
            layermask = Lset::all_cu_mask_n(board_copper_layer_count);
        } else {
            let range = layer_range(
                self.padstack.drill().start,
                self.padstack.drill().end,
                board_copper_layer_count,
            );
            let mut cnt = board_copper_layer_count;

            // PCB_LAYER_IDs are numbered from front to back, this is top to bottom.
            for id in range {
                layermask.set(id);
                cnt -= 1;

                if cnt <= 0 {
                    break;
                }
            }
        }

        if !self.is_tented(ctx, PcbLayerId::FMask) && layermask.contains(PcbLayerId::FCu) {
            layermask.set(PcbLayerId::FMask);
        }

        if !self.is_tented(ctx, PcbLayerId::BMask) && layermask.contains(PcbLayerId::BCu) {
            layermask.set(PcbLayerId::BMask);
        }

        layermask
    }

    /// Port of `PCB_VIA::SetLayerSet`: a through via is reset to F_Cu - B_Cu; otherwise the
    /// first copper layer (not B_Cu) becomes the start and the last one up to B_Cu the end.
    pub fn set_layer_set(&mut self, _env: &MutEnv, layers: Lset) {
        // Vias do not use a LSET, just a top and bottom layer pair
        // So we need to set these 2 layers according to the allowed layers in aLayerSet
        // For via through, only F_Cu and B_Cu are allowed. aLayerSet is ignored
        if self.via_type == Viatype::Through {
            self.padstack.drill_mut().start = PcbLayerId::FCu;
            self.padstack.drill_mut().end = PcbLayerId::BCu;
            return;
        }

        // For blind buried vias, find the top and bottom layers
        let mut top_found = false;
        let mut bottom_found = false;
        let drill = self.padstack.drill_mut();

        layers.run_on_layers(|layer| {
            // tpo layer and bottom Layer are copper layers, so consider only copper layers
            if is_copper_layer(layer.0) {
                // The top layer is the first layer found in list and
                // cannot the B_Cu
                if !top_found && layer != PcbLayerId::BCu {
                    drill.start = layer;
                    top_found = true;
                }

                // The bottom layer is the last layer found in list or B_Cu
                if !bottom_found {
                    drill.end = layer;
                }

                if layer == PcbLayerId::BCu {
                    bottom_found = true;
                }
            }
        });
    }

    /// Port of `PCB_VIA::IsOnLayer`: a copper layer of the drill span, or F_Mask / B_Mask when
    /// the via reaches that outer layer and is not tented there.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let drill = self.padstack.drill();

        if is_copper_layer(layer.0) && LayerRange::contains_range(drill.start.0, drill.end.0, layer.0)
        {
            return true;
        }

        // Test for via on mask layers: a via on on a mask layer if not tented and if
        // it is on the corresponding external copper layer
        if layer == PcbLayerId::FMask {
            return drill.start == PcbLayerId::FCu && !self.is_tented(ctx, PcbLayerId::FMask);
        } else if layer == PcbLayerId::BMask {
            return drill.end == PcbLayerId::BCu && !self.is_tented(ctx, PcbLayerId::BMask);
        }

        false
    }

    /// Port of `PCB_VIA::HasValidLayerPair( aCopperLayerCount )`: both end layers exist in a
    /// stack of `copper_layer_count` layers.
    pub fn has_valid_layer_pair(&self, copper_layer_count: i32) -> bool {
        // return true if top and bottom layers are valid, depending on the copper layer count
        // aCopperLayerCount is expected >= 2
        let layer_id = copper_layer_count * 2;
        let drill = self.padstack.drill();

        if drill.start > PcbLayerId::BCu && drill.start.0 > layer_id {
            return false;
        }

        if drill.end > PcbLayerId::BCu && drill.end.0 > layer_id {
            return false;
        }

        true
    }

    /// Port of `PCB_VIA::FitsEnabledLayers` (header inline): `BOARD_ITEM::FitsEnabledLayers` and
    /// `HasValidLayerPair` (one enabled layer is not enough for a via).
    pub fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        board_item::fits_enabled_layers(self, ctx, enabled, copper_layer_count)
            && self.has_valid_layer_pair(copper_layer_count)
    }

    /// `GetViaType()`.
    pub fn via_type(&self) -> Viatype {
        self.via_type
    }

    /// Port of `SetViaType()` (header inline): set the type, then `SanitizeLayers()` (if someone
    /// updates a via to through, kick out any non-outer layers).
    pub fn set_via_type(&mut self, env: &MutEnv, via_type: Viatype) {
        self.via_type = via_type;

        // If someone updates a VIA to TH, we want to kick out any non-outer layers
        self.sanitize_layers(env);
    }

    /// Port of `PCB_VIA::SetLayerPair`. C++ then calls `BOARD::InvalidateClearanceCache`, which
    /// has no model-side counterpart (`board-model.md` 7: the engine revalidates by time
    /// stamp).
    pub fn set_layer_pair(&mut self, env: &MutEnv, top_layer: PcbLayerId, bottom_layer: PcbLayerId) {
        self.padstack.drill_mut().start = top_layer;
        self.padstack.drill_mut().end = bottom_layer;
        self.sanitize_layers(env);
    }

    /// Port of `PCB_VIA::SetTopLayer`: refused when it equals the end layer.
    pub fn set_top_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        // refuse invalid via
        if layer == self.padstack.drill().end {
            return;
        }

        self.padstack.drill_mut().start = layer;
        self.sanitize_layers(env);
    }

    /// Port of `PCB_VIA::SetBottomLayer`: refused when it equals the start layer.
    pub fn set_bottom_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        // refuse invalid via
        if layer == self.padstack.drill().start {
            return;
        }

        self.padstack.drill_mut().end = layer;
        self.sanitize_layers(env);
    }

    /// Port of `PCB_VIA::LayerPair`: (top, bottom) in copper Z order; F_Cu / B_Cu for a through
    /// via.
    pub fn layer_pair(&self) -> (PcbLayerId, PcbLayerId) {
        let mut t_layer = PcbLayerId::FCu;
        let mut b_layer = PcbLayerId::BCu;

        if self.via_type != Viatype::Through {
            b_layer = self.padstack.drill().end;
            t_layer = self.padstack.drill().start;

            if !is_copper_layer_lower_than(b_layer, t_layer) {
                std::mem::swap(&mut b_layer, &mut t_layer);
            }
        }

        (t_layer, b_layer)
    }

    /// Port of `PCB_VIA::TopLayer`: `Padstack().Drill().start`.
    pub fn top_layer(&self) -> PcbLayerId {
        self.padstack.drill().start
    }

    /// Port of `PCB_VIA::BottomLayer`: `Padstack().Drill().end`.
    pub fn bottom_layer(&self) -> PcbLayerId {
        self.padstack.drill().end
    }

    /// Port of `PCB_VIA::SanitizeLayers`: F_Cu - B_Cu for a through via, the end layers in
    /// copper Z order, backdrill end points off copper (or off the board's stack) dropped, and
    /// a backdrill without a must-cut layer cleared.
    pub fn sanitize_layers(&mut self, env: &MutEnv) {
        if self.via_type == Viatype::Through {
            self.padstack.drill_mut().start = PcbLayerId::FCu;
            self.padstack.drill_mut().end = PcbLayerId::BCu;
        }

        {
            let drill = self.padstack.drill_mut();

            if !is_copper_layer_lower_than(drill.end, drill.start) {
                std::mem::swap(&mut drill.end, &mut drill.start);
            }
        }

        // BoardCopperLayerCount(): 32 without a board (the detached MutEnv says so).
        let copper_count = env.layers.copper_layer_count;

        let sanitize_backdrill = |drill: &mut DrillProps| {
            if drill.start != PcbLayerId::UndefinedLayer && !is_copper_layer(drill.start.0) {
                drill.start = PcbLayerId::UndefinedLayer;
            }

            if drill.end != PcbLayerId::UndefinedLayer && !is_copper_layer(drill.end.0) {
                drill.end = PcbLayerId::UndefinedLayer;
            }

            if copper_count > 0 {
                let cu_mask = Lset::all_cu_mask_n(copper_count);

                if drill.start != PcbLayerId::UndefinedLayer && !cu_mask.contains(drill.start) {
                    drill.start = PcbLayerId::UndefinedLayer;
                }

                if drill.end != PcbLayerId::UndefinedLayer && !cu_mask.contains(drill.end) {
                    drill.end = PcbLayerId::UndefinedLayer;
                }
            }

            // A backdrill side with no must-cut layer does not exist.
            if drill.end == PcbLayerId::UndefinedLayer {
                drill.size = Vector2I::new(0, 0);
            }
        };

        sanitize_backdrill(self.padstack.secondary_drill_mut());
        sanitize_backdrill(self.padstack.tertiary_drill_mut());
    }

    /// Port of the static `PCB_VIA::ValidateViaParameters`: the first problem of a proposed
    /// via definition (sizes, end layers, backdrills), `None` when it is valid. The C++
    /// defaults are `std::nullopt` and a copper layer count of 0 (no stack check).
    #[allow(clippy::too_many_arguments)]
    pub fn validate_via_parameters(
        diameter: Option<i32>,
        primary_drill: Option<i32>,
        primary_start_layer: Option<PcbLayerId>,
        primary_end_layer: Option<PcbLayerId>,
        secondary_drill: Option<i32>,
        secondary_start_layer: Option<PcbLayerId>,
        secondary_end_layer: Option<PcbLayerId>,
        tertiary_drill: Option<i32>,
        tertiary_start_layer: Option<PcbLayerId>,
        tertiary_end_layer: Option<PcbLayerId>,
        copper_layer_count: i32,
    ) -> Option<ViaParameterError> {
        let err = |message: &str, field: ViaParameterField| {
            Some(ViaParameterError {
                message: message.to_string(),
                field,
            })
        };

        if diameter.is_some_and(|d| d < geometry_min_size()) {
            return err("Via diameter is too small.", ViaParameterField::Diameter);
        }

        if primary_drill.is_some_and(|d| d < geometry_min_size()) {
            return err("Via drill is too small.", ViaParameterField::Drill);
        }

        if diameter.is_some() && primary_drill.is_none() {
            return err("No via hole size defined.", ViaParameterField::Drill);
        }

        if primary_drill.is_some() && diameter.is_none() {
            return err("No via diameter defined.", ViaParameterField::Diameter);
        }

        if let (Some(d), Some(p)) = (diameter, primary_drill) {
            if d <= p {
                return err(
                    "Via hole size must be smaller than via diameter",
                    ViaParameterField::Drill,
                );
            }
        }

        let mut copper_mask: Option<Lset> = None;

        let mut validate_layer =
            |layer: Option<PcbLayerId>, field: ViaParameterField| -> Option<ViaParameterError> {
                let layer = layer?;

                if layer == PcbLayerId::UndefinedLayer {
                    return None;
                }

                if !is_copper_layer(layer.0) {
                    return err("Via layer must be a copper layer.", field);
                }

                if copper_layer_count > 0 {
                    let mask = copper_mask
                        .get_or_insert_with(|| Lset::all_cu_mask_n(copper_layer_count));

                    if !mask.contains(layer) {
                        return err("Via layer is outside the board stack.", field);
                    }
                }

                None
            };

        if let Some(e) = validate_layer(primary_start_layer, ViaParameterField::StartLayer) {
            return Some(e);
        }

        if let Some(e) = validate_layer(primary_end_layer, ViaParameterField::EndLayer) {
            return Some(e);
        }

        if let (Some(s), Some(e)) = (primary_start_layer, primary_end_layer) {
            if s == e {
                return err(
                    "Via start layer and end layer cannot be the same",
                    ViaParameterField::StartLayer,
                );
            }
        }

        if let Some(secondary) = secondary_drill {
            if secondary < primary_drill.unwrap_or(geometry_min_size()) {
                return err(
                    "Backdrill diameter is too small.",
                    ViaParameterField::SecondaryDrill,
                );
            }

            if let Some(e) =
                validate_layer(secondary_start_layer, ViaParameterField::SecondaryStartLayer)
            {
                return Some(e);
            }

            if let Some(e) =
                validate_layer(secondary_end_layer, ViaParameterField::SecondaryEndLayer)
            {
                return Some(e);
            }
        }

        if let Some(tertiary) = tertiary_drill {
            if tertiary < primary_drill.unwrap_or(geometry_min_size()) {
                return err(
                    "Tertiary backdrill diameter is too small.",
                    ViaParameterField::TertiaryDrill,
                );
            }

            if let Some(e) =
                validate_layer(tertiary_start_layer, ViaParameterField::TertiaryStartLayer)
            {
                return Some(e);
            }

            if let Some(e) = validate_layer(tertiary_end_layer, ViaParameterField::TertiaryEndLayer)
            {
                return Some(e);
            }
        }

        None
    }

    /// Port of `PCB_VIA::IsMicroVia`.
    pub fn is_micro_via(&self) -> bool {
        self.via_type == Viatype::Microvia
    }

    /// Port of `PCB_VIA::IsBlindVia`: a blind / buried via with exactly one outer end layer.
    pub fn is_blind_via(&self) -> bool {
        // We don't actually have an GUI or file tokens to differentiate these, so we have to
        // look at the layers.
        if self.via_type == Viatype::Blind || self.via_type == Viatype::Buried {
            let drill = self.padstack.drill();
            let start_outer = drill.start == PcbLayerId::FCu || drill.start == PcbLayerId::BCu;
            let end_outer = drill.end == PcbLayerId::FCu || drill.end == PcbLayerId::BCu;

            return start_outer ^ end_outer;
        }

        false
    }

    /// Port of `PCB_VIA::IsBuriedVia`: a blind / buried via with no outer end layer.
    pub fn is_buried_via(&self) -> bool {
        // We don't actually have an GUI or file tokens to differentiate these, so we have to
        // look at the layers.
        if self.via_type == Viatype::Blind || self.via_type == Viatype::Buried {
            let drill = self.padstack.drill();

            return drill.start != PcbLayerId::FCu
                && drill.start != PcbLayerId::BCu
                && drill.end != PcbLayerId::FCu
                && drill.end != PcbLayerId::BCu;
        }

        false
    }

    /// Port of `PCB_VIA::IsGhostLayer( aLayer )`: a padstack layer (FRONT_INNER_BACK key) on
    /// which a micro / blind / buried via does not exist.
    pub fn is_ghost_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        if matches!(
            self.via_type,
            Viatype::Microvia | Viatype::Blind | Viatype::Buried
        ) && self.padstack.mode() == PadstackMode::FrontInnerBack
        {
            match layer {
                PcbLayerId::FCu => {
                    if self.padstack.drill().start != PcbLayerId::FCu {
                        return true;
                    }
                }
                PcbLayerId::BCu => {
                    if self.padstack.drill().end != PcbLayerId::BCu {
                        return true;
                    }
                }
                Padstack::INNER_LAYERS => {
                    if self.has_board(ctx) && ctx.board.copper_layer_count() == 2 {
                        return true;
                    }
                }
                _ => {
                    debug_assert!(false, "Unsupported layer for FRONT_INNER_BACK");
                }
            }
        }

        false
    }

    // ---- flashing / zone layer overrides ---------------------------------------------------

    /// Port of `PCB_VIA::FlashLayer( const LSET& aLayers )`: flashed on any of the layers.
    pub fn flash_layer_set(&self, ctx: &BoardCtx<'_>, layers: &Lset) -> bool {
        layers.iter().any(|layer| self.flash_layer(ctx, layer))
    }

    /// Port of `PCB_VIA::FlashLayer( int aLayer )`: whether the via has a pad on `layer`
    /// (`UNDEFINED_LAYER`: the "normal" shape, true). Conditionally flashed copper layers ask
    /// the zone layer override, then `ctx.connectivity` (`None`: an empty CONNECTIVITY_DATA,
    /// not connected).
    pub fn flash_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        // Return the "normal" shape if the caller doesn't specify a particular layer
        if layer == PcbLayerId::UndefinedLayer {
            return true;
        }

        if !self.has_board(ctx) {
            return true;
        }

        if !self.is_on_layer(ctx, layer) {
            return false;
        }

        if !is_copper_layer(layer.0) {
            return true;
        }

        match self.padstack.unconnected_layer_mode() {
            UnconnectedLayerMode::KeepAll => return true,
            UnconnectedLayerMode::RemoveExceptStartAndEnd => {
                if layer == self.padstack.drill().start || layer == self.padstack.drill().end {
                    return true;
                }
                // Check for removal below
            }
            UnconnectedLayerMode::RemoveAll => {
                // Check for removal below
            }
            UnconnectedLayerMode::StartEndOnly => {
                return layer == self.padstack.drill().start || layer == self.padstack.drill().end;
            }
        }

        if self.zone_layer_override(layer) == ZoneLayerOverride::ForceFlashed {
            true
        } else {
            self.is_connected_on_layer(ctx, layer)
        }
    }

    /// `board->GetConnectivity()->IsConnectedOnLayer( this, aLayer, nonZoneTypes )`.
    fn is_connected_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        match ctx.connectivity {
            Some(connectivity) => self.with_this(ctx, |this| {
                connectivity.is_connected_on_layer(ctx, this, layer, &NON_ZONE_TYPES)
            }),
            None => false,
        }
    }

    /// Port of `PCB_VIA::ConditionallyFlashed( aLayer )` (header inline).
    pub fn conditionally_flashed(&self, layer: PcbLayerId) -> bool {
        match self.padstack.unconnected_layer_mode() {
            UnconnectedLayerMode::KeepAll => false,
            UnconnectedLayerMode::RemoveAll => true,
            UnconnectedLayerMode::RemoveExceptStartAndEnd | UnconnectedLayerMode::StartEndOnly => {
                layer != self.padstack.drill().start && layer != self.padstack.drill().end
            }
        }
    }

    /// `SetRemoveUnconnected()` (deprecated in C++): REMOVE_ALL or KEEP_ALL.
    pub fn set_remove_unconnected(&mut self, set: bool) {
        self.padstack.set_unconnected_layer_mode(if set {
            UnconnectedLayerMode::RemoveAll
        } else {
            UnconnectedLayerMode::KeepAll
        });
    }

    /// `GetRemoveUnconnected()`: any mode but KEEP_ALL.
    pub fn remove_unconnected(&self) -> bool {
        self.padstack.unconnected_layer_mode() != UnconnectedLayerMode::KeepAll
    }

    /// `SetKeepStartEnd()` (deprecated in C++): REMOVE_EXCEPT_START_AND_END or REMOVE_ALL.
    pub fn set_keep_start_end(&mut self, set: bool) {
        self.padstack.set_unconnected_layer_mode(if set {
            UnconnectedLayerMode::RemoveExceptStartAndEnd
        } else {
            UnconnectedLayerMode::RemoveAll
        });
    }

    /// `GetKeepStartEnd()`.
    pub fn keep_start_end(&self) -> bool {
        self.padstack.unconnected_layer_mode() == UnconnectedLayerMode::RemoveExceptStartAndEnd
    }

    /// Port of `PCB_VIA::ClearZoneLayerOverrides`: `ZLO_NONE` everywhere. A derived-state
    /// writer: reach it on a stored via through `Board::clear_zone_layer_overrides`.
    pub fn clear_zone_layer_overrides(&mut self) {
        for entry in self.zone_layer_overrides.iter_mut() {
            *entry = ZoneLayerOverride::None;
        }
    }

    /// Port of `PCB_VIA::GetZoneLayerOverride( aLayer )`: `ZLO_NONE` for a layer without a
    /// copper ordinal.
    pub fn zone_layer_override(&self, layer: PcbLayerId) -> ZoneLayerOverride {
        if !has_layer_ordinal(layer) {
            return ZoneLayerOverride::None;
        }

        self.zone_layer_overrides[copper_layer_to_ordinal(layer)]
    }

    /// Port of `PCB_VIA::SetZoneLayerOverride( aLayer, aOverride )`: ignored for a layer
    /// without a copper ordinal. Reach it on a stored via through
    /// `Board::set_zone_layer_override`.
    pub fn set_zone_layer_override(&mut self, layer: PcbLayerId, value: ZoneLayerOverride) {
        if !has_layer_ordinal(layer) {
            return;
        }

        self.zone_layer_overrides[copper_layer_to_ordinal(layer)] = value;
    }

    /// Port of `PCB_VIA::GetOutermostConnectedLayers`: (topmost, bottommost) connected layer,
    /// `UNDEFINED_LAYER` for none. As in C++ the loop runs over the raw layer ids from
    /// `TopLayer()` to `BottomLayer()` (non-copper ids in between included).
    pub fn outermost_connected_layers(&self, ctx: &BoardCtx<'_>) -> (PcbLayerId, PcbLayerId) {
        let mut topmost = PcbLayerId::UndefinedLayer;
        let mut bottommost = PcbLayerId::UndefinedLayer;

        let mut layer = self.top_layer().0;

        while layer <= self.bottom_layer().0 {
            let mut connected = false;

            if self.zone_layer_override(PcbLayerId(layer)) == ZoneLayerOverride::ForceFlashed {
                connected = true;
            } else if self.has_board(ctx) && self.is_connected_on_layer(ctx, PcbLayerId(layer)) {
                connected = true;
            }

            if connected {
                if topmost == PcbLayerId::UndefinedLayer {
                    topmost = to_layer_id(layer);
                }

                bottommost = to_layer_id(layer);
            }

            layer += 1;
        }

        (topmost, bottommost)
    }

    /// `GetIsFree()`.
    pub fn is_free(&self) -> bool {
        self.is_free
    }

    /// `SetIsFree()`.
    pub fn set_is_free(&mut self, free: bool) {
        self.is_free = free;
    }

    // ---- geometry --------------------------------------------------------------------------

    /// `PCB_VIA::GetPosition()`: `m_Start`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.track.start
    }

    /// `PCB_VIA::SetPosition()`: `m_Start = m_End = aPoint`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.track.start = pos;
        self.track.end = pos;
    }

    /// `PCB_TRACK::Move()` (inherited).
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.track.start += delta;
        self.track.end += delta;
    }

    /// `PCB_VIA::IsOnCopperLayer()`: always true.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `PCB_VIA::HasHole()`: always true.
    pub fn has_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `PCB_VIA::HasDrilledHole()`: through, blind or buried.
    pub fn has_drilled_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        matches!(
            self.via_type,
            Viatype::Through | Viatype::Blind | Viatype::Buried
        )
    }

    /// The `[pos, dim)` box of a round via of `diameter` (radius rounded up), shared by both
    /// `GetBoundingBox` overloads.
    fn round_box(&self, diameter: i32) -> Box2I {
        // via is round, this is its radius, rounded up
        let radius = (diameter + 1) / 2;

        let ymax = self.track.start.y + radius;
        let xmax = self.track.start.x + radius;

        let ymin = self.track.start.y - radius;
        let xmin = self.track.start.x - radius;

        // return a rectangle which is [pos,dim) in nature.  therefore the +1
        box2i_safe_from_int(
            Vector2I::new(xmin, ymin),
            Vector2L::new(xmax as i64 - xmin as i64 + 1, ymax as i64 - ymin as i64 + 1),
        )
    }

    /// Port of `PCB_VIA::GetBoundingBox()`: the largest pad of the non-ghost padstack layers or
    /// the largest hole.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut diameter = 0;

        self.padstack.for_each_unique_layer(|layer| {
            if self.is_ghost_layer(ctx, layer) {
                return;
            }

            diameter = diameter.max(self.width(layer));
        });

        diameter = diameter.max(self.padstack.max_hole_size());

        self.round_box(diameter)
    }

    /// Port of `PCB_VIA::GetBoundingBox( aLayer )`: the pad on `layer`, or the largest hole if
    /// the layer is backdrilled / post-machined.
    pub fn bounding_box_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Box2I {
        let mut diameter = self.width(layer);

        if self.is_backdrilled_or_post_machined(ctx, layer) {
            diameter = diameter.max(self.padstack.max_hole_size());
        }

        self.round_box(diameter)
    }

    /// Port of `PCB_VIA::HitTest( aPosition, aAccuracy )`: within the accuracy plus half the
    /// pad of any non-ghost padstack layer.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        let mut hit = false;

        self.padstack.for_each_unique_layer(|layer| {
            if hit {
                return;
            }

            if self.is_ghost_layer(ctx, layer) {
                return;
            }

            let max_dist = accuracy + (self.width(layer) / 2);

            // rel_pos is aPosition relative to m_Start (or the center of the via)
            let rel = pos - self.track.start;
            let rel_pos = Vector2D::new(rel.x as f64, rel.y as f64);
            let dist = rel_pos.x * rel_pos.x + rel_pos.y * rel_pos.y;

            if dist <= (max_dist as f64) * max_dist as f64 {
                hit = true;
            }
        });

        hit
    }

    /// Port of `PCB_VIA::HitTest( aRect, aContained, aAccuracy )`: per non-ghost padstack
    /// layer, the pad's box inside the inflated rectangle, or the pad circle intersecting it.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.inflate(accuracy);

        let mut hit = false;

        self.padstack.for_each_unique_layer(|layer| {
            if hit {
                return;
            }

            if self.is_ghost_layer(ctx, layer) {
                return;
            }

            let mut bx = Box2I::from_pos(self.track.start);
            bx.inflate(self.width(layer) / 2);

            if contained {
                hit = arect.contains_box(&bx);
            } else {
                hit = arect.intersects_circle(self.track.start, self.width(layer) / 2);
            }
        });

        hit
    }

    // ---- texts -----------------------------------------------------------------------------

    /// Port of `PCB_VIA::GetItemDescription`: "<Kind> via <net> on <top> - <bottom>" (English).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let kind = match self.via_type {
            Viatype::Blind => "Blind via",
            Viatype::Buried => "Buried via",
            Viatype::Microvia => "Micro via",
            _ => "Via",
        };

        format!(
            "{} {} on {}",
            kind,
            board_connected_item::netname_msg(self, ctx),
            self.layer_mask_describe(ctx)
        )
    }

    /// Port of `PCB_TRACK::GetFriendlyName (PCB_VIA)`: "Via".
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Via".to_string()
    }

    /// Port of `PCB_VIA::LayerMaskDescribe`: "<top> - <bottom>" with the board's layer names.
    pub fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String {
        let (top_layer, bottom_layer) = self.layer_pair();

        format!(
            "{} - {}",
            board_item::layer_name_of(self, ctx, top_layer),
            board_item::layer_name_of(self, ctx, bottom_layer)
        )
    }

    // ---- transforms ------------------------------------------------------------------------

    /// Port of `PCB_TRACK::Rotate (PCB_VIA)`: both ends rotated about `centre`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        self.track.rotate(env, centre, angle);
    }

    /// Port of `PCB_VIA::Flip`: mirror the ends about `centre`; a non-through via also gets
    /// the flipped layer pair (`GetFlippedLayer`, the board's flip table in `env`).
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        if dir == FlipDirection::LeftRight {
            self.track.start.x = centre.x - (self.track.start.x - centre.x);
            self.track.end.x = centre.x - (self.track.end.x - centre.x);
        } else {
            self.track.start.y = centre.y - (self.track.start.y - centre.y);
            self.track.end.y = centre.y - (self.track.end.y - centre.y);
        }

        if self.via_type != Viatype::Through {
            let (top_layer, bottom_layer) = self.layer_pair();
            let top_layer = env.layers.flip_layer(top_layer);
            let bottom_layer = env.layers.flip_layer(bottom_layer);
            self.set_layer_pair(env, top_layer, bottom_layer);
        }
    }

    /// Port of `PCB_TRACK::Mirror (PCB_VIA)`: `Mirror( MIRROR_AXIS( aCentre, aFlipDirection ) )`,
    /// both ends reflected.
    pub fn mirror(&mut self, _env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.track.mirror_seg(&mirror_axis(centre, dir));
    }

    /// Port of the static `PCB_VIA::CollectMicroviaColumns( aBoard )`: runs of microvias that
    /// land on one another, each ordered from its outermost hop down. A run of one is left
    /// out, so an entry is a stack whether or not a generator built it. Two microvias belong to
    /// one structure when the upper one lands on the lower one (hole overlap; exact
    /// concentricity is only the extreme case of that). Pointer sets become `TrackId` sets
    /// (membership only, never iterated).
    pub fn collect_microvia_columns(ctx: &BoardCtx<'_>) -> Vec<Vec<TrackId>> {
        let board = ctx.board;
        let mut ordinals: BTreeMap<i32, i32> = BTreeMap::new();
        let mut n = 0;

        for layer in layer_range(PcbLayerId::FCu, PcbLayerId::BCu, board.copper_layer_count()) {
            ordinals.insert(layer.0, n);
            n += 1;
        }

        let mut microvias: Vec<(TrackId, &PcbVia)> = Vec::new();

        for (id, track) in board.tracks() {
            let Track::Via(via) = track else {
                continue;
            };

            if via.via_type() != Viatype::Microvia {
                continue;
            }

            // A via on a single layer lands on nothing.
            if via.top_layer() == via.bottom_layer() {
                continue;
            }

            if ordinals.contains_key(&via.top_layer().0)
                && ordinals.contains_key(&via.bottom_layer().0)
            {
                microvias.push((id, via));
            }
        }

        let ord = |layer: PcbLayerId| ordinals.get(&layer.0).copied().unwrap_or(0);
        let upper = |via: &PcbVia| ord(via.top_layer()).min(ord(via.bottom_layer()));
        let lower = |via: &PcbVia| ord(via.top_layer()).max(ord(via.bottom_layer()));

        // Touching holes have no wall between them, so they count as one column too.
        let overlaps = |first: &PcbVia, second: &PcbVia| {
            let reach = (first.drill_value(ctx) + second.drill_value(ctx)) as f64 / 2.0;

            (first.track.start - second.track.start).euclidean_norm() as f64 <= reach
        };

        // Overlap needs the centres closer than the largest hole, so bucket the vias by landing
        // layer and position and only the neighbouring buckets have to be looked at.
        let mut cell_size = 1;

        for (_, via) in &microvias {
            cell_size = cell_size.max(via.drill_value(ctx));
        }

        let cell_of = |coord: i32| (coord as f64 / cell_size as f64).floor() as i32;

        let mut by_cell: BTreeMap<(i32, i32, i32), Vec<(TrackId, &PcbVia)>> = BTreeMap::new();

        for &(id, via) in &microvias {
            let pos = via.track.start;
            by_cell
                .entry((upper(via), cell_of(pos.x), cell_of(pos.y)))
                .or_default()
                .push((id, via));
        }

        // The via a given one lands on, if any.
        let below = |id: TrackId, via: &PcbVia| -> Option<(TrackId, &PcbVia)> {
            let pos = via.track.start;

            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(cell) =
                        by_cell.get(&(lower(via), cell_of(pos.x) + dx, cell_of(pos.y) + dy))
                    else {
                        continue;
                    };

                    for &(other_id, other) in cell {
                        if other_id != id && overlaps(via, other) {
                            return Some((other_id, other));
                        }
                    }
                }
            }

            None
        };

        let mut carried: BTreeSet<TrackId> = BTreeSet::new();

        for &(id, via) in &microvias {
            if let Some((under, _)) = below(id, via) {
                carried.insert(under);
            }
        }

        let mut columns: Vec<Vec<TrackId>> = Vec::new();
        let mut taken: BTreeSet<TrackId> = BTreeSet::new();

        for &(id, via) in &microvias {
            // Walk down from the top of each structure so every one is built once.
            if carried.contains(&id) {
                continue;
            }

            let mut column: Vec<TrackId> = Vec::new();
            let mut lands_on_another = false;
            let mut step = Some((id, via));

            while let Some((step_id, step_via)) = step {
                if !taken.insert(step_id) {
                    lands_on_another = true;
                    break;
                }

                column.push(step_id);

                if column.len() > ordinals.len() {
                    break;
                }

                step = below(step_id, step_via);
            }

            if column.len() > 1 || lands_on_another {
                columns.push(column);
            }
        }

        columns
    }
}

/// `FROM_BOARD` (0) -> unset, 1 -> true, 2 -> false: the shared encoding of the five
/// `*_MODE` enums and their `std::optional<bool>` padstack members.
fn tri_state(mode: i32) -> Option<bool> {
    match mode {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}
