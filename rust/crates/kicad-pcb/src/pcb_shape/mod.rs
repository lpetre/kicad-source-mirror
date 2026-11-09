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

//! Port of `pcbnew/pcb_shape.h` and `pcbnew/pcb_shape.cpp` (`PCB_SHAPE`): data, constructors
//! and delegation shims (package M0c), the `PCB_SHAPE` overrides (stubs for package M9).
//! The library-frame machinery and the `EDA_SHAPE` setter overrides are in `lib_frame.rs`,
//! the shape functions in `shapes.rs`.

pub mod lib_frame;
pub mod shapes;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I, ki_round};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::{EdaShape, FillT, ShapeT};
use crate::footprint::Footprint;
use crate::ids::ItemKind;
use crate::item::{BoardItemBase, ConnectedItemBase, SharedShape};
use crate::item_ref::AnyItem;
use crate::traits::{
    BoardConnectedItem, BoardItem, BoardItemMut, EdaShapeItem, Flashing, Invalidate,
};

/// `DEFAULT_LINE_WIDTH` (`include/board_design_settings_defaults.h`), in mm.
pub const DEFAULT_LINE_WIDTH: f64 = 0.10;

/// `PCB_SHAPE` (`pcbnew/pcb_shape.h`): `BOARD_CONNECTED_ITEM` + `EDA_SHAPE` + library-frame
/// copy.
#[derive(Clone, Debug)]
pub struct PcbShape {
    pub base: ConnectedItemBase,
    /// The `EDA_SHAPE` part (baked board-frame copy for footprint children).
    pub shape: EdaShape,
    pub has_solder_mask: bool,           // m_hasSolderMask
    pub solder_mask_margin: Option<i32>, // m_solderMaskMargin
    /// Authoritative lib-frame geometry (`lib_*()` accessors).
    pub(crate) lib: PcbShapeLib,
}

/// The `m_lib*` members: authoritative geometry in the parent footprint's library frame (equal
/// to the board frame for board-level shapes; pad frame for pad primitives).
#[derive(Clone, Debug)]
pub struct PcbShapeLib {
    pub shape: ShapeT,                 // m_libShape
    pub start: Vector2I,               // m_libStart
    pub end: Vector2I,                 // m_libEnd
    pub corner_radius: i32,            // m_libCornerRadius
    pub arc_mid: Vector2I,             // m_libArcMid
    pub bezier_c1: Vector2I,           // m_libBezierC1
    pub bezier_c2: Vector2I,           // m_libBezierC2
    pub poly: ShapePolySet,            // m_libPoly
    pub ellipse_center: Vector2I,      // m_libEllipseCenter
    pub ellipse_major_radius: i32,     // m_libEllipseMajorRadius
    pub ellipse_minor_radius: i32,     // m_libEllipseMinorRadius
    pub ellipse_rotation: EdaAngle,    // m_libEllipseRotation
    pub ellipse_start_angle: EdaAngle, // m_libEllipseStartAngle
    pub ellipse_end_angle: EdaAngle,   // m_libEllipseEndAngle
}

impl PcbShapeLib {
    /// The constructor values of the `m_lib*` members: everything zero, `m_libShape( aShapeType )`.
    pub fn new(shape: ShapeT) -> Self {
        PcbShapeLib {
            shape,
            start: Vector2I::default(),
            end: Vector2I::default(),
            corner_radius: 0,
            arc_mid: Vector2I::default(),
            bezier_c1: Vector2I::default(),
            bezier_c2: Vector2I::default(),
            poly: ShapePolySet::default(),
            ellipse_center: Vector2I::default(),
            ellipse_major_radius: 0,
            ellipse_minor_radius: 0,
            ellipse_rotation: ANGLE_0,
            ellipse_start_angle: ANGLE_0,
            ellipse_end_angle: ANGLE_0,
        }
    }
}

impl Default for PcbShape {
    /// `PCB_SHAPE( nullptr, SHAPE_T::SEGMENT )`.
    fn default() -> Self {
        PcbShape::with_shape(ShapeT::Segment)
    }
}

