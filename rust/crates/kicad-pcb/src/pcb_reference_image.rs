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

//! Port of `pcbnew/pcb_reference_image.h` and `pcbnew/pcb_reference_image.cpp`
//! (`PCB_REFERENCE_IMAGE`) with its `REFERENCE_IMAGE` member (`include/reference_image.h`,
//! `common/reference_image.cpp`) and the data part of `BITMAP_BASE`
//! (`include/bitmap_base.h`), `board-model-types.md` section 10.
//!
//! The image payload is kept verbatim (`encoded_data`); pixels are never decoded, the size and
//! PPI come from a header-only probe (package M24, which owns the behaviour of this module).
//! Dropped: `BITMAP_BASE`'s `wxImage` / `wxBitmap` members, mirror / rotation preview state,
//! `m_pixelSizeIu` (derived from `ppi`).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId};
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, EdaOrientation, FlipDirection, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `PCB_REFERENCE_IMAGE` + `REFERENCE_IMAGE` + the parts of `BITMAP_BASE` that are data.
#[derive(Clone, Debug)]
pub struct PcbReferenceImage {
    pub base: BoardItemBase,
    /// REFERENCE_IMAGE::m_pos: XY coordinates of the center of the bitmap.
    pub pos: Vector2I,
    pub transform_origin_offset: Vector2I, // REFERENCE_IMAGE::m_transformOriginOffset
    pub orientation: EdaOrientation,       // REFERENCE_IMAGE::m_orientation
    pub opacity: f64,                      // REFERENCE_IMAGE::m_opacity
    pub scale: f64,                        // BITMAP_BASE::m_scale
    /// The `(data ...)` payload exactly as in the file (base64 text of the PNG/JPEG stream).
    pub encoded_data: String,
    /// Width, height in pixels and PPI read from the image header only (PNG IHDR/pHYs, JPEG
    /// SOF/JFIF): enough for GetBoundingBox. Pixels are never decoded.
    pub pixel_size: Vector2I,
    pub ppi: i32, // BITMAP_BASE::m_ppi
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbReferenceImage {
    /// `PCB_REFERENCE_IMAGE( aParent, pos = (0, 0), aLayer = F_Cu )`
    /// (`pcb_reference_image.cpp`), `REFERENCE_IMAGE( pcbIUScale )` (`reference_image.cpp`:
    /// position and origin offset (0, 0), orientation 0, opacity 1.0) and `BITMAP_BASE()`
    /// (`bitmap_base.cpp`: scale 1.0, 300 PPI, no image); nil UUID.
    fn default() -> Self {
        PcbReferenceImage {
            base: BoardItemBase::with_layer(PcbLayerId::FCu),
            pos: Vector2I::new(0, 0),
            transform_origin_offset: Vector2I::new(0, 0),
            orientation: EdaOrientation::new(ANGLE_0),
            opacity: 1.0,
            scale: 1.0, // 1.0 = original bitmap size
            encoded_data: String::new(),
            pixel_size: Vector2I::new(0, 0),
            ppi: 300, // the bitmap definition. the default is 300PPI
        }
    }
}

impl PcbReferenceImage {
    /// `new PCB_REFERENCE_IMAGE( aParent, VECTOR2I( 0, 0 ), aLayer )`: the constructor values
    /// on `layer` with a fresh UUID. A non-zero position is set afterwards with
    /// `set_position`, which applies the C++ overflow check.
    pub fn new(kiid_generator: &mut KiidGenerator, layer: PcbLayerId) -> Self {
        let mut image = PcbReferenceImage::default();
        image.base.layer = layer;
        image.base.uuid = Kiid::new(kiid_generator);
        image
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbReferenceImage {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbReferenceImage {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbReferenceImageT
    }
    fn class_name(&self) -> &'static str {
        "PCB_REFERENCE_IMAGE"
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbReferenceImage::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbReferenceImage::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbReferenceImage::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbReferenceImage::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbReferenceImage::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbReferenceImage::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbReferenceImage {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbReferenceImage::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbReferenceImage::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbReferenceImage::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbReferenceImage::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbReferenceImage::mirror(self, env, centre, dir)
    }
}

// ---- inherent methods (package M24) -------------------------------------------------------

impl PcbReferenceImage {
    /// `REFERENCE_IMAGE::GetOpacity()`.
    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// `REFERENCE_IMAGE::GetImageScale()` (`BITMAP_BASE::GetScale()`).
    pub fn image_scale(&self) -> f64 {
        self.scale
    }

    /// `REFERENCE_IMAGE::GetTransformOriginOffset()`.
    pub fn transform_origin_offset(&self) -> Vector2I {
        self.transform_origin_offset
    }

    /// `REFERENCE_IMAGE::GetImageOrientation()` / `PCB_REFERENCE_IMAGE::GetOrientation()`.
    pub fn orientation(&self) -> EdaAngle {
        self.orientation.angle()
    }

    /// `REFERENCE_IMAGE::SetImageScale( aScale )` (rejects a scale whose box would overflow).
    pub fn set_image_scale(&mut self, scale: f64) {
        crate::port_pending!("REFERENCE_IMAGE::SetImageScale", scale)
    }

    /// `REFERENCE_IMAGE::GetSize()`: the image size in IU (pixel size, PPI and scale).
    pub fn size(&self) -> Vector2I {
        crate::port_pending!("REFERENCE_IMAGE::GetSize")
    }

    /// `PCB_REFERENCE_IMAGE::GetPosition()`: `REFERENCE_IMAGE::m_pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_REFERENCE_IMAGE::SetPosition()` = `REFERENCE_IMAGE::SetPosition`: ignored when
    /// the box centred on `aPos` is not `IsBOX2Safe`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("REFERENCE_IMAGE::SetPosition", env, pos)
    }

    /// `PCB_REFERENCE_IMAGE::Move()`: `SetPosition( GetPosition() + aMoveVector )` (defers to
    /// SetPosition to check the new position overflow).
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let pos = self.pos + delta;
        self.set_position(env, pos);
    }

