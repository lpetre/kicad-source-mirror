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

//! Port of `common/drawing_sheet/ds_data_item.cpp` and `include/drawing_sheet/ds_data_item.h`.
//!
//! A `DS_DATA_ITEM` is a basic shape of a drawing sheet (frame references and title block):
//! line, rect, text, poly polygon or bitmap. Its coordinates are in mm, relative to one of
//! the 4 page corners. These items cannot be drawn or plotted "as is": they are converted to
//! a draw list ([`super::ds_draw_item::DsDrawItemList`]) for a given page. When building the
//! draw list coordinates are converted to draw/plot coordinates, texts are expanded if they
//! contain format symbols and items with `m_RepeatCount > 1` are created `m_RepeatCount`
//! times.
//!
//! The C++ class hierarchy (`DS_DATA_ITEM`, `DS_DATA_ITEM_POLYGONS`, `DS_DATA_ITEM_TEXT`,
//! `DS_DATA_ITEM_BITMAP`) is one struct with the common members and a [`DsDataItemKind`]
//! for the subclass members. The C++ reads the draw environment (`m_WSunits2Iu`, the page
//! corners, the default sizes) from the singleton `DS_DATA_MODEL::GetTheInstance()`; the port
//! passes it as [`DsDrawEnv`]. The draw items the C++ items own (`m_drawItems`) live in the
//! draw list; `SyncDrawItems` is [`super::ds_draw_item::DsDrawItemList::sync_draw_items`].

use kicad_core::color4d::Color4D;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT};
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::trigo::{rotate_point_xy, rotate_point_xy_d};
use kicad_geom::{Box2D, EdaAngle, Vector2D, Vector2I, ki_round};

use super::increment::StringIncrementer;

/// `TB_DEFAULT_TEXTSIZE`: default drawing sheet text size in mm.
pub const TB_DEFAULT_TEXTSIZE: f64 = 1.5;

/// `DS_MAX_REPEAT_COUNT`: the upper limit of `(repeat ..)`.
pub const DS_MAX_REPEAT_COUNT: i32 = 100;

/// Port of `enum CORNER_ANCHOR`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CornerAnchor {
    /// `RB_CORNER`: right bottom corner
    #[default]
    RbCorner,
    /// `RT_CORNER`: right top corner
    RtCorner,
    /// `LB_CORNER`: left bottom corner
    LbCorner,
    /// `LT_CORNER`: left top corner
    LtCorner,
}

/// Port of `enum PAGE_OPTION`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageOption {
    /// `ALL_PAGES`
    #[default]
    AllPages,
    /// `FIRST_PAGE_ONLY`
    FirstPageOnly,
    /// `SUBSEQUENT_PAGES`
    SubsequentPages,
}

/// Port of `DS_DATA_ITEM::DS_ITEM_TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsItemType {
    /// `DS_TEXT`
    Text,
    /// `DS_SEGMENT`
    Segment,
    /// `DS_RECT`
    Rect,
    /// `DS_POLYPOLYGON`
    PolyPolygon,
    /// `DS_BITMAP`
    Bitmap,
}

/// Port of `class POINT_COORD`: a coordinate in mm relative to a page corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointCoord {
    /// `m_Pos`
    pub pos: Vector2D,
    /// `m_Anchor`
    pub anchor: CornerAnchor,
}

/// The members of `DS_DATA_MODEL` that the data items read through
/// `DS_DATA_MODEL::GetTheInstance()` while building draw items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DsDrawEnv {
    /// `m_WSunits2Iu`: conversion factor between ws units (mm) and draw/plot units
    pub ws_units2_iu: f64,
    /// `m_RB_Corner`: coordinates of the right bottom corner (in mm)
    pub rb_corner: Vector2D,
    /// `m_LT_Corner`: coordinates of the left top corner (in mm)
    pub lt_corner: Vector2D,
    /// `m_DefaultLineWidth`: used when object line width is 0
    pub default_line_width: f64,
    /// `m_DefaultTextSize`: used when object text size is 0
    pub default_text_size: Vector2D,
    /// `m_DefaultTextThickness`: used when object text stroke width is 0
    pub default_text_thickness: f64,
    /// `m_EditMode`: in edit mode the `${...}` texts are displayed as is (drawing sheet editor)
    pub edit_mode: bool,
}

