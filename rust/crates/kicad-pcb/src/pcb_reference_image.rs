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
//! The image payload is kept as the base64 text of the original image bytes (`encoded_data`);
//! pixels are never decoded, the size and PPI come from a header-only probe of the PNG / JPEG
//! stream ([`probe_image_header`], replacing `wxImage::LoadFile`). Dropped: `BITMAP_BASE`'s
//! `wxImage` / `wxBitmap` members, mirror / rotation preview state, `m_pixelSizeIu` (derived
//! from `ppi`, as `REFERENCE_IMAGE::updatePixelSizeInIU` keeps it).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId};
use std::sync::Arc;

use kicad_geom::geometry::geometry_utils::{box_hit_test_box, box_hit_test_point, is_vec2_safe_xy};
use kicad_geom::{
    ANGLE_0, Box2D, Box2I, EdaAngle, EdaOrientation, FlipDirection, Seg, ShapeRect, Vector2D,
    Vector2I, ki_round, rotate_point, rotate_point_around,
};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::embedded_files::{wx_base64_decode, wx_base64_encode};
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

/// `pcbIUScale.MilsToIU( 1000 )`: the IU per inch of the board (25.4 mm).
const IU_PER_INCH: f64 = 25_400_000.0;

impl PcbReferenceImage {
    /// `REFERENCE_IMAGE::GetOpacity()`.
    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// `REFERENCE_IMAGE::SetOpacity()`: clamped to [0, 1].
    pub fn set_opacity(&mut self, opacity: f64) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// `REFERENCE_IMAGE::GetImageScale()` (`BITMAP_BASE::GetScale()`).
    pub fn image_scale(&self) -> f64 {
        self.scale
    }

    /// `REFERENCE_IMAGE::GetTransformOriginOffset()`.
    pub fn transform_origin_offset(&self) -> Vector2I {
        self.transform_origin_offset
    }

    /// `REFERENCE_IMAGE::SetTransformOriginOffset()`.
    pub fn set_transform_origin_offset(&mut self, offset: Vector2I) {
        self.transform_origin_offset = offset;
    }

    /// `REFERENCE_IMAGE::GetImageOrientation()` / `PCB_REFERENCE_IMAGE::GetOrientation()`.
    pub fn orientation(&self) -> EdaAngle {
        self.orientation.angle()
    }

    /// `REFERENCE_IMAGE::SetOrientation()` / `PCB_REFERENCE_IMAGE::SetOrientation()`.
    pub fn set_orientation(&mut self, angle: EdaAngle) {
        self.orientation = EdaOrientation::new(angle);
    }

    /// `BITMAP_BASE::GetPPI()`.
    pub fn ppi(&self) -> i32 {
        self.ppi
    }

    /// `BITMAP_BASE::GetImageData() != nullptr`: an image was read successfully.
    pub fn has_image(&self) -> bool {
        self.pixel_size.x > 0 && self.pixel_size.y > 0
    }

    /// `BITMAP_BASE::GetSizePixels()`: (0, 0) without an image.
    pub fn size_pixels(&self) -> Vector2I {
        self.pixel_size
    }

    /// `BITMAP_BASE::GetPixelSizeIu()` as `REFERENCE_IMAGE::updatePixelSizeInIU` keeps it:
    /// `(double) pcbIUScale.MilsToIU( 1000 ) / GetPPI()`.
    pub fn pixel_size_iu(&self) -> f64 {
        IU_PER_INCH / self.ppi as f64
    }

    /// `BITMAP_BASE::GetScalingFactor()`: `m_pixelSizeIu * m_scale`.
    pub fn scaling_factor(&self) -> f64 {
        self.pixel_size_iu() * self.scale
    }

    /// `REFERENCE_IMAGE::GetSize()` = `BITMAP_BASE::GetSize()`: the unrotated image size in
    /// IU, (0, 0) without an image.
    pub fn size(&self) -> Vector2I {
        let mut size = Vector2I::new(0, 0);

        if self.has_image() {
            size.x = ki_round(self.pixel_size.x as f64 * self.scaling_factor());
            size.y = ki_round(self.pixel_size.y as f64 * self.scaling_factor());
        }

        size
    }

    /// `REFERENCE_IMAGE::ReadImageFile( wxMemoryBuffer& )` (`BITMAP_BASE::ReadImageFile`,
    /// `SetImage`, `updatePPI`): keep `data` as the image data, read width, height and
    /// resolution from its header (PNG / JPEG, see [`probe_image_header`]) and update the PPI
    /// like `BITMAP_BASE::updatePPI` (it keeps its previous value when the image carries no
    /// usable resolution). Returns false if the image cannot be read; as in C++ the stored
    /// data is replaced anyway and the previous image (size, PPI) is kept.
    pub fn read_image_file(&mut self, data: &[u8]) -> bool {
        // Store the original image data in m_imageData
        self.encoded_data = wx_base64_encode(data);

        let Some(header) = probe_image_header(data) else {
            return false;
        };

        // SetImage: the size and the PPI of the new image
        self.pixel_size = Vector2I::new(header.width, header.height);
        self.ppi = header.update_ppi(self.ppi);
        true
    }

    /// `REFERENCE_IMAGE::PackToBytes` / `BITMAP_BASE::SaveImageData` with `m_imageData` set:
    /// the stored image bytes (empty without an image).
    pub fn image_data(&self) -> Vec<u8> {
        wx_base64_decode(&self.encoded_data).unwrap_or_default()
    }

