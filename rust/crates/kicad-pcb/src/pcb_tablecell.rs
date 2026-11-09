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

//! Port of `pcbnew/pcb_tablecell.h` and `pcbnew/pcb_tablecell.cpp` (`PCB_TABLECELL :
//! PCB_TEXTBOX`): data, constructor and delegation shims (package M0c), the overrides (stubs
//! for package M11).
//!
//! A cell is its own `EDA_TEXT` / `EDA_SHAPE` host (it overrides `GetShownText` and
//! `IsFilledForHitTesting`), so `ItemRef::as_eda_text()` and the generic mixin functions
//! dispatch to the cell's overrides and not to the embedded `PcbTextbox`. An inherited
//! `PCB_TEXTBOX` / `PCB_SHAPE` body calls the embedded `PcbTextbox` where it reaches no virtual
//! that `PCB_TABLECELL` overrides; the others (text shapes, hit tests) are stubs here.

use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::EdaShape;
use crate::eda_text::EdaText;
use crate::ids::ItemId;
use crate::item::{BoardItemBase, ConnectedItemBase, SharedShape};
use crate::pcb_textbox::PcbTextbox;
use crate::traits::{
    BoardConnectedItem, BoardItem, BoardItemMut, EdaShapeItem, EdaTextItem, Flashing, Invalidate,
};

/// `PCB_TABLECELL : PCB_TEXTBOX`. `parent == ParentRef::Item(table)`.
#[derive(Clone, Debug)]
pub struct PcbTablecell {
    /// The `PCB_TEXTBOX` part.
    pub textbox: PcbTextbox,
    pub col_span: i32, // m_colSpan
    pub row_span: i32, // m_rowSpan
}

impl Default for PcbTablecell {
    /// `PCB_TABLECELL( aParentTable )` with a nil UUID and the parent of a board-level, front
    /// table (not mirrored; the C++ mirrors the text for a flipped footprint or a back-layer
    /// table: the caller sets `attributes.mirrored`): spans 1, the rectangle `INT_MAX / 2`
    /// wide and high (`SetRectangleHeight` / `SetRectangleWidth`, whose `SetEnd` syncs the
    /// lib-frame end too).
    fn default() -> Self {
        let mut textbox = PcbTextbox::default();
        let half = i32::MAX / 2;
        let shape = &mut textbox.shape;
        shape.shape.rectangle_height = half as i64;
        shape.shape.rectangle_width = half as i64;
        shape.shape.end = Vector2I::new(half, half);
        shape.lib.end = Vector2I::new(half, half);
        PcbTablecell {
            textbox,
            col_span: 1,
            row_span: 1,
        }
    }
}

impl PcbTablecell {
    /// `new PCB_TABLECELL( aParentTable )`: the constructor values with a fresh UUID;
    /// `mirrored` = the parent table's side (`SetMirrored`).
    pub fn new(kiid_generator: &mut KiidGenerator, mirrored: bool) -> Self {
        let mut cell = PcbTablecell::default();
        cell.textbox.shape.base.item.uuid = Kiid::new(kiid_generator);
        cell.textbox.text.attributes.mirrored = mirrored;
        cell
    }

    /// `PCB_TABLECELL::GetParentGroup()`: the group of the parent table (a cell is never a
    /// group member itself).
    pub fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> {
        let table = ctx.board.get(self.textbox.shape.base.item.parent.item()?)?;
        table.as_dyn().parent_group(ctx)
    }

    /// `GetColSpan()`.
    pub fn col_span(&self) -> i32 {
        self.col_span
    }

    /// `SetColSpan()`.
    pub fn set_col_span(&mut self, span: i32) {
        self.col_span = span;
    }

    /// `GetRowSpan()`.
    pub fn row_span(&self) -> i32 {
        self.row_span
    }

    /// `SetRowSpan()`.
    pub fn set_row_span(&mut self, span: i32) {
        self.row_span = span;
    }

    /// `GetRow()`: from the cell's index in the parent table.
    pub fn row(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TABLECELL::GetRow", ctx)
    }

    /// `GetColumn()`.
    pub fn column(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TABLECELL::GetColumn", ctx)
    }

    /// `GetAddr()`.
    pub fn addr(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_TABLECELL::GetAddr", ctx)
    }

