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

//! Port of `include/eda_text.h` and `common/eda_text.cpp` (`EDA_TEXT`).
//!
//! `EdaText` is data only (`board-model.md` 6.2a): everything `EDA_TEXT` computes through its
//! virtuals goes through the host's `EdaTextItem` impl. The base bodies of those virtuals and
//! the non-virtual members that call virtuals are generic functions over the host here, with
//! method syntax through the blanket extension trait `EdaTextExt` (signatures: package M0c;
//! bodies: M10). Dropped from `EDA_TEXT`: `m_activeUrl` (UI hover), `m_text_var_refs`
//! (reactive text-variable graph of the editor), `m_IuScale` (always `pcbIUScale`),
//! `m_bbox_cacheMutex`.
//!
//! Fonts: `m_attributes.m_Font` is the font name (`TextAttributes::font`). Text is laid out
//! with the stroke font; an outline font is drawn from a valid render cache only (the outline
//! engine is not ported, `kicad-font` D1), otherwise with the stroke font as fallback
//! (`PORT_STATUS.md`, package M10).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::string_utils::{unescape_string, wx_string_split};
use kicad_core::text_vars::{
    ResolutionContext, TextVarOptions, TextVarResolver, expand_text_vars_ext,
};
use kicad_core::{GrTextHAlignT, GrTextVAlignT, PCB_IU_SCALE, TextAttributes};
use kicad_font::font::is_stroke_font_name;
use kicad_font::gr_text::{
    BOLD_STROKE_MULTIPLIER, TEXT_MAX_SIZE_MM, TEXT_MIN_SIZE_MM, clamp_text_pen_size_vec,
    get_pen_size_for_bold, get_pen_size_for_normal,
};
use kicad_font::{
    CallbackGal, Font, FontExt, FontLookup, Glyph, ITALIC_TILT, OutlineGlyph, StrokeFont, TextGal,
    get_font,
};
use kicad_geom::{
    Box2I, EdaAngle, Shape, ShapeCompound, ShapePolySet, ShapeSegment, ShapeSimple, Vector2I,
    get_rotated_around, ki_round, rotate_point, rotate_point_around,
};

use crate::cache::CacheCell;
use crate::ctx::{BoardCtx, MutEnv};
use kicad_font::FontRef;
use crate::traits::EdaTextItem;

/// `DEFAULT_SIZE_TEXT`: default text height, in mils.
pub const DEFAULT_SIZE_TEXT: i32 = 50;

/// `EDA_TEXT` data (`include/eda_text.h`). For footprint children `lib_pos` and the size and
/// thickness in `attributes` are library-frame values (`PCB_TEXT::GetTextPos` etc. apply the
/// parent transform).
#[derive(Clone, Debug)]
pub struct EdaText {
    pub hyperlink: String, // m_hyperlink
    pub text: String,      // m_text
    /// m_shown_text: eager cache of the unescaped text (`cacheShownText`).
    pub shown_text: String,
    pub shown_text_has_text_var_refs: bool, // m_shown_text_has_text_var_refs
    /// m_attributes: font, alignment, angle, line spacing, stroke width, italic, bold,
    /// underlined, color, mirrored, multiline, size, keep upright.
    pub attributes: TextAttributes,
    pub unresolved_font_name: String, // m_unresolvedFontName
    /// m_pos: library frame for footprint children (board frame through the host's
    /// `text_pos(ctx)`).
    pub(crate) lib_pos: Vector2I,
    /// m_visible (for fields).
    pub visible: bool,
    /// m_render_cache: also what `(render_cache ...)` in the file loads into, so it is data
    /// that survives `Clone` (unlike the other caches); validated by its stored text, angle and
    /// offset.
    pub(crate) render_cache: Option<Arc<EdaTextRenderCacheData>>,
    /// m_bbox_cache: line -> (position, box), cleared by the setters.
    pub(crate) bbox_cache: CacheCell<BTreeMap<i32, (Vector2I, Box2I)>>,
}

/// `EDA_TEXT_RENDER_CACHE_DATA`. Also what `(render_cache ...)` in the file loads into: it
/// lets outline-font text be plotted without the font.
#[derive(Clone, Debug)]
pub struct EdaTextRenderCacheData {
    pub text: String,
    pub font: Option<FontRef>,
    pub angle: EdaAngle,
    pub offset: Vector2I,
    pub mirrored: bool,
    pub glyphs: Vec<kicad_font::Glyph>,
}

impl Default for EdaText {
    /// `EDA_TEXT( pcbIUScale, wxEmptyString )`: visible, `DEFAULT_SIZE_TEXT` (50 mils) square,
    /// default `TEXT_ATTRIBUTES`, empty (shown) text.
    fn default() -> Self {
        let size = PCB_IU_SCALE.mils_to_iu(DEFAULT_SIZE_TEXT);
        let mut attributes = TextAttributes::default();
        attributes.size = (size, size);
        EdaText {
            hyperlink: String::new(),
            text: String::new(),
            shown_text: String::new(),
            shown_text_has_text_var_refs: false,
            attributes,
            unresolved_font_name: String::new(),
            lib_pos: Vector2I::default(),
            visible: true,
            render_cache: None,
            bbox_cache: CacheCell::default(),
        }
    }
}

/// `-aJustify` on a `GR_TEXT_H_ALIGN_T` (`PCB_TEXT::KeepUpright` / `Mirror`): left and right
/// swap. C++ turns `INDETERMINATE` (2) into the invalid value -2; it is kept unchanged here.
pub fn negate_h_align(align: GrTextHAlignT) -> GrTextHAlignT {
    match align {
        GrTextHAlignT::Left => GrTextHAlignT::Right,
        GrTextHAlignT::Right => GrTextHAlignT::Left,
        other => other,
    }
}

/// `-aJustify` on a `GR_TEXT_V_ALIGN_T` (`PCB_TEXT::KeepUpright`): top and bottom swap.
pub fn negate_v_align(align: GrTextVAlignT) -> GrTextVAlignT {
    match align {
        GrTextVAlignT::Top => GrTextVAlignT::Bottom,
        GrTextVAlignT::Bottom => GrTextVAlignT::Top,
        other => other,
    }
}

impl EdaText {
    /// The stored `m_pos` (library frame for footprint children). The writer reads it; board
    /// code uses the host's `text_pos(ctx)`.
    pub fn lib_pos(&self) -> Vector2I {
        self.lib_pos
    }

