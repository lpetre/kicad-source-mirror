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

//! Port of `include/font/font.h`, `common/font/font.cpp` and `include/font/kicad_font_name.h`:
//! the font-independent text layout engine (`KIFONT::FONT`) and the font lookup
//! (`FONT::GetFont`).
//!
//! The C++ draws through a `KIGFX::GAL*`; here the sink is a [`TextGal`] (see
//! `callback_gal.rs`).  The markup parse cache (`MARKUP_CACHE`, pure memoisation) is not
//! ported: every call parses its text.

use kicad_core::string_utils::wx_string_split;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, Vector2D, Vector2I, ki_round};

use crate::callback_gal::TextGal;
use crate::font_metrics::Metrics;
use crate::glyph::{Glyph, StrokeGlyph};
use crate::markup_parser::{MarkupNode, MarkupParser};
use crate::stroke_font::StrokeFont;

/// Port of `KICAD_FONT_NAME`: the "official" name of the built-in KiCad stroke font (always
/// existing).  This is also a keyword, that cannot be translated.
pub const KICAD_FONT_NAME: &str = "KiCad Font";

/// Port of `enum TEXT_STYLE`: the bits of [`TextStyleFlags`].
pub mod text_style {
    /// `TEXT_STYLE::BOLD`
    pub const BOLD: u32 = 1;
    /// `TEXT_STYLE::ITALIC`
    pub const ITALIC: u32 = 1 << 1;
    /// `TEXT_STYLE::SUBSCRIPT`
    pub const SUBSCRIPT: u32 = 1 << 2;
    /// `TEXT_STYLE::SUPERSCRIPT`
    pub const SUPERSCRIPT: u32 = 1 << 3;
    /// `TEXT_STYLE::OVERBAR`
    pub const OVERBAR: u32 = 1 << 4;
    /// `TEXT_STYLE::UNDERLINE`
    pub const UNDERLINE: u32 = 1 << 5;
}

/// Port of `TEXT_STYLE_FLAGS`.
pub type TextStyleFlags = u32;

/// Port of `ITALIC_TILT`: tilt factor for italic style (this is the scaling factor on dY
/// relative coordinates to give a tilted shape).
pub const ITALIC_TILT: f64 = 1.0 / 8.0;

/// Port of `IsBold( TEXT_STYLE_FLAGS )`.
pub fn is_bold(a_flags: TextStyleFlags) -> bool {
    a_flags & text_style::BOLD != 0
}

/// Port of `IsItalic( TEXT_STYLE_FLAGS )`.
pub fn is_italic(a_flags: TextStyleFlags) -> bool {
    a_flags & text_style::ITALIC != 0
}

/// Port of `IsSuperscript( TEXT_STYLE_FLAGS )`.
pub fn is_superscript(a_flags: TextStyleFlags) -> bool {
    a_flags & text_style::SUPERSCRIPT != 0
}

/// Port of `IsSubscript( TEXT_STYLE_FLAGS )`.
pub fn is_subscript(a_flags: TextStyleFlags) -> bool {
    a_flags & text_style::SUBSCRIPT != 0
}

/// `TO_UTF8( aText )` used as a C string: the text up to the first NUL character (the C++
/// builds the markup parser input with `std::string( const char* )`).
fn to_utf8_c_str(a_text: &str) -> &str {
    match a_text.find('\0') {
        Some(pos) => &a_text[..pos],
        None => a_text,
    }
}

/// Port of `FONT::linesCount`: the number of lines, ignoring a trailing newline.
pub fn lines_count(a_text: &str) -> u32 {
    if a_text.is_empty() {
        return 0; // std::count does not work well with empty strings
    }

    // aText.end() - 1 is to skip a newline character that is potentially at the end
    let mut chars: Vec<char> = a_text.chars().collect();
    chars.pop();
    chars.iter().filter(|&&c| c == '\n').count() as u32 + 1
}

/// Port of `KIFONT::FONT`: the font interface.
///
/// The required methods are the C++ virtual functions; the provided methods are the
/// non-virtual `FONT` members (the layout engine) and must not be overridden.
pub trait Font {
    /// Port of `FONT::IsStroke()`.
    fn is_stroke(&self) -> bool {
        false
    }

    /// Port of `FONT::IsOutline()`.
    fn is_outline(&self) -> bool {
        false
    }

    /// Port of `FONT::IsBold()`.
    fn is_bold(&self) -> bool {
        false
    }

    /// Port of `FONT::IsItalic()`.
    fn is_italic(&self) -> bool {
        false
    }

    /// Port of `FONT::GetName()` (`m_fontName`).
    fn name(&self) -> &str;

    /// `m_fontFileName` (empty for the stroke font).
    fn file_name(&self) -> &str {
        ""
    }

    /// Port of `FONT::GetInterline`: the distance between two lines of text for a glyph height.
    fn interline(&self, a_glyph_height: f64, a_font_metrics: &Metrics) -> f64;

