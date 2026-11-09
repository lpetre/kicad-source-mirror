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

//! Port of `pcbnew/pcb_field.h` and `pcbnew/pcb_field.cpp` (`PCB_FIELD : PCB_TEXT`): data,
//! constructors and delegation shims (package M0c), the overrides (stubs for package M10).
//!
//! `PcbField` is its own `EDA_TEXT` host (`board-model.md` 6.2a): it has a shim for every
//! virtual that `PCB_FIELD` or `PCB_TEXT` overrides. An inherited `PCB_TEXT` body calls the
//! embedded `PcbText` where it reaches no virtual that `PCB_FIELD` overrides (`GetShownText`,
//! `GetItemDescription`, `GetTextTypeDescription`); the others are stubs here.

use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_text::EdaText;
use crate::item::{BoardItemBase, SharedShape};
use crate::pcb_text::PcbText;
use crate::traits::{BoardItem, BoardItemMut, EdaTextItem, Flashing, Invalidate};

/// `FIELD_T` (`include/template_fieldnames.h`); the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum FieldT {
    /// The field id hasn't been set yet (a user field).
    #[default]
    User = 0,
    Reference,
    Value,
    Footprint,
    Datasheet,
    Description,
    IntersheetRefs,
    SheetName,
    SheetFilename,
    SheetUser,
    SheetRefSuffix,
}

/// `PCB_FIELD : PCB_TEXT`.
#[derive(Clone, Debug)]
pub struct PcbField {
    /// The `PCB_TEXT` part.
    pub text: PcbText,
    pub id: FieldT,       // m_id
    /// m_ordinal: sort order for non-mandatory fields.
    pub ordinal: i32,
    pub name: String,     // m_name
    pub private: bool,    // m_private
}

impl Default for PcbField {
    /// `PCB_FIELD( FOOTPRINT* aParent = nullptr, FIELD_T::USER, "" )` with a nil UUID: the
    /// footprint text constructor (keep upright, F_SilkS), ordinal 0 (the C++ asks the parent
    /// footprint's `GetNextFieldOrdinal()` for a user field; without a parent that is the
    /// footprint package's business, M19), not private.
    fn default() -> Self {
        PcbField {
            text: PcbText::for_footprint(None),
            id: FieldT::User,
            ordinal: 0,
            name: String::new(),
            private: false,
        }
    }
}

impl PcbField {
    /// `new PCB_FIELD( aParent, aFieldId, aName )` with a fresh UUID; `footprint_layer` is the
    /// parent footprint's layer (B_SilkS for a back-side footprint). The ordinal of a user field
    /// is left 0: the caller sets it from `Footprint::next_field_ordinal()` like the C++
    /// constructor.
    pub fn new(
        kiid_generator: &mut KiidGenerator,
        footprint_layer: Option<PcbLayerId>,
        id: FieldT,
        name: &str,
    ) -> Self {
        let mut text = PcbText::for_footprint(footprint_layer);
        text.base.uuid = Kiid::new(kiid_generator);
        PcbField {
            text,
            id,
            ordinal: 0,
            name: name.to_string(),
            private: false,
        }
    }

    /// `IsReference()`.
    pub fn is_reference(&self) -> bool {
        self.id == FieldT::Reference
    }

    /// `IsValue()`.
    pub fn is_value(&self) -> bool {
        self.id == FieldT::Value
    }

    /// `IsDatasheet()`.
    pub fn is_datasheet(&self) -> bool {
        self.id == FieldT::Datasheet
    }

    /// `IsPrivate()`.
    pub fn is_private(&self) -> bool {
        self.private
    }

    /// `SetPrivate()`.
    pub fn set_private(&mut self, private: bool) {
        self.private = private;
    }

    /// `GetId()`.
    pub fn field_id(&self) -> FieldT {
        self.id
    }

    /// `IsMandatory()`.
    pub fn is_mandatory(&self) -> bool {
        crate::port_pending!("PCB_FIELD::IsMandatory")
    }

    /// `GetOrdinal()`: the id for a mandatory field, else `m_ordinal`.
    pub fn ordinal(&self) -> i32 {
        crate::port_pending!("PCB_FIELD::GetOrdinal")
    }

    /// `SetOrdinal( aOrdinal, aType )`.
    pub fn set_ordinal(&mut self, ordinal: i32, field_type: FieldT) {
        self.id = field_type;
        self.ordinal = ordinal;
    }

    /// `GetName( aUseDefaultName )`.
    pub fn name(&self, use_default_name: bool) -> String {
        crate::port_pending!("PCB_FIELD::GetName", use_default_name)
    }