    /// `SetLibTextPos()` / the parser: store `m_pos` directly.
    pub fn set_lib_pos(&mut self, pos: Vector2I) {
        self.lib_pos = pos;
        self.clear_bounding_box_cache();
    }

    /// `GetText()`.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `HasTextVars()`.
    pub fn has_text_vars(&self) -> bool {
        self.shown_text_has_text_var_refs
    }

    /// `IsVisible()`.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// `GetAttributes()`.
    pub fn attributes(&self) -> &TextAttributes {
        &self.attributes
    }

    /// `IsItalic()`.
    pub fn is_italic(&self) -> bool {
        self.attributes.italic
    }

    /// `IsBold()`.
    pub fn is_bold(&self) -> bool {
        self.attributes.bold
    }

    /// `IsMirrored()`.
    pub fn is_mirrored(&self) -> bool {
        self.attributes.mirrored
    }

    /// `IsMultilineAllowed()`.
    pub fn is_multiline_allowed(&self) -> bool {
        self.attributes.multiline
    }

    /// `IsKeepUpright()`.
    pub fn is_keep_upright(&self) -> bool {
        self.attributes.keep_upright
    }

    /// `GetLineSpacing()`.
    pub fn line_spacing(&self) -> f64 {
        self.attributes.line_spacing
    }

    /// `GetHyperlink()`.
    pub fn hyperlink(&self) -> &str {
        &self.hyperlink
    }

    /// `HasHyperlink()`.
    pub fn has_hyperlink(&self) -> bool {
        !self.hyperlink.is_empty()
    }

    /// `GetUnresolvedFontName()`.
    pub fn unresolved_font_name(&self) -> &str {
        &self.unresolved_font_name
    }

    /// `GetHorizJustify()`.
    pub fn horiz_justify(&self) -> GrTextHAlignT {
        self.attributes.halign
    }

    /// `GetVertJustify()`.
    pub fn vert_justify(&self) -> GrTextVAlignT {
        self.attributes.valign
    }

    /// `GetFontName()`: the resolved font's name, empty without one.
    pub fn font_name(&self) -> &str {
        self.attributes.font.as_deref().unwrap_or("")
    }