    /// Port of `FONT::GetSuperSubSizeMultiplier`.
    fn super_sub_size_multiplier(&self) -> f64;

    /// Port of `FONT::GetSuperSubBaselineOffset`.
    fn super_sub_baseline_offset(&self, a_text_style: TextStyleFlags) -> f64;

    /// Port of `FONT::GetTextAsGlyphs`: convert one run of text (no markup) to glyphs.
    ///
    /// `a_bbox` receives the run's bounding box, `a_glyphs` (if given) is appended to; the
    /// return value is the position after the run.
    fn text_as_glyphs(
        &self,
        a_bbox: Option<&mut Box2I>,
        a_glyphs: Option<&mut Vec<Glyph>>,
        a_text: &str,
        a_size: Vector2I,
        a_position: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_text_style: TextStyleFlags,
    ) -> Vector2I;

    /// Port of `FONT::NameAsToken()`.
    fn name_as_token(&self) -> &str {
        self.name()
    }
}

/// The non-virtual `KIFONT::FONT` members, usable on any [`Font`] including `dyn Font`.
pub trait FontExt: Font {
    /// Port of `FONT::Draw( aGal, aText, aPosition, aAttributes, aFontMetrics )` (cursor
    /// `(0, 0)`, no mouse position): draw a (possibly multi-line, marked-up) string.  The
    /// glyphs are rotated and mirrored about `a_position`.
    fn draw(
        &self,
        a_gal: &mut dyn TextGal,
        a_text: &str,
        a_position: Vector2I,
        a_attrs: &TextAttributes,
        a_font_metrics: &Metrics,
    ) {
        self.draw_with_cursor(
            a_gal,
            a_text,
            a_position,
            Vector2I::new(0, 0),
            a_attrs,
            a_font_metrics,
            None,
            None,
        );
    }

    /// Port of `FONT::Draw( aGal, aText, aPosition, aCursor, aAttributes, aFontMetrics,
    /// aMousePos, aActiveUrl )`: `a_cursor` is subtracted from the line positions.
    fn draw_with_cursor(
        &self,
        a_gal: &mut dyn TextGal,
        a_text: &str,
        a_position: Vector2I,
        a_cursor: Vector2I,
        a_attrs: &TextAttributes,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    );

    /// Port of `FONT::StringBoundaryLimits`: the size of the bounding box of a single line of
    /// (marked-up) text.  For stroke fonts the box is inflated by `1.5 * a_thickness`.
    fn string_boundary_limits(
        &self,
        a_text: &str,
        a_size: Vector2I,
        a_thickness: i32,
        a_bold: bool,
        a_italic: bool,
        a_font_metrics: &Metrics,
    ) -> Vector2I;

    /// Port of `FONT::GetAlignedDrawPosition`: the position of the first line.
    fn aligned_draw_position(
        &self,
        a_text: &str,
        a_anchor: Vector2I,
        a_attributes: &TextAttributes,
        a_font_metrics: &Metrics,
    ) -> Vector2I;

    /// Port of `FONT::LinebreakText`: insert `\n` so that each line fits in `a_column_width`.
    fn linebreak_text(
        &self,
        a_text: &mut String,
        a_column_width: i32,
        a_size: Vector2I,
        a_thickness: i32,
        a_bold: bool,
        a_italic: bool,
    );

    /// Port of `FONT::getLinePositions`: split `a_text` on `\n` and compute the position of
    /// each line from the alignment.  Returns `( lines, positions, extents )`.
    fn line_positions(
        &self,
        a_text: &str,
        a_position: Vector2I,
        a_attrs: &TextAttributes,
        a_font_metrics: &Metrics,
    ) -> (Vec<String>, Vec<Vector2I>, Vec<Vector2I>);

    /// Port of `FONT::drawMarkup`: lay out one line of marked-up text; returns the position
    /// after it.
    fn draw_markup(
        &self,
        a_bounding_box: Option<&mut Box2I>,
        a_glyphs: Option<&mut Vec<Glyph>>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_text_style: TextStyleFlags,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    ) -> Vector2I;

    /// Port of `FONT::drawSingleLineText`.
    fn draw_single_line_text(
        &self,
        a_gal: &mut dyn TextGal,
        a_bounding_box: Option<&mut Box2I>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_italic: bool,
        a_underline: bool,
        a_hover: bool,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    );

    /// Port of `FONT::boundingBoxSingleLine`: returns the position after the line.
    fn bounding_box_single_line(
        &self,
        a_bbox: Option<&mut Box2I>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_italic: bool,
        a_font_metrics: &Metrics,
    ) -> Vector2I;

    /// Port of `FONT::wordbreakMarkup`: break a line into `( word, width )` pairs.
    fn wordbreak_markup(
        &self,
        a_words: &mut Vec<(String, i32)>,
        a_text: &str,
        a_size: Vector2I,
        a_text_style: TextStyleFlags,
    );
}