impl Default for DsDrawEnv {
    /// The values of the `DS_DATA_MODEL` constructor.
    fn default() -> Self {
        DsDrawEnv {
            ws_units2_iu: 1000.0,
            rb_corner: Vector2D::new(0.0, 0.0),
            lt_corner: Vector2D::new(0.0, 0.0),
            default_line_width: 0.0,
            default_text_size: Vector2D::new(TB_DEFAULT_TEXTSIZE, TB_DEFAULT_TEXTSIZE),
            default_text_thickness: 0.0,
            edit_mode: false,
        }
    }
}

/// The members of `DS_DATA_ITEM_POLYGONS`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DsDataItemPolygons {
    /// `m_Orient`: orientation
    pub orient: EdaAngle,
    /// `m_Corners`: corner list
    pub corners: Vec<Vector2D>,
    /// `m_polyIndexEnd`: index of the last point of each polygon
    poly_index_end: Vec<u32>,
    /// `m_minCoord`: min coord of corners, relative to m_Pos
    min_coord: Vector2D,
    /// `m_maxCoord`: max coord of corners, relative to m_Pos
    max_coord: Vector2D,
}

/// The members of `DS_DATA_ITEM_TEXT`.
#[derive(Clone, Debug, PartialEq)]
pub struct DsDataItemText {
    /// `m_TextBase`: the basic text, with format symbols
    pub text_base: String,
    /// `m_FullText`: the expanded text, shown on screen
    pub full_text: String,
    /// `m_Orient`: orientation in degrees
    pub orient: f64,
    /// `m_Hjustify`
    pub hjustify: GrTextHAlignT,
    /// `m_Vjustify`
    pub vjustify: GrTextVAlignT,
    /// `m_Italic`
    pub italic: bool,
    /// `m_Bold`
    pub bold: bool,
    /// `m_Font`: the face name given in `(font (face ..))`, `None` for the default (stroke)
    /// font. The C++ resolves it with `KIFONT::FONT::GetFont`; outline fonts are not part of
    /// the port and fall back to the stroke font (see `PORT_STATUS.md`).
    pub font: Option<String>,
    /// `m_TextSize`
    pub text_size: Vector2D,
    /// `m_TextColor`
    pub text_color: Color4D,
    /// `m_BoundingBoxSize`: when not null, the max size of the full text (`maxlen`,
    /// `maxheight`)
    pub bounding_box_size: Vector2D,
    /// `m_ConstrainedTextSize`: actual text size, if constrained by the bounding box
    pub constrained_text_size: Vector2D,
}

/// What `BITMAP_BASE::SetImage` / `updatePPI` read from an image: the size in pixels and the
/// resolution (supplied by an [`ImageProbe`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// `wxImage::GetWidth()`
    pub width: i32,
    /// `wxImage::GetHeight()`
    pub height: i32,
    /// `BITMAP_BASE::updatePPI()` result for a current value of 300: the image resolution in
    /// PPI, 300 when the image has none.
    pub ppi: i32,
}

/// Replacement of `wxImage::LoadFile` for the bitmaps of a drawing sheet: reads the header of
/// an image file. `None` when wx would not load it. The port has no image decoder; hosts that
/// have one (e.g. the board crate's PNG / JPEG header probe) pass it to the parser.
pub type ImageProbe = fn(&[u8]) -> Option<ImageInfo>;

/// The members of `DS_DATA_ITEM_BITMAP` and of its `BITMAP_BASE` (bitmaps are kept as data:
/// the image bytes and what the probe read from them).
#[derive(Clone, Debug, PartialEq)]
pub struct DsDataItemBitmap {
    /// `BITMAP_BASE::m_imageData`: the image file bytes (decoded base64 / legacy hex data)
    pub image_data: Vec<u8>,
    /// `BITMAP_BASE::m_originalImage != nullptr`: the image was loaded
    pub has_image: bool,
    /// The size in pixels, when known (`None`: no probe was given, see [`ImageProbe`])
    pub image_info: Option<ImageInfo>,
    /// `BITMAP_BASE::m_scale`
    pub scale: f64,
    /// `BITMAP_BASE::m_ppi`
    pub ppi: i32,
    /// `BITMAP_BASE::m_pixelSizeIu`
    pub pixel_size_iu: f64,
}

