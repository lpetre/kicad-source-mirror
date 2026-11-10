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
//! delegation shims (package M0c), the `PCB_TEXT` overrides (package M10). The bodies that
//! reach a virtual `PCB_FIELD` overrides are generic functions over the host (`PcbTextHost`),
//! shared with `pcb_field.rs`.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use kicad_core::layer_ids::is_back_layer;
use kicad_core::lset::Lset;
use kicad_core::string_utils::unescape_string;
use kicad_core::text_vars::{ResolutionContext, finalize_text_var_expansion};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId, TextAttributes};
use kicad_font::gr_text::get_knockout_text_margin;
use kicad_font::{CallbackGal, FontExt, TextGal};
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::{ERROR_INSIDE, ERROR_OUTSIDE, ErrorLoc};
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, Box2I, EdaAngle, EdaOrientation, FlipDirection, Shape,
    ShapePolySet, Vector2I, ki_round, mirror_point, rotate_point_around,
};

use crate::board_design_settings::BoardDesignSettings;
use crate::board_item;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_text::{
    self, EdaText, eda_text_offset, negate_h_align, negate_v_align, outline_glyph_triangulate,
    resolve_text_vars_shared_depth,
};
use crate::footprint::Footprint;
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
        text_hit_test(self, ctx, point, accuracy)
    }

    /// `PCB_TEXT::TextHitTest( aRect, aContains, aAccuracy )`.
    pub fn text_hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contains: bool,
        accuracy: i32,
    ) -> bool {
        text_hit_test_rect(self, ctx, rect, contains, accuracy)
    }

    /// `PCB_TEXT::GetTextTypeDescription()` (virtual, introduced by `PCB_TEXT`; `PCB_FIELD`
    /// overrides).
    pub fn text_type_description(&self, _ctx: &BoardCtx<'_>) -> String {
        "Text".to_string()
    }

    /// `PCB_TEXT::GetKnockoutCache( aFont, forResolvedText, aMaxError )` (`aMaxError` = the
    /// board's `GetMaxError()`; the font is the draw font).
    pub fn knockout_cache(&self, ctx: &BoardCtx<'_>, resolved_text: &str) -> ShapePolySet {
        knockout_cache(self, ctx, &self.knockout_cache, resolved_text)
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
        transform_text_to_poly_set(self, ctx, buffer, clearance, max_error, error_loc)
    }

    /// `PCB_TEXT::KeepUpright()`: an edit-time fold of the stored angle into `[0, 180)`,
    /// swapping the justification.
    pub fn keep_upright(&mut self, env: &MutEnv) {
        if !self.text.is_keep_upright() {
            return;
        }

        let mut new_angle = self.text_angle_env(env);
        new_angle.normalize();

        let needs_flipped = new_angle >= ANGLE_180;

        if needs_flipped {
            let h = negate_h_align(self.text.horiz_justify());
            let v = negate_v_align(self.text.vert_justify());
            self.text.set_horiz_justify(h);
            self.text.set_vert_justify(v);
            new_angle += ANGLE_180;
            new_angle.normalize();
            self.set_text_angle(env, new_angle);
        }
    }

    /// `PCB_TEXT::GetTextAngle()` as a mutator sees it (the parent frame of `env`).
    pub(crate) fn text_angle_env(&self, env: &MutEnv) -> EdaAngle {
        match env.parent.as_ref() {
            Some(parent) => {
                let mut angle = self.lib_text_angle.angle() + parent.transform.rotate();
                angle.normalize()
            }
            None => self.lib_text_angle.angle(),
        }
    }

    /// `PCB_TEXT::SetLibTextPos()`: `EDA_TEXT::Offset( aPos - EDA_TEXT::GetTextPos() )`.
    pub fn set_lib_text_pos(&mut self, pos: Vector2I) {
        let delta = pos - self.text.lib_pos;
        eda_text_offset(&mut self.text, delta);
    }

    /// `PCB_TEXT::SetLibTextSize()`: `EDA_TEXT::SetTextSize( aSize )` (enforcing the limits).
    pub fn set_lib_text_size(&mut self, size: Vector2I) {
        eda_text::set_text_size(self, &MutEnv::detached(), size, true);
    }

    /// `PCB_TEXT::SetLibTextThickness()`: `EDA_TEXT::SetTextThickness( aWidth )`.
    pub fn set_lib_text_thickness(&mut self, width: i32) {
        eda_text::set_text_thickness(self, &MutEnv::detached(), width);
    }
}

// ---- PCB_TEXT bodies as generic functions over the host (PcbText, PcbField) --------------
//
// `PCB_FIELD` overrides `GetShownText`, which these bodies reach virtually; they are written
// once over the host so that the field's override applies (`board-model.md` 6.2).

/// The bound of a `PCB_TEXT` host: the board item and its `EDA_TEXT` interface.
pub(crate) trait PcbTextHost: BoardItem + EdaTextItem {}

impl<T: BoardItem + EdaTextItem + ?Sized> PcbTextHost for T {}