/// Port of `FONT::Draw`.
#[allow(clippy::too_many_arguments)]
fn draw_impl<F: Font + ?Sized>(
    font: &F,
    a_gal: &mut dyn TextGal,
    a_text: &str,
    a_position: Vector2I,
    a_cursor: Vector2I,
    a_attrs: &TextAttributes,
    a_font_metrics: &Metrics,
    a_mouse_pos: Option<Vector2I>,
    mut a_active_url: Option<&mut String>,
) {
    if a_text.is_empty() {
        return;
    }

    let position: Vector2I = a_position - a_cursor;

    // Split multiline strings into separate ones and draw them line by line
    let (strings_list, positions, _extents) =
        line_positions_impl(font, a_text, position, a_attrs, a_font_metrics);

    a_gal.set_line_width(a_attrs.stroke_width as f32);

    let size = Vector2I::new(a_attrs.size.0, a_attrs.size.1);
    let angle = EdaAngle::from_degrees(a_attrs.angle);

    for i in 0..strings_list.len() {
        draw_single_line_text_impl(
            font,
            a_gal,
            None,
            &strings_list[i],
            positions[i],
            size,
            angle,
            a_attrs.mirrored,
            a_position,
            a_attrs.italic,
            a_attrs.underlined,
            a_attrs.hover,
            a_font_metrics,
            a_mouse_pos,
            a_active_url.as_deref_mut(),
        );
    }
}

/// Port of `FONT::getLinePositions`.
fn line_positions_impl<F: Font + ?Sized>(
    font: &F,
    a_text: &str,
    a_position: Vector2I,
    a_attrs: &TextAttributes,
    a_font_metrics: &Metrics,
) -> (Vec<String>, Vec<Vector2I>, Vec<Vector2I>) {
    let text_lines = wx_string_split(a_text, '\n');
    let line_count = text_lines.len() as i32;
    let mut positions: Vec<Vector2I> = Vec::with_capacity(text_lines.len());
    let mut extents: Vec<Vector2I> = Vec::with_capacity(text_lines.len());
    let size = Vector2I::new(a_attrs.size.0, a_attrs.size.1);

    // int = double: truncation
    let interline: i32 =
        (font.interline(size.y as f64, a_font_metrics) * a_attrs.line_spacing) as i32;
    let mut height: i32 = 0;

    for i in 0..line_count {
        let pos = Vector2I::new(
            a_position.x,
            a_position.y.wrapping_add(i.wrapping_mul(interline)),
        );
        let end: Vector2I = bounding_box_single_line_impl(
            font,
            None,
            &text_lines[i as usize],
            pos,
            size,
            a_attrs.italic,
            a_font_metrics,
        );
        let b_box: Vector2I = end - pos;

        extents.push(b_box);

        if i == 0 {
            // 1.17 is a fudge to match 6.0 positioning
            height = (height as f64 + size.y as f64 * 1.17) as i32;
        } else {
            height = height.wrapping_add(interline);
        }
    }

    let mut offset = Vector2I::new(0, 0);
    offset.y = offset.y.wrapping_add(size.y);

    if font.is_stroke() {
        // Fudge factors to match 6.0 positioning
        offset.x = (offset.x as f64 + a_attrs.stroke_width as f64 / 1.52) as i32;
        offset.y = (offset.y as f64 - a_attrs.stroke_width as f64 * 0.052) as i32;
    }

    match a_attrs.valign {
        GrTextVAlignT::Top => {}
        GrTextVAlignT::Center => offset.y = offset.y.wrapping_sub(height / 2),
        GrTextVAlignT::Bottom => offset.y = offset.y.wrapping_sub(height),
        // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
        GrTextVAlignT::Indeterminate => {}
    }

    for i in 0..line_count {
        let line_size: Vector2I = extents[i as usize];
        let mut line_offset: Vector2I = offset;

        line_offset.y = line_offset.y.wrapping_add(i.wrapping_mul(interline));

        match a_attrs.halign {
            GrTextHAlignT::Left => {}
            GrTextHAlignT::Center => line_offset.x = line_size.x.wrapping_neg() / 2,
            GrTextHAlignT::Right => {
                line_offset.x = line_size.x.wrapping_add(offset.x).wrapping_neg();
            }
            // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
            GrTextHAlignT::Indeterminate => {}
        }

        positions.push(a_position + line_offset);
    }

    (text_lines, positions, extents)
}

