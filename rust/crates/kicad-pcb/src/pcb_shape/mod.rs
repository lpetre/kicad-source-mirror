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

use std::sync::Arc;

use kicad_core::layer_ids::{is_copper_layer, is_solder_mask_layer};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{
    ANGLE_0, ANGLE_180, Box2I, EdaAngle, FlipDirection, Seg, ShapeCompound, ShapePolySet, Vector2I,
    ki_round,
};

use crate::eda_shape::shapes::make_effective_shapes_with_line_endings;
use crate::ids::NetCode;
use crate::item::BoardUse;
use crate::{board_connected_item, board_item};

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
    fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        PcbShape::style_from_settings(self, env, settings, check_side)
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
    /// Port of `PCB_SHAPE::GetBoundingBox()`: the `EDA_SHAPE` box merged with the line-ending
    /// box, normalized.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut bbox = crate::eda_shape::shape_bounding_box(self, ctx);

        if let Some(endings_bbox) = self
            .shape
            .line_endings_bounding_box(EdaShapeItem::effective_width(self, ctx))
        {
            bbox.merge(&endings_bbox);
        }

        bbox.normalize();

        bbox
    }

    /// `PCB_SHAPE::GetCenter()`: `getCenter()`.
    pub fn center(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        crate::eda_shape::shape_center(self)
    }

    /// Port of `PCB_SHAPE::GetEffectiveShape( aLayer, FLASHING, DRC_CONSTRAINT_T )`: the
    /// effective shapes with line endings, widened by the solder mask expansion on a mask
    /// layer of a copper shape with solder mask.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut width = EdaShapeItem::effective_width(self, ctx);

        if is_solder_mask_layer(layer)
            && self.has_solder_mask()
            && is_copper_layer(self.base.item.layer)
        {
            width += 2 * self.solder_mask_expansion(ctx);
        }

        Arc::new(ShapeCompound::from_shapes(
            make_effective_shapes_with_line_endings(self, ctx, width),
        ))
    }

    /// Port of `PCB_SHAPE::GetFriendlyName()`: `getFriendlyName( FRAME_PCB_EDITOR )`.
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        shapes::shape_friendly_name(&self.shape, self.is_proxy_item())
    }

    /// `PCB_SHAPE::HasLineStroke()`: true.
    pub fn has_line_stroke(&self) -> bool {
        true
    }

    /// Port of `PCB_SHAPE::HitTest( aPosition, aAccuracy )`: `hitTest()`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::eda_shape::shape_hit_test(self, ctx, pos, accuracy)
    }

    /// Port of `PCB_SHAPE::HitTest( aRect, aContained, aAccuracy )`: `hitTest()`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::eda_shape::shape_hit_test_rect(self, ctx, rect, contained, accuracy)
    }

    /// Port of `PCB_SHAPE::IsOnLayer()`: the shape's layer, and the mask layer of a copper
    /// shape with solder mask.
    pub fn is_on_layer(&self, _ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let my_layer = self.base.item.layer;

        if layer == my_layer {
            return true;
        }

        if self.has_solder_mask
            && ((layer == PcbLayerId::FMask && my_layer == PcbLayerId::FCu)
                || (layer == PcbLayerId::BMask && my_layer == PcbLayerId::BCu))
        {
            return true;
        }

        false
    }

    /// Port of `PCB_SHAPE::GetItemDescription()` (English; `_()` untranslated).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let mut parent_fp = board_item::parent_footprint(self, ctx);

        // Don't report parent footprint info from footprint editor, viewer, etc.
        if board_item::has_board(self, ctx) && ctx.board.board_use == BoardUse::FpHolder {
            parent_fp = None;
        }

        let layer_name = board_item::layer_name(self, ctx);

        if is_copper_layer(self.base.item.layer) {
            let netname_msg = board_connected_item::netname_msg(self, ctx);

            if let Some(fp) = parent_fp {
                format!(
                    "{} {} of {} on {}",
                    self.friendly_name(ctx),
                    netname_msg,
                    fp.reference(ctx),
                    layer_name
                )
            } else {
                format!(
                    "{} {} on {}",
                    self.friendly_name(ctx),
                    netname_msg,
                    layer_name
                )
            }
        } else if let Some(fp) = parent_fp {
            format!(
                "{} of {} on {}",
                self.friendly_name(ctx),
                fp.reference(ctx),
                layer_name
            )
        } else {
            format!("{} on {}", self.friendly_name(ctx), layer_name)
        }
    }

    /// `PCB_SHAPE::GetLayer()`: `m_layer`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.base.item.layer
    }

    /// Port of `PCB_SHAPE::GetLayerSet()`: the layer, plus the mask layer of a copper shape
    /// with solder mask.
    pub fn layer_set(&self, _ctx: &BoardCtx<'_>) -> Lset {
        let mut layermask = Lset::from_layers(&[self.base.item.layer]);

        if self.has_solder_mask {
            if layermask.contains(PcbLayerId::FCu) {
                layermask.set(PcbLayerId::FMask);
            }

            if layermask.contains(PcbLayerId::BCu) {
                layermask.set(PcbLayerId::BMask);
            }
        }

        layermask
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

    /// Port of `PCB_SHAPE::TransformShapeToPolySet()`: `EDA_SHAPE::TransformShapeToPolygon(
    /// .., false, true )` (hatched fills included).
    pub fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        _layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        crate::eda_shape::shape_transform_to_polygon(
            self, ctx, buffer, clearance, error, error_loc, false, true,
        )
    }

    /// Port of `PCB_SHAPE::TransformShapeToPolygon()`: `EDA_SHAPE::TransformShapeToPolygon(
    /// .., ignoreLineWidth, false )`.
    #[allow(clippy::too_many_arguments)]
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        _layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::eda_shape::shape_transform_to_polygon(
            self,
            ctx,
            buffer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
            false,
        )
    }

    /// Port of `PCB_SHAPE::IsConnected()`: only board-level copper shapes are connectable.
    pub fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        is_copper_layer(self.base.item.layer) && board_item::parent_footprint(self, ctx).is_none()
    }

    /// Port of `PCB_SHAPE::Flip( aCentre, aFlipDirection )`: a footprint child (or a pad
    /// primitive whose shape differs from its library shape) mirrors its library-frame
    /// geometry about the library-frame centre and re-bakes; anything else flips the board
    /// geometry and re-syncs.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        let mut xform = TransformTrs::default();
        let mut mirror_lib = false;

        if !self.parent_is_pad() && env.parent.is_some() {
            if let Some(frame) = env.parent {
                xform = frame.transform;
            }

            mirror_lib = true;
        } else if self.parent_is_pad() && self.shape.shape != self.lib.shape {
            // GetPrimitiveLibScale() of the owning pad (carried by the primitive's MutEnv).
            let (sx, sy) = env.pad_primitive_scale;
            xform.set_scale(sx, sy);
            mirror_lib = true;
        }

        let lib_shape = self.lib.shape;

        if mirror_lib
            && matches!(
                lib_shape,
                ShapeT::Segment
                    | ShapeT::Circle
                    | ShapeT::Arc
                    | ShapeT::Rectangle
                    | ShapeT::Bezier
                    | ShapeT::Poly
            )
        {
            let lib_center = xform.inverse_apply(centre);

            let mirror_pt = |p: &mut Vector2I| {
                if dir == FlipDirection::LeftRight {
                    p.x = 2 * lib_center.x - p.x;
                } else {
                    p.y = 2 * lib_center.y - p.y;
                }
            };

            if lib_shape == ShapeT::Arc {
                mirror_pt(&mut self.lib.start);
                mirror_pt(&mut self.lib.end);
                mirror_pt(&mut self.lib.arc_mid);
                std::mem::swap(&mut self.lib.start, &mut self.lib.end);
            } else {
                mirror_pt(&mut self.lib.start);
                mirror_pt(&mut self.lib.end);
            }

            if lib_shape == ShapeT::Bezier {
                mirror_pt(&mut self.lib.bezier_c1);
                mirror_pt(&mut self.lib.bezier_c2);
            }

            // Mirror() moves the arcs of the polygon as arcs, which a vertex at a time would
            // turn into segments
            if lib_shape == ShapeT::Poly {
                self.lib.poly.mirror(lib_center, dir);
            }

            let flipped = env.layers.flip_layer(self.base.item.layer);
            self.set_layer(env, flipped);
            self.rebake_from_transform_err(&xform, env.max_error);
            return;
        }

        if mirror_lib && matches!(lib_shape, ShapeT::Ellipse | ShapeT::EllipseArc) {
            let lib_center = xform.inverse_apply(centre);

            if dir == FlipDirection::LeftRight {
                self.lib.ellipse_center.x = 2 * lib_center.x - self.lib.ellipse_center.x;
            } else {
                self.lib.ellipse_center.y = 2 * lib_center.y - self.lib.ellipse_center.y;
            }

            self.lib.ellipse_rotation = -self.lib.ellipse_rotation;

            let old_start = self.lib.ellipse_start_angle;
            let old_end = self.lib.ellipse_end_angle;

            if dir == FlipDirection::LeftRight {
                self.lib.ellipse_start_angle = ANGLE_180 - old_end;
                self.lib.ellipse_end_angle = ANGLE_180 - old_start;
            } else {
                self.lib.ellipse_start_angle = -old_end;
                self.lib.ellipse_end_angle = -old_start;
            }

            let flipped = env.layers.flip_layer(self.base.item.layer);
            self.set_layer(env, flipped);
            self.rebake_from_transform_err(&xform, env.max_error);
            return;
        }

        self.shape.flip(centre, dir, env.max_error);

        let flipped = env.layers.flip_layer(self.base.item.layer);
        self.set_layer(env, flipped);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::Mirror( aCentre, aFlipDirection )` = `Mirror( MIRROR_AXIS( aCentre,
    /// aFlipDirection ) )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(env, &board_item::mirror_axis(centre, dir));
    }

    /// Port of `PCB_SHAPE::Mirror( const SEG& aAxis )`: `EDA_SHAPE::mirror()`, then sync. The
    /// `EDA_SHAPE` body's call of the virtual `SetShape( POLY )` (a rectangle mirrored about a
    /// skewed axis) also sets `m_libShape`.
    pub fn mirror_seg(&mut self, env: &MutEnv, axis: &Seg) {
        let before = self.shape.shape;
        self.shape.mirror(axis, env.max_error);
        self.note_virtual_set_shape(before);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::Move()`: `move()`, then sync.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.shape.move_(delta);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::Normalize()` for an item whose environment follows from its parent
    /// link alone: a board-level or detached shape, or a pad primitive (the parser normalizes
    /// board-level rectangles only). A footprint child needs [`PcbShape::normalize_env`]: here
    /// its board-frame geometry is normalized and the library frame is left as it was (see
    /// `PORT_STATUS.md`, M9).
    pub fn normalize(&mut self) {
        if self.base.item.parent.item().is_none() || self.parent_is_pad() {
            self.normalize_env(&MutEnv::detached());
            return;
        }

        let lib = self.lib.clone();
        self.normalize_env(&MutEnv::detached());
        self.lib = lib;
    }

    /// Port of `PCB_SHAPE::Normalize()`: a rectangle gets a normalized box; a polygon made of
    /// one axis-aligned four-segment outline becomes a rectangle.
    pub fn normalize_env(&mut self, env: &MutEnv) {
        if self.shape.shape == ShapeT::Rectangle {
            let start = self.shape.start();
            let end = self.shape.end();

            // BOX2I rect( start, end - start );
            // rect.Normalize();
            // SetStart( rect.GetPosition() );
            // SetEnd( rect.GetEnd() );
            //
            // Spelled out with the arithmetic of the reference build (FIX-FAB-3, finding X-2 of
            // review-fab): `end - start` is an int subtraction that wraps for a rectangle wider
            // or taller than 2^31 nm (an invalid board, which the reference still loads, plots
            // and saves), and on Linux `KiCheckedCast<int64_t, int>` in `Normalize()` /
            // `GetRight()` / `GetBottom()` is a plain narrowing conversion that wraps, while
            // `kicad_geom`'s `Box2I` clamps (its PORT_STATUS deviation D1).  For every
            // rectangle inside the int range this is exactly `Box2I::new( .. ).normalize()`.
            // Verified with pcb-dump against pcb-dump-rs (PORT_STATUS.md, FIX-FAB-3).
            let mut pos = start;
            let mut size: [i64; 2] = [
                end.x.wrapping_sub(start.x) as i64,
                end.y.wrapping_sub(start.y) as i64,
            ];

            if size[1] < 0 {
                size[1] = -size[1];
                pos.y = (pos.y as i64 - size[1]) as i32;
            }

            if size[0] < 0 {
                size[0] = -size[0];
                pos.x = (pos.x as i64 - size[0]) as i32;
            }

            let rect_end = Vector2I::new(
                (pos.x as i64 + size[0]) as i32,
                (pos.y as i64 + size[1]) as i32,
            );

            self.set_start(env, pos);
            self.set_end(env, rect_end);
        } else if self.shape.shape == ShapeT::Poly {
            let horizontal = |seg: &Seg| seg.a.y == seg.b.y;
            let vertical = |seg: &Seg| seg.a.x == seg.b.x;

            // Convert a poly back to a rectangle if appropriate
            let poly = self.shape.poly_shape();

            if poly.outline_count() == 1 && poly.c_outline(0).segment_count() == 4 {
                let outline = poly.c_outline(0);
                let (s0, s1, s2, s3) = (
                    outline.segment(0),
                    outline.segment(1),
                    outline.segment(2),
                    outline.segment(3),
                );

                if horizontal(&s0) && vertical(&s1) && horizontal(&s2) && vertical(&s3) {
                    self.set_shape(env, ShapeT::Rectangle);
                    self.set_start(env, Vector2I::new(s0.a.x.min(s0.b.x), s1.a.y.min(s1.b.y)));
                    self.set_end(env, Vector2I::new(s0.a.x.max(s0.b.x), s1.a.y.max(s1.b.y)));
                } else if vertical(&s0) && horizontal(&s1) && vertical(&s2) && horizontal(&s3) {
                    self.set_shape(env, ShapeT::Rectangle);
                    self.set_start(env, Vector2I::new(s1.a.x.min(s1.b.x), s0.a.y.min(s0.b.y)));
                    self.set_end(env, Vector2I::new(s1.a.x.max(s1.b.x), s0.a.y.max(s0.b.y)));
                }
            }
        }
    }

    /// Port of `PCB_SHAPE::NormalizeForCompare()`: like `Normalize()`, but a segment also
    /// gets its end points in a canonical order (graphic comparisons only).
    pub fn normalize_for_compare_env(&mut self, env: &MutEnv) {
        if self.shape.shape == ShapeT::Segment {
            let lib_start = self.lib_start();
            let lib_end = self.lib_end();

            if (lib_start.x > lib_end.x) || (lib_start.x == lib_end.x && lib_start.y < lib_end.y) {
                let s = self.shape.start();
                let e = self.shape.end();
                self.set_start(env, e);
                self.set_end(env, s);
            }
        } else {
            self.normalize_env(env);
        }
    }

    /// Port of `PCB_SHAPE::OnFootprintRescaled()`: `RebakeFromLib()`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
        self.rebake_from_lib(env);
    }

    /// Port of `PCB_SHAPE::OnFootprintTransformed()`: `RebakeFromLib()`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        self.rebake_from_lib(env);
    }

    /// Port of `PCB_SHAPE::Rotate()`: `rotate()`, then sync. The `EDA_SHAPE` body's call of
    /// the virtual `SetShape( POLY )` (a rectangle rotated by a non-cardinal angle) also sets
    /// `m_libShape`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let before = self.shape.shape;
        self.shape.rotate(centre, angle, env.max_error);
        self.note_virtual_set_shape(before);
        self.sync_lib_coords(env);
    }

    /// `EDA_SHAPE::rotate` / `mirror` turn a rectangle into a polygon through the virtual
    /// `SetShape( SHAPE_T::POLY )`, which `PCB_SHAPE` overrides to set `m_libShape` as well.
    fn note_virtual_set_shape(&mut self, before: ShapeT) {
        if before == ShapeT::Rectangle && self.shape.shape == ShapeT::Poly {
            self.lib.shape = ShapeT::Poly;
        }
    }

    /// Port of `PCB_SHAPE::SetLayer()`: `BOARD_ITEM::SetLayer`, and a shape moved off copper
    /// loses its net (`SetNetCode( -1 )`, which off copper is net 0: the board's unconnected
    /// net, or `OrphanedItem()` without a board, both net code 0).
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        board_item::set_layer(self, env, layer);

        if !is_copper_layer(self.base.item.layer) {
            self.base.net = NetCode::UNCONNECTED;
        }
    }

    /// Port of `PCB_SHAPE::SetLayerSet()`: the copper layer(s) of the set become the layer,
    /// a solder mask layer turns the solder mask on.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        layers.run_on_layers(|layer| {
            if is_copper_layer(layer) {
                self.set_layer(env, layer);
            } else if is_solder_mask_layer(layer) {
                self.set_has_solder_mask(true);
            }
        });
    }

    /// `PCB_SHAPE::SetPosition()`: `setPosition()` (no lib-frame sync in C++ either).
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.shape.set_position(pos);
    }

    /// Port of `PCB_SHAPE::StyleFromSettings()`: the line thickness of the layer class.
    pub fn style_from_settings(
        &mut self,
        _env: &MutEnv,
        settings: &BoardDesignSettings,
        _check_side: bool,
    ) {
        let width = settings.get_line_thickness(self.base.item.layer);
        self.shape.stroke.set_width(width);
    }

    /// Port of `PCB_SHAPE::getHatchingKnockouts()`.
    pub fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        shapes::hatching_knockouts(self, ctx)
    }

    /// `PCB_SHAPE::IsProxyItem()`: `m_proxyItem`.
    pub fn is_proxy_item(&self) -> bool {
        self.shape.proxy_item
    }

    /// Port of `PCB_SHAPE::getMaxError()`: `GetMaxError()`.
    pub fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 {
        board_item::max_error(self, ctx)
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
mod tests;

#[cfg(test)]
mod constructor_tests {
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
