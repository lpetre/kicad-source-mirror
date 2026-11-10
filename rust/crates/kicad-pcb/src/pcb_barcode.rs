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

//! Port of `pcbnew/pcb_barcode.h` and `pcbnew/pcb_barcode.cpp` (`PCB_BARCODE`): data,
//! constructor and delegation shims (package M0c); accessors, setters, `Move` / `Rotate` /
//! `Flip`, `SetLayer`, `GetBoundingBox`, the hit tests, `TransformShapeToPolygon` /
//! `GetEffectiveShape`, `Compare` and the complete `AssembleBarcode` around the symbol encoder
//! (package M25a). The zint call is isolated in [`encode_symbol`], which has no encoder yet and
//! fails like zint does on rejected content (PORT_STATUS.md, M25a).
//!
//! Original copyright: (C) 2020 Thomas Pointhuber <thomas.pointhuber@gmx.at>.

use std::borrow::Cow;
use std::sync::Arc;

use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_font::gr_text::get_pen_size_for_normal;
use kicad_geom::cmath;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::trigo::rotate_point_around;
use kicad_geom::{
    ANGLE_180, Box2I, DEGREES_T, EdaAngle, EdaOrientation, FlipDirection, Shape, ShapeLineChain,
    ShapePolySet, Vector2I, ki_round, mirror_axis, mirror_point,
};

use crate::board_design_settings::BoardDesignSettings;
use crate::board_item;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, ParentRef, SharedShape};
use crate::pcb_text::PcbText;
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `BARCODE_T`; the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum BarcodeT {
    Code39 = 0,
    Code128 = 1,
    DataMatrix = 2,
    QrCode = 3,
    MicroQrCode = 4,
}

/// `BARCODE_ECC_T`: error correction level for QR codes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum BarcodeEccT {
    /// Low.
    L = 1,
    /// Medium.
    M = 2,
    /// Quartile.
    Q = 3,
    /// High.
    H = 4,
}

/// `PCB_BARCODE_CACHE`.
#[derive(Clone, Debug, Default)]
pub struct PcbBarcodeCache {
    /// keyHash: hash of all cache-key inputs (`computeCacheKey`).
    pub key_hash: u64,
    pub poly: ShapePolySet,
    pub symbol_poly: ShapePolySet,
    pub text_poly: ShapePolySet,
    pub bbox: Box2I,
    pub last_error: String,
}

/// `PCB_BARCODE`.
#[derive(Clone, Debug)]
pub struct PcbBarcode {
    pub base: BoardItemBase,
    pub width: i32,  // m_width
    pub height: i32, // m_height
    /// m_libPos: FP-relative in a footprint, board absolute otherwise.
    pub(crate) lib_pos: Vector2I,
    /// m_margin: margin around the barcode (only valid for knockout).
    pub margin: Vector2I,
    /// m_text: inline text (content; its visibility is "show text"). The C++ constructs it
    /// with the barcode as parent; an inline value carries `ParentRef::Item(barcode)` once the
    /// barcode is in a board (`board-model.md` 2).
    pub text: PcbText,
    pub kind: BarcodeT, // m_kind
    /// m_libAngle: FP-relative in a footprint, board absolute otherwise.
    pub(crate) lib_angle: EdaOrientation,
    pub error_correction: BarcodeEccT, // m_errorCorrection
    pub(crate) cache: KeyedCache<PcbBarcodeCache>, // m_cache
}

impl Default for PcbBarcode {
    /// `PCB_BARCODE( aParent )` with a nil UUID: 40 mm square QR code, ECC L, on Dwgs_User.
    fn default() -> Self {
        PcbBarcode {
            base: BoardItemBase::with_layer(PcbLayerId::DwgsUser),
            width: PCB_IU_SCALE.mm_to_iu(40.0),
            height: PCB_IU_SCALE.mm_to_iu(40.0),
            lib_pos: Vector2I::default(),
            margin: Vector2I::default(),
            text: PcbText::default(),
            kind: BarcodeT::QrCode,
            lib_angle: EdaOrientation::default(),
            error_correction: BarcodeEccT::L,
            cache: KeyedCache::default(),
        }
    }
}

impl PcbBarcode {
    /// `new PCB_BARCODE( aParent )`: the constructor values with a fresh UUID (the inline
    /// text's UUID is drawn too, as its `EDA_ITEM` constructor does).
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut b = PcbBarcode::default();
        b.base.uuid = Kiid::new(kiid_generator);
        b.text.base.uuid = Kiid::new(kiid_generator);
        b
    }

    /// The stored `m_libPos` (lib frame inside a footprint); the writer reads it.
    pub fn lib_pos(&self) -> Vector2I {
        self.lib_pos
    }

    /// The stored `m_libAngle`.
    pub fn lib_angle(&self) -> EdaAngle {
        self.lib_angle.angle()
    }

    /// Parser: store `m_libPos` / `m_libAngle` directly.
    pub fn set_lib_placement(&mut self, pos: Vector2I, angle: EdaAngle) {
        self.lib_pos = pos;
        self.lib_angle = EdaOrientation::new(angle);
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbBarcode {
    fn invalidate_caches(&mut self) {
        self.cache.clear();
        self.text.invalidate_caches();
    }
}

impl BoardItem for PcbBarcode {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbBarcodeT
    }
    fn class_name(&self) -> &'static str {
        "BARCODE"
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbBarcode::bounding_box(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbBarcode::center(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbBarcode::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbBarcode::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbBarcode::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn is_knockout(&self) -> bool {
        PcbBarcode::is_knockout(self)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbBarcode::item_description(self, ctx, units, full)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbBarcode::position(self, ctx)
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
        PcbBarcode::transform_shape_to_polygon(
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

impl BoardItemMut for PcbBarcode {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbBarcode::flip(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbBarcode::move_by(self, env, delta)
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
        PcbBarcode::on_footprint_rescaled(
            self,
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate,
        )
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbBarcode::rotate(self, env, centre, angle)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        PcbBarcode::set_layer(self, env, layer)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbBarcode::set_position(self, env, pos)
    }
    fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        check_side: bool,
    ) {
        PcbBarcode::style_from_settings(self, env, settings, check_side)
    }
}

// ---- the PCB_BARCODE overrides (packages M25 / M10) --------------------------------------

impl PcbBarcode {
    /// `PCB_BARCODE::GetBoundingBox`: `AssembleBarcode()`, then the cached box.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        self.assemble_barcode(ctx).bbox
    }

    /// `PCB_BARCODE::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `PCB_BARCODE::GetEffectiveShape`: the assembled polygon on the barcode's layer (an empty
    /// set on any other layer).
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut poly = ShapePolySet::new();
        self.transform_shape_to_polygon(ctx, &mut poly, layer, 0, 0, ErrorLoc::Inside, true);

        Arc::new(poly)
    }

    /// `PCB_BARCODE::HitTest( const VECTOR2I& aPosition, int aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        self.assemble_barcode(ctx);

        if !self.bounding_box(ctx).contains(pos) {
            return false;
        }

        let mut hulls = ShapePolySet::new();

        self.bounding_hull(
            ctx,
            &mut hulls,
            PcbLayerId::UndefinedLayer,
            accuracy,
            ARC_LOW_DEF,
            ErrorLoc::Outside,
        );

        hulls.collide_point(pos, 0, None, None)
    }

    /// `PCB_BARCODE::HitTest( const BOX2I& aRect, bool aContained, int aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.inflate(accuracy);

        let mut bbox = self.bounding_box(ctx);

        if accuracy != 0 {
            bbox.inflate(accuracy);
        }

        if contained {
            return arect.contains_box(&bbox);
        }

        arect.intersects(&bbox)
    }

    /// `PCB_BARCODE::IsKnockout()`: `BOARD_ITEM::IsKnockout()`.
    pub fn is_knockout(&self) -> bool {
        self.base.is_knockout
    }

    /// `PCB_BARCODE::GetItemDescription`: `Barcode '<text>' on <layer>` (the raw text).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        format!(
            "Barcode '{}' on {}",
            self.text(),
            board_item::layer_name(self, ctx)
        )
    }

    /// `PCB_BARCODE::GetPosition`: `m_libPos` through the parent footprint's transform.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => fp.transform().apply(self.lib_pos),
            None => self.lib_pos,
        }
    }

    /// `PCB_BARCODE::TransformShapeToPolygon`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        _ignore_line_width: bool,
    ) {
        if layer != self.base.layer && layer != PcbLayerId::UndefinedLayer {
            return;
        }

        let cache = self.assemble_barcode(ctx);

        if clearance == 0 {
            buffer.append_set(&cache.poly);
        } else {
            let mut poly = cache.poly.clone();
            // C++ passes `aErrorLoc` as the `bool aSimplify` argument of `Inflate()`
            // (`ERROR_OUTSIDE` = 0, `ERROR_INSIDE` = 1).
            poly.inflate(
                clearance,
                CornerStrategy::ChamferAcuteCorners,
                error,
                error_loc == ErrorLoc::Inside,
            );
            buffer.append_set(&poly);
        }
    }

    /// `PCB_BARCODE::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        // The position mirrors in the library or board frame depending on if this in a FP.
        let lib_centre = match env.parent.as_ref() {
            Some(parent) => parent.transform.inverse_apply(centre),
            None => centre,
        };
        let axis = mirror_axis(dir);

        mirror_point(&mut self.lib_pos, lib_centre, dir);

        // A flip changes the side, so that's a half turn on top of the reflection.
        self.lib_angle = self.lib_angle.reflect(axis) + ANGLE_180;

        let layer = env.layers.flip_layer(self.base.layer);
        self.set_layer(env, layer);
        self.cache.clear(); // AssembleBarcode()
    }

    /// `PCB_BARCODE::Move`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        match env.parent.as_ref() {
            Some(parent) => {
                let xform = parent.transform;
                let lib_offset =
                    xform.inverse_apply(delta) - xform.inverse_apply(Vector2I::new(0, 0));
                self.lib_pos += lib_offset;
            }
            None => self.lib_pos += delta,
        }

        // m_text is intentionally not moved, ComputeTextPoly repositions it
        // under the symbol on the next AssembleBarcode.
    }

    /// `PCB_BARCODE::OnFootprintRescaled`: nothing (board values derive on read,
    /// `AssembleBarcode` rebuilds on next access).
    pub fn on_footprint_rescaled(
        &mut self,
        _env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
    }

    /// `PCB_BARCODE::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut board_pos = self.position_env(env);
        rotate_point_around(&mut board_pos, centre, angle);

        self.lib_pos = match env.parent.as_ref() {
            Some(parent) => parent.transform.inverse_apply(board_pos),
            None => board_pos,
        };

        self.lib_angle += angle;

        self.cache.clear(); // AssembleBarcode()
    }

    /// `PCB_BARCODE::SetLayer`: the barcode's and its text's layer.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        self.base.layer = layer;
        BoardItemMut::set_layer(&mut self.text, env, layer);
        self.cache.clear(); // AssembleBarcode()
    }

    /// `PCB_BARCODE::SetPosition`: `Move( aPos - GetPosition() )`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        let delta = pos - self.position_env(env);
        self.move_by(env, delta);
    }

    /// `PCB_BARCODE::StyleFromSettings`: `SetTextSize( settings.GetTextSize( GetLayer() ).y )`
    /// (the text setters see the parent footprint frame of `env`).
    pub fn style_from_settings(
        &mut self,
        env: &MutEnv,
        settings: &BoardDesignSettings,
        _check_side: bool,
    ) {
        let size = settings.get_text_size(self.base.layer).y;
        self.set_text_size(env, size);
    }
}