    /// `GetUntranslatedName()`.
    pub fn untranslated_name(&self) -> String {
        crate::port_pending!("PCB_FIELD::GetUntranslatedName")
    }

    /// `SetName()`.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// `GetShownText( aVariantName, aContext, aDepth )`.
    pub fn shown_text_for_variant(
        &self,
        ctx: &BoardCtx<'_>,
        variant_name: &str,
        context: ResolutionContext,
        depth: i32,
    ) -> String {
        crate::port_pending!("PCB_FIELD::GetShownText (variant)", ctx, variant_name, context, depth)
    }

    /// `PCB_FIELD::GetTextTypeDescription()`.
    pub fn text_type_description(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_FIELD::GetTextTypeDescription", ctx)
    }

    /// `PCB_TEXT::TextHitTest (PCB_FIELD)`: the box comes from the field's shown text.
    pub fn text_hit_test(&self, ctx: &BoardCtx<'_>, point: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXT::TextHitTest (PCB_FIELD)", ctx, point, accuracy)
    }

    /// `PCB_TEXT::TextHitTest (PCB_FIELD, rect)`.
    pub fn text_hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contains: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TEXT::TextHitTest (PCB_FIELD, rect)", ctx, rect, contains, accuracy)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbField {
    fn invalidate_caches(&mut self) {
        self.text.invalidate_caches();
    }
}

impl BoardItem for PcbField {
    fn base(&self) -> &BoardItemBase {
        &self.text.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.text.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbFieldT
    }
    fn class_name(&self) -> &'static str {
        "PCB_FIELD"
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbField::bounding_box(self, ctx)
    }
    fn effective_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT) -> SharedShape {
        PcbField::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbField::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(&self, ctx: &BoardCtx<'_>, rect: &Box2I, contained: bool, accuracy: i32) -> bool {
        PcbField::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(&self, ctx: &BoardCtx<'_>, units: &UnitsProvider<'_>, full: bool) -> String {
        PcbField::item_description(self, ctx, units, full)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbField::position(self, ctx)
    }
    fn transform_shape_to_polygon(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet, layer: PcbLayerId, clearance: i32, error: i32, error_loc: ErrorLoc, ignore_line_width: bool) {
        PcbField::transform_shape_to_polygon(self, ctx, buffer, layer, clearance, error, error_loc, ignore_line_width)
    }
}

impl BoardItemMut for PcbField {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbField::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbField::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbField::move_by(self, env, delta)
    }
    fn on_footprint_rescaled(&mut self, env: &MutEnv, ratio_x: f64, ratio_y: f64, linear_factor: f64, anchor: Vector2I, parent_rotate: EdaAngle) {
        PcbField::on_footprint_rescaled(self, env, ratio_x, ratio_y, linear_factor, anchor, parent_rotate)
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        PcbField::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbField::rotate(self, env, centre, angle)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbField::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbField::style_from_settings(self, settings, check_side)
    }
}

impl EdaTextItem for PcbField {
    fn eda_text(&self) -> &EdaText {
        &self.text.text
    }
    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text.text
    }
    fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbField::draw_rotation(self, ctx)
    }
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        PcbField::font_metrics(self, ctx)
    }
    fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbField::offset(self, env, delta)
    }
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        PcbField::set_text_angle(self, env, angle)
    }
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        PcbField::set_text_size(self, env, size, enforce_min)
    }
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        PcbField::set_text_thickness(self, env, width)
    }
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        PcbField::shown_text(self, ctx, context, depth)
    }
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbField::text_angle(self, ctx)
    }
    fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbField::text_pos(self, ctx)
    }
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbField::text_size(self, ctx)
    }
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbField::text_thickness(self, ctx)
    }
}

// ---- overrides and inherited PCB_TEXT bodies (package M10) -------------------------------