/// Port of the file-local `::drawMarkup( aBoundingBox, aGlyphs, aNode, ... )`: lay out one
/// markup node and its children; returns the position of the cursor for drawing the next
/// substring.
#[allow(clippy::too_many_arguments)]
pub fn draw_markup_node<F: Font + ?Sized>(
    mut a_bounding_box: Option<&mut Box2I>,
    mut a_glyphs: Option<&mut Vec<Glyph>>,
    a_node: Option<&MarkupNode>,
    a_position: Vector2I,
    a_font: &F,
    a_size: Vector2I,
    a_angle: EdaAngle,
    a_mirror: bool,
    a_origin: Vector2I,
    a_text_style: TextStyleFlags,
    a_font_metrics: &Metrics,
    a_mouse_pos: Option<Vector2I>,
    mut a_active_url: Option<&mut String>,
) -> Vector2I {
    let mut next_position: Vector2I = a_position;
    let mut draw_underline = false;
    let mut draw_overbar = false;
    let mut use_hover_color = false;
    let start: usize = a_glyphs.as_ref().map_or(0, |g| g.len());

    if let Some(node) = a_node {
        let mut text_style: TextStyleFlags = a_text_style;

        if !node.is_root() {
            if node.is_subscript() {
                text_style |= text_style::SUBSCRIPT;
            } else if node.is_superscript() {
                text_style |= text_style::SUPERSCRIPT;
            }

            if node.is_overbar() {
                draw_overbar = true;
            }

            if a_text_style & text_style::UNDERLINE != 0 {
                draw_underline = true;
            }

            if let Some(content) = node.content() {
                let mut bbox = Box2I::default();

                next_position = a_font.text_as_glyphs(
                    Some(&mut bbox),
                    a_glyphs.as_deref_mut(),
                    content,
                    a_size,
                    next_position,
                    a_angle,
                    a_mirror,
                    a_origin,
                    text_style,
                );

                if let Some(bb) = a_bounding_box.as_deref_mut() {
                    bb.merge(&bbox);
                }

                if node.is_url() {
                    if let Some(mouse) = a_mouse_pos {
                        if bbox.contains(mouse) {
                            use_hover_color = true;
                            draw_underline = true;
                        }
                    }
                }
            }
        }

        for child in &node.children {
            next_position = draw_markup_node(
                a_bounding_box.as_deref_mut(),
                a_glyphs.as_deref_mut(),
                Some(child),
                next_position,
                a_font,
                a_size,
                a_angle,
                a_mirror,
                a_origin,
                text_style,
                a_font_metrics,
                a_mouse_pos,
                a_active_url.as_deref_mut(),
            );
        }
    }

    if draw_underline {
        // Shorten the bar a little so its rounded ends don't make it over-long
        let bar_trim: f64 = a_size.x as f64 * 0.1;
        let bar_offset: f64 = a_font_metrics.underline_vertical_position(a_size.y as f64);

        push_bar(
            a_glyphs.as_deref_mut(),
            a_position,
            next_position,
            bar_trim,
            bar_offset,
            a_angle,
            a_mirror,
            a_origin,
        );
    }

    if draw_overbar {
        // Shorten the bar a little so its rounded ends don't make it over-long
        let bar_trim: f64 = a_size.x as f64 * 0.1;
        let bar_offset: f64 = a_font_metrics.overbar_vertical_position(a_size.y as f64);

        push_bar(
            a_glyphs.as_deref_mut(),
            a_position,
            next_position,
            bar_trim,
            bar_offset,
            a_angle,
            a_mirror,
            a_origin,
        );
    }

    if use_hover_color {
        if let Some(glyphs) = a_glyphs {
            for glyph in glyphs.iter_mut().skip(start) {
                glyph.set_is_hover(true);
            }
        }

        if let (Some(url), Some(node)) = (a_active_url, a_node) {
            if url.is_empty() {
                *url = node.content().unwrap_or("").to_string();
            }
        }
    }

    next_position
}

/// The underline / overbar glyph of `::drawMarkup`.
#[allow(clippy::too_many_arguments)]
fn push_bar(
    a_glyphs: Option<&mut Vec<Glyph>>,
    a_position: Vector2I,
    a_next_position: Vector2I,
    bar_trim: f64,
    bar_offset: f64,
    a_angle: EdaAngle,
    a_mirror: bool,
    a_origin: Vector2I,
) {
    let bar_start = Vector2D::new(
        a_position.x as f64 + bar_trim,
        a_position.y as f64 - bar_offset,
    );
    let bar_end = Vector2D::new(
        a_next_position.x as f64 - bar_trim,
        a_next_position.y as f64 - bar_offset,
    );

    if let Some(glyphs) = a_glyphs {
        let mut bar_glyph = StrokeGlyph::new();

        bar_glyph.add_point(bar_start);
        bar_glyph.add_point(bar_end);
        bar_glyph.finalize();

        // Transform( { 1.0, 1.0 }, { 0, 0 }, false, ... ): the tilt is `false`, i.e. 0.0
        glyphs.push(Glyph::Stroke(bar_glyph.transform(
            Vector2D::new(1.0, 1.0),
            Vector2I::new(0, 0),
            0.0,
            a_angle,
            a_mirror,
            a_origin,
        )));
    }
}

