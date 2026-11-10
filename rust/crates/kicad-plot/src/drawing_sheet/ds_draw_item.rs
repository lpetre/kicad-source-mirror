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

//! Port of `common/drawing_sheet/ds_draw_item.cpp` and `include/drawing_sheet/ds_draw_item.h`
//! (`DS_DRAW_ITEM_*`, `DS_DRAW_ITEM_LIST::BuildDrawItemsList`), of the `SyncDrawItems`
//! virtuals and `DS_DATA_ITEM_TEXT::SetConstrainedTextSize` of `ds_data_item.cpp`, and of
//! `DS_DRAW_ITEM_LIST::BuildFullText` / `GetTextVars` (`ds_painter.cpp`): the expansion of a
//! drawing sheet into draw items for one page.
//!
//! `DS_DRAW_ITEM_TEXT` derives from `EDA_TEXT`; the members of `EDA_TEXT` it uses
//! (`GetTextBox`, `GetEffectiveTextPenWidth`, `GetLinePositions`, `GetShownText`) are ported
//! here for the stroke font. GUI-only members (painting, hit tests, message panel, item
//! descriptions, `DS_DRAW_ITEM_PAGE`) are not ported.

use std::collections::BTreeMap;

use kicad_core::base_units::{EdaIuScale, PCB_IU_SCALE, UNITY_SCALE};
use kicad_core::color4d::Color4D;
use kicad_core::page_info::PageInfo;
use kicad_core::string_utils::{unescape_string, wx_string_split};
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT};
use kicad_core::text_vars::{
    ResolutionContext, TextVarOptions, TextVarResolver, expand_text_vars,
    finalize_text_var_expansion, resolve_text_vars,
};
use kicad_core::title_block::{TitleBlock, TitleBlockClock};
use kicad_font::font::ITALIC_TILT;
use kicad_font::gr_text::{
    BOLD_STROKE_MULTIPLIER, clamp_text_pen_size_vec, get_pen_size_for_bold, get_pen_size_for_normal,
};
use kicad_font::{Font, FontExt, Metrics, StrokeFont, default_font};
use kicad_geom::{
    Box2I, EdaAngle, Shape, ShapePolySet, Vector2I, ki_round, rotate_point, rotate_point_around,
};

use super::ds_data_item::{DsDataItem, DsDataItemKind, DsDrawEnv, DsItemType, PageOption};
use super::ds_data_model::{DrawingSheetFile, DsDataModel};

/// `productName` of `ds_painter.cpp`.
const PRODUCT_NAME: &str = "KiCad E.D.A.";

/// `DEFAULT_SIZE_TEXT`: default text height (in mils, i.e. 1/1000")
const DEFAULT_SIZE_TEXT: f64 = 50.0;

/// `TEXT_MIN_SIZE_MM`: minimum text size (1 micron).
const TEXT_MIN_SIZE_MM: f64 = 0.001;
/// `TEXT_MAX_SIZE_MM`: maximum text size in mm (~10 inches).
const TEXT_MAX_SIZE_MM: f64 = 250.0;

/// Port of `DS_DRAW_ITEM_BASE::m_peer` / `m_index`: the data item (index in the model's
/// list) a draw item comes from and the repeat index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsDrawItemPeer {
    /// Index of the `DS_DATA_ITEM` in `DS_DATA_MODEL::GetItems()`.
    pub peer: usize,
    /// `m_index`: the repeat index (`GetIndexInPeer`).
    pub index_in_peer: i32,
    /// `m_peer->GetPage1Option()`.
    pub page_option: PageOption,
}

/// Port of `DS_DRAW_ITEM_LINE` and `DS_DRAW_ITEM_RECT` (both `DS_DRAW_ITEM_BASE` with a start,
/// an end and a pen width).
#[derive(Clone, Debug, PartialEq)]
pub struct DsDrawItemSegment {
    /// `m_start`
    pub start: Vector2I,
    /// `m_end`
    pub end: Vector2I,
    /// `m_penWidth`
    pub pen_width: i32,
}

impl DsDrawItemSegment {
    /// Port of `DS_DRAW_ITEM_BASE::GetPenWidth`: `m_penWidth`, or 1 when it is not positive.
    pub fn get_pen_width(&self) -> i32 {
        if self.pen_width > 0 {
            self.pen_width
        } else {
            1
        }
    }

    /// Port of `DS_DRAW_ITEM_LINE::GetBoundingBox` / `DS_DRAW_ITEM_RECT::GetBoundingBox`.
    pub fn bounding_box(&self) -> Box2I {
        Box2I::new(self.start, self.end - self.start)
    }
}

/// Port of `DS_DRAW_ITEM_POLYPOLYGONS`.
#[derive(Clone, Debug)]
pub struct DsDrawItemPolyPolygons {
    /// `m_pos`: the anchor point of the shape
    pub pos: Vector2I,
    /// `m_penWidth`
    pub pen_width: i32,
    /// `m_Polygons`
    pub polygons: ShapePolySet,
}

impl DsDrawItemPolyPolygons {
    /// Port of `DS_DRAW_ITEM_BASE::GetPenWidth`: `m_penWidth`, or 1 when it is not positive.
    pub fn get_pen_width(&self) -> i32 {
        if self.pen_width > 0 {
            self.pen_width
        } else {
            1
        }
    }
}

