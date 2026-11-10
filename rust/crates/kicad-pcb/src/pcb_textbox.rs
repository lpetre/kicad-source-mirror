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
//! EDA_TEXT`): data, constructors and delegation shims (package M0c), the overrides (package
//! M11).
//!
//! `PcbTextbox` is its own host of both mixins and has a shim for every virtual that
//! `PCB_TEXTBOX` or `PCB_SHAPE` overrides. An inherited `PCB_SHAPE` body calls the embedded
//! `PcbShape` where it reaches no virtual that `PCB_TEXTBOX` overrides; where it does (the
//! virtual `syncLibCoords()`, `GetCorners()`, `GetBoundingBox()`, `GetShownText()`), the body is
//! written here over the host so that the override applies.
//!
//! The read-only `PCB_TEXTBOX` bodies are generic functions over [`TextboxHost`], implemented
//! by `PcbTextbox` and `PcbTablecell`: `PCB_TABLECELL` overrides `GetShownText()` and
//! `IsFilledForHitTesting()`, which these bodies reach virtually.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use kicad_core::layer_ids::{is_copper_layer, is_solder_mask_layer};
use kicad_core::text_vars::{ResolutionContext, finalize_text_var_expansion};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{
    GrTextHAlignT, GrTextVAlignT, KicadT, Kiid, KiidGenerator, Lset, PcbLayerId, StrokeParams,
};
use kicad_font::{CallbackGal, FontExt, TextGal};
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::{ERROR_OUTSIDE, ErrorLoc};
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, Box2I, EdaAngle, EdaOrientation, FlipDirection, Seg,
    Shape, ShapeCompound, ShapePolySet, Vector2D, Vector2I, ki_round, rotate_point_around,
};