    /// `SetText()` (the `EDA_TEXT` body: also `cacheShownText()`).
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cache_shown_text();
    }

    /// `EDA_TEXT::cacheShownText()`: `m_shown_text = UnescapeString( m_text )` and whether it
    /// holds `${` or `@{`; clears both caches. (`m_text_var_refs` is not kept, see the module
    /// comment.)
    pub fn cache_shown_text(&mut self) {
        if self.text.is_empty() {
            self.shown_text = String::new();
            self.shown_text_has_text_var_refs = false;
        } else {
            self.shown_text = unescape_string(&self.text);
            self.shown_text_has_text_var_refs =
                self.shown_text.contains("${") || self.shown_text.contains("@{");
        }

        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetVisible()`: clears the render cache only (which is why board-level text loses its
    /// render cache at load).
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.clear_render_cache();
    }

    /// `SetMultilineAllowed()`.
    pub fn set_multiline_allowed(&mut self, allow: bool) {
        self.attributes.multiline = allow;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetKeepUpright()`.
    pub fn set_keep_upright(&mut self, keep_upright: bool) {
        self.attributes.keep_upright = keep_upright;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetMirrored()`.
    pub fn set_mirrored(&mut self, mirrored: bool) {
        self.attributes.mirrored = mirrored;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetHorizJustify()`.
    pub fn set_horiz_justify(&mut self, align: GrTextHAlignT) {
        self.attributes.halign = align;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetVertJustify()`.
    pub fn set_vert_justify(&mut self, align: GrTextVAlignT) {
        self.attributes.valign = align;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetItalicFlag()`.
    pub fn set_italic_flag(&mut self, italic: bool) {
        self.attributes.italic = italic;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetItalic()`: an outline font switches to its italic variant (`GetFont( name, bold,
    /// italic )`, the same name here), then `SetItalicFlag()`.
    pub fn set_italic(&mut self, italic: bool) {
        if self.attributes.italic != italic
            && self.attributes.font.is_some()
            && !self.is_stroke_font()
        {
            // SetFont( KIFONT::FONT::GetFont( font->GetName(), IsBold(), aItalic ) )
            let font = self.attributes.font.clone();
            self.set_font(font);
        }

        self.set_italic_flag(italic);
    }

    /// `SetBoldFlag()`.
    pub fn set_bold_flag(&mut self, bold: bool) {
        self.attributes.bold = bold;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetBold()`: outline fonts switch variant; stroke fonts leave the stored width alone and
    /// scale it at render time (#12911).
    pub fn set_bold(&mut self, bold: bool) {
        if self.attributes.bold != bold && self.attributes.font.is_some() && !self.is_stroke_font()
        {
            let font = self.attributes.font.clone();
            self.set_font(font);
        }

        self.set_bold_flag(bold);
    }

    /// `SetLineSpacing()`.
    pub fn set_line_spacing(&mut self, line_spacing: f64) {
        self.attributes.line_spacing = line_spacing;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetHyperlink()`.
    pub fn set_hyperlink(&mut self, link: &str) {
        self.hyperlink = link.to_string();
    }

    /// `SetUnresolvedFontName()`.
    pub fn set_unresolved_font_name(&mut self, name: &str) {
        self.unresolved_font_name = name.to_string();
    }

    /// `SetFont( aFont )` (`None` = `nullptr`, else the font's name).
    pub fn set_font(&mut self, font: Option<String>) {
        self.attributes.font = font;
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `SetAttributes( const TEXT_ATTRIBUTES& )`.
    pub fn set_attributes(&mut self, attrs: &TextAttributes) {
        self.attributes = attrs.clone();
        self.clear_render_cache();
        self.clear_bounding_box_cache();
    }

    /// `EDA_TEXT::ResolveFont( aEmbeddedFonts )`: look the unresolved face name up
    /// (`KIFONT::FONT::GetFont`, the font's name is stored); a render cache with glyphs takes
    /// the resolved font so that it stays valid. Returns whether there was a name to resolve.
    pub fn resolve_font(&mut self, embedded_fonts: Option<&[String]>) -> bool {
        if self.unresolved_font_name.is_empty() {
            return false;
        }

        let lookup = get_font(
            &self.unresolved_font_name,
            self.is_bold(),
            self.is_italic(),
            embedded_fonts,
            false,
        );
        self.attributes.font = Some(lookup.name().to_string());

        // The resolved font may not be the same as the original font, so we want to continue to
        // use the existing glyph cache to preserve the existing geometry (important on the board,
        // even if not so much on the schematic).  We need to update the font so that KiCad will
        // decide the cache is still valid.  (It will get regenerated if the user edits the text.)
        let font_ref = self.font_ref();

        if let Some(cache) = self.render_cache.as_mut() {
            if !cache.glyphs.is_empty() {
                Arc::make_mut(cache).font = Some(font_ref);
            }
        }

        // The bbox cache isn't keyed on the font, so a box measured against the fallback font
        // before resolution would otherwise survive until the next setter.
        self.clear_bounding_box_cache();

        self.unresolved_font_name = String::new();
        true
    }

    /// The identity of the draw font as the render cache records it (`KIFONT::FONT*`): the
    /// font map key of `FONT::GetFont( name, bold, italic )`.
    pub fn font_ref(&self) -> FontRef {
        FontRef {
            name: self.font_name().to_string(),
            bold: self.is_bold(),
            italic: self.is_italic(),
        }
    }

    /// `GetDrawFont( nullptr )` as a lookup: the resolved font, else the stroke font
    /// (`FONT::GetFont( wxEmptyString, bold, italic )`). Geometry is laid out with
    /// `lookup.font()` (the stroke font, also for an outline font that is not available).
    pub fn draw_font_lookup(&self) -> FontLookup {
        get_font(
            self.font_name(),
            self.is_bold(),
            self.is_italic(),
            None,
            false,
        )
    }

    /// `EDA_TEXT::isStrokeFont()`: the resolved font's `IsStroke()`, else the unresolved face
    /// name (`FONT::IsStroke( name )`), else true. An outline font counts as outline here even
    /// though its geometry falls back to the stroke font.
    pub fn is_stroke_font(&self) -> bool {
        if self.attributes.font.is_some() {
            return !self.draw_font_lookup().is_outline_unavailable();
        }

        // The font is usually unresolved during load; key off the stored face name so callers
        // that run before font resolution (e.g. migration) still classify correctly.
        if !self.unresolved_font_name.is_empty() {
            return is_stroke_font_name(&self.unresolved_font_name);
        }

        true
    }

    /// `EDA_TEXT::SetupRenderCache( aResolvedText, aFont, aAngle, aOffset )` (the parser's
    /// `(render_cache ...)`).
    pub fn setup_render_cache(
        &mut self,
        resolved_text: &str,
        font: Option<FontRef>,
        angle: EdaAngle,
        offset: Vector2I,
    ) {
        let mirrored = self.is_mirrored();
        let cache = Arc::make_mut(self.render_cache.get_or_insert_with(|| {
            Arc::new(EdaTextRenderCacheData {
                text: String::new(),
                font: None,
                angle: EdaAngle::default(),
                offset: Vector2I::default(),
                mirrored: false,
                glyphs: Vec::new(),
            })
        }));

        cache.text = resolved_text.to_string();
        cache.font = font;
        cache.angle = angle;
        cache.offset = offset;
        cache.mirrored = mirrored;
        cache.glyphs.clear();
    }

    /// `EDA_TEXT::AddRenderCacheGlyph( aPoly )`: append an outline glyph and triangulate it
    /// (`OUTLINE_GLYPH::CacheTriangulation()`).
    pub fn add_render_cache_glyph(&mut self, poly: &ShapePolySet) {
        let cache = Arc::make_mut(self.render_cache.get_or_insert_with(|| {
            Arc::new(EdaTextRenderCacheData {
                text: String::new(),
                font: None,
                angle: EdaAngle::default(),
                offset: Vector2I::default(),
                mirrored: false,
                glyphs: Vec::new(),
            })
        }));

        let mut glyph = OutlineGlyph {
            poly: poly.clone(),
            is_hover: false,
        };
        outline_glyph_cache_triangulation(&mut glyph);
        cache.glyphs.push(Glyph::Outline(glyph));
    }

    /// The stored render cache (`m_render_cache`), for the writer and tests.
    pub fn render_cache_data(&self) -> Option<&EdaTextRenderCacheData> {
        self.render_cache.as_deref()
    }

    /// `ClearRenderCache()` (the `EDA_TEXT` body).
    pub fn clear_render_cache(&mut self) {
        self.render_cache = None;
    }

    /// `ClearBoundingBoxCache()`.
    pub fn clear_bounding_box_cache(&mut self) {
        self.bbox_cache.clear();
    }

    /// `IsDefaultFormatting()`. `GetAutoThickness()` is read from the stored thickness (for a
    /// scaled footprint child the virtual getter can round a tiny width to 0; see
    /// `PORT_STATUS.md`).
    pub fn is_default_formatting(&self) -> bool {
        !self.is_mirrored()
            && self.horiz_justify() == GrTextHAlignT::Center
            && self.vert_justify() == GrTextVAlignT::Center
            && self.attributes.stroke_width == 0
            && !self.is_italic()
            && !self.is_bold()
            && !self.is_multiline_allowed()
            && self.font_name().is_empty()
    }
}

/// `OUTLINE_GLYPH::CacheTriangulation()`: triangulate once (`aSimplify = false`, no index).
fn outline_glyph_cache_triangulation(glyph: &mut OutlineGlyph) {
    // Only call CacheTriangulation if it has never been done before.  Otherwise we'll hash
    // the triangulation to see if it has been edited, and glyphs are invariant after creation.
    if glyph.poly.triangulated_poly_count() == 0 {
        glyph.poly.cache_triangulation(false);
    }
}

/// `OUTLINE_GLYPH::Triangulate( aCallback )` (the triangulation is built on a copy when the
/// glyph has none yet, as the C++ `const_cast` does on the glyph).
pub(crate) fn outline_glyph_triangulate(
    glyph: &OutlineGlyph,
    callback: &mut dyn FnMut(Vector2I, Vector2I, Vector2I),
) {
    let triangulated;
    let poly = if glyph.poly.triangulated_poly_count() == 0 {
        let mut copy = glyph.clone();
        outline_glyph_cache_triangulation(&mut copy);
        triangulated = copy.poly;
        &triangulated
    } else {
        &glyph.poly
    };

    for i in 0..poly.triangulated_poly_count() {
        let polygon = poly.triangulated_polygon(i as i32);

        for j in 0..polygon.triangle_count() {
            let (a, b, c) = polygon.get_triangle(j);
            callback(a, b, c);
        }
    }
}

/// The non-triangulating outline branch of `CALLBACK_GAL::DrawGlyph`: a copy of the glyph,
/// fractured if it has holes, handed over outline by outline.
fn outline_glyph_outlines(
    glyph: &OutlineGlyph,
    callback: &mut dyn FnMut(&kicad_geom::ShapeLineChain),
) {
    let mut copy = glyph.poly.clone();

    if copy.has_holes() {
        copy.fracture(true);
    }

    for ii in 0..copy.outline_count() {
        callback(copy.c_outline(ii));
    }
}

/// Port of `ResolveTextVars( aSource, aResolver, aDepth )` for the `GetShownText` overrides:
/// the C++ resolver lambdas capture `aDepth` by reference and so see the pass counter that
/// `ResolveTextVars` increments; `depth` is shared through a `Cell` to reproduce that. The
/// loop is that of `kicad_core::text_vars::resolve_text_vars` (no expression evaluator:
/// text_eval is not ported).
pub(crate) fn resolve_text_vars_shared_depth(
    source: &str,
    resolver: &TextVarResolver<'_>,
    depth: &Cell<i32>,
) -> String {
    let opts = TextVarOptions::default();
    let mut text = source.to_string();

    while text.contains("${") || text.contains("@{") {
        depth.set(depth.get() + 1);

        if depth.get() > opts.max_depth {
            break;
        }

        // Always expand when ${} or @{} present to handle escape sequences (\${} and \@{}).
        if text.contains("${") || text.contains("@{") {
            text =
                expand_text_vars_ext(&text, Some(resolver), ResolutionContext::Internal, 0, &opts);
        }

        // Only evaluate if there are @{} expressions present (not escape markers)
        if text.contains("@{") {
            if let Some(eval) = opts.evaluator {
                text = eval(&text);
            }
        }
    }

    text
}

// ---- base bodies of the EDA_TEXT virtuals (EdaTextItem defaults) ------------------------

/// `EDA_TEXT::GetTextPos()`: `m_pos`.
pub fn text_pos<H: EdaTextItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> Vector2I {
    host.eda_text().lib_pos
}

/// `EDA_TEXT::GetTextSize()`: `m_attributes.m_Size`.
pub fn text_size<H: EdaTextItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> Vector2I {
    let (x, y) = host.eda_text().attributes.size;
    Vector2I::new(x, y)
}

/// `EDA_TEXT::GetTextThickness()`: `m_attributes.m_StrokeWidth`.
pub fn text_thickness<H: EdaTextItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> i32 {
    host.eda_text().attributes.stroke_width
}

/// `EDA_TEXT::GetTextAngle()`: `m_attributes.m_Angle.GetAngle()`.
pub fn text_angle<H: EdaTextItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> EdaAngle {
    EdaAngle::from_degrees(host.eda_text().attributes.angle)
}

/// `EDA_TEXT::GetShownText( aContext, aDepth )`: `m_text` for `RAW_VALUE`, else the cached
/// unescaped text.
pub fn shown_text<H: EdaTextItem + ?Sized>(
    host: &H,
    _ctx: &BoardCtx<'_>,
    context: ResolutionContext,
    _depth: i32,
) -> String {
    if context == ResolutionContext::RawValue {
        return host.eda_text().text.clone();
    }

    host.eda_text().shown_text.clone()
}

/// The virtual `GetTextPos()` as a mutator sees it: the stored `m_pos` mapped through the
/// parent footprint's transform (`PCB_TEXT::GetTextPos`, inherited by fields, dimensions and
/// the barcode text), `m_pos` without a parent footprint. `PCB_TEXTBOX` / `PCB_TABLECELL` do
/// not override `GetTextPos`; their `m_pos` is unused by the board code (`PORT_STATUS.md`).
pub(crate) fn text_pos_env<H: EdaTextItem + ?Sized>(host: &H, env: &MutEnv) -> Vector2I {
    let lib = host.eda_text().lib_pos;

    match env.parent.as_ref() {
        Some(parent) => parent.transform.apply(lib),
        None => lib,
    }
}

/// `EDA_TEXT::SetTextPos()`: `Offset( aPoint - GetTextPos() )`.
pub fn set_text_pos<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, pos: Vector2I) {
    let current = text_pos_env(host, env);
    host.offset(env, Vector2I::new(pos.x - current.x, pos.y - current.y));
}

/// `EDA_TEXT::SetTextSize( aNewSize, aEnforceMinTextSize )`: clamp each component to
/// `[TEXT_MIN_SIZE_MM, TEXT_MAX_SIZE_MM]` when asked (the `unityScale` exception does not apply:
/// board text is always `pcbIUScale`).
pub fn set_text_size<H: EdaTextItem + ?Sized>(
    host: &mut H,
    _env: &MutEnv,
    size: Vector2I,
    enforce_min: bool,
) {
    let mut new_size = size;

    if enforce_min {
        let min = PCB_IU_SCALE.mm_to_iu(TEXT_MIN_SIZE_MM);
        let max = PCB_IU_SCALE.mm_to_iu(TEXT_MAX_SIZE_MM);

        new_size = Vector2I::new(new_size.x.clamp(min, max), new_size.y.clamp(min, max));
    }

    let text = host.eda_text_mut();
    text.attributes.size = (new_size.x, new_size.y);
    text.clear_render_cache();
    text.clear_bounding_box_cache();
}

/// `EDA_TEXT::SetTextThickness()`.
pub fn set_text_thickness<H: EdaTextItem + ?Sized>(host: &mut H, _env: &MutEnv, width: i32) {
    let text = host.eda_text_mut();
    text.attributes.stroke_width = width;
    text.clear_render_cache();
    text.clear_bounding_box_cache();
}

/// `EDA_TEXT::SetTextAngle()` (`m_attributes.m_Angle` is an `EDA_ORIENTATION`: normalised).
pub fn set_text_angle<H: EdaTextItem + ?Sized>(host: &mut H, _env: &MutEnv, angle: EdaAngle) {
    let text = host.eda_text_mut();
    text.attributes.set_angle(angle.as_degrees());
    text.clear_render_cache();
    text.clear_bounding_box_cache();
}

/// `EDA_TEXT::Offset()`: move `m_pos` and the glyphs of the render cache.
pub fn offset<H: EdaTextItem + ?Sized>(host: &mut H, _env: &MutEnv, delta: Vector2I) {
    eda_text_offset(host.eda_text_mut(), delta)
}

/// The body of `EDA_TEXT::Offset()` on the data (also `PCB_TEXT::SetLibTextPos`).
pub(crate) fn eda_text_offset(text: &mut EdaText, delta: Vector2I) {
    if delta.x == 0 && delta.y == 0 {
        return;
    }

    text.lib_pos += delta;

    if let Some(cache) = text.render_cache.as_mut() {
        for glyph in Arc::make_mut(cache).glyphs.iter_mut() {
            match glyph {
                Glyph::Outline(outline) => outline.poly.move_(delta),
                Glyph::Stroke(stroke) => {
                    *stroke = stroke.transform(
                        kicad_geom::Vector2D::new(1.0, 1.0),
                        delta,
                        0.0,
                        kicad_geom::ANGLE_0,
                        false,
                        Vector2I::new(0, 0),
                    );
                }
            }
        }
    }

    text.clear_bounding_box_cache();
}

/// `EDA_TEXT::SetAutoThickness( aAuto )`: freeze the base width (not the effective one; Bold
/// still multiplies at render time) or go back to auto.
pub fn set_auto_thickness<H: EdaTextItem + ?Sized>(
    host: &mut H,
    ctx: &BoardCtx<'_>,
    env: &MutEnv,
    auto: bool,
) {
    if auto_thickness(host, ctx) != auto {
        let width = if auto {
            0
        } else {
            get_pen_size_for_normal(text_width(host, ctx))
        };
        host.set_text_thickness(env, width);
    }
}

/// `EDA_TEXT::MigrateLegacyBoldStrokeWidth()`: divide a bold stroke width that has the bold
/// multiplier baked in (files before #12911) back to its base.
pub fn migrate_legacy_bold_stroke_width<H: EdaTextItem + ?Sized>(
    host: &mut H,
    ctx: &BoardCtx<'_>,
    env: &MutEnv,
) {
    let thickness = host.text_thickness(ctx);

    // Outline faces never baked bold into the width; skip them.
    if !host.eda_text().is_bold() || thickness <= 1 || !host.eda_text().is_stroke_font() {
        return;
    }

    // Keep the base above the auto threshold so a very thin width doesn't become auto. Legacy
    // widths below ~3 IU (well under any physically meaningful stroke) can't round-trip exactly
    // through this floor; the effective width comes out slightly larger than before migration.
    host.set_text_thickness(
        env,
        std::cmp::max(2, ki_round(thickness as f64 / BOLD_STROKE_MULTIPLIER)),
    );
}

/// `EDA_TEXT::TextHitTest( aPoint, aAccuracy )` (the base body, called by the `PCB_TEXT`
/// override).
pub fn text_hit_test<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    point: Vector2I,
    accuracy: i32,
) -> bool {
    let rect = text_box(host, ctx, None).inflated(accuracy);
    let location = get_rotated_around(point, host.draw_pos(ctx), -host.draw_rotation(ctx));
    rect.contains(location)
}

/// `EDA_TEXT::TextHitTest( aRect, aContains, aAccuracy )` (the base body).
pub fn text_hit_test_rect<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    rect: &Box2I,
    contains: bool,
    accuracy: i32,
) -> bool {
    let rect = rect.inflated(accuracy);

    if contains {
        return rect.contains_box(&text_box(host, ctx, None));
    }

    rect.intersects_rotated(&text_box(host, ctx, None), host.draw_rotation(ctx))
}

/// `EDA_TEXT::GetRenderCache( aFont, forResolvedText, aOffset )`: the cached glyphs when the
/// draw font is an outline font and the cache is valid for this text, rotation, offset and
/// mirroring. `None` for a stroke font (as C++), and also where C++ would regenerate the cache
/// with the outline engine (not ported): the caller then draws with the stroke font.
pub fn render_cache<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    resolved_text: &str,
    offset: Vector2I,
) -> Option<Arc<EdaTextRenderCacheData>> {
    let text = host.eda_text();

    if !text.draw_font_lookup().is_outline_unavailable() {
        return None;
    }

    let resolved_angle = host.draw_rotation(ctx);
    let mirrored = text.is_mirrored();
    let cache = text.render_cache.as_ref()?;

    if cache.glyphs.is_empty()
        || cache.font.as_ref() != Some(&text.font_ref())
        || cache.text != resolved_text
        || cache.angle != resolved_angle
        || cache.offset != offset
        || cache.mirrored != mirrored
    {
        return None;
    }

    Some(Arc::clone(cache))
}

// ---- non-virtual EDA_TEXT members that call virtuals (EdaTextExt) ------------------------

/// `EDA_TEXT::GetTextBox( aSettings, aLine )` (`None` = `aLine == -1`: the whole text).
pub fn text_box<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>, line: Option<i32>) -> Box2I {
    let a_line = line.unwrap_or(-1);
    let draw_pos = host.draw_pos(ctx);
    let et = host.eda_text();

    let cached = et.bbox_cache.with(|cache| {
        cache
            .get(&a_line)
            .filter(|(pos, _)| *pos == draw_pos)
            .map(|(_, bbox)| *bbox)
    });

    if let Some(bbox) = cached {
        return bbox;
    }

    let mut bbox = Box2I::default();
    let mut strings: Vec<String> = Vec::new();
    let mut text = host.shown_text(ctx, ResolutionContext::ForCanvas, 0);
    let thickness = effective_text_pen_width(host, ctx, 0);

    if et.is_multiline_allowed() {
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
    let lookup = et.draw_font_lookup();
    let font: &StrokeFont = lookup.font();
    let font_size = host.text_size(ctx);
    let bold = et.is_bold();
    let italic = et.is_italic();
    let metrics = host.font_metrics(ctx);
    let mut extents =
        font.string_boundary_limits(&text, font_size, thickness, bold, italic, metrics);
    let mut overbar_offset = 0;

    // Creates bounding box (rectangle) for horizontal, left and top justified text. The
    // bounding box will be moved later according to the actual text options
    let mut textsize = Vector2I::new(extents.x, extents.y);
    let mut pos = draw_pos;
    let fudge_factor = ki_round(extents.y as f64 * 0.17);

    if font.is_stroke() {
        textsize.y += fudge_factor;
    }

    let interline = ki_round(font.interline(font_size.y as f64, metrics) * et.line_spacing());

    if et.is_multiline_allowed() && a_line > 0 && a_line < strings.len() as i32 {
        pos.y -= a_line * interline;
    }

    if text.contains("~{") {
        overbar_offset = extents.y / 6;
    }

    bbox.set_origin(pos);

    // for multiline texts and aLine < 0, merge all rectangles (aLine == -1 signals all lines)
    if et.is_multiline_allowed() && a_line < 0 && strings.len() > 1 {
        for s in strings.iter().skip(1) {
            extents = font.string_boundary_limits(s, font_size, thickness, bold, italic, metrics);
            textsize.x = std::cmp::max(textsize.x, extents.x);
        }

        // interline spacing is only *between* lines, so total height is the height of the first
        // line plus the interline distance (with interline spacing) for all subsequent lines
        // (C++: size_t arithmetic, converted back to int: wraps like i32).
        textsize.y = textsize
            .y
            .wrapping_add(((strings.len() - 1) as i32).wrapping_mul(interline));
    }

    textsize.y += overbar_offset;

    bbox.set_size_wh(textsize.x as i64, textsize.y as i64);

    // At this point the rectangle origin is the text origin (m_Pos).  This is correct only for
    // left and top justified, non-mirrored, non-overbarred texts. Recalculate for all others.
    let italic_offset: i64 = if et.is_italic() {
        ki_round(font_size.y as f64 * ITALIC_TILT) as i64
    } else {
        0
    };

    match et.horiz_justify() {
        GrTextHAlignT::Left => {
            if et.is_mirrored() {
                bbox.set_x((bbox.x() as i64 - (bbox.width() - italic_offset)) as i32);
            }
        }
        GrTextHAlignT::Center => {
            bbox.set_x((bbox.x() as i64 - (bbox.width() - italic_offset) / 2) as i32);
        }
        GrTextHAlignT::Right => {
            if !et.is_mirrored() {
                bbox.set_x((bbox.x() as i64 - (bbox.width() - italic_offset)) as i32);
            }
        }
        // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
        GrTextHAlignT::Indeterminate => {}
    }

    match et.vert_justify() {
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

    et.bbox_cache.with(|cache| {
        cache.insert(a_line, (draw_pos, bbox));
    });

    bbox
}

/// `EDA_TEXT::GetEffectiveTextPenWidth( aDefaultPenWidth )`.
pub fn effective_text_pen_width<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    default_pen_width: i32,
) -> i32 {
    let et = host.eda_text();
    let mut pen_width = host.text_thickness(ctx);

    if pen_width <= 1 {
        pen_width = default_pen_width;

        if et.is_bold() {
            pen_width = get_pen_size_for_bold(text_width(host, ctx));
        } else if pen_width <= 1 {
            pen_width = get_pen_size_for_normal(text_width(host, ctx));
        }
    } else if et.is_bold() && et.is_stroke_font() {
        pen_width = ki_round(pen_width as f64 * BOLD_STROKE_MULTIPLIER);
    }

    // Clip pen size for small texts:
    clamp_text_pen_size_vec(pen_width, host.text_size(ctx), false)
}

/// `EDA_TEXT::GetEffectiveTextShape( aTriangulate, aBBox, aAngle )` (`None` = the default
/// empty `BOX2I()`).
pub fn effective_text_shape<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    triangulate: bool,
    bbox: Option<&Box2I>,
    angle: EdaAngle,
) -> Arc<ShapeCompound> {
    let shape = RefCell::new(ShapeCompound::new());
    let et = host.eda_text();
    let lookup = et.draw_font_lookup();
    let font: &StrokeFont = lookup.font();
    let pen_width = effective_text_pen_width(host, ctx, 0);
    let shown_text = host.shown_text(ctx, ResolutionContext::ForCanvas, 0);
    let mut draw_pos = host.draw_pos(ctx);
    let mut attrs = et.attributes.clone();
    let size = host.text_size(ctx);

    attrs.size = (size.x, size.y);

    let mut cache: Option<Arc<EdaTextRenderCacheData>> = None;

    match bbox {
        Some(bbox) if bbox.width() != 0 => {
            draw_pos = bbox.center();
            attrs.halign = GrTextHAlignT::Center;
            attrs.valign = GrTextVAlignT::Center;
            attrs.set_angle(angle.as_degrees());
        }
        _ => {
            attrs.set_angle(host.draw_rotation(ctx).as_degrees());

            if lookup.is_outline_unavailable() {
                cache = render_cache(host, ctx, &shown_text, Vector2I::default());
            }
        }
    }

    let metrics = host.font_metrics(ctx);

    {
        let mut callback_gal = if triangulate {
            CallbackGal::new(
                // Stroke callback
                |pt1, pt2| {
                    shape
                        .borrow_mut()
                        .add_shape(Box::new(ShapeSegment::new(pt1, pt2, pen_width)));
                },
                // Triangulation callback
                |glyph| {
                    outline_glyph_triangulate(glyph, &mut |pt1, pt2, pt3| {
                        let mut tri_shape = ShapeSimple::new();

                        for point in [pt1, pt2, pt3] {
                            tri_shape.append_xy(point.x, point.y);
                        }

                        shape.borrow_mut().add_shape(Box::new(tri_shape));
                    });
                },
            )
        } else {
            CallbackGal::new(
                // Stroke callback
                |pt1, pt2| {
                    shape
                        .borrow_mut()
                        .add_shape(Box::new(ShapeSegment::new(pt1, pt2, pen_width)));
                },
                // Outline callback
                |glyph| {
                    outline_glyph_outlines(glyph, &mut |poly| {
                        shape.borrow_mut().add_shape(Box::new(poly.clone()));
                    });
                },
            )
        };

        match cache.as_ref() {
            Some(cache) => callback_gal.draw_glyphs(&cache.glyphs),
            None => font.draw(&mut callback_gal, &shown_text, draw_pos, &attrs, metrics),
        }
    }

    Arc::new(shape.into_inner())
}

/// `EDA_TEXT::GetLinePositions( aSettings, aPositions, aLineCount )`.
pub fn line_positions<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_count: i32,
) -> Vec<Vector2I> {
    let mut positions = Vec::new();

    // Position of first line of the multiline text according to the center of the multiline
    // text block
    let mut pos = host.draw_pos(ctx);

    // Offset to next line.
    let mut offset = Vector2I::new(0, interline(host, ctx));

    if line_count > 1 {
        match host.eda_text().vert_justify() {
            GrTextVAlignT::Top => {}
            GrTextVAlignT::Center => pos.y -= (line_count - 1) * offset.y / 2,
            GrTextVAlignT::Bottom => pos.y -= (line_count - 1) * offset.y,
            GrTextVAlignT::Indeterminate => {}
        }
    }

    // Rotate the position of the first line around the center of the multiline text block
    rotate_point_around(&mut pos, host.draw_pos(ctx), host.draw_rotation(ctx));

    // Rotate the offset lines to increase happened in the right direction
    rotate_point(&mut offset, host.draw_rotation(ctx));

    for _ in 0..line_count {
        positions.push(pos);
        pos += offset;
    }

    positions
}