// ---- accessors and setters of pcb_barcode.h -----------------------------------------------

impl PcbBarcode {
    /// `GetPosition()` as a mutator sees it (the parent frame of `env`).
    pub(crate) fn position_env(&self, env: &MutEnv) -> Vector2I {
        match env.parent.as_ref() {
            Some(parent) => parent.transform.apply(self.lib_pos),
            None => self.lib_pos,
        }
    }

    /// `GetAngle()` as a mutator sees it (the parent frame of `env`).
    pub(crate) fn angle_env(&self, env: &MutEnv) -> EdaAngle {
        match env.parent.as_ref() {
            Some(parent) => (self.lib_angle + parent.transform.rotate()).angle(),
            None => self.lib_angle.angle(),
        }
    }

    /// The inline `SetOrientation( double aDegrees )`: rotate about the position by the
    /// difference between the normalised new angle and `GetAngle()`.
    pub fn set_orientation(&mut self, env: &MutEnv, degrees: f64) {
        let new_angle = EdaAngle::new(degrees, DEGREES_T).normalized();
        let old_angle = self.angle_env(env);

        if new_angle != old_angle {
            let position = self.position_env(env);
            self.rotate(env, position, new_angle - old_angle);
        }
    }

    /// `GetOrientation()`: `GetAngle().AsDegrees()`.
    pub fn orientation(&self, ctx: &BoardCtx<'_>) -> f64 {
        self.angle(ctx).as_degrees()
    }

    /// `SetText()`: `m_text.SetText( aNewText )`.
    pub fn set_text(&mut self, text: &str) {
        self.text.text.set_text(text);
    }

    /// `GetText()`: `m_text.GetText()`.
    pub fn text(&self) -> &str {
        self.text.text.text()
    }

    /// `SetBarcodeText()`: `SetText()` + `AssembleBarcode()`.
    pub fn set_barcode_text(&mut self, text: &str) {
        self.set_text(text);
        self.cache.clear();
    }

    /// `GetShownText( aContext )`: `m_text.GetShownText( aContext )` (the text resolves its
    /// variables through the barcode's parent chain).
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext) -> String {
        self.text_host().shown_text(ctx, context, 0)
    }

    /// `SetTextSize( int aTextSize )`: square text of at least 1 IU (board frame) with the
    /// normal pen width for that height.
    pub fn set_text_size(&mut self, env: &MutEnv, text_size: i32) {
        let size = std::cmp::max(1, text_size);
        self.text
            .set_text_size(env, Vector2I::new(size, size), true);

        let height = self.text_height_env(env);
        self.text
            .set_text_thickness(env, std::cmp::max(1, get_pen_size_for_normal(height)));
        self.cache.clear(); // AssembleBarcode()
    }

    /// `GetTextSize()`: `m_text.GetTextHeight()`, which is the virtual `GetTextSize().y`:
    /// `PCB_TEXT::GetTextSize` scales the stored height by the parent footprint's Y scale
    /// (the text's parent chain is the barcode's).
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> i32 {
        let lib_height = self.text.text.attributes.size.1;

        match board_item::parent_footprint(self, ctx) {
            Some(fp) => ki_round(lib_height as f64 * fp.transform().scale_y().abs()),
            None => lib_height,
        }
    }

    /// `m_text.GetTextHeight()` as a mutator sees it (the parent frame of `env`).
    fn text_height_env(&self, env: &MutEnv) -> i32 {
        let lib_height = self.text.text.attributes.size.1;

        match env.parent.as_ref() {
            Some(parent) => ki_round(lib_height as f64 * parent.transform.scale_y().abs()),
            None => lib_height,
        }
    }

    /// `SetShowText()`: `m_text.SetVisible( aShow )` + `AssembleBarcode()`.
    pub fn set_show_text(&mut self, show: bool) {
        self.text.text.set_visible(show);
        self.cache.clear();
    }

    /// `GetShowText()`: `m_text.IsVisible()`.
    pub fn show_text(&self) -> bool {
        self.text.text.is_visible()
    }

    /// `SetIsKnockout()`: `BOARD_ITEM::SetIsKnockout( aEnable )` + `AssembleBarcode()`.
    pub fn set_is_knockout(&mut self, enable: bool) {
        self.base.is_knockout = enable;
        self.cache.clear();
    }

    /// `SetKind()`: when switching to Micro QR, validate and adjust ECC if needed. Don't
    /// auto-compute here as it may be called during loading.
    pub fn set_kind(&mut self, kind: BarcodeT) {
        self.kind = kind;

        if self.kind == BarcodeT::MicroQrCode && self.error_correction == BarcodeEccT::H {
            self.error_correction = BarcodeEccT::Q;
        }
    }

    /// `SetBarcodeKind()`: `SetKind()` + `AssembleBarcode()`.
    pub fn set_barcode_kind(&mut self, kind: BarcodeT) {
        self.set_kind(kind);
        self.cache.clear();
    }

    /// `SetErrorCorrection()`: Micro QR codes do not support High (H) error correction level.
    /// Don't auto-compute here as it may be called during loading.
    pub fn set_error_correction(&mut self, error_correction: BarcodeEccT) {
        self.error_correction =
            if self.kind == BarcodeT::MicroQrCode && error_correction == BarcodeEccT::H {
                BarcodeEccT::Q
            } else {
                error_correction
            };
    }

    /// `SetBarcodeErrorCorrection()`: `SetErrorCorrection()` + `AssembleBarcode()`.
    pub fn set_barcode_error_correction(&mut self, error_correction: BarcodeEccT) {
        self.set_error_correction(error_correction);
        self.cache.clear();
    }

    /// `KeepSquare()`: QR, Micro QR and Data Matrix symbols are square.
    pub fn keep_square(&self) -> bool {
        matches!(
            self.kind,
            BarcodeT::QrCode | BarcodeT::MicroQrCode | BarcodeT::DataMatrix
        )
    }

    /// `SetBarcodeWidth()`: also the height of a square kind; `AssembleBarcode()`.
    pub fn set_barcode_width(&mut self, width: i32) {
        self.width = width;

        if self.keep_square() {
            self.height = width;
        }

        self.cache.clear();
    }

    /// `SetBarcodeHeight()`: also the width of a square kind; `AssembleBarcode()`.
    pub fn set_barcode_height(&mut self, height: i32) {
        self.height = height;

        if self.keep_square() {
            self.width = height;
        }

        self.cache.clear();
    }

    /// `SetMarginX()`: at least 1 mm; `AssembleBarcode()`.
    pub fn set_margin_x(&mut self, x: i32) {
        self.margin.x = std::cmp::max(PCB_IU_SCALE.mm_to_iu(1.0), x);
        self.cache.clear();
    }

    /// `SetMarginY()`: at least 1 mm; `AssembleBarcode()`.
    pub fn set_margin_y(&mut self, y: i32) {
        self.margin.y = std::cmp::max(PCB_IU_SCALE.mm_to_iu(1.0), y);
        self.cache.clear();
    }

    /// `GetPolyShape()`: the assembled polygon (board frame).
    pub fn poly_shape(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        self.assemble_barcode(ctx).poly.clone()
    }

    /// `GetSymbolPoly()`: the scaled symbol, centred on the position, not rotated.
    pub fn symbol_poly(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        self.assemble_barcode(ctx).symbol_poly.clone()
    }

    /// `GetTextPoly()`: the human-readable text under the symbol, not rotated.
    pub fn text_poly(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        self.assemble_barcode(ctx).text_poly.clone()
    }

    /// `GetLastError()`: the encoder's message of the last assembly ("" on success).
    pub fn last_error(&self, ctx: &BoardCtx<'_>) -> String {
        self.assemble_barcode(ctx).last_error.clone()
    }

    /// `PCB_BARCODE::GetBoundingHull`: one rotated rectangle per part (symbol, text), each the
    /// part's bounding box inflated by `clearance`. An empty part contributes the inflated
    /// default box at the origin, rotated about the position, as in C++.
    pub fn bounding_hull(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        _max_error: i32,
        _error_loc: ErrorLoc,
    ) {
        let cache = self.assemble_barcode(ctx);

        let get_bounding_hull = |loc_buffer: &mut ShapePolySet, source: &ShapePolySet| {
            let rect = source.bbox(clearance);
            let mut corners = [Vector2I::default(); 4];

            corners[0].x = rect.origin().x;
            corners[0].y = rect.origin().y;
            corners[1].y = corners[0].y;
            corners[1].x = rect.right();
            corners[2].x = corners[1].x;
            corners[2].y = rect.bottom();
            corners[3].y = corners[2].y;
            corners[3].x = corners[0].x;

            loc_buffer.new_outline();

            let pos = self.position(ctx);
            let angle = self.angle(ctx);

            for corner in corners.iter_mut() {
                rotate_point_around(corner, pos, angle);
                loc_buffer.append(corner.x, corner.y, -1, -1, false);
            }
        };

        if layer == self.base.layer || layer == PcbLayerId::UndefinedLayer {
            get_bounding_hull(buffer, &cache.symbol_poly);
            get_bounding_hull(buffer, &cache.text_poly);
        }
    }

    /// `PCB_BARCODE::Similarity`: sixths for text, width, height, text height, position and
    /// kind.
    pub fn similarity(&self, ctx: &BoardCtx<'_>, other: &PcbBarcode) -> f64 {
        let mut similarity = 0.0;
        let weight = 1.0 / 6.0;

        if self.text() == other.text() {
            similarity += weight;
        }
        if self.width == other.width {
            similarity += weight;
        }
        if self.height == other.height {
            similarity += weight;
        }
        if self.text_size(ctx) == other.text_size(ctx) {
            similarity += weight;
        }
        if self.position(ctx) == other.position(ctx) {
            similarity += weight;
        }
        if self.kind == other.kind {
            similarity += weight;
        }

        similarity
    }

    /// `PCB_BARCODE::operator==( const PCB_BARCODE& )`: text, width, height, text height,
    /// position and kind.
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &PcbBarcode) -> bool {
        self.text() == other.text()
            && self.width == other.width
            && self.height == other.height
            && self.text_size(ctx) == other.text_size(ctx)
            && self.position(ctx) == other.position(ctx)
            && self.kind == other.kind
    }

    /// `PCB_BARCODE::Compare( aBarcode, aOther )`: negative, 0 or positive. Differences are
    /// C++ `int` subtractions (wrapping where the C++ would overflow).
    pub fn compare(ctx: &BoardCtx<'_>, barcode: &PcbBarcode, other: &PcbBarcode) -> i32 {
        let (pos_a, pos_b) = (barcode.position(ctx), other.position(ctx));

        let diff = pos_a.x.wrapping_sub(pos_b.x);
        if diff != 0 {
            return diff;
        }

        let diff = pos_a.y.wrapping_sub(pos_b.y);
        if diff != 0 {
            return diff;
        }

        let diff = wx_cmp(barcode.text(), other.text());
        if diff != 0 {
            return diff;
        }

        let diff = barcode.width.wrapping_sub(other.width);
        if diff != 0 {
            return diff;
        }

        let diff = barcode.height.wrapping_sub(other.height);
        if diff != 0 {
            return diff;
        }

        let diff = barcode.text_size(ctx).wrapping_sub(other.text_size(ctx));
        if diff != 0 {
            return diff;
        }

        let diff = barcode.kind as i32 - other.kind as i32;
        if diff != 0 {
            return diff;
        }

        let diff = barcode
            .angle(ctx)
            .as_tenths_of_a_degree()
            .wrapping_sub(other.angle(ctx).as_tenths_of_a_degree());
        if diff != 0 {
            return diff;
        }

        barcode.error_correction as i32 - other.error_correction as i32
    }
}