use crate::board_design_settings::BoardDesignSettings;
use crate::board_item;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::shapes::make_effective_shapes_with_line_endings;
use crate::eda_shape::{EdaShape, ShapeT};
use crate::eda_text::{self, EdaText, outline_glyph_triangulate, resolve_text_vars_shared_depth};
use crate::ids::ItemId;
use crate::item::{BoardItemBase, ConnectedItemBase, RecurseMode, SharedShape};
use crate::item_ref::{AnyItem, ItemRef};
use crate::pcb_shape::PcbShape;
use crate::pcb_text::{ellipsize_menu_text, footprint_resolve_text_var};
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

    /// `IsBorderEnabled()`.
    pub fn is_border_enabled(&self) -> bool {
        self.border_enabled
    }

    /// `SetBorderEnabled()`.
    pub fn set_border_enabled(&mut self, enabled: bool) {
        self.border_enabled = enabled;
    }

    /// `GetBorderWidth()`: the stored (library) stroke width `m_stroke.GetWidth()`.
    pub fn border_width(&self) -> i32 {
        self.shape.shape.stroke.width()
    }

    /// `SetBorderWidth()`: `m_stroke.SetWidth( aSize )` (no footprint scaling).
    pub fn set_border_width(&mut self, size: i32) {
        self.shape.shape.stroke.set_width(size);
    }

    /// Port of `GetLegacyTextMargin()`: `KiROUND( GetStroke().GetWidth() / 2.0 ) +
    /// KiROUND( GetTextSize().y * 0.75 )`.
    pub fn legacy_text_margin(&self, ctx: &BoardCtx<'_>) -> i32 {
        ki_round(self.stroke(ctx).width() as f64 / 2.0)
            + ki_round(self.text_size(ctx).y as f64 * 0.75)
    }

    /// Port of `GetMinSize()`.
    pub fn min_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        min_size(self, ctx)
    }

    /// Port of `GetDrawPos( aIsFlipped )`.
    pub fn draw_pos_flipped(&self, ctx: &BoardCtx<'_>, is_flipped: bool) -> Vector2I {
        draw_pos_flipped(self, ctx, is_flipped)
    }

    /// Port of `GetPolygonalBoxShape()`: a `PCB_SHAPE` polygon of the (rotated) box with the
    /// textbox's parent, stroke and layer.
    pub fn polygonal_box_shape(&self, ctx: &BoardCtx<'_>) -> PcbShape {
        // Build a PCB_SHAPE type POLYGON of the TEXTBOX, according to the box shape and the
        // textbox rotation
        let parent = self.shape.base.item.parent;
        let env = ctx.board.child_env(parent);
        let mut dummy = PcbShape::with_shape(ShapeT::Poly);
        dummy.base.item.parent = parent;
        dummy.set_width(&env, self.border_width());
        dummy.set_stroke(&env, self.stroke(ctx));
        dummy.set_layer(&env, self.shape.base.item.layer);
        let poly = self.corners(ctx); // Already rotated
        dummy.shape.set_poly_points(&poly);

        dummy
    }

    /// Port of `PCB_TEXTBOX::GetCorners()` (overrides the virtual introduced by `PCB_SHAPE`):
    /// `GetCornersInSequence( GetDrawRotation() )`.
    pub fn corners(&self, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        corners(self, ctx)
    }

    /// Port of `PCB_TEXTBOX::TransformTextToPolySet()`.
    pub fn transform_text_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        max_error: i32,
        error_loc: ErrorLoc,
    ) {
        transform_text_to_poly_set(self, ctx, buffer, clearance, max_error, error_loc)
    }

    /// `GetTextAngle()` as a mutator sees it (the parent frame of `env`).
    pub(crate) fn text_angle_env(&self, env: &MutEnv) -> EdaAngle {
        match env.parent.as_ref() {
            Some(parent) => {
                let mut angle = self.lib_text_angle.angle() + parent.transform.rotate();
                angle.normalize()
            }
            None => self.lib_text_angle.angle(),
        }
    }

    /// Port of `PCB_TEXTBOX::syncLibCoords()`: the inverse of `OnFootprintTransformed` (move
    /// centre, scale half-extents, keep the box axis-aligned). A true inverse at any angle.
    pub fn sync_lib_coords(&mut self, env: &MutEnv) {
        // Only the axis-aligned rectangle case needs the textbox-specific inverse.
        // Poly textboxes and standalone (non-FP) boxes use the base shape mapping.
        let Some(frame) = env.parent.as_ref() else {
            self.shape.sync_lib_coords(env);
            return;
        };

        if self.shape.shape.shape() != ShapeT::Rectangle {
            self.shape.sync_lib_coords(env);
            return;
        }

        let xform = &frame.transform;
        let start = self.shape.shape.start();
        let end = self.shape.shape.end();

        let board_center = Vector2I::new(
            start.x.wrapping_add(end.x) / 2,
            start.y.wrapping_add(end.y) / 2,
        );
        let board_half_w = end.x.wrapping_sub(start.x).wrapping_abs() / 2;
        let board_half_h = end.y.wrapping_sub(start.y).wrapping_abs() / 2;

        let lib_center = xform.inverse_apply(board_center);
        let lib_half_w = ki_round(board_half_w as f64 / xform.scale_x()).wrapping_abs();
        let lib_half_h = ki_round(board_half_h as f64 / xform.scale_y()).wrapping_abs();

        self.shape.lib.start = Vector2I::new(lib_center.x - lib_half_w, lib_center.y - lib_half_h);
        self.shape.lib.end = Vector2I::new(lib_center.x + lib_half_w, lib_center.y + lib_half_h);
    }

    /// Port of `PCB_TEXTBOX::SetShape()`: keep the lib shape as the runtime shape (the
    /// `EDA_SHAPE` body and `m_libShape = aShape`).
    pub fn set_shape(&mut self, env: &MutEnv, shape: ShapeT) {
        self.shape.set_shape(env, shape);
    }

    /// Port of `PCB_TEXTBOX::SetTop()`.
    pub fn set_top(&mut self, env: &MutEnv, value: i32) {
        let rotation = self.text_angle_env(env);
        let (start, end) = (self.shape.shape.start(), self.shape.shape.end());

        if rotation == ANGLE_90 || rotation == ANGLE_180 {
            self.set_end(env, Vector2I::new(end.x, value));
        } else {
            self.set_start(env, Vector2I::new(start.x, value));
        }
    }

    /// Port of `PCB_TEXTBOX::SetLeft()`.
    pub fn set_left(&mut self, env: &MutEnv, value: i32) {
        let rotation = self.text_angle_env(env);
        let (start, end) = (self.shape.shape.start(), self.shape.shape.end());

        if rotation == ANGLE_180 || rotation == ANGLE_270 {
            self.set_end(env, Vector2I::new(value, end.y));
        } else {
            self.set_start(env, Vector2I::new(value, start.y));
        }
    }

    /// Port of `PCB_TEXTBOX::SetRight()`.
    pub fn set_right(&mut self, env: &MutEnv, value: i32) {
        let rotation = self.text_angle_env(env);
        let (start, end) = (self.shape.shape.start(), self.shape.shape.end());

        if rotation == ANGLE_180 || rotation == ANGLE_270 {
            self.set_start(env, Vector2I::new(value, start.y));
        } else {
            self.set_end(env, Vector2I::new(value, end.y));
        }
    }

    /// Port of `PCB_TEXTBOX::SetBottom()`.
    pub fn set_bottom(&mut self, env: &MutEnv, value: i32) {
        let rotation = self.text_angle_env(env);
        let (start, end) = (self.shape.shape.start(), self.shape.shape.end());

        if rotation == ANGLE_90 || rotation == ANGLE_180 {
            self.set_start(env, Vector2I::new(start.x, value));
        } else {
            self.set_end(env, Vector2I::new(end.x, value));
        }
    }

    /// Port of `PCB_SHAPE::SetStart()` on a text box: the `EDA_SHAPE` body, then the textbox's
    /// `syncLibCoords()`.
    pub fn set_start(&mut self, env: &MutEnv, start: Vector2I) {
        self.shape.shape.set_start(start);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEnd()` on a text box.
    pub fn set_end(&mut self, env: &MutEnv, end: Vector2I) {
        self.shape.shape.set_end(end);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetCenter()` on a text box.
    pub fn set_center(&mut self, env: &MutEnv, center: Vector2I) {
        self.shape.shape.set_center(center);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetCornerRadius()` on a text box.
    pub fn set_corner_radius(&mut self, env: &MutEnv, corner_radius: i32) {
        self.shape.shape.set_corner_radius(corner_radius);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetArcGeometry()` on a text box.
    pub fn set_arc_geometry(
        &mut self,
        env: &MutEnv,
        start: Vector2I,
        mid: Vector2I,
        end: Vector2I,
    ) {
        self.shape.shape.set_arc_geometry(start, mid, end);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetArcAngleAndEnd()` on a text box.
    pub fn set_arc_angle_and_end(&mut self, env: &MutEnv, angle: EdaAngle, check_negative: bool) {
        self.shape
            .shape
            .set_arc_angle_and_end(angle, check_negative);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetBezierC1()` on a text box.
    pub fn set_bezier_c1(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.shape.set_bezier_c1(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetBezierC2()` on a text box.
    pub fn set_bezier_c2(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.shape.set_bezier_c2(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseCenter()` on a text box.
    pub fn set_ellipse_center(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.shape.set_ellipse_center(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseMajorRadius()` on a text box.
    pub fn set_ellipse_major_radius(&mut self, env: &MutEnv, r: i32) {
        self.shape.shape.set_ellipse_major_radius(r);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseMinorRadius()` on a text box.
    pub fn set_ellipse_minor_radius(&mut self, env: &MutEnv, r: i32) {
        self.shape.shape.set_ellipse_minor_radius(r);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseRotation()` on a text box.
    pub fn set_ellipse_rotation(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.shape.set_ellipse_rotation(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseStartAngle()` on a text box.
    pub fn set_ellipse_start_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.shape.set_ellipse_start_angle(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseEndAngle()` on a text box.
    pub fn set_ellipse_end_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.shape.set_ellipse_end_angle(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetStroke()` on a text box: `m_stroke = aStroke;
    /// SetWidth( aStroke.GetWidth() )` (no sync; `SetWidth` is not overridden).
    pub fn set_stroke(&mut self, env: &MutEnv, stroke: StrokeParams) {
        self.shape.set_stroke(env, stroke);
    }

    /// Port of `PCB_SHAPE::SetWidth()` on a text box (not overridden).
    pub fn set_width(&mut self, env: &MutEnv, width: i32) {
        self.shape.set_width(env, width);
    }

    /// Port of `PCB_SHAPE::SetPolyShape()` on a text box.
    pub fn set_poly_shape(&mut self, env: &MutEnv, poly: ShapePolySet) {
        self.shape.shape.set_poly_shape(poly);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::Normalize()` on a text box with its environment: the `EDA_SHAPE`
    /// geometry normalized through the virtual setters (the textbox's `syncLibCoords()`).
    pub fn normalize_env(&mut self, env: &MutEnv) {
        if self.shape.shape.shape() == ShapeT::Rectangle {
            let start = self.shape.shape.start();
            let end = self.shape.shape.end();

            let mut rect = Box2I::new(start, end - start);
            rect.normalize();

            self.set_start(env, rect.position());
            self.set_end(env, rect.end());
        } else if self.shape.shape.shape() == ShapeT::Poly {
            let horizontal = |seg: &Seg| seg.a.y == seg.b.y;
            let vertical = |seg: &Seg| seg.a.x == seg.b.x;

            // Convert a poly back to a rectangle if appropriate
            let poly = self.shape.shape.poly_shape();

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

    /// Port of `PCB_TEXTBOX::Mirror( const SEG& aAxis )`: the position and justification are
    /// mirrored, but not the text itself.
    pub fn mirror_seg(&mut self, env: &MutEnv, axis: &Seg) {
        if self.shape.shape.shape() == ShapeT::Rectangle {
            let start = self.shape.shape.start();
            let end = self.shape.shape.end();
            let centre = Vector2I::new(
                start.x.wrapping_add(end.x) / 2,
                start.y.wrapping_add(end.y) / 2,
            );
            let delta = axis.reflect_point(centre) - centre;

            self.set_start(env, start + delta);
            let end = self.shape.shape.end();
            self.set_end(env, end + delta);

            self.sync_lib_coords(env);
        } else {
            // A polygon is the drawn box already, so its vertices are what gets mirrored.
            // (PCB_SHAPE::Mirror: its virtual syncLibCoords() is the base one for a polygon.)
            self.shape.mirror_seg(env, axis);
        }

        let axis_angle = -EdaAngle::from_vector(axis.b - axis.a);
        let new_angle = self.text_angle_env(env).reflect(axis_angle);

        match env.parent.as_ref() {
            Some(parent) => self.set_lib_text_angle(new_angle - parent.transform.rotate()),
            None => self.set_lib_text_angle(new_angle),
        }

        let angle = self.text_angle_env(env);
        eda_text::set_text_angle(self, env, angle);
    }

    /// `BOARD_ITEM::IsSideSpecific()` for a mutator: the layer set
    /// (`PCB_SHAPE::GetLayerSet()`) against `LSET::SideSpecificMask()`, then the type of the
    /// principal layer on the board (`MutEnv::is_side_specific`).
    fn is_side_specific_env(&self, env: &MutEnv) -> bool {
        let layer = self.shape.base.item.layer;
        let mut layers = Lset::from_layers(&[layer]);

        if self.shape.has_solder_mask {
            if layers.contains(PcbLayerId::FCu) {
                layers.set(PcbLayerId::FMask);
            }

            if layers.contains(PcbLayerId::BCu) {
                layers.set(PcbLayerId::BMask);
            }
        }

        env.is_side_specific(&layers, layer)
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
    fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        PcbTextbox::style_from_settings(self, env, settings, check_side)
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
    fn corners_in_sequence(&self, ctx: &BoardCtx<'_>, angle: EdaAngle) -> Vec<Vector2I> {
        PcbTextbox::corners_in_sequence(self, ctx, angle)
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
    /// Port of `PCB_TEXTBOX::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        bounding_box(self, ctx)
    }

    /// `PCB_SHAPE::GetCenter (PCB_TEXTBOX)`: `getCenter()`.
    pub fn center(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        crate::eda_shape::shape_center(self)
    }

    /// Port of `PCB_TEXTBOX::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        effective_shape(self, ctx, layer, flash, usage)
    }

    /// `PCB_TEXTBOX::GetFriendlyName()`: "Text Box" (untranslated).
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Text Box".to_string()
    }

    /// `PCB_SHAPE::HasLineStroke`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn has_line_stroke(&self) -> bool {
        self.shape.has_line_stroke()
    }

    /// Port of `PCB_TEXTBOX::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        hit_test(self, ctx, pos, accuracy)
    }

    /// Port of `PCB_TEXTBOX::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        hit_test_rect(self, ctx, rect, contained, accuracy)
    }

    /// `PCB_SHAPE::IsOnLayer`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.shape.is_on_layer(ctx, layer)
    }

    /// Port of `PCB_TEXTBOX::GetItemDescription` (untranslated).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        let content = if full {
            self.shown_text(ctx, ResolutionContext::ForGui, 0)
        } else {
            ellipsize_menu_text(self.text.text())
        };

        format!(
            "PCB text box '{}' on {}",
            content,
            board_item::layer_name(self, ctx)
        )
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

    /// Port of `PCB_SHAPE::TransformShapeToPolySet (PCB_TEXTBOX)`: `EDA_SHAPE::
    /// TransformShapeToPolygon( .., false, true )` over the text box host.
    pub fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        transform_shape_to_poly_set(self, ctx, buffer, layer, clearance, error, error_loc)
    }

    /// Port of `PCB_TEXTBOX::TransformShapeToPolygon`.
    #[allow(clippy::too_many_arguments)]
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
        transform_shape_to_polygon(
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

    /// `PCB_SHAPE::IsConnected`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        self.shape.is_connected(ctx)
    }

    /// Port of `PCB_TEXTBOX::Flip`: `PCB_SHAPE::Flip` (whose closing `syncLibCoords()` is the
    /// base one: it is reached only without a parent footprint), then the lib text angle
    /// reflected with an added half turn.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.shape.flip(env, centre, dir);

        // A flip changes the side it is read from, so add a half turn on top of the
        // reflection.
        let axis = kicad_geom::mirror_axis(dir);
        self.lib_text_angle = self.lib_text_angle.reflect(axis) + ANGLE_180;

        let angle = self.text_angle_env(env);
        eda_text::set_text_angle(self, env, angle);

        if self.is_side_specific_env(env) {
            let mirrored = !self.text.is_mirrored();
            self.text.set_mirrored(mirrored);
        }
    }

    /// Port of `PCB_TEXTBOX::Mirror( aCentre, aFlipDirection )`: `Mirror( MIRROR_AXIS( .. ) )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(env, &board_item::mirror_axis(centre, dir));
    }

    /// Port of `PCB_TEXTBOX::Move`: `PCB_SHAPE::Move` (with the textbox's `syncLibCoords()`),
    /// then `EDA_TEXT::Offset`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.shape.shape.move_(delta);
        self.sync_lib_coords(env);
        eda_text::offset(self, env, delta);
    }

    /// `PCB_SHAPE::Normalize (PCB_TEXTBOX)` for an item whose environment follows from its
    /// parent link alone (as [`PcbShape::normalize`]): a board-level or detached text box gets
    /// [`PcbTextbox::normalize_env`] with no parent frame; for a footprint child (or a cell)
    /// the board-frame geometry is normalized and the library frame kept (`PORT_STATUS.md`,
    /// M9 / M11). Table code and the board pass the environment to `normalize_env`.
    pub fn normalize(&mut self) {
        if self.shape.base.item.parent.item().is_none() {
            self.normalize_env(&MutEnv::detached());
            return;
        }

        let lib = self.shape.lib.clone();
        self.normalize_env(&MutEnv::detached());
        self.shape.lib = lib;
    }

    /// Port of `PCB_TEXTBOX::OnFootprintRescaled`: `OnFootprintTransformed()`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
        self.on_footprint_transformed(env);
    }

    /// Port of `PCB_TEXTBOX::OnFootprintTransformed`: an axis-aligned library rectangle is
    /// re-baked by centre and half-extents (it stays a rectangle; the rotation lives in the
    /// text angle), anything else through `PCB_SHAPE::OnFootprintTransformed`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        match env.parent.as_ref() {
            Some(frame) if self.shape.lib_shape() == ShapeT::Rectangle => {
                let xform = &frame.transform;
                self.shape.shape.shape = ShapeT::Rectangle;

                let lib_start = self.shape.lib.start;
                let lib_end = self.shape.lib.end;
                let lib_center = Vector2I::new(
                    lib_start.x.wrapping_add(lib_end.x) / 2,
                    lib_start.y.wrapping_add(lib_end.y) / 2,
                );
                let lib_half_w = lib_end.x.wrapping_sub(lib_start.x).wrapping_abs() / 2;
                let lib_half_h = lib_end.y.wrapping_sub(lib_start.y).wrapping_abs() / 2;

                let new_center = xform.apply(lib_center);
                let new_half_w = ki_round(lib_half_w as f64 * xform.scale_x()).wrapping_abs();
                let new_half_h = ki_round(lib_half_h as f64 * xform.scale_y()).wrapping_abs();

                self.shape.shape.set_start(Vector2I::new(
                    new_center.x - new_half_w,
                    new_center.y - new_half_h,
                ));
                self.shape.shape.set_end(Vector2I::new(
                    new_center.x + new_half_w,
                    new_center.y + new_half_h,
                ));
            }
            _ => self.shape.on_footprint_transformed(env),
        }

        let angle = self.text_angle_env(env);
        eda_text::set_text_angle(self, env, angle);
        self.text.clear_render_cache();
        self.text.clear_bounding_box_cache();
    }

    /// Port of `PCB_TEXTBOX::Rotate`: slide the axis-aligned box, the rotation lives in the
    /// text angle.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        // Slide the axis-aligned box, rotation lives in the text angle.
        let start = self.shape.shape.start();
        let end = self.shape.shape.end();
        let old_center = Vector2D::new(
            start.x.wrapping_add(end.x) as f64 * 0.5,
            start.y.wrapping_add(end.y) as f64 * 0.5,
        );
        let mut new_center = old_center;
        rotate_point_around(
            &mut new_center,
            Vector2D::new(centre.x as f64, centre.y as f64),
            angle,
        );

        let delta = Vector2I::new(
            ki_round(new_center.x - old_center.x),
            ki_round(new_center.y - old_center.y),
        );
        self.shape.shape.set_start(start + delta);
        let end = self.shape.shape.end();
        self.shape.shape.set_end(end + delta);

        self.sync_lib_coords(env);

        let mut new_abs = self.text_angle_env(env) + angle;
        let new_abs = new_abs.normalize();
        eda_text::set_text_angle(self, env, new_abs);

        match env.parent.as_ref() {
            Some(parent) => {
                self.lib_text_angle = EdaOrientation::new(new_abs - parent.transform.rotate())
            }
            None => self.lib_text_angle = EdaOrientation::new(new_abs),
        }
    }

    /// `PCB_SHAPE::SetLayer`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        self.shape.set_layer(env, layer)
    }

    /// `PCB_SHAPE::SetLayerSet`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        self.shape.set_layer_set(env, layers)
    }

    /// `PCB_SHAPE::SetPosition (PCB_TEXTBOX)`: `setPosition()` (no lib-frame sync in C++
    /// either).
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.shape.shape.set_position(pos);
    }

    /// Port of `PCB_TEXTBOX::StyleFromSettings`.
    pub fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        self.shape.style_from_settings(env, settings, check_side);

        let layer = self.shape.base.item.layer;

        self.set_text_size(env, settings.get_text_size(layer), true);
        self.set_text_thickness(env, settings.get_text_thickness(layer));
        self.text.set_italic(settings.get_text_italic(layer));

        // GetParentFootprint()
        if env.parent.is_some() {
            self.text.set_keep_upright(settings.get_text_upright(layer));
        }

        if check_side {
            // board->IsBackLayer( GetLayer() ); IsBackLayer( GetLayer() ) without a board
            self.text.set_mirrored(env.layers.is_back_layer(layer));
        }
    }

    /// Port of `PCB_TEXTBOX::GetDrawPos()`: `GetDrawPos( false )`.
    pub fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        draw_pos_flipped(self, ctx, false)
    }

    /// `PCB_TEXTBOX::getFontMetrics()`: `GetFontMetrics()` (the default metrics).
    pub fn font_metrics(&self, _ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        kicad_font::Metrics::default_metrics()
    }

    /// Port of `PCB_TEXTBOX::SetTextAngle`: rotate the box about its position by the change.
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        let delta = angle.normalized() - self.text_angle_env(env);
        let position = crate::eda_shape::shape_position(self);
        self.rotate(env, position, delta);
    }

    /// Port of `PCB_TEXTBOX::SetTextSize`: the parent footprint's scales are stripped.
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        let mut new_size = size;

        if let Some(parent) = env.parent.as_ref() {
            let xf = &parent.transform;
            new_size = Vector2I::new(
                ki_round(new_size.x as f64 / xf.scale_x().abs()),
                ki_round(new_size.y as f64 / xf.scale_y().abs()),
            );
        }

        eda_text::set_text_size(self, env, new_size, enforce_min);
    }

    /// Port of `PCB_TEXTBOX::SetTextThickness`: the parent footprint's mean scale is
    /// stripped.
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        let mut width = width;

        if let Some(parent) = env.parent.as_ref() {
            let xf = &parent.transform;
            let factor = (xf.scale_x().abs() + xf.scale_y().abs()) * 0.5;
            width = ki_round(width as f64 / factor);
        }

        eda_text::set_text_thickness(self, env, width);
    }

    /// Port of `PCB_TEXTBOX::GetShownText`: the resolved text, line-broken to the box width.
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        let parent_footprint = board_item::parent_footprint(self, ctx);
        let has_board = board_item::has_board(self, ctx);
        let depth_cell = Cell::new(depth);

        let resolver = |token: &mut String| -> bool {
            if token == "LAYER" {
                *token = board_item::layer_name(self, ctx);
                return true;
            }

            if let Some(fp) = parent_footprint {
                if footprint_resolve_text_var(fp, ctx, token, None, depth_cell.get() + 1) {
                    return true;
                }
            }

            if has_board
                && ctx.board.resolve_text_var(
                    token,
                    &ctx.board.current_variant,
                    depth_cell.get() + 1,
                )
            {
                return true;
            }

            false
        };

        let mut text = eda_text::shown_text(self, ctx, context, depth);

        if self.text.has_text_vars() && context != ResolutionContext::RawValue {
            text = resolve_text_vars_shared_depth(&text, &resolver, &depth_cell);
            finalize_text_var_expansion(&mut text, context);
        }

        linebreak_to_box(self, ctx, &mut text);

        text
    }

    /// Port of `PCB_TEXTBOX::GetTextAngle`: the lib angle plus the parent footprint's
    /// orientation, normalised; the lib angle without one.
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => {
                let angle = self.lib_text_angle.angle() + fp.orientation();
                angle.normalized()
            }
            None => self.lib_text_angle.angle(),
        }
    }

    /// Port of `PCB_TEXTBOX::GetTextSize`: the stored size scaled by the parent footprint's
    /// scales.
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        let lib_size = eda_text::text_size(self, ctx);

        match board_item::parent_footprint(self, ctx) {
            Some(fp) => {
                let xf = fp.transform();
                Vector2I::new(
                    ki_round(lib_size.x as f64 * xf.scale_x().abs()),
                    ki_round(lib_size.y as f64 * xf.scale_y().abs()),
                )
            }
            None => lib_size,
        }
    }

    /// Port of `PCB_TEXTBOX::GetTextThickness`: the stored thickness scaled by the parent
    /// footprint's mean scale.
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        let lib_thickness = eda_text::text_thickness(self, ctx);

        match board_item::parent_footprint(self, ctx) {
            Some(fp) => {
                let xf = fp.transform();
                let factor = (xf.scale_x().abs() + xf.scale_y().abs()) * 0.5;
                ki_round(lib_thickness as f64 * factor)
            }
            None => lib_thickness,
        }
    }

    /// Port of `PCB_TEXTBOX::GetBotRight`.
    pub fn bot_right(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        bot_right(self, ctx)
    }

    /// Port of `PCB_TEXTBOX::GetCornersInSequence`.
    pub fn corners_in_sequence(&self, ctx: &BoardCtx<'_>, angle: EdaAngle) -> Vec<Vector2I> {
        corners_in_sequence(self, ctx, angle)
    }

    /// Port of `PCB_SHAPE::getHatchingKnockouts (PCB_TEXTBOX)`.
    pub fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        hatching_knockouts(self, ctx)
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

    /// Port of `PCB_TEXTBOX::GetTopLeft`.
    pub fn top_left(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        top_left(self, ctx)
    }

    /// `PCB_SHAPE::GetWidth`, inherited by `PCB_TEXTBOX` (the base body reaches no virtual that `PCB_TEXTBOX` overrides).
    pub fn width(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.shape.width(ctx)
    }
}