    /// `BITMAP_BASE::GetLegacyPPI()`: the PPI KiCad derived before the pixels-per-cm fix
    /// (board versions before 20260623), read again from the image header.
    pub fn legacy_ppi(&self) -> i32 {
        if !self.has_image() {
            return self.ppi;
        }

        match probe_image_header(&self.image_data()) {
            Some(header) => header.legacy_ppi(self.ppi),
            None => self.ppi,
        }
    }

    /// `REFERENCE_IMAGE::boundingSize( aOrientation )`: the axis-aligned extent of the image
    /// rotated by `orientation`.
    fn bounding_size_at(&self, orientation: EdaAngle) -> Vector2I {
        // Un-rotated size of the image.
        let size = self.size();

        if orientation.is_horizontal() {
            return size;
        }

        if orientation.is_vertical() {
            return Vector2I::new(size.y, size.x);
        }

        // The raster is stored unrotated, so its axis-aligned extent grows with the orientation.
        let angle_cos = orientation.cos().abs();
        let angle_sin = orientation.sin().abs();

        let w = size.x as f64 * angle_cos + size.y as f64 * angle_sin;
        let h = size.x as f64 * angle_sin + size.y as f64 * angle_cos;

        Vector2I::new(ki_round(w), ki_round(h))
    }

    /// `REFERENCE_IMAGE::boundingSize()`: the extent at the current orientation.
    fn bounding_size(&self) -> Vector2I {
        self.bounding_size_at(self.orientation.angle())
    }

    /// `BOX2D::ByCenter( aCenter, aSize )` followed by `IsBOX2Safe`.
    fn box2d_safe(center: Vector2D, size: Vector2D) -> bool {
        Box2D::by_center(center, size).is_box2_safe()
    }

    /// `REFERENCE_IMAGE::SetImageScale( aScale )`: rejects a non-positive scale and (in
    /// `scaleBy`) a scale whose box would overflow.
    pub fn set_image_scale(&mut self, scale: f64) {
        if scale <= 0.0 {
            return;
        }

        let ratio = scale / self.scale;
        self.scale_by(ratio);
    }

    /// `REFERENCE_IMAGE::SetWidth( aWidth )`.
    pub fn set_width(&mut self, width: i32) {
        if width <= 0 {
            return;
        }

        let ratio = width as f64 / self.size().x as f64;
        self.scale_by(ratio);
    }

    /// `REFERENCE_IMAGE::SetHeight( aHeight )`.
    pub fn set_height(&mut self, height: i32) {
        if height <= 0 {
            return;
        }

        let ratio = height as f64 / self.size().y as f64;
        self.scale_by(ratio);
    }

    /// `REFERENCE_IMAGE::scaleBy( aRatio )`: scale about the transform origin.
    fn scale_by(&mut self, ratio: f64) {
        if ratio <= 0.0 {
            return;
        }

        // `m_pos + m_transformOriginOffset` is an int sum in C++ (wraps on the reference build).
        let current_origin = Vector2D::new(
            self.pos.x.wrapping_add(self.transform_origin_offset.x) as f64,
            self.pos.y.wrapping_add(self.transform_origin_offset.y) as f64,
        );
        let new_offset = Vector2D::new(
            self.transform_origin_offset.x as f64 * ratio,
            self.transform_origin_offset.y as f64 * ratio,
        );
        let new_center = Vector2D::new(
            current_origin.x - new_offset.x,
            current_origin.y - new_offset.y,
        );
        let bounding = self.bounding_size();
        let new_size = Vector2D::new(bounding.x as f64 * ratio, bounding.y as f64 * ratio);

        // The span of the image is limited to the size of the coordinate system
        if !is_vec2_safe_xy(new_size) {
            return;
        }

        // Any overflow, just reject the call
        if !Self::box2d_safe(new_center, new_size) {
            return;
        }

        self.scale *= ratio;
        self.transform_origin_offset = ki_round(new_offset);

        // Don't need to recheck the box, we just did that
        self.pos = ki_round(new_center);
    }