impl PcbField {
    /// `PCB_TEXT::GetBoundingBox (PCB_FIELD)`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TEXT::GetBoundingBox (PCB_FIELD)", ctx)
    }

    /// `PCB_TEXT::GetEffectiveShape (PCB_FIELD)`.
    pub fn effective_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT) -> SharedShape {
        crate::port_pending!("PCB_TEXT::GetEffectiveShape (PCB_FIELD)", ctx, layer, flash, usage)
    }

    /// `PCB_TEXT::HitTest (PCB_FIELD)`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXT::HitTest (PCB_FIELD)", ctx, pos, accuracy)
    }

    /// `PCB_TEXT::HitTest (PCB_FIELD)`.
    pub fn hit_test_rect(&self, ctx: &BoardCtx<'_>, rect: &Box2I, contained: bool, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TEXT::HitTest (PCB_FIELD)", ctx, rect, contained, accuracy)
    }

    /// `PCB_FIELD::GetItemDescription`.
    pub fn item_description(&self, ctx: &BoardCtx<'_>, units: &UnitsProvider<'_>, full: bool) -> String {
        crate::port_pending!("PCB_FIELD::GetItemDescription", ctx, units, full)
    }

    /// `PCB_TEXT::GetPosition`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text.position(ctx)
    }

    /// `PCB_TEXT::TransformShapeToPolygon (PCB_FIELD)`.
    pub fn transform_shape_to_polygon(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet, layer: PcbLayerId, clearance: i32, error: i32, error_loc: ErrorLoc, ignore_line_width: bool) {
        crate::port_pending!("PCB_TEXT::TransformShapeToPolygon (PCB_FIELD)", ctx, buffer, layer, clearance, error, error_loc, ignore_line_width)
    }

    /// `PCB_TEXT::Flip`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.text.flip(env, centre, dir)
    }

    /// `PCB_TEXT::Mirror`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.text.mirror(env, centre, dir)
    }

    /// `PCB_TEXT::Move`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.text.move_by(env, delta)
    }

    /// `PCB_TEXT::OnFootprintRescaled`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn on_footprint_rescaled(&mut self, env: &MutEnv, ratio_x: f64, ratio_y: f64, linear_factor: f64, anchor: Vector2I, parent_rotate: EdaAngle) {
        self.text.on_footprint_rescaled(env, ratio_x, ratio_y, linear_factor, anchor, parent_rotate)
    }

    /// `PCB_TEXT::OnFootprintTransformed`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        self.text.on_footprint_transformed(env)
    }

    /// `PCB_TEXT::Rotate`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        self.text.rotate(env, centre, angle)
    }

    /// `PCB_TEXT::SetPosition`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        self.text.set_position(env, pos)
    }

    /// `PCB_TEXT::StyleFromSettings`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        self.text.style_from_settings(settings, check_side)
    }

    /// `PCB_TEXT::GetDrawRotation`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.text.draw_rotation(ctx)
    }

    /// `PCB_TEXT::getFontMetrics`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        self.text.font_metrics(ctx)
    }

    /// `PCB_TEXT::Offset`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        self.text.offset(env, delta)
    }

    /// `PCB_TEXT::SetTextAngle`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.text.set_text_angle(env, angle)
    }

    /// `PCB_TEXT::SetTextSize`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        self.text.set_text_size(env, size, enforce_min)
    }

    /// `PCB_TEXT::SetTextThickness`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        self.text.set_text_thickness(env, width)
    }

    /// `PCB_FIELD::GetShownText`.
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        crate::port_pending!("PCB_FIELD::GetShownText", ctx, context, depth)
    }

    /// `PCB_TEXT::GetTextAngle`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.text.text_angle(ctx)
    }

    /// `PCB_TEXT::GetTextPos`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text.text_pos(ctx)
    }

    /// `PCB_TEXT::GetTextSize`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text.text_size(ctx)
    }

    /// `PCB_TEXT::GetTextThickness`, inherited by `PCB_FIELD` (the base body reaches no virtual that `PCB_FIELD` overrides).
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.text.text_thickness(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_FIELD::PCB_FIELD( FOOTPRINT*, FIELD_T, const wxString& )` (pcbnew/pcb_field.cpp).
    #[test]
    fn constructor_values() {
        let f = PcbField::default();
        assert_eq!((f.id, f.ordinal, f.private), (FieldT::User, 0, false));
        assert!(f.name.is_empty());
        assert_eq!(f.text.base.layer, PcbLayerId::FSilkS);
        assert!(f.text.text.attributes.keep_upright);
        assert_eq!((FieldT::Reference as i32, FieldT::SheetRefSuffix as i32), (1, 10));

        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Field).typed::<PcbField>().unwrap();
        let mut kiids = KiidGenerator::default();
        let value = PcbField::new(&mut kiids, Some(PcbLayerId::BCu), FieldT::Value, "Value");
        assert_eq!(value.text.base.layer, PcbLayerId::BSilkS);
        assert!(value.is_value() && !value.is_reference());
        fx.set_child(id, value);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].name, "Value");
        assert_eq!(fx.board[id].type_(), KicadT::PcbFieldT);
    }
}
