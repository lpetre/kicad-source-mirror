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

//! Port of `pcbnew/pcb_textbox.h` and `pcbnew/pcb_textbox.cpp` (`PCB_TEXTBOX : PCB_SHAPE,
//! EDA_TEXT`): data, constructors and delegation shims (package M0c), the overrides (stubs for
//! package M11).
//!
//! `PcbTextbox` is its own host of both mixins and has a shim for every virtual that
//! `PCB_TEXTBOX` or `PCB_SHAPE` overrides. An inherited `PCB_SHAPE` body calls the embedded
//! `PcbShape` where it reaches no virtual that `PCB_TEXTBOX` overrides; the others are stubs.

use kicad_core::StrokeParams;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{GrTextHAlignT, GrTextVAlignT, KicadT, Kiid, KiidGenerator, Lset, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{
    Box2I, EdaAngle, EdaOrientation, FlipDirection, ShapePolySet, Vector2I, ki_round,
};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::{EdaShape, ShapeT};
use crate::eda_text::EdaText;
use crate::item::{BoardItemBase, ConnectedItemBase, SharedShape};
use crate::pcb_shape::PcbShape;
use crate::traits::{
    BoardConnectedItem, BoardItem, BoardItemMut, EdaShapeItem, EdaTextItem, Flashing, Invalidate,
};

/// `PCB_TEXTBOX : PCB_SHAPE, EDA_TEXT`.
#[derive(Clone, Debug)]
pub struct PcbTextbox {
    /// The box (`PCB_SHAPE` part: a rectangle or, rotated, a polygon).
    pub shape: PcbShape,
    /// The `EDA_TEXT` part.
    pub text: EdaText,
    /// m_borderEnabled: controls drawing the border (as defined by the stroke members).
    pub border_enabled: bool,
    pub margin_left: i32,   // m_marginLeft
    pub margin_top: i32,    // m_marginTop
    pub margin_right: i32,  // m_marginRight
    pub margin_bottom: i32, // m_marginBottom
    /// m_libTextAngle: text angle in the parent footprint's lib frame.
    pub(crate) lib_text_angle: EdaOrientation,
}

impl Default for PcbTextbox {
    /// `PCB_TEXTBOX( nullptr, PCB_TEXTBOX_T )` with a nil UUID.
    fn default() -> Self {
        PcbTextbox::with_shape(PcbShape::with_shape(ShapeT::Rectangle))
    }
}

impl PcbTextbox {
    /// `PCB_TEXTBOX( aParent, aType )` around the given `PCB_SHAPE( aParent, aType,
    /// SHAPE_T::RECTANGLE )`: left / centre justified, multiline, border on, every margin
    /// `GetLegacyTextMargin()` (computed as for an item without a parent footprint:
    /// `KiROUND( stroke width / 2.0 ) + KiROUND( text height * 0.75 )`).
    pub(crate) fn with_shape(shape: PcbShape) -> Self {
        let mut text = EdaText::default();
        text.attributes.halign = GrTextHAlignT::Left;
        text.attributes.valign = GrTextVAlignT::Center;
        text.attributes.multiline = true;
        let stroke_width = shape.shape.stroke.width().max(0);
        let default_margin: i32 =
            ki_round(stroke_width as f64 / 2.0) + ki_round(text.attributes.size.1 as f64 * 0.75);
        PcbTextbox {
            shape,
            text,
            border_enabled: true,
            margin_left: default_margin,
            margin_top: default_margin,
            margin_right: default_margin,
            margin_bottom: default_margin,
            lib_text_angle: EdaOrientation::default(),
        }
    }

    /// `new PCB_TEXTBOX( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut t = PcbTextbox::default();
        t.shape.base.item.uuid = Kiid::new(kiid_generator);
        t
    }

    /// `GetLibTextAngle()`.
    pub fn lib_text_angle(&self) -> EdaAngle {
        self.lib_text_angle.angle()
    }

    /// `SetLibTextAngle()`.
    pub fn set_lib_text_angle(&mut self, angle: EdaAngle) {
        self.lib_text_angle = EdaOrientation::new(angle);
    }

    /// `GetLegacyTextMargin()`: `KiROUND( GetStroke().GetWidth() / 2.0 ) +
    /// KiROUND( GetTextSize().y * 0.75 )`.
    pub fn legacy_text_margin(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TEXTBOX::GetLegacyTextMargin", ctx)
    }

    /// `GetMinSize()`.
    pub fn min_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetMinSize", ctx)
    }

    /// `GetDrawPos( aIsFlipped )`.
    pub fn draw_pos_flipped(&self, ctx: &BoardCtx<'_>, is_flipped: bool) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetDrawPos (flipped)", ctx, is_flipped)
    }

    /// `GetPolygonalBoxShape()`.
    pub fn polygonal_box_shape(&self, ctx: &BoardCtx<'_>) -> PcbShape {
        crate::port_pending!("PCB_TEXTBOX::GetPolygonalBoxShape", ctx)
    }

    /// `PCB_TEXTBOX::GetCorners()` (overrides the virtual introduced by `PCB_SHAPE`).
    pub fn corners(&self, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        crate::port_pending!("PCB_TEXTBOX::GetCorners", ctx)
    }

    /// `PCB_TEXTBOX::syncLibCoords()`.
    pub fn sync_lib_coords(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_TEXTBOX::syncLibCoords", env)
    }

    /// `PCB_TEXTBOX::SetShape()`.
    pub fn set_shape(&mut self, env: &MutEnv, shape: ShapeT) {
        crate::port_pending!("PCB_TEXTBOX::SetShape", env, shape)
    }

    /// `PCB_TEXTBOX::SetTop()`.
    pub fn set_top(&mut self, env: &MutEnv, value: i32) {
        crate::port_pending!("PCB_TEXTBOX::SetTop", env, value)
    }

    /// `PCB_TEXTBOX::SetLeft()`.
    pub fn set_left(&mut self, env: &MutEnv, value: i32) {
        crate::port_pending!("PCB_TEXTBOX::SetLeft", env, value)
    }

    /// `PCB_TEXTBOX::SetRight()`.
    pub fn set_right(&mut self, env: &MutEnv, value: i32) {
        crate::port_pending!("PCB_TEXTBOX::SetRight", env, value)
    }

    /// `PCB_TEXTBOX::SetBottom()`.
    pub fn set_bottom(&mut self, env: &MutEnv, value: i32) {
        crate::port_pending!("PCB_TEXTBOX::SetBottom", env, value)
    }

    /// `PCB_SHAPE::SetStart (PCB_TEXTBOX)`: ends with the textbox's `syncLibCoords()`.
    pub fn set_start(&mut self, env: &MutEnv, start: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetStart (PCB_TEXTBOX)", env, start)
    }

    /// `PCB_SHAPE::SetEnd (PCB_TEXTBOX)`.
    pub fn set_end(&mut self, env: &MutEnv, end: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetEnd (PCB_TEXTBOX)", env, end)
    }

    /// `PCB_SHAPE::SetStroke (PCB_TEXTBOX)`.
    pub fn set_stroke(&mut self, env: &MutEnv, stroke: StrokeParams) {
        crate::port_pending!("PCB_SHAPE::SetStroke (PCB_TEXTBOX)", env, stroke)
    }

    /// `PCB_SHAPE::SetPolyShape (PCB_TEXTBOX)`.
    pub fn set_poly_shape(&mut self, env: &MutEnv, poly: ShapePolySet) {
        crate::port_pending!("PCB_SHAPE::SetPolyShape (PCB_TEXTBOX)", env, poly)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbTextbox {
    fn invalidate_caches(&mut self) {
        self.shape.invalidate_caches();
        self.text.clear_bounding_box_cache();
    }
}

impl BoardItem for PcbTextbox {
    fn base(&self) -> &BoardItemBase {
        &self.shape.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.shape.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbTextboxT
    }
    fn class_name(&self) -> &'static str {
        "PCB_TEXTBOX"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.shape.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.shape.base)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbTextbox::bounding_box(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::center(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbTextbox::effective_shape(self, ctx, layer, flash, usage)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbTextbox::friendly_name(self, ctx)
    }
    fn has_line_stroke(&self) -> bool {
        PcbTextbox::has_line_stroke(self)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbTextbox::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbTextbox::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbTextbox::is_on_layer(self, ctx, layer)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbTextbox::item_description(self, ctx, units, full)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbTextbox::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbTextbox::layer_set(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::position(self, ctx)
    }
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        PcbTextbox::stroke(self, ctx)
    }
    fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        PcbTextbox::transform_shape_to_poly_set(
            self, ctx, buffer, layer, clearance, error, error_loc,
        )
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
        PcbTextbox::transform_shape_to_polygon(
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
}

impl BoardConnectedItem for PcbTextbox {
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbTextbox::is_connected(self, ctx)
    }
}

impl BoardItemMut for PcbTextbox {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTextbox::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTextbox::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbTextbox::move_by(self, env, delta)
    }
    fn normalize(&mut self) {
        PcbTextbox::normalize(self)
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
        PcbTextbox::on_footprint_rescaled(
            self,
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate,
        )
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        PcbTextbox::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbTextbox::rotate(self, env, centre, angle)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        PcbTextbox::set_layer(self, env, layer)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbTextbox::set_layer_set(self, env, layers)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbTextbox::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbTextbox::style_from_settings(self, settings, check_side)
    }
}