    /// `PCB_REFERENCE_IMAGE::GetPosition()`: `REFERENCE_IMAGE::m_pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_REFERENCE_IMAGE::SetPosition()` = `REFERENCE_IMAGE::SetPosition`: ignored when
    /// the box centred on `aPos` is not `IsBOX2Safe`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.reference_image_set_position(pos);
    }

    /// `REFERENCE_IMAGE::SetPosition( aPos )` (no environment needed).
    pub fn reference_image_set_position(&mut self, pos: Vector2I) {
        let size = self.bounding_size();

        if !Self::box2d_safe(
            Vector2D::new(pos.x as f64, pos.y as f64),
            Vector2D::new(size.x as f64, size.y as f64),
        ) {
            return;
        }

        self.pos = pos;
    }

    /// `PCB_REFERENCE_IMAGE::Move()`: `SetPosition( GetPosition() + aMoveVector )` (defers to
    /// SetPosition to check the new position overflow).
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let pos = self.pos + delta;
        self.set_position(env, pos);
    }

    /// `PCB_REFERENCE_IMAGE::Rotate()` = `REFERENCE_IMAGE::Rotate`: the rotation is stored on
    /// the image (the raster is never resampled) and the transform origin follows the content.
    pub fn rotate(&mut self, _env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        rotate_point_around(&mut self.pos, centre, angle);
        rotate_point(&mut self.transform_origin_offset, angle);

        self.orientation += angle;
    }

    /// `REFERENCE_IMAGE::Flip( aCentre, aFlipDirection )`: false (nothing changed) if the
    /// mirrored box would overflow.
    ///
    /// Deviation: `BITMAP_BASE::Mirror` also mirrors the pixels and drops the original image
    /// data, so that C++ re-encodes the mirrored raster on save; pixels are never decoded
    /// here, the stored image data stays as it is (`PORT_STATUS.md`).
    fn reference_image_flip(&mut self, centre: Vector2I, dir: FlipDirection) -> bool {
        let mut new_pos = self.pos;
        // MIRROR wraps on `int` overflow like the reference build (the oracle's `edgeflip` case).
        kicad_geom::mirror_point(&mut new_pos, centre, dir);

        let size = self.bounding_size();

        if !Self::box2d_safe(
            Vector2D::new(new_pos.x as f64, new_pos.y as f64),
            Vector2D::new(size.x as f64, size.y as f64),
        ) {
            return false;
        }

        self.pos = new_pos;

        // A reflection reverses the sense of the stored rotation.
        self.orientation = -self.orientation;

        kicad_geom::mirror_point(&mut self.transform_origin_offset, Vector2I::new(0, 0), dir);
        true
    }

    /// `PCB_REFERENCE_IMAGE::Flip()`: mirror, then move to the flipped layer.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        if self.reference_image_flip(centre, dir) {
            // SetLayer( GetFlippedLayer( GetLayer() ) ): BOARD_ITEM::SetLayer is m_layer = aLayer.
            self.base.layer = env.layers.flip_layer(self.base.layer);
        }
    }

    /// `PCB_REFERENCE_IMAGE::Mirror( aMirrorRef, aFlipDirection )`: mirror the content about
    /// the axis through `centre` without changing layers.
    pub fn mirror(&mut self, _env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.reference_image_flip(centre, dir);
    }

    /// `PCB_REFERENCE_IMAGE::Mirror( const SEG& aAxis )` = `REFERENCE_IMAGE::Mirror`: mirror
    /// the content about `axis` without changing layers; false if the box would overflow.
    /// The pixel deviation of [`flip`](Self::flip) applies.
    pub fn mirror_seg(&mut self, _env: &MutEnv, axis: &Seg) -> bool {
        let axis_angle = -EdaAngle::from_vector(axis.b - axis.a);
        let new_orientation = self.orientation.angle().reflect(axis_angle);
        let new_pos = axis.reflect_point(self.pos);

        // Check the bounding box with the new orientation to ensure it fits within the
        // coordinate system.
        let size = self.bounding_size_at(new_orientation);

        if !Self::box2d_safe(
            Vector2D::new(new_pos.x as f64, new_pos.y as f64),
            Vector2D::new(size.x as f64, size.y as f64),
        ) {
            return false;
        }

        self.pos = new_pos;
        self.orientation = EdaOrientation::new(new_orientation);

        // The transform origin is an offset from the image center, so it reflects about the
        // axis through the image origin and not about the mirror axis itself.
        let offset_axis = Seg::new(Vector2I::new(0, 0), axis.b - axis.a);
        self.transform_origin_offset = offset_axis.reflect_point(self.transform_origin_offset);

        true
    }

    /// `PCB_REFERENCE_IMAGE::GetBoundingBox()` = `REFERENCE_IMAGE::GetBoundingBox`:
    /// `BOX2I::ByCenter( m_pos, boundingSize() )`.
    pub fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        self.reference_image_bounding_box()
    }

    /// `REFERENCE_IMAGE::GetBoundingBox()` (no context needed).
    pub fn reference_image_bounding_box(&self) -> Box2I {
        Box2I::by_center(self.pos, self.bounding_size())
    }

    /// `PCB_REFERENCE_IMAGE::GetEffectiveShape()`: the bounding box as a `SHAPE_RECT` (the
    /// `int64` box size narrowed to `int` as the C++ constructor call does).
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        _layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let bbox = self.bounding_box(ctx);
        Arc::new(ShapeRect::from_pos_size(
            bbox.position(),
            bbox.width() as i32,
            bbox.height() as i32,
        ))
    }

    /// `PCB_REFERENCE_IMAGE::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        box_hit_test_point(pos, &self.bounding_box(ctx), accuracy)
    }

    /// `PCB_REFERENCE_IMAGE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        box_hit_test_box(rect, &self.bounding_box(ctx), contained, accuracy)
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

// ---- image header probe (replaces wxImage::LoadFile, no pixel decoding) -------------------

/// `wxIMAGE_RESOLUTION_CM`.
const WX_IMAGE_RESOLUTION_CM: i32 = 2;

/// The `wxIMAGE_OPTION_RESOLUTIONX` option of the loaded `wxImage`, as wx 3.2 sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageResolution {
    /// No resolution option (no PNG `pHYs`, no JPEG JFIF marker).
    None,
    /// `SetOption( name, int )`: PNG unknown unit (the `png_uint_32` converted to `int`), JPEG
    /// JFIF density.
    Int(i32),
    /// PNG pixels per metre: wx stores `wxString::Format( "%f", ppm / 100.0 )` (pixels/cm).
    PngPerMetre(u32),
}

