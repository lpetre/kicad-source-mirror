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

//! The virtual interfaces of the board item hierarchy (`board-model.md` sections 6.2 and 6.2a):
//! ports of the `EDA_ITEM` / `BOARD_ITEM` / `BOARD_CONNECTED_ITEM` virtuals
//! (`include/eda_item.h`, `include/board_item.h`, `pcbnew/board_connected_item.h`) and of the
//! `EDA_TEXT` / `EDA_SHAPE` mixin virtuals (`include/eda_text.h`, `include/eda_shape.h`).
//!
//! Default bodies are the C++ base-class bodies. A base body that only returns a constant is
//! written here; every other base body is a generic function in the module that ports its C++
//! file (`board_item.rs`, `board_connected_item.rs`, `eda_text.rs`, `eda_shape/mod.rs`), which
//! the default forwards to. That is what lets the owning package port it without touching this
//! file, and what lets an override call its base class explicitly (C++
//! `BOARD_ITEM::FitsEnabledLayers( .. ) && ..` is `board_item::fits_enabled_layers( self, .. )
//! && ..`): a trait default cannot be called from the type that overrides it.
//!
//! Rule (binding, 6.2): an `impl Trait for T` block contains no behaviour. For each virtual the
//! C++ class (or one of its C++ bases below the class the trait ports) overrides it has a
//! one-line shim forwarding to an inherent method of `T` with the same name and signature; a
//! missing inherent method makes the shim recurse, which the crate-level
//! `#![deny(unconditional_recursion)]` turns into a build error.
//!
//! This file is **frozen** (M0a review): the traits are the virtual interfaces only.
//! Non-virtual members of `EDA_TEXT` / `EDA_SHAPE` that call the virtuals are generic
//! functions over the host in `eda_text.rs` / `eda_shape/`, with method syntax through the
//! blanket-implemented extension traits `EdaTextExt` / `EdaShapeExt` defined there (so they
//! cannot be overridden, exactly like a non-virtual C++ member). A virtual that turns out to
//! be missing here is added by the orchestrator, never by a package.

use std::sync::Arc;

use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, Lset, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, FlipDirection, ShapePolySet, ShapeSegment, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::EdaShape;
use crate::eda_text::EdaText;
use crate::ids::{ItemId, NetCode};
use crate::item::{BoardItemBase, ConnectedItemBase, RecurseMode, SharedShape};
use crate::item_ref::{AnyItem, ItemRef};
use crate::netclass::Netclass;
use crate::{board_connected_item, board_item, eda_shape, eda_text};

pub use kicad_core::Flashing;

/// Every item clears its derived caches here; `Board` calls it before handing out `&mut`.
pub trait Invalidate {
    /// Drop every per-item derived cache (the C++ `SetDirty()` / cache resets).
    fn invalidate_caches(&mut self);
}

/// The Rust `this` as a `BOARD_ITEM*` for a value that may live outside the arena (rev 2.3).
/// A generic base-class body (`B: BoardItem`) cannot name the `AnyItem` variant of its host;
/// this supertrait can. Implemented by `board_items!` for every arena type and by hand for
/// `NetinfoItem` and the three structs of the track family, whose `AnyItem` form is the
/// tagged `Track` (they hand over a copy of themselves, `base.id` included).
/// `board_item::with_this` is the entry point.
pub trait AsAnyItem {
    /// Calls `f` exactly once with the item as an `AnyItem`.
    fn with_any_item(&self, f: &mut dyn FnMut(AnyItem<'_>));
}

/// The `EDA_ITEM` + `BOARD_ITEM` virtuals (read-only part).
pub trait BoardItem: Invalidate + AsAnyItem {
    // ---- identity / structure (no context) ----

    /// The `EDA_ITEM` / `BOARD_ITEM` data members.
    fn base(&self) -> &BoardItemBase;

    /// Mutable data members.
    fn base_mut(&mut self) -> &mut BoardItemBase;

    /// `Type()`.
    fn type_(&self) -> KicadT;

    /// `GetClass()`.
    fn class_name(&self) -> &'static str;

    /// The `BOARD_CONNECTED_ITEM` part (`dynamic_cast<BOARD_CONNECTED_ITEM*>`).
    fn connected(&self) -> Option<&ConnectedItemBase> {
        None
    }

    /// Mutable `BOARD_CONNECTED_ITEM` part.
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        None
    }

    /// `m_Uuid`.
    fn uuid(&self) -> &Kiid {
        &self.base().uuid
    }

    /// `GetParentGroup()` as a handle (`m_group`; `PCB_TABLECELL` answers with its table's
    /// group).
    fn parent_group(&self, _ctx: &BoardCtx<'_>) -> Option<ItemId> {
        self.base().group
    }

    // ---- layers ----

    /// `GetLayer()`.
    fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.base().layer
    }