/// `EDA_TEXT::GetInterline( aSettings )`.
pub fn interline<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    let lookup = host.eda_text().draw_font_lookup();
    let font: &StrokeFont = lookup.font();

    ki_round(
        font.interline(text_height(host, ctx) as f64, host.font_metrics(ctx))
            * host.eda_text().line_spacing(),
    )
}

/// `wxString::Cmp`: negative, 0 or positive (only the sign is meaningful).
fn wx_cmp(a: &str, b: &str) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `EDA_TEXT::Compare( aOther )`: negative, 0 or positive.
pub fn compare_text<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    other: &dyn EdaTextItem,
) -> i32 {
    let a = host.eda_text();
    let b = other.eda_text();

    let val = a.attributes.compare(&b.attributes);

    if val != 0 {
        return val;
    }

    let pos = host.text_pos(ctx);
    let other_pos = other.text_pos(ctx);

    if pos.x != other_pos.x {
        return pos.x.wrapping_sub(other_pos.x);
    }

    if pos.y != other_pos.y {
        return pos.y.wrapping_sub(other_pos.y);
    }

    let val = wx_cmp(a.font_name(), b.font_name());

    if val != 0 {
        return val;
    }

    let val = wx_cmp(&a.hyperlink, &b.hyperlink);

    if val != 0 {
        return val;
    }

    wx_cmp(&a.text, &b.text)
}

