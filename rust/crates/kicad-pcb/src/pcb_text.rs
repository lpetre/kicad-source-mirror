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

//! Port of `pcbnew/pcb_text.h` and `pcbnew/pcb_text.cpp` (`PCB_TEXT`): data, constructors and
//! delegation shims (package M0c), the `PCB_TEXT` overrides (stubs for package M10).

use kicad_core::layer_ids::is_back_layer;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId, TextAttributes};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, EdaOrientation, FlipDirection, ShapePolySet, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_text::EdaText;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, EdaTextItem, Flashing, Invalidate};

/// `PCB_TEXT` (`pcbnew/pcb_text.h`).
#[derive(Clone, Debug)]
pub struct PcbText {
    pub base: BoardItemBase,
    /// The `EDA_TEXT` part.
    pub text: EdaText,
    /// m_libTextAngle: text angle in the parent footprint's lib frame.
    pub(crate) lib_text_angle: EdaOrientation,
    pub(crate) knockout_cache: KeyedCache<PcbTextKnockoutCacheData>, // m_knockout_cache
}

/// The inputs and the result of `m_knockout_cache` (`PCB_TEXT::buildBoundingHull` /
/// `TransformTextToPolySet` with knockout).
#[derive(Clone, Debug)]
pub struct PcbTextKnockoutCacheData {
    pub text: String,
    pub text_attrs: TextAttributes,
    pub angle: EdaAngle,
    pub pos: Vector2I,
    pub cache: ShapePolySet,
}

impl Default for PcbText {
    /// `PCB_TEXT( BOARD_ITEM* aParent )` with a nil UUID: layer F_Cu (`BOARD_ITEM`), multiline
    /// allowed, lib text angle 0.
    fn default() -> Self {
        let mut text = EdaText::default();
        text.attributes.multiline = true; // SetMultilineAllowed( true )
        PcbText {
            base: BoardItemBase::default(),
            text,
            lib_text_angle: EdaOrientation::default(),
            knockout_cache: KeyedCache::default(),
        }
    }
}

impl PcbText {
    /// `new PCB_TEXT( BOARD_ITEM* aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut t = PcbText::default();
        t.base.uuid = Kiid::new(kiid_generator);
        t
    }

    /// `PCB_TEXT( FOOTPRINT* aParent )` with a nil UUID: keep upright, layer F_SilkS (B_SilkS on
    /// a back-side footprint). The C++ `SetTextPos( aParent->GetPosition() )` puts the
    /// library-frame position at the footprint origin, which is the default `lib_pos`.
    pub fn for_footprint(footprint_layer: Option<PcbLayerId>) -> Self {
        let mut t = PcbText {
            base: BoardItemBase::with_layer(PcbLayerId::FSilkS),
            ..PcbText::default()
        };
        t.text.attributes.keep_upright = true;
        if footprint_layer.is_some_and(is_back_layer) {
            t.base.layer = PcbLayerId::BSilkS;
        }
        t
    }

    /// `GetLibTextAngle()`: the stored library-frame angle.
    pub fn lib_text_angle(&self) -> EdaAngle {
        self.lib_text_angle.angle()
    }

    /// `SetLibTextAngle()`.
    pub fn set_lib_text_angle(&mut self, angle: EdaAngle) {
        self.lib_text_angle = EdaOrientation::new(angle);
    }

    /// `PCB_TEXT::TextHitTest( aPoint, aAccuracy )` (virtual in `EDA_TEXT`).
    pub fn text_hit_test(&self, ctx: &BoardCtx<'_>, point: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXT::TextHitTest", ctx, point, accuracy)
    }

    /// `PCB_TEXT::TextHitTest( aRect, aContains, aAccuracy )`.
    pub fn text_hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contains: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!(
            "PCB_TEXT::TextHitTest (rect)",
            ctx,
            rect,
            contains,
            accuracy
        )
    }

    /// `PCB_TEXT::GetTextTypeDescription()` (virtual, introduced by `PCB_TEXT`; `PCB_FIELD`
    /// overrides).
    pub fn text_type_description(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_TEXT::GetTextTypeDescription", ctx)
    }

    /// `PCB_TEXT::GetKnockoutCache( aFont, forResolvedText, aMaxError )`.
    pub fn knockout_cache(&self, ctx: &BoardCtx<'_>, resolved_text: &str) -> ShapePolySet {
        crate::port_pending!("PCB_TEXT::GetKnockoutCache", ctx, resolved_text)
    }

    /// `PCB_TEXT::TransformTextToPolySet( aBuffer, aClearance, aMaxError, aErrorLoc )`.
    pub fn transform_text_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        max_error: i32,
        error_loc: ErrorLoc,
    ) {
        crate::port_pending!(
            "PCB_TEXT::TransformTextToPolySet",
            ctx,
            buffer,
            clearance,
            max_error,
            error_loc
        )
    }

    /// `PCB_TEXT::KeepUpright()`.
    pub fn keep_upright(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_TEXT::KeepUpright", env)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbText {
    fn invalidate_caches(&mut self) {
        self.knockout_cache.clear();
        self.text.clear_bounding_box_cache();
    }
}