    /// `GetLayerSet()`: `LSET( { m_layer } )`, empty for `UNDEFINED_LAYER`.
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        board_item::layer_set(self, ctx)
    }

    /// `IsOnLayer( aLayer )`: `m_layer == aLayer`.
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        board_item::is_on_layer(self, ctx, layer)
    }

    /// `IsOnCopperLayer()`.
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        board_item::is_on_copper_layer(self, ctx)
    }

    /// `IsSideSpecific()`.
    fn is_side_specific(&self, ctx: &BoardCtx<'_>) -> bool {
        board_item::is_side_specific(self, ctx)
    }

    /// `IsLayerAgnostic()`.
    fn is_layer_agnostic(&self) -> bool {
        false
    }

    /// `FitsEnabledLayers( aEnabledLayers, aCopperLayerCount )`.
    fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        board_item::fits_enabled_layers(self, ctx, enabled, copper_layer_count)
    }

    // ---- geometry (board frame) ----

    /// `GetPosition()` (`EDA_ITEM`: `VECTOR2I()`).
    fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        Vector2I::default()
    }

    /// `GetCenter()`: `GetBoundingBox().GetCenter()`.
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.bounding_box(ctx).center()
    }

    /// `GetBoundingBox()` (`EDA_ITEM`: a zero-sized box at the origin).
    fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        Box2I::new(Vector2I::new(0, 0), Vector2I::new(0, 0))
    }

    /// `GetEffectiveShape( aLayer, aFlash, aUsage )`; `PcbLayerId::UndefinedLayer` = all layers.
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        board_item::effective_shape(self, ctx, layer, flash, usage)
    }

    /// `GetEffectiveHoleShape`; `None` where C++ returns null.
    fn effective_hole_shape(
        &self,
        _ctx: &BoardCtx<'_>,
        _layer: PcbLayerId,
        _usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        None
    }

    /// `TransformShapeToPolygon`.
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
        board_item::transform_shape_to_polygon(
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

    /// `TransformShapeToPolySet`.
    fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        board_item::transform_shape_to_poly_set(
            self, ctx, buffer, layer, clearance, error, error_loc,
        )
    }

    /// `HasHole()`.
    fn has_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        false
    }

    /// `HasDrilledHole()`.
    fn has_drilled_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        false
    }

    /// `IsTented( aLayer )`.
    fn is_tented(&self, _ctx: &BoardCtx<'_>, _layer: PcbLayerId) -> bool {
        false
    }

    /// `HasLineStroke()`.
    fn has_line_stroke(&self) -> bool {
        false
    }

    /// `GetStroke()`.
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        board_item::stroke(self, ctx)
    }

    /// `HitTest( aPosition, aAccuracy )`.
    fn hit_test(&self, _ctx: &BoardCtx<'_>, _pos: Vector2I, _accuracy: i32) -> bool {
        false
    }

    /// `HitTest( aRect, aContained, aAccuracy )`.
    fn hit_test_rect(
        &self,
        _ctx: &BoardCtx<'_>,
        _rect: &Box2I,
        _contained: bool,
        _accuracy: i32,
    ) -> bool {
        false
    }

    // ---- state ----

    /// `IsLocked()`: parent group, FPHOLDER, `m_isLocked`; PAD overrides.
    fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool {
        board_item::is_locked(self, ctx)
    }

    /// `IsKnockout()`.
    fn is_knockout(&self) -> bool {
        self.base().is_knockout
    }

    // ---- structure ----

    /// `RunOnChildren( aFunction, aMode )`.
    fn run_on_children(
        &self,
        _ctx: &BoardCtx<'_>,
        _mode: RecurseMode,
        _f: &mut dyn FnMut(ItemRef<'_>),
    ) {
    }

    // ---- text used verbatim in DRC reports ----

    /// `GetItemDescription( aUnitsProvider, aFull )`.
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        board_item::item_description(self, ctx, units, full)
    }

    /// `GetFriendlyName()`.
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        board_item::friendly_name(self, ctx)
    }

    /// `LayerMaskDescribe()`.
    fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String {
        board_item::layer_mask_describe(self, ctx)
    }
}