/// `wxString::Cmp`: negative, 0 or positive (only the sign is meaningful; UTF-8 byte order is
/// code point order).
fn wx_cmp(a: &str, b: &str) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

// ---- AssembleBarcode and its parts ---------------------------------------------------------

/// FNV-1a over the cache-key inputs (`computeCacheKey` uses `hash_val`; only equality of the
/// key matters).
struct KeyHasher(u64);

impl KeyHasher {
    fn new() -> Self {
        KeyHasher(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn int(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
}

impl PcbBarcode {
    /// The inner text as the host of its own `EDA_TEXT` virtuals. In C++ `m_text`'s parent is
    /// the barcode, so its `GetParentFootprint()` / `GetBoard()` are the barcode's. A text
    /// whose parent is not set (a parsed or detached barcode) is viewed with the barcode's
    /// parent, which answers both the same way.
    fn text_host(&self) -> Cow<'_, PcbText> {
        if self.text.base.parent != ParentRef::None || self.base.parent == ParentRef::None {
            Cow::Borrowed(&self.text)
        } else {
            let mut text = self.text.clone();
            text.base.parent = self.base.parent;
            Cow::Owned(text)
        }
    }

    /// Port of `PCB_BARCODE::computeCacheKey`: the shown text, size, position, margin, kind,
    /// angle, error correction, text visibility and height, knockout and layer.
    fn compute_cache_key(&self, ctx: &BoardCtx<'_>) -> u64 {
        let pos = self.position(ctx);
        let angle = self.angle(ctx);

        let mut h = KeyHasher::new();
        let shown = self.shown_text(ctx, ResolutionContext::ForCanvas);
        h.int(shown.len() as i32);
        h.bytes(shown.as_bytes());
        h.int(self.width);
        h.int(self.height);
        h.int(pos.x);
        h.int(pos.y);
        h.int(self.margin.x);
        h.int(self.margin.y);
        h.int(self.kind as i32);
        h.bytes(&angle.as_degrees().to_bits().to_le_bytes());
        h.int(self.error_correction as i32);
        h.int(i32::from(self.text.text.is_visible()));
        h.int(self.text_size(ctx));
        h.int(i32::from(self.is_knockout()));
        h.int(self.base.layer.0);
        h.0
    }

    /// `GetAngle()`: board frame (the lib angle plus the parent footprint's orientation).
    pub fn angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => (self.lib_angle + fp.orientation()).angle(),
            None => self.lib_angle.angle(),
        }
    }

    /// Port of `PCB_BARCODE::AssembleBarcode`: encode the symbol, scale it to the requested
    /// size around the position, place the text under it, apply the knockout, mirror on the
    /// back and rotate. The result is cached under the C++ cache key.
    pub fn assemble_barcode(&self, ctx: &BoardCtx<'_>) -> Arc<PcbBarcodeCache> {
        let key = self.compute_cache_key(ctx);

        self.cache
            .get_or_build(|c| c.key_hash == key, || self.build_cache(ctx, key))
    }

    /// The body of `AssembleBarcode` after the cache-key test.
    fn build_cache(&self, ctx: &BoardCtx<'_>, key: u64) -> PcbBarcodeCache {
        let mut cache = PcbBarcodeCache::default();

        self.compute_barcode(ctx, &mut cache);

        let pos = self.position(ctx);

        // Scale the symbol polygon to the desired barcode width/height and center it at pos.
        let half = Vector2I::new(self.width / 2, self.height / 2);
        rescale_symbol_poly(&mut cache, pos - half, pos + half);

        self.compute_text_poly(ctx, &mut cache);

        // Build full m_poly from symbol + optional text, then apply knockout if requested
        cache.poly.remove_all_contours();
        cache.poly.append_set(&cache.symbol_poly);

        if self.text.text.is_visible() && cache.text_poly.outline_count() != 0 {
            cache.poly.append_set(&cache.text_poly);
        }

        cache.poly.fracture(true);

        if self.is_knockout() {
            // Enforce minimum margin: at least 10% of the smallest side of the barcode, rounded
            // up to the nearest 0.1 mm. Use this as a lower bound for both axes.
            let min_side = std::cmp::min(self.width, self.height);
            let ten_percent = (min_side + 9) / 10; // ceil(minSide * 0.1)
            let step_01mm = std::cmp::max(1, PCB_IU_SCALE.mm_to_iu(0.1));
            let ten_percent_rounded = ((ten_percent + step_01mm - 1) / step_01mm) * step_01mm;

            // Build inversion rectangle based on the local bbox of the current combined geometry
            let mut bbox = cache.poly.bbox(0);
            bbox.inflate_xy(
                std::cmp::max(self.margin.x, ten_percent_rounded),
                std::cmp::max(self.margin.y, ten_percent_rounded),
            );

            let mut rect = ShapeLineChain::new();
            rect.append_xy(bbox.left(), bbox.top(), false);
            rect.append_xy(bbox.right(), bbox.top(), false);
            rect.append_xy(bbox.right(), bbox.bottom(), false);
            rect.append_xy(bbox.left(), bbox.bottom(), false);
            rect.set_closed(true);

            let mut ko = ShapePolySet::new();
            ko.add_outline(&rect);
            ko.boolean_subtract(&cache.poly);
            ko.fracture(true);
            cache.poly = ko;
        }

        if self.is_side_specific(ctx)
            && board_item::has_board(self, ctx)
            && ctx.board.is_back_layer(self.base.layer)
        {
            cache.poly.mirror(pos, FlipDirection::LeftRight);
        }

        let angle = self.angle(ctx);

        if !angle.is_zero() {
            cache.poly.rotate(angle, pos);
        }

        cache.poly.cache_triangulation(false);
        cache.bbox = cache.poly.bbox(0);
        cache.key_hash = key;
        cache
    }