impl BoardItem for PcbText {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbTextT
    }
    fn class_name(&self) -> &'static str {
        "PCB_TEXT"
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbText::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbText::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbText::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbText::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbText::item_description(self, ctx, units, full)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbText::position(self, ctx)
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
        PcbText::transform_shape_to_polygon(
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

impl BoardItemMut for PcbText {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbText::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbText::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbText::move_by(self, env, delta)
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
        PcbText::on_footprint_rescaled(
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
        PcbText::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbText::rotate(self, env, centre, angle)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbText::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbText::style_from_settings(self, settings, check_side)
    }
}

impl EdaTextItem for PcbText {
    fn eda_text(&self) -> &EdaText {
        &self.text
    }
    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text
    }
    fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbText::draw_rotation(self, ctx)
    }
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        PcbText::font_metrics(self, ctx)
    }
    fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbText::offset(self, env, delta)
    }
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        PcbText::set_text_angle(self, env, angle)
    }
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        PcbText::set_text_size(self, env, size, enforce_min)
    }
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        PcbText::set_text_thickness(self, env, width)
    }
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        PcbText::shown_text(self, ctx, context, depth)
    }
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbText::text_angle(self, ctx)
    }
    fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbText::text_pos(self, ctx)
    }
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbText::text_size(self, ctx)
    }
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbText::text_thickness(self, ctx)
    }
}

// ---- the PCB_TEXT overrides (package M10) ------------------------------------------------

impl PcbText {
    /// `PCB_TEXT::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TEXT::GetBoundingBox", ctx)
    }

    /// `PCB_TEXT::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_TEXT::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TEXT::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXT::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_TEXT::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TEXT::HitTest", ctx, rect, contained, accuracy)
    }

    /// `PCB_TEXT::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TEXT::GetItemDescription", ctx, units, full)
    }

    /// `PCB_TEXT::GetPosition()`: `GetTextPos()`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text_pos(ctx)
    }

    /// `PCB_TEXT::TransformShapeToPolygon`.
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
            "PCB_TEXT::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_TEXT::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TEXT::Flip", env, centre, dir)
    }

    /// `PCB_TEXT::Mirror`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TEXT::Mirror", env, centre, dir)
    }

    /// `PCB_TEXT::Move()`: `Offset( aMoveVector )`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.offset(env, delta)
    }

    /// `PCB_TEXT::OnFootprintRescaled`.
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
            "PCB_TEXT::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `PCB_TEXT::OnFootprintTransformed`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_TEXT::OnFootprintTransformed", env)
    }

    /// `PCB_TEXT::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_TEXT::Rotate", env, centre, angle)
    }

    /// `PCB_TEXT::SetPosition()`: `SetTextPos( aPos )`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        self.set_text_pos(env, pos)
    }

    /// `PCB_TEXT::StyleFromSettings`.
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        crate::port_pending!("PCB_TEXT::StyleFromSettings", settings, check_side)
    }

    /// `PCB_TEXT::GetDrawRotation`.
    pub fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PCB_TEXT::GetDrawRotation", ctx)
    }

    /// `PCB_TEXT::getFontMetrics`.
    pub fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        crate::port_pending!("PCB_TEXT::getFontMetrics", ctx)
    }

    /// `PCB_TEXT::Offset`.
    pub fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PCB_TEXT::Offset", env, delta)
    }

    /// `PCB_TEXT::SetTextAngle`.
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        crate::port_pending!("PCB_TEXT::SetTextAngle", env, angle)
    }

    /// `PCB_TEXT::SetTextSize`.
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        crate::port_pending!("PCB_TEXT::SetTextSize", env, size, enforce_min)
    }

    /// `PCB_TEXT::SetTextThickness`.
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        crate::port_pending!("PCB_TEXT::SetTextThickness", env, width)
    }

    /// `PCB_TEXT::GetShownText`.
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        crate::port_pending!("PCB_TEXT::GetShownText", ctx, context, depth)
    }

    /// `PCB_TEXT::GetTextAngle`.
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PCB_TEXT::GetTextAngle", ctx)
    }

    /// `PCB_TEXT::GetTextPos`.
    pub fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXT::GetTextPos", ctx)
    }

    /// `PCB_TEXT::GetTextSize`.
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TEXT::GetTextSize", ctx)
    }

    /// `PCB_TEXT::GetTextThickness`.
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_TEXT::GetTextThickness", ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_TEXT::PCB_TEXT( BOARD_ITEM*, .. )` and `PCB_TEXT( FOOTPRINT*, .. )`
    /// (pcbnew/pcb_text.cpp).
    #[test]
    fn constructor_values() {
        let t = PcbText::default();
        assert_eq!(t.base.layer, PcbLayerId::FCu);
        assert!(t.text.attributes.multiline && !t.text.attributes.keep_upright);
        assert_eq!(t.lib_text_angle(), EdaAngle::from_degrees(0.0));
        assert_eq!(t.text.attributes.size, (1_270_000, 1_270_000));

        let f = PcbText::for_footprint(Some(PcbLayerId::FCu));
        assert_eq!(f.base.layer, PcbLayerId::FSilkS);
        assert!(f.text.attributes.keep_upright);
        assert_eq!(
            PcbText::for_footprint(Some(PcbLayerId::BCu)).base.layer,
            PcbLayerId::BSilkS
        );
        assert_eq!(PcbText::for_footprint(None).base.layer, PcbLayerId::FSilkS);

        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Text).typed::<PcbText>().unwrap();
        let mut kiids = KiidGenerator::default();
        let mut text = PcbText::new(&mut kiids);
        text.text.set_lib_pos(Vector2I::new(1_000, 2_000));
        text.set_lib_text_angle(EdaAngle::from_degrees(370.0));
        fx.set_child(id, text);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].text.lib_pos(), Vector2I::new(1_000, 2_000));
        assert_eq!(fx.board[id].lib_text_angle(), EdaAngle::from_degrees(10.0));
    }
}