    /// `GetUnwrappedShownText( aContext, aDepth )`.
    pub fn unwrapped_shown_text(
        &self,
        ctx: &BoardCtx<'_>,
        context: ResolutionContext,
        depth: i32,
    ) -> String {
        crate::port_pending!("PCB_TABLECELL::GetUnwrappedShownText", ctx, context, depth)
    }

    /// `GetRowHeight()`.
    pub fn row_height(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TABLECELL::GetRowHeight", ctx)
    }

    /// `GetColumnWidth()`.
    pub fn column_width(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TABLECELL::GetColumnWidth", ctx)
    }

    /// `PCB_TEXTBOX::GetCorners (PCB_TABLECELL)`.
    pub fn corners(&self, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        self.textbox.corners(ctx)
    }

    /// `PCB_TEXTBOX::syncLibCoords (PCB_TABLECELL)`.
    pub fn sync_lib_coords(&mut self, env: &MutEnv) {
        self.textbox.sync_lib_coords(env)
    }

    /// `PCB_SHAPE::SetStart (PCB_TABLECELL)`.
    pub fn set_start(&mut self, env: &MutEnv, start: Vector2I) {
        self.textbox.set_start(env, start)
    }

    /// `PCB_SHAPE::SetEnd (PCB_TABLECELL)`.
    pub fn set_end(&mut self, env: &MutEnv, end: Vector2I) {
        self.textbox.set_end(env, end)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbTablecell {
    fn invalidate_caches(&mut self) {
        self.textbox.invalidate_caches();
    }
}

impl BoardItem for PcbTablecell {
    fn base(&self) -> &BoardItemBase {
        &self.textbox.shape.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.textbox.shape.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbTablecellT
    }
    fn class_name(&self) -> &'static str {
        "PCB_TABLECELL"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.textbox.shape.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.textbox.shape.base)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbTablecell::bounding_box(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTablecell::center(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbTablecell::effective_shape(self, ctx, layer, flash, usage)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbTablecell::friendly_name(self, ctx)
    }
    fn has_line_stroke(&self) -> bool {
        PcbTablecell::has_line_stroke(self)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbTablecell::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbTablecell::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbTablecell::is_on_layer(self, ctx, layer)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbTablecell::item_description(self, ctx, units, full)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbTablecell::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbTablecell::layer_set(self, ctx)
    }
    fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> {
        PcbTablecell::parent_group(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTablecell::position(self, ctx)
    }
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        PcbTablecell::stroke(self, ctx)
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
        PcbTablecell::transform_shape_to_poly_set(
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
        PcbTablecell::transform_shape_to_polygon(
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

impl BoardConnectedItem for PcbTablecell {
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbTablecell::is_connected(self, ctx)
    }
}

impl BoardItemMut for PcbTablecell {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTablecell::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTablecell::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbTablecell::move_by(self, env, delta)
    }
    fn normalize(&mut self) {
        PcbTablecell::normalize(self)
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
        PcbTablecell::on_footprint_rescaled(
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
        PcbTablecell::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbTablecell::rotate(self, env, centre, angle)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        PcbTablecell::set_layer(self, env, layer)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbTablecell::set_layer_set(self, env, layers)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbTablecell::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbTablecell::style_from_settings(self, settings, check_side)
    }
}

impl EdaTextItem for PcbTablecell {
    fn eda_text(&self) -> &EdaText {
        &self.textbox.text
    }
    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.textbox.text
    }
    fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTablecell::draw_pos(self, ctx)
    }
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        PcbTablecell::font_metrics(self, ctx)
    }
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        PcbTablecell::set_text_angle(self, env, angle)
    }
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        PcbTablecell::set_text_size(self, env, size, enforce_min)
    }
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        PcbTablecell::set_text_thickness(self, env, width)
    }
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        PcbTablecell::shown_text(self, ctx, context, depth)
    }
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbTablecell::text_angle(self, ctx)
    }
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTablecell::text_size(self, ctx)
    }
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbTablecell::text_thickness(self, ctx)
    }
}