/// `EDA_TEXT::GetAutoThickness()`: `GetTextThickness() == 0`.
pub fn auto_thickness<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> bool {
    host.text_thickness(ctx) == 0
}

/// `EDA_TEXT::GetTextWidth()`: `GetTextSize().x`.
pub fn text_width<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    host.text_size(ctx).x
}

/// `EDA_TEXT::GetTextHeight()`: `GetTextSize().y`.
pub fn text_height<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    host.text_size(ctx).y
}

/// `EDA_TEXT::GetTextStyleName()` (untranslated).
pub fn text_style_name<H: EdaTextItem + ?Sized>(host: &H) -> String {
    let mut style = 0;

    if host.eda_text().is_italic() {
        style = 1;
    }

    if host.eda_text().is_bold() {
        style += 2;
    }

    const STYLEMSG: [&str; 4] = ["Normal", "Italic", "Bold", "Bold+Italic"];

    STYLEMSG[style].to_string()
}

/// The non-virtual `EDA_TEXT` members that call the virtuals, with method syntax. Defined
/// here and blanket-implemented so they cannot be overridden (`board-model.md` 6.2a).
pub trait EdaTextExt: EdaTextItem {
    /// `GetTextBox( aSettings, aLine )`.
    fn text_box(&self, ctx: &BoardCtx<'_>, line: Option<i32>) -> Box2I {
        text_box(self, ctx, line)
    }