/// What `BITMAP_BASE::SetImage` / `updatePPI` read from a `wxImage`: the size in pixels and
/// the resolution options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: i32,
    pub height: i32,
    /// `wxIMAGE_OPTION_RESOLUTIONX`.
    pub resolution_x: ImageResolution,
    /// `GetOptionInt( wxIMAGE_OPTION_RESOLUTIONUNIT )` (0 when absent).
    pub resolution_unit: i32,
}

impl ImageHeader {
    /// `BITMAP_BASE::updatePPI()`: the PPI after reading this image, `current` if the image has
    /// no usable resolution (`dpiX <= 1`).
    pub fn update_ppi(&self, current: i32) -> i32 {
        // GetOption( wxIMAGE_OPTION_RESOLUTIONX ).ToCDouble( &dpiX ): the "%f" text of
        // ppm / 100.0 parses back to that double exactly.
        let dpi_x = match self.resolution_x {
            ImageResolution::None => 0.0,
            ImageResolution::Int(v) => v as f64,
            ImageResolution::PngPerMetre(ppm) => ppm as f64 / 100.0,
        };

        if dpi_x > 1.0 {
            if self.resolution_unit == WX_IMAGE_RESOLUTION_CM {
                ki_round(dpi_x * 2.54)
            } else {
                ki_round(dpi_x)
            }
        } else {
            current
        }
    }

    /// `BITMAP_BASE::GetLegacyPPI()` for this image: the pre-fix path read pixels/cm with
    /// `GetOptionInt()` (`wxAtoi`), truncating "37.8" to 37 before the * 2.54 multiply.
    pub fn legacy_ppi(&self, ppi: i32) -> i32 {
        let dpi_x: i64 = match self.resolution_x {
            ImageResolution::None => 0,
            ImageResolution::Int(v) => i64::from(v),
            ImageResolution::PngPerMetre(ppm) => i64::from(ppm / 100),
        };

        if dpi_x > 1 {
            if self.resolution_unit == WX_IMAGE_RESOLUTION_CM {
                ki_round(dpi_x as f64 * 2.54)
            } else {
                dpi_x as i32
            }
        } else {
            ppi
        }
    }
}

/// Header-only replacement of `wxImage::LoadFile( stream, wxBITMAP_TYPE_ANY )` for the formats
/// KiCad stores (PNG, JPEG): `None` where wx (libpng / libjpeg-turbo) fails while reading the
/// header. Pixel data is not decoded, so a file with a valid header but corrupt image data is
/// accepted here and rejected by C++; other formats wx can read (BMP, GIF, TIFF, ...) are not
/// recognised (`PORT_STATUS.md`).
pub fn probe_image_header(data: &[u8]) -> Option<ImageHeader> {
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        probe_png(data)
    } else if data.starts_with(&[0xFF, 0xD8]) {
        probe_jpeg(data)
    } else {
        None
    }
}

/// CRC-32 (ISO 3309, as PNG chunks use it).
fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;

    for &b in bytes {
        crc ^= u32::from(b);

        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }

    !crc
}

/// libpng's `png_read_info` as far as wx reads it: IHDR (with `png_check_IHDR` and the
/// default user limits of 1000000 pixels), the chunks before the first IDAT, `pHYs`
/// (`png_handle_pHYs`: a wrong length, CRC error, duplicate or late chunk is a benign error
/// and ignored), PLTE required for palette images, unknown critical chunks rejected.
fn probe_png(data: &[u8]) -> Option<ImageHeader> {
    const PNG_UINT_31_MAX: u32 = 0x7FFF_FFFF;
    const PNG_USER_LIMIT: u32 = 1_000_000;

    let be32 = |p: &[u8]| u32::from_be_bytes([p[0], p[1], p[2], p[3]]);

    let mut pos = 8usize;
    let mut header: Option<(u32, u32, u8)> = None;
    let mut phys: Option<(u32, u8)> = None;
    let mut have_plte = false;

    // The image as wx reports it once IHDR was read.
    let finish = |header: (u32, u32, u8), phys: Option<(u32, u8)>| {
        let (resolution_x, resolution_unit) = match phys {
            None => (ImageResolution::None, 0),
            // PNG_RESOLUTION_METER
            Some((x, 1)) => (ImageResolution::PngPerMetre(x), WX_IMAGE_RESOLUTION_CM),
            // PNG_RESOLUTION_UNKNOWN and (with a warning) any other unit
            Some((x, _)) => (ImageResolution::Int(x as i32), 0),
        };

        ImageHeader {
            width: header.0 as i32,
            height: header.1 as i32,
            resolution_x,
            resolution_unit,
        }
    };

    loop {
        // Data that ends after a valid IHDR is accepted by the reference build (the oracle's
        // `png_trunc` case); before IHDR it is not an image.
        let Some(chunk_head) = data.get(pos..pos + 8) else {
            return header.map(|h| finish(h, phys));
        };

        let length = be32(chunk_head);

        if length > PNG_UINT_31_MAX {
            return None;
        }

        let chunk_type: [u8; 4] = chunk_head[4..8].try_into().ok()?;
        let body_end = (pos + 8).checked_add(length as usize)?;

        let (Some(body), Some(crc_bytes)) = (
            data.get(pos + 8..body_end),
            data.get(body_end..body_end + 4),
        ) else {
            return header.map(|h| finish(h, phys));
        };

        let crc_ok = png_crc32(&data[pos + 4..body_end]) == be32(crc_bytes);
        let critical = chunk_type[0] & 0x20 == 0;

        match &chunk_type {
            b"IHDR" => {
                if header.is_some() || length != 13 || !crc_ok {
                    return None;
                }

                let width = be32(&body[0..4]);
                let height = be32(&body[4..8]);
                let bit_depth = body[8];
                let color_type = body[9];

                if width == 0
                    || height == 0
                    || width > PNG_UINT_31_MAX
                    || height > PNG_UINT_31_MAX
                    || width > PNG_USER_LIMIT
                    || height > PNG_USER_LIMIT
                {
                    return None;
                }

                let depth_ok = match color_type {
                    0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                    _ => false,
                };

                // compression method, filter method, interlace method
                if !depth_ok || body[10] != 0 || body[11] != 0 || body[12] > 1 {
                    return None;
                }

                header = Some((width, height, color_type));
            }

            _ if header.is_none() => return None, // "Missing IHDR before ..."

            b"PLTE" => {
                if !crc_ok {
                    return None;
                }

                have_plte = true;
            }

            b"pHYs" => {
                if length == 9 && crc_ok && phys.is_none() {
                    phys = Some((be32(&body[0..4]), body[8]));
                }
            }

            b"IDAT" => {
                let h = header?;

                if h.2 == 3 && !have_plte {
                    return None; // "Missing PLTE before IDAT"
                }

                return Some(finish(h, phys));
            }

            b"IEND" => return None,

            _ => {
                if critical {
                    return None; // unknown critical chunk
                }
            }
        }

        pos = body_end + 4;
    }
}