impl EdaShapeItem for PcbTablecell {
    fn eda_shape(&self) -> &EdaShape {
        &self.textbox.shape.shape
    }
    fn eda_shape_mut(&mut self) -> &mut EdaShape {
        &mut self.textbox.shape.shape
    }
    fn bot_right(&self) -> Vector2I {
        PcbTablecell::bot_right(self)
    }
    fn corners_in_sequence(&self, angle: EdaAngle) -> Vec<Vector2I> {
        PcbTablecell::corners_in_sequence(self, angle)
    }
    fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        PcbTablecell::hatching_knockouts(self, ctx)
    }
    fn is_filled_for_hit_testing(&self) -> bool {
        PcbTablecell::is_filled_for_hit_testing(self)
    }
    fn is_proxy_item(&self) -> bool {
        PcbTablecell::is_proxy_item(self)
    }
    fn shape_draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbTablecell::shape_draw_rotation(self, ctx)
    }
    fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbTablecell::shape_max_error(self, ctx)
    }
    fn top_left(&self) -> Vector2I {
        PcbTablecell::top_left(self)
    }
    fn width(&self) -> i32 {
        PcbTablecell::width(self)
    }
}

// ---- overrides and inherited PCB_TEXTBOX / PCB_SHAPE bodies (package M11) ----------------