    /// `GetEffectiveTextPenWidth( aDefaultPenWidth )`.
    fn effective_text_pen_width(&self, ctx: &BoardCtx<'_>, default_pen_width: i32) -> i32 {
        effective_text_pen_width(self, ctx, default_pen_width)
    }

    /// `GetEffectiveTextShape( aTriangulate, aBBox, aAngle )`.
    fn effective_text_shape(
        &self,
        ctx: &BoardCtx<'_>,
        triangulate: bool,
        bbox: Option<&Box2I>,
        angle: EdaAngle,
    ) -> Arc<ShapeCompound> {
        effective_text_shape(self, ctx, triangulate, bbox, angle)
    }

    /// `GetLinePositions( aSettings, aPositions, aLineCount )`.
    fn line_positions(&self, ctx: &BoardCtx<'_>, line_count: i32) -> Vec<Vector2I> {
        line_positions(self, ctx, line_count)
    }

    /// `GetInterline( aSettings )`.
    fn interline(&self, ctx: &BoardCtx<'_>) -> i32 {
        interline(self, ctx)
    }

    /// `Compare( aOther )`.
    fn compare_text(&self, ctx: &BoardCtx<'_>, other: &dyn EdaTextItem) -> i32 {
        compare_text(self, ctx, other)
    }