// ---- PCB_TEXTBOX bodies as generic functions over the host (PcbTextbox, PcbTablecell) ----

/// A `PCB_TEXTBOX` host: the text box itself or a table cell (which overrides
/// `GetShownText()` and `IsFilledForHitTesting()`).
pub(crate) trait TextboxHost: BoardItem + EdaTextItem + EdaShapeItem {
    /// The `PCB_TEXTBOX` part.
    fn textbox(&self) -> &PcbTextbox;
    /// `this` as an `AnyItem` (for rule evaluation and `ctx.view`).
    fn any_item(&self) -> AnyItem<'_>;
}

impl TextboxHost for PcbTextbox {
    fn textbox(&self) -> &PcbTextbox {
        self
    }
    fn any_item(&self) -> AnyItem<'_> {
        AnyItem::Textbox(self)
    }
}

/// Port of `PCB_TEXTBOX::GetCornersInSequence( angle )`: the four corners of the box, in
/// reading order for the text angle; `EDA_SHAPE::GetCornersInSequence` for a polygon box.
pub(crate) fn corners_in_sequence<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    mut angle: EdaAngle,
) -> Vec<Vector2I> {
    let es = host.eda_shape();

    if es.shape() != ShapeT::Rectangle {
        return crate::eda_shape::corners_in_sequence(host, ctx, angle);
    }

    let (start, end) = (es.start(), es.end());
    let left = start.x.min(end.x);
    let right = start.x.max(end.x);
    let top = start.y.min(end.y);
    let bottom = start.y.max(end.y);

    let center = Vector2I::new(left.wrapping_add(right) / 2, top.wrapping_add(bottom) / 2);

    let mut pts = vec![
        Vector2I::new(left, top),
        Vector2I::new(right, top),
        Vector2I::new(right, bottom),
        Vector2I::new(left, bottom),
    ];

    angle.normalize();

    if !angle.is_zero() {
        for p in pts.iter_mut() {
            rotate_point_around(p, center, angle);
        }
    }

    pts
}