impl PcbTablecell {
    /// `PCB_TEXTBOX::GetBoundingBox (PCB_TABLECELL)`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TEXTBOX::GetBoundingBox (PCB_TABLECELL)", ctx)
    }

    /// `PCB_SHAPE::GetCenter`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.textbox.center(ctx)
    }

    /// `PCB_TEXTBOX::GetEffectiveShape (PCB_TABLECELL)`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!(
            "PCB_TEXTBOX::GetEffectiveShape (PCB_TABLECELL)",
            ctx,
            layer,
            flash,
            usage
        )
    }

    /// `PCB_TABLECELL::GetFriendlyName()`: "Table Cell" (untranslated).
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Table Cell".to_string()
    }

    /// `PCB_SHAPE::HasLineStroke`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn has_line_stroke(&self) -> bool {
        self.textbox.has_line_stroke()
    }

    /// `PCB_TEXTBOX::HitTest (PCB_TABLECELL)`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXTBOX::HitTest (PCB_TABLECELL)", ctx, pos, accuracy)
    }

    /// `PCB_TEXTBOX::HitTest (PCB_TABLECELL)`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!(
            "PCB_TEXTBOX::HitTest (PCB_TABLECELL)",
            ctx,
            rect,
            contained,
            accuracy
        )
    }

    /// `PCB_SHAPE::IsOnLayer`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.textbox.is_on_layer(ctx, layer)
    }

    /// `PCB_TABLECELL::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TABLECELL::GetItemDescription", ctx, units, full)
    }

    /// `PCB_SHAPE::GetLayer`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.textbox.layer(ctx)
    }

    /// `PCB_SHAPE::GetLayerSet`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        self.textbox.layer_set(ctx)
    }

    /// `PCB_SHAPE::GetPosition`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.textbox.position(ctx)
    }

    /// `PCB_SHAPE::GetStroke`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        self.textbox.stroke(ctx)
    }

    /// `PCB_SHAPE::TransformShapeToPolySet (PCB_TABLECELL)`.
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
            "PCB_SHAPE::TransformShapeToPolySet (PCB_TABLECELL)",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc
        )
    }

    /// `PCB_TEXTBOX::TransformShapeToPolygon (PCB_TABLECELL)`.
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
            "PCB_TEXTBOX::TransformShapeToPolygon (PCB_TABLECELL)",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_SHAPE::IsConnected`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        self.textbox.is_connected(ctx)
    }

    /// `PCB_TEXTBOX::Flip`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.textbox.flip(env, centre, dir)
    }

    /// `PCB_TEXTBOX::Mirror`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.textbox.mirror(env, centre, dir)
    }

    /// `PCB_TEXTBOX::Move`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.textbox.move_by(env, delta)
    }

    /// `PCB_SHAPE::Normalize`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn normalize(&mut self) {
        self.textbox.normalize()
    }

    /// `PCB_TEXTBOX::OnFootprintRescaled`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        self.textbox.on_footprint_rescaled(
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate,
        )
    }

    /// `PCB_TEXTBOX::OnFootprintTransformed`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        self.textbox.on_footprint_transformed(env)
    }

    /// `PCB_TEXTBOX::Rotate`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        self.textbox.rotate(env, centre, angle)
    }

    /// `PCB_SHAPE::SetLayer`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        self.textbox.set_layer(env, layer)
    }

    /// `PCB_SHAPE::SetLayerSet`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        self.textbox.set_layer_set(env, layers)
    }

    /// `PCB_SHAPE::SetPosition`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        self.textbox.set_position(env, pos)
    }

    /// `PCB_TEXTBOX::StyleFromSettings`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        self.textbox.style_from_settings(settings, check_side)
    }

    /// `PCB_TEXTBOX::GetDrawPos (PCB_TABLECELL)`.
    pub fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXTBOX::GetDrawPos (PCB_TABLECELL)", ctx)
    }

    /// `PCB_TEXTBOX::getFontMetrics`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        self.textbox.font_metrics(ctx)
    }

    /// `PCB_TEXTBOX::SetTextAngle`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.textbox.set_text_angle(env, angle)
    }

    /// `PCB_TEXTBOX::SetTextSize`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        self.textbox.set_text_size(env, size, enforce_min)
    }

    /// `PCB_TEXTBOX::SetTextThickness`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        self.textbox.set_text_thickness(env, width)
    }

    /// `PCB_TABLECELL::GetShownText`.
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        crate::port_pending!("PCB_TABLECELL::GetShownText", ctx, context, depth)
    }

    /// `PCB_TEXTBOX::GetTextAngle`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.textbox.text_angle(ctx)
    }

    /// `PCB_TEXTBOX::GetTextSize`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.textbox.text_size(ctx)
    }

    /// `PCB_TEXTBOX::GetTextThickness`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.textbox.text_thickness(ctx)
    }

    /// `PCB_TEXTBOX::GetBotRight`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn bot_right(&self) -> Vector2I {
        self.textbox.bot_right()
    }

    /// `PCB_TEXTBOX::GetCornersInSequence`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn corners_in_sequence(&self, angle: EdaAngle) -> Vec<Vector2I> {
        self.textbox.corners_in_sequence(angle)
    }

    /// `PCB_SHAPE::getHatchingKnockouts (PCB_TABLECELL)`.
    pub fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        crate::port_pending!("PCB_SHAPE::getHatchingKnockouts (PCB_TABLECELL)", ctx)
    }

    /// `PCB_TABLECELL::IsFilledForHitTesting()`: true.
    pub fn is_filled_for_hit_testing(&self) -> bool {
        true
    }

    /// `PCB_SHAPE::IsProxyItem`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn is_proxy_item(&self) -> bool {
        self.textbox.is_proxy_item()
    }

    /// `PCB_TEXTBOX::getDrawRotation`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn shape_draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.textbox.shape_draw_rotation(ctx)
    }

    /// `PCB_SHAPE::getMaxError`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.textbox.shape_max_error(ctx)
    }

    /// `PCB_TEXTBOX::GetTopLeft`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn top_left(&self) -> Vector2I {
        self.textbox.top_left()
    }

    /// `PCB_SHAPE::GetWidth`, inherited by `PCB_TABLECELL` (the base body reaches no virtual that `PCB_TABLECELL` overrides).
    pub fn width(&self) -> i32 {
        self.textbox.width()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::pcb_table::PcbTable;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_TABLECELL::PCB_TABLECELL( aParentTable )` (pcbnew/pcb_tablecell.cpp).
    #[test]
    fn constructor_values() {
        let c = PcbTablecell::default();
        assert_eq!((c.col_span, c.row_span), (1, 1));
        let half = i32::MAX / 2;
        assert_eq!(c.textbox.shape.shape.rectangle_height, half as i64);
        assert_eq!(c.textbox.shape.shape.end, Vector2I::new(half, half));
        assert!(!c.textbox.text.attributes.mirrored);
        assert!(c.is_filled_for_hit_testing());
        assert_eq!(c.textbox.shape.base.item.layer, PcbLayerId::FCu);

        let mut fx = rotated_flipped_scaled();
        let table = fx.child(ItemKind::Table).typed::<PcbTable>().unwrap();
        let mut kiids = KiidGenerator::default();
        let cell = fx.add_cell(table, PcbTablecell::new(&mut kiids, true));
        fx.board.check_invariants().unwrap();
        assert!(fx.board[cell].textbox.text.attributes.mirrored);
        assert_eq!(fx.board[table].cells.len(), 2);
    }
}