    /// `GetAutoThickness()`.
    fn auto_thickness(&self, ctx: &BoardCtx<'_>) -> bool {
        auto_thickness(self, ctx)
    }

    /// `GetTextWidth()`.
    fn text_width(&self, ctx: &BoardCtx<'_>) -> i32 {
        text_width(self, ctx)
    }

    /// `GetTextHeight()`.
    fn text_height(&self, ctx: &BoardCtx<'_>) -> i32 {
        text_height(self, ctx)
    }

    /// `GetTextStyleName()`.
    fn text_style_name(&self) -> String {
        text_style_name(self)
    }
}

impl<T: EdaTextItem + ?Sized> EdaTextExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EDA_TEXT( pcbIUScale )` (common/eda_text.cpp): visible, 50 mil text, empty.
    #[test]
    fn constructor_values() {
        let t = EdaText::default();
        assert_eq!(t.attributes.size, (1_270_000, 1_270_000));
        assert!(t.visible && t.attributes.multiline);
        assert_eq!(t.attributes.stroke_width, 0);
        assert!(t.text.is_empty() && t.shown_text.is_empty() && !t.has_text_vars());
        assert_eq!(t.lib_pos(), Vector2I::default());
        assert!(t.render_cache.is_none());
    }

    // ---- qa/tests/common/test_eda_text.cpp ---------------------------------------------
    //
    // The C++ cases use a bare `EDA_TEXT( unityScale )`; here a detached `PcbText` is the
    // host (no parent footprint: its getters are the `EDA_TEXT` bodies, except
    // `GetTextAngle`, which none of these cases reads). The `std::hash<EDA_TEXT>` checks are
    // not ported (no hash of EDA_TEXT in the port).

    use crate::board::Board;
    use crate::pcb_text::PcbText;
    use kicad_font::gr_text::get_pen_size_for_bold;

    /// `makeText200()`: a 200 x 200 text, size not clamped.
    fn make_text200() -> PcbText {
        let mut t = PcbText::default();
        t.set_text_size(&MutEnv::detached(), Vector2I::new(200, 200), false);
        t
    }

    /// `Compare` (test_eda_text.cpp).
    #[test]
    fn compare() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let env = MutEnv::detached();
        let mut a = PcbText::default();
        let mut b = PcbText::default();

        assert_eq!(a.compare_text(&ctx, &b), 0);

        a.text.set_text("A");
        assert!(a.compare_text(&ctx, &b) > 0);

        b.text.set_text("B");
        assert!(a.compare_text(&ctx, &b) < 0);

        a.text.set_text("B");
        a.set_text_pos(&env, Vector2I::new(1, 0));
        assert!(a.compare_text(&ctx, &b) > 0);

        a.set_text_pos(&env, Vector2I::new(-1, 0));
        assert!(a.compare_text(&ctx, &b) < 0);

        a.set_text_pos(&env, Vector2I::new(0, 0));
        b.set_text_pos(&env, Vector2I::new(0, 1));
        assert!(a.compare_text(&ctx, &b) < 0);

        b.set_text_pos(&env, Vector2I::new(0, -1));
        assert!(a.compare_text(&ctx, &b) > 0);
    }

    /// `BoldFlagLeavesStoredThicknessAlone` (#12911).
    #[test]
    fn bold_flag_leaves_stored_thickness_alone() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let mut t = make_text200();
        t.set_text_thickness(&MutEnv::detached(), 30);

        t.text.set_bold_flag(true);
        assert!(t.text.is_bold());
        assert_eq!(t.text_thickness(&ctx), 30);

        t.text.set_bold(false);
        assert!(!t.text.is_bold());
        assert_eq!(t.text_thickness(&ctx), 30);
    }

    /// `EffectiveWidthAppliesBoldMultiplierForStroke`: 10 -> 16.
    #[test]
    fn effective_width_applies_bold_multiplier_for_stroke() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let mut t = make_text200();
        t.set_text_thickness(&MutEnv::detached(), 10);

        assert_eq!(t.effective_text_pen_width(&ctx, 0), 10);

        t.text.set_bold_flag(true);
        assert_eq!(t.effective_text_pen_width(&ctx, 0), 16);
    }

    /// `EffectiveWidthAutoBoldMatchesLegacy`.
    #[test]
    fn effective_width_auto_bold_matches_legacy() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let mut t = make_text200();
        t.set_text_thickness(&MutEnv::detached(), 0);
        t.text.set_bold_flag(true);

        assert_eq!(
            t.effective_text_pen_width(&ctx, 0),
            get_pen_size_for_bold(t.text_width(&ctx))
        );
    }

    /// `MigrateLegacyBoldStrokeWidthRecoversBase`.
    #[test]
    fn migrate_legacy_bold_stroke_width_recovers_base() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let env = MutEnv::detached();

        let mut bold = make_text200();
        bold.set_text_thickness(&env, 32);
        bold.text.set_bold_flag(true);
        migrate_legacy_bold_stroke_width(&mut bold, &ctx, &env);
        assert_eq!(bold.text_thickness(&ctx), 20);
        assert_eq!(bold.effective_text_pen_width(&ctx, 0), 32); // rendered width preserved

        let mut normal = make_text200();
        normal.set_text_thickness(&env, 32);
        migrate_legacy_bold_stroke_width(&mut normal, &ctx, &env);
        assert_eq!(normal.text_thickness(&ctx), 32);

        let mut auto_width = make_text200();
        auto_width.set_text_thickness(&env, 0);
        auto_width.text.set_bold_flag(true);
        migrate_legacy_bold_stroke_width(&mut auto_width, &ctx, &env);
        assert_eq!(auto_width.text_thickness(&ctx), 0);

        // A width dividing to <= 1 must not collapse into the auto special case: floor at 2.
        let mut tiny = make_text200();
        tiny.set_text_thickness(&env, 2);
        tiny.text.set_bold_flag(true);
        migrate_legacy_bold_stroke_width(&mut tiny, &ctx, &env);
        assert_eq!(tiny.text_thickness(&ctx), 2);
        assert_eq!(tiny.effective_text_pen_width(&ctx, 0), 3);
    }

    /// `AutoThicknessOffFreezesBaseNotEffective`.
    #[test]
    fn auto_thickness_off_freezes_base_not_effective() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let env = MutEnv::detached();
        let mut t = make_text200();
        t.set_text_thickness(&env, 0);
        t.text.set_bold_flag(true);

        let auto_effective = t.effective_text_pen_width(&ctx, 0);

        set_auto_thickness(&mut t, &ctx, &env, false);
        assert_eq!(
            t.text_thickness(&ctx),
            get_pen_size_for_normal(t.text_width(&ctx))
        );
        assert_eq!(t.effective_text_pen_width(&ctx, 0), auto_effective);
    }

    /// `cacheShownText` / `SetText`: unescaping and the text-variable flag.
    #[test]
    fn cache_shown_text() {
        let mut t = EdaText::default();
        t.set_text("a{slash}b ${REFERENCE}");
        assert_eq!(t.shown_text, "a/b ${REFERENCE}");
        assert!(t.has_text_vars());
        t.set_text("");
        assert!(t.shown_text.is_empty() && !t.has_text_vars());
        t.set_text("@{1+1}");
        assert!(t.has_text_vars());
        assert!(!EdaText::default().is_default_formatting()); // multiline default on
        let mut d = EdaText::default();
        d.set_multiline_allowed(false);
        assert!(d.is_default_formatting());
    }

    /// `SetTextSize` clamps to `[TEXT_MIN_SIZE_MM, TEXT_MAX_SIZE_MM]` when enforced.
    #[test]
    fn set_text_size_clamps() {
        let board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let mut t = PcbText::default();
        t.set_text_size(&MutEnv::detached(), Vector2I::new(10, 300_000_000), true);
        assert_eq!(t.text_size(&ctx), Vector2I::new(1_000, 250_000_000));
        t.set_text_size(&MutEnv::detached(), Vector2I::new(10, 300_000_000), false);
        assert_eq!(t.text_size(&ctx), Vector2I::new(10, 300_000_000));
        assert_eq!(text_style_name(&t), "Normal");
        t.text.set_bold_flag(true);
        t.text.set_italic_flag(true);
        assert_eq!(text_style_name(&t), "Bold+Italic");
    }
}