impl PcbShape {
    /// `PCB_SHAPE( aParent, aShapeType )` with a nil UUID: `EDA_SHAPE( aShapeType,
    /// mmToIU( DEFAULT_LINE_WIDTH ), FILL_T::NO_FILL )`, no solder mask, lib members zero.
    pub fn with_shape(shape: ShapeT) -> Self {
        PcbShape {
            base: ConnectedItemBase::default(),
            shape: EdaShape::new(
                shape,
                PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH),
                FillT::NoFill,
            ),
            has_solder_mask: false,
            solder_mask_margin: None,
            lib: PcbShapeLib::new(shape),
        }
    }

    /// `new PCB_SHAPE( aParent, aShapeType )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator, shape: ShapeT) -> Self {
        let mut s = PcbShape::with_shape(shape);
        s.base.item.uuid = Kiid::new(kiid_generator);
        s
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbShape {
    fn invalidate_caches(&mut self) {
        self.shape.invalidate_caches();
    }
}

impl BoardItem for PcbShape {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbShapeT
    }
    fn class_name(&self) -> &'static str {
        "PCB_SHAPE"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbShape::bounding_box(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbShape::center(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbShape::effective_shape(self, ctx, layer, flash, usage)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbShape::friendly_name(self, ctx)
    }
    fn has_line_stroke(&self) -> bool {
        PcbShape::has_line_stroke(self)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbShape::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbShape::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbShape::is_on_layer(self, ctx, layer)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbShape::item_description(self, ctx, units, full)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbShape::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbShape::layer_set(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbShape::position(self, ctx)
    }
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        PcbShape::stroke(self, ctx)
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
        PcbShape::transform_shape_to_poly_set(self, ctx, buffer, layer, clearance, error, error_loc)
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
        PcbShape::transform_shape_to_polygon(
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

impl BoardConnectedItem for PcbShape {
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbShape::is_connected(self, ctx)
    }
}

impl BoardItemMut for PcbShape {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbShape::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbShape::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbShape::move_by(self, env, delta)
    }
    fn normalize(&mut self) {
        PcbShape::normalize(self)
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
        PcbShape::on_footprint_rescaled(
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
        PcbShape::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbShape::rotate(self, env, centre, angle)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        PcbShape::set_layer(self, env, layer)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbShape::set_layer_set(self, env, layers)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbShape::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbShape::style_from_settings(self, settings, check_side)
    }
}

impl EdaShapeItem for PcbShape {
    fn eda_shape(&self) -> &EdaShape {
        &self.shape
    }
    fn eda_shape_mut(&mut self) -> &mut EdaShape {
        &mut self.shape
    }
    fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        PcbShape::hatching_knockouts(self, ctx)
    }
    fn is_proxy_item(&self) -> bool {
        PcbShape::is_proxy_item(self)
    }
    fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbShape::shape_max_error(self, ctx)
    }
    fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbShape::width(self, ctx)
    }
}

// ---- the PCB_SHAPE overrides (package M9) ------------------------------------------------