/// Port of `DS_DRAW_ITEM_TEXT` (with the members of its `EDA_TEXT` base it uses).
#[derive(Clone, Debug, PartialEq)]
pub struct DsDrawItemText {
    /// `EDA_TEXT::m_text`
    pub text: String,
    /// `EDA_TEXT::m_shown_text`: `UnescapeString( m_text )`
    pub shown_text: String,
    /// `EDA_TEXT::m_pos`
    pub pos: Vector2I,
    /// `m_attributes.m_Size`
    pub size: Vector2I,
    /// `m_attributes.m_StrokeWidth`
    pub thickness: i32,
    /// `m_attributes.m_Font`: the face name of an outline font (`None` = default font). The
    /// port draws every drawing sheet text with the stroke font.
    pub font: Option<String>,
    /// `m_attributes.m_Italic`
    pub italic: bool,
    /// `m_attributes.m_Bold`
    pub bold: bool,
    /// `m_attributes.m_Color`
    pub color: Color4D,
    /// `m_attributes.m_Halign`
    pub h_justify: GrTextHAlignT,
    /// `m_attributes.m_Valign`
    pub v_justify: GrTextVAlignT,
    /// `m_attributes.m_Angle`
    pub angle: EdaAngle,
    /// `m_attributes.m_Multiline`
    pub multiline_allowed: bool,
}

/// `METRICS::Default()` (`DS_DRAW_ITEM_BASE::GetFontMetrics`).
fn font_metrics() -> &'static Metrics {
    Metrics::default_metrics()
}

/// The font drawing sheet texts are drawn with (`GetDrawFont`): the stroke font.
fn draw_font() -> &'static StrokeFont {
    default_font()
}