/// Port of `FONT::drawMarkup` (the member): parse `a_text` and lay it out.
#[allow(clippy::too_many_arguments)]
fn draw_markup_impl<F: Font + ?Sized>(
    font: &F,
    a_bounding_box: Option<&mut Box2I>,
    a_glyphs: Option<&mut Vec<Glyph>>,
    a_text: &str,
    a_position: Vector2I,
    a_size: Vector2I,
    a_angle: EdaAngle,
    a_mirror: bool,
    a_origin: Vector2I,
    a_text_style: TextStyleFlags,
    a_font_metrics: &Metrics,
    a_mouse_pos: Option<Vector2I>,
    a_active_url: Option<&mut String>,
) -> Vector2I {
    let markup_parser = MarkupParser::new(to_utf8_c_str(a_text));
    let root = markup_parser.parse();

    draw_markup_node(
        a_bounding_box,
        a_glyphs,
        root.as_ref(),
        a_position,
        font,
        a_size,
        a_angle,
        a_mirror,
        a_origin,
        a_text_style,
        a_font_metrics,
        a_mouse_pos,
        a_active_url,
    )
}

/// Port of `FONT::drawSingleLineText`.
#[allow(clippy::too_many_arguments)]
fn draw_single_line_text_impl<F: Font + ?Sized>(
    font: &F,
    a_gal: &mut dyn TextGal,
    a_bounding_box: Option<&mut Box2I>,
    a_text: &str,
    a_position: Vector2I,
    a_size: Vector2I,
    a_angle: EdaAngle,
    a_mirror: bool,
    a_origin: Vector2I,
    a_italic: bool,
    a_underline: bool,
    a_hover: bool,
    a_font_metrics: &Metrics,
    a_mouse_pos: Option<Vector2I>,
    a_active_url: Option<&mut String>,
) {
    let mut text_style: TextStyleFlags = 0;

    if a_italic {
        text_style |= text_style::ITALIC;
    }

    if a_underline {
        text_style |= text_style::UNDERLINE;
    }

    let mut glyphs: Vec<Glyph> = Vec::new();

    let _ = draw_markup_impl(
        font,
        a_bounding_box,
        Some(&mut glyphs),
        a_text,
        a_position,
        a_size,
        a_angle,
        a_mirror,
        a_origin,
        text_style,
        a_font_metrics,
        a_mouse_pos,
        a_active_url,
    );

    if a_hover {
        for glyph in glyphs.iter_mut() {
            glyph.set_is_hover(true);
        }
    }

    a_gal.draw_glyphs(&glyphs);
}

/// Port of `FONT::boundingBoxSingleLine`.
fn bounding_box_single_line_impl<F: Font + ?Sized>(
    font: &F,
    a_bbox: Option<&mut Box2I>,
    a_text: &str,
    a_position: Vector2I,
    a_size: Vector2I,
    a_italic: bool,
    a_font_metrics: &Metrics,
) -> Vector2I {
    let mut text_style: TextStyleFlags = 0;

    if a_italic {
        text_style |= text_style::ITALIC;
    }

    draw_markup_impl(
        font,
        a_bbox,
        None,
        a_text,
        a_position,
        a_size,
        ANGLE_0,
        false,
        Vector2I::new(0, 0),
        text_style,
        a_font_metrics,
        None,
        None,
    )
}

/// `wxString::Trim()` (from the right): remove trailing white space (`wxSafeIsspace`).
fn wx_trim_right(s: &str) -> &str {
    s.trim_end_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r'])
}

/// `wxStringTokenizer( aText, " ", wxTOKEN_RET_DELIMS )` (wxWidgets 3.2): the tokens, each
/// keeping its trailing delimiter.  Tokenizing stops when only delimiters are left (except
/// for the very first token of a non-empty string), so trailing runs of spaces after the
/// first one are dropped: `"a  "` gives `"a "`, `"  "` gives `" "`, `"  a"` gives
/// `" "`, `" "`, `"a"`.
fn wx_tokenize_ret_delims(a_text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut pos = 0;

    loop {
        // DoHasMoreTokens()
        let rest = &a_text[pos..];
        let has_more = if rest.chars().any(|c| c != ' ') {
            true
        } else {
            // special hack for wxTOKEN_RET_EMPTY: we should return the initial empty token
            // even if there are only delimiters after it
            !a_text.is_empty() && pos == 0
        };

        if !has_more {
            break;
        }

        // GetNextToken(): in wxTOKEN_RET_DELIMS mode the delimiter is part of the token
        match rest.find(' ') {
            None => {
                tokens.push(rest.to_string());
                pos = a_text.len();
            }
            Some(d) => {
                tokens.push(rest[..d + 1].to_string());
                pos += d + 1;
            }
        }
    }

    tokens
}