    /// Port of `PCB_BARCODE::ComputeTextPoly`: the text rendered as polygons, centred
    /// horizontally under the symbol 1 mm below it. Nothing when the text is hidden or the
    /// symbol is empty.
    fn compute_text_poly(&self, ctx: &BoardCtx<'_>, cache: &mut PcbBarcodeCache) {
        cache.text_poly.remove_all_contours();

        if !self.text.text.is_visible() {
            return;
        }

        let text = self.text_host();
        let mut text_poly = ShapePolySet::new();
        text.transform_text_to_poly_set(
            ctx,
            &mut text_poly,
            0,
            board_item::max_error(self, ctx),
            ErrorLoc::Inside,
        );

        if text_poly.outline_count() == 0 {
            return;
        }

        // PCB_TEXT::GetDrawRotation now includes the parent FP orientation, so
        // TransformTextToPolySet rendered the glyphs already rotated by the FP
        // angle. The final AssembleBarcode rotation (m_cache->poly.Rotate by
        // GetAngle = lib_angle + FP_orient) would then rotate the glyphs a
        // second time, producing twice the intended visual rotation. Undo the
        // m_text-side rotation here so the glyph-orientation contribution is
        // only the lib_angle, and the final poly rotation applies the FP-orient
        // part uniformly with the symbol.
        if let Some(fp) = board_item::parent_footprint(self, ctx) {
            let fp_orient = fp.orientation();

            if !fp_orient.is_zero() {
                text_poly.rotate(-fp_orient, text.text_pos(ctx));
            }
        }

        if cache.symbol_poly.outline_count() == 0 {
            return;
        }

        let text_bbox = text_poly.bbox(0);
        let symbol_bbox = cache.symbol_poly.bbox(0);
        let text_offset = PCB_IU_SCALE.mm_to_iu(1.0);
        let text_pos = Vector2I::new(
            symbol_bbox.center().x - text_bbox.center().x,
            symbol_bbox.bottom() - text_bbox.top() + text_offset,
        );

        text_poly.move_(text_pos);

        cache.text_poly = text_poly;
        cache.text_poly.cache_triangulation(false);
    }

    /// Port of `PCB_BARCODE::ComputeBarcode`: the symbol polygon from the encoder's module
    /// rectangles and hexagons, centred on the origin. On an empty text nothing is encoded;
    /// on an encoder failure (here also: no encoder, see [`encode_symbol`]) the symbol stays
    /// empty and `last_error` says why.
    fn compute_barcode(&self, ctx: &BoardCtx<'_>, cache: &mut PcbBarcodeCache) {
        cache.symbol_poly.remove_all_contours();
        cache.last_error.clear();

        let text = self.shown_text(ctx, ResolutionContext::ForCanvas);

        if text.is_empty() {
            return;
        }

        let Some(symbol) = encode_symbol(self.kind, &text, self.error_correction) else {
            if !text.is_ascii() {
                cache.last_error = "This barcode type does not support international \
                                    characters. Use QR Code or Data Matrix instead."
                    .to_string();
            } else {
                cache.last_error = ENCODER_NOT_AVAILABLE.to_string();
            }

            return;
        };

        for rect in &symbol.rectangles {
            // Round using absolute edges to avoid cumulative rounding drift across modules.
            let x1: i32 = ki_round(rect.x * symbol.scale);
            let x2: i32 = ki_round((rect.x + rect.width) * symbol.scale);
            let y1: i32 = ki_round(rect.y * symbol.scale);
            let y2: i32 = ki_round((rect.y + rect.height) * symbol.scale);

            let mut shapeline = ShapeLineChain::new();
            shapeline.append_xy(x1, y1, false);
            shapeline.append_xy(x2, y1, false);
            shapeline.append_xy(x2, y2, false);
            shapeline.append_xy(x1, y2, false);
            shapeline.set_closed(true);

            cache.symbol_poly.add_outline(&shapeline);
        }

        for hex in &symbol.hexagons {
            // Compute vertices from center using minimal-diameter (inscribed circle) radius.
            let r = f64::from(hex.diameter) / 2.0; // minimal radius
            let cx = f64::from(hex.x);
            let cy = f64::from(hex.y);

            // Base orientation has apex at top; hex->rotation rotates by 0/90/180/270 degrees.
            let base_angles: [f64; 6] = [90.0, 30.0, -30.0, -90.0, -150.0, 150.0];
            let rot = f64::from(hex.rotation);

            let mut poly = ShapeLineChain::new();

            for base in base_angles {
                let ang = (base + rot) * std::f64::consts::PI / 180.0;
                let vx: i32 = ki_round(cx + r * cmath::cos(ang));
                let vy: i32 = ki_round(cy + r * cmath::sin(ang));
                poly.append_xy(vx, vy, false);
            }

            poly.set_closed(true);

            cache.symbol_poly.add_outline(&poly);
        }

        // Set the position of the barcode to the center of the symbol polygon
        if cache.symbol_poly.outline_count() > 0 {
            let pos = cache.symbol_poly.bbox(0).center();
            cache.symbol_poly.move_(-pos);
        }

        cache.symbol_poly.cache_triangulation(false);
    }
}

/// Port of `PCB_BARCODE::rescaleSymbolPoly`: rescale only the symbol polygon to the requested
/// rectangle.
fn rescale_symbol_poly(cache: &mut PcbBarcodeCache, top_left: Vector2I, bot_right: Vector2I) {
    let bbox = cache.symbol_poly.bbox(0);
    // `int oldW = bbox.GetWidth()`: the 64-bit size narrowed to `int`.
    let old_w = bbox.width() as i32;
    let old_h = bbox.height() as i32;

    let new_position = (top_left + bot_right) / 2;
    let mut new_w = bot_right.x - top_left.x;
    let mut new_h = bot_right.y - top_left.y;
    // Guard against zero/negative sizes from interactive edits; enforce a tiny minimum
    let min_iu = std::cmp::max(1, PCB_IU_SCALE.mm_to_iu(0.01));
    new_w = std::cmp::max(new_w, min_iu);
    new_h = std::cmp::max(new_h, min_iu);

    let scale_x = if old_w != 0 {
        f64::from(new_w) / f64::from(old_w)
    } else {
        1.0
    };
    let scale_y = if old_h != 0 {
        f64::from(new_h) / f64::from(old_h)
    } else {
        1.0
    };

    let old_center = bbox.center();
    cache.symbol_poly.scale(scale_x, scale_y, old_center);

    // After scaling, move the symbol polygon to be centered at the new position
    let new_center = cache.symbol_poly.bbox(0).center();
    let delta = new_position - new_center;

    if delta != Vector2I::new(0, 0) {
        cache.symbol_poly.move_(delta);
    }
}

// ---- the symbol encoder (zint) boundary ----------------------------------------------------

/// `last_error` of a barcode whose symbol could not be encoded because the port has no
/// encoder yet (in C++ this is zint's `errtxt`).
pub const ENCODER_NOT_AVAILABLE: &str = "Barcode symbol encoder not available in this port";

/// `ECI_UTF8` (pcb_barcode.cpp): the ECI selected for non-ASCII QR / Data Matrix content.
pub const ECI_UTF8: i32 = 26;

/// zint's `BARCODE_*` symbology numbers selected by `ComputeBarcode` (zint 2.15.0, bundled
/// in `thirdparty/zint`).
pub mod zint_symbology {
    /// `BARCODE_CODE39`.
    pub const CODE39: i32 = 8;
    /// `BARCODE_CODE128`.
    pub const CODE128: i32 = 20;
    /// `BARCODE_QRCODE`.
    pub const QRCODE: i32 = 58;
    /// `BARCODE_DATAMATRIX`.
    pub const DATAMATRIX: i32 = 71;
    /// `BARCODE_MICROQR`.
    pub const MICROQR: i32 = 97;
}