impl Default for DsDataItemBitmap {
    /// `new BITMAP_BASE`: scale 1, 300 PPI, `m_pixelSizeIu = 254000.0 / m_ppi`.
    fn default() -> Self {
        DsDataItemBitmap {
            image_data: Vec::new(),
            has_image: false,
            image_info: None,
            scale: 1.0,
            ppi: 300,
            pixel_size_iu: 254000.0 / 300.0,
        }
    }
}

impl DsDataItemBitmap {
    /// `BITMAP_BASE::GetScalingFactor()`: `m_pixelSizeIu * m_scale`.
    pub fn scaling_factor(&self) -> f64 {
        self.pixel_size_iu * self.scale
    }

    /// `BITMAP_BASE::GetPPI()`.
    pub fn bitmap_ppi(&self) -> i32 {
        self.ppi
    }
}

/// The subclass of a data item.
#[derive(Clone, Debug, PartialEq)]
pub enum DsDataItemKind {
    /// A plain `DS_DATA_ITEM` (line or rect).
    Graphic,
    /// `DS_DATA_ITEM_POLYGONS`
    Polygons(DsDataItemPolygons),
    /// `DS_DATA_ITEM_TEXT`
    Text(DsDataItemText),
    /// `DS_DATA_ITEM_BITMAP`
    Bitmap(DsDataItemBitmap),
}

/// Port of `class DS_DATA_ITEM` (and its subclasses, see [`DsDataItemKind`]).
#[derive(Clone, Debug, PartialEq)]
pub struct DsDataItem {
    /// `m_Name`: a name used in drawing sheet editor to identify items
    pub name: String,
    /// `m_Info`: a comment, only useful in drawing sheet editor
    pub info: String,
    /// `m_Pos`
    pub pos: PointCoord,
    /// `m_End`
    pub end: PointCoord,
    /// `m_LineWidth`
    pub line_width: f64,
    /// `m_RepeatCount`: repeat count for duplicate items
    pub repeat_count: i32,
    /// `m_IncrementVector`: for duplicate items: move vector for position increment
    pub increment_vector: Vector2D,
    /// `m_IncrementLabel`
    pub increment_label: i32,
    /// `m_type`
    type_: DsItemType,
    /// `m_pageOption`
    page_option: PageOption,
    /// The subclass members.
    pub kind: DsDataItemKind,
}