/// Port of the file-local `::wordbreakMarkup`: break marked-up text into "words".
///
/// In this context, a "word" is EITHER a run of marked-up text (subscript, superscript or
/// overbar), OR a run of non-marked-up text separated by spaces.
pub fn wordbreak_markup_node<F: Font + ?Sized>(
    a_words: &mut Vec<(String, i32)>,
    a_node: &MarkupNode,
    a_font: &F,
    a_size: Vector2I,
    a_text_style: TextStyleFlags,
    a_inside_markup: bool,
) {
    let mut text_style: TextStyleFlags = a_text_style;

    let width_of = |text: &str, style: TextStyleFlags| -> i32 {
        a_font
            .text_as_glyphs(
                None,
                None,
                text,
                a_size,
                Vector2I::new(0, 0),
                ANGLE_0,
                false,
                Vector2I::new(0, 0),
                style,
            )
            .x
    };

    if !a_node.is_root() {
        let mut escape_char: Option<char> = None;

        if a_node.is_subscript() {
            escape_char = Some('_');
            text_style = text_style::SUBSCRIPT;
        } else if a_node.is_superscript() {
            escape_char = Some('^');
            text_style = text_style::SUPERSCRIPT;
        }

        if a_node.is_overbar() {
            escape_char = Some('~');
            text_style |= text_style::OVERBAR;
        }

        if let Some(escape) = escape_char {
            let mut word = format!("{escape}{{");
            let mut width: i32 = 0;

            if let Some(content) = a_node.content() {
                let next_x = width_of(content, text_style);
                word.push_str(content);
                width = width.wrapping_add(next_x);
            }

            let mut child_words: Vec<(String, i32)> = Vec::new();

            for child in &a_node.children {
                wordbreak_markup_node(&mut child_words, child, a_font, a_size, text_style, true);
            }

            for (child_word, child_width) in &child_words {
                word.push_str(child_word);
                width = width.wrapping_add(*child_width);
            }

            word.push('}');
            a_words.push((word, width));
            return;
        } else if a_inside_markup {
            // Inside a markup node, preserve the content verbatim (no tokenization).
            // wxStringTokenizer collapses consecutive spaces, which corrupts content
            // like ~{     } where spaces are intentional.
            let content = a_node.content().unwrap_or("");
            let w = width_of(content, text_style);

            a_words.push((content.to_string(), w));
        } else {
            let text_run = a_node.content().unwrap_or("");

            for word in wx_tokenize_ret_delims(text_run) {
                let chars = wx_trim_right(&word);
                let w = width_of(if chars.is_empty() { &word } else { chars }, text_style);

                a_words.push((word, w));
            }
        }
    }

    for child in &a_node.children {
        wordbreak_markup_node(a_words, child, a_font, a_size, text_style, a_inside_markup);
    }
}

impl<F: Font + ?Sized> FontExt for F {
    fn draw_with_cursor(
        &self,
        a_gal: &mut dyn TextGal,
        a_text: &str,
        a_position: Vector2I,
        a_cursor: Vector2I,
        a_attrs: &TextAttributes,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    ) {
        draw_impl(
            self,
            a_gal,
            a_text,
            a_position,
            a_cursor,
            a_attrs,
            a_font_metrics,
            a_mouse_pos,
            a_active_url,
        );
    }

    fn string_boundary_limits(
        &self,
        a_text: &str,
        a_size: Vector2I,
        a_thickness: i32,
        a_bold: bool,
        a_italic: bool,
        a_font_metrics: &Metrics,
    ) -> Vector2I {
        let mut bounding_box = Box2I::default();
        let mut text_style: TextStyleFlags = 0;

        if a_bold {
            text_style |= text_style::BOLD;
        }

        if a_italic {
            text_style |= text_style::ITALIC;
        }

        let _ = draw_markup_impl(
            self,
            Some(&mut bounding_box),
            None,
            a_text,
            Vector2I::new(0, 0),
            a_size,
            ANGLE_0,
            false,
            Vector2I::new(0, 0),
            text_style,
            a_font_metrics,
            None,
            None,
        );

        if self.is_stroke() {
            // Inflate by a bit more than thickness/2 to catch diacriticals, descenders, etc.
            bounding_box.inflate(ki_round(a_thickness as f64 * 1.5));
        } else if self.is_outline() {
            // Outline fonts have thickness built in, and *usually* stay within their
            // ascent/descent
        }

        // BOX2I::GetSize() is a VECTOR2L, converted (clamped) to VECTOR2I
        bounding_box.size().cast()
    }

    fn aligned_draw_position(
        &self,
        a_text: &str,
        a_anchor: Vector2I,
        a_attributes: &TextAttributes,
        a_font_metrics: &Metrics,
    ) -> Vector2I {
        let (_, positions, _) =
            line_positions_impl(self, a_text, a_anchor, a_attributes, a_font_metrics);

        positions.first().copied().unwrap_or(a_anchor)
    }