impl EdaTextItem for PcbTextbox {
    fn eda_text(&self) -> &EdaText {
        &self.text
    }
    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text
    }
    fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::draw_pos(self, ctx)
    }
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        PcbTextbox::font_metrics(self, ctx)
    }
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        PcbTextbox::set_text_angle(self, env, angle)
    }
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        PcbTextbox::set_text_size(self, env, size, enforce_min)
    }
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        PcbTextbox::set_text_thickness(self, env, width)
    }
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        PcbTextbox::shown_text(self, ctx, context, depth)
    }
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbTextbox::text_angle(self, ctx)
    }
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::text_size(self, ctx)
    }
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbTextbox::text_thickness(self, ctx)
    }
}

impl EdaShapeItem for PcbTextbox {
    fn eda_shape(&self) -> &EdaShape {
        &self.shape.shape
    }
    fn eda_shape_mut(&mut self) -> &mut EdaShape {
        &mut self.shape.shape
    }
    fn bot_right(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::bot_right(self, ctx)
    }
    fn corners_in_sequence(&self, angle: EdaAngle) -> Vec<Vector2I> {
        PcbTextbox::corners_in_sequence(self, angle)
    }
    fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        PcbTextbox::hatching_knockouts(self, ctx)
    }
    fn is_proxy_item(&self) -> bool {
        PcbTextbox::is_proxy_item(self)
    }
    fn shape_draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbTextbox::shape_draw_rotation(self, ctx)
    }
    fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbTextbox::shape_max_error(self, ctx)
    }
    fn top_left(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTextbox::top_left(self, ctx)
    }
    fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbTextbox::width(self, ctx)
    }
}