/// Port of `PCB_TEXTBOX::GetCorners()`: `GetCornersInSequence( GetDrawRotation() )`.
pub(crate) fn corners<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
    corners_in_sequence(host, ctx, host.draw_rotation(ctx))
}

/// Port of `PCB_TEXTBOX::GetTopLeft()`.
pub(crate) fn top_left<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vector2I {
    let rotation = host.draw_rotation(ctx);
    let es = host.eda_shape();
    let (start, end) = (es.start(), es.end());

    if rotation == ANGLE_90 {
        Vector2I::new(start.x, end.y)
    } else if rotation == ANGLE_180 {
        end
    } else if rotation == ANGLE_270 {
        Vector2I::new(end.x, start.y)
    } else {
        start
    }
}

/// Port of `PCB_TEXTBOX::GetBotRight()`.
pub(crate) fn bot_right<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vector2I {
    let rotation = host.draw_rotation(ctx);
    let es = host.eda_shape();
    let (start, end) = (es.start(), es.end());

    if rotation == ANGLE_90 {
        Vector2I::new(end.x, start.y)
    } else if rotation == ANGLE_180 {
        start
    } else if rotation == ANGLE_270 {
        Vector2I::new(start.x, end.y)
    } else {
        end
    }
}

/// Port of `PCB_TEXTBOX::GetMinSize()`: only the height is constrained (the text height plus
/// the margins across it); the width is 0 so that the text can rewrap.
pub(crate) fn min_size<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vector2I {
    let tb = host.textbox();

    if host.eda_text().text().is_empty() {
        return Vector2I::new(0, 0);
    }

    let text_box = eda_text::text_box(host, ctx, None);

    let mut text_height = (text_box.height().wrapping_abs()) as i32;

    if host.text_angle(ctx).is_horizontal() {
        text_height += tb.margin_top + tb.margin_bottom;
    } else {
        text_height += tb.margin_left + tb.margin_right;
    }

    // Only enforce minimum height. Width returns 0 so the user can freely shrink width
    // (text rewraps) while height is constrained to fit the wrapped text content.
    // GetTextBox returns dimensions in text-local coordinates. For 90/270 degree rotations,
    // the text's natural height maps to screen x-axis.
    let rotation = host.draw_rotation(ctx);

    if rotation == ANGLE_90 || rotation == ANGLE_270 {
        return Vector2I::new(text_height, 0);
    }

    Vector2I::new(0, text_height)
}

