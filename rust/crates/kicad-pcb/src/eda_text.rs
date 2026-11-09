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

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::text_vars::ResolutionContext;
use kicad_core::{PCB_IU_SCALE, TextAttributes};
use kicad_geom::{Box2I, EdaAngle, ShapeCompound, Vector2I};

use crate::cache::CacheCell;
use crate::ctx::{BoardCtx, MutEnv};
use crate::foundation_stub::FontRef;
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

    /// `GetUnresolvedFontName()`.
    pub fn unresolved_font_name(&self) -> &str {
        &self.unresolved_font_name
    }

    /// `SetText()` (the `EDA_TEXT` body: also `cacheShownText()`).
    pub fn set_text(&mut self, text: &str) {
        crate::port_pending!("EDA_TEXT::SetText", text)
    }

    /// `SetVisible()`.
    pub fn set_visible(&mut self, visible: bool) {
        crate::port_pending!("EDA_TEXT::SetVisible", visible)
    }

    /// `SetMultilineAllowed()`.
    pub fn set_multiline_allowed(&mut self, allow: bool) {
        crate::port_pending!("EDA_TEXT::SetMultilineAllowed", allow)
    }

    /// `SetKeepUpright()`.
    pub fn set_keep_upright(&mut self, keep_upright: bool) {
        crate::port_pending!("EDA_TEXT::SetKeepUpright", keep_upright)
    }

    /// `SetMirrored()`.
    pub fn set_mirrored(&mut self, mirrored: bool) {
        crate::port_pending!("EDA_TEXT::SetMirrored", mirrored)
    }

    /// `ClearRenderCache()` (the `EDA_TEXT` body).
    pub fn clear_render_cache(&mut self) {
        self.render_cache = None;
    }

    /// `ClearBoundingBoxCache()`.
    pub fn clear_bounding_box_cache(&mut self) {
        self.bbox_cache.clear();
    }

    /// `IsDefaultFormatting()`.
    pub fn is_default_formatting(&self) -> bool {
        crate::port_pending!("EDA_TEXT::IsDefaultFormatting")
    }
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

/// `EDA_TEXT::GetShownText( aContext, aDepth )`.
pub fn shown_text<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    context: ResolutionContext,
    depth: i32,
) -> String {
    crate::port_pending!("EDA_TEXT::GetShownText", host, ctx, context, depth)
}

/// `EDA_TEXT::SetTextPos()`: `Offset( aPoint - GetTextPos() )`.
pub fn set_text_pos<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, pos: Vector2I) {
    crate::port_pending!("EDA_TEXT::SetTextPos", host, env, pos)
}

/// `EDA_TEXT::SetTextSize( aNewSize, aEnforceMinTextSize )`.
pub fn set_text_size<H: EdaTextItem + ?Sized>(
    host: &mut H,
    env: &MutEnv,
    size: Vector2I,
    enforce_min: bool,
) {
    crate::port_pending!("EDA_TEXT::SetTextSize", host, env, size, enforce_min)
}

/// `EDA_TEXT::SetTextThickness()`.
pub fn set_text_thickness<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, width: i32) {
    crate::port_pending!("EDA_TEXT::SetTextThickness", host, env, width)
}

/// `EDA_TEXT::SetTextAngle()`.
pub fn set_text_angle<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, angle: EdaAngle) {
    crate::port_pending!("EDA_TEXT::SetTextAngle", host, env, angle)
}

/// `EDA_TEXT::Offset()`.
pub fn offset<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, delta: Vector2I) {
    crate::port_pending!("EDA_TEXT::Offset", host, env, delta)
}

// ---- non-virtual EDA_TEXT members that call virtuals (EdaTextExt) ------------------------

/// `EDA_TEXT::GetTextBox( aSettings, aLine )` (`None` = `aLine == -1`: the whole text).
pub fn text_box<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>, line: Option<i32>) -> Box2I {
    crate::port_pending!("EDA_TEXT::GetTextBox", host, ctx, line)
}

/// `EDA_TEXT::GetEffectiveTextPenWidth( aDefaultPenWidth )`.
pub fn effective_text_pen_width<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    default_pen_width: i32,
) -> i32 {
    crate::port_pending!(
        "EDA_TEXT::GetEffectiveTextPenWidth",
        host,
        ctx,
        default_pen_width
    )
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
    crate::port_pending!(
        "EDA_TEXT::GetEffectiveTextShape",
        host,
        ctx,
        triangulate,
        bbox,
        angle
    )
}

/// `EDA_TEXT::GetLinePositions( aSettings, aPositions, aLineCount )`.
pub fn line_positions<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_count: i32,
) -> Vec<Vector2I> {
    crate::port_pending!("EDA_TEXT::GetLinePositions", host, ctx, line_count)
}

/// `EDA_TEXT::GetInterline( aSettings )`.
pub fn interline<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    crate::port_pending!("EDA_TEXT::GetInterline", host, ctx)
}

/// `EDA_TEXT::Compare( aOther )`: negative, 0 or positive.
pub fn compare_text<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    other: &dyn EdaTextItem,
) -> i32 {
    crate::port_pending!("EDA_TEXT::Compare", host, ctx, other)
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

/// `EDA_TEXT::GetTextStyleName()`.
pub fn text_style_name<H: EdaTextItem + ?Sized>(host: &H) -> String {
    crate::port_pending!("EDA_TEXT::GetTextStyleName", host)
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
}