/// The `BOARD_CONNECTED_ITEM` virtuals.
pub trait BoardConnectedItem: BoardItem {
    /// `GetNetCode()`.
    fn net_code(&self) -> NetCode {
        self.connected().map_or(NetCode::ORPHANED, |c| c.net)
    }

    /// `SetNet( aNetInfo )` (virtual; rev 2.3): `m_netinfo = aNetInfo`, with
    /// `NetCode::ORPHANED` standing for the null pointer (`GetNetCode()` then answers -1).
    /// Unlike `SetNetCode` (`Board::set_item_net`) there is no copper check and no board
    /// lookup. ZONE overrides it: a copper-thieving zone drops the net. Stored items are
    /// reached through `Board::set_item_netinfo`.
    fn set_net(&mut self, net: NetCode) {
        board_connected_item::set_net(self, net)
    }

    /// `IsConnected()` (ZONE: not a rule area; PCB_SHAPE: copper and not in a footprint).
    fn is_connected(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `GetEffectiveNetClass()` (`board-model.md` 10).
    fn effective_netclass<'a>(&self, ctx: &BoardCtx<'a>) -> &'a Netclass {
        board_connected_item::effective_netclass(self, ctx)
    }

    /// `GetOwnClearance( aLayer, aSource )`: `ctx.rules.cached_own_clearance`, 0 without rules.
    fn own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        board_connected_item::own_clearance(self, ctx, layer, want_source)
    }

    /// `GetLocalClearance()`.
    fn local_clearance(&self) -> Option<i32> {
        None
    }

    /// `GetClearanceOverrides( aSource )`.
    fn clearance_overrides(
        &self,
        _ctx: &BoardCtx<'_>,
        _want_source: bool,
    ) -> Option<(i32, Option<String>)> {
        None
    }
}

/// Mutating virtuals of leaf items. Containers (footprint, group, generator, table) cascade
/// through `Board` methods instead (`board-model.md` 8.3).
pub trait BoardItemMut: BoardItem {
    /// `SetPosition()` (`EDA_ITEM`: does nothing).
    fn set_position(&mut self, _env: &MutEnv, _pos: Vector2I) {}

    /// `Move()` (`BOARD_ITEM`: `wxFAIL_MSG`, does nothing).
    fn move_by(&mut self, _env: &MutEnv, _delta: Vector2I) {}

    /// `Rotate()` (`BOARD_ITEM`: error message box, does nothing).
    fn rotate(&mut self, _env: &MutEnv, _centre: Vector2I, _angle: EdaAngle) {}