/// Port of `PCB_TEXTBOX::GetDrawPos( aIsFlipped )`: the anchor on the (rotated) box for the
/// justification, moved inwards by the margins.
pub(crate) fn draw_pos_flipped<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    is_flipped: bool,
) -> Vector2I {
    let tb = host.textbox();
    let et = host.eda_text();
    let draw_angle = host.draw_rotation(ctx);
    let corners = corners_in_sequence(host, ctx, draw_angle);
    let mut horizontal_alignment = et.horiz_justify();
    let vertical_alignment = et.vert_justify();
    let mut text_anchor = Vector2I::default();
    let mut offset = Vector2I::default();

    // Calculate midpoints
    let mid_top = (corners[0] + corners[1]) / 2;
    let mid_bottom = (corners[3] + corners[2]) / 2;
    let mid_left = (corners[0] + corners[3]) / 2;
    let mid_right = (corners[1] + corners[2]) / 2;
    let center = (corners[0] + corners[1] + corners[2] + corners[3]) / 4;

    if et.is_mirrored() != is_flipped {
        horizontal_alignment = match et.horiz_justify() {
            GrTextHAlignT::Left => GrTextHAlignT::Right,
            GrTextHAlignT::Center => GrTextHAlignT::Center,
            GrTextHAlignT::Right => GrTextHAlignT::Left,
            GrTextHAlignT::Indeterminate => GrTextHAlignT::Indeterminate,
        };
    }

    // wxASSERT_MSG: "Indeterminate state legal only in dialogs."
    if horizontal_alignment == GrTextHAlignT::Indeterminate
        || vertical_alignment == GrTextVAlignT::Indeterminate
    {
        return center;
    }

    use GrTextHAlignT as H;
    use GrTextVAlignT as V;

    match (horizontal_alignment, vertical_alignment) {
        (H::Left, V::Top) => text_anchor = corners[0],
        (H::Center, V::Top) => text_anchor = mid_top,
        (H::Right, V::Top) => text_anchor = corners[1],
        (H::Left, V::Center) => text_anchor = mid_left,
        (H::Center, V::Center) => text_anchor = center,
        (H::Right, V::Center) => text_anchor = mid_right,
        (H::Left, V::Bottom) => text_anchor = corners[3],
        (H::Center, V::Bottom) => text_anchor = mid_bottom,
        (H::Right, V::Bottom) => text_anchor = corners[2],
        _ => {}
    }

    if horizontal_alignment == H::Left {
        offset.x = tb.margin_left;
    } else if horizontal_alignment == H::Right {
        offset.x = -tb.margin_right;
    }

    if vertical_alignment == V::Top {
        offset.y = tb.margin_top;
    } else if vertical_alignment == V::Bottom {
        offset.y = -tb.margin_bottom;
    }

    kicad_geom::rotate_point(&mut offset, host.draw_rotation(ctx));
    text_anchor + offset
}