impl DsDataItem {
    /// Port of `DS_DATA_ITEM::DS_DATA_ITEM( DS_ITEM_TYPE aType )` for a line or a rect.
    pub fn new(a_type: DsItemType) -> Self {
        DsDataItem {
            name: String::new(),
            info: String::new(),
            pos: PointCoord::default(),
            end: PointCoord::default(),
            line_width: 0.0,
            repeat_count: 1,
            increment_vector: Vector2D::new(0.0, 0.0),
            increment_label: 1,
            type_: a_type,
            page_option: PageOption::AllPages,
            kind: DsDataItemKind::Graphic,
        }
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::DS_DATA_ITEM_POLYGONS()`.
    pub fn new_polygons() -> Self {
        let mut item = Self::new(DsItemType::PolyPolygon);
        item.kind = DsDataItemKind::Polygons(DsDataItemPolygons::default());
        item
    }

    /// Port of `DS_DATA_ITEM_TEXT::DS_DATA_ITEM_TEXT( const wxString& aTextBase )`.
    pub fn new_text(a_text_base: &str) -> Self {
        let mut item = Self::new(DsItemType::Text);
        item.increment_label = 1;
        item.line_width = 0.0; // 0 means use default value
        item.kind = DsDataItemKind::Text(DsDataItemText {
            text_base: a_text_base.to_string(),
            full_text: String::new(),
            orient: 0.0,
            hjustify: GrTextHAlignT::Left,
            vjustify: GrTextVAlignT::Center,
            italic: false,
            bold: false,
            font: None,
            text_size: Vector2D::new(0.0, 0.0),
            text_color: Color4D::UNSPECIFIED,
            bounding_box_size: Vector2D::new(0.0, 0.0),
            constrained_text_size: Vector2D::new(0.0, 0.0),
        });
        item
    }

    /// Port of `DS_DATA_ITEM_BITMAP::DS_DATA_ITEM_BITMAP( BITMAP_BASE* aImage )`.
    pub fn new_bitmap(a_image: DsDataItemBitmap) -> Self {
        let mut item = Self::new(DsItemType::Bitmap);
        item.kind = DsDataItemKind::Bitmap(a_image);
        item
    }

    /// Port of `DS_DATA_ITEM::GetType`.
    pub fn type_(&self) -> DsItemType {
        self.type_
    }

    /// Port of `DS_DATA_ITEM::GetPage1Option`.
    pub fn page1_option(&self) -> PageOption {
        self.page_option
    }

    /// Port of `DS_DATA_ITEM::SetPage1Option`.
    pub fn set_page1_option(&mut self, a_choice: PageOption) {
        self.page_option = a_choice;
    }

    /// Port of `DS_DATA_ITEM::SetStart`.
    pub fn set_start(&mut self, a_posx: f64, a_posy: f64, a_anchor: CornerAnchor) {
        self.pos.pos.x = a_posx;
        self.pos.pos.y = a_posy;
        self.pos.anchor = a_anchor;
    }

    /// Port of `DS_DATA_ITEM::SetEnd`.
    pub fn set_end(&mut self, a_posx: f64, a_posy: f64, a_anchor: CornerAnchor) {
        self.end.pos.x = a_posx;
        self.end.pos.y = a_posy;
        self.end.anchor = a_anchor;
    }

    /// The text members, if this is a `DS_DATA_ITEM_TEXT`.
    pub fn text(&self) -> Option<&DsDataItemText> {
        match &self.kind {
            DsDataItemKind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The polygon members, if this is a `DS_DATA_ITEM_POLYGONS`.
    pub fn polygons(&self) -> Option<&DsDataItemPolygons> {
        match &self.kind {
            DsDataItemKind::Polygons(p) => Some(p),
            _ => None,
        }
    }

    /// The bitmap members, if this is a `DS_DATA_ITEM_BITMAP`.
    pub fn bitmap(&self) -> Option<&DsDataItemBitmap> {
        match &self.kind {
            DsDataItemKind::Bitmap(b) => Some(b),
            _ => None,
        }
    }

    /// Port of the `GetPenSizeIU` virtuals (`DS_DATA_ITEM`, `DS_DATA_ITEM_POLYGONS`,
    /// `DS_DATA_ITEM_TEXT`).
    pub fn pen_size_iu(&self, env: &DsDrawEnv) -> i32 {
        match &self.kind {
            DsDataItemKind::Polygons(_) => ki_round(self.line_width * env.ws_units2_iu),
            DsDataItemKind::Text(_) => {
                if self.line_width != 0.0 {
                    ki_round(self.line_width * env.ws_units2_iu)
                } else {
                    ki_round(env.default_text_thickness * env.ws_units2_iu)
                }
            }
            _ => {
                if self.line_width != 0.0 {
                    ki_round(self.line_width * env.ws_units2_iu)
                } else {
                    ki_round(env.default_line_width * env.ws_units2_iu)
                }
            }
        }
    }

    /// Port of `DS_DATA_ITEM::GetStartPos( int ii )`: the position (mm) of the repeat `ii`.
    pub fn start_pos(&self, env: &DsDrawEnv, ii: i32) -> Vector2D {
        let mut pos = Vector2D::new(
            self.pos.pos.x + (self.increment_vector.x * ii as f64),
            self.pos.pos.y + (self.increment_vector.y * ii as f64),
        );

        match self.pos.anchor {
            CornerAnchor::RbCorner => {
                // right bottom corner
                pos = env.rb_corner - pos;
            }
            CornerAnchor::RtCorner => {
                // right top corner
                pos.x = env.rb_corner.x - pos.x;
                pos.y += env.lt_corner.y;
            }
            CornerAnchor::LbCorner => {
                // left bottom corner
                pos.x += env.lt_corner.x;
                pos.y = env.rb_corner.y - pos.y;
            }
            CornerAnchor::LtCorner => {
                // left top corner
                pos = env.lt_corner + pos;
            }
        }

        pos
    }

    /// Port of `DS_DATA_ITEM::GetStartPosIU( int ii )`.
    pub fn start_pos_iu(&self, env: &DsDrawEnv, ii: i32) -> Vector2I {
        let pos = self.start_pos(env, ii) * env.ws_units2_iu;
        Vector2I::new(ki_round(pos.x), ki_round(pos.y))
    }

    /// Port of `DS_DATA_ITEM::GetEndPos( int ii )`.
    pub fn end_pos(&self, env: &DsDrawEnv, ii: i32) -> Vector2D {
        let mut pos = Vector2D::new(
            self.end.pos.x + (self.increment_vector.x * ii as f64),
            self.end.pos.y + (self.increment_vector.y * ii as f64),
        );

        match self.end.anchor {
            CornerAnchor::RbCorner => {
                pos = env.rb_corner - pos;
            }
            CornerAnchor::RtCorner => {
                pos.x = env.rb_corner.x - pos.x;
                pos.y += env.lt_corner.y;
            }
            CornerAnchor::LbCorner => {
                pos.x += env.lt_corner.x;
                pos.y = env.rb_corner.y - pos.y;
            }
            CornerAnchor::LtCorner => {
                pos = env.lt_corner + pos;
            }
        }

        pos
    }

    /// Port of `DS_DATA_ITEM::GetEndPosIU( int ii )`.
    pub fn end_pos_iu(&self, env: &DsDrawEnv, ii: i32) -> Vector2I {
        let mut pos = self.end_pos(env, ii);
        pos *= env.ws_units2_iu;
        Vector2I::new(ki_round(pos.x), ki_round(pos.y))
    }

    /// Port of the `IsInsidePage( int ii )` virtuals (`DS_DATA_ITEM`,
    /// `DS_DATA_ITEM_POLYGONS`): true if the repeat `ii` is inside the page.
    pub fn is_inside_page(&self, env: &DsDrawEnv, ii: i32) -> bool {
        let mut page = Box2D::new(env.lt_corner, env.rb_corner - env.lt_corner);

        if let DsDataItemKind::Polygons(poly) = &self.kind {
            // The start of the run is a legitimate position even when it lies in the page
            // margin, so cull a repeat only where the run walks off the page
            page.merge_point(self.start_pos(env, 0) + poly.min_coord);
            page.merge_point(self.start_pos(env, 0) + poly.max_coord);

            let pos = self.start_pos(env, ii);
            return page.contains(pos + poly.min_coord) && page.contains(pos + poly.max_coord);
        }

        // The start of the run is a legitimate position even when it lies in the page
        // margin, so cull a repeat only where the run walks off the page
        page.merge_point(self.start_pos(env, 0));

        // Text and bitmap have no real end point, so only test it for lines and rects.
        if self.type_() == DsItemType::Segment || self.type_() == DsItemType::Rect {
            page.merge_point(self.end_pos(env, 0));

            if !page.contains(self.end_pos(env, ii)) {
                return false;
            }
        }

        page.contains(self.start_pos(env, ii))
    }

    /// Port of `DS_DATA_ITEM::MoveStartPointTo( const VECTOR2D& aPosition )`.
    pub fn move_start_point_to(&mut self, env: &DsDrawEnv, a_position: Vector2D) {
        self.pos.pos = relative_to_anchor(env, self.pos.anchor, a_position);
    }

    /// Port of `DS_DATA_ITEM::MoveEndPointTo( const VECTOR2D& aPosition )`: only items
    /// defined by 2 points (segments and rectangles) have an end point.
    pub fn move_end_point_to(&mut self, env: &DsDrawEnv, a_position: Vector2D) {
        let position = relative_to_anchor(env, self.end.anchor, a_position);

        // Modify m_End only for items having 2 coordinates
        match self.type_() {
            DsItemType::Segment | DsItemType::Rect => self.end.pos = position,
            _ => {}
        }
    }

    /// Port of `DS_DATA_ITEM::MoveTo( const VECTOR2D& aPosition )` for the data item (the
    /// draw items of the C++ are rebuilt by the next draw list).
    pub fn move_to(&mut self, env: &DsDrawEnv, a_position: Vector2D) {
        let vector = a_position - self.start_pos(env, 0);
        let endpos = vector + self.end_pos(env, 0);

        self.move_start_point_to(env, a_position);
        self.move_end_point_to(env, endpos);
    }

    /// Port of `DS_DATA_ITEM::GetClassName` (untranslated).
    pub fn class_name(&self) -> &'static str {
        match self.type_() {
            DsItemType::Text => "Text",
            DsItemType::Segment => "Line",
            DsItemType::Rect => "Rectangle",
            DsItemType::PolyPolygon => "Imported Shape",
            DsItemType::Bitmap => "Image",
        }
    }

    // ---- DS_DATA_ITEM_POLYGONS -------------------------------------------------------------

    /// Port of `DS_DATA_ITEM_POLYGONS::AppendCorner`.
    pub fn append_corner(&mut self, a_corner: Vector2D) {
        if let DsDataItemKind::Polygons(p) = &mut self.kind {
            p.corners.push(a_corner);
        }
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::CloseContour`: store the index of the last corner of
    /// the current polygon (`m_Corners.size() - 1`, an `unsigned` that wraps for an empty
    /// list).
    pub fn close_contour(&mut self) {
        if let DsDataItemKind::Polygons(p) = &mut self.kind {
            p.poly_index_end
                .push((p.corners.len() as u32).wrapping_sub(1));
        }
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::SetBoundingBox`: the bounding box of the set of
    /// polygons, relative to `m_Pos`.
    ///
    /// The C++ converts each corner (mm) to a `VECTOR2I` (clamped truncation) before the
    /// rotation, so the box is computed on whole millimetres; ported as is.
    pub fn set_bounding_box(&mut self) {
        let DsDataItemKind::Polygons(p) = &mut self.kind else {
            return;
        };

        if p.corners.is_empty() {
            p.min_coord = Vector2D::new(0.0, 0.0);
            p.max_coord = Vector2D::new(0.0, 0.0);
            return;
        }

        let to_int = |v: Vector2D| -> Vector2I {
            let min_i = i32::MIN as f64;
            let max_i = i32::MAX as f64;
            Vector2I::new(
                v.x.clamp(min_i, max_i) as i32,
                v.y.clamp(min_i, max_i) as i32,
            )
        };

        let mut pos = to_int(p.corners[0]);
        rotate_point_xy(&mut pos.x, &mut pos.y, p.orient);
        p.min_coord = Vector2D::new(pos.x as f64, pos.y as f64);
        p.max_coord = p.min_coord;

        for corner in &p.corners[1..] {
            pos = to_int(*corner);
            rotate_point_xy(&mut pos.x, &mut pos.y, p.orient);

            if p.min_coord.x > pos.x as f64 {
                p.min_coord.x = pos.x as f64;
            }

            if p.min_coord.y > pos.y as f64 {
                p.min_coord.y = pos.y as f64;
            }

            if p.max_coord.x < pos.x as f64 {
                p.max_coord.x = pos.x as f64;
            }

            if p.max_coord.y < pos.y as f64 {
                p.max_coord.y = pos.y as f64;
            }
        }
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::GetPolyCount`.
    pub fn poly_count(&self) -> i32 {
        self.polygons().map_or(0, |p| p.poly_index_end.len() as i32)
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::GetPolyIndexStart`.
    pub fn poly_index_start(&self, a_contour: u32) -> u32 {
        let Some(p) = self.polygons() else {
            return 0;
        };

        if a_contour == 0 {
            0
        } else {
            p.poly_index_end[a_contour as usize - 1].wrapping_add(1)
        }
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::GetPolyIndexEnd`.
    pub fn poly_index_end(&self, a_contour: u32) -> u32 {
        self.polygons()
            .map_or(0, |p| p.poly_index_end[a_contour as usize])
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::GetCornerPosition`: the coordinate (in mm) of the
    /// corner `a_idx` of the repeated item `a_repeat`.
    pub fn corner_position(&self, env: &DsDrawEnv, a_idx: u32, a_repeat: i32) -> Vector2D {
        let Some(p) = self.polygons() else {
            return Vector2D::new(0.0, 0.0);
        };

        let mut pos = p.corners[a_idx as usize];

        // Rotation:
        rotate_point_xy_d(&mut pos.x, &mut pos.y, p.orient);
        pos += self.start_pos(env, a_repeat);
        pos
    }

    /// Port of `DS_DATA_ITEM_POLYGONS::GetCornerPositionIU`: the coordinate (in draw/plot
    /// units, truncated) of the corner `a_idx` of the repeated item `a_repeat`.
    pub fn corner_position_iu(&self, env: &DsDrawEnv, a_idx: u32, a_repeat: i32) -> Vector2I {
        let mut pos = self.corner_position(env, a_idx, a_repeat);
        pos *= env.ws_units2_iu;
        Vector2I::new(f64_to_i32_x86(pos.x), f64_to_i32_x86(pos.y))
    }

    // ---- DS_DATA_ITEM_TEXT -----------------------------------------------------------------

    /// Port of `DS_DATA_ITEM_TEXT::IncrementLabel( int aIncr )`: build an incremented copy of
    /// `m_TextBase` into `m_FullText`. The rightmost letter or number is stepped by `a_incr`,
    /// carrying within its own type (a letter rolls z -> aa, a number counts up). If nothing
    /// can be stepped, `m_FullText` is left equal to `m_TextBase`.
    pub fn increment_label(&mut self, a_incr: i32) {
        let DsDataItemKind::Text(t) = &mut self.kind else {
            return;
        };

        let incrementer = StringIncrementer {
            skip_iosqxz: false,       // step through every letter
            alphabetic_max_index: -1, // no upper bound on label length
        };

        // Step the rightmost letter or number, carrying within its own type so a
        // letter rolls z -> aa instead of running into punctuation.
        match incrementer.increment(&t.text_base, a_incr, 0) {
            Some(stepped) => t.full_text = stepped,
            None => t.full_text = t.text_base.clone(),
        }
    }

    /// Port of `DS_DATA_ITEM_TEXT::ReplaceAntiSlashSequence`: replace the `\n` sequence by
    /// EOL and the sequence `\\` by only one `\` inside `m_FullText`. Returns true if the EOL
    /// symbol is found or is inserted (multiline text).
    pub fn replace_anti_slash_sequence(&mut self) -> bool {
        let DsDataItemKind::Text(t) = &mut self.kind else {
            return false;
        };

        let mut text: Vec<char> = t.full_text.chars().collect();
        let mut multiline = false;
        let mut ii: usize = 0;

        while ii < text.len() {
            if text[ii] == '\n' {
                multiline = true;
            } else if text[ii] == '\\' {
                ii += 1;

                if ii >= text.len() {
                    break;
                }

                if text[ii] == '\\' {
                    // a double \\ sequence is replaced by a single \ char
                    text.remove(ii);
                    ii = ii.wrapping_sub(1);
                } else if text[ii] == 'n' {
                    // Replace the "\n" sequence by a EOL char
                    multiline = true;
                    text[ii] = '\n';
                    text.remove(ii - 1);
                    ii = ii.wrapping_sub(1);
                }
            }

            ii = ii.wrapping_add(1);
        }

        t.full_text = text.into_iter().collect();
        multiline
    }
}

/// The position of `a_position` (mm, relative to the page origin) relative to the reference
/// corner `a_anchor` (the switch of `MoveStartPointTo` / `MoveEndPointTo`).
fn relative_to_anchor(env: &DsDrawEnv, a_anchor: CornerAnchor, a_position: Vector2D) -> Vector2D {
    let mut position = Vector2D::new(0.0, 0.0);

    // Calculate the position of the starting point
    // relative to the reference corner
    // aPosition is the position relative to the right top paper corner
    match a_anchor {
        CornerAnchor::RbCorner => {
            position = env.rb_corner - a_position;
        }
        CornerAnchor::RtCorner => {
            position.x = env.rb_corner.x - a_position.x;
            position.y = a_position.y - env.lt_corner.y;
        }
        CornerAnchor::LbCorner => {
            position.x = a_position.x - env.lt_corner.x;
            position.y = env.rb_corner.y - a_position.y;
        }
        CornerAnchor::LtCorner => {
            position = a_position - env.lt_corner;
        }
    }

    position
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anti_slash_sequences() {
        let mut item = DsDataItem::new_text("");

        let mut run = |s: &str| {
            if let DsDataItemKind::Text(t) = &mut item.kind {
                t.full_text = s.to_string();
            }
            let ml = item.replace_anti_slash_sequence();
            (item.text().unwrap().full_text.clone(), ml)
        };

        assert_eq!(run("a\\nb"), ("a\nb".to_string(), true));
        assert_eq!(run("a\\\\b"), ("a\\b".to_string(), false));
        assert_eq!(run("a\\"), ("a\\".to_string(), false));
        assert_eq!(run("a\nb"), ("a\nb".to_string(), true));
        assert_eq!(run("\\x"), ("\\x".to_string(), false));
    }
}