    /// `Flip()` (`BOARD_ITEM`: error message box, does nothing).
    fn flip(&mut self, _env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {}

    /// `Mirror( aCentre, aFlipDirection )` (`BOARD_ITEM`: error message box, does nothing).
    fn mirror(&mut self, _env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {}

    /// `SetLayer()`: `m_layer = aLayer`. (`BOARD_CONNECTED_ITEM::SetLayer` additionally calls
    /// `InvalidateClearanceCache`, which has no model-side counterpart: `board-model.md` 7.)
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        board_item::set_layer(self, env, layer)
    }

    /// `SetLayerSet()`: a single-layer set becomes `SetLayer`; anything else is
    /// `UNIMPLEMENTED_FOR` in C++ (ignored).
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        board_item::set_layer_set(self, env, layers)
    }

    /// `Normalize()`.
    fn normalize(&mut self) {}

    /// `OnFootprintTransformed()`.
    fn on_footprint_transformed(&mut self, _env: &MutEnv) {}

    /// `OnFootprintRescaled( aRatioX, aRatioY, aLinearFactor, aAnchor, aParentRotate )`.
    fn on_footprint_rescaled(
        &mut self,
        _env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
    }

    /// `StyleFromSettings( settings, aCheckSide )`. Takes the mutator snapshot (rev 2.3): the
    /// `PCB_TEXT` / `PCB_TEXTBOX` overrides call the frame-converting `SetTextSize` /
    /// `SetTextThickness`, test `GetParentFootprint()` and ask `BOARD::IsBackLayer`.
    fn style_from_settings(
        &mut self,
        _env: &MutEnv,
        _settings: &BoardDesignSettings,
        _check_side: bool,
    ) {
    }
}

/// Virtual interface of `EDA_TEXT`. Hosts: `PcbText` (`PcbField`, `PcbDimension` and the
/// barcode text delegate to it), `PcbTextbox` (`PcbTablecell` delegates). Defaults = the
/// `EDA_TEXT` bodies (in `eda_text.rs`). Never call an algorithm on the bare `EdaText`.
pub trait EdaTextItem {
    /// The `EDA_TEXT` data members.
    fn eda_text(&self) -> &EdaText;

    /// Mutable `EDA_TEXT` data members.
    fn eda_text_mut(&mut self) -> &mut EdaText;

    /// `GetTextPos()` (stored value).
    fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        eda_text::text_pos(self, ctx)
    }

    /// `GetTextSize()`.
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        eda_text::text_size(self, ctx)
    }

    /// `GetTextThickness()`.
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        eda_text::text_thickness(self, ctx)
    }

    /// `GetTextAngle()`.
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        eda_text::text_angle(self, ctx)
    }

    /// `GetDrawPos()`.
    fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text_pos(ctx)
    }

    /// `GetDrawRotation()`.
    fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.text_angle(ctx)
    }

    /// `GetShownText( aContext, aDepth )` (this tree: a `RESOLUTION_CONTEXT`, not the
    /// `aAllowExtraText` flag of KiCad 9).
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        eda_text::shown_text(self, ctx, context, depth)
    }

    /// `getFontMetrics()` (`EDA_TEXT`: `KIFONT::METRICS::Default()`).
    fn font_metrics(&self, _ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        kicad_font::Metrics::default_metrics()
    }

    /// `SetTextPos()`.
    fn set_text_pos(&mut self, env: &MutEnv, pos: Vector2I) {
        eda_text::set_text_pos(self, env, pos)
    }

    /// `SetTextSize( aNewSize, aEnforceMinTextSize )`.
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        eda_text::set_text_size(self, env, size, enforce_min)
    }

    /// `SetTextThickness()`.
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        eda_text::set_text_thickness(self, env, width)
    }

    /// `SetTextAngle()`.
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        eda_text::set_text_angle(self, env, angle)
    }

    /// `Offset()`.
    fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        eda_text::offset(self, env, delta)
    }

    // Non-virtual EDA_TEXT members that call the virtuals (GetTextBox,
    // GetEffectiveTextPenWidth, GetEffectiveTextShape, GetLinePositions, GetInterline, Compare,
    // ...) are not here: they are generic functions in eda_text.rs with method syntax through
    // `eda_text::EdaTextExt` (see the module comment).
}