/// The `zint_symbol` settings `ComputeBarcode` makes before `ZBarcode_Encode` (everything else
/// stays at the `ZBarcode_Create` defaults).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZintOptions {
    /// `symbol->symbology`.
    pub symbology: i32,
    /// `symbol->option_1`: the ECC level for QR / Micro QR, zint's default -1 otherwise.
    pub option_1: i32,
    /// `symbol->eci`: [`ECI_UTF8`] for non-ASCII QR / Data Matrix content, else 0.
    pub eci: i32,
    /// `symbol->input_mode = UNICODE_MODE` (1).
    pub input_mode: i32,
    /// `symbol->show_hrt = 0`: no human-readable text.
    pub show_hrt: i32,
}

/// The `switch( m_kind )` and the ECI choice of `ComputeBarcode`.
pub fn zint_options(kind: BarcodeT, text: &str, error_correction: BarcodeEccT) -> ZintOptions {
    let mut options = ZintOptions {
        symbology: 0,
        option_1: -1,
        eci: 0,
        input_mode: 1,
        show_hrt: 0,
    };

    match kind {
        BarcodeT::Code39 => options.symbology = zint_symbology::CODE39,
        BarcodeT::Code128 => options.symbology = zint_symbology::CODE128,
        BarcodeT::QrCode => {
            options.symbology = zint_symbology::QRCODE;
            options.option_1 = error_correction as i32;
        }
        BarcodeT::MicroQrCode => {
            options.symbology = zint_symbology::MICROQR;
            options.option_1 = error_correction as i32;
        }
        BarcodeT::DataMatrix => options.symbology = zint_symbology::DATAMATRIX,
    }

    if (kind == BarcodeT::QrCode || kind == BarcodeT::DataMatrix) && !text.is_ascii() {
        options.eci = ECI_UTF8;
    }

    options
}

/// One `zint_vector_rect` of `ZBarcode_Buffer_Vector( symbol, 0 )` (zint `float`s).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ModuleRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One `zint_vector_hexagon` (MaxiCode; none of the kinds KiCad offers produces them, the
/// loop is ported for completeness).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ModuleHexagon {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub rotation: i32,
}

/// The encoded symbol as `ComputeBarcode` reads it: `symbol->scale` and the vector lists in
/// zint's list order.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ModuleMatrix {
    pub scale: f32,
    pub rectangles: Vec<ModuleRect>,
    pub hexagons: Vec<ModuleHexagon>,
}