impl DsDrawItemText {
    /// Port of `DS_DRAW_ITEM_TEXT::DS_DRAW_ITEM_TEXT( const EDA_IU_SCALE& aIuScale,
    /// DS_DATA_ITEM* aPeer, int aIndex, const wxString& aText, const VECTOR2I& aPos, const
    /// VECTOR2I& aSize, int aPenWidth, KIFONT::FONT* aFont, bool aItalic, bool aBold, const
    /// KIGFX::COLOR4D& aColor )`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        a_iu_scale: &EdaIuScale,
        a_text: &str,
        a_pos: Vector2I,
        a_size: Vector2I,
        a_pen_width: i32,
        a_font: Option<String>,
        a_italic: bool,
        a_bold: bool,
        a_color: Color4D,
    ) -> Self {
        // EDA_TEXT( aIuScale, aText ): the size set there is replaced below
        let mut text = DsDrawItemText {
            text: a_text.to_string(),
            shown_text: if a_text.is_empty() {
                String::new()
            } else {
                unescape_string(a_text)
            },
            pos: Vector2I::new(0, 0),
            size: Vector2I::new(0, 0),
            thickness: 0,
            font: None,
            italic: false,
            bold: false,
            color: Color4D::UNSPECIFIED,
            h_justify: GrTextHAlignT::Center,
            v_justify: GrTextVAlignT::Center,
            angle: EdaAngle::from_degrees(0.0),
            multiline_allowed: false,
        };

        let default_size = ki_round(DEFAULT_SIZE_TEXT * a_iu_scale.iu_per_mils);
        text.set_text_size(a_iu_scale, Vector2I::new(default_size, default_size));

        text.pos = a_pos;
        text.set_text_size(a_iu_scale, a_size);
        text.thickness = a_pen_width;
        text.font = a_font;
        text.italic = a_italic;
        text.bold = a_bold;
        text.color = a_color;
        text
    }

    /// Port of `EDA_TEXT::SetTextSize( VECTOR2I aNewSize, bool aEnforceMinTextSize = true )`.
    fn set_text_size(&mut self, a_iu_scale: &EdaIuScale, a_new_size: Vector2I) {
        let mut new_size = a_new_size;

        // Plotting uses unityScale and independently scales the text.  If we clamp here we'll
        // clamp to *really* small values.
        let enforce_min_text_size = a_iu_scale.iu_per_mm != UNITY_SCALE.iu_per_mm;

        if enforce_min_text_size {
            let min = a_iu_scale.mm_to_iu(TEXT_MIN_SIZE_MM);
            let max = a_iu_scale.mm_to_iu(TEXT_MAX_SIZE_MM);

            new_size = Vector2I::new(new_size.x.clamp(min, max), new_size.y.clamp(min, max));
        }

        self.size = new_size;
    }

    /// Port of `EDA_TEXT::GetShownText( aContext )`: the unescaped text (`m_text` for
    /// `RAW_VALUE`).
    pub fn get_shown_text(&self, a_context: ResolutionContext) -> String {
        if a_context == ResolutionContext::RawValue {
            return self.text.clone();
        }

        self.shown_text.clone()
    }

    /// Port of `EDA_TEXT::GetEffectiveTextPenWidth( int aDefaultPenWidth )`.
    pub fn effective_text_pen_width(&self, a_default_pen_width: i32) -> i32 {
        let mut pen_width = self.thickness;

        if pen_width <= 1 {
            pen_width = a_default_pen_width;

            if self.bold {
                pen_width = get_pen_size_for_bold(self.size.x);
            } else if pen_width <= 1 {
                pen_width = get_pen_size_for_normal(self.size.x);
            }
        } else if self.bold {
            // isStrokeFont(): drawing sheet texts are drawn with the stroke font
            pen_width = ki_round(pen_width as f64 * BOLD_STROKE_MULTIPLIER);
        }

        // Clip pen size for small texts:
        clamp_text_pen_size_vec(pen_width, self.size, false)
    }

    /// Port of `EDA_TEXT::GetInterline( aSettings )`.
    pub fn interline(&self) -> i32 {
        // GetLineSpacing() is 1.0 for drawing sheet texts
        ki_round(draw_font().interline(self.size.y as f64, font_metrics()) * 1.0)
    }

    /// Port of `EDA_TEXT::GetTextBox( aSettings, aLine )` (`a_line` -1: all lines), i.e.
    /// `DS_DRAW_ITEM_TEXT::GetBoundingBox` for `a_line == -1`.
    pub fn text_box(&self, a_line: i32) -> Box2I {
        let draw_pos = self.pos;
        let mut bbox = Box2I::default();
        let mut strings: Vec<String> = Vec::new();
        let mut text = self.get_shown_text(ResolutionContext::ForCanvas);
        let thickness = self.effective_text_pen_width(0);

        if self.multiline_allowed {
            strings = wx_string_split(&text, '\n');

            // GetCount() == 0 for void strings with multilines allowed
            if !strings.is_empty() {
                if a_line >= 0 && a_line < strings.len() as i32 {
                    text = strings[a_line as usize].clone();
                } else {
                    text = strings[0].clone();
                }
            }
        }

        // calculate the H and V size
        let font = draw_font();
        let font_size = self.size;
        let bold = self.bold;
        let italic = self.italic;
        let mut extents =
            font.string_boundary_limits(&text, font_size, thickness, bold, italic, font_metrics());
        let mut overbar_offset = 0;

        // Creates bounding box (rectangle) for horizontal, left and top justified text. The
        // bounding box will be moved later according to the actual text options
        let mut textsize = Vector2I::new(extents.x, extents.y);
        let mut pos = draw_pos;
        let fudge_factor = ki_round(extents.y as f64 * 0.17);

        // font->IsStroke()
        textsize.y += fudge_factor;

        let interline = ki_round(font.interline(font_size.y as f64, font_metrics()) * 1.0);

        if self.multiline_allowed && a_line > 0 && a_line < strings.len() as i32 {
            pos.y -= a_line * interline;
        }

        if text.contains("~{") {
            overbar_offset = extents.y / 6;
        }

        bbox.set_origin(pos);

        // for multiline texts and aLine < 0, merge all rectangles (aLine == -1 signals all
        // lines)
        if self.multiline_allowed && a_line < 0 && strings.len() > 1 {
            for s in strings.iter().skip(1) {
                extents = font.string_boundary_limits(
                    s,
                    font_size,
                    thickness,
                    bold,
                    italic,
                    font_metrics(),
                );
                textsize.x = textsize.x.max(extents.x);
            }

            // interline spacing is only *between* lines, so total height is the height of the
            // first line plus the interline distance (with interline spacing) for all
            // subsequent lines (size_t arithmetic converted back to int)
            textsize.y = textsize
                .y
                .wrapping_add(((strings.len() - 1) as i32).wrapping_mul(interline));
        }

        textsize.y += overbar_offset;

        bbox.set_size_wh(textsize.x as i64, textsize.y as i64);

        // At this point the rectangle origin is the text origin (m_Pos).  This is correct only
        // for left and top justified, non-mirrored, non-overbarred texts. Recalculate for all
        // others.
        let italic_offset: i64 = if self.italic {
            ki_round(font_size.y as f64 * ITALIC_TILT) as i64
        } else {
            0
        };

        match self.h_justify {
            GrTextHAlignT::Left => {
                // never mirrored
            }
            GrTextHAlignT::Center => {
                bbox.set_x((bbox.x() as i64 - (bbox.width() - italic_offset) / 2) as i32);
            }
            GrTextHAlignT::Right => {
                bbox.set_x((bbox.x() as i64 - (bbox.width() - italic_offset)) as i32);
            }
            // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
            GrTextHAlignT::Indeterminate => {}
        }

        match self.v_justify {
            GrTextVAlignT::Top => {
                bbox.offset_xy(0, -fudge_factor);
            }
            GrTextVAlignT::Center => {
                bbox.set_y((bbox.y() as i64 - bbox.height() / 2) as i32);
            }
            GrTextVAlignT::Bottom => {
                bbox.set_y((bbox.y() as i64 - bbox.height()) as i32);
                bbox.offset_xy(0, fudge_factor);
            }
            GrTextVAlignT::Indeterminate => {}
        }

        bbox.normalize(); // Make h and v sizes always >= 0
        bbox
    }

    /// Port of `DS_DRAW_ITEM_TEXT::GetBoundingBox`: `EDA_TEXT::GetTextBox( nullptr )`.
    pub fn bounding_box(&self) -> Box2I {
        self.text_box(-1)
    }

    /// Port of `EDA_TEXT::GetLinePositions( aSettings, aPositions, aLineCount )`: the
    /// position of each line of a multiline text.
    pub fn line_positions(&self, a_line_count: i32) -> Vec<Vector2I> {
        let mut positions = Vec::new();

        // Position of first line of the multiline text according to the center of the
        // multiline text block
        let mut pos = self.pos;

        // Offset to next line.
        let mut offset = Vector2I::new(0, self.interline());

        if a_line_count > 1 {
            match self.v_justify {
                GrTextVAlignT::Top => {}
                GrTextVAlignT::Center => {
                    pos.y -= (a_line_count - 1) * offset.y / 2;
                }
                GrTextVAlignT::Bottom => {
                    pos.y -= (a_line_count - 1) * offset.y;
                }
                GrTextVAlignT::Indeterminate => {}
            }
        }

        // Rotate the position of the first line around the center of the multiline text block
        rotate_point_around(&mut pos, self.pos, self.angle);

        // Rotate the offset lines to increase happened in the right direction
        rotate_point(&mut offset, self.angle);

        for _ in 0..a_line_count {
            positions.push(pos);
            pos += offset;
        }

        positions
    }

    /// Port of `DS_DRAW_ITEM_TEXT::GetApproxBBox`: a really dumb over-approximation (the
    /// canvas uses it instead of the real text box).
    pub fn approx_bbox(&self) -> Box2I {
        let text = self.get_shown_text(ResolutionContext::ForCanvas);
        let mut bbox = Box2I::from_pos(self.pos);

        let len = text.chars().count() as i32;
        bbox.set_width(ki_round(len as f64 * self.size.x as f64 * 1.3) as i64);
        bbox.set_height(self.size.y as i64);

        match self.h_justify {
            GrTextHAlignT::Left => {}
            GrTextHAlignT::Center => bbox.offset_xy(-(bbox.width() as i32) / 2, 0),
            GrTextHAlignT::Right => bbox.offset_xy(-(bbox.width() as i32), 0),
            GrTextHAlignT::Indeterminate => {}
        }

        match self.v_justify {
            GrTextVAlignT::Top => {}
            GrTextVAlignT::Center => bbox.offset_xy(0, -(bbox.height() as i32) / 2),
            GrTextVAlignT::Bottom => bbox.offset_xy(0, -(bbox.height() as i32)),
            GrTextVAlignT::Indeterminate => {}
        }

        bbox.inflate_xy(self.size.x, self.size.y / 2);
        bbox
    }
}