/// The line breaking that closes `PCB_TEXTBOX::GetShownText` / `PCB_TABLECELL::GetShownText`:
/// `LinebreakText` to the width between the margins across the reading direction.
pub(crate) fn linebreak_to_box<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    text: &mut String,
) {
    let tb = host.textbox();
    let et = host.eda_text();
    let lookup = et.draw_font_lookup();
    let font = lookup.font();
    let draw_angle = host.draw_rotation(ctx);
    let corners = corners_in_sequence(host, ctx, draw_angle);
    let mut col_width: i32 = (corners[1] - corners[0]).euclidean_norm();

    if host.text_angle(ctx).is_horizontal() {
        col_width -= tb.margin_left + tb.margin_right;
    } else {
        col_width -= tb.margin_top + tb.margin_bottom;
    }

    font.linebreak_text(
        text,
        col_width,
        host.text_size(ctx),
        eda_text::effective_text_pen_width(host, ctx, 0),
        et.is_bold(),
        et.is_italic(),
    );
}

/// Port of `PCB_TEXTBOX::GetBoundingBox()`: the corners, inflated by half the border width.
pub(crate) fn bounding_box<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Box2I {
    let mut bbox = Box2I::default();

    for pt in corners(host, ctx) {
        bbox.merge_point(pt);
    }

    bbox.inflate(0.max(host.width(ctx)) / 2);
    bbox.normalize();

    bbox
}

/// Port of `PCB_TEXTBOX::HitTest( aPosition, aAccuracy )`.
pub(crate) fn hit_test<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    pos: Vector2I,
    accuracy: i32,
) -> bool {
    let mut rect = bounding_box(host, ctx);

    rect.inflate(accuracy);

    rect.contains(pos)
}

/// Port of `PCB_TEXTBOX::HitTest( aRect, aContained, aAccuracy )`.
pub(crate) fn hit_test_rect<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    rect: &Box2I,
    contained: bool,
    accuracy: i32,
) -> bool {
    let mut rect = *rect;

    rect.inflate(accuracy);

    if contained {
        return rect.contains_box(&bounding_box(host, ctx));
    }

    rect.intersects(&bounding_box(host, ctx))
}

/// Port of `PCB_SHAPE::GetSolderMaskExpansion()` with the text box host as `this` (rule
/// evaluation sees the text box or cell).
fn solder_mask_expansion<H: TextboxHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    let shape = &host.textbox().shape;
    let mut margin = 0;
    let has_board = board_item::has_board(host, ctx);
    let drc_engine = ctx.rules.filter(|engine| {
        has_board
            && engine.has_rules_for_constraint_type(DrcConstraintT::SolderMaskExpansionConstraint)
    });

    if let Some(drc_engine) = drc_engine {
        let this = ctx.view(host.any_item());
        let constraint = drc_engine.eval_rules(
            ctx,
            DrcConstraintT::SolderMaskExpansionConstraint,
            Some(this),
            None,
            shape.base.item.layer,
        );

        if constraint.value.has_opt() {
            margin = constraint.value.opt();
        }
    } else if let Some(local) = shape.solder_mask_margin {
        margin = local;
    } else if has_board {
        margin = ctx.board.design_settings().solder_mask_expansion;
    }

    // Ensure the resulting mask opening has a non-negative size
    if margin < 0 && !shape.shape.is_solid_fill() {
        margin = margin.max(-host.width(ctx) / 2);
    }

    margin
}

/// Port of `PCB_SHAPE::GetEffectiveShape()` over the text box host.
fn pcb_shape_effective_shape<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
) -> SharedShape {
    let shape = &host.textbox().shape;
    let mut width = host.effective_width(ctx);

    if is_solder_mask_layer(layer)
        && shape.has_solder_mask
        && is_copper_layer(shape.base.item.layer)
    {
        width += 2 * solder_mask_expansion(host, ctx);
    }

    Arc::new(ShapeCompound::from_shapes(
        make_effective_shapes_with_line_endings(host, ctx, width),
    ))
}

/// Port of `PCB_TEXTBOX::GetEffectiveShape()`: the text shape plus, with a non-negative
/// `PCB_SHAPE::GetStroke()` width (always: `GetWidth()` clamps at 0), the box shapes.
pub(crate) fn effective_shape<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    _flash: Flashing,
    _usage: DrcConstraintT,
) -> SharedShape {
    let text_shape = eda_text::effective_text_shape(host, ctx, true, None, ANGLE_0);
    let mut shape: ShapeCompound = Arc::unwrap_or_clone(text_shape);

    if host.width(ctx) >= 0 {
        shape.add_shape_shared(&pcb_shape_effective_shape(host, ctx, layer));
    }

    Arc::new(shape)
}

/// Port of `PCB_TEXTBOX::TransformTextToPolySet()`: the raw attributes (stored size and
/// angle, unlike `PCB_TEXT`), the knockout box rotated by the text angle, `Inflate` with
/// `simplify = true`.
pub(crate) fn transform_text_to_poly_set<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    clearance: i32,
    max_error: i32,
    error_loc: ErrorLoc,
) {
    let et = host.eda_text();
    let lookup = et.draw_font_lookup();
    let font = lookup.font();
    let pen_width = eda_text::effective_text_pen_width(host, ctx, 0);
    let attrs = et.attributes.clone();
    let shown_text = host.shown_text(ctx, ResolutionContext::ForCanvas, 0);
    let mut clearance = clearance;

    // The polygonal shape of a text can have many basic shapes, so combining these shapes can
    // be very useful to create a final shape with a lot less vertices to speedup calculations.
    // Simplify shapes is not usually always efficient, but in this case it is.
    let text_shape = RefCell::new(ShapePolySet::new());

    {
        let mut callback_gal = CallbackGal::new(
            // Stroke callback
            |pt1, pt2| {
                transform_oval_to_polygon(
                    &mut text_shape.borrow_mut(),
                    pt1,
                    pt2,
                    pen_width,
                    max_error,
                    error_loc,
                    0,
                );
            },
            // Triangulation callback
            |glyph| {
                outline_glyph_triangulate(glyph, &mut |pt1, pt2, pt3| {
                    let mut shape = text_shape.borrow_mut();
                    shape.new_outline();

                    for point in [pt1, pt2, pt3] {
                        shape.append(point.x, point.y, -1, -1, false);
                    }
                });
            },
        );

        match eda_text::render_cache(host, ctx, &shown_text, Vector2I::default()) {
            Some(cache) => callback_gal.draw_glyphs(&cache.glyphs),
            None => font.draw(
                &mut callback_gal,
                &shown_text,
                host.draw_pos(ctx),
                &attrs,
                host.font_metrics(ctx),
            ),
        }
    }

    let mut text_shape = text_shape.into_inner();
    text_shape.simplify();

    if host.is_knockout() {
        let mut final_poly = ShapePolySet::new();

        host.transform_shape_to_polygon(
            ctx,
            &mut final_poly,
            host.base().layer,
            clearance,
            max_error,
            error_loc,
            false,
        );

        // A TEXTBOX can be rotated, but TransformShapeToPolygon does not handle the text
        // rotation. so rotate the finalPoly by the text rotation
        let angle = host.text_angle(ctx);

        if angle != ANGLE_0 {
            final_poly.rotate(angle, crate::eda_shape::shape_center(host));
        }

        final_poly.boolean_subtract(&text_shape);

        buffer.append_set(&final_poly);
    } else {
        if clearance > 0 || error_loc == ERROR_OUTSIDE {
            if error_loc == ERROR_OUTSIDE {
                clearance += max_error;
            }

            text_shape.inflate(clearance, CornerStrategy::RoundAllCorners, max_error, true);
        }

        buffer.append_set(&text_shape);
    }
}