/// `KIUI::EllipsizeMenuText( aString )`: unescaped, line breaks and tabs as spaces, at most
/// 36 characters (34 + `...`).
pub(crate) fn ellipsize_menu_text(text: &str) -> String {
    let mut msg = unescape_string(text);

    msg = msg.replace('\n', " ");
    msg = msg.replace('\r', " ");
    msg = msg.replace('\t', " ");

    if msg.chars().count() > 36 {
        msg = msg.chars().take(34).collect::<String>() + "...";
    }

    msg
}

/// `FOOTPRINT::ResolveTextVar( aToken, aDepth )` (`variant == None`) /
/// `( aToken, aVariantName, aDepth )`.
pub(crate) fn footprint_resolve_text_var(
    fp: &Footprint,
    ctx: &BoardCtx<'_>,
    token: &mut String,
    variant: Option<&str>,
    depth: i32,
) -> bool {
    match variant {
        Some(v) => fp.resolve_text_var_for_variant(ctx, token, v, depth),
        None => fp.resolve_text_var(ctx, token, depth),
    }
}

/// `PCB_TEXT::getKnockoutMargin()`.
pub(crate) fn knockout_margin<H: PcbTextHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    get_knockout_text_margin(
        Vector2I::new(
            eda_text::text_width(host, ctx),
            eda_text::text_height(host, ctx),
        ),
        eda_text::effective_text_pen_width(host, ctx, 0),
    )
}

/// `PCB_TEXT::GetBoundingBox()`.
pub(crate) fn bounding_box<H: PcbTextHost + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Box2I {
    let angle = host.draw_rotation(ctx);
    let mut rect = eda_text::text_box(host, ctx, None);

    if host.is_knockout() {
        rect.inflate(knockout_margin(host, ctx));
    }

    if !angle.is_zero() {
        rect = rect.bounding_box_rotated(host.text_pos(ctx), angle);
    }

    rect
}

/// `PCB_TEXT::TextHitTest( aPoint, aAccuracy )`.
pub(crate) fn text_hit_test<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    point: Vector2I,
    accuracy: i32,
) -> bool {
    let mut accuracy = accuracy;

    if host.is_knockout() {
        accuracy += get_knockout_text_margin(
            host.text_size(ctx),
            eda_text::effective_text_pen_width(host, ctx, 0),
        );
    }

    eda_text::text_hit_test(host, ctx, point, accuracy)
}

/// `PCB_TEXT::TextHitTest( aRect, aContains, aAccuracy )`.
pub(crate) fn text_hit_test_rect<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    rect: &Box2I,
    contains: bool,
    accuracy: i32,
) -> bool {
    let mut rect = *rect;

    rect.inflate(accuracy);

    if contains {
        return rect.contains_box(&bounding_box(host, ctx));
    }

    rect.intersects(&bounding_box(host, ctx))
}

/// `PCB_TEXT::GetEffectiveShape()`: the knockout polygons, else `GetEffectiveTextShape()`.
pub(crate) fn effective_shape<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
) -> SharedShape {
    if host.is_knockout() {
        let mut poly = ShapePolySet::new();

        transform_text_to_poly_set(
            host,
            ctx,
            &mut poly,
            0,
            board_item::max_error(host, ctx),
            ERROR_INSIDE,
        );

        return Arc::new(poly);
    }

    eda_text::effective_text_shape(host, ctx, true, None, ANGLE_0)
}

/// `PCB_TEXT::GetKnockoutCache( aFont, forResolvedText, aMaxError )`.
pub(crate) fn knockout_cache<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    cache: &KeyedCache<PcbTextKnockoutCacheData>,
    resolved_text: &str,
) -> ShapePolySet {
    let mut attrs = host.eda_text().attributes.clone();
    let size = host.text_size(ctx);
    attrs.size = (size.x, size.y);
    let draw_angle = host.draw_rotation(ctx);
    let draw_pos = host.draw_pos(ctx);

    if let Some(current) = cache.peek() {
        if !current.cache.is_empty()
            && current.text_attrs == attrs
            && current.text == resolved_text
            && current.angle == draw_angle
        {
            if current.pos == draw_pos {
                return current.cache.clone();
            }

            let mut moved = (*current).clone();
            moved.cache.move_(draw_pos - current.pos);
            moved.pos = draw_pos;
            let result = moved.cache.clone();
            cache.store(moved);
            return result;
        }
    }

    let mut poly = ShapePolySet::new();

    transform_text_to_poly_set(
        host,
        ctx,
        &mut poly,
        0,
        board_item::max_error(host, ctx),
        ERROR_INSIDE,
    );
    poly.fracture(true);

    cache.store(PcbTextKnockoutCacheData {
        text: resolved_text.to_string(),
        text_attrs: attrs,
        angle: draw_angle,
        pos: draw_pos,
        cache: poly.clone(),
    });

    poly
}