// ---- overrides and inherited PCB_SHAPE bodies (package M11) ------------------------------

impl PcbTextbox {
    /// `PCB_TEXTBOX::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TEXTBOX::GetBoundingBox", ctx)
    }

    /// `PCB_SHAPE::GetCenter (PCB_TEXTBOX)`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_SHAPE::GetCenter (PCB_TEXTBOX)", ctx)
    }

    /// `PCB_TEXTBOX::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_TEXTBOX::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TEXTBOX::GetFriendlyName()`: "Text Box" (untranslated).
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Text Box".to_string()
    }

    /// `PCB_SHAPE::HasLineStroke`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn has_line_stroke(&self) -> bool {
        self.shape.has_line_stroke()
    }

    /// `PCB_TEXTBOX::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXTBOX::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_TEXTBOX::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TEXTBOX::HitTest", ctx, rect, contained, accuracy)
    }

    /// `PCB_SHAPE::IsOnLayer`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.shape.is_on_layer(ctx, layer)
    }

    /// `PCB_TEXTBOX::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TEXTBOX::GetItemDescription", ctx, units, full)
    }

    /// `PCB_SHAPE::GetLayer`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.shape.layer(ctx)
    }

    /// `PCB_SHAPE::GetLayerSet`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        self.shape.layer_set(ctx)
    }

    /// `PCB_SHAPE::GetPosition`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.shape.position(ctx)
    }

    /// `PCB_SHAPE::GetStroke`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        self.shape.stroke(ctx)
    }

    /// `PCB_SHAPE::TransformShapeToPolySet (PCB_TEXTBOX)`.
    pub fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        crate::port_pending!(
            "PCB_SHAPE::TransformShapeToPolySet (PCB_TEXTBOX)",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc
        )
    }

    /// `PCB_TEXTBOX::TransformShapeToPolygon`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PCB_TEXTBOX::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_SHAPE::IsConnected`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        self.shape.is_connected(ctx)
    }

    /// `PCB_TEXTBOX::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TEXTBOX::Flip", env, centre, dir)
    }

    /// `PCB_TEXTBOX::Mirror`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TEXTBOX::Mirror", env, centre, dir)
    }

    /// `PCB_TEXTBOX::Move`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PCB_TEXTBOX::Move", env, delta)
    }

    /// `PCB_SHAPE::Normalize (PCB_TEXTBOX)`.
    pub fn normalize(&mut self) {
        crate::port_pending!("PCB_SHAPE::Normalize (PCB_TEXTBOX)")
    }

    /// `PCB_TEXTBOX::OnFootprintRescaled`.
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
            "PCB_TEXTBOX::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `PCB_TEXTBOX::OnFootprintTransformed`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_TEXTBOX::OnFootprintTransformed", env)
    }

    /// `PCB_TEXTBOX::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_TEXTBOX::Rotate", env, centre, angle)
    }

    /// `PCB_SHAPE::SetLayer`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        self.shape.set_layer(env, layer)
    }

    /// `PCB_SHAPE::SetLayerSet`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        self.shape.set_layer_set(env, layers)
    }

    /// `PCB_SHAPE::SetPosition (PCB_TEXTBOX)`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetPosition (PCB_TEXTBOX)", env, pos)
    }

    /// `PCB_TEXTBOX::StyleFromSettings`.
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        crate::port_pending!("PCB_TEXTBOX::StyleFromSettings", settings, check_side)
    }

    /// `PCB_TEXTBOX::GetDrawPos`.
    pub fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetDrawPos", ctx)
    }

    /// `PCB_TEXTBOX::getFontMetrics`.
    pub fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        crate::port_pending!("PCB_TEXTBOX::getFontMetrics", ctx)
    }

    /// `PCB_TEXTBOX::SetTextAngle`.
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        crate::port_pending!("PCB_TEXTBOX::SetTextAngle", env, angle)
    }

    /// `PCB_TEXTBOX::SetTextSize`.
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        crate::port_pending!("PCB_TEXTBOX::SetTextSize", env, size, enforce_min)
    }

    /// `PCB_TEXTBOX::SetTextThickness`.
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        crate::port_pending!("PCB_TEXTBOX::SetTextThickness", env, width)
    }

    /// `PCB_TEXTBOX::GetShownText`.
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        crate::port_pending!("PCB_TEXTBOX::GetShownText", ctx, context, depth)
    }

    /// `PCB_TEXTBOX::GetTextAngle`.
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PCB_TEXTBOX::GetTextAngle", ctx)
    }

    /// `PCB_TEXTBOX::GetTextSize`.
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetTextSize", ctx)
    }

    /// `PCB_TEXTBOX::GetTextThickness`.
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TEXTBOX::GetTextThickness", ctx)
    }

    /// `PCB_TEXTBOX::GetBotRight`.
    pub fn bot_right(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetBotRight", ctx)
    }

    /// `PCB_TEXTBOX::GetCornersInSequence`.
    pub fn corners_in_sequence(&self, angle: EdaAngle) -> Vec<Vector2I> {
        crate::port_pending!("PCB_TEXTBOX::GetCornersInSequence", angle)
    }

    /// `PCB_SHAPE::getHatchingKnockouts (PCB_TEXTBOX)`.
    pub fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        crate::port_pending!("PCB_SHAPE::getHatchingKnockouts (PCB_TEXTBOX)", ctx)
    }

    /// `PCB_SHAPE::IsProxyItem`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn is_proxy_item(&self) -> bool {
        self.shape.is_proxy_item()
    }

    /// `PCB_TEXTBOX::getDrawRotation()`: `GetDrawRotation()`.
    pub fn shape_draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        EdaTextItem::draw_rotation(self, ctx)
    }

    /// `PCB_SHAPE::getMaxError`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.shape.shape_max_error(ctx)
    }

    /// `PCB_TEXTBOX::GetTopLeft`.
    pub fn top_left(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetTopLeft", ctx)
    }

    /// `PCB_SHAPE::GetWidth`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.shape.width(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eda_shape::FillT;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_TEXTBOX::PCB_TEXTBOX( aParent, aType )` (pcbnew/pcb_textbox.cpp).
    #[test]
    fn constructor_values() {
        let t = PcbTextbox::default();
        assert_eq!(t.shape.shape.shape, ShapeT::Rectangle);
        assert_eq!(t.shape.lib.shape, ShapeT::Rectangle);
        assert_eq!(t.shape.shape.fill, FillT::NoFill);
        assert_eq!(t.text.attributes.halign, GrTextHAlignT::Left);
        assert_eq!(t.text.attributes.valign, GrTextVAlignT::Center);
        assert!(t.text.attributes.multiline && t.border_enabled);
        // KiROUND( 100000 / 2.0 ) + KiROUND( 1270000 * 0.75 )
        assert_eq!(t.margin_left, 1_002_500);
        assert_eq!(
            (t.margin_top, t.margin_right, t.margin_bottom),
            (1_002_500, 1_002_500, 1_002_500)
        );
        assert_eq!(t.lib_text_angle(), EdaAngle::from_degrees(0.0));
        assert_eq!(t.shape.base.item.layer, PcbLayerId::FCu);

        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Textbox).typed::<PcbTextbox>().unwrap();
        let mut kiids = KiidGenerator::default();
        let mut tb = PcbTextbox::new(&mut kiids);
        tb.border_enabled = false;
        fx.set_child(id, tb);
        fx.board.check_invariants().unwrap();
        assert!(!fx.board[id].border_enabled);
    }
}