/// Port of `PCB_TEXTBOX::TransformShapeToPolygon()`: the box treated as filled even without
/// a background colour, plus the border segments when the border is enabled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn transform_shape_to_polygon<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    _layer: PcbLayerId,
    clearance: i32,
    max_error: i32,
    error_loc: ErrorLoc,
    _ignore_line_width: bool,
) {
    // Don't use PCB_SHAPE::TransformShapeToPolygon.  We want to treat the textbox as filled
    // even if there's no background colour.
    let tb = host.textbox();
    let es = host.eda_shape();
    let width = host.width(ctx) + (2 * clearance);

    if es.shape() == ShapeT::Rectangle {
        let pts = es.rect_corners();

        buffer.new_outline();

        for pt in &pts {
            buffer.append(pt.x, pt.y, -1, -1, false);
        }

        if tb.border_enabled && width > 0 {
            // Add in segments
            for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
                transform_oval_to_polygon(buffer, pts[a], pts[b], width, max_error, error_loc, 0);
            }
        }
    } else if es.shape() == ShapeT::Poly {
        // Non-cardinally-rotated rect
        buffer.new_outline();

        let poly = es.poly_shape().c_outline(0);

        for ii in 0..poly.point_count() {
            let pt = poly.c_point(ii);
            buffer.append(pt.x, pt.y, -1, -1, false);
        }

        if tb.border_enabled && width > 0 {
            for ii in 0..poly.segment_count() {
                let seg = poly.segment(ii);
                transform_oval_to_polygon(buffer, seg.a, seg.b, width, max_error, error_loc, 0);
            }
        }
    }
}

/// Port of `PCB_SHAPE::TransformShapeToPolySet()` over the text box host:
/// `EDA_SHAPE::TransformShapeToPolygon( .., false, true )`.
pub(crate) fn transform_shape_to_poly_set<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    _layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
) {
    crate::eda_shape::shape_transform_to_polygon(
        host, ctx, buffer, clearance, error, error_loc, false, true,
    )
}