/// `PCB_TEXT::buildBoundingHull( aBuffer, aRenderedText, aClearance )`: the bounding box of
/// the rendered text in the text's own frame, inflated by `clearance`, rotated back and
/// appended as one outline.
pub(crate) fn build_bounding_hull<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    rendered_text: &ShapePolySet,
    clearance: i32,
) {
    let mut poly = rendered_text.clone();
    let draw_rotation = host.draw_rotation(ctx);
    let draw_pos = host.draw_pos(ctx);

    poly.rotate(-draw_rotation, draw_pos);

    let rect = poly.bbox(clearance);
    let mut corners = [Vector2I::default(); 4];

    corners[0].x = rect.origin().x;
    corners[0].y = rect.origin().y;
    corners[1].y = corners[0].y;
    corners[1].x = rect.right();
    corners[2].x = corners[1].x;
    corners[2].y = rect.bottom();
    corners[3].y = corners[2].y;
    corners[3].x = corners[0].x;

    buffer.new_outline();

    for corner in corners.iter_mut() {
        rotate_point_around(corner, draw_pos, draw_rotation);
        buffer.append(corner.x, corner.y, -1, -1, false);
    }
}

/// `PCB_TEXT::TransformTextToPolySet( aBuffer, aClearance, aMaxError, aErrorLoc )`.
pub(crate) fn transform_text_to_poly_set<H: PcbTextHost + ?Sized>(
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
    let mut attrs = et.attributes.clone();
    let shown_text = host.shown_text(ctx, ResolutionContext::ForCanvas, 0);
    let mut clearance = clearance;

    attrs.set_angle(host.draw_rotation(ctx).as_degrees());
    let size = host.text_size(ctx);
    attrs.size = (size.x, size.y);

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
                host.text_pos(ctx),
                &attrs,
                kicad_font::Metrics::default_metrics(),
            ),
        }
    }

    let mut text_shape = text_shape.into_inner();
    text_shape.simplify();

    if host.is_knockout() {
        let mut final_poly = ShapePolySet::new();
        let margin = get_knockout_text_margin(Vector2I::new(attrs.size.0, attrs.size.1), pen_width);

        build_bounding_hull(host, ctx, &mut final_poly, &text_shape, margin + clearance);
        final_poly.boolean_subtract(&text_shape);

        buffer.append_set(&final_poly);
    } else {
        if clearance > 0 || error_loc == ERROR_OUTSIDE {
            if error_loc == ERROR_OUTSIDE {
                clearance += max_error;
            }

            text_shape.inflate(clearance, CornerStrategy::RoundAllCorners, max_error, false);
        }

        buffer.append_set(&text_shape);
    }
}