/// Virtual interface of `EDA_SHAPE`. Hosts: `PcbShape` (`PcbTextbox`, `PcbTablecell` delegate,
/// with the `PCB_TEXTBOX` overrides). Defaults = the `EDA_SHAPE` bodies (in
/// `eda_shape/mod.rs`).
pub trait EdaShapeItem {
    /// The `EDA_SHAPE` data members.
    fn eda_shape(&self) -> &EdaShape;

    /// Mutable `EDA_SHAPE` data members.
    fn eda_shape_mut(&mut self) -> &mut EdaShape;

    /// `GetWidth()` (virtual). Takes the context because `PCB_SHAPE::GetWidth` (inherited by
    /// `PCB_TEXTBOX` / `PCB_TABLECELL`) scales the stored library width by the parent
    /// footprint's linear scale (`fpScaleLinear( transformFp() )`). The stored value is
    /// `eda_shape().stroke.width()`.
    fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        eda_shape::width(self, ctx)
    }

    /// `GetEffectiveWidth()` (virtual; no PCB class overrides it): `GetWidth()`.
    fn effective_width(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.width(ctx)
    }

    /// `GetHatchLineWidth()` (virtual; no PCB class overrides it): `GetEffectiveWidth()`.
    fn hatch_line_width(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.effective_width(ctx)
    }

    /// `GetHatchLineSpacing()` (virtual; no PCB class overrides it):
    /// `GetHatchLineWidth() * 10`.
    fn hatch_line_spacing(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.hatch_line_width(ctx) * 10
    }

    /// `IsProxyItem()`.
    fn is_proxy_item(&self) -> bool {
        eda_shape::is_proxy_item(self)
    }

    /// `IsFilledForHitTesting()` (`EDA_SHAPE`: `IsSolidFill()`; PCB_TABLECELL: true).
    fn is_filled_for_hit_testing(&self) -> bool {
        eda_shape::is_filled_for_hit_testing(self)
    }

    /// `GetTopLeft()`. Takes the context because the `PCB_TEXTBOX` override picks the corner
    /// by `GetDrawRotation()`, i.e. `GetTextAngle()`, which adds the parent footprint's
    /// orientation.
    fn top_left(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        eda_shape::top_left(self, ctx)
    }

    /// `GetBotRight()` (see `top_left`).
    fn bot_right(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        eda_shape::bot_right(self, ctx)
    }

    /// `GetCornersInSequence( aAngle )`. Takes the context because the `EDA_SHAPE` body reads
    /// `getBoundingBox()`, i.e. the virtual `GetWidth()` (rev 2.3).
    fn corners_in_sequence(&self, ctx: &BoardCtx<'_>, angle: EdaAngle) -> Vec<Vector2I> {
        eda_shape::corners_in_sequence(self, ctx, angle)
    }

    /// `getDrawRotation()`.
    fn shape_draw_rotation(&self, _ctx: &BoardCtx<'_>) -> EdaAngle {
        ANGLE_0
    }

    /// `getMaxError()` (PCB_SHAPE: the board's value).
    fn shape_max_error(&self, _ctx: &BoardCtx<'_>) -> i32 {
        100
    }

    /// `getHatchingKnockouts()` (PCB_SHAPE reads other items).
    fn hatching_knockouts(&self, _ctx: &BoardCtx<'_>) -> ShapePolySet {
        ShapePolySet::default()
    }

    // Non-virtual EDA_SHAPE members that call the virtuals (makeEffectiveShapes, hatching,
    // getBoundingBox, hitTest, TransformShapeToPolygon, ...) are not here: they are generic
    // functions in eda_shape/{mod,geometry,shapes}.rs with method syntax through
    // `eda_shape::EdaShapeExt` (see the module comment).
}