/// Port of `DS_DRAW_ITEM_BITMAP`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DsDrawItemBitmap {
    /// `m_pos`: position of reference point
    pub pos: Vector2I,
}

/// The `DS_DRAW_ITEM_BASE` subclasses (`KICAD_T`: `WSG_LINE_T`, `WSG_RECT_T`, `WSG_POLY_T`,
/// `WSG_TEXT_T`, `WSG_BITMAP_T`).
#[derive(Clone, Debug)]
pub enum DsDrawItemShape {
    /// `DS_DRAW_ITEM_LINE`
    Line(DsDrawItemSegment),
    /// `DS_DRAW_ITEM_RECT`
    Rect(DsDrawItemSegment),
    /// `DS_DRAW_ITEM_POLYPOLYGONS`
    PolyPolygons(DsDrawItemPolyPolygons),
    /// `DS_DRAW_ITEM_TEXT`
    Text(DsDrawItemText),
    /// `DS_DRAW_ITEM_BITMAP`
    Bitmap(DsDrawItemBitmap),
}

/// One draw item of a [`DsDrawItemList`] (`DS_DRAW_ITEM_BASE*`).
#[derive(Clone, Debug)]
pub struct DsDrawItem {
    /// `m_peer`, `m_index`
    pub peer: DsDrawItemPeer,
    /// The item.
    pub shape: DsDrawItemShape,
}

impl DsDrawItem {
    /// Port of the `GetBoundingBox` overrides. A bitmap's box is that of its image size
    /// (`BITMAP_BASE::GetSize`), which needs the image of the data item.
    pub fn bounding_box(&self, model: &DsDataModel) -> Box2I {
        match &self.shape {
            DsDrawItemShape::Line(s) | DsDrawItemShape::Rect(s) => s.bounding_box(),
            DsDrawItemShape::PolyPolygons(p) => p.polygons.bbox(0),
            DsDrawItemShape::Text(t) => t.bounding_box(),
            DsDrawItemShape::Bitmap(b) => {
                let mut bbox = Box2I::default();

                if let Some(bitmap) = model.item(self.peer.peer as u32).and_then(|i| i.bitmap())
                    && let Some(info) = bitmap.image_info
                {
                    // BITMAP_BASE::GetSize(): KiROUND( pixels * GetScalingFactor() )
                    let bm_size = Vector2I::new(
                        ki_round(info.width as f64 * bitmap.scaling_factor()),
                        ki_round(info.height as f64 * bitmap.scaling_factor()),
                    );
                    bbox.set_size(bm_size);
                    bbox.set_origin(Vector2I::new(
                        b.pos.x - bm_size.x / 2,
                        b.pos.y - bm_size.y / 2,
                    ));
                }

                bbox
            }
        }
    }
}

/// Port of `class DS_DRAW_ITEM_LIST`: the draw items of a drawing sheet for one page, and the
/// context the texts are expanded with.
pub struct DsDrawItemList<'a> {
    /// `m_graphicList`: items to draw/plot
    graphic_list: Vec<DsDrawItem>,
    /// `m_iuScale`: IU scale for drawing
    iu_scale: EdaIuScale,
    /// `m_plotterMilsToIu`: IU scale for plotting
    plotter_mils_to_iu: f64,
    /// `m_penSize`: the default line width for drawings, used when an item has a pen size
    /// = 0
    pen_size: i32,
    /// `m_isFirstPage`: is this the first page or not
    is_first_page: bool,
    /// `m_sheetCount`: the number of sheets, for text variable references, in schematic
    sheet_count: i32,
    /// `m_titleBlock`: for text variable references
    title_block: Option<&'a TitleBlock>,
    /// `m_paperFormat`: for text variable references
    paper_format: String,
    /// `m_fileName`: for text variable references
    file_name: String,
    /// `m_sheetName`: for text variable references
    sheet_name: String,
    /// `m_sheetPath`: for text variable references
    sheet_path: String,
    /// `m_pageNumber`: the actual page number displayed in the title block
    page_number: String,
    /// `m_sheetLayer`: for text variable references
    sheet_layer: String,
    /// `m_variantName`: for `${VARIANT}` text variable reference
    variant_name: String,
    /// `m_variantDesc`: for `${VARIANT_DESC}` text variable reference
    variant_desc: String,
    /// `m_project`: for project-based text variable references (`PROJECT::TextVarResolver`)
    project: Option<&'a TextVarResolver<'a>>,
    /// `m_context`
    context: ResolutionContext,
    /// `m_properties`: for text variable references
    properties: Option<&'a BTreeMap<String, String>>,
    /// Not in C++: `GetBaseVersion()` when a `PGM_BASE` exists (`PgmOrNull()`), for
    /// `${KICAD_VERSION}`; `None` leaves the variable unresolved, as without a program.
    kicad_version: Option<String>,
    /// Not in C++: the clock of `TITLE_BLOCK::GetCurrentDate` / `GetCurrentTime*`.
    clock: TitleBlockClock,
    /// Not in C++: `EXPRESSION_EVALUATOR::Evaluate` for `@{...}` expressions (text_eval is
    /// not ported; `None` leaves them as they are).
    evaluator: Option<&'a dyn Fn(&str) -> String>,
}