/// Port of `PCB_SHAPE::getHatchingKnockouts()` over the text box host (`this`, the virtual
/// `GetBoundingBox()` and the parent footprint are the text box's / cell's).
pub(crate) fn hatching_knockouts<H: TextboxHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
) -> ShapePolySet {
    let mut knockouts = ShapePolySet::new();

    if !board_item::has_board(host, ctx) {
        return knockouts;
    }

    let layer = host.base().layer;
    let bbox = host.bounding_box(ctx);
    let max_error = ARC_LOW_DEF;
    let this_id = host.base().id();

    let knockout_item = |item: ItemRef<'_>, knockouts: &mut ShapePolySet| {
        let mut margin = host.hatch_line_spacing(ctx) / 2;

        if item.type_() == KicadT::PcbTextboxT {
            margin = 0;
        }

        item.transform_shape_to_polygon(
            knockouts,
            layer,
            margin,
            max_error,
            ErrorLoc::Outside,
            false,
        );
    };

    for (id, _) in ctx.board.drawings() {
        if Some(id) == this_id {
            continue;
        }

        let Some(item) = ctx.item(id) else {
            continue;
        };

        if matches!(
            item.type_(),
            KicadT::PcbFieldT | KicadT::PcbTextT | KicadT::PcbTextboxT | KicadT::PcbShapeT
        ) && item.layer() == layer
            && item.bounding_box().intersects(&bbox)
        {
            knockout_item(item, &mut knockouts);
        }
    }

    let parent_fp = board_item::parent_footprint(host, ctx).and_then(|fp| fp.base.id());

    for (fp_id, footprint) in ctx.board.footprints() {
        if Some(ItemId::from(fp_id)) == parent_fp {
            continue;
        }

        // GetCourtyard() returns the front courtyard for any non-back layer, so only knock it
        // out when the hatched shape actually lives on a courtyard layer.
        if layer == PcbLayerId::FCrtYd || layer == PcbLayerId::BCrtYd {
            knockouts.append_set(&footprint.courtyard(ctx, layer));
        }

        // Knockout footprint fields
        footprint.run_on_children(ctx, RecurseMode::Recurse, &mut |item: ItemRef<'_>| {
            let kind = item.type_();
            let invisible_field = kind == KicadT::PcbFieldT
                && !item
                    .as_eda_text()
                    .is_some_and(|text| text.eda_text().is_visible());

            if (kind == KicadT::PcbFieldT || kind == KicadT::PcbShapeT)
                && item.layer() == layer
                && !invisible_field
                && item.bounding_box().intersects(&bbox)
            {
                knockout_item(item, &mut knockouts);
            }
        });
    }

    knockouts
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

    // ---- ports of qa/tests/pcbnew/test_pcb_textbox.cpp (model parts) ----------------------

    const MM: i32 = 1_000_000;

    /// `PCB_TEXTBOX_FIXTURE`: "Hello World", 1 mm text, box (0, 0) - (20 mm, 5 mm) on a board.
    fn fixture() -> (crate::board::Board, PcbTextbox) {
        let board = crate::board::Board::new();
        let env = MutEnv::detached();
        let mut tb = PcbTextbox::default();
        tb.shape.base.item.parent = crate::item::ParentRef::Board;
        tb.text.set_text("Hello World");
        tb.set_text_size(&env, Vector2I::new(MM, MM), true);
        tb.set_start(&env, Vector2I::new(0, 0));
        tb.set_end(&env, Vector2I::new(20 * MM, 5 * MM));
        (board, tb)
    }

    /// `GetMinSizeReturnsHeightOnly`: width is unconstrained so text can rewrap, height is
    /// bounded by the wrapped content.
    #[test]
    fn get_min_size_returns_height_only() {
        let (board, tb) = fixture();
        let ctx = BoardCtx::bare(&board);
        let min_size = tb.min_size(&ctx);
        assert_eq!(min_size.x, 0);
        assert!(min_size.y > 0);
    }

    /// `GetMinSizeGrowsWithMoreText`.
    #[test]
    fn get_min_size_grows_with_more_text() {
        let (board, mut tb) = fixture();
        let short = tb.min_size(&BoardCtx::bare(&board));
        tb.text.set_text("Hello World\nSecond Line\nThird Line");
        tb.invalidate_caches();
        let long = tb.min_size(&BoardCtx::bare(&board));
        assert!(long.y > short.y);
    }

    /// `GetMinSizeIncludesMargins`.
    #[test]
    fn get_min_size_includes_margins() {
        let (board, mut tb) = fixture();
        tb.text.set_text("A");
        (
            tb.margin_left,
            tb.margin_right,
            tb.margin_top,
            tb.margin_bottom,
        ) = (0, 0, 0, 0);
        let no_margin = tb.min_size(&BoardCtx::bare(&board));
        tb.margin_top = MM;
        tb.margin_bottom = MM;
        let with_margin = tb.min_size(&BoardCtx::bare(&board));
        assert_eq!((no_margin.x, with_margin.x), (0, 0));
        assert!(with_margin.y > no_margin.y);
    }

    /// `GetMinSizeHeightAtLeastTextHeight`.
    #[test]
    fn get_min_size_height_at_least_text_height() {
        let (board, mut tb) = fixture();
        tb.text.set_text("A");
        let ctx = BoardCtx::bare(&board);
        assert!(tb.min_size(&ctx).y >= tb.text_size(&ctx).y);
    }

    /// `GetMinSizeEmptyText`.
    #[test]
    fn get_min_size_empty_text() {
        let (board, mut tb) = fixture();
        tb.text.set_text("");
        assert_eq!(tb.min_size(&BoardCtx::bare(&board)), Vector2I::new(0, 0));
    }

    /// `GetMinSizeRotated90`: at 0 degrees the bound is on y, at 90 degrees it moves to x.
    #[test]
    fn get_min_size_rotated_90() {
        let (board, mut tb) = fixture();
        tb.text.set_text("Wide Text");
        let min0 = tb.min_size(&BoardCtx::bare(&board));
        tb.set_text_angle(&MutEnv::detached(), EdaAngle::from_degrees(90.0));
        let min90 = tb.min_size(&BoardCtx::bare(&board));
        assert_eq!(min0.x, 0);
        assert!(min0.y > 0);
        assert!(min90.x > 0);
        assert_eq!(min90.y, 0);
    }

    /// `GetMinSizeAllowsWidthReduction`.
    #[test]
    fn get_min_size_allows_width_reduction() {
        let (board, mut tb) = fixture();
        tb.text
            .set_text("This is a very long line of text that should exceed the box width");
        let min_size = tb.min_size(&BoardCtx::bare(&board));
        assert_eq!(min_size.x, 0);
        assert!(min_size.y > 0);
    }

    /// `ThirtyDegreeIncrementsToCardinalKeepsVisualRotated`: guards a past snap-back where
    /// 3x30 degrees landed on cardinal and GetCornersInSequence returned an un-rotated AABB.
    #[test]
    fn thirty_degree_increments_to_cardinal_keeps_visual_rotated() {
        let (board, mut tb) = fixture();
        let env = MutEnv::detached();

        for _ in 0..3 {
            tb.rotate(&env, Vector2I::new(0, 0), EdaAngle::from_degrees(30.0));
        }

        let ctx = BoardCtx::bare(&board);
        assert!((tb.text_angle(&ctx).as_degrees() - 90.0).abs() < 1e-6 * 90.0);

        let corners = tb.corners(&ctx);
        assert_eq!(corners.len(), 4);
        let mut visual = Box2I::default();

        for p in &corners {
            visual.merge_point(*p);
        }

        assert!((visual.width() - (5 * MM) as i64).abs() <= 1);
        assert!((visual.height() - (20 * MM) as i64).abs() <= 1);
    }

    /// `NonCardinalRotationDoesNotCrash`.
    #[test]
    fn non_cardinal_rotation_does_not_crash() {
        let (board, mut tb) = fixture();
        tb.rotate(
            &MutEnv::detached(),
            Vector2I::new(0, 0),
            EdaAngle::from_degrees(30.0),
        );
        let ctx = BoardCtx::bare(&board);
        let corners = tb.corners(&ctx);
        assert_eq!(corners.len(), 4);
        let mut bbox = Box2I::default();

        for p in &corners {
            bbox.merge_point(*p);
        }

        assert!(bbox.width() > 0 && bbox.height() > 0);
        let _ = tb.draw_pos(&ctx);
    }

    /// `CardinalRotationsRoundTrip`.
    #[test]
    fn cardinal_rotations_round_trip() {
        let (_board, mut tb) = fixture();
        let (start, end) = (tb.shape.shape.start(), tb.shape.shape.end());
        let pivot = Vector2I::new(7 * MM, 11 * MM);

        for _ in 0..4 {
            tb.rotate(&MutEnv::detached(), pivot, EdaAngle::from_degrees(90.0));
        }

        assert_eq!(tb.shape.shape.start(), start);
        assert_eq!(tb.shape.shape.end(), end);
    }

    /// `NonCardinalRotationKeepsLibShapeRectangle`: the on-disk form must stay a rectangle
    /// even after a tilted rotation.
    #[test]
    fn non_cardinal_rotation_keeps_lib_shape_rectangle() {
        let (_board, mut tb) = fixture();
        tb.rotate(
            &MutEnv::detached(),
            Vector2I::new(0, 0),
            EdaAngle::from_degrees(30.0),
        );
        assert_eq!(tb.shape.lib_shape(), ShapeT::Rectangle);
    }

    /// `TextBoxInFootprintKeepsRectangleShape` / the lib-frame round trip of
    /// `test_footprint_transform_sync.cpp`: a text box in a rotated, flipped, non-uniformly
    /// scaled footprint re-bakes from its library rectangle, and `syncLibCoords()` inverts
    /// `OnFootprintTransformed()` (centre and half-extents).
    #[test]
    fn footprint_child_sync_inverts_rebake() {
        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Textbox).typed::<PcbTextbox>().unwrap();
        let mut tb = PcbTextbox::default();
        tb.shape.lib.start = Vector2I::new(-2 * MM, -MM);
        tb.shape.lib.end = Vector2I::new(4 * MM, 3 * MM);
        fx.set_child(id, tb);
        fx.board
            .modify_textbox(id, |t, env| t.on_footprint_transformed(env));

        let t = &fx.board[id];
        assert_eq!(t.shape.shape.shape(), ShapeT::Rectangle);
        let (lib_start, lib_end) = (t.shape.lib.start, t.shape.lib.end);
        fx.board.modify_textbox(id, |t, env| t.sync_lib_coords(env));
        let t = &fx.board[id];
        assert!((t.shape.lib.start - lib_start).euclidean_norm() <= 1);
        assert!((t.shape.lib.end - lib_end).euclidean_norm() <= 1);
    }
}