    fn linebreak_text(
        &self,
        a_text: &mut String,
        a_column_width: i32,
        a_size: Vector2I,
        a_thickness: i32,
        a_bold: bool,
        a_italic: bool,
    ) {
        let mut text_style: TextStyleFlags = 0;

        if a_bold {
            text_style |= text_style::BOLD;
        }

        if a_italic {
            text_style |= text_style::ITALIC;
        }

        let space_width: i32 = self
            .text_as_glyphs(
                None,
                None,
                " ",
                a_size,
                Vector2I::new(0, 0),
                ANGLE_0,
                false,
                Vector2I::new(0, 0),
                text_style,
            )
            .x;

        let text_lines = wx_string_split(a_text, '\n');

        a_text.clear();

        for ii in 0..text_lines.len() {
            let mut markup: Vec<(String, i32)> = Vec::new();
            let mut words: Vec<(String, i32)> = Vec::new();

            self.wordbreak_markup(&mut markup, &text_lines[ii], a_size, text_style);

            for (run, run_width) in markup {
                match words.last_mut() {
                    Some(last) if !last.0.ends_with(' ') => {
                        last.0.push_str(&run);
                        last.1 = last.1.wrapping_add(run_width);
                    }
                    _ => words.push((run, run_width)),
                }
            }

            let mut bury_mode = false;
            let mut line_width: i32 = 0;
            let mut pending_spaces = String::new();

            for (word, word_width) in &words {
                let mut pending_space_width: i32 =
                    (pending_spaces.chars().count() as i32).wrapping_mul(space_width);
                let overflow = line_width
                    .wrapping_add(pending_space_width)
                    .wrapping_add(*word_width)
                    > a_column_width.wrapping_sub(a_thickness);

                if overflow && !pending_spaces.is_empty() {
                    a_text.push('\n');
                    line_width = 0;
                    pending_spaces.clear();
                    pending_space_width = 0;
                    bury_mode = true;
                }

                if word == " " {
                    pending_spaces.push_str(word);
                } else {
                    if bury_mode {
                        bury_mode = false;
                    } else {
                        a_text.push_str(&pending_spaces);
                        line_width = line_width.wrapping_add(pending_space_width);
                    }

                    if word.ends_with(' ') {
                        // word.Left( word.Length() - 1 )
                        a_text.push_str(&word[..word.len() - 1]);
                        pending_spaces = " ".to_string();
                    } else {
                        a_text.push_str(word);
                        pending_spaces.clear();
                    }

                    line_width = line_width.wrapping_add(*word_width);
                }
            }

            // Add the newlines back onto the string
            if ii != text_lines.len() - 1 {
                a_text.push('\n');
            }
        }
    }

    fn line_positions(
        &self,
        a_text: &str,
        a_position: Vector2I,
        a_attrs: &TextAttributes,
        a_font_metrics: &Metrics,
    ) -> (Vec<String>, Vec<Vector2I>, Vec<Vector2I>) {
        line_positions_impl(self, a_text, a_position, a_attrs, a_font_metrics)
    }

    fn draw_markup(
        &self,
        a_bounding_box: Option<&mut Box2I>,
        a_glyphs: Option<&mut Vec<Glyph>>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_text_style: TextStyleFlags,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    ) -> Vector2I {
        draw_markup_impl(
            self,
            a_bounding_box,
            a_glyphs,
            a_text,
            a_position,
            a_size,
            a_angle,
            a_mirror,
            a_origin,
            a_text_style,
            a_font_metrics,
            a_mouse_pos,
            a_active_url,
        )
    }

    fn draw_single_line_text(
        &self,
        a_gal: &mut dyn TextGal,
        a_bounding_box: Option<&mut Box2I>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_italic: bool,
        a_underline: bool,
        a_hover: bool,
        a_font_metrics: &Metrics,
        a_mouse_pos: Option<Vector2I>,
        a_active_url: Option<&mut String>,
    ) {
        draw_single_line_text_impl(
            self,
            a_gal,
            a_bounding_box,
            a_text,
            a_position,
            a_size,
            a_angle,
            a_mirror,
            a_origin,
            a_italic,
            a_underline,
            a_hover,
            a_font_metrics,
            a_mouse_pos,
            a_active_url,
        );
    }

    fn bounding_box_single_line(
        &self,
        a_bbox: Option<&mut Box2I>,
        a_text: &str,
        a_position: Vector2I,
        a_size: Vector2I,
        a_italic: bool,
        a_font_metrics: &Metrics,
    ) -> Vector2I {
        bounding_box_single_line_impl(
            self,
            a_bbox,
            a_text,
            a_position,
            a_size,
            a_italic,
            a_font_metrics,
        )
    }

    fn wordbreak_markup(
        &self,
        a_words: &mut Vec<(String, i32)>,
        a_text: &str,
        a_size: Vector2I,
        a_text_style: TextStyleFlags,
    ) {
        let markup_parser = MarkupParser::new(to_utf8_c_str(a_text));

        if let Some(root) = markup_parser.parse() {
            wordbreak_markup_node(a_words, &root, self, a_size, a_text_style, false);
        }
    }
}