impl<'a> DsDrawItemList<'a> {
    /// Port of `DS_DRAW_ITEM_LIST( const EDA_IU_SCALE& aIuScale, RESOLUTION_CONTEXT aContext
    /// = INTERNAL )`.
    pub fn new(a_iu_scale: EdaIuScale, a_context: ResolutionContext) -> Self {
        DsDrawItemList {
            graphic_list: Vec::new(),
            iu_scale: a_iu_scale,
            plotter_mils_to_iu: 0.0,
            pen_size: 1,
            is_first_page: true,
            sheet_count: 1,
            title_block: None,
            paper_format: String::new(),
            file_name: String::new(),
            sheet_name: String::new(),
            sheet_path: String::new(),
            page_number: "1".to_string(),
            sheet_layer: String::new(),
            variant_name: String::new(),
            variant_desc: String::new(),
            project: None,
            context: a_context,
            properties: None,
            kicad_version: None,
            clock: TitleBlockClock::default(),
            evaluator: None,
        }
    }

    /// A list with `pcbIUScale` (`DRC_TEST_PROVIDER_MISC::testTextVars`).
    pub fn new_pcb(a_context: ResolutionContext) -> Self {
        Self::new(PCB_IU_SCALE, a_context)
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetProject` (the project's `TextVarResolver`).
    pub fn set_project(&mut self, a_project: Option<&'a TextVarResolver<'a>>) {
        self.project = a_project;
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetTitleBlock`.
    pub fn set_title_block(&mut self, a_tblock: Option<&'a TitleBlock>) {
        self.title_block = a_tblock;
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetProperties`.
    pub fn set_properties(&mut self, a_props: Option<&'a BTreeMap<String, String>>) {
        self.properties = a_props;
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetPaperFormat`.
    pub fn set_paper_format(&mut self, a_format_name: &str) {
        self.paper_format = a_format_name.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetFileName`.
    pub fn set_file_name(&mut self, a_file_name: &str) {
        self.file_name = a_file_name.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetSheetName`.
    pub fn set_sheet_name(&mut self, a_sheet_name: &str) {
        self.sheet_name = a_sheet_name.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetSheetPath`.
    pub fn set_sheet_path(&mut self, a_sheet_path: &str) {
        self.sheet_path = a_sheet_path.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetSheetLayer`.
    pub fn set_sheet_layer(&mut self, a_sheet_layer: &str) {
        self.sheet_layer = a_sheet_layer.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetVariantName`.
    pub fn set_variant_name(&mut self, a_variant: &str) {
        self.variant_name = a_variant.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetVariantDesc`.
    pub fn set_variant_desc(&mut self, a_desc: &str) {
        self.variant_desc = a_desc.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetDefaultPenSize`.
    pub fn set_default_pen_size(&mut self, a_pen_size: i32) {
        self.pen_size = a_pen_size;
    }

    /// Port of `DS_DRAW_ITEM_LIST::GetDefaultPenSize`.
    pub fn default_pen_size(&self) -> i32 {
        self.pen_size
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetPlotterMilsToIUfactor`.
    pub fn set_plotter_mils_to_iu_factor(&mut self, a_mils2_iu: f64) {
        self.plotter_mils_to_iu = a_mils2_iu;
    }

    /// Port of `DS_DRAW_ITEM_LIST::GetMilsToIUfactor`.
    pub fn mils_to_iu_factor(&self) -> f64 {
        if self.plotter_mils_to_iu > 0.0 {
            self.plotter_mils_to_iu
        } else {
            self.iu_scale.iu_per_mils
        }
    }

    /// Port of `DS_DRAW_ITEM_LIST::GetIuScale`.
    pub fn iu_scale(&self) -> &EdaIuScale {
        &self.iu_scale
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetPageNumber`.
    pub fn set_page_number(&mut self, a_page_number: &str) {
        self.page_number = a_page_number.to_string();
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetIsFirstPage`.
    pub fn set_is_first_page(&mut self, a_is_first_page: bool) {
        self.is_first_page = a_is_first_page;
    }

    /// Port of `DS_DRAW_ITEM_LIST::SetSheetCount`.
    pub fn set_sheet_count(&mut self, a_sheet_count: i32) {
        self.sheet_count = a_sheet_count;
    }

    /// Not in C++: the `GetBaseVersion()` text of `${KICAD_VERSION}` (`None`: no program, the
    /// variable stays unresolved).
    pub fn set_kicad_version(&mut self, a_base_version: Option<&str>) {
        self.kicad_version = a_base_version.map(str::to_string);
    }

    /// Not in C++: the clock texts of `${CURRENT_DATE}` and the `CURRENT_TIME_*` variables.
    pub fn set_clock(&mut self, a_clock: &TitleBlockClock) {
        self.clock = a_clock.clone();
    }

    /// Not in C++: the `@{...}` expression evaluator (`EXPRESSION_EVALUATOR::Evaluate`).
    pub fn set_evaluator(&mut self, a_evaluator: Option<&'a dyn Fn(&str) -> String>) {
        self.evaluator = a_evaluator;
    }

    /// The draw items, in list order (`GetFirst` / `GetNext`).
    pub fn items(&self) -> &[DsDrawItem] {
        &self.graphic_list
    }

    /// Port of `DS_DRAW_ITEM_LIST::Append`.
    pub fn append(&mut self, a_item: DsDrawItem) {
        self.graphic_list.push(a_item);
    }

    /// Port of `DS_DRAW_ITEM_LIST::GetTextVars`: the text variables a drawing sheet resolves.
    pub fn get_text_vars(a_vars: &mut Vec<String>) {
        for v in [
            "KICAD_VERSION",
            "#",
            "##",
            "SHEETNAME",
            "SHEETPATH",
            "FILENAME",
            "FILEPATH",
            "PROJECTNAME",
            "PAPER",
            "LAYER",
            "VARIANT",
            "VARIANT_DESC",
        ] {
            a_vars.push(v.to_string());
        }

        TitleBlock::get_contextual_text_vars(a_vars);
    }

    /// Port of `DS_DRAW_ITEM_LIST::BuildDrawItemsList( const PAGE_INFO& aPageInfo, const
    /// TITLE_BLOCK& aTitleBlock )`: drawing or plotting the drawing sheet, the list is
    /// (re)built from the model for the page.
    pub fn build_draw_items_list(
        &mut self,
        model: &mut DsDataModel,
        a_page_info: &PageInfo,
        a_title_block: &'a TitleBlock,
    ) {
        self.title_block = Some(a_title_block);
        self.paper_format = a_page_info.type_as_string().to_string();

        // Build the basic layout shape, if the layout list is empty
        if model.count() == 0 && !model.void_list_allowed() {
            let _ = model.load_drawing_sheet(DrawingSheetFile::NoFileName, false);
        }

        model.setup_draw_environment(a_page_info, self.mils_to_iu_factor());

        let (env, items) = model.env_and_items_mut();
        let env = *env;

        #[allow(clippy::if_same_then_else)] // the C++ structure
        for (idx, ws_item) in items.iter_mut().enumerate() {
            // Generate it only if the page option allows this
            if ws_item.page1_option() == PageOption::FirstPageOnly && !self.is_first_page {
                continue;
            } else if ws_item.page1_option() == PageOption::SubsequentPages && self.is_first_page {
                continue;
            }

            self.sync_draw_items(&env, idx, ws_item);
        }
    }

    /// Port of the `SyncDrawItems( DS_DRAW_ITEM_LIST* aCollector, KIGFX::VIEW* aView )`
    /// virtuals of `DS_DATA_ITEM`, `DS_DATA_ITEM_POLYGONS`, `DS_DATA_ITEM_TEXT` and
    /// `DS_DATA_ITEM_BITMAP` with this list as the collector and no view: append the draw
    /// items of the data item `a_idx` (one per repeat inside the page).
    ///
    /// The C++ deletes the draw items of the previous sync (owned by the data item, also
    /// removed from the collector); a new list starts empty here.
    pub fn sync_draw_items(&mut self, env: &DsDrawEnv, a_idx: usize, a_item: &mut DsDataItem) {
        match a_item.type_() {
            DsItemType::Segment | DsItemType::Rect => self.sync_graphic(env, a_idx, a_item),
            DsItemType::PolyPolygon => self.sync_polygons(env, a_idx, a_item),
            DsItemType::Text => self.sync_text(env, a_idx, a_item),
            DsItemType::Bitmap => self.sync_bitmap(env, a_idx, a_item),
        }
    }

    fn peer(a_idx: usize, j: i32, a_item: &DsDataItem) -> DsDrawItemPeer {
        DsDrawItemPeer {
            peer: a_idx,
            index_in_peer: j,
            page_option: a_item.page1_option(),
        }
    }

    /// `DS_DATA_ITEM::SyncDrawItems`.
    fn sync_graphic(&mut self, env: &DsDrawEnv, a_idx: usize, a_item: &DsDataItem) {
        let mut pensize = a_item.pen_size_iu(env);

        if pensize == 0 {
            pensize = self.default_pen_size();
        }

        for j in 0..a_item.repeat_count {
            if j > 0 && !a_item.is_inside_page(env, j) {
                continue;
            }

            let seg = DsDrawItemSegment {
                start: a_item.start_pos_iu(env, j),
                end: a_item.end_pos_iu(env, j),
                pen_width: pensize,
            };

            let shape = if a_item.type_() == DsItemType::Segment {
                DsDrawItemShape::Line(seg)
            } else {
                DsDrawItemShape::Rect(seg)
            };

            self.append(DsDrawItem {
                peer: Self::peer(a_idx, j, a_item),
                shape,
            });
        }
    }

    /// `DS_DATA_ITEM_POLYGONS::SyncDrawItems`.
    fn sync_polygons(&mut self, env: &DsDrawEnv, a_idx: usize, a_item: &DsDataItem) {
        for j in 0..a_item.repeat_count {
            if j > 0 && !a_item.is_inside_page(env, j) {
                continue;
            }

            let pensize = a_item.pen_size_iu(env);
            let mut poly_shape = DsDrawItemPolyPolygons {
                pos: a_item.start_pos_iu(env, j),
                pen_width: pensize,
                polygons: ShapePolySet::new(),
            };

            // Transfer all outlines (basic polygons)
            let polygons = &mut poly_shape.polygons;

            for kk in 0..a_item.poly_count() {
                // Create new outline
                let mut ist = a_item.poly_index_start(kk as u32);
                let iend = a_item.poly_index_end(kk as u32);

                polygons.new_outline();

                while ist <= iend {
                    let p = a_item.corner_position_iu(env, ist, j);
                    ist += 1;
                    polygons.append(p.x, p.y, -1, -1, false);
                }
            }

            self.append(DsDrawItem {
                peer: Self::peer(a_idx, j, a_item),
                shape: DsDrawItemShape::PolyPolygons(poly_shape),
            });
        }
    }

    /// `DS_DATA_ITEM_TEXT::SyncDrawItems`.
    fn sync_text(&mut self, env: &DsDrawEnv, a_idx: usize, a_item: &mut DsDataItem) {
        let mut pensize = a_item.pen_size_iu(env);
        let mut multilines = false;

        if env.edit_mode {
            if let DsDataItemKind::Text(t) = &mut a_item.kind {
                t.full_text = t.text_base.clone();
            }
        } else {
            if let DsDataItemKind::Text(t) = &mut a_item.kind {
                t.full_text = self.build_full_text(&t.text_base);
            }

            multilines = a_item.replace_anti_slash_sequence();
        }

        let DsDataItemKind::Text(t) = &a_item.kind else {
            return;
        };

        // Feed bold worksheet text an auto width so its Bold flag reproduces the historical
        // bold pen size through the render-time multiplier; otherwise fall back to the
        // default.
        if t.bold {
            pensize = 0;
        } else if pensize == 0 {
            pensize = self.default_pen_size();
        }

        let constrained = set_constrained_text_size(env, t);

        if let DsDataItemKind::Text(t) = &mut a_item.kind {
            t.constrained_text_size = constrained;
        }

        let textsize = Vector2I::new(
            ki_round(constrained.x * env.ws_units2_iu),
            ki_round(constrained.y * env.ws_units2_iu),
        );

        for j in 0..a_item.repeat_count {
            if j > 0 && !a_item.is_inside_page(env, j) {
                continue;
            }

            let Some(t) = a_item.text() else {
                return;
            };

            let mut text = DsDrawItemText::new(
                &self.iu_scale,
                &t.full_text,
                a_item.start_pos_iu(env, j),
                textsize,
                pensize,
                t.font.clone(),
                t.italic,
                t.bold,
                t.text_color.clone(),
            );

            text.h_justify = t.hjustify;
            text.v_justify = t.vjustify;
            text.angle = EdaAngle::from_degrees(t.orient);
            text.multiline_allowed = multilines;

            self.append(DsDrawItem {
                peer: Self::peer(a_idx, j, a_item),
                shape: DsDrawItemShape::Text(text),
            });

            // Increment label for the next text (has no meaning for multiline texts)
            if a_item.repeat_count > 1 && !multilines {
                a_item.increment_label((j + 1) * a_item.increment_label);
            }
        }
    }

    /// `DS_DATA_ITEM_BITMAP::SyncDrawItems`.
    fn sync_bitmap(&mut self, env: &DsDrawEnv, a_idx: usize, a_item: &mut DsDataItem) {
        let mils_to_iu = self.mils_to_iu_factor();

        let DsDataItemKind::Bitmap(bitmap) = &mut a_item.kind else {
            return;
        };

        let pix_size_iu = mils_to_iu * 1000.0 / bitmap.bitmap_ppi() as f64;
        bitmap.pixel_size_iu = pix_size_iu;

        if !bitmap.has_image {
            return;
        }

        for j in 0..a_item.repeat_count {
            if j > 0 && !a_item.is_inside_page(env, j) {
                continue;
            }

            self.append(DsDrawItem {
                peer: Self::peer(a_idx, j, a_item),
                shape: DsDrawItemShape::Bitmap(DsDrawItemBitmap {
                    pos: a_item.start_pos_iu(env, j),
                }),
            });
        }
    }

    /// Port of `TITLE_BLOCK::TextVarResolver( aToken, aProject, aContext )` for the list's
    /// project (the clock fields read the list's clock).
    fn title_block_text_var_resolver(
        &self,
        a_title_block: &TitleBlock,
        a_token: &mut String,
    ) -> bool {
        let original_token = a_token.clone();
        let mut token_updated = false;

        match a_token.as_str() {
            "ISSUE_DATE" => {
                *a_token = a_title_block.date().to_string();
                token_updated = true;
            }
            "CURRENT_DATE" => {
                *a_token = self.clock.current_date.clone();
                token_updated = true;
            }
            "CURRENT_TIME_HH_MM_SS" => {
                *a_token = self.clock.current_time_hhmmss.clone();
                token_updated = true;
            }
            "CURRENT_TIME_LOCALE" => {
                *a_token = self.clock.current_time_locale.clone();
                token_updated = true;
            }
            "REVISION" => {
                *a_token = a_title_block.revision().to_string();
                token_updated = true;
            }
            "TITLE" => {
                *a_token = a_title_block.title().to_string();
                token_updated = true;
            }
            "COMPANY" => {
                *a_token = a_title_block.company().to_string();
                token_updated = true;
            }
            _ => {
                let chars: Vec<char> = a_token.chars().collect();

                // aToken->Left( aToken->Len() - 1 ).IsSameAs( wxT( "COMMENT" ) )
                if chars.len() == 8 && chars[..7].iter().copied().eq("COMMENT".chars()) {
                    let c = chars[7];

                    if ('1'..='9').contains(&c) {
                        *a_token = a_title_block.comment((c as u8 - b'1') as usize).to_string();
                        token_updated = true;
                    }
                }
            }
        }

        if token_updated {
            if a_token == "CURRENT_DATE" {
                *a_token = self.clock.current_date.clone();
            } else if let Some(project) = self.project {
                *a_token = expand_text_vars(a_token, Some(project), self.context);
            }

            // This is the default fallback, so don't claim we resolved it
            if *a_token == format!("${{{original_token}}}") {
                return false;
            }

            return true;
        }

        false
    }

    /// The `wsResolver` lambda of `BuildFullText`; `title_block` is `m_titleBlock` (cleared
    /// while the value of a title block field is expanded).
    fn ws_resolve(&self, token: &mut String, title_block: Option<&TitleBlock>) -> bool {
        let mut token_updated = false;

        match token.as_str() {
            "KICAD_VERSION" if self.kicad_version.is_some() => {
                *token = format!(
                    "{} {}",
                    PRODUCT_NAME,
                    self.kicad_version.as_deref().unwrap_or("")
                );
                token_updated = true;
            }
            "#" => {
                *token = self.page_number.clone();
                token_updated = true;
            }
            "##" => {
                *token = format!("{}", self.sheet_count);
                token_updated = true;
            }
            "SHEETNAME" => {
                *token = self.sheet_name.clone();
                token_updated = true;
            }
            "SHEETPATH" => {
                *token = self.sheet_path.clone();
                token_updated = true;
            }
            "FILENAME" => {
                // wxFileName( m_fileName ).GetFullName()
                *token = wx_file_name_full_name(&self.file_name);
                token_updated = true;
            }
            "FILEPATH" => {
                // wxFileName( m_fileName ).GetFullPath()
                *token = self.file_name.clone();
                return true;
            }
            "PAPER" => {
                *token = self.paper_format.clone();
                token_updated = true;
            }
            "LAYER" => {
                *token = self.sheet_layer.clone();
                token_updated = true;
            }
            "VARIANT" => {
                *token = self.variant_name.clone();
                token_updated = true;
            }
            "VARIANT_DESC" => {
                *token = self.variant_desc.clone();
                token_updated = true;
            }
            _ => {
                if let Some(tb) = title_block {
                    if self.title_block_text_var_resolver(tb, token) {
                        // No need for tokenUpdated; TITLE_BLOCK::TextVarResolver() already
                        // goes up to the project.
                        //
                        // However, the title block may have variables in it itself, so we
                        // need to run the worksheet resolver again.
                        let resolver = |t: &mut String| self.ws_resolve(t, None);
                        *token = expand_text_vars(token, Some(&resolver), self.context);

                        return true;
                    }
                } else if let Some(props) = self.properties
                    && let Some(value) = props.get(token.as_str())
                {
                    *token = value.clone();
                    token_updated = true;
                }
            }
        }

        if token_updated {
            let project_resolver = |t: &mut String| -> bool { self.project.is_some_and(|p| p(t)) };

            let mut depth = 0;
            let opts = TextVarOptions {
                evaluator: self.evaluator,
                ..TextVarOptions::default()
            };
            *token = resolve_text_vars(token, Some(&project_resolver), &mut depth, &opts);
            return true;
        }

        if let Some(project) = self.project
            && project(token)
        {
            return true;
        }

        false
    }

    /// Port of `DS_DRAW_ITEM_LIST::BuildFullText( const wxString& aTextbase )`: the text with
    /// its variables expanded.
    pub fn build_full_text(&self, a_textbase: &str) -> String {
        let ws_resolver = |token: &mut String| -> bool { self.ws_resolve(token, self.title_block) };

        let mut retv = expand_text_vars(a_textbase, Some(&ws_resolver), self.context);

        if retv.contains("@{") {
            // Must not be static. Painting can run on parallel workers and a shared
            // evaluator races on its internal error collector.
            if let Some(evaluator) = self.evaluator {
                retv = evaluator(&retv);
            }
        }

        finalize_text_var_expansion(&mut retv, self.context);

        retv
    }
}

/// `wxFileName( aPath ).GetFullName()` on Linux: the part after the last `/`.
fn wx_file_name_full_name(path: &str) -> String {
    match path.rfind('/') {
        Some(pos) => path[pos + 1..].to_string(),
        None => path.to_string(),
    }
}

/// Port of `DS_DATA_ITEM_TEXT::SetConstrainedTextSize`: calculate `m_ConstrainedTextSize`
/// from `m_TextSize` to keep the X size and the full Y size of the text smaller than
/// `m_BoundingBoxSize` if `m_BoundingBoxSize.x` or `m_BoundingBoxSize.y > 0`; if
/// `m_BoundingBoxSize.x` or `m_BoundingBoxSize.y == 0` the corresponding text size is not
/// constrained. Returns the constrained size.
pub fn set_constrained_text_size(
    env: &DsDrawEnv,
    t: &super::ds_data_item::DsDataItemText,
) -> kicad_geom::Vector2D {
    let mut constrained = t.text_size;

    if constrained.x == 0.0 {
        constrained.x = env.default_text_size.x;
    }

    if constrained.y == 0.0 {
        constrained.y = env.default_text_size.y;
    }

    if t.bounding_box_size.x > 0.0 || t.bounding_box_size.y > 0.0 {
        // to know the X and Y size of the line, we should use EDA_TEXT::GetTextBox()
        // but this function uses integers
        // So, to avoid truncations with our unit in mm, use microns.
        const FSCALE: f64 = 1000.0;

        let linewidth = 0;
        let size_micron = Vector2I::new(
            ki_round(constrained.x * FSCALE),
            ki_round(constrained.y * FSCALE),
        );

        let mut dummy = DsDrawItemText::new(
            &UNITY_SCALE,
            &t.full_text,
            Vector2I::new(0, 0),
            size_micron,
            linewidth,
            t.font.clone(),
            t.italic,
            t.bold,
            t.text_color.clone(),
        );
        dummy.multiline_allowed = true;
        dummy.h_justify = t.hjustify;
        dummy.v_justify = t.vjustify;
        dummy.angle = EdaAngle::from_degrees(t.orient);

        let rect = dummy.text_box(-1);

        // KiROUND( (int) rect.GetWidth() / FSCALE )
        let size = kicad_geom::Vector2D::new(
            ki_round(rect.width() as i32 as f64 / FSCALE) as f64,
            ki_round(rect.height() as i32 as f64 / FSCALE) as f64,
        );

        if t.bounding_box_size.x > 0.0 && size.x > t.bounding_box_size.x {
            constrained.x *= t.bounding_box_size.x / size.x;
        }

        if t.bounding_box_size.y > 0.0 && size.y > t.bounding_box_size.y {
            constrained.y *= t.bounding_box_size.y / size.y;
        }
    }

    constrained
}