impl PcbShape {
    /// `PCB_SHAPE::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_SHAPE::GetBoundingBox", ctx)
    }

    /// `PCB_SHAPE::GetCenter()`: `getCenter()`.
    pub fn center(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        crate::eda_shape::shape_center(self)
    }

    /// `PCB_SHAPE::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_SHAPE::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_SHAPE::GetFriendlyName`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_SHAPE::GetFriendlyName", ctx)
    }

    /// `PCB_SHAPE::HasLineStroke()`: true.
    pub fn has_line_stroke(&self) -> bool {
        true
    }

    /// `PCB_SHAPE::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_SHAPE::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_SHAPE::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_SHAPE::HitTest", ctx, rect, contained, accuracy)
    }

    /// `PCB_SHAPE::IsOnLayer`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PCB_SHAPE::IsOnLayer", ctx, layer)
    }

    /// `PCB_SHAPE::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_SHAPE::GetItemDescription", ctx, units, full)
    }

    /// `PCB_SHAPE::GetLayer()`: `m_layer`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.base.item.layer
    }

    /// `PCB_SHAPE::GetLayerSet`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PCB_SHAPE::GetLayerSet", ctx)
    }

    /// `PCB_SHAPE::GetPosition()`: `getPosition()`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        crate::eda_shape::shape_position(self)
    }

    /// `PCB_SHAPE::GetStroke()`: `m_stroke` with the width of `GetWidth()`.
    pub fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        let mut s = self.shape.stroke.clone();
        s.set_width(self.width(ctx));
        s
    }

    /// `PCB_SHAPE::TransformShapeToPolySet`.
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
            "PCB_SHAPE::TransformShapeToPolySet",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc
        )
    }

    /// `PCB_SHAPE::TransformShapeToPolygon`.
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
            "PCB_SHAPE::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_SHAPE::IsConnected`.
    pub fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("PCB_SHAPE::IsConnected", ctx)
    }

    /// `PCB_SHAPE::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_SHAPE::Flip", env, centre, dir)
    }

    /// `PCB_SHAPE::Mirror`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_SHAPE::Mirror", env, centre, dir)
    }

    /// `PCB_SHAPE::Move`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PCB_SHAPE::Move", env, delta)
    }

    /// `PCB_SHAPE::Normalize`.
    pub fn normalize(&mut self) {
        crate::port_pending!("PCB_SHAPE::Normalize")
    }

    /// `PCB_SHAPE::OnFootprintRescaled`.
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
            "PCB_SHAPE::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `PCB_SHAPE::OnFootprintTransformed`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_SHAPE::OnFootprintTransformed", env)
    }

    /// `PCB_SHAPE::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_SHAPE::Rotate", env, centre, angle)
    }

    /// `PCB_SHAPE::SetLayer`.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        crate::port_pending!("PCB_SHAPE::SetLayer", env, layer)
    }

    /// `PCB_SHAPE::SetLayerSet`.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("PCB_SHAPE::SetLayerSet", env, layers)
    }

    /// `PCB_SHAPE::SetPosition`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetPosition", env, pos)
    }

    /// `PCB_SHAPE::StyleFromSettings`.
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        crate::port_pending!("PCB_SHAPE::StyleFromSettings", settings, check_side)
    }

    /// `PCB_SHAPE::getHatchingKnockouts`.
    pub fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        crate::port_pending!("PCB_SHAPE::getHatchingKnockouts", ctx)
    }

    /// `PCB_SHAPE::IsProxyItem()`: `m_proxyItem`.
    pub fn is_proxy_item(&self) -> bool {
        self.shape.proxy_item
    }

    /// `PCB_SHAPE::getMaxError`.
    pub fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_SHAPE::getMaxError", ctx)
    }

    /// `PCB_SHAPE::transformFp()`: the footprint whose transform maps this shape's library
    /// frame onto the board. `None` for a pad primitive (its frame is the pad's) and for a
    /// shape outside a footprint; otherwise `GetParentFootprint()`, which walks the whole
    /// parent chain (a table cell reaches the footprint through its table).
    pub fn transform_fp<'a>(&self, ctx: &BoardCtx<'a>) -> Option<&'a Footprint> {
        let mut parent = ctx.board.get(self.base.item.parent.item()?)?;

        if parent.kind() == ItemKind::Pad {
            return None;
        }

        loop {
            if let AnyItem::Footprint(fp) = parent {
                return Some(fp);
            }

            parent = ctx.board.get(parent.base().parent.item()?)?;
        }
    }

    /// `PCB_SHAPE::GetWidth()`: the library stroke width, negative widths clamped to zero
    /// (they mean something in eeschema but break plotters and exporters here), scaled by the
    /// parent footprint's linear scale.
    pub fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        let lib = self.shape.stroke.width().max(0);
        let s = fp_scale_linear(self.transform_fp(ctx));

        if s == 1.0 {
            lib
        } else {
            ki_round(lib as f64 * s)
        }
    }
}