/// The single call into the symbol encoder: `ZBarcode_Create`, the settings of
/// [`zint_options`], `ZBarcode_Encode( symbol, utf8, length )` and
/// `ZBarcode_Buffer_Vector( symbol, 0 )`. `None` = encoding failed (`>= ZINT_ERROR`).
///
/// The port has no encoder yet (PORT_STATUS.md, package M25a): this always returns `None`,
/// so every non-empty barcode assembles exactly like a C++ barcode whose content zint
/// rejects (empty symbol, no text polygon, knockout box around the origin).
pub fn encode_symbol(
    kind: BarcodeT,
    text: &str,
    error_correction: BarcodeEccT,
) -> Option<ModuleMatrix> {
    let _options = zint_options(kind, text, error_correction);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_BARCODE::PCB_BARCODE( aParent )` (pcbnew/pcb_barcode.cpp).
    #[test]
    fn constructor_values() {
        let b = PcbBarcode::default();
        assert_eq!((b.width, b.height), (40_000_000, 40_000_000));
        assert_eq!(
            (b.kind, b.error_correction),
            (BarcodeT::QrCode, BarcodeEccT::L)
        );
        assert_eq!(b.base.layer, PcbLayerId::DwgsUser);
        assert_eq!(
            (b.lib_pos(), b.margin),
            (Vector2I::default(), Vector2I::default())
        );
        assert_eq!(b.lib_angle(), EdaAngle::from_degrees(0.0));
        assert_eq!(b.text.base.layer, PcbLayerId::FCu);

        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Barcode).typed::<PcbBarcode>().unwrap();
        let mut kiids = KiidGenerator::default();
        let mut code = PcbBarcode::new(&mut kiids);
        code.set_lib_placement(Vector2I::new(5, 6), EdaAngle::from_degrees(90.0));
        fx.set_child(id, code);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].lib_pos(), Vector2I::new(5, 6));
        assert!(!fx.board[id].is_knockout());
    }

    // ---- package M25a -------------------------------------------------------------------

    use crate::io::{FormatOptions, ParseOptions, format_board, parse_board};
    use crate::item_ref::AnyItem;

    /// Barcodes in a rotated footprint, a footprint on the back, a non-uniformly scaled
    /// footprint and on the board (front, back, copper, degenerate size). The empty-text ones
    /// are encoded by nobody (C++ returns before `ZBarcode_Encode`), so their whole assembly
    /// is comparable with the reference; the others only in what does not depend on the symbol.
    const FRAMES_BOARD: &str = r#"(kicad_pcb
	(version 20260206)
	(generator "pcbnew")
	(generator_version "10.99")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers
		(0 "F.Cu" signal)
		(2 "B.Cu" signal)
		(9 "F.Adhes" user "F.Adhesive")
		(11 "B.Adhes" user "B.Adhesive")
		(13 "F.Paste" user)
		(15 "B.Paste" user)
		(5 "F.SilkS" user "F.Silkscreen")
		(7 "B.SilkS" user "B.Silkscreen")
		(1 "F.Mask" user)
		(3 "B.Mask" user)
		(17 "Dwgs.User" user "User.Drawings")
		(19 "Cmts.User" user "User.Comments")
		(25 "Edge.Cuts" user)
		(27 "Margin" user)
		(31 "F.CrtYd" user "F.Courtyard")
		(29 "B.CrtYd" user "B.Courtyard")
		(35 "F.Fab" user)
		(33 "B.Fab" user)
	)
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(footprint "lib:fp_rot"
		(layer "F.Cu")
		(uuid "10000000-0000-0000-0000-000000000001")
		(at 100 50 30)
		(property "Reference" "U1" (at 0 -3 30) (layer "F.SilkS") (uuid "10000000-0000-0000-0000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "V" (at 0 3 30) (layer "F.Fab") (uuid "10000000-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
		(barcode (at 2 3 15) (layer "F.SilkS") (size 6 6) (text "") (text_height 1) (type qr) (ecc_level M) (hide no) (knockout yes) (margins 0.5 0.7) (uuid "10000000-0000-0000-0000-000000000004"))
		(barcode (at -4 1 0) (layer "F.SilkS") (size 8 3) (text "FPCODE") (text_height 1) (type code128) (uuid "10000000-0000-0000-0000-000000000005"))
	)
	(footprint "lib:fp_back"
		(layer "B.Cu")
		(uuid "20000000-0000-0000-0000-000000000001")
		(at 60 80 -60)
		(property "Reference" "U2" (at 0 -3 -60) (layer "B.SilkS") (uuid "20000000-0000-0000-0000-000000000002") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
		(property "Value" "V" (at 0 3 -60) (layer "B.Fab") (uuid "20000000-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
		(barcode (at 1.5 -2 45) (layer "B.SilkS") (size 5 5) (text "") (text_height 0.8) (type datamatrix) (knockout yes) (uuid "20000000-0000-0000-0000-000000000004"))
		(barcode (at -1 2.5 -20) (layer "B.SilkS") (size 4 4) (text "BACK") (text_height 0.8) (type qr) (ecc_level H) (knockout yes) (uuid "20000000-0000-0000-0000-000000000005"))
	)
	(footprint "lib:fp_scaled"
		(layer "F.Cu")
		(uuid "30000000-0000-0000-0000-000000000001")
		(transform (translate 140.5 70.25) (rotate 12.5) (scale 1.5 0.75))
		(property "Reference" "U3" (at 0 -3 12.5) (layer "F.SilkS") (uuid "30000000-0000-0000-0000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "V" (at 0 3 12.5) (layer "F.Fab") (uuid "30000000-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
		(barcode (at 3.3 -1.7 33) (layer "F.SilkS") (size 7 7) (text "") (text_height 1.2) (type microqr) (ecc_level Q) (knockout yes) (margins 1.1 0.3) (uuid "30000000-0000-0000-0000-000000000004"))
		(barcode (at -2 1 0) (layer "F.Fab") (size 9 2) (text "SCALE") (text_height 1.2) (type code39) (hide yes) (uuid "30000000-0000-0000-0000-000000000005"))
	)
	(barcode (at 20 20 0) (layer "F.SilkS") (size 10 10) (text "") (text_height 1) (type qr) (ecc_level L) (knockout yes) (uuid "40000000-0000-0000-0000-000000000001"))
	(barcode (at 40 20 37.5) (layer "B.SilkS") (size 12 8) (text "") (text_height 1) (type code39) (knockout yes) (margins 2 3) (uuid "40000000-0000-0000-0000-000000000002"))
	(barcode (at 60 20 90) (layer "F.Cu") (size 10 10) (text "") (text_height 1) (type qr) (ecc_level L) (uuid "40000000-0000-0000-0000-000000000003"))
	(barcode (at 80 20 -45) (layer "B.Cu") (size 10 10) (text "HELLO") (text_height 1) (type qr) (ecc_level L) (knockout yes) (uuid "40000000-0000-0000-0000-000000000004"))
	(barcode (at 80.123456 21.654321 0) (layer "Dwgs.User") (size 0 0) (text "") (text_height 1) (type datamatrix) (knockout yes) (uuid "40000000-0000-0000-0000-000000000005"))
)
"#;

    /// `pcb-dump board --no-init --sections misc,bbox` of `FRAMES_BOARD` (reference build):
    /// uuid, `GetBoundingBox()`, `GetPolyShape()` of the empty-text barcodes.
    const FRAMES_ORACLE: [(&str, &str, &str); 7] = [
        (
            "10000000-0000-0000-0000-000000000004",
            "-1469038,-211579,1521458,1662879",
            "P(1 {C(closed w=0 n=4 [52420,1140717 -1106691,1451300 -1469038,99004 -309927,-211579])})",
        ),
        (
            "20000000-0000-0000-0000-000000000004",
            "3267767,-2353553,1414214,1414213",
            "P(1 {C(closed w=0 n=4 [3974874,-939340 4681981,-1646447 3974874,-2353553 3267767,-1646447])})",
        ),
        (
            "30000000-0000-0000-0000-000000000004",
            "154489,336876,2607569,2372345",
            "P(1 {C(closed w=0 n=4 [2762058,1511015 916983,2709221 154489,1535082 1999564,336876])})",
        ),
        (
            "40000000-0000-0000-0000-000000000001",
            "-1000000,-1000000,2000000,2000000",
            "P(1 {C(closed w=0 n=4 [1000000,1000000 -1000000,1000000 -1000000,-1000000 1000000,-1000000])})",
        ),
        (
            "40000000-0000-0000-0000-000000000002",
            "56145914,-23815107,6825982,7195166",
            "P(1 {C(closed w=0 n=4 [59798483,-16619941 62971896,-19054987 59319327,-23815107 56145914,-21380061])})",
        ),
        ("40000000-0000-0000-0000-000000000003", "0,0,0,0", "P(0)"),
        ("40000000-0000-0000-0000-000000000005", "0,0,0,0", "P(0)"),
    ];

    /// `VPolySet` of `pcb-dump` for polygons without arcs.
    fn v_polyset(s: &ShapePolySet) -> String {
        let chain = |c: &ShapeLineChain| {
            let pts: Vec<String> = c
                .c_points()
                .iter()
                .map(|p| format!("{},{}", p.x, p.y))
                .collect();
            format!(
                "C({} w=0 n={} [{}])",
                if c.is_closed() { "closed" } else { "open" },
                c.point_count(),
                pts.join(" ")
            )
        };
        let mut r = format!("P({}", s.outline_count());

        for i in 0..s.outline_count() {
            r.push_str(" {");
            r.push_str(&chain(s.c_outline(i)));

            for h in 0..s.hole_count(i) {
                r.push_str(" hole ");
                r.push_str(&chain(s.c_hole(i, h)));
            }

            r.push('}');
        }

        r.push(')');
        r
    }

    fn v_box(b: &Box2I) -> String {
        format!(
            "{},{},{},{}",
            b.origin().x,
            b.origin().y,
            b.width(),
            b.height()
        )
    }

    fn barcode_of<'a>(board: &'a Board, uuid: &str) -> &'a PcbBarcode {
        let ctx = BoardCtx::bare(board);
        let id = ctx
            .resolve(&Kiid::parse(uuid).unwrap())
            .and_then(|r| r.id())
            .unwrap_or_else(|| panic!("{uuid} not found"));

        match board.get(id) {
            Some(AnyItem::Barcode(b)) => b,
            _ => panic!("{uuid} is not a barcode"),
        }
    }

    use crate::board::Board;

    /// `AssembleBarcode` (knockout box, back-side mirror, rotation, footprint frames) and
    /// `GetBoundingBox` against the reference build, for the barcodes whose symbol is empty
    /// in C++ as well.
    #[test]
    fn assemble_matches_oracle_without_symbol() {
        let board = parse_board(FRAMES_BOARD, "barcode_frames", &ParseOptions::default()).unwrap();
        let ctx = BoardCtx::bare(&board);

        for (uuid, bbox, poly) in FRAMES_ORACLE {
            let b = barcode_of(&board, uuid);
            assert_eq!(v_box(&b.bounding_box(&ctx)), bbox, "{uuid} bbox");
            assert_eq!(v_polyset(&b.poly_shape(&ctx)), poly, "{uuid} poly");
            assert_eq!(b.symbol_poly(&ctx).outline_count(), 0);
            assert_eq!(b.text_poly(&ctx).outline_count(), 0);
            assert_eq!(b.last_error(&ctx), "");
        }

        // `GetTextSize()` (board frame) of every barcode, pcb-dump misc `barcode.text_size`.
        for (uuid, size) in [
            ("10000000-0000-0000-0000-000000000004", 1_000_000),
            ("10000000-0000-0000-0000-000000000005", 1_000_000),
            ("20000000-0000-0000-0000-000000000004", 800_000),
            ("20000000-0000-0000-0000-000000000005", 800_000),
            ("30000000-0000-0000-0000-000000000004", 1_200_000),
            ("30000000-0000-0000-0000-000000000005", 1_200_000),
            ("40000000-0000-0000-0000-000000000004", 1_000_000),
        ] {
            assert_eq!(barcode_of(&board, uuid).text_size(&ctx), size, "{uuid}");
        }

        // Non-empty text: the encoder is not available (C++: zint encodes it).
        let b = barcode_of(&board, "40000000-0000-0000-0000-000000000004");
        assert_eq!(b.last_error(&ctx), ENCODER_NOT_AVAILABLE);
        assert_eq!(b.symbol_poly(&ctx).outline_count(), 0);
    }

    /// Frame handling of the parser path (`SetPosition`, `SetOrientation` -> `Rotate`) against
    /// the reference: position, angle, library position and angle.
    #[test]
    fn frames_match_oracle() {
        let board = parse_board(FRAMES_BOARD, "barcode_frames", &ParseOptions::default()).unwrap();
        let ctx = BoardCtx::bare(&board);
        #[rustfmt::skip]
        // uuid, GetPosition, GetAngle, GetLibraryPos, GetLibraryAngle (pcb-dump item / misc)
        let expected = [
            ("10000000-0000-0000-0000-000000000004", (2000000, 3000000), 15.0, (-61370490, -89703194), 345.0),
            ("20000000-0000-0000-0000-000000000004", (1500000, -2000000), 45.0, (-100264083, 9662486), 105.0),
            ("30000000-0000-0000-0000-000000000004", (3300000, -1700000), 33.0, (-78916655, -133253350), 20.5),
            ("40000000-0000-0000-0000-000000000002", (40000000, 20000000), 37.5, (40000000, 20000000), 37.5),
        ];

        for (uuid, pos, angle, lib_pos, lib_angle) in expected {
            let b = barcode_of(&board, uuid);
            assert_eq!(b.position(&ctx), Vector2I::new(pos.0, pos.1), "{uuid}");
            assert_eq!(b.angle(&ctx).as_degrees(), angle, "{uuid}");
            assert_eq!(b.lib_pos(), Vector2I::new(lib_pos.0, lib_pos.1), "{uuid}");
            assert_eq!(b.lib_angle().as_degrees(), lib_angle, "{uuid}");
        }
    }

    /// `PCB_BARCODE::Move`, `Rotate`, `Flip`, `SetPosition` and `SetLayer` in a rotated,
    /// flipped, non-uniformly scaled footprint: the board-frame results are consistent with
    /// the mutation, the stored values stay in the library frame.
    #[test]
    fn mutators_in_footprint_frame() {
        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Barcode).typed::<PcbBarcode>().unwrap();

        fx.board.modify_barcode(id, |b, env| {
            b.set_position(env, Vector2I::new(1_000_000, 2_000_000));
        });
        let ctx = BoardCtx::bare(&fx.board);
        let pos = fx.board[id].position(&ctx);
        assert!((pos.x - 1_000_000).abs() <= 1 && (pos.y - 2_000_000).abs() <= 1);

        fx.board.modify_barcode(id, |b, env| {
            b.move_by(env, Vector2I::new(500_000, -250_000));
            let p = b.position_env(env);
            b.rotate(env, p, EdaAngle::from_degrees(90.0));
        });
        fx.board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&fx.board);
        let pos = fx.board[id].position(&ctx);
        assert!((pos.x - 1_500_000).abs() <= 2 && (pos.y - 1_750_000).abs() <= 2);
        let angle_before = fx.board[id].angle(&ctx);

        fx.board.modify_barcode(id, |b, env| {
            let p = b.position_env(env);
            b.flip(env, p, FlipDirection::LeftRight);
        });
        let ctx = BoardCtx::bare(&fx.board);
        let b = &fx.board[id];
        assert_eq!(b.base.layer, PcbLayerId::DwgsUser);
        assert_eq!(b.text.base.layer, PcbLayerId::DwgsUser);
        assert_ne!(b.angle(&ctx), angle_before);

        fx.board
            .modify_barcode(id, |b, env| b.set_layer(env, PcbLayerId::FSilkS));
        fx.board.modify_barcode(id, |b, env| {
            let p = b.position_env(env);
            b.flip(env, p, FlipDirection::LeftRight);
        });
        let b = &fx.board[id];
        assert_eq!(
            (b.base.layer, b.text.base.layer),
            (PcbLayerId::BSilkS, PcbLayerId::BSilkS)
        );
        fx.board.check_invariants().unwrap();
    }

    /// Port of `BarcodeWriteRead` (qa/tests/pcbnew/test_barcode_load_save.cpp).
    #[test]
    fn barcode_write_read() {
        let mut board = Board::new();
        let env = MutEnv::detached();
        let mut barcode = PcbBarcode::new(&mut board.kiid_generator);
        barcode.base.parent = ParentRef::Board;
        barcode.set_text("12345");
        barcode.set_layer(&env, PcbLayerId::FSilkS);
        barcode.set_position(&env, Vector2I::new(1_000_000, 2_000_000));
        barcode.width = 3_000_000;
        barcode.height = 3_000_000;
        barcode.set_text_size(&env, 1_500_000);
        barcode.set_kind(BarcodeT::QrCode);
        barcode.set_error_correction(BarcodeEccT::M);
        let uuid = barcode.base.uuid;
        let id = board.add_barcode(barcode, crate::board::AddMode::Append);

        let text = format_board(&board, &FormatOptions::default()).unwrap();
        let board2 = parse_board(&text, "barcode_roundtrip", &ParseOptions::default()).unwrap();
        let loaded = barcode_of(&board2, &uuid.as_string());
        let barcode = &board[id];
        let (ctx, ctx2) = (BoardCtx::bare(&board), BoardCtx::bare(&board2));

        assert_eq!(loaded.text(), barcode.text());
        assert_eq!(loaded.kind, barcode.kind);
        assert_eq!(loaded.error_correction, barcode.error_correction);
        assert_eq!(loaded.width, barcode.width);
        assert_eq!(loaded.height, barcode.height);
        assert_eq!(loaded.text_size(&ctx2), barcode.text_size(&ctx));
        assert_eq!(
            loaded.poly_shape(&ctx2).bbox(0).centre(),
            barcode.poly_shape(&ctx).bbox(0).centre()
        );
    }

    /// `BarcodeFootprintWriteRead` (test_barcode_load_save.cpp) through the board file: the
    /// footprint barcodes of `FRAMES_BOARD` survive a write / read with the same members.
    /// The scaled footprint's barcodes are left out until the writer writes the board-frame
    /// `GetTextSize()` (change request of package M25a).
    #[test]
    fn barcode_footprint_write_read() {
        let board = parse_board(FRAMES_BOARD, "barcode_frames", &ParseOptions::default()).unwrap();
        let text = format_board(&board, &FormatOptions::default()).unwrap();
        let board2 = parse_board(&text, "barcode_frames2", &ParseOptions::default()).unwrap();
        let (ctx, ctx2) = (BoardCtx::bare(&board), BoardCtx::bare(&board2));

        for uuid in [
            "10000000-0000-0000-0000-000000000004",
            "10000000-0000-0000-0000-000000000005",
            "20000000-0000-0000-0000-000000000005",
        ] {
            let (a, b) = (barcode_of(&board, uuid), barcode_of(&board2, uuid));
            assert_eq!(a.text(), b.text());
            assert_eq!((a.kind, a.error_correction), (b.kind, b.error_correction));
            assert_eq!((a.width, a.height), (b.width, b.height));
            assert_eq!(a.text_size(&ctx), b.text_size(&ctx2), "{uuid}");
            assert_eq!(a.lib_pos(), b.lib_pos(), "{uuid}");
            assert_eq!(a.position(&ctx), b.position(&ctx2), "{uuid}");
            assert_eq!(
                a.poly_shape(&ctx).bbox(0).centre(),
                b.poly_shape(&ctx2).bbox(0).centre()
            );
            assert_eq!(PcbBarcode::compare(&ctx, a, a), 0);
            assert!(a.eq_item(&ctx, a));
        }
    }

    /// Port of `BarcodePositioningAlignment` (test_barcode_load_save.cpp). Without an encoder
    /// the canonical symbol is empty, so the coverage check holds trivially; the mutator path
    /// (SetPosition, Rotate, SetShowText, SetIsKnockout) runs as in C++.
    #[test]
    fn barcode_positioning_alignment() {
        let mut board = Board::new();
        let env = MutEnv::detached();

        #[rustfmt::skip]
        let cases = [
            (BarcodeT::QrCode, (0, 0), 2_000_000, 2_000_000, false, false, 0.0, "TEST1"),
            (BarcodeT::QrCode, (5_000_000, 3_000_000), 3_000_000, 3_000_000, false, false, 0.0, "TEST2"),
            (BarcodeT::QrCode, (-2_000_000, -1_000_000), 1_500_000, 1_500_000, false, false, 0.0, "TEST3"),
            (BarcodeT::QrCode, (1_000_000, 2_000_000), 2_500_000, 2_500_000, true, false, 0.0, "WITHTEXT"),
            (BarcodeT::QrCode, (2_000_000, 1_000_000), 2_000_000, 2_000_000, false, true, 0.0, "KNOCKOUT"),
            (BarcodeT::QrCode, (3_000_000, 2_000_000), 2_000_000, 2_000_000, false, false, 45.0, "ROTATED"),
            (BarcodeT::Code39, (4_000_000, 1_000_000), 3_000_000, 800_000, false, false, 0.0, "CODE39TEST"),
            (BarcodeT::Code128, (1_000_000, 4_000_000), 3_500_000, 1_000_000, false, false, 0.0, "CODE128"),
            (BarcodeT::DataMatrix, (3_000_000, 3_000_000), 1_800_000, 1_800_000, false, false, 0.0, "DATAMATRIX"),
            (BarcodeT::MicroQrCode, (2_000_000, 4_000_000), 1_200_000, 1_200_000, false, false, 0.0, "microQR"),
            (BarcodeT::QrCode, (1_500_000, 1_500_000), 2_200_000, 2_200_000, true, true, 90.0, "COMPLEX"),
        ];

        for (i, (kind, pos, width, height, with_text, knockout, angle, text)) in
            cases.into_iter().enumerate()
        {
            let mut barcode = PcbBarcode::new(&mut board.kiid_generator);
            barcode.base.parent = ParentRef::Board;
            barcode.set_text(text);
            barcode.set_show_text(false);
            barcode.set_layer(&env, PcbLayerId::FSilkS);
            barcode.width = width;
            barcode.height = height;
            barcode.set_kind(kind);
            barcode.set_error_correction(BarcodeEccT::M);

            let ctx = BoardCtx::bare(&board);
            let mut canonical_poly = barcode.poly_shape(&ctx);
            let position = Vector2I::new(pos.0, pos.1);

            barcode.set_position(&env, position);

            if angle != 0.0 {
                barcode.rotate(&env, position, EdaAngle::from_degrees(angle));
            }

            barcode.set_show_text(with_text);
            barcode.set_is_knockout(knockout);

            let mut barcode_poly = barcode.poly_shape(&ctx);

            // Barcode poly should completely cover canonical poly
            canonical_poly.rotate(barcode.angle(&ctx), Vector2I::new(0, 0));
            canonical_poly.move_(barcode.position(&ctx));

            let mut no_holes_poly = ShapePolySet::new();
            barcode_poly.unfracture();

            for ii in 0..barcode_poly.outline_count() {
                no_holes_poly.add_outline(barcode_poly.c_outline(ii));
            }

            // Handle rounding errors
            if angle != 0.0 {
                no_holes_poly.inflate(1, CornerStrategy::RoundAllCorners, ARC_LOW_DEF, false);
            }

            canonical_poly.boolean_subtract(&no_holes_poly);
            assert!(
                canonical_poly.is_empty(),
                "Test case {i} ({text}): barcode poly isn't aligned with canonical shape"
            );

            board.add_barcode(barcode, crate::board::AddMode::Append);
        }

        board.check_invariants().unwrap();
    }

    /// Port of `BarcodeTextVariableExpansion` (test_barcode_load_save.cpp): the encoded text is
    /// the shown text. The symbol comparisons hold trivially without an encoder.
    #[test]
    fn barcode_text_variable_expansion() {
        let mut board = Board::new();
        board
            .properties
            .insert("PART_NUMBER".to_string(), "PN12345".to_string());
        board
            .properties
            .insert("VERSION".to_string(), "1.0".to_string());
        let env = MutEnv::detached();

        let make = |board: &mut Board, text: &str, x: f64| {
            let mut barcode = PcbBarcode::new(&mut board.kiid_generator);
            barcode.base.parent = ParentRef::Board;
            barcode.set_text(text);
            barcode.set_layer(&env, PcbLayerId::FSilkS);
            barcode.set_position(
                &env,
                Vector2I::new(PCB_IU_SCALE.mm_to_iu(x), PCB_IU_SCALE.mm_to_iu(50.0)),
            );
            barcode.width = PCB_IU_SCALE.mm_to_iu(10.0);
            barcode.height = PCB_IU_SCALE.mm_to_iu(10.0);
            barcode.set_kind(BarcodeT::QrCode);
            barcode.set_error_correction(BarcodeEccT::M);
            board.add_barcode(barcode, crate::board::AddMode::Append)
        };

        let var = make(&mut board, "${PART_NUMBER}_${VERSION}", 50.0);
        let reference = make(&mut board, "PN12345_1.0", 100.0);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(board[var].text(), "${PART_NUMBER}_${VERSION}");
        assert_eq!(
            board[var].shown_text(&ctx, ResolutionContext::ForCanvas),
            "PN12345_1.0"
        );

        let (a, b) = (
            board[var].symbol_poly(&ctx),
            board[reference].symbol_poly(&ctx),
        );
        assert_eq!(a.outline_count(), b.outline_count());
        assert_eq!(a.bbox(0).width(), b.bbox(0).width());
        assert_eq!(a.bbox(0).height(), b.bbox(0).height());
    }

    /// `BarcodeUndefinedVariable` (test_barcode_load_save.cpp) as far as it does not need the
    /// encoder: the raw and shown texts. (`GetSymbolPoly().OutlineCount() > 0` needs a QR
    /// encoder, PORT_STATUS.md.)
    #[test]
    fn barcode_undefined_variable() {
        let mut board = Board::new();
        let mut barcode = PcbBarcode::new(&mut board.kiid_generator);
        barcode.set_text("${UNDEFINED_VAR}");
        barcode.set_kind(BarcodeT::QrCode);
        let id = board.add_barcode(barcode, crate::board::AddMode::Append);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(board[id].text(), "${UNDEFINED_VAR}");
        assert_eq!(
            board[id].shown_text(&ctx, ResolutionContext::ForCanvas),
            "${UNDEFINED_VAR}"
        );
    }

    /// Port of `BarcodeCode39UndefinedVariable` (test_barcode_load_save.cpp): CODE_39 cannot
    /// encode `${}`, the symbol stays empty.
    #[test]
    fn barcode_code39_undefined_variable() {
        let mut board = Board::new();
        let env = MutEnv::detached();
        let mut barcode = PcbBarcode::new(&mut board.kiid_generator);
        barcode.base.parent = ParentRef::Board;
        barcode.set_text("${UNDEFINED_VAR}");
        barcode.set_layer(&env, PcbLayerId::FSilkS);
        barcode.set_position(
            &env,
            Vector2I::new(PCB_IU_SCALE.mm_to_iu(50.0), PCB_IU_SCALE.mm_to_iu(50.0)),
        );
        barcode.width = PCB_IU_SCALE.mm_to_iu(20.0);
        barcode.height = PCB_IU_SCALE.mm_to_iu(5.0);
        barcode.set_kind(BarcodeT::Code39);
        let id = board.add_barcode(barcode, crate::board::AddMode::Append);
        let ctx = BoardCtx::bare(&board);

        assert_eq!(board[id].symbol_poly(&ctx).outline_count(), 0);
    }

    /// `SetKind` / `SetErrorCorrection` (Micro QR has no H), `KeepSquare` sizes, margins,
    /// `Compare` ordering, item description and the zint settings of `ComputeBarcode`.
    #[test]
    fn setters_compare_and_encoder_options() {
        let mut b = PcbBarcode::default();
        b.set_error_correction(BarcodeEccT::H);
        b.set_kind(BarcodeT::MicroQrCode);
        assert_eq!(b.error_correction, BarcodeEccT::Q);
        b.set_error_correction(BarcodeEccT::H);
        assert_eq!(b.error_correction, BarcodeEccT::Q);

        b.set_barcode_width(7);
        assert_eq!((b.width, b.height), (7, 7));
        b.set_kind(BarcodeT::Code128);
        b.set_barcode_height(3);
        assert_eq!((b.width, b.height), (7, 3));
        b.set_margin_x(5);
        b.set_margin_y(2_000_000);
        assert_eq!(b.margin, Vector2I::new(1_000_000, 2_000_000));

        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut c = b.clone();
        c.width = 8;
        assert!(PcbBarcode::compare(&ctx, &b, &c) < 0);
        assert!(PcbBarcode::compare(&ctx, &c, &b) > 0);
        b.set_text("A");
        assert_eq!(
            b.item_description(
                &ctx,
                &UnitsProvider::new(&PCB_IU_SCALE, kicad_core::eda_units::EdaUnits::Mm),
                false
            ),
            "Barcode 'A' on User.Drawings"
        );

        let qr = zint_options(BarcodeT::QrCode, "\u{e9}", BarcodeEccT::H);
        assert_eq!((qr.symbology, qr.option_1, qr.eci), (58, 4, ECI_UTF8));
        let mqr = zint_options(BarcodeT::MicroQrCode, "\u{e9}", BarcodeEccT::Q);
        assert_eq!((mqr.symbology, mqr.option_1, mqr.eci), (97, 3, 0));
        let c39 = zint_options(BarcodeT::Code39, "AB", BarcodeEccT::L);
        assert_eq!((c39.symbology, c39.option_1, c39.eci), (8, -1, 0));
    }

    /// The `(barcode ..)` blocks of `kicad-cli pcb upgrade --force` of `FRAMES_BOARD`
    /// (reference build), one trimmed line per token line.
    const FRAMES_RESAVE: &str = r#"(barcode
(at 2 3 15)
(layer "F.SilkS")
(size 6 6)
(text "")
(text_height 1)
(type qr)
(ecc_level M)
(hide no)
(knockout yes)
(margins 0.5 0.7)
(uuid "10000000-0000-0000-0000-000000000004")
)
(barcode
(at -4 1 0)
(layer "F.SilkS")
(size 8 3)
(text "FPCODE")
(text_height 1)
(type code128)
(hide no)
(knockout no)
(uuid "10000000-0000-0000-0000-000000000005")
)
(barcode
(at 3.3 -1.7 33)
(layer "F.SilkS")
(size 7 7)
(text "")
(text_height 1.2)
(type microqr)
(ecc_level Q)
(hide no)
(knockout yes)
(margins 1.1 0.3)
(uuid "30000000-0000-0000-0000-000000000004")
)
(barcode
(at -2 1 0)
(layer "F.Fab")
(size 9 2)
(text "SCALE")
(text_height 1.2)
(type code39)
(hide yes)
(knockout no)
(uuid "30000000-0000-0000-0000-000000000005")
)
(barcode
(at 1.5 -2 45)
(layer "B.SilkS")
(size 5 5)
(text "")
(text_height 0.8)
(type datamatrix)
(hide no)
(knockout yes)
(uuid "20000000-0000-0000-0000-000000000004")
)
(barcode
(at -1 2.5 340)
(layer "B.SilkS")
(size 4 4)
(text "BACK")
(text_height 0.8)
(type qr)
(ecc_level H)
(hide no)
(knockout yes)
(uuid "20000000-0000-0000-0000-000000000005")
)
(barcode
(at 60 20 90)
(layer "F.Cu")
(size 10 10)
(text "")
(text_height 1)
(type qr)
(ecc_level L)
(hide no)
(knockout no)
(uuid "40000000-0000-0000-0000-000000000003")
)
(barcode
(at 80 20 315)
(layer "B.Cu")
(size 10 10)
(text "HELLO")
(text_height 1)
(type qr)
(ecc_level L)
(hide no)
(knockout yes)
(uuid "40000000-0000-0000-0000-000000000004")
)
(barcode
(at 20 20 0)
(layer "F.SilkS")
(size 10 10)
(text "")
(text_height 1)
(type qr)
(ecc_level L)
(hide no)
(knockout yes)
(uuid "40000000-0000-0000-0000-000000000001")
)
(barcode
(at 40 20 37.5)
(layer "B.SilkS")
(size 12 8)
(text "")
(text_height 1)
(type code39)
(hide no)
(knockout yes)
(margins 2 3)
(uuid "40000000-0000-0000-0000-000000000002")
)
(barcode
(at 80.123456 21.654321 0)
(layer "Dwgs.User")
(size 0 0)
(text "")
(text_height 1)
(type datamatrix)
(hide no)
(knockout yes)
(uuid "40000000-0000-0000-0000-000000000005")
)"#;

    /// The `(barcode` blocks of a prettified board file, lines trimmed.
    fn barcode_blocks(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if lines[i].trim() == "(barcode" {
                let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
                let mut j = i;

                while lines[j] != format!("{indent})") {
                    j += 1;
                }

                let block: Vec<&str> = lines[i..=j].iter().map(|l| l.trim()).collect();
                out.push(block.join("\n"));
                i = j + 1;
            } else {
                i += 1;
            }
        }

        out
    }

    /// Byte level: the Rust parser + writer re-save the barcodes of `FRAMES_BOARD` like the
    /// reference `pcb upgrade --force` (position and angle from `GetPosition` / `GetAngle`
    /// through the footprint frames). The scaled footprint's two blocks differ in
    /// `text_height` until the writer writes the board-frame `GetTextSize()` (change request
    /// of package M25a); they are checked apart from that line.
    #[test]
    fn resave_matches_oracle() {
        let board = parse_board(FRAMES_BOARD, "barcode_frames", &ParseOptions::default()).unwrap();
        let text = format_board(&board, &FormatOptions::default()).unwrap();
        let got = barcode_blocks(&text);
        let want: Vec<&str> = FRAMES_RESAVE.split("\n)\n").collect();
        assert_eq!(got.len(), want.len());

        for (g, w) in got.iter().zip(want) {
            let w = if w.ends_with("\n)") {
                w.to_string()
            } else {
                format!("{w}\n)")
            };

            if w.contains("30000000-0000-0000-0000-00000000000") {
                let strip = |b: &str| {
                    b.lines()
                        .filter(|l| !l.starts_with("(text_height"))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                assert_eq!(strip(g), strip(&w));
            } else {
                assert_eq!(g, &w);
            }
        }
    }
}
