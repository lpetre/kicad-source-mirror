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
//! constructor and delegation shims (package M0c), the overrides (stubs; the symbol encoder is
//! tier 2, package M25: until then the shape is the bounding rectangle, `board-model-types.md`
//! 17).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, EdaOrientation, FlipDirection, ShapePolySet, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::cache::KeyedCache;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, SharedShape};
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

    /// `GetAngle()`: board frame.
    pub fn angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PCB_BARCODE::GetAngle", ctx)
    }

    /// `AssembleBarcode()`: encode the symbol and fill the cache (tier 2, M25).
    pub fn assemble_barcode(&self, ctx: &BoardCtx<'_>) -> std::sync::Arc<PcbBarcodeCache> {
        crate::port_pending!("PCB_BARCODE::AssembleBarcode", ctx)
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
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbBarcode::style_from_settings(self, settings, check_side)
    }
}

// ---- the PCB_BARCODE overrides (packages M25 / M10) --------------------------------------

impl PcbBarcode {
    /// `PCB_BARCODE::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_BARCODE::GetBoundingBox", ctx)
    }

    /// `PCB_BARCODE::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `PCB_BARCODE::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_BARCODE::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_BARCODE::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_BARCODE::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_BARCODE::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_BARCODE::HitTest", ctx, rect, contained, accuracy)
    }

    /// `PCB_BARCODE::IsKnockout()`: `BOARD_ITEM::IsKnockout()`.
    pub fn is_knockout(&self) -> bool {
        self.base.is_knockout
    }

    /// `PCB_BARCODE::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_BARCODE::GetItemDescription", ctx, units, full)
    }

    /// `PCB_BARCODE::GetPosition`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_BARCODE::GetPosition", ctx)
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
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PCB_BARCODE::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_BARCODE::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_BARCODE::Flip", env, centre, dir)
    }

    /// `PCB_BARCODE::Move`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PCB_BARCODE::Move", env, delta)
    }

    /// `PCB_BARCODE::OnFootprintRescaled`.
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
            "PCB_BARCODE::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `PCB_BARCODE::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_BARCODE::Rotate", env, centre, angle)
    }

    /// `PCB_BARCODE::SetLayer`.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        crate::port_pending!("PCB_BARCODE::SetLayer", env, layer)
    }

    /// `PCB_BARCODE::SetPosition`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PCB_BARCODE::SetPosition", env, pos)
    }

    /// `PCB_BARCODE::StyleFromSettings`.
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        crate::port_pending!("PCB_BARCODE::StyleFromSettings", settings, check_side)
    }
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
}