/// libjpeg-turbo's `jpeg_read_header` (`read_markers`) as wx 3.2 uses it: SOI, the markers up
/// to SOS, the frame size of the (single) SOF marker and the density of the JFIF APP0 marker
/// (`examine_app0`; wx sets the resolution options only if `saw_JFIF_marker`).
fn probe_jpeg(data: &[u8]) -> Option<ImageHeader> {
    let be16 = |p: &[u8]| u16::from_be_bytes([p[0], p[1]]);

    let mut pos = 2usize;
    let mut size: Option<(u16, u16)> = None;
    let mut jfif: Option<(u8, u16)> = None;

    loop {
        // next_marker: skip any non-FF bytes, then FF fill bytes
        while *data.get(pos)? != 0xFF {
            pos += 1;
        }

        while *data.get(pos)? == 0xFF {
            pos += 1;
        }

        let marker = *data.get(pos)?;
        pos += 1;

        match marker {
            // RSTn, TEM: no parameters
            0xD0..=0xD7 | 0x01 => continue,
            0xD8 => return None, // SOI again: JERR_SOI_DUPLICATE
            0xD9 => return None, // EOI before SOS: no image (JERR_NO_IMAGE)
            _ => {}
        }

        let length = be16(data.get(pos..pos + 2)?) as usize;

        if length < 2 {
            return None; // JERR_BAD_LENGTH
        }

        let body = data.get(pos + 2..pos + length);

        match marker {
            // SOF0, SOF1, SOF2, SOF9, SOF10: supported by libjpeg-turbo 2.1 (arithmetic
            // decoding included); the other SOF types are JERR_SOF_UNSUPPORTED
            0xC0 | 0xC1 | 0xC2 | 0xC9 | 0xCA => {
                if size.is_some() {
                    return None; // JERR_SOF_DUPLICATE
                }

                let body = body?;

                if body.len() < 6 {
                    return None;
                }

                let height = be16(&body[1..3]);
                let width = be16(&body[3..5]);
                let components = body[5];

                if height == 0 || width == 0 || components == 0 {
                    return None; // JERR_EMPTY_IMAGE
                }

                size = Some((width, height));
            }

            0xC3 | 0xC5..=0xC7 | 0xCB | 0xCD..=0xCF => return None,

            // APP0
            0xE0 => {
                // get_interesting_appn reads min( length - 2, 14 ) bytes
                let datalen = (length - 2).min(14);
                let app = data.get(pos + 2..pos + 2 + datalen)?;

                if datalen >= 14 && &app[0..5] == b"JFIF\0" {
                    jfif = Some((app[7], be16(&app[8..10])));
                }
            }

            // SOS
            0xDA => {
                let (width, height) = size?; // JERR_SOS_NO_SOF

                let (resolution_x, resolution_unit) = match jfif {
                    Some((unit, x_density)) => {
                        (ImageResolution::Int(i32::from(x_density)), i32::from(unit))
                    }
                    None => (ImageResolution::None, 0),
                };

                return Some(ImageHeader {
                    width: i32::from(width),
                    height: i32::from(height),
                    resolution_x,
                    resolution_unit,
                });
            }

            _ => {}
        }

        pos += length;
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
        let i = PcbReferenceImage::new(&mut kiid_generator, PcbLayerId::DwgsUser);
        assert_eq!(i.base.layer, PcbLayerId::DwgsUser);
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

    // ---- M24: ports of the C++ tests and the differential oracle ---------------------------

    /// `png_3780_ppm` of qa/tests/pcbnew/test_reference_image_load.cpp and
    /// qa/tests/common/test_bitmap_base.cpp (`PPIFromNonIntegerPixelsPerCm`).
    const PNG_3780_PPM: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x09, 0x70, 0x48, 0x59, 0x73, 0x00, 0x00, 0x0E, 0xC4,
        0x00, 0x00, 0x0E, 0xC4, 0x01, 0x95, 0x2B, 0x0E, 0x1B, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44,
        0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xF7,
        0x03, 0x41, 0x43, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// `png_data_4tile` of qa/tests/common/test_bitmap_base.cpp (8x8, pHYs 3780 ppm).
    const PNG_4TILE: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x06, 0x00, 0x00, 0x00, 0xC4,
        0x0F, 0xBE, 0x8B, 0x00, 0x00, 0x00, 0x04, 0x73, 0x42, 0x49, 0x54, 0x08, 0x08, 0x08, 0x08,
        0x7C, 0x08, 0x64, 0x88, 0x00, 0x00, 0x00, 0x09, 0x70, 0x48, 0x59, 0x73, 0x00, 0x00, 0x0E,
        0xC4, 0x00, 0x00, 0x0E, 0xC4, 0x01, 0x95, 0x2B, 0x0E, 0x1B, 0x00, 0x00, 0x00, 0x19, 0x74,
        0x45, 0x58, 0x74, 0x53, 0x6F, 0x66, 0x74, 0x77, 0x61, 0x72, 0x65, 0x00, 0x77, 0x77, 0x77,
        0x2E, 0x69, 0x6E, 0x6B, 0x73, 0x63, 0x61, 0x70, 0x65, 0x2E, 0x6F, 0x72, 0x67, 0x9B, 0xEE,
        0x3C, 0x1A, 0x00, 0x00, 0x00, 0x40, 0x49, 0x44, 0x41, 0x54, 0x18, 0x95, 0xA5, 0xCD, 0x4B,
        0x0A, 0xC0, 0x20, 0x14, 0x43, 0xD1, 0x63, 0x77, 0xA7, 0xDD, 0xFF, 0x06, 0xD4, 0x7D, 0x3C,
        0x07, 0x95, 0x42, 0x3F, 0xE0, 0xC0, 0x84, 0x0C, 0x02, 0xE1, 0x86, 0x78, 0x99, 0x13, 0x1D,
        0x0D, 0xC5, 0xCF, 0xA0, 0x21, 0x66, 0xEA, 0x61, 0xA9, 0x2F, 0xA1, 0x4C, 0x4A, 0x45, 0x4E,
        0x71, 0xA1, 0x6E, 0xA5, 0x67, 0xB5, 0xBC, 0xD8, 0x1F, 0x0C, 0xA7, 0xFD, 0x23, 0x67, 0x70,
        0xDA, 0x89, 0xA2, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// qa/tests/common/test_bitmap_base.cpp `Empty` and `PPIFromNonIntegerPixelsPerCm`,
    /// `BasicProps` (size and PPI; the test's pixel size override of 2.0 IU is a BITMAP_BASE
    /// setter that REFERENCE_IMAGE never calls, so the scaled size is checked with the derived
    /// pixel size).
    #[test]
    fn bitmap_base_ppi() {
        let i = PcbReferenceImage::default();
        assert!(!i.has_image());
        assert_eq!(i.size(), Vector2I::new(0, 0));
        assert_eq!(i.legacy_ppi(), 300);

        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(PNG_3780_PPM));
        // 3780 PPM should give 96 PPI, not 94 (the old truncation result)
        assert_eq!(i.ppi(), 96);
        assert_eq!(i.legacy_ppi(), 94);

        let mut t = PcbReferenceImage::default();
        assert!(t.read_image_file(PNG_4TILE));
        assert_eq!(t.ppi(), 96);
        assert_eq!(t.size_pixels(), Vector2I::new(8, 8));
        assert_eq!(t.image_data(), PNG_4TILE.to_vec());
    }

    /// qa/tests/pcbnew/test_reference_image_load.cpp `ReferenceImagePpiScaleMigration`, the
    /// model part: the parser's migration multiplies the scale by GetPPI() / GetLegacyPPI().
    #[test]
    fn reference_image_ppi_scale_migration() {
        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(PNG_3780_PPM));
        assert_eq!(i.ppi(), 96);
        assert_eq!(i.legacy_ppi(), 94);
        i.set_image_scale(2.0);
        let legacy = i.legacy_ppi();
        i.set_image_scale(i.image_scale() * i.ppi() as f64 / legacy as f64);
        assert!((i.image_scale() - 2.0 * 96.0 / 94.0).abs() < 1e-9);
    }

    /// qa/tests/pcbnew/test_reference_image_load.cpp `ReferenceImageRotateTransformOrigin`.
    #[test]
    fn reference_image_rotate_transform_origin() {
        let env = MutEnv::detached();
        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(PNG_3780_PPM));
        i.set_position(&env, Vector2I::new(1000, 2000));
        i.set_transform_origin_offset(Vector2I::new(300, -100));
        let pos = i.pos;

        // Rotating about the image position leaves the position unchanged; the offset must
        // follow the content.
        i.rotate(&env, pos, kicad_geom::ANGLE_90);
        assert_eq!(i.pos, pos);
        assert_eq!(i.transform_origin_offset(), Vector2I::new(-100, -300));

        // Now mirror it and make sure that the transform origin follows the content
        i.mirror(&env, pos, FlipDirection::LeftRight);
        assert_eq!(i.pos, pos);
        assert_eq!(i.transform_origin_offset(), Vector2I::new(100, -300));
    }

    /// qa/tests/pcbnew/test_reference_image_load.cpp `ReferenceImageStoresRotation` with a
    /// 42x24 PNG (the C++ test builds the raster with wxImage; the header is what matters).
    #[test]
    fn reference_image_stores_rotation() {
        let env = MutEnv::detached();
        let mut png = PNG_3780_PPM.to_vec();
        png[16..20].copy_from_slice(&42u32.to_be_bytes());
        png[20..24].copy_from_slice(&24u32.to_be_bytes());
        let crc = png_crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());

        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(&png));
        i.set_position(&env, Vector2I::new(1000, 2000));
        let pos = i.pos;
        let size = i.size();
        assert_ne!(size.x, size.y);

        i.rotate(&env, pos, kicad_geom::ANGLE_45);
        assert_eq!(i.orientation(), kicad_geom::ANGLE_45);
        assert_eq!(i.pos, pos);
        assert_eq!(i.size(), size);

        // The axis-aligned box of a rotated rectangle spans |w cos| + |h sin|.
        let expected = (size.x + size.y) as f64 * std::f64::consts::FRAC_1_SQRT_2;
        let bbox = i.reference_image_bounding_box();
        assert!((bbox.width() as f64 - expected).abs() / expected < 1e-5);
        assert!((bbox.height() as f64 - expected).abs() / expected < 1e-5);

        // A quarter turn is stored too, rather than messing with pixel data.
        i.rotate(&env, pos, kicad_geom::ANGLE_45);
        assert_eq!(i.orientation(), kicad_geom::ANGLE_90);
        assert_eq!(i.size(), size);
    }

    /// qa/tests/pcbnew/test_reference_image_load.cpp `ReferenceImageFlipLayer` (on a fixture
    /// image instead of the loaded board: the parser is another package).
    #[test]
    fn reference_image_flip_layer() {
        let env = MutEnv::detached();
        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(PNG_3780_PPM));
        i.set_position(&env, Vector2I::new(100_000_000, 46_000_000));
        let orig_layer = i.base.layer;
        let orig_pos = i.pos;
        let flipped = env.layers.flip_layer(orig_layer);
        assert_ne!(flipped, orig_layer);

        for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
            i.flip(&env, orig_pos, dir);
            assert_eq!(i.base.layer, flipped);
            assert_eq!(i.pos, orig_pos);
            i.flip(&env, orig_pos, dir);
            assert_eq!(i.base.layer, orig_layer);
            assert_eq!(i.pos, orig_pos);
        }
    }

    /// `GetEffectiveShape` / `HitTest` of `PCB_REFERENCE_IMAGE`: the bounding box.
    #[test]
    fn reference_image_shape_and_hit_test() {
        let fx = rotated_flipped_scaled();
        let ctx = BoardCtx::bare(&fx.board);
        let env = MutEnv::detached();
        let mut i = PcbReferenceImage::default();
        assert!(i.read_image_file(PNG_4TILE));
        i.set_position(&env, Vector2I::new(1000, 1000));
        let bbox = i.bounding_box(&ctx);
        assert_eq!(bbox, Box2I::by_center(Vector2I::new(1000, 1000), i.size()));
        let shape = i.effective_shape(
            &ctx,
            PcbLayerId::FCu,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        assert_eq!(shape.bbox(0), bbox);
        assert!(i.hit_test(&ctx, Vector2I::new(1000, 1000), 0));
        assert!(!i.hit_test(&ctx, bbox.end() + Vector2I::new(10, 10), 0));
        assert!(i.hit_test(&ctx, bbox.end() + Vector2I::new(10, 10), 10));
        assert!(i.hit_test_rect(&ctx, &bbox, true, 0));
    }

    // ---- differential oracle (tests/oracle/m24_oracle.cpp, golden tests/golden/m24_images.txt)

    fn bits(v: f64) -> String {
        format!("{:016x}", v.to_bits())
    }

    fn state(out: &mut Vec<String>, l: &str, k: &str, r: &PcbReferenceImage) {
        let bb = r.reference_image_bounding_box();
        out.push(format!(
            "img|{l}|{k}|pos={},{} off={},{} scale={} orient={} size={},{} bbox={},{},{},{}",
            r.pos.x,
            r.pos.y,
            r.transform_origin_offset.x,
            r.transform_origin_offset.y,
            bits(r.image_scale()),
            bits(r.orientation().as_degrees()),
            r.size().x,
            r.size().y,
            bb.x(),
            bb.y(),
            bb.width(),
            bb.height()
        ));
    }

    fn loaded(data: &[u8]) -> PcbReferenceImage {
        let mut r = PcbReferenceImage::default();
        r.read_image_file(data);
        r
    }

    /// The Rust side of `runImage` in m24_oracle.cpp.
    fn run_image(out: &mut Vec<String>, l: &str, data: &[u8]) {
        let env = MutEnv::detached();
        let deg = EdaAngle::from_degrees;

        {
            let mut r = PcbReferenceImage::default();
            let ok = r.read_image_file(data);
            out.push(format!(
                "img|{l}|read|ok={} px={},{} ppi={} legacy={} pxiu={}",
                i32::from(ok),
                r.size_pixels().x,
                r.size_pixels().y,
                r.ppi(),
                r.legacy_ppi(),
                bits(r.pixel_size_iu())
            ));
            state(out, l, "loaded", &r);

            if !ok {
                return;
            }
        }

        // S1: scales about a transform origin
        {
            let mut r = loaded(data);
            r.set_position(&env, Vector2I::new(1000, 2000));
            r.set_transform_origin_offset(Vector2I::new(300, -100));

            for (i, s) in [0.159863, 2.0, 0.5, 1e-9, 1e9, -1.0, 0.0, 3.25, 1e4]
                .into_iter()
                .enumerate()
            {
                r.set_image_scale(s);
                state(out, l, &format!("scale{i}"), &r);
            }

            r.set_width(123456);
            state(out, l, "width", &r);
            r.set_height(654321);
            state(out, l, "height", &r);
            r.set_width(0);
            state(out, l, "width0", &r);
        }

        // S2: rotations
        {
            let mut r = loaded(data);
            r.set_image_scale(2.5);
            r.set_position(&env, Vector2I::new(-3000, 1500));
            r.set_transform_origin_offset(Vector2I::new(250, 75));

            for (i, a) in [30.0, 45.0, 90.0, 137.5, -10.0, 270.0, 0.1]
                .into_iter()
                .enumerate()
            {
                r.rotate(&env, Vector2I::new(500, 700), deg(a));
                state(out, l, &format!("rot{i}"), &r);
            }
        }

        // S3: flips and mirrors
        {
            let mut r = loaded(data);
            r.set_position(&env, Vector2I::new(1234, -5678));
            r.set_transform_origin_offset(Vector2I::new(11, 22));
            r.set_orientation(deg(30.0));
            let ok = r.reference_image_flip(Vector2I::new(10, 20), FlipDirection::LeftRight);
            out.push(format!("img|{l}|flip0|{}", i32::from(ok)));
            state(out, l, "flip0s", &r);
            let ok = r.reference_image_flip(Vector2I::new(10, 20), FlipDirection::TopBottom);
            out.push(format!("img|{l}|flip1|{}", i32::from(ok)));
            state(out, l, "flip1s", &r);
            let ok = r.mirror_seg(&env, &Seg::new(Vector2I::new(0, 0), Vector2I::new(100, 37)));
            out.push(format!("img|{l}|mirror0|{}", i32::from(ok)));
            state(out, l, "mirror0s", &r);
            let ok = r.mirror_seg(&env, &Seg::new(Vector2I::new(5, 5), Vector2I::new(5, 105)));
            out.push(format!("img|{l}|mirror1|{}", i32::from(ok)));
            state(out, l, "mirror1s", &r);
        }

        // S4: overflow checks
        {
            let mut r = loaded(data);
            r.set_image_scale(40.0);
            r.set_position(&env, Vector2I::new(2147483000, 0));
            state(out, l, "edge0", &r);
            r.set_position(&env, Vector2I::new(2140000000, -2140000000));
            state(out, l, "edge1", &r);
            let ok =
                r.reference_image_flip(Vector2I::new(-2140000000, 0), FlipDirection::LeftRight);
            out.push(format!("img|{l}|edgeflip|{}", i32::from(ok)));
            state(out, l, "edge2", &r);
            r.set_image_scale(1e6);
            state(out, l, "edge3", &r);
        }
    }

    /// The crude `(image .. (data ..))` scan of m24_oracle.cpp `corpusImages`.
    fn corpus_images(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut pos = 0;

        while let Some(found) = text[pos..].find("(image") {
            pos += found + 6;

            if !text[pos..].starts_with(|c: char| c.is_ascii_whitespace()) {
                continue;
            }

            let d = pos + text[pos..].find("(data").unwrap();
            let e = d + text[d..].find(')').unwrap();
            out.push(
                text[d + 5..e]
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace() && *c != '"')
                    .collect(),
            );
        }

        out
    }

    /// Replays the reference image section of the M24 oracle (synthetic PNG / JPEG headers
    /// and every reference image of the corpus boards): read result, PPI, legacy PPI, sizes,
    /// scale / rotate / flip / mirror maths and the overflow refusals, bit for bit.
    #[test]
    fn m24_oracle_images() {
        let golden = include_str!("../tests/golden/m24_images.txt");
        let mut out = Vec::new();
        let mut expected = Vec::new();
        let mut boards = Vec::new();

        for line in golden.lines() {
            if let Some(rest) = line.strip_prefix("payload|") {
                let (label, data) = rest.split_once('|').unwrap();
                let bytes = wx_base64_decode(data).unwrap();
                run_image(&mut out, label, &bytes);
            } else {
                expected.push(line.to_string());

                let label = line.split('|').nth(1).unwrap();

                if let Some((board, _)) = label.split_once('#')
                    && boards.last() != Some(&board.to_string())
                {
                    boards.push(board.to_string());
                }
            }
        }

        let root = kicad_qa::kicad_source_root();

        for board in &boards {
            let text = std::fs::read_to_string(root.join(board)).unwrap();

            for (n, data) in corpus_images(&text).iter().enumerate() {
                let bytes = wx_base64_decode(data).unwrap();
                run_image(&mut out, &format!("{board}#{n}"), &bytes);
            }
        }

        let mismatches: Vec<_> = expected
            .iter()
            .zip(out.iter())
            .filter(|(a, b)| a != b)
            .take(10)
            .collect();
        assert!(mismatches.is_empty(), "{mismatches:#?}");
        assert_eq!(expected.len(), out.len());
        assert!(expected.len() > 900);
    }
}