/// `fpScaleLinear( aFp )` (static, pcbnew/pcb_shape.cpp): the mean of the footprint's X and Y
/// scale; 1.0 without a footprint.
pub(crate) fn fp_scale_linear(fp: Option<&Footprint>) -> f64 {
    match fp {
        None => 1.0,
        Some(fp) => {
            let xform = fp.transform();
            (xform.scale_x() + xform.scale_y()) * 0.5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_SHAPE::PCB_SHAPE( aParent, aShapeType )` (pcbnew/pcb_shape.cpp): 0.1 mm line, no
    /// fill, no solder mask, lib members zero with the same shape.
    #[test]
    fn constructor_values() {
        let s = PcbShape::default();
        assert_eq!(s.shape.shape, ShapeT::Segment);
        assert_eq!(s.shape.stroke.width(), 100_000);
        assert_eq!(s.shape.fill, FillT::NoFill);
        assert!(!s.has_solder_mask);
        assert_eq!(s.solder_mask_margin, None);
        assert_eq!(s.lib.shape, ShapeT::Segment);
        assert_eq!(
            (s.lib.start, s.lib.end, s.lib.corner_radius),
            (Vector2I::default(), Vector2I::default(), 0)
        );
        assert_eq!(s.lib.ellipse_rotation, ANGLE_0);
        assert_eq!(s.base.item.layer, PcbLayerId::FCu);
        assert!(s.base.item.uuid == Kiid::default());

        let mut fx = rotated_flipped_scaled();
        let mut kiids = KiidGenerator::default();
        let shape_id = fx.child(ItemKind::Shape).typed::<PcbShape>().unwrap();
        let mut rect = PcbShape::new(&mut kiids, ShapeT::Rectangle);
        assert!(rect.base.item.uuid != Kiid::default());
        rect.has_solder_mask = true;
        fx.set_child(shape_id, rect);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[shape_id].lib.shape, ShapeT::Rectangle);
        assert!(fx.board[shape_id].has_solder_mask);
        let added = fx.add_child(PcbShape::with_shape(ShapeT::Circle), |fp| &mut fp.drawings);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[added].type_(), KicadT::PcbShapeT);
    }

    /// `PCB_SHAPE::GetWidth` / `GetStroke` (pcbnew/pcb_shape.cpp): the library width, clamped
    /// at zero and scaled by `fpScaleLinear( transformFp() )`; `PCB_TEXTBOX` and
    /// `PCB_TABLECELL` inherit it (the cell reaches the footprint through its table). The
    /// fixture footprint is scaled (1.5, 0.75): linear scale 1.125.
    #[test]
    fn width_scales_with_the_parent_footprint() {
        let mut fx = rotated_flipped_scaled();
        let shape_id = fx.child(ItemKind::Shape).typed::<PcbShape>().unwrap();
        let textbox_id = fx.child(ItemKind::Textbox);
        let textbox_id = textbox_id
            .typed::<crate::pcb_textbox::PcbTextbox>()
            .unwrap();
        let cell_id = fx.child(ItemKind::Tablecell);
        let cell_id = cell_id
            .typed::<crate::pcb_tablecell::PcbTablecell>()
            .unwrap();
        let detached = PcbShape::default();

        {
            let ctx = &BoardCtx::bare(&fx.board);
            let shape = &fx.board[shape_id];
            assert_eq!(
                shape.transform_fp(ctx).unwrap().base.id(),
                Some(fx.footprint.into())
            );
            assert_eq!(shape.shape.stroke.width(), 100_000);
            assert_eq!(EdaShapeItem::width(shape, ctx), 112_500);
            assert_eq!(shape.effective_width(ctx), 112_500);
            assert_eq!(shape.hatch_line_width(ctx), 112_500);
            assert_eq!(shape.hatch_line_spacing(ctx), 1_125_000);
            assert_eq!(BoardItem::stroke(shape, ctx).width(), 112_500);

            assert_eq!(EdaShapeItem::width(&fx.board[textbox_id], ctx), 112_500);
            assert_eq!(EdaShapeItem::width(&fx.board[cell_id], ctx), 112_500);

            // No parent: the stored value.
            assert!(detached.transform_fp(ctx).is_none());
            assert_eq!(EdaShapeItem::width(&detached, ctx), 100_000);
        }

        // Negative widths are clamped before scaling.
        let mut negative = PcbShape::default();
        negative.shape.stroke.set_width(-1);
        fx.set_child(shape_id, negative);
        let ctx = &BoardCtx::bare(&fx.board);
        assert_eq!(EdaShapeItem::width(&fx.board[shape_id], ctx), 0);
    }
}