/// `PCB_TEXT::TransformShapeToPolygon()`: the text polygons, of which only the rotated
/// bounding hull is appended.
pub(crate) fn transform_shape_to_polygon<H: PcbTextHost + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    clearance: i32,
    max_error: i32,
    error_loc: ErrorLoc,
) {
    let mut poly = ShapePolySet::new();

    transform_text_to_poly_set(host, ctx, &mut poly, 0, max_error, error_loc);

    build_bounding_hull(host, ctx, buffer, &poly, clearance);
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
    fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        PcbText::style_from_settings(self, env, settings, check_side)
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
        bounding_box(self, ctx)
    }

    /// `PCB_TEXT::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        _layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        effective_shape(self, ctx)
    }

    /// `PCB_TEXT::HitTest`: `TextHitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        self.text_hit_test(ctx, pos, accuracy)
    }

    /// `PCB_TEXT::HitTest`: `TextHitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        self.text_hit_test_rect(ctx, rect, contained, accuracy)
    }

    /// `PCB_TEXT::GetItemDescription` (untranslated).
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

        if let Some(parent_fp) = board_item::parent_footprint(self, ctx) {
            let reference = parent_fp.reference(ctx);
            return format!("Footprint text of {reference} ({content})");
        }

        format!(
            "PCB text '{}' on {}",
            content,
            board_item::layer_name(self, ctx)
        )
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
        _layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        _ignore_line_width: bool,
    ) {
        transform_shape_to_polygon(self, ctx, buffer, clearance, error, error_loc)
    }

    /// `PCB_TEXT::Flip`: the position mirrors in the library frame of the parent footprint
    /// (board frame without one), the lib angle is reflected plus a half turn, the layer
    /// flips, and side-specific text is mirrored.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        // The position mirrors in the library or board frame depending on if this is in a FP.
        let lib_centre = match env.parent.as_ref() {
            Some(parent) => parent.transform.inverse_apply(centre),
            None => centre,
        };
        let axis = kicad_geom::kicore::mirror::mirror_axis(dir);

        let mut lib_pos = self.text.lib_pos;
        mirror_point(&mut lib_pos, lib_centre, dir);
        self.set_lib_text_pos(lib_pos);

        // A flip changes the side it is read from, so add a half turn on top of the reflection.
        self.lib_text_angle = self.lib_text_angle.reflect(axis) + ANGLE_180;

        let angle = self.text_angle_env(env);
        eda_text::set_text_angle(self, env, angle);

        let layer = env.layers.flip_layer(self.base.layer);
        self.set_layer(env, layer);

        // IsSideSpecific(): BOARD_ITEM::GetLayerSet() is LSET( { m_layer } )
        if env.is_side_specific(&Lset::from_layers(&[self.base.layer]), self.base.layer) {
            let mirrored = !self.text.is_mirrored();
            self.text.set_mirrored(mirrored);
        }
    }

    /// `PCB_TEXT::Mirror( aCentre, aFlipDirection )`: `Mirror( MIRROR_AXIS( .. ) )`; the
    /// position and justification are mirrored, but not the text itself.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        let axis = board_item::mirror_axis(centre, dir);
        let new_pos = axis.reflect_point(eda_text::text_pos_env(self, env));

        match env.parent.as_ref() {
            Some(parent) => {
                let lib = parent.transform.inverse_apply(new_pos);
                self.set_lib_text_pos(lib);
            }
            None => self.set_text_pos(env, new_pos),
        }

        let axis_angle = -EdaAngle::from_vector(axis.b - axis.a);
        let mut new_angle = self.text_angle_env(env).reflect(axis_angle).normalized();

        // If we're mirroring "left-right-ish", swap the justification and fix the text orientation.
        if new_angle > ANGLE_90 && new_angle <= ANGLE_270 {
            new_angle -= ANGLE_180;
            let h = negate_h_align(self.text.horiz_justify());
            self.text.set_horiz_justify(h);
        }

        self.set_text_angle(env, new_angle);
    }

    /// `PCB_TEXT::Move()`: `Offset( aMoveVector )`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.offset(env, delta)
    }

    /// `PCB_TEXT::OnFootprintRescaled`: `OnFootprintTransformed()`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
        self.on_footprint_transformed(env)
    }

    /// `PCB_TEXT::OnFootprintTransformed`: re-bake the board angle, drop the caches.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        let angle = self.text_angle_env(env);
        self.set_text_angle(env, angle);
        self.text.clear_render_cache();
        self.text.clear_bounding_box_cache();
    }

    /// `PCB_TEXT::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut pt = eda_text::text_pos_env(self, env);
        rotate_point_around(&mut pt, centre, angle);
        self.set_text_pos(env, pt);

        let mut new_angle = self.text_angle_env(env) + angle;
        new_angle.normalize();
        self.set_text_angle(env, new_angle);
    }

    /// `PCB_TEXT::SetPosition()`: `SetTextPos( aPos )`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        self.set_text_pos(env, pos)
    }

    /// `PCB_TEXT::StyleFromSettings`.
    pub fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        let layer = self.base.layer;

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

    /// `PCB_TEXT::GetDrawRotation`: footprint text with keep-upright folds into `]-90, 90]`,
    /// everything else is normalised to `[0, 360)`.
    pub fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        let mut rotation = self.text_angle(ctx);

        if board_item::parent_footprint(self, ctx).is_some() && self.text.is_keep_upright() {
            // Keep angle between ]-90..90] deg. Otherwise the text is not easy to read
            while rotation > ANGLE_90 {
                rotation -= ANGLE_180;
            }

            while rotation <= -ANGLE_90 {
                rotation += ANGLE_180;
            }
        } else {
            rotation.normalize();
        }

        rotation
    }

    /// `PCB_TEXT::getFontMetrics`: `BOARD_ITEM::GetFontMetrics()` = `METRICS::Default()`.
    pub fn font_metrics(&self, _ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        kicad_font::Metrics::default_metrics()
    }

    /// `PCB_TEXT::Offset`: a board-frame delta becomes a library-frame delta through the
    /// parent footprint's transform.
    pub fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        match env.parent.as_ref() {
            Some(parent) => {
                let transform = parent.transform;
                let cur_board_pos = transform.apply(self.text.lib_pos);
                let new_lib_pos = transform.inverse_apply(cur_board_pos + delta);
                let lib_delta = new_lib_pos - self.text.lib_pos;
                self.text.clear_render_cache();
                eda_text::offset(self, env, lib_delta);
            }
            None => eda_text::offset(self, env, delta),
        }
    }

    /// `PCB_TEXT::SetTextAngle`: the lib angle is the board angle minus the parent
    /// footprint's orientation.
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.lib_text_angle = match env.parent.as_ref() {
            Some(parent) => EdaOrientation::new(angle - parent.transform.rotate()),
            None => EdaOrientation::new(angle),
        };

        eda_text::set_text_angle(self, env, angle);
    }

    /// `PCB_TEXT::SetTextSize`: divide by the parent footprint's scale first.
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        let mut new_size = size;

        if let Some(parent) = env.parent.as_ref() {
            let xf = parent.transform;
            new_size = Vector2I::new(
                ki_round(new_size.x as f64 / xf.scale_x().abs()),
                ki_round(new_size.y as f64 / xf.scale_y().abs()),
            );
        }

        eda_text::set_text_size(self, env, new_size, enforce_min);
    }

    /// `PCB_TEXT::SetTextThickness`: divide by the mean of the parent footprint's scales.
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        let mut width = width;

        if let Some(parent) = env.parent.as_ref() {
            let xf = parent.transform;
            let factor = (xf.scale_x().abs() + xf.scale_y().abs()) * 0.5;
            width = ki_round(width as f64 / factor);
        }

        eda_text::set_text_thickness(self, env, width);
    }

    /// `PCB_TEXT::GetShownText`: the cached unescaped text with `${LAYER}`, the parent
    /// footprint's and the board's variables resolved (unless `RAW_VALUE`).
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

            // board can be null in some cases when saving a footprint in FP editor
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

        text
    }

    /// `PCB_TEXT::GetTextAngle`: the lib angle plus the parent footprint's orientation,
    /// normalised; the lib angle without one.
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => {
                let mut angle = self.lib_text_angle.angle() + fp.orientation();
                angle.normalize()
            }
            None => self.lib_text_angle.angle(),
        }
    }

    /// `PCB_TEXT::GetTextPos`: the stored position through the parent footprint's transform.
    pub fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => fp.transform().apply(self.text.lib_pos),
            None => self.text.lib_pos,
        }
    }

    /// `PCB_TEXT::GetTextSize`: the stored size scaled by the parent footprint's scales.
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

    /// `PCB_TEXT::GetTextThickness`: the stored width scaled by the mean of the parent
    /// footprint's scales.
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

    use crate::eda_text::EdaTextExt;
    use kicad_core::GrTextHAlignT;
    use kicad_geom::{ShapeRef, ShapeSegment};

    /// A board-level text as `parsePCB_TEXT` builds it from
    /// `(gr_text "<text>" (at x y) (layer ..) (effects (font (size s s) (thickness t))
    /// (justify left [mirror])))`.
    fn board_text(text: &str, pos: Vector2I, size: i32, thickness: i32, mirrored: bool) -> PcbText {
        let env = MutEnv::detached();
        let mut t = PcbText::default();
        t.text.set_text(text);
        t.set_text_pos(&env, pos);
        t.set_text_size(&env, Vector2I::new(size, size), true);
        t.set_text_thickness(&env, thickness);
        t.text.set_horiz_justify(GrTextHAlignT::Left);
        t.text.set_mirrored(mirrored);
        t
    }

    /// Segment count, the first two and the last segment, and the sum of all coordinates.
    fn segments_digest(shape: &kicad_geom::ShapeCompound) -> (usize, Vec<[i32; 4]>, i64, i32) {
        let segs: Vec<&ShapeSegment> = shape
            .shapes()
            .iter()
            .map(|s| match s.as_shape_ref() {
                ShapeRef::Segment(seg) => seg,
                _ => panic!("stroke text yields segments only"),
            })
            .collect();
        let coords = |s: &ShapeSegment| {
            let seg = s.get_seg();
            [seg.a.x, seg.a.y, seg.b.x, seg.b.y]
        };
        let sum = segs
            .iter()
            .flat_map(|s| coords(s))
            .map(i64::from)
            .sum::<i64>();
        let picks = vec![
            coords(segs[0]),
            coords(segs[1]),
            coords(segs[segs.len() - 1]),
        ];
        (segs.len(), picks, sum, segs[0].get_width())
    }

    /// Outline count, contour count (outlines + holes), vertex count, sum of all coordinates
    /// and the first three vertices of the first outline.
    fn polys_digest(poly: &ShapePolySet) -> (i32, i32, i32, i64, Vec<Vector2I>) {
        let mut contours = 0;
        let mut sum = 0i64;
        let mut verts = 0;

        for o in 0..poly.outline_count() {
            let mut chains = vec![poly.c_outline(o)];
            chains.extend((0..poly.hole_count(o)).map(|h| poly.c_hole(o, h)));

            for chain in chains {
                contours += 1;
                verts += chain.point_count();
                sum += chain
                    .c_points()
                    .iter()
                    .map(|p| i64::from(p.x) + i64::from(p.y))
                    .sum::<i64>();
            }
        }

        let first = poly.c_outline(0).c_points()[..3].to_vec();
        (poly.outline_count(), contours, verts, sum, first)
    }

    /// Oracle: `pcb-dump board --sections text,bbox` of
    /// `qa/data/pcbnew/incorrect_text_mirroring_drc.kicad_pcb`, item
    /// `8d9ea4cf-1047-42af-bf72-13258f22d6ad` (three-line text, left justified, 3 mm,
    /// thickness 0.3 mm): `text_box`, `bbox`, `interline`, `effective_pen_width`,
    /// `text_shape` (`GetEffectiveTextShape( false )`), `effective_shape`,
    /// `text_polys.c0_e5000_in` (`TransformTextToPolySet( 0, 5000, ERROR_INSIDE )`).
    #[test]
    fn oracle_multiline_board_text() {
        let board = crate::board::Board::default();
        let ctx = BoardCtx::bare(&board);
        let t = board_text(
            "testboard for DRC:\n- mirrored text on top-layers\n- non-mirrored text on bottom layers",
            Vector2I::new(15_000_000, -49_000_000),
            3_000_000,
            300_000,
            false,
        );

        let text_box = t.text_box(&ctx, None);
        assert_eq!(
            (
                text_box.x(),
                text_box.y(),
                text_box.width(),
                text_box.height()
            ),
            (15_000_000, -56_111_332, 89_157_147, 14_222_664)
        );
        assert_eq!(t.bounding_box(&ctx), text_box);
        assert_eq!(t.interline(&ctx), 4_829_832);
        assert_eq!(t.effective_text_pen_width(&ctx, 0), 300_000);
        assert_eq!(t.draw_rotation(&ctx), EdaAngle::from_degrees(0.0));

        let shape = t.effective_text_shape(&ctx, false, None, kicad_geom::ANGLE_0);
        let (n, picks, sum, width) = segments_digest(&shape);
        assert_eq!(n, 654);
        assert_eq!(width, 300_000);
        assert_eq!(
            picks,
            vec![
                [15_483_082, -54_457_574, 16_625_939, -54_457_574],
                [15_911_653, -55_457_574, 15_911_653, -52_886_146],
                [103_054_515, -44_797_910, 103_340_229, -44_655_053],
            ]
        );
        assert_eq!(sum, 6_979_105_697);

        // GetEffectiveShape (not knockout): the triangulating variant, the same segments.
        let effective = t.effective_shape(
            &ctx,
            PcbLayerId::FSilkS,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        match effective.as_shape_ref() {
            ShapeRef::Compound(c) => assert_eq!(segments_digest(c).2, 6_979_105_697),
            _ => panic!("GetEffectiveTextShape is a compound"),
        }

        let mut poly = ShapePolySet::new();
        t.transform_text_to_poly_set(&ctx, &mut poly, 0, 5_000, ErrorLoc::Inside);
        let (outlines, contours, verts, vsum, first) = polys_digest(&poly);
        assert_eq!((outlines, contours, verts), (75, 103, 5_667));
        assert_eq!(vsum, 30_571_239_285);
        assert_eq!(
            first,
            vec![
                Vector2I::new(96_845_840, -44_926_615),
                Vector2I::new(96_889_229, -44_887_335),
                Vector2I::new(96_914_283, -44_834_443),
            ]
        );
    }

    /// Oracle: as above, item `ec347ae7-892c-45b9-a6fe-fe03f0d9c93a` ("simple text 1", 1.5 mm,
    /// thickness 0.25 mm, `justify left mirror`).
    #[test]
    fn oracle_mirrored_board_text() {
        let board = crate::board::Board::default();
        let ctx = BoardCtx::bare(&board);
        let t = board_text(
            "simple text 1",
            Vector2I::new(89_000_000, -16_000_000),
            1_500_000,
            250_000,
            true,
        );

        let text_box = t.text_box(&ctx, None);
        assert_eq!(
            (
                text_box.x(),
                text_box.y(),
                text_box.width(),
                text_box.height()
            ),
            (73_264_286, -17_316_250, 15_735_714, 2_632_500)
        );
        assert_eq!(t.interline(&ctx), 2_414_916);

        let shape = t.effective_text_shape(&ctx, false, None, kicad_geom::ANGLE_0);
        let (n, picks, sum, _) = segments_digest(&shape);
        assert_eq!(n, 90);
        assert_eq!(
            picks,
            vec![
                [88_549_812, -15_390_500, 88_406_955, -15_319_071],
                [88_406_955, -15_319_071, 88_121_241, -15_319_071],
                [74_549_812, -16_461_928, 74_692_669, -16_390_500],
            ]
        );
        assert_eq!(sum, 12_060_033_396);

        let mut poly = ShapePolySet::new();
        t.transform_text_to_poly_set(&ctx, &mut poly, 0, 5_000, ErrorLoc::Inside);
        let (outlines, contours, verts, vsum, first) = polys_digest(&poly);
        assert_eq!((outlines, contours, verts), (12, 15, 864));
        assert_eq!(vsum, 57_176_718_504);
        assert_eq!(
            first,
            vec![
                Vector2I::new(84_431_341, -16_441_669),
                Vector2I::new(84_435_489, -16_439_950),
                Vector2I::new(84_439_971, -16_439_632),
            ]
        );

        // Hit tests against the oracle text box.
        assert!(t.hit_test(&ctx, Vector2I::new(80_000_000, -16_000_000), 0));
        assert!(!t.hit_test(&ctx, Vector2I::new(90_000_000, -16_000_000), 0));
    }

    /// A footprint text in library-frame values, as the parser stores `(at x y angle)`
    /// (`SetLibTextPos`, board angle through `SetTextAngle`) and `(size h w)` /
    /// `(thickness t)` (`SetLibTextSize`, `SetLibTextThickness`) for files from
    /// `FIRST_FP_AFFINE_TRANSFORM` on.
    #[allow(clippy::too_many_arguments)]
    fn fp_text(
        text: &str,
        lib_pos: Vector2I,
        board_angle: f64,
        fp_orientation: f64,
        size: (i32, i32),
        thickness: i32,
        halign: GrTextHAlignT,
        mirrored: bool,
    ) -> PcbText {
        let mut t = PcbText::for_footprint(Some(PcbLayerId::FCu));
        t.text.set_text(text);
        t.set_lib_text_pos(lib_pos);
        t.set_lib_text_angle(EdaAngle::from_degrees(board_angle - fp_orientation));
        t.text.attributes.set_angle(board_angle);
        t.set_lib_text_size(Vector2I::new(size.0, size.1));
        t.set_lib_text_thickness(thickness);
        t.text.set_horiz_justify(halign);
        t.text.set_mirrored(mirrored);
        t
    }

    /// Oracle: `pcb-dump board --sections text,bbox` of a board made of the header of
    /// `incorrect_text_mirroring_drc.kicad_pcb` (version raised to 20261031) and one footprint
    /// `(transform (translate 10 20) (rotate 30) (scale 1.5 0.75))` holding
    /// `(property "Reference" "R1" (at 1 2 15) .. (size 1 1.2) (thickness 0.15))` (uuid ..02),
    /// `(property "Value" "10k" (at -1 3 200) .. (size 1.2 1.2) (thickness 0.15) (justify left))`
    /// (..03) and `(fp_text user "abc" (at 0 -2 100) .. (size 1.2 1.2) (thickness 0.2)
    /// (justify right mirror))` (..04): `text_pos`, `text_size`, `text_thickness`,
    /// `draw_rotation`, `text_box`, `bbox`, `interline`, `text_shape`, `text_polys`.
    #[test]
    fn oracle_footprint_texts_scaled_rotated() {
        use crate::pcb_field::{FieldT, PcbField};
        use crate::test_support::{board_with_footprint, rotated_scaled_transform};

        type Expect = (
            Vector2I,
            Vector2I,
            i32,
            f64,
            (i32, i32, i64, i64),
            (i32, i32, i64, i64),
            i32,
            (usize, [i32; 4], i64),
            (i32, i32, i32, i64, Vector2I),
        );

        let mut fx = board_with_footprint(rotated_scaled_transform(), false);
        let mut kiids = KiidGenerator::default();

        let reference = fp_text(
            "R1",
            Vector2I::new(1_000_000, 2_000_000),
            15.0,
            30.0,
            (1_200_000, 1_000_000),
            150_000,
            GrTextHAlignT::Center,
            false,
        );
        let value = fp_text(
            "10k",
            Vector2I::new(-1_000_000, 3_000_000),
            200.0,
            30.0,
            (1_200_000, 1_200_000),
            150_000,
            GrTextHAlignT::Left,
            false,
        );
        let user = fp_text(
            "abc",
            Vector2I::new(0, -2_000_000),
            100.0,
            30.0,
            (1_200_000, 1_200_000),
            200_000,
            GrTextHAlignT::Right,
            true,
        );

        let mut ref_field = PcbField::new(&mut kiids, Some(PcbLayerId::FCu), FieldT::Reference, "");
        ref_field.text = reference;
        let ref_id = fx.child(ItemKind::Field).typed::<PcbField>().unwrap();
        fx.set_child(ref_id, ref_field);

        let mut value_field = PcbField::new(&mut kiids, Some(PcbLayerId::FCu), FieldT::Value, "");
        value_field.text = value;
        let value_id = fx.add_child(value_field, |fp| &mut fp.fields);

        let user_id = fx.child(ItemKind::Text).typed::<PcbText>().unwrap();
        fx.set_child(user_id, user);
        fx.board.check_invariants().unwrap();

        let ctx = BoardCtx::bare(&fx.board);

        let check = |host: &dyn Fn() -> (
            Vector2I,
            Vector2I,
            i32,
            f64,
            Box2I,
            Box2I,
            i32,
            Arc<kicad_geom::ShapeCompound>,
            ShapePolySet,
        ),
                     e: Expect| {
            let (pos, size, thick, rot, tbox, bbox, inter, shape, poly) = host();
            assert_eq!(pos, e.0);
            assert_eq!(size, e.1);
            assert_eq!(thick, e.2);
            assert_eq!(rot, e.3);
            assert_eq!((tbox.x(), tbox.y(), tbox.width(), tbox.height()), e.4);
            assert_eq!((bbox.x(), bbox.y(), bbox.width(), bbox.height()), e.5);
            assert_eq!(inter, e.6);
            let (n, picks, sum, _) = segments_digest(&shape);
            assert_eq!((n, picks[0], sum), e.7);
            let (outlines, contours, verts, vsum, first) = polys_digest(&poly);
            assert_eq!((outlines, contours, verts, vsum, first[0]), e.8);
        };

        let field_values = |id: crate::ids::Id<PcbField>| {
            let f = &ctx.board[id];
            let mut poly = ShapePolySet::new();
            f.transform_text_to_poly_set(&ctx, &mut poly, 0, 5_000, ErrorLoc::Inside);
            (
                f.text_pos(&ctx),
                f.text_size(&ctx),
                f.text_thickness(&ctx),
                f.draw_rotation(&ctx).as_degrees(),
                f.text_box(&ctx, None),
                f.bounding_box(&ctx),
                f.interline(&ctx),
                f.effective_text_shape(&ctx, false, None, kicad_geom::ANGLE_0),
                poly,
            )
        };

        check(
            &|| field_values(ref_id),
            (
                Vector2I::new(12_049_038, 20_549_038),
                Vector2I::new(1_800_000, 750_000),
                168_750,
                15.0,
                (10_218_770, 19_814_132, 3_660_536, 1_469_813),
                (10_090_927, 19_365_465, 3_916_222, 2_367_147),
                1_207_458,
                (
                    16,
                    [11_847_042, 20_954_291, 11_175_051, 20_764_609],
                    1_032_979_120,
                ),
                (
                    2,
                    3,
                    146,
                    4_740_069_929,
                    Vector2I::new(11_499_779, 20_224_804),
                ),
            ),
        );
        check(
            &|| field_values(value_id),
            (
                Vector2I::new(9_825_962, 22_698_557),
                Vector2I::new(1_800_000, 900_000),
                168_750,
                20.0,
                (9_825_962, 21_875_901, 5_031_965, 1_645_313),
                (9_544_597, 20_204_480, 5_291_231, 3_267_122),
                1_448_950,
                (
                    28,
                    [11_347_150, 22_579_667, 10_380_609, 22_931_459],
                    1_901_801_392,
                ),
                (
                    3,
                    4,
                    268,
                    9_087_979_556,
                    Vector2I::new(10_565_541, 21_827_420),
                ),
            ),
        );
        check(
            &|| {
                let t = &ctx.board[user_id];
                let mut poly = ShapePolySet::new();
                t.transform_text_to_poly_set(&ctx, &mut poly, 0, 5_000, ErrorLoc::Inside);
                (
                    t.text_pos(&ctx),
                    t.text_size(&ctx),
                    t.text_thickness(&ctx),
                    t.draw_rotation(&ctx).as_degrees(),
                    t.text_box(&ctx, None),
                    t.bounding_box(&ctx),
                    t.interline(&ctx),
                    t.effective_text_shape(&ctx, false, None, kicad_geom::ANGLE_0),
                    poly,
                )
            },
            (
                Vector2I::new(9_250_000, 18_700_962),
                Vector2I::new(1_800_000, 900_000),
                225_000,
                -80.0,
                (9_250_000, 17_779_587, 5_114_999, 1_842_750),
                (8_342_623, 18_540_967, 2_702_964, 5_357_281),
                1_448_950,
                (
                    37,
                    [9_498_191, 22_446_341, 9_962_458, 22_364_478],
                    2_289_329_332,
                ),
                (
                    3,
                    5,
                    302,
                    9_305_365_696,
                    Vector2I::new(10_007_767, 22_261_506),
                ),
            ),
        );
    }

    /// Board-frame mutators of a footprint text on the rotated / scaled frame fixture
    /// (`test_footprint_transform_sync.cpp` style): `SetTextPos` / `Move` / `Rotate` keep the
    /// board-frame values the C++ getters report, `SetTextAngle` stores the lib angle,
    /// `SetTextSize` divides by the footprint scale, `Flip` twice restores the text.
    #[test]
    fn footprint_text_mutators_round_trip() {
        use crate::test_support::{board_with_footprint, rotated_scaled_transform};

        let mut fx = board_with_footprint(rotated_scaled_transform(), false);
        let id = fx.child(ItemKind::Text).typed::<PcbText>().unwrap();
        let env = fx.board.mut_env(id.into());
        let mut t = fx.board[id].clone();

        t.set_text_pos(&env, Vector2I::new(12_000_000, 21_000_000));
        t.set_text_angle(&env, EdaAngle::from_degrees(45.0));
        t.set_text_size(&env, Vector2I::new(1_500_000, 1_500_000), true);
        t.set_text_thickness(&env, 150_000);
        let lib_after_set = t.text.lib_pos();
        fx.set_child(id, t.clone());

        {
            let ctx = BoardCtx::bare(&fx.board);
            let stored = &ctx.board[id];
            // Board position back through the transform (inverse-apply then apply: within 1 nm).
            let pos = stored.text_pos(&ctx);
            assert!((pos.x - 12_000_000).abs() <= 1 && (pos.y - 21_000_000).abs() <= 1);
            assert_eq!(stored.lib_text_angle(), EdaAngle::from_degrees(15.0));
            assert_eq!(stored.text_angle(&ctx), EdaAngle::from_degrees(45.0));
            assert_eq!(stored.text.attributes.size, (1_000_000, 2_000_000));
            assert_eq!(stored.text_size(&ctx), Vector2I::new(1_500_000, 1_500_000));
            assert_eq!(stored.text.attributes.stroke_width, 133_333);
            assert_eq!(stored.text_thickness(&ctx), 150_000);
        }

        // Move by a board delta and back: the library position is restored.
        t.move_by(&env, Vector2I::new(3_000_000, -1_000_000));
        t.move_by(&env, Vector2I::new(-3_000_000, 1_000_000));
        let back = t.text.lib_pos();
        assert!((back.x - lib_after_set.x).abs() <= 1 && (back.y - lib_after_set.y).abs() <= 1);

        // Rotate a full turn in four steps: angle back to 45 degrees.
        for _ in 0..4 {
            t.rotate(
                &env,
                Vector2I::new(10_000_000, 20_000_000),
                EdaAngle::from_degrees(90.0),
            );
        }
        assert_eq!(t.text_angle_env(&env), EdaAngle::from_degrees(45.0));

        // Flip twice: lib position, lib angle, layer and mirroring restored.
        let before = (
            t.text.lib_pos(),
            t.lib_text_angle(),
            t.base.layer,
            t.text.is_mirrored(),
        );
        t.flip(
            &env,
            Vector2I::new(10_000_000, 20_000_000),
            FlipDirection::TopBottom,
        );
        assert_eq!(t.base.layer, PcbLayerId::BCu);
        assert!(t.text.is_mirrored());
        t.flip(
            &env,
            Vector2I::new(10_000_000, 20_000_000),
            FlipDirection::TopBottom,
        );
        assert_eq!(
            (
                t.text.lib_pos(),
                t.lib_text_angle(),
                t.base.layer,
                t.text.is_mirrored()
            ),
            before
        );

        fx.set_child(id, t);
        fx.board.check_invariants().unwrap();
    }
}