    /// `PCB_REFERENCE_IMAGE::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_REFERENCE_IMAGE::Rotate", env, centre, angle)
    }

    /// `PCB_REFERENCE_IMAGE::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_REFERENCE_IMAGE::Flip", env, centre, dir)
    }

    /// `PCB_REFERENCE_IMAGE::Mirror( aMirrorRef, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_REFERENCE_IMAGE::Mirror", env, centre, dir)
    }

    /// `PCB_REFERENCE_IMAGE::GetBoundingBox()`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_REFERENCE_IMAGE::GetBoundingBox", ctx)
    }

    /// `PCB_REFERENCE_IMAGE::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!(
            "PCB_REFERENCE_IMAGE::GetEffectiveShape",
            ctx,
            layer,
            flash,
            usage
        )
    }

    /// `PCB_REFERENCE_IMAGE::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_REFERENCE_IMAGE::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_REFERENCE_IMAGE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!(
            "PCB_REFERENCE_IMAGE::HitTest (rect)",
            ctx,
            rect,
            contained,
            accuracy
        )
    }

    /// `PCB_REFERENCE_IMAGE::GetItemDescription()`: "Reference Image".
    pub fn item_description(
        &self,
        _ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        "Reference Image".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_REFERENCE_IMAGE::PCB_REFERENCE_IMAGE( .. )` (pcbnew/pcb_reference_image.cpp),
    /// `REFERENCE_IMAGE::REFERENCE_IMAGE( aIuScale )` (common/reference_image.cpp),
    /// `BITMAP_BASE::BITMAP_BASE( pos )` (common/bitmap_base.cpp).
    #[test]
    fn reference_image_constructor_defaults() {
        let i = PcbReferenceImage::default();
        assert_eq!(i.base.layer, PcbLayerId::FCu);
        assert_eq!(i.pos, Vector2I::new(0, 0));
        assert_eq!(i.transform_origin_offset(), Vector2I::new(0, 0));
        assert_eq!(i.orientation(), ANGLE_0);
        assert_eq!(i.opacity(), 1.0);
        assert_eq!(i.image_scale(), 1.0);
        assert_eq!(i.ppi, 300);
        assert!(i.encoded_data.is_empty());
        assert_eq!(i.pixel_size, Vector2I::new(0, 0));
        assert!(i.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&i), "PCB_REFERENCE_IMAGE");

        let mut kiid_generator = KiidGenerator::default();
        let i = PcbReferenceImage::new(&mut kiid_generator, PcbLayerId::UserDrawings);
        assert_eq!(i.base.layer, PcbLayerId::UserDrawings);
        assert!(i.base.uuid != Kiid::default());
    }

    /// The image in the fixture footprint reads its position through the trait.
    #[test]
    fn reference_image_in_footprint() {
        let mut fx = rotated_flipped_scaled();
        let id = fx
            .child(ItemKind::Image)
            .typed::<PcbReferenceImage>()
            .unwrap();
        let mut i = PcbReferenceImage::default();
        i.pos = Vector2I::new(4, 5);
        fx.set_child(id, i);
        fx.board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&fx.board);
        assert_eq!(
            BoardItem::position(&fx.board[id], &ctx),
            Vector2I::new(4, 5)
        );
    }
}