/// Port of the static `FONT::IsStroke( const wxString& aFontName )`: whether a face name
/// designates the stroke font (`_( "Default Font" )`, untranslated here, or `"KiCad Font"`).
pub fn is_stroke_font_name(a_font_name: &str) -> bool {
    // This would need a more complex implementation if we ever support more stroke fonts
    // than the KiCad Font.
    a_font_name == "Default Font" || a_font_name == KICAD_FONT_NAME
}

/// Port of `FONT::getDefaultFont()`: the stroke font.
pub fn default_font() -> &'static StrokeFont {
    static DEFAULT: std::sync::OnceLock<StrokeFont> = std::sync::OnceLock::new();

    DEFAULT.get_or_init(|| StrokeFont::load_font("").expect("the empty name loads the stroke font"))
}

/// Result of [`get_font`] (`FONT::GetFont`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontLookup {
    /// The built-in stroke font was requested (empty name or a name starting with
    /// `"KiCad Font"`).
    Stroke,
    /// A named outline (TrueType/OpenType) font was requested.  This port has no outline font
    /// engine (FreeType/HarfBuzz/fontconfig), so the font is not available; callers that
    /// need geometry get the stroke font as fallback (what C++ `FONT::GetFont` returns when
    /// `OUTLINE_FONT::LoadFont` fails), and must report the substitution.  Outline text with
    /// a valid render cache does not need the font.
    OutlineUnavailable {
        /// The requested face name (`m_fontName` keeps the asked-for name in C++).
        name: String,
        /// The requested bold flag.
        bold: bool,
        /// The requested italic flag.
        italic: bool,
        /// `aForDrawingSheet`
        for_drawing_sheet: bool,
    },
}

impl FontLookup {
    /// The font to lay out text with: the stroke font in both cases (fallback for an
    /// unavailable outline font).
    pub fn font(&self) -> &'static StrokeFont {
        default_font()
    }

    /// Whether the requested font is an outline font that is not available.
    pub fn is_outline_unavailable(&self) -> bool {
        matches!(self, FontLookup::OutlineUnavailable { .. })
    }

    /// Port of `FONT::GetName()` of the font C++ would return: the stroke font name, or the
    /// asked-for outline name.
    pub fn name(&self) -> &str {
        match self {
            FontLookup::Stroke => KICAD_FONT_NAME,
            FontLookup::OutlineUnavailable { name, .. } => name,
        }
    }
}

/// Port of `FONT::GetFont( aFontName, aBold, aItalic, aEmbeddedFiles, aForDrawingSheet )`.
///
/// `a_embedded_files` (font files embedded in the board) are accepted for signature fidelity;
/// they would only matter to the outline font engine, which is not ported.
pub fn get_font(
    a_font_name: &str,
    a_bold: bool,
    a_italic: bool,
    a_embedded_files: Option<&[String]>,
    a_for_drawing_sheet: bool,
) -> FontLookup {
    let _ = a_embedded_files;

    if a_font_name.is_empty() || a_font_name.starts_with(KICAD_FONT_NAME) {
        return FontLookup::Stroke;
    }

    FontLookup::OutlineUnavailable {
        name: a_font_name.to_string(),
        bold: a_bold,
        italic: a_italic,
        for_drawing_sheet: a_for_drawing_sheet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_font_lookup() {
        assert_eq!(get_font("", false, false, None, false), FontLookup::Stroke);
        assert_eq!(
            get_font("KiCad Font", true, true, None, false),
            FontLookup::Stroke
        );
        assert_eq!(
            get_font("KiCad Font Bold", false, false, None, false),
            FontLookup::Stroke
        );

        let lookup = get_font("Ubuntu Sans", true, false, None, false);
        assert!(lookup.is_outline_unavailable());
        assert_eq!(lookup.name(), "Ubuntu Sans");
        assert!(lookup.font().is_stroke());

        assert!(is_stroke_font_name("KiCad Font"));
        assert!(is_stroke_font_name("Default Font"));
        assert!(!is_stroke_font_name("KiCad Font Bold"));
    }

    #[test]
    fn lines_count_ignores_trailing_newline() {
        assert_eq!(lines_count(""), 0);
        assert_eq!(lines_count("a"), 1);
        assert_eq!(lines_count("a\n"), 1);
        assert_eq!(lines_count("a\nb"), 2);
        assert_eq!(lines_count("\n\n"), 2);
    }

    #[test]
    fn tokenizer_matches_wx() {
        assert_eq!(wx_tokenize_ret_delims(""), Vec::<String>::new());
        assert_eq!(wx_tokenize_ret_delims(" "), vec![" "]);
        assert_eq!(wx_tokenize_ret_delims("  "), vec![" "]);
        assert_eq!(wx_tokenize_ret_delims("a  "), vec!["a "]);
        assert_eq!(wx_tokenize_ret_delims("  a"), vec![" ", " ", "a"]);
        assert_eq!(wx_tokenize_ret_delims("a b"), vec!["a ", "b"]);
    }
}
